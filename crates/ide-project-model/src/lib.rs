#![doc = "Modelo neutro de projeto compartilhado pelos build systems."]

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct BuildSystemId(pub String);

impl BuildSystemId {
    #[must_use]
    pub fn label(&self) -> String {
        let mut characters = self.0.chars();
        characters.next().map_or_else(String::new, |first| {
            first.to_uppercase().collect::<String>() + characters.as_str()
        })
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ModuleId(pub String);

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ProjectCoordinates {
    pub group: String,
    pub artifact: String,
    pub version: String,
}

impl ProjectCoordinates {
    #[must_use]
    pub fn label(&self) -> String {
        if self.version.is_empty() {
            format!("{}:{}", self.group, self.artifact)
        } else {
            format!("{}:{}:{}", self.group, self.artifact, self.version)
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum DependencyScope {
    #[default]
    Compile,
    Provided,
    Runtime,
    Test,
    System,
}

impl DependencyScope {
    #[must_use]
    pub fn parse(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "provided" | "compileonly" | "compileonlyapi" => Self::Provided,
            "runtime" | "runtimeonly" => Self::Runtime,
            "test" | "testimplementation" | "testcompileonly" | "testruntimeonly" => Self::Test,
            "system" => Self::System,
            _ => Self::Compile,
        }
    }

    /// Escopos que não participam da compilação das fontes principais.
    #[must_use]
    pub const fn is_test_only(self) -> bool {
        matches!(self, Self::Test)
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Compile => "compile",
            Self::Provided => "provided",
            Self::Runtime => "runtime",
            Self::Test => "test",
            Self::System => "system",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Dependency {
    pub coordinates: ProjectCoordinates,
    pub scope: DependencyScope,
    pub optional: bool,
    /// Artefato resolvido no repositório local, quando encontrado.
    pub path: Option<PathBuf>,
}

impl Dependency {
    #[must_use]
    pub fn new(coordinates: ProjectCoordinates) -> Self {
        Self {
            coordinates,
            scope: DependencyScope::default(),
            optional: false,
            path: None,
        }
    }

    #[must_use]
    pub fn with_scope(mut self, scope: DependencyScope) -> Self {
        self.scope = scope;
        self
    }

    #[must_use]
    pub fn with_path(mut self, path: Option<PathBuf>) -> Self {
        self.path = path;
        self
    }
}

/// Raízes de código de um módulo, separadas por finalidade.
///
/// `generated` e `generated_test` guardam saídas de processadores de anotações e
/// geradores do build, que participam da análise e da compilação como qualquer
/// outra fonte, mas nunca são editadas pelo usuário.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SourceRoots {
    pub main: Vec<PathBuf>,
    pub test: Vec<PathBuf>,
    pub resources: Vec<PathBuf>,
    pub generated: Vec<PathBuf>,
    pub generated_test: Vec<PathBuf>,
}

impl SourceRoots {
    pub fn push_main(&mut self, path: impl Into<PathBuf>) {
        push_unique(&mut self.main, path.into());
    }

    pub fn push_test(&mut self, path: impl Into<PathBuf>) {
        push_unique(&mut self.test, path.into());
    }

    pub fn push_resources(&mut self, path: impl Into<PathBuf>) {
        push_unique(&mut self.resources, path.into());
    }

    pub fn push_generated(&mut self, path: impl Into<PathBuf>) {
        push_unique(&mut self.generated, path.into());
    }

    pub fn push_generated_test(&mut self, path: impl Into<PathBuf>) {
        push_unique(&mut self.generated_test, path.into());
    }

    /// Todas as raízes que contêm código compilável, incluindo geradas.
    #[must_use]
    pub fn compilable(&self) -> Vec<PathBuf> {
        let mut roots = Vec::new();
        for group in [
            &self.main,
            &self.generated,
            &self.test,
            &self.generated_test,
        ] {
            for root in group {
                push_unique(&mut roots, root.clone());
            }
        }
        roots
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.compilable().is_empty() && self.resources.is_empty()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProjectModule {
    pub id: ModuleId,
    pub name: String,
    pub root: PathBuf,
    pub manifest: PathBuf,
    pub coordinates: Option<ProjectCoordinates>,
    pub packaging: String,
    pub source_roots: SourceRoots,
    pub dependencies: Vec<Dependency>,
    pub output_directory: PathBuf,
    pub test_output_directory: PathBuf,
    pub children: Vec<ModuleId>,
    /// Plugins declarados pelo módulo, como `spring-boot-maven-plugin` ou
    /// `org.springframework.boot`. Identificam como a aplicação é executada.
    pub plugins: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProjectModel {
    pub build_system: BuildSystemId,
    pub root: PathBuf,
    pub name: String,
    pub modules: Vec<ProjectModule>,
    pub properties: BTreeMap<String, String>,
}

impl ProjectModel {
    #[must_use]
    pub fn new(
        build_system: BuildSystemId,
        root: impl Into<PathBuf>,
        name: impl Into<String>,
    ) -> Self {
        Self {
            build_system,
            root: root.into(),
            name: name.into(),
            modules: Vec::new(),
            properties: BTreeMap::new(),
        }
    }

    #[must_use]
    pub fn module(&self, id: &ModuleId) -> Option<&ProjectModule> {
        self.modules.iter().find(|module| &module.id == id)
    }

    #[must_use]
    pub fn root_module(&self) -> Option<&ProjectModule> {
        self.modules
            .iter()
            .find(|module| module.root == self.root)
            .or_else(|| self.modules.first())
    }

    /// Raízes de código de todos os módulos, incluindo as geradas.
    #[must_use]
    pub fn source_roots(&self) -> Vec<PathBuf> {
        let mut roots = Vec::new();
        for module in &self.modules {
            for root in module.source_roots.compilable() {
                push_unique(&mut roots, root);
            }
        }
        roots
    }

    #[must_use]
    pub fn generated_source_roots(&self) -> Vec<PathBuf> {
        let mut roots = Vec::new();
        for module in &self.modules {
            for root in module
                .source_roots
                .generated
                .iter()
                .chain(&module.source_roots.generated_test)
            {
                push_unique(&mut roots, root.clone());
            }
        }
        roots
    }

    /// Diretórios de saída e artefatos de dependências, na ordem de resolução.
    #[must_use]
    pub fn classpath_entries(&self) -> Vec<PathBuf> {
        let mut entries = Vec::new();
        for module in &self.modules {
            push_unique(&mut entries, module.output_directory.clone());
            push_unique(&mut entries, module.test_output_directory.clone());
        }
        for module in &self.modules {
            for dependency in &module.dependencies {
                if let Some(path) = &dependency.path {
                    push_unique(&mut entries, path.clone());
                }
            }
        }
        entries
    }

    /// Dependências únicas do projeto inteiro, ordenadas por coordenada.
    #[must_use]
    pub fn dependencies(&self) -> Vec<Dependency> {
        let mut dependencies: Vec<Dependency> = Vec::new();
        for module in &self.modules {
            for dependency in &module.dependencies {
                if !dependencies
                    .iter()
                    .any(|existing| existing.coordinates == dependency.coordinates)
                {
                    dependencies.push(dependency.clone());
                }
            }
        }
        dependencies.sort_by_key(|dependency| dependency.coordinates.label());
        dependencies
    }

    /// Indica se o arquivo pertence a alguma raiz de código do projeto.
    #[must_use]
    pub fn contains_source(&self, path: &Path) -> bool {
        self.source_roots()
            .iter()
            .any(|root| path.starts_with(root))
    }

    /// Indica que algum módulo declara o plugin informado.
    #[must_use]
    pub fn declares_plugin(&self, plugin: &str) -> bool {
        self.modules
            .iter()
            .any(|module| module.plugins.iter().any(|declared| declared == plugin))
    }

    #[must_use]
    pub fn summary(&self) -> String {
        format!(
            "{} • {} • {} módulo(s) • {} dependência(s)",
            self.build_system.label(),
            self.name,
            self.modules.len(),
            self.dependencies().len()
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProjectDescriptor {
    pub build_system: BuildSystemId,
    pub root: PathBuf,
    /// Arquivo que identificou o projeto (`pom.xml`, `settings.gradle`, ...).
    pub manifest: PathBuf,
    pub name: Option<String>,
    /// Wrapper versionado no projeto, quando existir.
    pub wrapper: Option<PathBuf>,
}

fn push_unique(entries: &mut Vec<PathBuf>, path: PathBuf) {
    if !entries.contains(&path) {
        entries.push(path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn module(name: &str, root: &str) -> ProjectModule {
        let root = PathBuf::from(root);
        let mut source_roots = SourceRoots::default();
        source_roots.push_main(root.join("src/main/java"));
        source_roots.push_test(root.join("src/test/java"));
        source_roots.push_generated(root.join("target/generated-sources/annotations"));
        ProjectModule {
            id: ModuleId(name.to_owned()),
            name: name.to_owned(),
            root: root.clone(),
            manifest: root.join("pom.xml"),
            coordinates: Some(ProjectCoordinates {
                group: "com.example".to_owned(),
                artifact: name.to_owned(),
                version: "1.0.0".to_owned(),
            }),
            packaging: "jar".to_owned(),
            source_roots,
            dependencies: vec![
                Dependency::new(ProjectCoordinates {
                    group: "org.example".to_owned(),
                    artifact: "shared".to_owned(),
                    version: "2.0".to_owned(),
                })
                .with_path(Some(PathBuf::from("/repo/shared-2.0.jar"))),
            ],
            output_directory: root.join("target/classes"),
            test_output_directory: root.join("target/test-classes"),
            children: Vec::new(),
            plugins: vec!["spring-boot-maven-plugin".to_owned()],
        }
    }

    #[test]
    fn source_roots_include_generated_code_without_duplicates() {
        let mut model = ProjectModel::new(BuildSystemId("maven".to_owned()), "/p", "demo");
        model.modules = vec![module("app", "/p/app"), module("lib", "/p/lib")];

        let roots = model.source_roots();
        assert!(roots.contains(&PathBuf::from(
            "/p/app/target/generated-sources/annotations"
        )));
        assert_eq!(roots.len(), 6);
        assert_eq!(model.generated_source_roots().len(), 2);
        assert!(model.contains_source(Path::new("/p/lib/src/main/java/Main.java")));
        assert!(!model.contains_source(Path::new("/p/other/Main.java")));
    }

    #[test]
    fn classpath_lists_outputs_before_dependencies_and_deduplicates() {
        let mut model = ProjectModel::new(BuildSystemId("gradle".to_owned()), "/p", "demo");
        model.modules = vec![module("app", "/p/app"), module("lib", "/p/lib")];

        let entries = model.classpath_entries();
        assert_eq!(entries.len(), 5);
        assert_eq!(entries[0], PathBuf::from("/p/app/target/classes"));
        assert_eq!(entries[4], PathBuf::from("/repo/shared-2.0.jar"));
        assert_eq!(model.dependencies().len(), 1);
    }

    #[test]
    fn summary_reports_build_system_modules_and_dependencies() {
        let mut model = ProjectModel::new(BuildSystemId("maven".to_owned()), "/p", "demo");
        model.modules = vec![module("app", "/p/app")];

        assert_eq!(
            model.summary(),
            "Maven • demo • 1 módulo(s) • 1 dependência(s)"
        );
        assert!(model.declares_plugin("spring-boot-maven-plugin"));
        assert!(!model.declares_plugin("maven-war-plugin"));
    }

    #[test]
    fn scopes_are_parsed_from_maven_and_gradle_vocabulary() {
        assert_eq!(DependencyScope::parse("test"), DependencyScope::Test);
        assert_eq!(
            DependencyScope::parse("testImplementation"),
            DependencyScope::Test
        );
        assert_eq!(
            DependencyScope::parse("compileOnly"),
            DependencyScope::Provided
        );
        assert_eq!(
            DependencyScope::parse("runtimeOnly"),
            DependencyScope::Runtime
        );
        assert_eq!(
            DependencyScope::parse("implementation"),
            DependencyScope::Compile
        );
        assert!(DependencyScope::Test.is_test_only());
        assert!(!DependencyScope::Provided.is_test_only());
    }
}
