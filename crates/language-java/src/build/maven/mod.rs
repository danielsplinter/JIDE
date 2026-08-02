#![doc = "Adapter externo para projetos Maven."]

mod pom;
mod xml;

mod installations;

pub use installations::{MavenInstallation, detect_installations, installation_from_home};

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
        BuildSystemId, Dependency, DependencyScope, ModuleId, ProjectCoordinates,
        ProjectDescriptor, ProjectModel, ProjectModule, SourceRoots,
    },
};

use crate::build::maven::pom::EffectivePom;

pub const MAVEN_BUILD_SYSTEM_ID: &str = "maven";
const MANIFEST: &str = "pom.xml";
const MAX_MODULE_DEPTH: usize = 8;
const MAX_MODULES: usize = 256;

pub struct MavenAdapter {
    processes: Arc<dyn ProcessSupervisor>,
    timeout: Duration,
    repository: Option<PathBuf>,
    /// Instalação escolhida na janela de configurações.
    ///
    /// Vale acima do `PATH` e das variáveis de ambiente, mas **abaixo** do
    /// wrapper do projeto: `mvnw` existe para fixar a versão com que aquele
    /// projeto compila, e a preferência do usuário não pode passar por cima
    /// disso sem avisar.
    home: Option<PathBuf>,
}

impl MavenAdapter {
    #[must_use]
    pub fn new(processes: Arc<dyn ProcessSupervisor>) -> Self {
        Self {
            processes,
            timeout: Duration::from_secs(600),
            repository: local_repository(),
            home: None,
        }
    }

    #[must_use]
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Substitui o repositório local usado para resolver artefatos.
    #[must_use]
    pub fn with_repository(mut self, repository: Option<PathBuf>) -> Self {
        self.repository = repository;
        self
    }

    /// Fixa a instalação a usar, como escolhida nas configurações.
    #[must_use]
    pub fn with_home(mut self, home: Option<PathBuf>) -> Self {
        self.home = home;
        self
    }

    /// Nome do executável do Maven na plataforma.
    fn executable(&self, descriptor: &ProjectDescriptor) -> Result<PathBuf, BuildError> {
        if let Some(wrapper) = &descriptor.wrapper
            && wrapper.is_file()
        {
            return Ok(wrapper.clone());
        }
        // A escolha do usuário vem antes da máquina: ele apontou uma instalação
        // justamente porque a do `PATH` não era a que queria.
        if let Some(home) = &self.home {
            let executavel = home
                .join("bin")
                .join(if cfg!(windows) { "mvn.cmd" } else { "mvn" });
            if executavel.is_file() {
                return Ok(executavel);
            }
        }
        if let Some(found) = find_in_path("mvn") {
            return Ok(found);
        }
        for variable in ["MAVEN_HOME", "M2_HOME"] {
            let Some(home) = std::env::var_os(variable) else {
                continue;
            };
            let executable =
                PathBuf::from(home)
                    .join("bin")
                    .join(if cfg!(windows) { "mvn.cmd" } else { "mvn" });
            if executable.is_file() {
                return Ok(executable);
            }
        }
        Err(BuildError::ToolNotFound(
            "mvn was not found in the wrapper, PATH or MAVEN_HOME".to_owned(),
        ))
    }
}

#[async_trait]
impl BuildSystemAdapter for MavenAdapter {
    fn build_system_id(&self) -> BuildSystemId {
        BuildSystemId(MAVEN_BUILD_SYSTEM_ID.to_owned())
    }

    async fn detect_project(&self, root: &Path) -> Result<Option<ProjectDescriptor>, BuildError> {
        let manifest = root.join(MANIFEST);
        if !manifest.is_file() {
            return Ok(None);
        }
        let name = pom::read(&manifest, None)
            .ok()
            .map(|pom| pom.name.unwrap_or(pom.artifact));
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
        let mut model = ProjectModel::new(self.build_system_id(), &root, "");
        let repository = self.repository.clone();
        import_module(
            &root,
            None,
            &mut model,
            repository.as_deref(),
            MAX_MODULE_DEPTH,
        )?;
        let root_module = model.root_module().cloned();
        model.name = root_module
            .as_ref()
            .map_or_else(|| directory_name(&root), |module| module.name.clone());
        if let Some(module) = root_module
            && let Ok(pom) = pom::read(&module.manifest, None)
        {
            model.properties = pom.properties;
        }
        Ok(model)
    }

