#![doc = "Adapter externo para projetos Gradle."]

mod script;

use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use async_trait::async_trait;
use ide_process::{ProcessRequest, ProcessSupervisor, find_in_path};
use ide_project::{
    build::{
        BuildCommandRequest, BuildCommandResult, BuildError, BuildSystemAdapter,
        ProjectImportRequest,
    },
    model::{
        BuildSystemId, Dependency, ModuleId, ProjectCoordinates, ProjectDescriptor, ProjectModel,
        ProjectModule, SourceRoots,
    },
};

pub const GRADLE_BUILD_SYSTEM_ID: &str = "gradle";
const SETTINGS_FILES: &[&str] = &["settings.gradle", "settings.gradle.kts"];
const BUILD_FILES: &[&str] = &["build.gradle", "build.gradle.kts"];
const MAX_MODULES: usize = 256;

pub struct GradleAdapter {
    processes: Arc<dyn ProcessSupervisor>,
    timeout: Duration,
    cache: Option<PathBuf>,
}

impl GradleAdapter {
    #[must_use]
    pub fn new(processes: Arc<dyn ProcessSupervisor>) -> Self {
        Self {
            processes,
            timeout: Duration::from_secs(900),
            cache: module_cache(),
        }
    }

    #[must_use]
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Substitui o cache de módulos usado para resolver artefatos.
    #[must_use]
    pub fn with_cache(mut self, cache: Option<PathBuf>) -> Self {
        self.cache = cache;
        self
    }

    fn executable(&self, descriptor: &ProjectDescriptor) -> Result<PathBuf, BuildError> {
        if let Some(wrapper) = &descriptor.wrapper
            && wrapper.is_file()
        {
            return Ok(wrapper.clone());
        }
        if let Some(found) = find_in_path("gradle") {
            return Ok(found);
        }
        if let Some(home) = std::env::var_os("GRADLE_HOME") {
            let executable = PathBuf::from(home).join("bin").join(if cfg!(windows) {
                "gradle.bat"
            } else {
                "gradle"
            });
            if executable.is_file() {
                return Ok(executable);
            }
        }
        Err(BuildError::ToolNotFound(
            "gradle was not found in the wrapper, PATH or GRADLE_HOME".to_owned(),
        ))
    }
}

#[async_trait]
impl BuildSystemAdapter for GradleAdapter {
    fn build_system_id(&self) -> BuildSystemId {
        BuildSystemId(GRADLE_BUILD_SYSTEM_ID.to_owned())
    }

    async fn detect_project(&self, root: &Path) -> Result<Option<ProjectDescriptor>, BuildError> {
        let Some(manifest) =
            first_existing(root, SETTINGS_FILES).or_else(|| first_existing(root, BUILD_FILES))
        else {
            return Ok(None);
        };
        let name = first_existing(root, SETTINGS_FILES)
            .and_then(|settings| fs::read_to_string(settings).ok())
            .and_then(|settings| script::root_project_name(&settings))
            .or_else(|| Some(directory_name(root)));
        Ok(Some(ProjectDescriptor {
            build_system: self.build_system_id(),
            root: root.to_path_buf(),
            manifest,
            name,
            wrapper: wrapper_path(root),
        }))
    }

    async fn import_project(
        &self,
        request: ProjectImportRequest,
    ) -> Result<ProjectModel, BuildError> {
        let root = request.descriptor.root.clone();
        if !request.descriptor.manifest.is_file() {
            return Err(BuildError::NotAProject);
        }
        let settings = first_existing(&root, SETTINGS_FILES)
            .and_then(|path| fs::read_to_string(path).ok())
            .unwrap_or_default();
        let name = request
            .descriptor
            .name
            .clone()
            .unwrap_or_else(|| directory_name(&root));
        let mut model = ProjectModel::new(self.build_system_id(), &root, name);

        let mut paths = vec![".".to_owned()];
        paths.extend(script::included_modules(&settings));
        for relative in paths.into_iter().take(MAX_MODULES) {
            let directory = if relative == "." {
                root.clone()
            } else {
                root.join(&relative)
            };
            if !directory.is_dir() {
                continue;
            }
            let Some(manifest) = first_existing(&directory, BUILD_FILES) else {
                continue;
            };
            let id = ModuleId(relative.clone());
            let script = fs::read_to_string(&manifest).unwrap_or_default();
            model.modules.push(ProjectModule {
                id,
                name: if relative == "." {
                    model.name.clone()
                } else {
                    directory_name(&directory)
                },
                root: directory.clone(),
                manifest,
                coordinates: None,
                packaging: "jar".to_owned(),
                source_roots: source_roots(&directory),
                dependencies: dependencies(&script, self.cache.as_deref()),
                output_directory: directory.join("build/classes/java/main"),
                test_output_directory: directory.join("build/classes/java/test"),
                children: Vec::new(),
                plugins: script::declared_plugins(&script),
            });
        }
        let children: Vec<ModuleId> = model
            .modules
            .iter()
            .filter(|module| module.id.0 != ".")
            .map(|module| module.id.clone())
            .collect();
        if let Some(root_module) = model.modules.iter_mut().find(|module| module.id.0 == ".") {
            root_module.children = children;
        }
        if model.modules.is_empty() {
            return Err(BuildError::Manifest(
                "no Gradle module declares a build script".to_owned(),
            ));
        }
        Ok(model)
    }

