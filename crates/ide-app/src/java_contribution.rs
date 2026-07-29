//! Composição embutida da linguagem Java.

use std::sync::Arc;

use async_trait::async_trait;
use ide_application::{
    LanguageContribution, LanguageDescriptor, NewItemTemplate, NewItemTemplateId, SettingsSection,
    TaskDescriptor, TaskExecutionContext, TaskExecutionError, TaskExecutionResult, TaskExecutor,
    TaskId,
};
use ide_debug_api::DebugAdapter;
use ide_domain::LanguageId;
use ide_language_api::LanguageProvider;
use ide_process::ProcessSupervisor;
use ide_project::build::BuildSystemRegistry;
use ide_toolchain_api::{
    CompilationRequest, CompilerAdapter, ExecutionRequest, RuntimeAdapter, TestAdapter,
    TestRequest, ToolchainProvider,
};
use java_debug_adapter::JavaDebugAdapter;
use java_gradle_adapter::GradleAdapter;
use java_maven_adapter::MavenAdapter;
use java_toolchain::{ClasspathBuilder, JavaToolchainAdapter, JavaToolchainProvider};
use language_java::JavaLanguageProvider;

pub const JAVA_LANGUAGE_ID: &str = "java";
pub const COMPILE_TASK_ID: &str = "java.compile";
pub const RUN_TASK_ID: &str = "java.run";
pub const TEST_TASK_ID: &str = "java.test";

#[must_use]
pub fn language_id() -> LanguageId {
    LanguageId(JAVA_LANGUAGE_ID.to_owned())
}

struct JavaTaskExecutor {
    compiler: Arc<dyn CompilerAdapter>,
    runtime: Arc<dyn RuntimeAdapter>,
    tests: Arc<dyn TestAdapter>,
}

#[async_trait]
impl TaskExecutor for JavaTaskExecutor {
    fn supported_language(&self) -> LanguageId {
        language_id()
    }

    async fn execute(
        &self,
        task: &TaskDescriptor,
        context: TaskExecutionContext,
    ) -> Result<TaskExecutionResult, TaskExecutionError> {
        let output_directory = context.workspace_root.join(".er-ide").join("classes");
        let mut builder =
            ClasspathBuilder::new().workspace_defaults(&context.workspace_root, &output_directory);
        for entry in context.classpath_entries {
            builder = builder.with_entry(entry);
        }
        let classpath = builder.build();
        let compilation = CompilationRequest {
            installation: context.installation.clone(),
            source_files: context.source_files,
            output_directory: output_directory.clone(),
            classpath: classpath.clone(),
            additional_args: vec![
                "-source".to_owned(),
                "8".to_owned(),
                "-target".to_owned(),
                "8".to_owned(),
            ],
            working_directory: context.workspace_root.clone(),
        };
        let main_class = context.active_document.as_ref().and_then(main_class_name);

        if task.id.0 == TEST_TASK_ID {
            let main_class = main_class.ok_or_else(|| {
                TaskExecutionError::Failed("Open a Java file before running or testing".to_owned())
            })?;
            let tested = self
                .tests
                .run_tests(TestRequest {
                    compilation,
                    targets: vec![main_class],
                    args: Vec::new(),
                })
                .await
                .map_err(|error| TaskExecutionError::Failed(error.to_string()))?;
            let mut stdout = tested.compilation.stdout;
            let mut stderr = tested.compilation.stderr;
            let mut success = tested.compilation.success;
            for case in tested.cases {
                stdout.push_str(&case.execution.stdout);
                stderr.push_str(&case.execution.stderr);
                success &= case.execution.success;
            }
            return Ok(TaskExecutionResult {
                success,
                status: if success {
                    "Java test completed".to_owned()
                } else {
                    "Java test failed".to_owned()
                },
                stdout,
                stderr,
            });
        }

        let compiled = self
            .compiler
            .compile(compilation)
            .await
            .map_err(|error| TaskExecutionError::Failed(error.to_string()))?;
        let mut stdout = compiled.stdout;
        let mut stderr = compiled.stderr;
        if !compiled.success || task.id.0 == COMPILE_TASK_ID {
            return Ok(TaskExecutionResult {
                success: compiled.success,
                status: if compiled.success {
                    "Java compilation completed".to_owned()
                } else {
                    format!("Java compilation failed ({})", compiled.exit_code)
                },
                stdout,
                stderr,
            });
        }
        if task.id.0 != RUN_TASK_ID {
            return Err(TaskExecutionError::Failed(format!(
                "unsupported Java task: {}",
                task.id.0
            )));
        }
        let main_class = main_class.ok_or_else(|| {
            TaskExecutionError::Failed("Open a Java file before running or testing".to_owned())
        })?;
        let mut run_classpath = classpath;
        if !run_classpath.entries.contains(&output_directory) {
            run_classpath.entries.insert(0, output_directory);
        }
        let executed = self
            .runtime
            .run(ExecutionRequest {
                installation: context.installation,
                entry_point: main_class,
                classpath: run_classpath,
                args: Vec::new(),
                runtime_args: Vec::new(),
                working_directory: context.workspace_root,
            })
            .await
            .map_err(|error| TaskExecutionError::Failed(error.to_string()))?;
        stdout.push_str(&executed.stdout);
        stderr.push_str(&executed.stderr);
        Ok(TaskExecutionResult {
            success: executed.success,
            status: if executed.success {
                "Java execution completed".to_owned()
            } else {
                format!("Java execution failed ({})", executed.exit_code)
            },
            stdout,
            stderr,
        })
    }
}

