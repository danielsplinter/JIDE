use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        Arc, Mutex, RwLock,
        atomic::{AtomicU64, Ordering},
    },
};

use crate::registry::Registry;
use crate::routing::{
    ProviderSelection, document_extension, ensure_not_cancelled, normalize_extension,
};
use crate::worker::ProviderWorker;
use ide_domain::{
    AccessorKind, AccessorPlan, CompletionItem, CompletionRequest, DefinitionRequest, Diagnostic,
    DocumentChange, DocumentId, DocumentSnapshot, LanguageId, Location, ProviderId,
    ReferencesRequest, RequestId, SemanticSnapshot, SemanticSymbol, SyntaxSnapshot, TextPosition,
};
use ide_language_api::LanguageToolchainConfig;
use ide_language_api::{
    LanguageActivationContext, LanguageCapabilities, LanguageError, LanguageMetadata,
    LanguageProvider, LanguageRequestContext, MemberAccess, ProviderState,
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
    /// O provider deixou de poder responder, e o documento perdeu a rota.
    ///
    /// Quem chama trata reabrindo o documento: a próxima abertura escolhe o
    /// candidato seguinte, que é o provider nativo. Ver a fase 3b da `23`.
    #[error("provider is no longer available: {0}")]
    ProviderGone(String),
    /// O provider está inteiro e não sabe responder **esta** pergunta.
    ///
    /// Diferente de `ProviderGone`, que demite quem a devolveu. Aqui a resposta
    /// é procurar quem saiba. Ver a fase 5 da `25`.
    #[error("no provider could answer: {0}")]
    Unresolved(String),
    /// O documento não está aberto em provider nenhum.
    ///
    /// Variante própria, e não texto dentro de `Provider`, porque quem chama
    /// **age diferente**: aqui ele reabre; num erro de pedido, ele tenta de
    /// novo do mesmo ponto.
    #[error("document {0:?} is not open in any provider")]
    DocumentNotRouted(DocumentId),
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
            LanguageError::Unresolved(message) => Self::Unresolved(message),
            LanguageError::Unavailable(message) => Self::ProviderGone(message),
        }
    }
}

pub struct LanguageHost {
    workspace_root: RwLock<PathBuf>,
    /// JDK escolhido na IDE, repassado a cada ativação.
    source_roots: RwLock<Vec<PathBuf>>,
    toolchains: RwLock<HashMap<LanguageId, LanguageToolchainConfig>>,
    config: LanguageHostConfig,
    registry: Mutex<Registry>,
    next_request_id: AtomicU64,
    /// Quem a próxima pergunta sem resposta mandou acordar.
    ///
    /// **Anotado, e não ativado aqui.** Subir um analisador externo é criar um
    /// processo, e criar processo no Windows com antivírus no caminho leva o que
    /// leva — feito na chamada, isso acontece na thread da interface, e a janela
    /// para. É o mesmo defeito que a busca textual e a busca por tipo já tiveram
    /// nesta IDE, pela mesma razão.
    ///
    /// Quem tem o laço de quadros drena isto e ativa fora dele. Ver a fase 5 da
    /// `25`.
    a_acordar: Mutex<Vec<ProviderId>>,
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
            source_roots: RwLock::new(Vec::new()),
            toolchains: RwLock::new(HashMap::new()),
            config: LanguageHostConfig {
                worker_queue_capacity: config.worker_queue_capacity.max(1),
                max_active_providers: config.max_active_providers.max(1),
            },
            registry: Mutex::new(Registry::default()),
            next_request_id: AtomicU64::new(1),
            a_acordar: Mutex::new(Vec::new()),
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
    pub fn set_toolchain(
        &self,
        language_id: LanguageId,
        toolchain: Option<LanguageToolchainConfig>,
    ) -> Result<bool, LanguageHostError> {
        let mut toolchains = self
            .toolchains
            .write()
            .map_err(|_| LanguageHostError::WorkerStopped)?;
        if let Some(config) = &toolchain
            && config.language_id != language_id
        {
            return Err(LanguageHostError::InvalidMetadata(
                "toolchain language does not match its registry key".to_owned(),
            ));
        }
        if toolchains.get(&language_id) == toolchain.as_ref() {
            return Ok(false);
        }
        match toolchain {
            Some(config) => {
                toolchains.insert(language_id, config);
            }
            None => {
                toolchains.remove(&language_id);
            }
        }
        Ok(true)
    }

    pub fn set_source_roots(&self, roots: Vec<PathBuf>) -> Result<bool, LanguageHostError> {
        let mut source_roots = self
            .source_roots
            .write()
            .map_err(|_| LanguageHostError::WorkerStopped)?;
        if *source_roots == roots {
            return Ok(false);
        }
        *source_roots = roots;
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
        let mut registry = self
            .registry
            .lock()
            .map_err(|_| LanguageHostError::WorkerStopped)?;
        registry.register(provider)
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
        registry.configure_selection(language_id, selection)
    }

    /// Se alguma linguagem ativa ainda está preparando o projeto.
    ///
    /// **A IDE não sabe o que está sendo preparado, e não precisa saber.** Uma
    /// entende que um analisador monta o projeto antes de responder; outra
    /// constrói um índice. Aqui as duas viram a mesma frase: ainda não dá para
    /// contar com a resposta completa.
    ///
    /// Ler isto não fala com a fila do worker — ele atende um pedido por vez, e a
    /// pergunta ficaria atrás justamente do trabalho sobre o qual se pergunta.
    /// Ver `ReadinessSignal`.
    #[must_use]
    pub fn preparing(&self) -> bool {
        self.registry.lock().is_ok_and(|registry| {
            registry
                .providers
                .values()
                .filter_map(|entry| entry.worker.as_ref())
                .any(|worker| worker.is_preparing())
        })
    }

