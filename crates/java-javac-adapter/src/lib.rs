#![doc = "Adapters externos para javac, java e execução de testes Java."]

use std::{fs, sync::Arc, time::Duration};

use async_trait::async_trait;
use ide_domain::LanguageId;
use ide_process::{ProcessRequest, ProcessSupervisor};
use ide_toolchain_api::{
    Classpath, CompilationRequest, CompilationResult, CompilerAdapter, ExecutionRequest,
    ExecutionResult, RuntimeAdapter, TestAdapter, TestCaseResult, TestRequest, TestResult,
    ToolchainError,
};
use java_toolchain::jdk_executable;

pub struct JavaJavacAdapter {
    processes: Arc<dyn ProcessSupervisor>,
    timeout: Duration,
}

impl JavaJavacAdapter {
    #[must_use]
    pub fn new(processes: Arc<dyn ProcessSupervisor>) -> Self {
        Self {
            processes,
            timeout: Duration::from_secs(120),
        }
    }

    #[must_use]
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }
}

#[async_trait]
impl CompilerAdapter for JavaJavacAdapter {
    fn supported_language(&self) -> LanguageId {
        LanguageId("java".to_owned())
    }

    async fn compile(
        &self,
        request: CompilationRequest,
    ) -> Result<CompilationResult, ToolchainError> {
        if request.source_files.is_empty() {
            return Err(ToolchainError::Operation(
                "no Java source files were provided".to_owned(),
            ));
        }
        for source in &request.source_files {
            if !source.is_file() {
                return Err(ToolchainError::Operation(format!(
                    "source file does not exist: {}",
                    source.display()
                )));
            }
        }
        fs::create_dir_all(&request.output_directory)
            .map_err(|error| ToolchainError::Operation(error.to_string()))?;
        let mut args = vec![
            "-encoding".to_owned(),
            "UTF-8".to_owned(),
            "-d".to_owned(),
            request.output_directory.to_string_lossy().into_owned(),
        ];
        append_classpath(&mut args, &request.classpath)?;
        if let Some(source) = &request.source_level {
            args.extend(["-source".to_owned(), source.clone()]);
        }
        if let Some(target) = &request.target_level {
            args.extend(["-target".to_owned(), target.clone()]);
        }
        args.extend(request.additional_args);
        args.extend(
            request
                .source_files
                .iter()
                .map(|source| source.to_string_lossy().into_owned()),
        );
        let output = self
            .processes
            .execute(ProcessRequest {
                program: jdk_executable(&request.installation.home, "javac"),
                args,
                working_directory: Some(request.working_directory),
                timeout: Some(self.timeout),
                environment: vec![(
                    "JAVA_HOME".to_owned(),
                    request.installation.home.to_string_lossy().into_owned(),
                )],
            })
            .await
            .map_err(|error| ToolchainError::Operation(error.to_string()))?;
        Ok(CompilationResult {
            success: output.exit_code == 0,
            exit_code: output.exit_code,
            stdout: output.stdout,
            stderr: output.stderr,
            output_directory: request.output_directory,
        })
    }
}

#[async_trait]
impl RuntimeAdapter for JavaJavacAdapter {
    fn supported_language(&self) -> LanguageId {
        LanguageId("java".to_owned())
    }

    async fn run(&self, request: ExecutionRequest) -> Result<ExecutionResult, ToolchainError> {
        if request.main_class.trim().is_empty() {
            return Err(ToolchainError::Operation(
                "main class is required".to_owned(),
            ));
        }
        let mut args = request.jvm_args;
        append_classpath(&mut args, &request.classpath)?;
        args.push(request.main_class);
        args.extend(request.args);
        let output = self
            .processes
            .execute(ProcessRequest {
                program: jdk_executable(&request.installation.home, "java"),
                args,
                working_directory: Some(request.working_directory),
                timeout: Some(self.timeout),
                environment: vec![(
                    "JAVA_HOME".to_owned(),
                    request.installation.home.to_string_lossy().into_owned(),
                )],
            })
            .await
            .map_err(|error| ToolchainError::Operation(error.to_string()))?;
        Ok(ExecutionResult {
            success: output.exit_code == 0,
            exit_code: output.exit_code,
            stdout: output.stdout,
            stderr: output.stderr,
        })
    }
}

#[async_trait]
impl TestAdapter for JavaJavacAdapter {
    fn supported_language(&self) -> LanguageId {
        LanguageId("java".to_owned())
    }

    async fn run_tests(&self, request: TestRequest) -> Result<TestResult, ToolchainError> {
        let installation = request.compilation.installation.clone();
        let working_directory = request.compilation.working_directory.clone();
        let output_directory = request.compilation.output_directory.clone();
        let mut classpath = request.compilation.classpath.clone();
        if !classpath.entries.contains(&output_directory) {
            classpath.entries.insert(0, output_directory);
        }
        let compilation = self.compile(request.compilation).await?;
        let mut cases = Vec::new();
        if compilation.success {
            for class_name in request.test_classes {
                let execution = self
                    .run(ExecutionRequest {
                        installation: installation.clone(),
                        main_class: class_name.clone(),
                        classpath: classpath.clone(),
                        args: request.args.clone(),
                        jvm_args: Vec::new(),
                        working_directory: working_directory.clone(),
                    })
                    .await?;
                cases.push(TestCaseResult {
                    class_name,
                    execution,
                });
            }
        }
        Ok(TestResult { compilation, cases })
    }
}

