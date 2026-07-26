#![doc = "Contratos versionados e independentes de linguagem."]

use async_trait::async_trait;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use ide_domain::{
    CompletionItem, CompletionRequest, DefinitionRequest, Diagnostic, DocumentChange, DocumentId,
    DocumentSnapshot, LanguageId, Location, ProviderId, ReferencesRequest, RequestId,
    SemanticSnapshot, SyntaxSnapshot,
};
use thiserror::Error;

pub const LANGUAGE_API_VERSION: ApiVersion = ApiVersion { major: 1, minor: 1 };

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

#[derive(Clone, Debug, Default)]
pub struct CancellationToken {
    cancelled: Arc<AtomicBool>,
}

impl CancellationToken {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }

    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }
}

#[derive(Clone, Debug)]
pub struct LanguageRequestContext {
    pub request_id: RequestId,
    pub cancellation: CancellationToken,
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
    async fn diagnostics(&self, document_id: DocumentId) -> Result<Vec<Diagnostic>, LanguageError>;
    async fn syntax(&self, _document_id: DocumentId) -> Result<SyntaxSnapshot, LanguageError> {
        Err(LanguageError::Unsupported("syntax snapshot".to_owned()))
    }
    async fn semantic(&self, _document_id: DocumentId) -> Result<SemanticSnapshot, LanguageError> {
        Err(LanguageError::Unsupported("semantic snapshot".to_owned()))
    }
    async fn completion(
        &self,
        _request: CompletionRequest,
    ) -> Result<Vec<CompletionItem>, LanguageError> {
        Err(LanguageError::Unsupported("completion".to_owned()))
    }
    async fn definition(
        &self,
        _request: DefinitionRequest,
    ) -> Result<Vec<Location>, LanguageError> {
        Err(LanguageError::Unsupported("definition".to_owned()))
    }
    async fn references(
        &self,
        _request: ReferencesRequest,
    ) -> Result<Vec<Location>, LanguageError> {
        Err(LanguageError::Unsupported("references".to_owned()))
    }
    async fn shutdown(&self) -> Result<(), LanguageError>;
}

#[derive(Debug, Error)]
pub enum LanguageError {
    #[error("provider is disabled")]
    Disabled,
    #[error("request was cancelled")]
    Cancelled,
    #[error("operation is not supported: {0}")]
    Unsupported(String),
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