    pub fn providers(&self) -> Result<Vec<ProviderSnapshot>, LanguageHostError> {
        let registry = self
            .registry
            .lock()
            .map_err(|_| LanguageHostError::WorkerStopped)?;
        Ok(registry.snapshots())
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
        registry.trigger_characters(document_id)
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
        let mut aceito = None;
        // O documento é aberto em todos os candidatos que **já estão de pé**, e
        // não em todos os registrados. Dois providers da mesma linguagem podem
        // ter capacidades complementares, e um deles sozinho não responde a tudo
        // — mas subir um analisador externo custa 1,9 GB e trinta segundos, e
        // pagá-los ao abrir o primeiro `.ts` é pagá-los mesmo quando ninguém vai
        // perguntar nada que exija tipos. Ver a fase 5 da `25`.
        //
        // Se nenhum estiver de pé, o primeiro candidato é ativado: o arquivo
        // precisa de alguém que lhe dê realce agora.
        //
        // Quem não subir — o analisador externo sem Node, por exemplo — apenas
        // fica de fora, e o que ele saberia responder passa a não ter resposta.
        // É a degradação da ADR-025 acontecendo por capacidade, e não por
        // documento inteiro.
        let (de_pe, capacidades): (
            std::collections::HashSet<ProviderId>,
            std::collections::HashMap<ProviderId, LanguageCapabilities>,
        ) = {
            let registry = self
                .registry
                .lock()
                .map_err(|_| LanguageHostError::WorkerStopped)?;
            (
                registry
                    .providers
                    .iter()
                    .filter(|(_, entry)| entry.worker.is_some())
                    .map(|(id, _)| id.clone())
                    .collect(),
                registry
                    .providers
                    .iter()
                    .map(|(id, entry)| (id.clone(), entry.capabilities))
                    .collect(),
            )
        };
        // O que os anteriores já cobrem. Um candidato entra se **acrescenta**
        // alguma capacidade — é a composição da `04`, e sem ela um provider com
        // capacidade exclusiva nunca seria ativado.
        let mut cobertas = LanguageCapabilities::empty();
        // `nenhum_subiu` vale até um deles **conseguir** subir, e não até o
        // primeiro ser tentado: um principal que falha na ativação precisa
        // deixar a vez para o seguinte, que é a degradação da ADR-025.
        let mut nenhum_subiu = true;
        for provider_id in candidates {
            let minhas = capacidades
                .get(&provider_id)
                .copied()
                .unwrap_or_else(LanguageCapabilities::empty);
            let acrescenta = !minhas.difference(cobertas).is_empty();
            // De pé já, ou acrescenta o que ninguém cobre, ou ninguém subiu
            // ainda e o arquivo precisa de alguém agora.
            if !de_pe.contains(&provider_id) && !acrescenta && !nenhum_subiu {
                continue;
            }
            match self.ensure_active(&provider_id) {
                Ok(worker) => match worker
                    .open_document(context.clone(), document.clone())
                    .await
                {
                    Ok(()) => {
                        nenhum_subiu = false;
                        cobertas |= minhas;
                        self.registry
                            .lock()
                            .map_err(|_| LanguageHostError::WorkerStopped)?
                            .route(document.id, provider_id.clone());
                        // O primeiro que aceita é o que se devolve: é o
                        // principal da ordem declarada.
                        aceito.get_or_insert(provider_id);
                    }
                    Err(error) => last_error = Some(error),
                },
                Err(error) => last_error = Some(error),
            }
        }
        aceito.ok_or_else(|| {
            last_error.unwrap_or(LanguageHostError::ProviderUnavailable {
                extension,
                capabilities: LanguageCapabilities::empty(),
            })
        })
    }

    /// Espera as linguagens ativas terminarem seus índices.
    ///
    /// Existe para quem precisa da resposta completa — testes, e um dia um
    /// indicador na barra de estado. O uso normal não espera.
    pub async fn wait_until_indexed(
        &self,
        timeout: std::time::Duration,
    ) -> Result<bool, LanguageHostError> {
        let workers: Vec<Arc<ProviderWorker>> = {
            let registry = self
                .registry
                .lock()
                .map_err(|_| LanguageHostError::WorkerStopped)?;
            registry
                .providers
                .values()
                .filter_map(|entry| entry.worker.clone())
                .collect()
        };
        let mut pronto = true;
        for worker in workers {
            pronto &= worker.wait_until_indexed(timeout).await?;
        }
        Ok(pronto)
    }

    /// Avisa as linguagens ativas que um arquivo mudou em disco.
    ///
    /// Vai a **todas**, e não à do documento: quem grava um `.java` pode estar
    /// com um `.xml` aberto, e cada linguagem decide se o arquivo lhe interessa
    /// — o padrão do contrato é ignorar.
    pub async fn file_changed(&self, path: &std::path::Path) -> Result<(), LanguageHostError> {
        let workers: Vec<Arc<ProviderWorker>> = {
            let registry = self
                .registry
                .lock()
                .map_err(|_| LanguageHostError::WorkerStopped)?;
            registry
                .providers
                .values()
                .filter_map(|entry| entry.worker.clone())
                .collect()
        };
        for worker in workers {
            worker.file_changed(path.to_path_buf()).await?;
        }
        Ok(())
    }

    pub async fn change_document(
        &self,
        context: LanguageRequestContext,
        change: DocumentChange,
    ) -> Result<(), LanguageHostError> {
        // A mudança vai a **todos** os que têm o documento: cada um mantém a
        // própria cópia do texto, e um que não a receba passaria a responder
        // sobre o arquivo de antes — a resposta velha com cara de certa.
        let mut ultimo = Ok(());
        for provider_id in self.providers_for_document(change.document_id)? {
            let Ok(worker) = self.ensure_active(&provider_id) else {
                continue;
            };
            let resultado = self.note_result(
                worker.provider_id(),
                worker.change_document(context.clone(), change.clone()).await,
            );
            if resultado.is_err() {
                ultimo = resultado;
            }
        }
        ultimo
    }

    fn providers_for_document(
        &self,
        document_id: DocumentId,
    ) -> Result<Vec<ProviderId>, LanguageHostError> {
        self.registry
            .lock()
            .map_err(|_| LanguageHostError::WorkerStopped)?
            .providers_for_document(document_id)
    }

    /// Enfileira a mudança sem esperar a resposta.
    ///
    /// É o que a digitação usa: o receptor devolvido diz depois se deu certo, e
    /// enquanto isso a janela não fica parada esperando a análise. Falhar aqui
    /// significa que a mudança **não** entrou na fila, e quem chamou não deve
    /// avançar o que considera já enviado.
    pub fn post_change_document(
        &self,
        context: LanguageRequestContext,
        change: DocumentChange,
    ) -> Result<oneshot::Receiver<Result<(), LanguageHostError>>, LanguageHostError> {
        // Todos recebem, e quem chamou fica com o receptor do primeiro: é o que
        // decide se a mudança entrou na fila, e a contrapressão de um vale para
        // a sincronização inteira (ADR-017).
        let mut primeiro = None;
        for provider_id in self.providers_for_document(change.document_id)? {
            let Ok(worker) = self.ensure_active(&provider_id) else {
                continue;
            };
            let receptor = worker.post_change(context.clone(), change.clone())?;
            primeiro.get_or_insert(receptor);
        }
        primeiro.ok_or(LanguageHostError::DocumentNotRouted(change.document_id))
    }

    /// Pede o realce sem esperar. Ver [`Self::post_change_document`].
    pub fn post_syntax(
        &self,
        context: LanguageRequestContext,
        document_id: DocumentId,
    ) -> Result<oneshot::Receiver<Result<SyntaxSnapshot, LanguageHostError>>, LanguageHostError>
    {
        let worker = self.worker_for_document(document_id, LanguageCapabilities::SYNTAX)?;
        worker.post_syntax(context, document_id)
    }