fn append_classpath(args: &mut Vec<String>, classpath: &Classpath) -> Result<(), ToolchainError> {
    if !classpath.entries.is_empty() {
        args.push("-classpath".to_owned());
        args.push(classpath.argument()?);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use ide_domain::ProcessId;
    use ide_process::{ProcessError, ProcessOutput, ProcessStatus};
    use ide_toolchain_api::{ToolchainId, ToolchainInstallation};

    use super::*;

    #[derive(Default)]
    struct FakeProcesses {
        requests: Mutex<Vec<ProcessRequest>>,
    }

    #[async_trait]
    impl ProcessSupervisor for FakeProcesses {
        async fn spawn(&self, _request: ProcessRequest) -> Result<ProcessId, ProcessError> {
            Ok(ProcessId(1))
        }

        async fn terminate(&self, _process_id: ProcessId) -> Result<(), ProcessError> {
            Ok(())
        }

        async fn status(&self, _process_id: ProcessId) -> Result<ProcessStatus, ProcessError> {
            Ok(ProcessStatus::Exited(0))
        }

        async fn execute(&self, request: ProcessRequest) -> Result<ProcessOutput, ProcessError> {
            if let Ok(mut requests) = self.requests.lock() {
                requests.push(request);
            }
            Ok(ProcessOutput {
                exit_code: 0,
                stdout: "ok".to_owned(),
                stderr: String::new(),
            })
        }
    }

    fn installation(root: &std::path::Path) -> ToolchainInstallation {
        ToolchainInstallation {
            id: ToolchainId("test-jdk".to_owned()),
            home: root.to_path_buf(),
            version: Some("test".to_owned()),
        }
    }

    #[test]
    fn javac_and_java_receive_classpath_and_java_home() {
        let root =
            std::env::temp_dir().join(format!("er-ide-javac-adapter-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        assert!(fs::create_dir_all(root.join("bin")).is_ok());
        assert!(fs::write(jdk_executable(&root, "javac"), []).is_ok());
        assert!(fs::write(jdk_executable(&root, "java"), []).is_ok());
        let source = root.join("Main.java");
        assert!(fs::write(&source, "class Main {}").is_ok());
        let output = root.join("classes");
        let processes = Arc::new(FakeProcesses::default());
        let adapter = JavaJavacAdapter::new(processes.clone());
        let runtime = match tokio::runtime::Builder::new_current_thread().build() {
            Ok(runtime) => runtime,
            Err(error) => panic!("runtime failed: {error}"),
        };
        let compilation = runtime.block_on(adapter.compile(CompilationRequest {
            installation: installation(&root),
            source_files: vec![source],
            output_directory: output.clone(),
            classpath: Classpath {
                entries: vec![output.clone()],
            },
            source_level: Some("8".to_owned()),
            target_level: Some("8".to_owned()),
            additional_args: Vec::new(),
            working_directory: root.clone(),
        }));
        assert!(compilation.is_ok_and(|result| result.success));
        let execution = runtime.block_on(adapter.run(ExecutionRequest {
            installation: installation(&root),
            main_class: "Main".to_owned(),
            classpath: Classpath {
                entries: vec![output],
            },
            args: Vec::new(),
            jvm_args: Vec::new(),
            working_directory: root.clone(),
        }));
        assert!(execution.is_ok_and(|result| result.success));
        let requests = match processes.requests.lock() {
            Ok(requests) => requests,
            Err(error) => panic!("request lock failed: {error}"),
        };
        assert_eq!(requests.len(), 2);
        assert!(requests[0].args.contains(&"-classpath".to_owned()));
        assert!(requests.iter().all(|request| {
            request
                .environment
                .iter()
                .any(|(name, _)| name == "JAVA_HOME")
        }));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn test_run_compiles_before_executing_each_test_class() {
        let root = std::env::temp_dir().join(format!("er-ide-test-adapter-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        assert!(fs::create_dir_all(root.join("bin")).is_ok());
        assert!(fs::write(jdk_executable(&root, "javac"), []).is_ok());
        assert!(fs::write(jdk_executable(&root, "java"), []).is_ok());
        let source = root.join("ExampleTest.java");
        assert!(fs::write(&source, "class ExampleTest {}").is_ok());
        let output = root.join("classes");
        let processes = Arc::new(FakeProcesses::default());
        let adapter = JavaJavacAdapter::new(processes.clone());
        let runtime = match tokio::runtime::Builder::new_current_thread().build() {
            Ok(runtime) => runtime,
            Err(error) => panic!("runtime failed: {error}"),
        };

        let result = runtime.block_on(adapter.run_tests(TestRequest {
            compilation: CompilationRequest {
                installation: installation(&root),
                source_files: vec![source],
                output_directory: output.clone(),
                classpath: Classpath {
                    entries: vec![output],
                },
                source_level: None,
                target_level: None,
                additional_args: Vec::new(),
                working_directory: root.clone(),
            },
            test_classes: vec!["ExampleTest".to_owned()],
            args: vec!["--quiet".to_owned()],
        }));

        assert!(result.is_ok_and(|result| {
            result.compilation.success
                && result.cases.len() == 1
                && result.cases[0].execution.success
                && result.cases[0].class_name == "ExampleTest"
        }));
        let requests = match processes.requests.lock() {
            Ok(requests) => requests,
            Err(error) => panic!("request lock failed: {error}"),
        };
        assert_eq!(requests.len(), 2);
        assert!(
            requests[0]
                .program
                .ends_with(jdk_executable(&root, "javac"))
        );
        assert!(requests[1].program.ends_with(jdk_executable(&root, "java")));
        let _ = fs::remove_dir_all(root);
    }
}