    async fn execute(
        &self,
        request: BuildCommandRequest,
    ) -> Result<BuildCommandResult, BuildError> {
        if request.goals.is_empty() {
            return Err(BuildError::Tool("no Gradle tasks were provided".to_owned()));
        }
        let program = self.executable(&request.descriptor)?;
        let mut args = vec!["--console=plain".to_owned()];
        if request.offline {
            args.push("--offline".to_owned());
        }
        let prefix = request
            .module
            .as_ref()
            .filter(|module| module.0 != ".")
            .map(|module| format!(":{}:", module.0.replace('/', ":")));
        args.extend(request.goals.iter().map(|goal| match &prefix {
            Some(prefix) if !goal.starts_with(':') => format!("{prefix}{goal}"),
            _ => goal.clone(),
        }));
        args.extend(request.extra_args.iter().cloned());
        let command_line = format!("{} {}", program.display(), args.join(" "));
        let environment = request.environment.into_iter().collect();
        let output = self
            .processes
            .execute(ProcessRequest {
                program,
                args,
                working_directory: Some(request.descriptor.root),
                timeout: Some(self.timeout),
                environment,
            })
            .await
            .map_err(|error| BuildError::Tool(error.to_string()))?;
        Ok(BuildCommandResult {
            success: output.exit_code == 0,
            exit_code: output.exit_code,
            stdout: output.stdout,
            stderr: output.stderr,
            command_line,
        })
    }
}

fn source_roots(directory: &Path) -> SourceRoots {
    let mut roots = SourceRoots::default();
    roots.push_main(directory.join("src/main/java"));
    roots.push_test(directory.join("src/test/java"));
    roots.push_resources(directory.join("src/main/resources"));
    roots.push_resources(directory.join("src/test/resources"));
    for generated in generated_directories(&directory.join("build/generated")) {
        if generated.ends_with("test") {
            roots.push_generated_test(generated);
        } else {
            roots.push_generated(generated);
        }
    }
    roots
}

/// Saídas de geradores ficam em `build/generated/**/{main,test}`.
fn generated_directories(root: &Path) -> Vec<PathBuf> {
    fn collect(directory: &Path, depth: usize, output: &mut Vec<PathBuf>) {
        if depth == 0 || output.len() >= 32 {
            return;
        }
        let Ok(entries) = fs::read_dir(directory) else {
            return;
        };
        let mut children: Vec<PathBuf> = entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.is_dir())
            .take(32)
            .collect();
        children.sort();
        for child in children {
            if child.ends_with("main") || child.ends_with("test") {
                output.push(child);
            } else {
                collect(&child, depth - 1, output);
            }
        }
    }
    let mut directories = Vec::new();
    collect(root, 4, &mut directories);
    directories
}

fn dependencies(script: &str, cache: Option<&Path>) -> Vec<Dependency> {
    script::declared_dependencies(script)
        .into_iter()
        .map(|(coordinates, scope)| {
            let path = cache.and_then(|cache| artifact_path(cache, &coordinates));
            Dependency::new(coordinates)
                .with_scope(scope)
                .with_path(path)
        })
        .collect()
}

/// O cache do Gradle guarda o artefato sob um diretório de hash por versão.
fn artifact_path(cache: &Path, coordinates: &ProjectCoordinates) -> Option<PathBuf> {
    if coordinates.version.is_empty() {
        return None;
    }
    let version_directory = cache
        .join(&coordinates.group)
        .join(&coordinates.artifact)
        .join(&coordinates.version);
    let expected = format!("{}-{}.jar", coordinates.artifact, coordinates.version);
    let mut hashes: Vec<PathBuf> = fs::read_dir(version_directory)
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .take(32)
        .collect();
    hashes.sort();
    hashes
        .into_iter()
        .find_map(|hash| hash.join(&expected).is_file().then(|| hash.join(&expected)))
}

fn first_existing(root: &Path, names: &[&str]) -> Option<PathBuf> {
    names
        .iter()
        .map(|name| root.join(name))
        .find(|candidate| candidate.is_file())
}

fn wrapper_path(root: &Path) -> Option<PathBuf> {
    let names: &[&str] = if cfg!(windows) {
        &["gradlew.bat", "gradlew"]
    } else {
        &["gradlew"]
    };
    names
        .iter()
        .map(|name| root.join(name))
        .find(|candidate| candidate.is_file())
}

fn module_cache() -> Option<PathBuf> {
    let home = std::env::var_os("GRADLE_USER_HOME")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .or_else(|| std::env::var_os("USERPROFILE"))
                .map(|home| PathBuf::from(home).join(".gradle"))
        })?;
    Some(home.join("caches").join("modules-2").join("files-2.1"))
}