    pub async fn close_document(
        &self,
        context: LanguageRequestContext,
        document_id: DocumentId,
    ) -> Result<(), LanguageHostError> {
        // Fecha em todos os que o tinham aberto. Deixar um de fora manteria o
        // arquivo carregado num provider que ninguém mais consulta — e num
        // analisador externo isso é memória retida sem dono.
        let mut resultado = Ok(());
        for provider_id in self.providers_for_document(document_id)? {
            let Ok(worker) = self.ensure_active(&provider_id) else {
                continue;
            };
            let fechado = worker.close_document(context.clone(), document_id).await;
            if fechado.is_err() {
                resultado = fechado;
            }
        }
        self.registry
            .lock()
            .map_err(|_| LanguageHostError::WorkerStopped)?
            .unroute(document_id);
        resultado
    }

    pub async fn diagnostics(
        &self,
        context: LanguageRequestContext,
        document_id: DocumentId,
    ) -> Result<Vec<Diagnostic>, LanguageHostError> {
        let worker = self.worker_for_document(document_id, LanguageCapabilities::DIAGNOSTICS)?;
        self.note_result(worker.provider_id(), worker.diagnostics(context, document_id).await)
    }

    pub async fn syntax(
        &self,
        context: LanguageRequestContext,
        document_id: DocumentId,
    ) -> Result<SyntaxSnapshot, LanguageHostError> {
        let worker = self.worker_for_document(document_id, LanguageCapabilities::SYNTAX)?;
        self.note_result(worker.provider_id(), worker.syntax(context, document_id).await)
    }

    pub async fn semantic(
        &self,
        context: LanguageRequestContext,
        document_id: DocumentId,
    ) -> Result<SemanticSnapshot, LanguageHostError> {
        let worker = self.worker_for_document(document_id, LanguageCapabilities::SEMANTICS)?;
        self.note_result(worker.provider_id(), worker.semantic(context, document_id).await)
    }

    /// Completação, tentando os candidatos em ordem até alguém saber.
    ///
    /// # "Não sei" não é falha, e não demite ninguém
    ///
    /// O provider nativo de TypeScript sabe o tipo de um receptor declarado e
    /// não sabe o de `.pipe(map(x => x.`. Antes desta distinção, admitir esse
    /// limite o **derrubava**: `Unavailable` quer dizer "deixei de existir", e o
    /// host tirava suas rotas — o arquivo perdia realce por causa de uma
    /// completação que ninguém podia responder.
    ///
    /// Agora ele diz `Unresolved`, e a resposta do host é procurar quem saiba.
    /// Ver a fase 5 da `25`.
    pub async fn completion(
        &self,
        context: LanguageRequestContext,
        request: CompletionRequest,
    ) -> Result<Vec<CompletionItem>, LanguageHostError> {
        let candidatos =
            self.workers_for_document(request.document_id, LanguageCapabilities::COMPLETION)?;
        let mut ultimo = None;
        for worker in candidatos {
            let resposta = self.note_result(
                worker.provider_id(),
                worker.completion(context.clone(), request.clone()).await,
            );
            match resposta {
                Err(LanguageHostError::Unresolved(motivo)) => ultimo = Some(motivo),
                outro => return outro,
            }
        }
        // Ninguém que estava de pé soube. **Agora** vale acordar quem falta:
        // esta é a primeira pergunta da sessão que exige mais do que o índice
        // alcança, e é o momento em que o analisador externo passa a valer o que
        // custa.
        //
        // Ele sobe sem o texto de nada, e por isso esta pergunta ainda não é
        // respondida por ele: a aplicação reabre os documentos no quadro
        // seguinte. Num projeto grande o analisador leva trinta segundos para
        // montar o projeto de qualquer forma — um quadro a mais não é o que se
        // vai sentir.
        self.acordar_proximo(request.document_id, LanguageCapabilities::COMPLETION);
        Err(LanguageHostError::Unresolved(ultimo.unwrap_or_else(|| {
            "nenhum provider soube responder".to_owned()
        })))
    }

    pub async fn member_access(
        &self,
        context: LanguageRequestContext,
        document_id: DocumentId,
        text: String,
        offset: usize,
    ) -> Result<Option<MemberAccess>, LanguageHostError> {
        let worker = self.worker_for_document(document_id, LanguageCapabilities::COMPLETION)?;
        self.note_result(worker.provider_id(), worker.member_access(context, text, offset).await)
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
        self.note_result(worker.provider_id(), worker.type_members(context, type_name, prefix).await)
    }

    /// Acessores que faltam ao tipo sob a posição.
    pub async fn accessor_plan(
        &self,
        context: LanguageRequestContext,
        document_id: DocumentId,
        position: TextPosition,
        kind: AccessorKind,
    ) -> Result<AccessorPlan, LanguageHostError> {
        let worker = self.worker_for_document(document_id, LanguageCapabilities::COMPLETION)?;
        worker
            .accessor_plan(context, document_id, position, kind)
            .await
    }

    /// Onde um nome é referenciado no projeto, para renomear um arquivo.
    ///
    /// A rota é por extensão, e não por documento: o arquivo pode não estar
    /// aberto, que é o caso comum ao renomear pela árvore.
    pub async fn references_to_name(
        &self,
        context: LanguageRequestContext,
        extension: &str,
        name: String,
    ) -> Result<Vec<Location>, LanguageHostError> {
        let provider_id =
            self.provider_for_extension(extension, LanguageCapabilities::REFERENCES)?;
        let worker = self.ensure_active(&provider_id)?;
        self.note_result(worker.provider_id(), worker.references_to_name(context, name).await)
    }

    /// Construtor do tipo na posição, com os campos escolhidos.
    pub async fn constructor_source(
        &self,
        context: LanguageRequestContext,
        document_id: DocumentId,
        position: TextPosition,
        fields: Vec<String>,
    ) -> Result<Option<String>, LanguageHostError> {
        let worker = self.worker_for_document(document_id, LanguageCapabilities::COMPLETION)?;
        worker
            .constructor_source(context, document_id, position, fields)
            .await
    }

    /// Tipos do projeto que casam com a consulta, para a busca por nome.
    pub async fn workspace_types(
        &self,
        context: LanguageRequestContext,
        extension: &str,
        query: String,
        limit: usize,
    ) -> Result<Vec<SemanticSymbol>, LanguageHostError> {
        // Busca por nome é capacidade própria, e não um apêndice da completação:
        // um provider pode ter índice de nomes sem saber tipar uma expressão.
        let provider_id =
            self.provider_for_extension(extension, LanguageCapabilities::WORKSPACE_SYMBOLS)?;
        let worker = self.ensure_active(&provider_id)?;
        self.note_result(worker.provider_id(), worker.workspace_types(context, query, limit).await)
    }