    async fn execute(
        &self,
        request: BuildCommandRequest,
    ) -> Result<BuildCommandResult, BuildError> {
        if request.goals.is_empty() {
            return Err(BuildError::Tool("no Maven goals were provided".to_owned()));
        }
        let program = self.executable(&request.descriptor)?;
        let mut args = vec!["-B".to_owned()];
        if request.offline {
            args.push("-o".to_owned());
        }
        if let Some(module) = &request.module
            && module.0 != "."
        {
            args.extend(["-pl".to_owned(), module.0.clone()]);
        }
        args.extend(request.goals.iter().cloned());
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

fn import_module(
    directory: &Path,
    parent: Option<&EffectivePom>,
    model: &mut ProjectModel,
    repository: Option<&Path>,
    depth: usize,
) -> Result<Option<ModuleId>, BuildError> {
    if depth == 0 || model.modules.len() >= MAX_MODULES {
        return Ok(None);
    }
    let manifest = directory.join(MANIFEST);
    if !manifest.is_file() {
        return Ok(None);
    }
    let effective = pom::read(&manifest, parent).map_err(BuildError::Manifest)?;
    let id = module_id(&model.root, directory);
    let module_index = model.modules.len();
    model.modules.push(ProjectModule {
        id: id.clone(),
        name: effective
            .name
            .clone()
            .unwrap_or_else(|| effective.artifact.clone()),
        root: directory.to_path_buf(),
        manifest,
        coordinates: Some(ProjectCoordinates {
            group: effective.group.clone(),
            artifact: effective.artifact.clone(),
            version: effective.version.clone(),
        }),
        packaging: effective.packaging.clone(),
        source_roots: source_roots(directory, &effective),
        dependencies: dependencies(&effective, repository),
        output_directory: output_directory(directory, &effective, false),
        test_output_directory: output_directory(directory, &effective, true),
        children: Vec::new(),
        plugins: effective.plugins.clone(),
    });

    let mut children = Vec::new();
    for module in &effective.modules {
        let child_directory = normalize(&directory.join(module));
        let child_directory = if child_directory.join(MANIFEST).is_file() {
            child_directory
        } else {
            continue;
        };
        if let Some(child) = import_module(
            &child_directory,
            Some(&effective),
            model,
            repository,
            depth - 1,
        )? {
            children.push(child);
        }
    }
    if let Some(module) = model.modules.get_mut(module_index) {
        module.children = children;
    }
    Ok(Some(id))
}

fn source_roots(directory: &Path, effective: &EffectivePom) -> SourceRoots {
    let mut roots = SourceRoots::default();
    roots.push_main(resolve(
        directory,
        effective.source_directory.as_deref(),
        "src/main/java",
    ));
    roots.push_test(resolve(
        directory,
        effective.test_source_directory.as_deref(),
        "src/test/java",
    ));
    roots.push_resources(directory.join("src").join("main").join("resources"));
    roots.push_resources(directory.join("src").join("test").join("resources"));
    let build = resolve(directory, effective.build_directory.as_deref(), "target");
    for entry in generated_directories(&build.join("generated-sources")) {
        roots.push_generated(entry);
    }
    for entry in generated_directories(&build.join("generated-test-sources")) {
        roots.push_generated_test(entry);
    }
    roots
}

/// Cada gerador do Maven cria seu próprio diretório sob `generated-sources`.
fn generated_directories(root: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(root) else {
        return Vec::new();
    };
    let mut directories: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .take(64)
        .collect();
    directories.sort();
    directories
}

fn dependencies(effective: &EffectivePom, repository: Option<&Path>) -> Vec<Dependency> {
    effective
        .dependencies
        .iter()
        .map(|raw| {
            let version = raw
                .version
                .clone()
                .or_else(|| effective.managed_versions.get(&raw.key()).cloned())
                .unwrap_or_default();
            let coordinates = ProjectCoordinates {
                group: raw.group.clone(),
                artifact: raw.artifact.clone(),
                version,
            };
            let scope = raw
                .scope
                .as_deref()
                .map_or(DependencyScope::Compile, DependencyScope::parse);
            let path = raw
                .system_path
                .as_ref()
                .map(PathBuf::from)
                .filter(|path| path.is_file())
                .or_else(|| artifact_path(repository?, &coordinates));
            Dependency {
                coordinates,
                scope,
                optional: raw.optional,
                path,
            }
        })
        .collect()
}

/// Layout padrão do repositório local: `group/path/artifact/version/artifact-version.jar`.
fn artifact_path(repository: &Path, coordinates: &ProjectCoordinates) -> Option<PathBuf> {
    if coordinates.version.is_empty() {
        return None;
    }
    let mut path = repository.to_path_buf();
    for segment in coordinates.group.split('.') {
        path.push(segment);
    }
    path.push(&coordinates.artifact);
    path.push(&coordinates.version);
    path.push(format!(
        "{}-{}.jar",
        coordinates.artifact, coordinates.version
    ));
    path.is_file().then_some(path)
}

fn output_directory(directory: &Path, effective: &EffectivePom, test: bool) -> PathBuf {
    let declared = if test {
        effective.test_output_directory.as_deref()
    } else {
        effective.output_directory.as_deref()
    };
    if let Some(declared) = declared {
        return resolve(directory, Some(declared), "");
    }
    let build = resolve(directory, effective.build_directory.as_deref(), "target");
    build.join(if test { "test-classes" } else { "classes" })
}

fn resolve(directory: &Path, declared: Option<&str>, fallback: &str) -> PathBuf {
    let value = declared.unwrap_or(fallback);
    let candidate = PathBuf::from(value);
    if candidate.is_absolute() {
        candidate
    } else {
        normalize(&directory.join(candidate))
    }
}

/// Resolve `.` e `..` sem tocar no sistema de arquivos.
fn normalize(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other),
        }
    }
    normalized
}

