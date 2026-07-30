use ide_domain::{
    AccessorKind, AccessorPlan, TextPosition,
    CompletionItem, CompletionRequest, DefinitionRequest, Diagnostic, DocumentChange, DocumentId,
    DocumentSnapshot, Location, ReferencesRequest, SemanticSnapshot,
    SemanticSymbol,
    SyntaxSnapshot,
};
use ide_language_api::{
    ActiveLanguage, LanguageActivationContext, LanguageMetadata, LanguageProvider,
    LanguageRequestContext, MemberAccess,
};
use tokio::sync::oneshot;

use crate::host::LanguageHostError;
use crate::routing::ensure_not_cancelled;

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
    MemberAccess {
        context: LanguageRequestContext,
        text: String,
        offset: usize,
        response: oneshot::Sender<Result<Option<MemberAccess>, LanguageHostError>>,
    },
    TypeMembers {
        context: LanguageRequestContext,
        type_name: String,
        prefix: String,
        response: oneshot::Sender<Result<Vec<CompletionItem>, LanguageHostError>>,
    },
    AccessorPlanFor {
        context: LanguageRequestContext,
        document_id: DocumentId,
        position: TextPosition,
        kind: AccessorKind,
        response: oneshot::Sender<Result<AccessorPlan, LanguageHostError>>,
    },
    ReferencesToName {
        context: LanguageRequestContext,
        name: String,
        response: oneshot::Sender<Result<Vec<Location>, LanguageHostError>>,
    },
    ConstructorSource {
        context: LanguageRequestContext,
        document_id: DocumentId,
        position: TextPosition,
        fields: Vec<String>,
        response: oneshot::Sender<Result<Option<String>, LanguageHostError>>,
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

pub(super) struct ProviderWorker {
    sender: SyncSender<WorkerRequest>,
    thread: Mutex<Option<JoinHandle<()>>>,
}

impl ProviderWorker {
    pub(super) fn spawn(
        provider: Arc<dyn LanguageProvider>,
        metadata: LanguageMetadata,
        context: LanguageActivationContext,
        queue_capacity: usize,
    ) -> Result<Self, LanguageHostError> {
        let (sender, receiver) = sync_channel(queue_capacity);
        let (initialized_tx, initialized_rx) = sync_channel(1);
        let thread_name = format!("language-{}", metadata.provider_id.0);
        let handle = thread::Builder::new()
            .name(thread_name)
            .spawn(move || {
                let runtime = match tokio::runtime::Builder::new_current_thread()
                    .enable_time()
                    .build()
                {
                    Ok(runtime) => runtime,
                    Err(error) => {
                        let _ = initialized_tx
                            .send(Err(LanguageHostError::Provider(error.to_string())));
                        return;
                    }
                };
                let active = match runtime.block_on(provider.activate(context)) {
                    Ok(active) if active.language_id() == &metadata.language_id => active,
                    Ok(_) => {
                        let _ = initialized_tx.send(Err(LanguageHostError::InvalidMetadata(
                            "active provider returned a different language id".to_owned(),
                        )));
                        return;
                    }
                    Err(error) => {
                        let _ = initialized_tx.send(Err(error.into()));
                        return;
                    }
                };
                if initialized_tx.send(Ok(())).is_err() {
                    return;
                }
                run_worker(&runtime, active, receiver);
            })
            .map_err(|error| LanguageHostError::Provider(error.to_string()))?;
        initialized_rx
            .recv()
            .map_err(|_| LanguageHostError::WorkerStopped)??;
        Ok(Self {
            sender,
            thread: Mutex::new(Some(handle)),
        })
    }

    pub(super) async fn open_document(
        &self,
        context: LanguageRequestContext,
        document: DocumentSnapshot,
    ) -> Result<(), LanguageHostError> {
        ensure_not_cancelled(&context)?;
        let (response, receiver) = oneshot::channel();
        self.send(WorkerRequest::Open {
            context,
            document,
            response,
        })?;
        receiver
            .await
            .map_err(|_| LanguageHostError::WorkerStopped)?
    }

    pub(super) async fn change_document(
        &self,
        context: LanguageRequestContext,
        change: DocumentChange,
    ) -> Result<(), LanguageHostError> {
        ensure_not_cancelled(&context)?;
        let (response, receiver) = oneshot::channel();
        self.send(WorkerRequest::Change {
            context,
            change,
            response,
        })?;
        receiver
            .await
            .map_err(|_| LanguageHostError::WorkerStopped)?
    }

    /// Enfileira a mudança e devolve por onde vem a resposta, sem esperar.
    ///
    /// O provider já roda em thread própria; era a **espera** que punha a
    /// análise no meio da digitação. Quem posta segue com a tecla e recolhe o
    /// resultado depois.
    pub(super) fn post_change(
        &self,
        context: LanguageRequestContext,
        change: DocumentChange,
    ) -> Result<oneshot::Receiver<Result<(), LanguageHostError>>, LanguageHostError> {
        ensure_not_cancelled(&context)?;
        let (response, receiver) = oneshot::channel();
        self.send(WorkerRequest::Change {
            context,
            change,
            response,
        })?;
        Ok(receiver)
    }

    /// Pede o realce e devolve por onde ele vem. O espelho de [`Self::post_change`].
    ///
    /// A fila do worker é ordenada, então o realce pedido depois de uma mudança
    /// fala do texto **com** ela aplicada.
    pub(super) fn post_syntax(
        &self,
        context: LanguageRequestContext,
        document_id: DocumentId,
    ) -> Result<oneshot::Receiver<Result<SyntaxSnapshot, LanguageHostError>>, LanguageHostError> {
        ensure_not_cancelled(&context)?;
        let (response, receiver) = oneshot::channel();
        self.send(WorkerRequest::Syntax {
            context,
            document_id,
            response,
        })?;
        Ok(receiver)
    }

    pub(super) async fn close_document(
        &self,
        context: LanguageRequestContext,
        document_id: DocumentId,
    ) -> Result<(), LanguageHostError> {
        ensure_not_cancelled(&context)?;
        let (response, receiver) = oneshot::channel();
        self.send(WorkerRequest::Close {
            context,
            document_id,
            response,
        })?;
        receiver
            .await
            .map_err(|_| LanguageHostError::WorkerStopped)?
    }

    pub(super) async fn diagnostics(
        &self,
        context: LanguageRequestContext,
        document_id: DocumentId,
    ) -> Result<Vec<Diagnostic>, LanguageHostError> {
        ensure_not_cancelled(&context)?;
        let (response, receiver) = oneshot::channel();
        self.send(WorkerRequest::Diagnostics {
            context,
            document_id,
            response,
        })?;
        receiver
            .await
            .map_err(|_| LanguageHostError::WorkerStopped)?
    }

    pub(super) async fn syntax(
        &self,
        context: LanguageRequestContext,
        document_id: DocumentId,
    ) -> Result<SyntaxSnapshot, LanguageHostError> {
        ensure_not_cancelled(&context)?;
        let (response, receiver) = oneshot::channel();
        self.send(WorkerRequest::Syntax {
            context,
            document_id,
            response,
        })?;
        receiver
            .await
            .map_err(|_| LanguageHostError::WorkerStopped)?
    }

    pub(super) async fn semantic(
        &self,
        context: LanguageRequestContext,
        document_id: DocumentId,
    ) -> Result<SemanticSnapshot, LanguageHostError> {
        ensure_not_cancelled(&context)?;
        let (response, receiver) = oneshot::channel();
        self.send(WorkerRequest::Semantic {
            context,
            document_id,
            response,
        })?;
        receiver
            .await
            .map_err(|_| LanguageHostError::WorkerStopped)?
    }

    pub(super) async fn completion(
        &self,
        context: LanguageRequestContext,
        request: CompletionRequest,
    ) -> Result<Vec<CompletionItem>, LanguageHostError> {
        ensure_not_cancelled(&context)?;
        let (response, receiver) = oneshot::channel();
        self.send(WorkerRequest::Completion {
            context,
            request,
            response,
        })?;
        receiver
            .await
            .map_err(|_| LanguageHostError::WorkerStopped)?
    }

    pub(super) async fn member_access(
        &self,
        context: LanguageRequestContext,
        text: String,
        offset: usize,
    ) -> Result<Option<MemberAccess>, LanguageHostError> {
        ensure_not_cancelled(&context)?;
        let (response, receiver) = oneshot::channel();
        self.send(WorkerRequest::MemberAccess {
            context,
            text,
            offset,
            response,
        })?;
        receiver
            .await
            .map_err(|_| LanguageHostError::WorkerStopped)?
    }

    pub(super) async fn type_members(
        &self,
        context: LanguageRequestContext,
        type_name: String,
        prefix: String,
    ) -> Result<Vec<CompletionItem>, LanguageHostError> {
        ensure_not_cancelled(&context)?;
        let (response, receiver) = oneshot::channel();
        self.send(WorkerRequest::TypeMembers {
            context,
            type_name,
            prefix,
            response,
        })?;
        receiver
            .await
            .map_err(|_| LanguageHostError::WorkerStopped)?
    }

    pub(super) async fn accessor_plan(
        &self,
        context: LanguageRequestContext,
        document_id: DocumentId,
        position: TextPosition,
        kind: AccessorKind,
    ) -> Result<AccessorPlan, LanguageHostError> {
        ensure_not_cancelled(&context)?;
        let (response, receiver) = oneshot::channel();
        self.send(WorkerRequest::AccessorPlanFor {
            context,
            document_id,
            position,
            kind,
            response,
        })?;
        receiver
            .await
            .map_err(|_| LanguageHostError::WorkerStopped)?
    }

    pub(super) async fn references_to_name(
        &self,
        context: LanguageRequestContext,
        name: String,
    ) -> Result<Vec<Location>, LanguageHostError> {
        ensure_not_cancelled(&context)?;
        let (response, receiver) = oneshot::channel();
        self.send(WorkerRequest::ReferencesToName {
            context,
            name,
            response,
        })?;
        receiver
            .await
            .map_err(|_| LanguageHostError::WorkerStopped)?
    }

    pub(super) async fn constructor_source(
        &self,
        context: LanguageRequestContext,
        document_id: DocumentId,
        position: TextPosition,
        fields: Vec<String>,
    ) -> Result<Option<String>, LanguageHostError> {
        ensure_not_cancelled(&context)?;
        let (response, receiver) = oneshot::channel();
        self.send(WorkerRequest::ConstructorSource {
            context,
            document_id,
            position,
            fields,
            response,
        })?;
        receiver
            .await
            .map_err(|_| LanguageHostError::WorkerStopped)?
    }

    pub(super) async fn workspace_types(
        &self,
        context: LanguageRequestContext,
        query: String,
        limit: usize,
    ) -> Result<Vec<SemanticSymbol>, LanguageHostError> {
        ensure_not_cancelled(&context)?;
        let (response, receiver) = oneshot::channel();
        self.send(WorkerRequest::WorkspaceTypes {
            context,
            query,
            limit,
            response,
        })?;
        receiver
            .await
            .map_err(|_| LanguageHostError::WorkerStopped)?
    }

    pub(super) async fn definition(
        &self,
        context: LanguageRequestContext,
        request: DefinitionRequest,
    ) -> Result<Vec<Location>, LanguageHostError> {
        ensure_not_cancelled(&context)?;
        let (response, receiver) = oneshot::channel();
        self.send(WorkerRequest::Definition {
            context,
            request,
            response,
        })?;
        receiver
            .await
            .map_err(|_| LanguageHostError::WorkerStopped)?
    }

    pub(super) async fn references(
        &self,
        context: LanguageRequestContext,
        request: ReferencesRequest,
    ) -> Result<Vec<Location>, LanguageHostError> {
        ensure_not_cancelled(&context)?;
        let (response, receiver) = oneshot::channel();
        self.send(WorkerRequest::References {
            context,
            request,
            response,
        })?;
        receiver
            .await
            .map_err(|_| LanguageHostError::WorkerStopped)?
    }

    pub(super) async fn shutdown(&self) -> Result<(), LanguageHostError> {
        let (response, receiver) = oneshot::channel();
        self.send(WorkerRequest::Shutdown { response })?;
        let result = receiver
            .await
            .map_err(|_| LanguageHostError::WorkerStopped)?;
        let handle = self
            .thread
            .lock()
            .map_err(|_| LanguageHostError::WorkerStopped)?
            .take();
        if let Some(handle) = handle {
            handle
                .join()
                .map_err(|_| LanguageHostError::WorkerStopped)?;
        }
        result
    }

    fn send(&self, request: WorkerRequest) -> Result<(), LanguageHostError> {
        self.sender.try_send(request).map_err(|error| match error {
            TrySendError::Full(_) => LanguageHostError::Backpressure,
            TrySendError::Disconnected(_) => LanguageHostError::WorkerStopped,
        })
    }
}

fn run_worker(
    runtime: &tokio::runtime::Runtime,
    active: Box<dyn ActiveLanguage>,
    receiver: Receiver<WorkerRequest>,
) {
    while let Ok(request) = receiver.recv() {
        match request {
            WorkerRequest::Open {
                context,
                document,
                response,
            } => {
                let result = if context.cancellation.is_cancelled() {
                    Err(LanguageHostError::Cancelled)
                } else {
                    runtime
                        .block_on(active.open_document(document))
                        .map_err(Into::into)
                };
                let _ = response.send(result);
            }
            WorkerRequest::Change {
                context,
                change,
                response,
            } => {
                let result = if context.cancellation.is_cancelled() {
                    Err(LanguageHostError::Cancelled)
                } else {
                    runtime
                        .block_on(active.change_document(change))
                        .map_err(Into::into)
                };
                let _ = response.send(result);
            }
            WorkerRequest::Close {
                context,
                document_id,
                response,
            } => {
                let result = if context.cancellation.is_cancelled() {
                    Err(LanguageHostError::Cancelled)
                } else {
                    runtime
                        .block_on(active.close_document(document_id))
                        .map_err(Into::into)
                };
                let _ = response.send(result);
            }
            WorkerRequest::Diagnostics {
                context,
                document_id,
                response,
            } => {
                let result = if context.cancellation.is_cancelled() {
                    Err(LanguageHostError::Cancelled)
                } else {
                    runtime
                        .block_on(active.diagnostics(document_id))
                        .map_err(Into::into)
                };
                let _ = response.send(result);
            }
            WorkerRequest::Syntax {
                context,
                document_id,
                response,
            } => {
                let result = if context.cancellation.is_cancelled() {
                    Err(LanguageHostError::Cancelled)
                } else {
                    runtime
                        .block_on(active.syntax(document_id))
                        .map_err(Into::into)
                };
                let _ = response.send(result);
            }
            WorkerRequest::Semantic {
                context,
                document_id,
                response,
            } => {
                let result = if context.cancellation.is_cancelled() {
                    Err(LanguageHostError::Cancelled)
                } else {
                    runtime
                        .block_on(active.semantic(document_id))
                        .map_err(Into::into)
                };
                let _ = response.send(result);
            }
            WorkerRequest::Completion {
                context,
                request,
                response,
            } => {
                let result = if context.cancellation.is_cancelled() {
                    Err(LanguageHostError::Cancelled)
                } else {
                    runtime
                        .block_on(active.completion(request))
                        .map_err(Into::into)
                };
                let _ = response.send(result);
            }
            WorkerRequest::MemberAccess {
                context,
                text,
                offset,
                response,
            } => {
                let result = if context.cancellation.is_cancelled() {
                    Err(LanguageHostError::Cancelled)
                } else {
                    runtime
                        .block_on(active.member_access(&text, offset))
                        .map_err(Into::into)
                };
                let _ = response.send(result);
            }
            WorkerRequest::TypeMembers {
                context,
                type_name,
                prefix,
                response,
            } => {
                let result = if context.cancellation.is_cancelled() {
                    Err(LanguageHostError::Cancelled)
                } else {
                    runtime
                        .block_on(active.type_members(&type_name, &prefix))
                        .map_err(Into::into)
                };
                let _ = response.send(result);
            }
            WorkerRequest::AccessorPlanFor {
                context,
                document_id,
                position,
                kind,
                response,
            } => {
                let result = if context.cancellation.is_cancelled() {
                    Err(LanguageHostError::Cancelled)
                } else {
                    runtime
                        .block_on(active.accessor_plan(document_id, position, kind))
                        .map_err(Into::into)
                };
                let _ = response.send(result);
            }
            WorkerRequest::ReferencesToName {
                context,
                name,
                response,
            } => {
                let result = if context.cancellation.is_cancelled() {
                    Err(LanguageHostError::Cancelled)
                } else {
                    runtime
                        .block_on(active.references_to_name(&name))
                        .map_err(Into::into)
                };
                let _ = response.send(result);
            }
            WorkerRequest::ConstructorSource {
                context,
                document_id,
                position,
                fields,
                response,
            } => {
                let result = if context.cancellation.is_cancelled() {
                    Err(LanguageHostError::Cancelled)
                } else {
                    runtime
                        .block_on(active.constructor_source(document_id, position, fields))
                        .map_err(Into::into)
                };
                let _ = response.send(result);
            }
            WorkerRequest::WorkspaceTypes {
                context,
                query,
                limit,
                response,
            } => {
                let result = if context.cancellation.is_cancelled() {
                    Err(LanguageHostError::Cancelled)
                } else {
                    runtime
                        .block_on(active.workspace_types(&query, limit))
                        .map_err(Into::into)
                };
                let _ = response.send(result);
            }
            WorkerRequest::Definition {
                context,
                request,
                response,
            } => {
                let result = if context.cancellation.is_cancelled() {
                    Err(LanguageHostError::Cancelled)
                } else {
                    runtime
                        .block_on(active.definition(request))
                        .map_err(Into::into)
                };
                let _ = response.send(result);
            }
            WorkerRequest::References {
                context,
                request,
                response,
            } => {
                let result = if context.cancellation.is_cancelled() {
                    Err(LanguageHostError::Cancelled)
                } else {
                    runtime
                        .block_on(active.references(request))
                        .map_err(Into::into)
                };
                let _ = response.send(result);
            }
            WorkerRequest::Shutdown { response } => {
                let result = runtime.block_on(active.shutdown()).map_err(Into::into);
                let _ = response.send(result);
                break;
            }
        }
    }
}
use std::{
    sync::{
        Arc, Mutex,
        mpsc::{Receiver, SyncSender, TrySendError, sync_channel},
    },
    thread::{self, JoinHandle},
};
