#![doc = "Contratos versionados e independentes de linguagem."]

use async_trait::async_trait;
use ide_domain::{Diagnostic, DocumentChange, DocumentId, DocumentSnapshot, LanguageId, ProviderId};
use thiserror::Error;

pub const LANGUAGE_API_VERSION: ApiVersion = ApiVersion { major: 1, minor: 0 };

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ApiVersion {
    pub major: u16,
    pub minor: u16,
}

bitflags::bitflags! {
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub struct LanguageCapabilities: u64 {
        const SYNTAX = 1 << 0;
        const SEMANTICS = 1 << 1;
        const COMPLETION = 1 << 2;
        const DIAGNOSTICS = 1 << 3;
        const DEFINITION = 1 << 4;
        const REFERENCES = 1 << 5;
        const RENAME = 1 << 6;
        const FORMAT = 1 << 7;
        const BUILD = 1 << 8;
        const RUN = 1 << 9;
        const DEBUG = 1 << 10;
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LanguageMetadata {
    pub language_id: LanguageId,
    pub provider_id: ProviderId,
    pub display_name: String,
    pub extensions: Vec<String>,
    pub api_version: ApiVersion,
}

#[derive(Clone, Debug)]
pub struct LanguageActivationContext {
    pub workspace_root: std::path::PathBuf,
}

#[async_trait]
pub trait LanguageProvider: Send + Sync {
    fn metadata(&self) -> LanguageMetadata;
    fn capabilities(&self) -> LanguageCapabilities;
    async fn activate(
        &self,
        context: LanguageActivationContext,
    ) -> Result<Box<dyn ActiveLanguage>, LanguageError>;
}

#[async_trait]
pub trait ActiveLanguage: Send + Sync {
    fn language_id(&self) -> &LanguageId;
    async fn open_document(&self, document: DocumentSnapshot) -> Result<(), LanguageError>;
    async fn change_document(&self, change: DocumentChange) -> Result<(), LanguageError>;
    async fn close_document(&self, document_id: DocumentId) -> Result<(), LanguageError>;
    async fn diagnostics(&self, document_id: DocumentId)
        -> Result<Vec<Diagnostic>, LanguageError>;
    async fn shutdown(&self) -> Result<(), LanguageError>;
}

#[derive(Debug, Error)]
pub enum LanguageError {
    #[error("provider is disabled")]
    Disabled,
    #[error("provider failed: {0}")]
    Provider(String),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProviderState {
    Registered,
    Disabled,
    Activating,
    Active,
    Suspended,
    Failed,
    ShuttingDown,
}