    /// A definição, tentando os candidatos em ordem até alguém saber.
    ///
    /// O índice sabe navegar pelo `import` dentro do projeto e não alcança
    /// dependência instalada; quando ele diz que não sabe, a pergunta passa
    /// adiante. Ver a fase 5 da `25`.
    pub async fn definition(
        &self,
        context: LanguageRequestContext,
        request: DefinitionRequest,
    ) -> Result<Vec<Location>, LanguageHostError> {
        let candidatos =
            self.workers_for_document(request.document_id, LanguageCapabilities::DEFINITION)?;
        let mut ultimo = None;
        for worker in candidatos {
            let resposta = self.note_result(
                worker.provider_id(),
                worker.definition(context.clone(), request.clone()).await,
            );
            match resposta {
                Err(LanguageHostError::Unresolved(motivo)) => ultimo = Some(motivo),
                outro => return outro,
            }
        }
        self.acordar_proximo(request.document_id, LanguageCapabilities::DEFINITION);
        Err(LanguageHostError::Unresolved(ultimo.unwrap_or_else(|| {
            "nenhum provider soube responder".to_owned()
        })))
    }

    pub async fn references(
        &self,
        context: LanguageRequestContext,
        request: ReferencesRequest,
    ) -> Result<Vec<Location>, LanguageHostError> {
        let worker =
            self.worker_for_document(request.document_id, LanguageCapabilities::REFERENCES)?;
        self.note_result(worker.provider_id(), worker.references(context, request).await)
    }

