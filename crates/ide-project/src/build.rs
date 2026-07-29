//! Contratos para sistemas de build externos.

use std::{collections::BTreeMap, sync::Arc};

use async_trait::async_trait;
use thiserror::Error;

use crate::model::{BuildSystemId, ModuleId, ProjectDescriptor, ProjectModel};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProjectImportRequest {
    pub descriptor: ProjectDescriptor,
    /// JDK selecionado, repassado às ferramentas externas quando necessário.
    pub environment: BTreeMap<String, String>,
    pub offline: bool,
}

impl ProjectImportRequest {
    #[must_use]
    pub const fn new(descriptor: ProjectDescriptor) -> Self {
        Self {
            descriptor,
            environment: BTreeMap::new(),
            offline: false,
        }
    }

    #[must_use]
    pub fn with_environment_variable(
        mut self,
        name: impl Into<String>,
        value: impl Into<String>,
    ) -> Self {
        self.environment.insert(name.into(), value.into());
        self
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BuildCommandRequest {
    pub descriptor: ProjectDescriptor,
    /// Metas do Maven ou tarefas do Gradle, na ordem informada.
    pub goals: Vec<String>,
    pub module: Option<ModuleId>,
    pub environment: BTreeMap<String, String>,
    pub offline: bool,
    pub extra_args: Vec<String>,
}

impl BuildCommandRequest {
    #[must_use]
    pub fn new(descriptor: ProjectDescriptor, goals: Vec<String>) -> Self {
        Self {
            descriptor,
            goals,
            module: None,
            environment: BTreeMap::new(),
            offline: false,
            extra_args: Vec::new(),
        }
    }

    #[must_use]
    pub fn with_environment_variable(
        mut self,
        name: impl Into<String>,
        value: impl Into<String>,
    ) -> Self {
        self.environment.insert(name.into(), value.into());
        self
    }

    #[must_use]
    pub fn with_module(mut self, module: Option<ModuleId>) -> Self {
        self.module = module;
        self
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BuildCommandResult {
    pub success: bool,
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
    /// Comando executado, já sem segredos, para registro e depuração.
    pub command_line: String,
}

#[async_trait]
pub trait BuildSystemAdapter: Send + Sync {
    fn build_system_id(&self) -> BuildSystemId;

    async fn detect_project(
        &self,
        root: &std::path::Path,
    ) -> Result<Option<ProjectDescriptor>, BuildError>;

    async fn import_project(
        &self,
        request: ProjectImportRequest,
    ) -> Result<ProjectModel, BuildError>;

    async fn execute(&self, request: BuildCommandRequest)
    -> Result<BuildCommandResult, BuildError>;
}

/// Adapters registrados na ordem de prioridade de detecção.
#[derive(Clone, Default)]
pub struct BuildSystemRegistry {
    adapters: Vec<Arc<dyn BuildSystemAdapter>>,
}

impl BuildSystemRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, adapter: Arc<dyn BuildSystemAdapter>) {
        let id = adapter.build_system_id();
        self.adapters
            .retain(|existing| existing.build_system_id() != id);
        self.adapters.push(adapter);
    }

    #[must_use]
    pub fn adapter(&self, id: &BuildSystemId) -> Option<Arc<dyn BuildSystemAdapter>> {
        self.adapters
            .iter()
            .find(|adapter| &adapter.build_system_id() == id)
            .cloned()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.adapters.is_empty()
    }

    /// Primeiro adapter que reconhece a raiz informada.
    pub async fn detect(
        &self,
        root: &std::path::Path,
    ) -> Result<Option<(Arc<dyn BuildSystemAdapter>, ProjectDescriptor)>, BuildError> {
        for adapter in &self.adapters {
            if let Some(descriptor) = adapter.detect_project(root).await? {
                return Ok(Some((adapter.clone(), descriptor)));
            }
        }
        Ok(None)
    }
}

#[derive(Debug, Error)]
pub enum BuildError {
    #[error("directory is not a project of this build system")]
    NotAProject,
    #[error("invalid build manifest: {0}")]
    Manifest(String),
    #[error("build tool was not found: {0}")]
    ToolNotFound(String),
    #[error("build tool failed: {0}")]
    Tool(String),
    #[error("build I/O failed: {0}")]
    Io(String),
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    struct FakeAdapter {
        id: &'static str,
        manifest: &'static str,
    }

    #[async_trait]
    impl BuildSystemAdapter for FakeAdapter {
        fn build_system_id(&self) -> BuildSystemId {
            BuildSystemId(self.id.to_owned())
        }

        async fn detect_project(
            &self,
            root: &Path,
        ) -> Result<Option<ProjectDescriptor>, BuildError> {
            if root.ends_with(self.manifest) {
                return Ok(Some(ProjectDescriptor {
                    build_system: self.build_system_id(),
                    root: root.to_path_buf(),
                    manifest: root.join(self.manifest),
                    name: None,
                    wrapper: None,
                }));
            }
            Ok(None)
        }

        async fn import_project(
            &self,
            request: ProjectImportRequest,
        ) -> Result<ProjectModel, BuildError> {
            Ok(ProjectModel::new(
                self.build_system_id(),
                request.descriptor.root,
                "fake",
            ))
        }

        async fn execute(
            &self,
            _request: BuildCommandRequest,
        ) -> Result<BuildCommandResult, BuildError> {
            Err(BuildError::NotAProject)
        }
    }

    fn runtime() -> tokio::runtime::Runtime {
        match tokio::runtime::Builder::new_current_thread().build() {
            Ok(runtime) => runtime,
            Err(error) => panic!("runtime creation failed: {error}"),
        }
    }

    #[test]
    fn detection_returns_the_first_registered_adapter_that_recognizes_the_root() {
        let mut registry = BuildSystemRegistry::new();
        registry.register(Arc::new(FakeAdapter {
            id: "maven",
            manifest: "maven-project",
        }));
        registry.register(Arc::new(FakeAdapter {
            id: "gradle",
            manifest: "gradle-project",
        }));

        let detected = runtime().block_on(registry.detect(Path::new("/w/gradle-project")));
        assert!(matches!(
            detected,
            Ok(Some((_, descriptor))) if descriptor.build_system == BuildSystemId("gradle".to_owned())
        ));
        let missing = runtime().block_on(registry.detect(Path::new("/w/plain")));
        assert!(matches!(missing, Ok(None)));
    }

    #[test]
    fn registering_the_same_build_system_twice_replaces_the_adapter() {
        let mut registry = BuildSystemRegistry::new();
        registry.register(Arc::new(FakeAdapter {
            id: "maven",
            manifest: "old",
        }));
        registry.register(Arc::new(FakeAdapter {
            id: "maven",
            manifest: "maven-project",
        }));

        let detected = runtime().block_on(registry.detect(Path::new("/w/maven-project")));
        assert!(matches!(detected, Ok(Some(_))));
        assert!(
            registry
                .adapter(&BuildSystemId("maven".to_owned()))
                .is_some()
        );
    }
}