/// Identidade do módulo relativa à raiz, no formato aceito por `mvn -pl`.
fn module_id(root: &Path, directory: &Path) -> ModuleId {
    let relative = directory
        .strip_prefix(root)
        .ok()
        .map(|relative| relative.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|| directory.to_string_lossy().into_owned());
    ModuleId(if relative.is_empty() {
        ".".to_owned()
    } else {
        relative
    })
}

fn wrapper_path(root: &Path) -> Option<PathBuf> {
    let names: &[&str] = if cfg!(windows) {
        &["mvnw.cmd", "mvnw.bat", "mvnw"]
    } else {
        &["mvnw"]
    };
    names
        .iter()
        .map(|name| root.join(name))
        .find(|candidate| candidate.is_file())
}

fn local_repository() -> Option<PathBuf> {
    if let Some(repository) = std::env::var_os("M2_REPO") {
        return Some(PathBuf::from(repository));
    }
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)?;
    Some(home.join(".m2").join("repository"))
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
                stdout: "BUILD SUCCESS".to_owned(),
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
        let root = std::env::temp_dir().join(format!("er-ide-maven-{name}-{}", std::process::id()));
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
            &root.join("pom.xml"),
            r#"<project>
  <groupId>com.example</groupId>
  <artifactId>demo</artifactId>
  <version>1.0.0</version>
  <packaging>pom</packaging>
  <name>Demo</name>
  <properties><slf4j.version>1.7.36</slf4j.version></properties>
  <dependencyManagement>
    <dependencies>
      <dependency>
        <groupId>org.slf4j</groupId>
        <artifactId>slf4j-api</artifactId>
        <version>${slf4j.version}</version>
      </dependency>
    </dependencies>
  </dependencyManagement>
  <modules>
    <module>app</module>
    <module>missing</module>
  </modules>