fn directory_name(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("project")
        .to_owned()
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use ide_domain::ProcessId;
    use ide_process::{ProcessError, ProcessOutput, ProcessStatus};
    use ide_project::model::DependencyScope;

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
                stdout: "BUILD SUCCESSFUL".to_owned(),
                stderr: String::new(),
            })
        }
    }

    fn write(path: &Path, content: &str) {
        if let Some(parent) = path.parent() {
            assert!(fs::create_dir_all(parent).is_ok());
        }
        assert!(fs::write(path, content).is_ok());
    }

    fn workspace(name: &str) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("er-ide-gradle-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        assert!(fs::create_dir_all(&root).is_ok());
        root
    }

    fn runtime() -> tokio::runtime::Runtime {
        match tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
        {
            Ok(runtime) => runtime,
            Err(error) => panic!("runtime creation failed: {error}"),
        }
    }

    fn multi_module(root: &Path) {
        write(
            &root.join("settings.gradle"),
            "rootProject.name = 'demo'\ninclude ':app'\n",
        );
        write(&root.join("build.gradle"), "plugins { id 'java' }\n");
        write(
            &root.join("app/build.gradle"),
            "dependencies {\n  implementation 'org.slf4j:slf4j-api:1.7.36'\n  testImplementation 'junit:junit:4.13.2'\n}\n",
        );
        write(&root.join("app/src/main/java/Main.java"), "class Main {}");
        write(
            &root.join("app/build/generated/sources/annotationProcessor/java/main/Generated.java"),
            "class Generated {}",
        );
    }

    #[test]
    fn detects_gradle_projects_by_settings_or_build_script() {
        let root = workspace("detect");
        let adapter = GradleAdapter::new(Arc::new(FakeProcesses::default()));
        assert!(matches!(
            runtime().block_on(adapter.detect_project(&root)),
            Ok(None)
        ));

        multi_module(&root);
        let detected = runtime().block_on(adapter.detect_project(&root));
        assert!(matches!(
            detected,
            Ok(Some(descriptor))
                if descriptor.name.as_deref() == Some("demo")
                    && descriptor.manifest == root.join("settings.gradle")
        ));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn imports_included_modules_dependencies_and_generated_sources() {
        let root = workspace("import");
        multi_module(&root);
        let cache = root.join("cache");
        write(
            &cache.join("org.slf4j/slf4j-api/1.7.36/abc123/slf4j-api-1.7.36.jar"),
            "",
        );
        let adapter =
            GradleAdapter::new(Arc::new(FakeProcesses::default())).with_cache(Some(cache.clone()));
        let descriptor = match runtime().block_on(adapter.detect_project(&root)) {
            Ok(Some(descriptor)) => descriptor,
            other => panic!("detection failed: {other:?}"),
        };

        let model = match runtime()
            .block_on(adapter.import_project(ProjectImportRequest::new(descriptor)))
        {
            Ok(model) => model,
            Err(error) => panic!("import failed: {error}"),
        };

        assert_eq!(model.name, "demo");
        assert_eq!(model.modules.len(), 2);
        let app = match model.module(&ModuleId("app".to_owned())) {
            Some(module) => module,
            None => panic!("app module is missing"),
        };
        assert_eq!(app.dependencies.len(), 2);
        assert_eq!(
            app.dependencies[0].path.as_deref(),
            Some(
                cache
                    .join("org.slf4j/slf4j-api/1.7.36/abc123/slf4j-api-1.7.36.jar")
                    .as_path()
            )
        );
        assert_eq!(app.dependencies[1].scope, DependencyScope::Test);
        assert!(
            model
                .generated_source_roots()
                .contains(&root.join("app/build/generated/sources/annotationProcessor/java/main"))
        );
        assert!(
            model
                .library_paths()
                .contains(&root.join("app/build/classes/java/main"))
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn build_runs_the_wrapper_with_qualified_task_and_java_home() {
        let root = workspace("build");
        multi_module(&root);
        let wrapper = root.join(if cfg!(windows) {
            "gradlew.bat"
        } else {
            "gradlew"
        });
        write(&wrapper, "");
        let processes = Arc::new(FakeProcesses::default());
        let adapter = GradleAdapter::new(processes.clone());
        let descriptor = match runtime().block_on(adapter.detect_project(&root)) {
            Ok(Some(descriptor)) => descriptor,
            other => panic!("detection failed: {other:?}"),
        };

        let result = runtime().block_on(
            adapter.execute(
                BuildCommandRequest::new(descriptor, vec!["build".to_owned()])
                    .with_module(Some(ModuleId("app".to_owned())))
                    .with_environment_variable("JAVA_HOME", "/jdk"),
            ),
        );
        assert!(matches!(result, Ok(result) if result.success));

        let requests = match processes.requests.lock() {
            Ok(requests) => requests,
            Err(error) => panic!("request lock failed: {error}"),
        };
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].program, wrapper);
        assert_eq!(
            requests[0].args,
            vec!["--console=plain".to_owned(), ":app:build".to_owned()]
        );
        assert!(
            requests[0]
                .environment
                .iter()
                .any(|(name, _)| name == "JAVA_HOME")
        );
        let _ = fs::remove_dir_all(root);
    }
}