fn main_class_name(document: &ide_domain::DocumentSnapshot) -> Option<String> {
    if !document
        .path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case(JAVA_LANGUAGE_ID))
    {
        return None;
    }
    let class = document.path.file_stem()?.to_str()?;
    let package = document.text.lines().find_map(|line| {
        line.trim()
            .strip_prefix("package ")
            .and_then(|value| value.strip_suffix(';'))
            .map(str::trim)
    });
    Some(package.map_or_else(|| class.to_owned(), |package| format!("{package}.{class}")))
}

#[must_use]
pub fn contribution(processes: Arc<dyn ProcessSupervisor>) -> LanguageContribution {
    let provider: Arc<dyn LanguageProvider> = Arc::new(JavaLanguageProvider::new());
    let toolchain: Arc<dyn ToolchainProvider> = Arc::new(JavaToolchainProvider::new());
    let adapter = Arc::new(JavaToolchainAdapter::new(processes));
    let compiler: Arc<dyn CompilerAdapter> = adapter.clone();
    let runtime: Arc<dyn RuntimeAdapter> = adapter.clone();
    let tests: Arc<dyn TestAdapter> = adapter;
    let debugger: Arc<dyn DebugAdapter> = Arc::new(JavaDebugAdapter::new());
    let task_executor: Arc<dyn TaskExecutor> = Arc::new(JavaTaskExecutor {
        compiler: compiler.clone(),
        runtime: runtime.clone(),
        tests: tests.clone(),
    });

    let mut contribution = LanguageContribution::new(
        LanguageDescriptor {
            language_id: language_id(),
            display_name: "Java".to_owned(),
            extensions: vec!["java".to_owned()],
        },
        provider,
    );
    contribution.toolchain = Some(toolchain);
    contribution.compiler = Some(compiler);
    contribution.runtime = Some(runtime);
    contribution.tests = Some(tests);
    contribution.debugger = Some(debugger);
    contribution.task_executor = Some(task_executor);
    contribution.new_item_templates = vec![
        NewItemTemplate {
            id: NewItemTemplateId::new("java.package"),
            title: "Novo pacote".to_owned(),
            name_caption: "Classe (opcional)".to_owned(),
            file_extension: None,
            allows_empty_name: true,
        },
        NewItemTemplate {
            id: NewItemTemplateId::new("java.class"),
            title: "Nova classe".to_owned(),
            name_caption: "Nome da classe".to_owned(),
            file_extension: Some("java".to_owned()),
            allows_empty_name: false,
        },
        NewItemTemplate {
            id: NewItemTemplateId::new("java.interface"),
            title: "Nova interface".to_owned(),
            name_caption: "Nome da interface".to_owned(),
            file_extension: Some("java".to_owned()),
            allows_empty_name: false,
        },
    ];
    contribution.settings_sections = vec![SettingsSection {
        id: "java.compiler-vm".to_owned(),
        title: "Compilador e VM".to_owned(),
    }];
    contribution.tasks = vec![
        TaskDescriptor {
            id: TaskId(COMPILE_TASK_ID.to_owned()),
            title: "Compilar".to_owned(),
            requires_active_document: false,
        },
        TaskDescriptor {
            id: TaskId(RUN_TASK_ID.to_owned()),
            title: "Executar".to_owned(),
            requires_active_document: true,
        },
        TaskDescriptor {
            id: TaskId(TEST_TASK_ID.to_owned()),
            title: "Testar".to_owned(),
            requires_active_document: true,
        },
    ];
    contribution
}

pub fn register_build_systems(
    registry: &mut BuildSystemRegistry,
    processes: Arc<dyn ProcessSupervisor>,
) {
    registry.register(Arc::new(MavenAdapter::new(processes.clone())));
    registry.register(Arc::new(GradleAdapter::new(processes)));
}

#[cfg(test)]
mod tests {
    use ide_process::NativeProcessSupervisor;

    use super::*;

    #[test]
    fn java_is_described_as_data_in_one_composition_unit() {
        let contribution = contribution(Arc::new(NativeProcessSupervisor::default()));
        assert_eq!(contribution.descriptor.language_id, language_id());
        assert_eq!(contribution.descriptor.extensions, ["java"]);
        assert!(contribution.toolchain.is_some());
        assert!(contribution.compiler.is_some());
        assert!(contribution.runtime.is_some());
        assert!(contribution.tests.is_some());
        assert!(contribution.debugger.is_some());
        assert!(contribution.task_executor.is_some());
        assert_eq!(contribution.new_item_templates.len(), 3);
        assert_eq!(contribution.settings_sections.len(), 1);
        assert_eq!(contribution.tasks.len(), 3);
    }
}
