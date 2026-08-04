#![doc = "Contratos versionados e independentes de linguagem."]

use async_trait::async_trait;
use std::{collections::BTreeMap, path::PathBuf};

use ide_domain::{
    AccessorKind, AccessorPlan, CompletionItem, CompletionRequest, DefinitionRequest, Diagnostic,
    DocumentChange, DocumentId, DocumentSnapshot, LanguageId, Location, ProviderId,
    ReferencesRequest, RequestId, SemanticSnapshot, SemanticSymbol, SyntaxSnapshot, TextPosition,
    TextRange,
};

/// O cancelamento é do domínio, e não deste contrato.
///
/// Fica reexportado para quem já o importava daqui, e para que o contrato de
/// linguagens continue legível sem ir procurar noutra crate. Ver a ADR-024.
pub use ide_domain::CancellationToken;
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
        /// Responder "quais tipos do projeto se chamam assim".
        ///
        /// Separada de `COMPLETION` porque as duas têm preços muito diferentes:
        /// buscar por nome pede um índice de nomes, e completar depois de um
        /// ponto pede saber o **tipo** de uma expressão. Um provider pode ter o
        /// primeiro e não o segundo, e antes desta separação declarar a busca
        /// obrigava a prometer o ponto junto.
        const WORKSPACE_SYMBOLS = 1 << 11;
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

/// Um sinal de que a linguagem ainda está preparando o projeto.
///
/// # Por que é um sinal compartilhado, e não uma pergunta
///
/// Perguntar exigiria falar com o worker da linguagem, e ele atende **um pedido
/// por vez**. Quando um analisador leva trinta segundos montando o projeto, a
/// pergunta "você já terminou?" ficaria na fila atrás justamente do trabalho
/// sobre o qual se está perguntando — a resposta chegaria junto com o fim, que é
/// quando ela deixa de importar.
///
/// O sinal é entregue **uma vez**, na ativação, e lido de fora quantas vezes for
/// preciso sem tocar na thread que trabalha.
///
/// # Neutro de propósito
///
/// Nada aqui é de TypeScript. Uma linguagem que monte índice em segundo plano —
/// Java, hoje — pode usar o mesmo sinal, e a IDE continua sem saber o que
/// qualquer uma delas está fazendo: ela sabe só que ainda não dá para contar com
/// a resposta completa.
#[derive(Clone, Debug, Default)]
pub struct ReadinessSignal(std::sync::Arc<std::sync::atomic::AtomicBool>);

impl ReadinessSignal {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Diz que a preparação terminou. Não há caminho de volta.
    pub fn mark_ready(&self) {
        self.0.store(true, std::sync::atomic::Ordering::Release);
    }

    #[must_use]
    pub fn is_ready(&self) -> bool {
        self.0.load(std::sync::atomic::Ordering::Acquire)
    }
}

#[async_trait]
pub trait ActiveLanguage: Send + Sync {
    fn language_id(&self) -> &LanguageId;

    /// O sinal que diz quando esta linguagem terminou de preparar o projeto.
    ///
    /// `None` é "não há o que esperar": a linguagem responde por completo desde
    /// a ativação, que é o caso de todo provider nativo. Quem devolve um sinal
    /// promete marcá-lo — um sinal que nunca fica pronto deixaria a IDE dizendo
    /// para sempre que está carregando.
    fn readiness(&self) -> Option<ReadinessSignal> {
        None
    }
    async fn open_document(&self, document: DocumentSnapshot) -> Result<(), LanguageError>;
    async fn change_document(&self, change: DocumentChange) -> Result<(), LanguageError>;
    async fn close_document(&self, document_id: DocumentId) -> Result<(), LanguageError>;
    async fn diagnostics(&self, document_id: DocumentId) -> Result<Vec<Diagnostic>, LanguageError>;

    /// Avisa que um arquivo mudou em disco, para o índice acompanhar.
    ///
    /// Gravar deixa de esperar a próxima ativação: a classe criada agora entra
    /// na completação sem reiniciar nada. Linguagens sem índice ignoram.
    async fn file_changed(&self, _path: &std::path::Path) -> Result<(), LanguageError> {
        Ok(())
    }

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
    /// O realce, a estrutura e os diagnósticos de um documento.
    ///
    /// # `visible` é uma dica, e não um recorte obrigatório
    ///
    /// Ele diz **o que está na tela**. Uma linguagem pode usá-lo para não
    /// percorrer o que ninguém vê, e pode ignorá-lo — devolver mais do que se
    /// pediu é sempre correto, e devolver menos que a faixa pedida não é.
    ///
    /// Medido no provider de TypeScript, que é onde isto nasceu: num arquivo de
    /// 3 144 linhas, montar o realce inteiro custa 22 ms **a cada tecla**, para
    /// desenhar cerca de cinquenta linhas. São 7 783 realces produzidos para uns
    /// poucos aparecerem.
    ///
    /// **A estrutura e os diagnósticos continuam do arquivo inteiro.** Eles
    /// alimentam painel e contagem, e não a pintura do texto; recortá-los faria
    /// a lista de símbolos encolher ao rolar.
    ///
    /// `None` pede tudo, e é o que quem não desenha usa.
    async fn syntax(
        &self,
        _document_id: DocumentId,
        _visible: Option<TextRange>,
    ) -> Result<SyntaxSnapshot, LanguageError> {
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
    /// Este pedido falhou, e o provider continua de pé.
    #[error("provider failed: {0}")]
    Provider(String),
    /// Esta pergunta eu não sei responder, e continuo vivo.
    ///
    /// # Por que não é `Unavailable`
    ///
    /// `Unavailable` quer dizer "deixei de existir", e o host reage a ela
    /// **demitindo** o provider: tira as rotas e o marca como falho. É o certo
    /// para um processo que morreu.
    ///
    /// Mas um provider pode estar inteiro e não saber uma resposta. O índice de
    /// TypeScript sabe o tipo de um receptor declarado e não sabe o de
    /// `.pipe(map(x => x.` — e responder `Unavailable` ali o derrubaria por
    /// admitir um limite que ele sempre teve.
    ///
    /// Com esta variante, o host faz a coisa certa: **tenta o próximo
    /// candidato**, que é o analisador externo, e ninguém é demitido. É o que a
    /// fase 5 da `25` precisa para subir o analisador só quando ele for pedido.
    #[error("cannot answer: {0}")]
    Unresolved(String),
    /// O provider deixou de poder responder: o processo morreu, o canal fechou.
    ///
    /// É diferente de `Provider`, e a diferença decide o que o host faz. Falhar
    /// num pedido é falhar num pedido; deixar de existir é outra coisa, e é o
    /// que faz o documento ser reencaminhado ao próximo candidato — o provider
    /// nativo, no caso de TypeScript.
    ///
    /// Sem esta distinção, "o nativo é o chão" da ADR-025 seria uma frase: o
    /// documento ficaria preso ao provider morto, e quem espera resposta
    /// esperaria para sempre. Ver a fase 3b da `23`.
    #[error("provider is no longer available: {0}")]
    Unavailable(String),
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
