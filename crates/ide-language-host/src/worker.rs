use ide_domain::{
    CompletionItem, CompletionRequest, DefinitionRequest, Diagnostic, DocumentChange, DocumentId,
    DocumentSnapshot, Location, ReferencesRequest, SemanticSnapshot, SemanticSymbol,
    SyntaxSnapshot,
};
use ide_language_api::LanguageRequestContext;
use tokio::sync::oneshot;

use super::LanguageHostError;

pub(super) enum WorkerRequest {
    Open {
        context: LanguageRequestContext,
        document: DocumentSnapshot,
        response: oneshot::Sender<Result<(), LanguageHostError>>,
    },
    Change {
        context: LanguageRequestContext,
        change: DocumentChange,
        response: oneshot::Sender<Result<(), LanguageHostError>>,
    },
    Close {
        context: LanguageRequestContext,
        document_id: DocumentId,
        response: oneshot::Sender<Result<(), LanguageHostError>>,
    },
    Diagnostics {
        context: LanguageRequestContext,
        document_id: DocumentId,
        response: oneshot::Sender<Result<Vec<Diagnostic>, LanguageHostError>>,
    },
    Syntax {
        context: LanguageRequestContext,
        document_id: DocumentId,
        response: oneshot::Sender<Result<SyntaxSnapshot, LanguageHostError>>,
    },
    Semantic {
        context: LanguageRequestContext,
        document_id: DocumentId,
        response: oneshot::Sender<Result<SemanticSnapshot, LanguageHostError>>,
    },
    Completion {
        context: LanguageRequestContext,
        request: CompletionRequest,
        response: oneshot::Sender<Result<Vec<CompletionItem>, LanguageHostError>>,
    },
    TypeMembers {
        context: LanguageRequestContext,
        type_name: String,
        prefix: String,
        response: oneshot::Sender<Result<Vec<CompletionItem>, LanguageHostError>>,
    },
    WorkspaceTypes {
        context: LanguageRequestContext,
        query: String,
        limit: usize,
        response: oneshot::Sender<Result<Vec<SemanticSymbol>, LanguageHostError>>,
    },
    Definition {
        context: LanguageRequestContext,
        request: DefinitionRequest,
        response: oneshot::Sender<Result<Vec<Location>, LanguageHostError>>,
    },
    References {
        context: LanguageRequestContext,
        request: ReferencesRequest,
        response: oneshot::Sender<Result<Vec<Location>, LanguageHostError>>,
    },
    Shutdown {
        response: oneshot::Sender<Result<(), LanguageHostError>>,
    },
}
