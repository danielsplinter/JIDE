#![doc = "Registro, ciclo de vida e isolamento dos providers de linguagem."]

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, RwLock,
        atomic::{AtomicU64, Ordering},
        mpsc::{Receiver, SyncSender, TrySendError, sync_channel},
    },
    thread::{self, JoinHandle},
};

use ide_domain::{
    CompletionItem, CompletionRequest, DefinitionRequest, Diagnostic, DocumentChange, DocumentId,
    DocumentSnapshot, LanguageId, Location, ProviderId, ReferencesRequest, RequestId,
    SemanticSnapshot, SemanticSymbol, SyntaxSnapshot,
};
use ide_language_api::{
    ActiveLanguage, LANGUAGE_API_VERSION, LanguageActivationContext, LanguageCapabilities,
    LanguageError, LanguageMetadata, LanguageProvider, LanguageRequestContext, ProviderState,
};
use thiserror::Error;
use tokio::sync::oneshot;

#[derive(Clone, Debug)]
pub struct LanguageHostConfig {
    pub worker_queue_capacity: usize,
    pub max_active_providers: usize,
}

impl Default for LanguageHostConfig {
    fn default() -> Self {
        Self {
            worker_queue_capacity: 64,
            max_active_providers: 8,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderSelection {
    pub primary: ProviderId,
    pub fallbacks: Vec<ProviderId>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderSnapshot {
    pub metadata: LanguageMetadata,
    pub capabilities: LanguageCapabilities,
    pub state: ProviderState,
    pub last_error: Option<String>,
}

#[derive(Debug, Error)]
pub enum LanguageHostError {
    #[error("provider {0:?} is already registered")]
    DuplicateProvider(ProviderId),
    #[error("provider {0:?} is not registered")]
    ProviderNotFound(ProviderId),
    #[error("provider {0:?} is disabled")]
    ProviderDisabled(ProviderId),
    #[error(
        "language API {actual_major}.{actual_minor} is incompatible with host {expected_major}.{expected_minor}"
    )]
    IncompatibleApi {
        expected_major: u16,
        expected_minor: u16,
        actual_major: u16,
        actual_minor: u16,
    },
    #[error("invalid provider metadata: {0}")]
    InvalidMetadata(String),
    #[error("no provider supports extension .{extension} with capabilities {capabilities:?}")]
    ProviderUnavailable {
        extension: String,
        capabilities: LanguageCapabilities,
    },
    #[error("provider {provider:?} does not expose capabilities {required:?}")]
    CapabilityUnavailable {
        provider: ProviderId,
        required: LanguageCapabilities,
    },
    #[error("active provider limit ({0}) reached")]
    ActiveProviderLimit(usize),
    #[error("language worker queue is full")]
    Backpressure,
    #[error("language request was cancelled")]
    Cancelled,
    #[error("language worker stopped unexpectedly")]
    WorkerStopped,
    #[error("provider failed: {0}")]
    Provider(String),
}

impl From<LanguageError> for LanguageHostError {
    fn from(error: LanguageError) -> Self {
        match error {
            LanguageError::Disabled => Self::Provider("provider is disabled".to_owned()),
            LanguageError::Cancelled => Self::Cancelled,
            LanguageError::Unsupported(operation) => {
                Self::Provider(format!("unsupported operation: {operation}"))
            }
            LanguageError::Provider(message) => Self::Provider(message),
        }
    }
}

struct ProviderEntry {
    provider: Arc<dyn LanguageProvider>,
    metadata: LanguageMetadata,
    capabilities: LanguageCapabilities,
    state: ProviderState,
    worker: Option<Arc<ProviderWorker>>,
    last_error: Option<String>,
}

#[derive(Default)]
struct Registry {
    providers: HashMap<ProviderId, ProviderEntry>,
    selections: HashMap<LanguageId, ProviderSelection>,
    document_routes: HashMap<DocumentId, ProviderId>,
}

pub struct LanguageHost {
    workspace_root: RwLock<PathBuf>,
    /// JDK escolhido na IDE, repassado a cada ativação.
    jdk_home: RwLock<Option<PathBuf>>,
    config: LanguageHostConfig,
    registry: Mutex<Registry>,
    next_request_id: AtomicU64,
}

impl LanguageHost {
    #[must_use]
    pub fn new(workspace_root: impl Into<PathBuf>) -> Self {
        Self::with_config(workspace_root, LanguageHostConfig::default())
    }

    #[must_use]
    pub fn with_config(workspace_root: impl Into<PathBuf>, config: LanguageHostConfig) -> Self {
        Self {
            workspace_root: RwLock::new(workspace_root.into()),
            jdk_home: RwLock::new(None),
            config: LanguageHostConfig {
                worker_queue_capacity: config.worker_queue_capacity.max(1),
                max_active_providers: config.max_active_providers.max(1),
            },
            registry: Mutex::new(Registry::default()),
            next_request_id: AtomicU64::new(1),
        }
    }

    pub fn set_workspace_root(&self, root: impl Into<PathBuf>) -> Result<(), LanguageHostError> {
        let mut workspace_root = self
            .workspace_root
            .write()
            .map_err(|_| LanguageHostError::WorkerStopped)?;
        *workspace_root = root.into();
        Ok(())
    }

    /// Registra o JDK escolhido na IDE. Devolve `true` quando ele mudou.
    ///
    /// Mudar não basta para os providers já ativos: eles indexaram a biblioteca
    /// padrão do JDK anterior. Quem troca é responsável por chamar
    /// [`LanguageHost::reactivate`] em seguida.
    pub fn set_jdk_home(&self, home: Option<PathBuf>) -> Result<bool, LanguageHostError> {
        let mut jdk_home = self
            .jdk_home
            .write()
            .map_err(|_| LanguageHostError::WorkerStopped)?;
        if *jdk_home == home {
            return Ok(false);
        }
        *jdk_home = home;
        Ok(true)
    }

    /// Desliga os providers ativos para que a próxima requisição os reativite.
    ///
    /// A ativação é preguiçosa, então derrubar o worker e voltar o estado para
    /// registrado é o bastante: a próxima requisição sobe um provider novo com o
    /// contexto atual. As rotas de documento ficam, mas os documentos abertos
    /// não — o provider novo nasce sem nenhum, e quem sincroniza precisa
    /// reabri-los.
    pub async fn reactivate(&self) -> Result<(), LanguageHostError> {
        let workers = {
            let mut registry = self
                .registry
                .lock()
                .map_err(|_| LanguageHostError::WorkerStopped)?;
            registry
                .providers
                .values_mut()
                .filter(|entry| entry.state == ProviderState::Active)
                .filter_map(|entry| {
                    entry.state = ProviderState::Registered;
                    entry.worker.take()
                })
                .collect::<Vec<_>>()
        };
        for worker in workers {
            worker.shutdown().await?;
        }
        Ok(())
    }

    pub fn register(&self, provider: Arc<dyn LanguageProvider>) -> Result<(), LanguageHostError> {
        let mut metadata = provider.metadata();
        validate_metadata(&metadata)?;
        metadata.extensions = metadata
            .extensions
            .iter()
            .map(|extension| normalize_extension(extension))
            .collect();
        metadata.extensions.sort();
        metadata.extensions.dedup();

        let mut registry = self
            .registry
            .lock()
            .map_err(|_| LanguageHostError::WorkerStopped)?;
        if registry.providers.contains_key(&metadata.provider_id) {
            return Err(LanguageHostError::DuplicateProvider(metadata.provider_id));
        }
        registry.providers.insert(
            metadata.provider_id.clone(),
            ProviderEntry {
                capabilities: provider.capabilities(),
                provider,
                metadata,
                state: ProviderState::Registered,
                worker: None,
                last_error: None,
            },
        );
        Ok(())
    }

    pub fn configure_selection(
        &self,
        language_id: LanguageId,
        selection: ProviderSelection,
    ) -> Result<(), LanguageHostError> {
        let mut registry = self
            .registry
            .lock()
            .map_err(|_| LanguageHostError::WorkerStopped)?;
        for provider_id in std::iter::once(&selection.primary).chain(&selection.fallbacks) {
            let entry = registry
                .providers
                .get(provider_id)
                .ok_or_else(|| LanguageHostError::ProviderNotFound(provider_id.clone()))?;
            if entry.metadata.language_id != language_id {
                return Err(LanguageHostError::InvalidMetadata(format!(
                    "provider {} belongs to language {}, not {}",
                    provider_id.0, entry.metadata.language_id.0, language_id.0
                )));
            }
        }
        registry.selections.insert(language_id, selection);
        Ok(())
    }

    pub fn providers(&self) -> Result<Vec<ProviderSnapshot>, LanguageHostError> {
        let registry = self
            .registry
            .lock()
            .map_err(|_| LanguageHostError::WorkerStopped)?;
        let mut providers = registry
            .providers
            .values()
            .map(|entry| ProviderSnapshot {
                metadata: entry.metadata.clone(),
                capabilities: entry.capabilities,
                state: entry.state,
                last_error: entry.last_error.clone(),
            })
            .collect::<Vec<_>>();
        providers.sort_by(|left, right| left.metadata.provider_id.cmp(&right.metadata.provider_id));
        Ok(providers)
    }

    pub fn provider_for_extension(
        &self,
        extension: &str,
        required: LanguageCapabilities,
    ) -> Result<ProviderId, LanguageHostError> {
        self.candidate_ids(extension, required)?
            .into_iter()
            .next()
            .ok_or_else(|| LanguageHostError::ProviderUnavailable {
                extension: normalize_extension(extension),
                capabilities: required,
            })
    }

    /// Caracteres que pedem completação sozinhos, para o documento aberto.
    ///
    /// Vêm do provider que atende o documento, não de uma lista do editor: cada
    /// linguagem tem a sua, e o editor não conhece sintaxe nenhuma.
    #[must_use]
    pub fn trigger_characters(&self, document_id: DocumentId) -> Vec<char> {
        let Ok(registry) = self.registry.lock() else {
            return Vec::new();
        };
        registry
            .document_routes
            .get(&document_id)
            .and_then(|provider_id| registry.providers.get(provider_id))
            .map(|entry| entry.metadata.trigger_characters.clone())
            .unwrap_or_default()
    }

    #[must_use]
    pub fn request_context(&self) -> LanguageRequestContext {
        LanguageRequestContext {
            request_id: RequestId(self.next_request_id.fetch_add(1, Ordering::Relaxed)),
            cancellation: Default::default(),
        }
    }

    pub async fn open_document(
        &self,
        context: LanguageRequestContext,
        document: DocumentSnapshot,
    ) -> Result<ProviderId, LanguageHostError> {
        ensure_not_cancelled(&context)?;
        let extension = document_extension(&document.path);
        let candidates = self.candidate_ids(&extension, LanguageCapabilities::empty())?;
        let mut last_error = None;
        for provider_id in candidates {
            match self.ensure_active(&provider_id) {
                Ok(worker) => match worker
                    .open_document(context.clone(), document.clone())
                    .await
                {
                    Ok(()) => {
                        self.registry
                            .lock()
                            .map_err(|_| LanguageHostError::WorkerStopped)?
                            .document_routes
                            .insert(document.id, provider_id.clone());
                        return Ok(provider_id);
                    }
                    Err(error) => last_error = Some(error),
                },
                Err(error) => last_error = Some(error),
            }
        }
        Err(
            last_error.unwrap_or(LanguageHostError::ProviderUnavailable {
                extension,
                capabilities: LanguageCapabilities::empty(),
            }),
        )
    }

    pub async fn change_document(
        &self,
        context: LanguageRequestContext,
        change: DocumentChange,
    ) -> Result<(), LanguageHostError> {
        let worker = self.worker_for_document(change.document_id, LanguageCapabilities::empty())?;
        worker.change_document(context, change).await
    }

    pub async fn close_document(
        &self,
        context: LanguageRequestContext,
        document_id: DocumentId,
    ) -> Result<(), LanguageHostError> {
        let worker = self.worker_for_document(document_id, LanguageCapabilities::empty())?;
        let result = worker.close_document(context, document_id).await;
        if result.is_ok() {
            self.registry
                .lock()
                .map_err(|_| LanguageHostError::WorkerStopped)?
                .document_routes
                .remove(&document_id);
        }
        result
    }

    pub async fn diagnostics(
        &self,
        context: LanguageRequestContext,
        document_id: DocumentId,
    ) -> Result<Vec<Diagnostic>, LanguageHostError> {
        let worker = self.worker_for_document(document_id, LanguageCapabilities::DIAGNOSTICS)?;
        worker.diagnostics(context, document_id).await
    }

    pub async fn syntax(
        &self,
        context: LanguageRequestContext,
        document_id: DocumentId,
    ) -> Result<SyntaxSnapshot, LanguageHostError> {
        let worker = self.worker_for_document(document_id, LanguageCapabilities::SYNTAX)?;
        worker.syntax(context, document_id).await
    }

    pub async fn semantic(
        &self,
        context: LanguageRequestContext,
        document_id: DocumentId,
    ) -> Result<SemanticSnapshot, LanguageHostError> {
        let worker = self.worker_for_document(document_id, LanguageCapabilities::SEMANTICS)?;
        worker.semantic(context, document_id).await
    }

    pub async fn completion(
        &self,
        context: LanguageRequestContext,
        request: CompletionRequest,
    ) -> Result<Vec<CompletionItem>, LanguageHostError> {
        let worker =
            self.worker_for_document(request.document_id, LanguageCapabilities::COMPLETION)?;
        worker.completion(context, request).await
    }

    /// Membros de um tipo, para telas que não têm documento.
    ///
    /// O roteamento é pelo documento aberto porque é ele que diz de qual
    /// linguagem se está falando — a pergunta não tem posição, mas continua sendo
    /// sobre um projeto de uma linguagem só.
    pub async fn type_members(
        &self,
        context: LanguageRequestContext,
        document_id: DocumentId,
        type_name: String,
        prefix: String,
    ) -> Result<Vec<CompletionItem>, LanguageHostError> {
        let worker = self.worker_for_document(document_id, LanguageCapabilities::COMPLETION)?;
        worker.type_members(context, type_name, prefix).await
    }

    /// Tipos do projeto que casam com a consulta, para a busca por nome.
    pub async fn workspace_types(
        &self,
        context: LanguageRequestContext,
        extension: &str,
        query: String,
        limit: usize,
    ) -> Result<Vec<SemanticSymbol>, LanguageHostError> {
        let provider_id =
            self.provider_for_extension(extension, LanguageCapabilities::COMPLETION)?;
        let worker = self.ensure_active(&provider_id)?;
        worker.workspace_types(context, query, limit).await
    }

    pub async fn definition(
        &self,
        context: LanguageRequestContext,
        request: DefinitionRequest,
    ) -> Result<Vec<Location>, LanguageHostError> {
        let worker =
            self.worker_for_document(request.document_id, LanguageCapabilities::DEFINITION)?;
        worker.definition(context, request).await
    }

    pub async fn references(
        &self,
        context: LanguageRequestContext,
        request: ReferencesRequest,
    ) -> Result<Vec<Location>, LanguageHostError> {
        let worker =
            self.worker_for_document(request.document_id, LanguageCapabilities::REFERENCES)?;
        worker.references(context, request).await
    }

    pub async fn disable(&self, provider_id: &ProviderId) -> Result<(), LanguageHostError> {
        let worker = {
            let mut registry = self
                .registry
                .lock()
                .map_err(|_| LanguageHostError::WorkerStopped)?;
            let entry = registry
                .providers
                .get_mut(provider_id)
                .ok_or_else(|| LanguageHostError::ProviderNotFound(provider_id.clone()))?;
            entry.state = ProviderState::ShuttingDown;
            entry.worker.take()
        };
        if let Some(worker) = worker {
            worker.shutdown().await?;
        }
        let mut registry = self
            .registry
            .lock()
            .map_err(|_| LanguageHostError::WorkerStopped)?;
        if let Some(entry) = registry.providers.get_mut(provider_id) {
            entry.state = ProviderState::Disabled;
        }
        registry
            .document_routes
            .retain(|_, routed_provider| routed_provider != provider_id);
        Ok(())
    }

    pub fn enable(&self, provider_id: &ProviderId) -> Result<(), LanguageHostError> {
        let mut registry = self
            .registry
            .lock()
            .map_err(|_| LanguageHostError::WorkerStopped)?;
        let entry = registry
            .providers
            .get_mut(provider_id)
            .ok_or_else(|| LanguageHostError::ProviderNotFound(provider_id.clone()))?;
        if entry.state == ProviderState::Disabled || entry.state == ProviderState::Failed {
            entry.state = ProviderState::Registered;
            entry.last_error = None;
        }
        Ok(())
    }

    pub async fn shutdown(&self) -> Result<(), LanguageHostError> {
        let ids = {
            let registry = self
                .registry
                .lock()
                .map_err(|_| LanguageHostError::WorkerStopped)?;
            registry
                .providers
                .iter()
                .filter(|(_, entry)| entry.worker.is_some())
                .map(|(id, _)| id.clone())
                .collect::<Vec<_>>()
        };
        for id in ids {
            self.disable(&id).await?;
        }
        Ok(())
    }

    fn candidate_ids(
        &self,
        extension: &str,
        required: LanguageCapabilities,
    ) -> Result<Vec<ProviderId>, LanguageHostError> {
        let extension = normalize_extension(extension);
        let registry = self
            .registry
            .lock()
            .map_err(|_| LanguageHostError::WorkerStopped)?;
        let mut matching = registry
            .providers
            .iter()
            .filter(|(_, entry)| {
                entry.state != ProviderState::Disabled
                    && entry.metadata.extensions.contains(&extension)
                    && entry.capabilities.contains(required)
            })
            .map(|(id, entry)| (id.clone(), entry.metadata.language_id.clone()))
            .collect::<Vec<_>>();
        matching.sort_by(|left, right| left.0.cmp(&right.0));
        if matching.is_empty() {
            return Err(LanguageHostError::ProviderUnavailable {
                extension,
                capabilities: required,
            });
        }

        let language_id = matching[0].1.clone();
        let mut ordered = Vec::new();
        if let Some(selection) = registry.selections.get(&language_id) {
            for id in std::iter::once(&selection.primary).chain(&selection.fallbacks) {
                if matching.iter().any(|candidate| &candidate.0 == id) {
                    ordered.push(id.clone());
                }
            }
        }
        for (id, _) in matching {
            if !ordered.contains(&id) {
                ordered.push(id);
            }
        }
        Ok(ordered)
    }

    fn ensure_active(
        &self,
        provider_id: &ProviderId,
    ) -> Result<Arc<ProviderWorker>, LanguageHostError> {
        let (provider, metadata, workspace_root) = {
            let mut registry = self
                .registry
                .lock()
                .map_err(|_| LanguageHostError::WorkerStopped)?;
            let active_count = registry
                .providers
                .values()
                .filter(|entry| entry.worker.is_some())
                .count();
            let entry = registry
                .providers
                .get_mut(provider_id)
                .ok_or_else(|| LanguageHostError::ProviderNotFound(provider_id.clone()))?;
            if entry.state == ProviderState::Disabled {
                return Err(LanguageHostError::ProviderDisabled(provider_id.clone()));
            }
            if let Some(worker) = &entry.worker {
                return Ok(Arc::clone(worker));
            }
            if active_count >= self.config.max_active_providers {
                return Err(LanguageHostError::ActiveProviderLimit(
                    self.config.max_active_providers,
                ));
            }
            entry.state = ProviderState::Activating;
            let root = self
                .workspace_root
                .read()
                .map_err(|_| LanguageHostError::WorkerStopped)?
                .clone();
            (Arc::clone(&entry.provider), entry.metadata.clone(), root)
        };
        let jdk_home = self
            .jdk_home
            .read()
            .map_err(|_| LanguageHostError::WorkerStopped)?
            .clone();

        let worker = ProviderWorker::spawn(
            provider,
            metadata.clone(),
            LanguageActivationContext {
                workspace_root,
                jdk_home,
            },
            self.config.worker_queue_capacity,
        );
        let mut registry = self
            .registry
            .lock()
            .map_err(|_| LanguageHostError::WorkerStopped)?;
        let entry = registry
            .providers
            .get_mut(provider_id)
            .ok_or_else(|| LanguageHostError::ProviderNotFound(provider_id.clone()))?;
        match worker {
            Ok(worker) => {
                let worker = Arc::new(worker);
                entry.state = ProviderState::Active;
                entry.worker = Some(Arc::clone(&worker));
                entry.last_error = None;
                Ok(worker)
            }
            Err(error) => {
                entry.state = ProviderState::Failed;
                entry.last_error = Some(error.to_string());
                Err(error)
            }
        }
    }

    fn worker_for_document(
        &self,
        document_id: DocumentId,
        required: LanguageCapabilities,
    ) -> Result<Arc<ProviderWorker>, LanguageHostError> {
        let provider_id = {
            let registry = self
                .registry
                .lock()
                .map_err(|_| LanguageHostError::WorkerStopped)?;
            registry
                .document_routes
                .get(&document_id)
                .cloned()
                .ok_or_else(|| {
                    LanguageHostError::Provider("document is not open in a provider".to_owned())
                })?
        };
        {
            let registry = self
                .registry
                .lock()
                .map_err(|_| LanguageHostError::WorkerStopped)?;
            let entry = registry
                .providers
                .get(&provider_id)
                .ok_or_else(|| LanguageHostError::ProviderNotFound(provider_id.clone()))?;
            if !entry.capabilities.contains(required) {
                return Err(LanguageHostError::CapabilityUnavailable {
                    provider: provider_id,
                    required,
                });
            }
        }
        self.ensure_active(&provider_id)
    }
}

fn validate_metadata(metadata: &LanguageMetadata) -> Result<(), LanguageHostError> {
    if metadata.api_version.major != LANGUAGE_API_VERSION.major {
        return Err(LanguageHostError::IncompatibleApi {
            expected_major: LANGUAGE_API_VERSION.major,
            expected_minor: LANGUAGE_API_VERSION.minor,
            actual_major: metadata.api_version.major,
            actual_minor: metadata.api_version.minor,
        });
    }
    if metadata.provider_id.0.trim().is_empty()
        || metadata.language_id.0.trim().is_empty()
        || metadata.display_name.trim().is_empty()
        || metadata.extensions.is_empty()
    {
        return Err(LanguageHostError::InvalidMetadata(
            "ids, display name and extensions are required".to_owned(),
        ));
    }
    Ok(())
}

fn normalize_extension(extension: &str) -> String {
    extension
        .trim()
        .trim_start_matches('.')
        .to_ascii_lowercase()
}

fn document_extension(path: &Path) -> String {
    path.extension()
        .and_then(|extension| extension.to_str())
        .map(normalize_extension)
        .unwrap_or_default()
}

fn ensure_not_cancelled(context: &LanguageRequestContext) -> Result<(), LanguageHostError> {
    if context.cancellation.is_cancelled() {
        Err(LanguageHostError::Cancelled)
    } else {
        Ok(())
    }
}

enum WorkerRequest {
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

struct ProviderWorker {
    sender: SyncSender<WorkerRequest>,
    thread: Mutex<Option<JoinHandle<()>>>,
}

impl ProviderWorker {
    fn spawn(
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

    async fn open_document(
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

    async fn change_document(
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

    async fn close_document(
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

    async fn diagnostics(
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

    async fn syntax(
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

    async fn semantic(
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

    async fn completion(
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

    async fn type_members(
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

    async fn workspace_types(
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

    async fn definition(
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

    async fn references(
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

    async fn shutdown(&self) -> Result<(), LanguageHostError> {
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

#[cfg(test)]
mod tests {
    use std::{
        fmt::Debug,
        path::PathBuf,
        sync::{
            Arc, Mutex,
            atomic::{AtomicUsize, Ordering},
        },
        thread::ThreadId,
    };

    use async_trait::async_trait;
    use ide_domain::{DocumentId, DocumentSnapshot, LanguageId, ProviderId, SemanticSymbol};
    use ide_language_api::{
        ActiveLanguage, ApiVersion, LANGUAGE_API_VERSION, LanguageActivationContext,
        LanguageCapabilities, LanguageError, LanguageMetadata, LanguageProvider, ProviderState,
    };

    use super::{LanguageHost, LanguageHostError, ProviderSelection};

    fn success<T, E: Debug>(result: Result<T, E>) -> T {
        match result {
            Ok(value) => value,
            Err(error) => panic!("expected success, got {error:?}"),
        }
    }

    struct TestProvider {
        provider_id: ProviderId,
        capabilities: LanguageCapabilities,
        activations: Arc<AtomicUsize>,
        shutdowns: Arc<AtomicUsize>,
        worker_thread: Arc<Mutex<Option<ThreadId>>>,
        fail_activation: bool,
    }

    impl TestProvider {
        fn new(provider_id: &str, capabilities: LanguageCapabilities) -> Self {
            Self {
                provider_id: ProviderId(provider_id.to_owned()),
                capabilities,
                activations: Arc::new(AtomicUsize::new(0)),
                shutdowns: Arc::new(AtomicUsize::new(0)),
                worker_thread: Arc::new(Mutex::new(None)),
                fail_activation: false,
            }
        }

        fn failing(provider_id: &str, capabilities: LanguageCapabilities) -> Self {
            Self {
                fail_activation: true,
                ..Self::new(provider_id, capabilities)
            }
        }
    }

    #[async_trait]
    impl LanguageProvider for TestProvider {
        fn metadata(&self) -> LanguageMetadata {
            LanguageMetadata {
                language_id: LanguageId("java".to_owned()),
                provider_id: self.provider_id.clone(),
                display_name: self.provider_id.0.clone(),
                extensions: vec![".JAVA".to_owned()],
                api_version: LANGUAGE_API_VERSION,
                trigger_characters: vec!['.'],
            }
        }

        fn capabilities(&self) -> LanguageCapabilities {
            self.capabilities
        }

        async fn activate(
            &self,
            _context: LanguageActivationContext,
        ) -> Result<Box<dyn ActiveLanguage>, LanguageError> {
            self.activations.fetch_add(1, Ordering::Relaxed);
            if let Ok(mut worker_thread) = self.worker_thread.lock() {
                *worker_thread = Some(std::thread::current().id());
            }
            if self.fail_activation {
                return Err(LanguageError::Provider("activation failed".to_owned()));
            }
            Ok(Box::new(TestActiveLanguage {
                language_id: LanguageId("java".to_owned()),
                shutdowns: Arc::clone(&self.shutdowns),
            }))
        }
    }

    struct TestActiveLanguage {
        language_id: LanguageId,
        shutdowns: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl ActiveLanguage for TestActiveLanguage {
        fn language_id(&self) -> &LanguageId {
            &self.language_id
        }

        async fn workspace_types(
            &self,
            _query: &str,
            _limit: usize,
        ) -> Result<Vec<SemanticSymbol>, LanguageError> {
            Ok(Vec::new())
        }

        async fn open_document(&self, _document: DocumentSnapshot) -> Result<(), LanguageError> {
            Ok(())
        }

        async fn change_document(
            &self,
            _change: ide_domain::DocumentChange,
        ) -> Result<(), LanguageError> {
            Ok(())
        }

        async fn close_document(&self, _document_id: DocumentId) -> Result<(), LanguageError> {
            Ok(())
        }

        async fn diagnostics(
            &self,
            _document_id: DocumentId,
        ) -> Result<Vec<ide_domain::Diagnostic>, LanguageError> {
            Ok(Vec::new())
        }

        async fn shutdown(&self) -> Result<(), LanguageError> {
            self.shutdowns.fetch_add(1, Ordering::Relaxed);
            Ok(())
        }
    }

    fn document(id: u64) -> DocumentSnapshot {
        DocumentSnapshot {
            id: DocumentId(id),
            path: PathBuf::from("src/Main.JAVA"),
            version: 1,
            text: "class Main {}".to_owned(),
        }
    }

    #[test]
    fn registration_normalizes_extensions_and_rejects_duplicates() {
        let host = LanguageHost::new(".");
        let provider = Arc::new(TestProvider::new(
            "java.syntax",
            LanguageCapabilities::SYNTAX,
        ));
        success(host.register(provider.clone()));
        assert_eq!(
            success(host.provider_for_extension("java", LanguageCapabilities::SYNTAX)),
            ProviderId("java.syntax".to_owned())
        );
        assert!(matches!(
            host.register(provider),
            Err(LanguageHostError::DuplicateProvider(_))
        ));
    }

    #[test]
    fn workspace_types_activate_by_extension_without_an_open_document() {
        let host = LanguageHost::new(".");
        success(host.register(Arc::new(TestProvider::new(
            "java.types",
            LanguageCapabilities::COMPLETION,
        ))));
        let found = pollster::block_on(host.workspace_types(
            host.request_context(),
            "java",
            String::new(),
            100,
        ));
        assert!(
            matches!(found, Ok(ref types) if types.is_empty()),
            "a busca do workspace não pode depender de uma rota de documento: {found:?}"
        );
    }

    #[test]
    fn incompatible_major_version_is_rejected() {
        struct IncompatibleProvider;

        #[async_trait]
        impl LanguageProvider for IncompatibleProvider {
            fn metadata(&self) -> LanguageMetadata {
                LanguageMetadata {
                    language_id: LanguageId("java".to_owned()),
                    provider_id: ProviderId("incompatible".to_owned()),
                    display_name: "Incompatible".to_owned(),
                    extensions: vec!["java".to_owned()],
                    api_version: ApiVersion {
                        major: LANGUAGE_API_VERSION.major + 1,
                        minor: 0,
                    },
                    trigger_characters: Vec::new(),
                }
            }

            fn capabilities(&self) -> LanguageCapabilities {
                LanguageCapabilities::SYNTAX
            }

            async fn activate(
                &self,
                _context: LanguageActivationContext,
            ) -> Result<Box<dyn ActiveLanguage>, LanguageError> {
                Err(LanguageError::Provider("must not activate".to_owned()))
            }
        }

        let host = LanguageHost::new(".");
        assert!(matches!(
            host.register(Arc::new(IncompatibleProvider)),
            Err(LanguageHostError::IncompatibleApi { .. })
        ));
    }

    #[test]
    fn configured_fallback_activates_when_primary_fails() {
        let host = LanguageHost::new(".");
        let primary = Arc::new(TestProvider::failing(
            "java.primary",
            LanguageCapabilities::SYNTAX | LanguageCapabilities::DIAGNOSTICS,
        ));
        let fallback = Arc::new(TestProvider::new(
            "java.fallback",
            LanguageCapabilities::SYNTAX | LanguageCapabilities::DIAGNOSTICS,
        ));
        success(host.register(primary.clone()));
        success(host.register(fallback.clone()));
        success(host.configure_selection(
            LanguageId("java".to_owned()),
            ProviderSelection {
                primary: ProviderId("java.primary".to_owned()),
                fallbacks: vec![ProviderId("java.fallback".to_owned())],
            },
        ));

        let selected = success(pollster::block_on(
            host.open_document(host.request_context(), document(1)),
        ));
        assert_eq!(selected, ProviderId("java.fallback".to_owned()));
        assert_eq!(primary.activations.load(Ordering::Relaxed), 1);
        assert_eq!(fallback.activations.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn activation_is_lazy_unique_and_runs_in_an_isolated_worker() {
        let caller_thread = std::thread::current().id();
        let host = LanguageHost::new(".");
        let provider = Arc::new(TestProvider::new(
            "java.worker",
            LanguageCapabilities::SYNTAX | LanguageCapabilities::DIAGNOSTICS,
        ));
        success(host.register(provider.clone()));
        assert_eq!(provider.activations.load(Ordering::Relaxed), 0);

        success(pollster::block_on(
            host.open_document(host.request_context(), document(1)),
        ));
        success(pollster::block_on(
            host.open_document(host.request_context(), document(2)),
        ));
        assert_eq!(provider.activations.load(Ordering::Relaxed), 1);
        let worker_thread = match provider.worker_thread.lock() {
            Ok(thread) => *thread,
            Err(error) => panic!("worker thread lock failed: {error}"),
        };
        assert!(worker_thread.is_some_and(|thread| thread != caller_thread));
    }

    /// Trocar o JDK derruba o provider ativo para que ele reindexe.
    ///
    /// O provider indexa a biblioteca padrão na ativação, então continuar com o
    /// worker antigo significaria responder pelo JDK que o usuário acabou de
    /// abandonar. Registrar o mesmo caminho de novo não derruba nada.
    #[test]
    fn changing_the_jdk_brings_active_providers_down_to_reindex() {
        let host = LanguageHost::new(".");
        let provider = Arc::new(TestProvider::new(
            "java.jdk",
            LanguageCapabilities::SYNTAX | LanguageCapabilities::COMPLETION,
        ));
        success(host.register(provider.clone()));
        assert!(success(host.set_jdk_home(Some(PathBuf::from("jdk-17")))));
        // O mesmo JDK não é uma troca.
        assert!(!success(host.set_jdk_home(Some(PathBuf::from("jdk-17")))));

        let document = DocumentSnapshot {
            id: DocumentId(1),
            path: "A.java".into(),
            version: 1,
            text: "class A {}".to_owned(),
        };
        success(pollster::block_on(
            host.open_document(host.request_context(), document),
        ));
        assert_eq!(provider.activations.load(Ordering::Relaxed), 1);

        assert!(success(host.set_jdk_home(Some(PathBuf::from("jdk-21")))));
        success(pollster::block_on(host.reactivate()));
        // A rota do documento fica: a próxima requisição sobe um provider novo.
        // O resultado da operação não interessa aqui — este provider de teste não
        // implementa `syntax`. O que se verifica é a reativação.
        let _ = pollster::block_on(host.syntax(host.request_context(), DocumentId(1)));
        assert_eq!(provider.activations.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn cancellation_prevents_dispatch_and_disable_shuts_worker_down() {
        let host = LanguageHost::new(".");
        let provider = Arc::new(TestProvider::new(
            "java.lifecycle",
            LanguageCapabilities::SYNTAX,
        ));
        success(host.register(provider.clone()));
        let context = host.request_context();
        context.cancellation.cancel();
        assert!(matches!(
            pollster::block_on(host.open_document(context, document(1))),
            Err(LanguageHostError::Cancelled)
        ));
        assert_eq!(provider.activations.load(Ordering::Relaxed), 0);

        success(pollster::block_on(
            host.open_document(host.request_context(), document(2)),
        ));
        success(pollster::block_on(
            host.disable(&ProviderId("java.lifecycle".to_owned())),
        ));
        assert_eq!(provider.shutdowns.load(Ordering::Relaxed), 1);
        let snapshots = success(host.providers());
        assert_eq!(snapshots[0].state, ProviderState::Disabled);
    }
}
