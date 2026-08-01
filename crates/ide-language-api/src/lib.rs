#![doc = "Contratos versionados e independentes de linguagem."]

use async_trait::async_trait;
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use ide_domain::{
    AccessorKind, AccessorPlan, CompletionItem, CompletionRequest, DefinitionRequest, Diagnostic,
    DocumentChange, DocumentId, DocumentSnapshot, LanguageId, Location, ProviderId,
    ReferencesRequest, RequestId, SemanticSnapshot, SemanticSymbol, SyntaxSnapshot, TextPosition,
};
use thiserror::Error;

pub const LANGUAGE_API_VERSION: ApiVersion = ApiVersion { major: 2, minor: 0 };

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
    /// Caracteres que, ao serem digitados, pedem completação sozinhos.
    ///
    /// Em Java é o ponto; em outra linguagem pode ser `::` ou `->`. Quem sabe
    /// disso é a linguagem, e por isso o editor pergunta em vez de carregar uma
    /// lista própria — a alternativa seria a shell decidir sobre a sintaxe de
    /// uma linguagem que ela não conhece.
    pub trigger_characters: Vec<char>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LanguageToolchainConfig {
    pub language_id: LanguageId,
    pub installation_root: PathBuf,
    pub properties: BTreeMap<String, String>,
}

#[derive(Clone, Debug)]
pub struct LanguageActivationContext {
    pub workspace_root: PathBuf,
    pub source_roots: Vec<PathBuf>,
    /// Toolchains selecionadas, associadas à linguagem que as interpreta.
    ///
    /// O contrato não conhece JDK, SDK ou runtime concreto. Cada provider usa a
    /// instalação da sua linguagem e interpreta propriedades próprias.
    pub toolchains: Vec<LanguageToolchainConfig>,
}

impl LanguageActivationContext {
    #[must_use]
    pub fn toolchain(&self, language_id: &LanguageId) -> Option<&LanguageToolchainConfig> {
        self.toolchains
            .iter()
            .find(|toolchain| &toolchain.language_id == language_id)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MemberAccess {
    pub receiver: String,
    pub prefix: String,
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

    /// Espera o índice do projeto ficar pronto, se houver um.
    ///
    /// Ativar não espera mais: uma linguagem pode devolver o ambiente na hora e
    /// montar o índice em segundo plano. Até ele chegar, o que depende do
    /// projeto responde **o que já tem** — nada, no começo — e o que depende só
    /// do documento aberto responde igual.
    ///
    /// Quem precisa da resposta completa chama isto. O padrão é `true`: uma
    /// linguagem sem índice já está pronta.
    async fn wait_until_indexed(&self, _timeout: std::time::Duration) -> bool {
        true
    }
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
    async fn member_access(
        &self,
        _text: &str,
        _offset: usize,
    ) -> Result<Option<MemberAccess>, LanguageError> {
        Err(LanguageError::Unsupported("member access".to_owned()))
    }
    /// Membros públicos de um tipo nomeado, sem documento nem posição.
    ///
    /// A completação normal parte de um ponto dentro de um arquivo, e é dali que
    /// ela descobre o tipo do receptor. Há telas em que não existe arquivo — o
    /// editor de expressões do depurador é uma delas —, mas o tipo já é conhecido
    /// por outro meio. O índice consultado é o mesmo da completação comum: o
    /// projeto inteiro, as dependências e a biblioteca padrão. Uma classe que não
    /// participa do que está sendo depurado é tão conhecida quanto as outras.
    async fn type_members(
        &self,
        _type_name: &str,
        _prefix: &str,
    ) -> Result<Vec<CompletionItem>, LanguageError> {
        Err(LanguageError::Unsupported("type members".to_owned()))
    }
    /// Tipos do projeto cujo nome casa com o que foi digitado.
    ///
    /// Serve à busca por nome — abrir uma classe sem saber em que pasta ela está.
    /// Só entram tipos com arquivo no workspace: o resultado existe para ser
    /// aberto, e uma classe dentro de um jar não tem onde ser aberta.
    ///
    /// Consulta vazia devolve tudo o que couber no teto, para a janela ter o que
    /// mostrar antes da primeira letra.
    async fn workspace_types(
        &self,
        _query: &str,
        _limit: usize,
    ) -> Result<Vec<SemanticSymbol>, LanguageError> {
        Err(LanguageError::Unsupported("workspace types".to_owned()))
    }
    /// Acessores que faltam ao tipo que contém a posição.
    ///
    /// A linguagem devolve o texto pronto e onde ele entra; quem chama escolhe
    /// quais usar. É o que permite a tela oferecer "gerar getter" sem saber o
    /// que é um getter.
    async fn accessor_plan(
        &self,
        _document_id: DocumentId,
        _position: TextPosition,
        _kind: AccessorKind,
    ) -> Result<AccessorPlan, LanguageError> {
        Err(LanguageError::Unsupported("accessor plan".to_owned()))
    }
    /// Construtor do tipo que contém a posição, com os campos escolhidos.
    ///
    /// Separado do plano porque o texto **depende da escolha**: os acessores dão
    /// um trecho por campo, e o construtor dá um só, a partir do conjunto. Lista
    /// vazia é um construtor sem parâmetros — resposta legítima, e não ausência
    /// de resposta. `None` é o tipo já ter um construtor de mesma assinatura,
    /// caso em que escrever outro não compilaria.
    async fn constructor_source(
        &self,
        _document_id: DocumentId,
        _position: TextPosition,
        _fields: Vec<String>,
    ) -> Result<Option<String>, LanguageError> {
        Err(LanguageError::Unsupported("constructor source".to_owned()))
    }
    /// Onde um nome é referenciado no projeto inteiro.
    ///
    /// Diferente de `references`, que parte de uma posição num arquivo aberto:
    /// renomear um arquivo fala de um nome que talvez não esteja aberto em lugar
    /// nenhum. Quem sabe o que conta como referência — um uso do tipo, e não a
    /// palavra solta dentro de um comentário — é a linguagem.
    async fn references_to_name(&self, _name: &str) -> Result<Vec<Location>, LanguageError> {
        Err(LanguageError::Unsupported("references to name".to_owned()))
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
