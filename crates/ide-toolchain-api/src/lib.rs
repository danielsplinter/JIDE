#![doc = "Contratos para toolchains externas."]

use std::path::PathBuf;

use async_trait::async_trait;
use ide_domain::LanguageId;
use thiserror::Error;

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct ToolchainId(pub String);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToolchainInstallation {
    pub id: ToolchainId,
    pub home: PathBuf,
    pub version: Option<String>,
}

#[derive(Clone, Debug)]
pub struct DetectionContext {
    pub workspace_root: Option<PathBuf>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToolchainValidation {
    pub valid: bool,
    pub details: Vec<String>,
}

#[async_trait]
pub trait ToolchainProvider: Send + Sync {
    fn toolchain_id(&self) -> ToolchainId;
    fn supported_languages(&self) -> Vec<LanguageId>;
    async fn detect(
        &self,
        context: DetectionContext,
    ) -> Result<Vec<ToolchainInstallation>, ToolchainError>;
    async fn validate(
        &self,
        installation: &ToolchainInstallation,
    ) -> Result<ToolchainValidation, ToolchainError>;
}

#[derive(Debug, Error)]
pub enum ToolchainError {
    #[error("toolchain executable was not found")]
    NotFound,
    #[error("toolchain operation failed: {0}")]
    Operation(String),
}