    /// Tira um provider de serviço, sem removê-lo do registro.
    ///
    /// Ele continua listado — e é isso que permite a IDE dizer que **está
    /// desligado**, em vez de a pergunta simplesmente não achar nada. A diferença
    /// entre "não existe" e "existe e está fora" é o que separa uma resposta de um
    /// silêncio.
    ///
    /// Se ele estava ativo, o worker é encerrado: desligar sem parar o processo
    /// seria desligar só no papel, e o analisador externo continuaria com a
    /// memória dele.
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
        registry.remove_provider_routes(provider_id);
        Ok(())
    }

    /// Suspende os providers que não são usados há mais que o limite.
    ///
    /// Suspender é derrubar o worker e soltar o `ActiveLanguage` — é ali que o
    /// índice mora, e é por isso que a memória volta. O provider **continua
    /// candidato**: o próximo pedido o reativa sozinho, e a única coisa que se
    /// nota é essa primeira resposta ser mais lenta.
    ///
    /// **Só entra quem não tem documento aberto.** Um provider com aba aberta
    /// não está ocioso, por mais parado que esteja: a tecla seguinte custaria
    /// reindexar o projeto inteiro no meio da digitação, e o remédio seria pior.
    /// Na prática, o caso que isto resolve é o comum — abrir um `.ts` de manhã,
    /// fechá-lo, e passar o dia em Java com o índice do outro retido.
    ///
    /// Quem tem relógio é a aplicação; o host tem o estado. Por isso isto é
    /// chamado de fora, e não por um temporizador aqui dentro.
    ///
    /// Difere de `disable`: suspender é automático e reversível sem ninguém
    /// saber; desligar é decisão de quem usa, e tira a linguagem da tela.
    pub async fn suspend_idle(
        &self,
        idle_for: std::time::Duration,
    ) -> Result<Vec<ProviderId>, LanguageHostError> {
        let agora = std::time::Instant::now();
        let ociosos = {
            let mut registry = self
                .registry
                .lock()
                .map_err(|_| LanguageHostError::WorkerStopped)?;
            let com_documento: std::collections::HashSet<ProviderId> =
                registry.document_routes.values().flatten().cloned().collect();
            registry
                .providers
                .iter_mut()
                .filter(|(id, entry)| {
                    entry.worker.is_some()
                        && entry.state == ProviderState::Active
                        && !com_documento.contains(*id)
                        && agora.duration_since(entry.last_used) >= idle_for
                })
                .filter_map(|(id, entry)| {
                    entry.state = ProviderState::Suspended;
                    entry.worker.take().map(|worker| (id.clone(), worker))
                })
                .collect::<Vec<_>>()
        };
        let mut suspensos = Vec::new();
        for (id, worker) in ociosos {
            worker.shutdown().await?;
            suspensos.push(id);
        }
        Ok(suspensos)
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
        let registry = self
            .registry
            .lock()
            .map_err(|_| LanguageHostError::WorkerStopped)?;
        registry.candidate_ids(extension, required)
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
            entry.last_used = std::time::Instant::now();
            if let Some(worker) = &entry.worker {
                return Ok(Arc::clone(worker));
            }
            // **Já está subindo.** Sem esta guarda, cada pergunta feita enquanto
            // um analisador monta o projeto mandava subir **outro**: trinta
            // segundos de carga, 1,9 GB cada, e um clique a mais criava mais um.
            // A máquina engasgava, e o sintoma era a IDE travada.
            //
            // Subir é demorado e ninguém está esperando aqui: quem perguntou já
            // recebeu "não sei" e vai perguntar de novo.
            if entry.state == ProviderState::Activating {
                return Err(LanguageHostError::Unresolved(format!(
                    "{} ainda está subindo",
                    provider_id.0
                )));
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
        let source_roots = self
            .source_roots
            .read()
            .map_err(|_| LanguageHostError::WorkerStopped)?
            .clone();
        let toolchains = self
            .toolchains
            .read()
            .map_err(|_| LanguageHostError::WorkerStopped)?
            .values()
            .cloned()
            .collect();

        let worker = ProviderWorker::spawn(
            provider,
            metadata.clone(),
            LanguageActivationContext {
                workspace_root,
                source_roots,
                toolchains,
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

    /// Demite o provider que disse ter deixado de existir.
    ///
    /// Marca-o como falho e **desfaz as rotas** dos documentos que ele atendia.
    /// Não reabre nada: quem tem o texto é a aplicação, não o host. A próxima
    /// sincronização encontra o documento sem rota e o abre no candidato
    /// seguinte — que é o provider nativo, pela ordem declarada.
    ///
    /// O host guardar o texto para reabrir sozinho seria a alternativa, e
    /// duplicaria em memória o que o editor já tem. Ver a fase 3b da `23`.
    fn note_result<T>(
        &self,
        provider_id: &ProviderId,
        result: Result<T, LanguageHostError>,
    ) -> Result<T, LanguageHostError> {
        if matches!(result, Err(LanguageHostError::ProviderGone(_)))
            && let Ok(mut registry) = self.registry.lock()
        {
            if let Some(entry) = registry.providers.get_mut(provider_id) {
                entry.state = ProviderState::Failed;
                entry.worker = None;
            }
            registry.remove_provider_routes(provider_id);
        }
        result
    }

    /// Documentos que um provider já de pé ainda não recebeu.
    ///
    /// Um analisador que sobe **depois** da abertura não tem o texto de nada, e
    /// quem o tem é o editor. Esta lista é o que a aplicação usa para reabrir o
    /// que falta — o host não guarda texto, e guardá-lo duplicaria em memória o
    /// que já existe uma vez. Ver a fase 3b da `23` e a fase 5 da `25`.
    #[must_use]
    pub fn documents_missing_providers(&self) -> Vec<DocumentId> {
        let Ok(registry) = self.registry.lock() else {
            return Vec::new();
        };
        let de_pe: Vec<&ProviderId> = registry
            .providers
            .iter()
            .filter(|(_, entry)| entry.worker.is_some())
            .map(|(id, _)| id)
            .collect();
        registry
            .document_routes
            .iter()
            .filter(|(_, rotas)| {
                de_pe.iter().any(|id| {
                    // De pé, atende a mesma linguagem de quem já tem o
                    // documento, e não está entre eles.
                    !rotas.contains(id)
                        && rotas.iter().any(|rota| {
                            registry.providers.get(rota).map(|entry| &entry.metadata.language_id)
                                == registry.providers.get(*id).map(|entry| &entry.metadata.language_id)
                        })
                })
            })
            .map(|(document_id, _)| *document_id)
            .collect()
    }

    /// Todos os candidatos que podem responder, na ordem declarada.
    ///
    /// O primeiro é quem responde primeiro; os seguintes existem para quando ele
    /// disser que não sabe. Só entram os que **já têm o documento**: um provider
    /// que ainda não foi ativado não tem o texto, e perguntar a ele daria uma
    /// resposta sobre um arquivo que ele nunca viu.
    fn workers_for_document(
        &self,
        document_id: DocumentId,
        required: LanguageCapabilities,
    ) -> Result<Vec<Arc<ProviderWorker>>, LanguageHostError> {
        let ids = {
            let registry = self
                .registry
                .lock()
                .map_err(|_| LanguageHostError::WorkerStopped)?;
            registry.providers_for_capability(document_id, required)?
        };
        let workers: Vec<_> = ids
            .iter()
            .filter_map(|id| self.ensure_active(id).ok())
            .collect();
        if workers.is_empty() {
            return Err(LanguageHostError::DocumentNotRouted(document_id));
        }
        Ok(workers)
    }

    /// Ativa o primeiro candidato ainda parado que tenha a capacidade.
    ///
    /// Chamado quando todos os que estavam de pé disseram que não sabem. É o que
    /// faz o analisador externo subir **na pergunta**, e não na abertura.
    fn acordar_proximo(&self, document_id: DocumentId, required: LanguageCapabilities) {
        let Ok(registry) = self.registry.lock() else {
            return;
        };
        let Some(rotas) = registry.document_routes.get(&document_id).cloned() else {
            return;
        };
        let Some(linguagem) = rotas
            .first()
            .and_then(|id| registry.providers.get(id))
            .map(|entry| entry.metadata.language_id.clone())
        else {
            return;
        };
        let parado = registry
            .providers
            .iter()
            .find(|(id, entry)| {
                entry.worker.is_none()
                    && entry.state != ProviderState::Disabled
                    && entry.state != ProviderState::Failed
                    // Quem já está subindo não precisa ser mandado subir de novo.
                    && entry.state != ProviderState::Activating
                    && entry.metadata.language_id == linguagem
                    && entry.capabilities.contains(required)
                    && !rotas.contains(id)
            })
            .map(|(id, _)| id.clone());
        drop(registry);
        if let Some(id) = parado
            && let Ok(mut fila) = self.a_acordar.lock()
            && !fila.contains(&id)
        {
            fila.push(id);
        }
    }

    /// Quem foi mandado acordar e ainda não subiu.
    ///
    /// Drenado por quem tem o laço de quadros, que ativa **fora** da thread da
    /// interface: subir um analisador é criar um processo, e criar processo não
    /// cabe num quadro.
    #[must_use]
    pub fn take_pending_activation(&self) -> Vec<ProviderId> {
        self.a_acordar
            .lock()
            .map(|mut fila| std::mem::take(&mut *fila))
            .unwrap_or_default()
    }

    /// Sobe um provider, agora. Quem chama decide em que thread.
    pub fn activate_provider(&self, provider_id: &ProviderId) -> Result<(), LanguageHostError> {
        self.ensure_active(provider_id).map(|_| ())
    }

    fn worker_for_document(
        &self,
        document_id: DocumentId,
        required: LanguageCapabilities,
    ) -> Result<Arc<ProviderWorker>, LanguageHostError> {
        // Quem responde é quem **sabe** responder, e não quem pegou o documento
        // primeiro. Ver a composição de capacidades da `04`.
        let provider_id = {
            let registry = self
                .registry
                .lock()
                .map_err(|_| LanguageHostError::WorkerStopped)?;
            registry.provider_for_capability(document_id, required)?
        };
        self.ensure_active(&provider_id)
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
        LanguageCapabilities, LanguageError, LanguageMetadata, LanguageProvider,
        LanguageToolchainConfig, ProviderState,
    };

    use super::{LanguageHost, LanguageHostError, ProviderSelection};

    fn success<T, E: Debug>(result: Result<T, E>) -> T {
        match result {
            Ok(value) => value,
            Err(error) => panic!("expected success, got {error:?}"),
        }
    }

    fn java_toolchain(path: &str) -> LanguageToolchainConfig {
        LanguageToolchainConfig {
            language_id: LanguageId("java".to_owned()),
            installation_root: PathBuf::from(path),
            properties: Default::default(),
        }
    }

    struct TestProvider {
        provider_id: ProviderId,
        language_id: LanguageId,
        extension: String,
        capabilities: LanguageCapabilities,
        activations: Arc<AtomicUsize>,
        shutdowns: Arc<AtomicUsize>,
        worker_thread: Arc<Mutex<Option<ThreadId>>>,
        fail_activation: bool,
        /// Ligado, o provider passa a responder que deixou de existir.
        ///
        /// É como se simula o processo do analisador morrendo no meio da
        /// sessão, sem precisar de processo nenhum.
        gone: Arc<std::sync::atomic::AtomicBool>,
        /// Ligado, o provider responde que **não sabe** — e continua vivo.
        ///
        /// É o limite do provider nativo de TypeScript diante de um tipo que ele
        /// não alcança, e o que a fase 5 da `25` precisa distinguir de morte.
        nao_sabe: Arc<std::sync::atomic::AtomicBool>,
    }

    impl TestProvider {
        fn new(provider_id: &str, capabilities: LanguageCapabilities) -> Self {
            Self {
                provider_id: ProviderId(provider_id.to_owned()),
                language_id: LanguageId("java".to_owned()),
                extension: ".JAVA".to_owned(),
                capabilities,
                activations: Arc::new(AtomicUsize::new(0)),
                shutdowns: Arc::new(AtomicUsize::new(0)),
                worker_thread: Arc::new(Mutex::new(None)),
                fail_activation: false,
                gone: Arc::new(std::sync::atomic::AtomicBool::new(false)),
                nao_sabe: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            }
        }

        fn failing(provider_id: &str, capabilities: LanguageCapabilities) -> Self {
            Self {
                fail_activation: true,
                ..Self::new(provider_id, capabilities)
            }
        }

        fn for_language(
            provider_id: &str,
            language_id: &str,
            extension: &str,
            capabilities: LanguageCapabilities,
        ) -> Self {
            Self {
                language_id: LanguageId(language_id.to_owned()),
                extension: extension.to_owned(),
                ..Self::new(provider_id, capabilities)
            }
        }
    }

    #[async_trait]
    impl LanguageProvider for TestProvider {
        fn metadata(&self) -> LanguageMetadata {
            LanguageMetadata {
                language_id: self.language_id.clone(),
                provider_id: self.provider_id.clone(),
                display_name: self.provider_id.0.clone(),
                extensions: vec![self.extension.clone()],
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
                nao_sabe: Arc::clone(&self.nao_sabe),
                provider_id: self.provider_id.clone(),
                language_id: self.language_id.clone(),
                shutdowns: Arc::clone(&self.shutdowns),
                gone: Arc::clone(&self.gone),
            }))
        }
    }

    #[test]
    fn registers_a_fake_language_without_application_or_ui_changes() {
        let host = LanguageHost::new(".");
        let provider = Arc::new(TestProvider::for_language(
            "fake.native",
            "fake",
            ".fake",
            LanguageCapabilities::SYNTAX,
        ));
        success(host.register(provider.clone()));
        let document = DocumentSnapshot {
            id: DocumentId(77),
            path: "sample.fake".into(),
            version: 1,
            text: "fake source".to_owned(),
        };
        success(pollster::block_on(
            host.open_document(host.request_context(), document),
        ));
        assert_eq!(provider.activations.load(Ordering::Relaxed), 1);
        assert_eq!(
            success(host.provider_for_extension("fake", LanguageCapabilities::SYNTAX)),
            ProviderId("fake.native".to_owned())
        );
    }

    struct TestActiveLanguage {
        language_id: LanguageId,
        shutdowns: Arc<AtomicUsize>,
        gone: Arc<std::sync::atomic::AtomicBool>,
        nao_sabe: Arc<std::sync::atomic::AtomicBool>,
        provider_id: ProviderId,
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

        async fn completion(
            &self,
            _: ide_domain::CompletionRequest,
        ) -> Result<Vec<ide_domain::CompletionItem>, LanguageError> {
            if self.nao_sabe.load(Ordering::Relaxed) {
                return Err(LanguageError::Unresolved("não sei o tipo".to_owned()));
            }
            Ok(vec![ide_domain::CompletionItem {
                label: self.provider_id.0.clone(),
                detail: None,
                kind: ide_domain::CompletionKind::Field,
            }])
        }

        async fn syntax(&self, document_id: DocumentId) -> Result<ide_domain::SyntaxSnapshot, LanguageError> {
            if self.gone.load(Ordering::Relaxed) {
                return Err(LanguageError::Unavailable("o processo morreu".to_owned()));
            }
            Ok(ide_domain::SyntaxSnapshot {
                document_id,
                version: 1,
                outline: Vec::new(),
                highlights: Vec::new(),
                imports: Vec::new(),
                diagnostics: Vec::new(),
            })
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

    /// O documento migra para o provider de baixo quando o de cima morre.
    ///
    /// É o que dá dente à ADR-025: sem isto, "o nativo é o chão" seria uma
    /// frase, e o documento ficaria preso ao provider morto.
    ///
    /// O host **não** reabre sozinho, e é decisão: quem tem o texto é a
    /// aplicação. Ele desfaz a rota, e a próxima abertura escolhe o candidato
    /// seguinte. Guardar o texto aqui duplicaria o que o editor já tem.
    #[test]
    fn a_document_moves_to_the_provider_below_when_the_one_above_dies() {
        let host = LanguageHost::new(".");
        let principal = Arc::new(TestProvider::new(
            "ts.service",
            LanguageCapabilities::SYNTAX,
        ));
        let chao = Arc::new(TestProvider::new("ts.syntax", LanguageCapabilities::SYNTAX));
        success(host.register(principal.clone()));
        success(host.register(chao.clone()));
        success(host.configure_selection(
            LanguageId("java".to_owned()),
            ProviderSelection {
                primary: ProviderId("ts.service".to_owned()),
                fallbacks: vec![ProviderId("ts.syntax".to_owned())],
            },
        ));

        let documento = DocumentSnapshot {
            id: DocumentId(1),
            path: PathBuf::from("/w/pedido.java"),
            version: 1,
            text: "class Pedido {}".to_owned(),
        };
        let aceito = success(pollster::block_on(
            host.open_document(host.request_context(), documento.clone()),
        ));
        assert_eq!(
            aceito,
            ProviderId("ts.service".to_owned()),
            "a ordem declarada é que decide, e não a alfabética"
        );
        assert!(
            pollster::block_on(host.syntax(host.request_context(), DocumentId(1))).is_ok(),
            "de pé, o principal responde"
        );

        // O processo do analisador morre no meio da sessão.
        principal.gone.store(true, Ordering::Relaxed);
        let depois = pollster::block_on(host.syntax(host.request_context(), DocumentId(1)));
        assert!(
            matches!(depois, Err(LanguageHostError::ProviderGone(_))),
            "a morte precisa ser dita, e não confundida com falha de pedido: {depois:?}"
        );

        // **A fase 5 da `25` trocou a queda imediata por não subir o analisador
        // à toa.** Antes, abrir ativava todos os candidatos, e o de baixo já
        // tinha o documento: a queda não custava reabertura. Agora abrir ativa
        // só quem já está de pé, porque subir um analisador externo custa 1,9 GB
        // e trinta segundos — e pagá-los ao abrir o primeiro `.ts` é pagá-los
        // mesmo quando ninguém vai perguntar nada que exija tipos.
        //
        // O que se perdeu vale pouco na prática: com o índice como principal,
        // quem morre é o analisador, e o índice já está de pé respondendo. O
        // caso deste teste — o principal morrendo — passou a custar a reabertura
        // que a linha abaixo exercita, e quem a faz é a aplicação, que tem o
        // texto.
        assert!(
            pollster::block_on(host.syntax(host.request_context(), DocumentId(1))).is_err(),
            "sem ter recebido o documento, o de baixo não tem o que responder"
        );

        // E reabrir continua funcionando, que é o caminho de quando **nenhum**
        // dos que restam tem o documento.
        let renovado = success(pollster::block_on(
            host.open_document(host.request_context(), documento),
        ));
        assert_eq!(renovado, ProviderId("ts.syntax".to_owned()));
    }

    /// Quando o único provider morre, o documento fica sem rota.
    ///
    /// É o caminho que a aplicação trata reabrindo — e ele só aparece quando não
    /// há mais ninguém com o arquivo, porque com dois a queda é imediata.
    #[test]
    fn a_document_with_no_provider_left_loses_its_route() {
        let host = LanguageHost::new(".");
        let unico = Arc::new(TestProvider::new(
            "java.native",
            LanguageCapabilities::SYNTAX,
        ));
        success(host.register(unico.clone()));
        success(pollster::block_on(host.open_document(
            host.request_context(),
            DocumentSnapshot {
                id: DocumentId(1),
                path: PathBuf::from("/w/Pedido.java"),
                version: 1,
                text: "class Pedido {}".to_owned(),
            },
        )));

        unico.gone.store(true, Ordering::Relaxed);
        let morte = pollster::block_on(host.syntax(host.request_context(), DocumentId(1)));
        assert!(matches!(morte, Err(LanguageHostError::ProviderGone(_))));

        let orfao = pollster::block_on(host.syntax(host.request_context(), DocumentId(1)));
        assert!(
            matches!(orfao, Err(LanguageHostError::DocumentNotRouted(_))),
            "sem ninguém com o arquivo, o documento fica sem rota: {orfao:?}"
        );
    }

    /// O provider ocioso cai, e volta sozinho quando é pedido.
    ///
    /// Suspender solta o `ActiveLanguage`, e é ali que o índice mora — a `20`
    /// mediu 103 MB só o de Java. O que se cobra aqui é que ele saia e volte
    /// sem ninguém precisar saber.
    #[test]
    fn an_idle_provider_is_suspended_and_comes_back_when_asked() {
        let host = LanguageHost::new(".");
        let provider = Arc::new(TestProvider::new(
            "java.native",
            LanguageCapabilities::SYNTAX,
        ));
        success(host.register(provider.clone()));

        let documento = DocumentSnapshot {
            id: DocumentId(1),
            path: PathBuf::from("/w/Pedido.java"),
            version: 1,
            text: "class Pedido {}".to_owned(),
        };
        success(pollster::block_on(
            host.open_document(host.request_context(), documento.clone()),
        ));
        assert_eq!(provider.activations.load(Ordering::Relaxed), 1);

        // Com documento aberto ele não é ocioso, por mais parado que esteja:
        // a tecla seguinte custaria reindexar no meio da digitação.
        let com_aba = success(pollster::block_on(
            host.suspend_idle(std::time::Duration::ZERO),
        ));
        assert!(
            com_aba.is_empty(),
            "provider com aba aberta não pode ser suspenso: {com_aba:?}"
        );

        // Fechada a última aba, ele passa a ser candidato à suspensão.
        success(pollster::block_on(
            host.close_document(host.request_context(), DocumentId(1)),
        ));
        let suspensos = success(pollster::block_on(
            host.suspend_idle(std::time::Duration::ZERO),
        ));
        assert_eq!(suspensos, vec![ProviderId("java.native".to_owned())]);
        assert_eq!(
            provider.shutdowns.load(Ordering::Relaxed),
            1,
            "suspender precisa soltar o ActiveLanguage, que é onde o índice mora"
        );

        // E ele continua candidato: o pedido seguinte o traz de volta sozinho.
        success(pollster::block_on(
            host.open_document(host.request_context(), documento),
        ));
        assert_eq!(
            provider.activations.load(Ordering::Relaxed),
            2,
            "voltar é reativar, e é a única coisa que se nota"
        );
        assert!(pollster::block_on(host.syntax(host.request_context(), DocumentId(1))).is_ok());
    }

    /// Quem foi usado agora não é suspenso.
    #[test]
    fn a_provider_in_use_is_not_suspended() {
        let host = LanguageHost::new(".");
        success(host.register(Arc::new(TestProvider::new(
            "java.native",
            LanguageCapabilities::SYNTAX,
        ))));
        success(pollster::block_on(host.open_document(
            host.request_context(),
            DocumentSnapshot {
                id: DocumentId(1),
                path: PathBuf::from("/w/Pedido.java"),
                version: 1,
                text: "class Pedido {}".to_owned(),
            },
        )));
        success(pollster::block_on(
            host.close_document(host.request_context(), DocumentId(1)),
        ));
        let suspensos = success(pollster::block_on(
            host.suspend_idle(std::time::Duration::from_secs(300)),
        ));
        assert!(
            suspensos.is_empty(),
            "usado agora, o provider fica de pé: {suspensos:?}"
        );
    }

    /// Dois providers da mesma linguagem, com capacidades **complementares**.
    ///
    /// É o caso real de TypeScript: o externo responde com tipo — completação,
    /// diagnóstico, definição — e o nativo responde por realce. Nenhum dos dois
    /// faz tudo, e a `04` chama isso de composição de capacidades.
    ///
    /// O que se cobra aqui é se o host compõe ou se ele apenas escolhe.
    #[test]
    fn two_providers_with_complementary_capabilities() {
        let host = LanguageHost::new(".");
        success(host.register(Arc::new(TestProvider::new(
            "ts.service",
            LanguageCapabilities::COMPLETION,
        ))));
        success(host.register(Arc::new(TestProvider::new(
            "ts.syntax",
            LanguageCapabilities::SYNTAX,
        ))));
        success(host.configure_selection(
            LanguageId("java".to_owned()),
            ProviderSelection {
                primary: ProviderId("ts.service".to_owned()),
                fallbacks: vec![ProviderId("ts.syntax".to_owned())],
            },
        ));

        success(pollster::block_on(host.open_document(
            host.request_context(),
            DocumentSnapshot {
                id: DocumentId(1),
                path: PathBuf::from("/w/pedido.java"),
                version: 1,
                text: "class Pedido {}".to_owned(),
            },
        )));

        let realce = pollster::block_on(host.syntax(host.request_context(), DocumentId(1)));
        assert!(
            realce.is_ok(),
            "o realce precisa vir de quem sabe realçar, e não do provider que              pegou o documento primeiro: {realce:?}"
        );
    }

    /// A ordem entre providers é a declarada, e não a alfabética.
    ///
    /// Sem seleção declarada, `ts.service` viria antes de `ts.syntax` por sair
    /// antes no alfabeto — e um teste que não trocasse os nomes de lugar
    /// passaria pelo motivo errado.
    #[test]
    fn the_declared_order_wins_over_the_alphabet() {
        let host = LanguageHost::new(".");
        success(host.register(Arc::new(TestProvider::new(
            "aaa.primeiro",
            LanguageCapabilities::SYNTAX,
        ))));
        success(host.register(Arc::new(TestProvider::new(
            "zzz.ultimo",
            LanguageCapabilities::SYNTAX,
        ))));
        success(host.configure_selection(
            LanguageId("java".to_owned()),
            ProviderSelection {
                primary: ProviderId("zzz.ultimo".to_owned()),
                fallbacks: vec![ProviderId("aaa.primeiro".to_owned())],
            },
        ));
        let escolhido = success(host.provider_for_extension(
            "java",
            LanguageCapabilities::SYNTAX,
        ));
        assert_eq!(escolhido, ProviderId("zzz.ultimo".to_owned()));
    }

    /// Dizer "não sei" faz o host procurar quem saiba, e não demite ninguém.
    ///
    /// # O defeito que este teste guarda
    ///
    /// A fase 4 da `25` deu ao provider nativo de TypeScript a resposta "não sei
    /// o tipo desta expressão". Ela saía como `Unavailable`, que para o host
    /// quer dizer **deixei de existir** — e o host reagia tirando as rotas dele.
    ///
    /// Na prática: o primeiro `.pipe(map(x => x.` de uma sessão derrubava o
    /// provider que dá realce e estrutura, e o arquivo ficava sem cor por causa
    /// de uma completação que ninguém podia responder.
    ///
    /// O limite de um provider não é a morte dele.
    #[test]
    fn saying_it_does_not_know_looks_for_someone_who_does() {
        let host = LanguageHost::new(".");
        let indice = Arc::new(TestProvider::new(
            "ts.indice",
            LanguageCapabilities::SYNTAX | LanguageCapabilities::COMPLETION,
        ));
        let externo = Arc::new(TestProvider::new(
            "ts.externo",
            LanguageCapabilities::COMPLETION,
        ));
        let nao_sabe = Arc::clone(&indice.nao_sabe);
        success(host.register(Arc::clone(&indice) as Arc<dyn LanguageProvider>));
        success(host.register(Arc::clone(&externo) as Arc<dyn LanguageProvider>));
        success(host.configure_selection(
            LanguageId("java".to_owned()),
            ProviderSelection {
                primary: ProviderId("ts.indice".to_owned()),
                fallbacks: vec![ProviderId("ts.externo".to_owned())],
            },
        ));
        success(pollster::block_on(host.open_document(
            host.request_context(),
            DocumentSnapshot {
                id: DocumentId(1),
                path: PathBuf::from("/w/a.java"),
                version: 1,
                text: String::new(),
            },
        )));

        let pedir = || {
            pollster::block_on(host.completion(
                host.request_context(),
                ide_domain::CompletionRequest {
                    document_id: DocumentId(1),
                    position: ide_domain::TextPosition { line: 0, column: 0 },
                    prefix: String::new(),
                },
            ))
        };

        // Sabendo, o primeiro responde.
        let itens = success(pedir());
        assert_eq!(itens.first().map(|item| item.label.as_str()), Some("ts.indice"));

        // Não sabendo, e sem mais ninguém com o documento, a resposta é
        // `Unresolved` — **e o analisador é acordado agora**. É o momento em que
        // ele passa a valer o que custa: a primeira pergunta da sessão que exige
        // mais do que o índice alcança.
        nao_sabe.store(true, Ordering::Relaxed);
        assert!(
            matches!(pedir(), Err(LanguageHostError::Unresolved(_))),
            "sem ninguém que saiba e tenha o documento, a resposta é dizer isso"
        );
        // A pergunta **anota** quem acordar, e não o acorda: subir um analisador
        // é criar um processo, e criar processo não cabe num quadro. Quem tem o
        // laço drena e ativa fora da thread da interface.
        assert_eq!(
            externo.activations.load(Ordering::Relaxed),
            0,
            "a pergunta não pode subir processo na thread de quem perguntou"
        );
        let pendentes = host.take_pending_activation();
        assert_eq!(
            pendentes,
            vec![ProviderId("ts.externo".to_owned())],
            "mas ela precisa dizer quem acordar"
        );
        for id in pendentes {
            success(host.activate_provider(&id));
        }
        assert_eq!(externo.activations.load(Ordering::Relaxed), 1);

        // Ele subiu sem o texto de nada, e quem o tem é a aplicação: ela reabre.
        assert!(
            !host.documents_missing_providers().is_empty(),
            "o host precisa dizer quais documentos faltam a quem subiu"
        );
        success(pollster::block_on(host.open_document(
            host.request_context(),
            DocumentSnapshot {
                id: DocumentId(1),
                path: PathBuf::from("/w/a.java"),
                version: 2,
                text: String::new(),
            },
        )));

        let itens = success(pedir());
        assert_eq!(
            itens.first().map(|item| item.label.as_str()),
            Some("ts.externo"),
            "com o documento em mãos, quem sabe responde"
        );

        let estados = success(host.providers());
        let indice = estados
            .iter()
            .find(|snapshot| snapshot.metadata.provider_id.0 == "ts.indice");
        assert!(
            indice.is_some_and(|snapshot| snapshot.state != ProviderState::Failed),
            "admitir um limite não pode demitir quem o admitiu: {indice:?}"
        );
        assert!(
            pollster::block_on(host.syntax(host.request_context(), DocumentId(1))).is_ok(),
            "e ele continua respondendo o que sabe"
        );
    }

    /// Quem já está subindo não é mandado subir de novo.
    ///
    /// # O defeito que este teste guarda
    ///
    /// Subir um analisador leva trinta segundos num projeto grande. Sem esta
    /// guarda, **cada pergunta feita nesse intervalo mandava subir outro**: mais
    /// um processo de 1,9 GB por clique, todos montando o mesmo projeto ao mesmo
    /// tempo. A máquina engasgava, e o sintoma era a IDE travada.
    #[test]
    fn a_provider_already_coming_up_is_not_started_twice() {
        let host = LanguageHost::new(".");
        let devagar = Arc::new(TestProvider::new(
            "ts.devagar",
            LanguageCapabilities::SYNTAX,
        ));
        success(host.register(Arc::clone(&devagar) as Arc<dyn LanguageProvider>));

        // Marcado como subindo, sem worker: é o estado no meio da ativação.
        {
            let Ok(mut registry) = host.registry.lock() else {
                panic!("o registro precisa estar acessível");
            };
            let Some(entry) = registry
                .providers
                .get_mut(&ProviderId("ts.devagar".to_owned()))
            else {
                panic!("o provider precisa estar registrado");
            };
            entry.state = ProviderState::Activating;
        }

        let segunda = host.activate_provider(&ProviderId("ts.devagar".to_owned()));
        assert!(
            matches!(segunda, Err(LanguageHostError::Unresolved(_))),
            "quem já está subindo não sobe de novo: {segunda:?}"
        );
        assert_eq!(
            devagar.activations.load(Ordering::Relaxed),
            0,
            "nenhuma ativação nova pode ter começado"
        );
    }

    #[test]
    fn workspace_types_activate_by_extension_without_an_open_document() {
        let host = LanguageHost::new(".");
        // A busca é roteada por `WORKSPACE_SYMBOLS`, e não por `COMPLETION`: um
        // provider pode ter índice de nomes sem saber tipar uma expressão.
        success(host.register(Arc::new(TestProvider::new(
            "java.types",
            LanguageCapabilities::WORKSPACE_SYMBOLS,
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
        assert!(success(host.set_toolchain(
            LanguageId("java".to_owned()),
            Some(java_toolchain("jdk-17")),
        )));
        // O mesmo JDK não é uma troca.
        assert!(!success(host.set_toolchain(
            LanguageId("java".to_owned()),
            Some(java_toolchain("jdk-17")),
        )));

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

        assert!(success(host.set_toolchain(
            LanguageId("java".to_owned()),
            Some(java_toolchain("jdk-21")),
        )));
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