</project>"#,
        );
        write(
            &root.join("app").join("pom.xml"),
            r#"<project>
  <parent>
    <groupId>com.example</groupId>
    <artifactId>demo</artifactId>
    <version>1.0.0</version>
  </parent>
  <artifactId>demo-app</artifactId>
  <dependencies>
    <dependency>
      <groupId>org.slf4j</groupId>
      <artifactId>slf4j-api</artifactId>
    </dependency>
    <dependency>
      <groupId>junit</groupId>
      <artifactId>junit</artifactId>
      <version>4.13.2</version>
      <scope>test</scope>
    </dependency>
  </dependencies>
  <build>
    <plugins>
      <plugin>
        <groupId>org.springframework.boot</groupId>
        <artifactId>spring-boot-maven-plugin</artifactId>
      </plugin>
    </plugins>
  </build>
</project>"#,
        );
        write(&root.join("app/src/main/java/Main.java"), "class Main {}");
        write(
            &root.join("app/target/generated-sources/annotations/Generated.java"),
            "class Generated {}",
        );
    }

    #[test]
    fn detects_maven_projects_and_ignores_other_directories() {
        let root = workspace("detect");
        let adapter = MavenAdapter::new(Arc::new(FakeProcesses::default()));
        let plain = runtime().block_on(adapter.detect_project(&root));
        assert!(matches!(plain, Ok(None)));

        multi_module(&root);
        let detected = runtime().block_on(adapter.detect_project(&root));
        assert!(matches!(
            detected,
            Ok(Some(descriptor))
                if descriptor.name.as_deref() == Some("Demo")
                    && descriptor.manifest == root.join("pom.xml")
                    && descriptor.wrapper.is_none()
        ));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn imports_modules_dependencies_and_generated_sources() {
        let root = workspace("import");
        multi_module(&root);
        let repository = root.join("repo");
        write(
            &repository.join("org/slf4j/slf4j-api/1.7.36/slf4j-api-1.7.36.jar"),
            "",
        );
        let adapter = MavenAdapter::new(Arc::new(FakeProcesses::default()))
            .with_repository(Some(repository.clone()));
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

        assert_eq!(model.name, "Demo");
        assert_eq!(model.modules.len(), 2, "aggregator and app module");
        let app = match model.module(&ModuleId("app".to_owned())) {
            Some(module) => module,
            None => panic!("app module is missing"),
        };
        assert_eq!(app.packaging, "jar");
        assert_eq!(
            app.coordinates.as_ref().map(ProjectCoordinates::label),
            Some("com.example:demo-app:1.0.0".to_owned())
        );
        assert_eq!(app.dependencies.len(), 2);
        let slf4j = match app
            .dependencies
            .iter()
            .find(|dependency| dependency.coordinates.artifact == "slf4j-api")
        {
            Some(dependency) => dependency,
            None => panic!("managed dependency is missing"),
        };
        assert_eq!(
            slf4j.coordinates.version, "1.7.36",
            "version comes from dependencyManagement with an interpolated property"
        );
        assert_eq!(
            slf4j.path.as_deref(),
            Some(
                repository
                    .join("org/slf4j/slf4j-api/1.7.36/slf4j-api-1.7.36.jar")
                    .as_path()
            )
        );
        assert!(
            app.dependencies
                .iter()
                .any(|dependency| dependency.scope == DependencyScope::Test)
        );
        assert!(
            model.declares_plugin("spring-boot-maven-plugin"),
            "os plugins do build identificam como a aplicação é executada"
        );
        assert!(
            model
                .generated_source_roots()
                .contains(&root.join("app/target/generated-sources/annotations"))
        );
        assert!(model.contains_source(&root.join("app/src/main/java/Main.java")));
        assert!(
            model
                .library_paths()
                .contains(&root.join("app/target/classes"))
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn build_runs_the_wrapper_with_batch_mode_module_and_java_home() {
        let root = workspace("build");
        multi_module(&root);
        let wrapper = root.join(if cfg!(windows) { "mvnw.cmd" } else { "mvnw" });
        write(&wrapper, "");
        let processes = Arc::new(FakeProcesses::default());
        let adapter = MavenAdapter::new(processes.clone());
        let descriptor = match runtime().block_on(adapter.detect_project(&root)) {
            Ok(Some(descriptor)) => descriptor,
            other => panic!("detection failed: {other:?}"),
        };
        assert_eq!(descriptor.wrapper.as_deref(), Some(wrapper.as_path()));

        let result = runtime().block_on(
            adapter.execute(
                BuildCommandRequest::new(descriptor, vec!["compile".to_owned()])
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
            vec![
                "-B".to_owned(),
                "-pl".to_owned(),
                "app".to_owned(),
                "compile".to_owned()
            ]
        );
        assert!(
            requests[0]
                .environment
                .iter()
                .any(|(name, value)| name == "JAVA_HOME" && value.contains("jdk"))
        );
        assert_eq!(
            requests[0].working_directory.as_deref(),
            Some(root.as_path())
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn build_requires_goals_and_import_requires_a_manifest() {
        let root = workspace("errors");
        let adapter = MavenAdapter::new(Arc::new(FakeProcesses::default()));
        let descriptor = ProjectDescriptor {
            build_system: BuildSystemId(MAVEN_BUILD_SYSTEM_ID.to_owned()),
            root: root.clone(),
            manifest: root.join(MANIFEST),
            name: None,
            wrapper: None,
        };
        assert!(matches!(
            runtime().block_on(
                adapter.execute(BuildCommandRequest::new(descriptor.clone(), Vec::new()))
            ),
            Err(BuildError::Tool(_))
        ));
        assert!(matches!(
            runtime().block_on(adapter.import_project(ProjectImportRequest::new(descriptor))),
            Err(BuildError::NotAProject)
        ));
        let _ = fs::remove_dir_all(root);
    }
}
