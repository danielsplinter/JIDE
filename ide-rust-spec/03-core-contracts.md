# 03 — Contratos Centrais

## Tipos básicos

```rust
pub struct LanguageId(pub String);
pub struct ProviderId(pub String);
pub struct DocumentId(pub u64);
pub struct WorkspaceId(pub u64);
pub struct ProjectId(pub u64);
pub struct SymbolId(pub u64);

pub struct TextPosition {
    pub line: u32,
    pub column: u32,
}

pub struct TextRange {
    pub start: TextPosition,
    pub end: TextPosition,
}
```

## Contrato de provider de linguagem

```rust
#[async_trait::async_trait]
pub trait LanguageProvider: Send + Sync {
    fn metadata(&self) -> LanguageMetadata;

    fn capabilities(&self) -> LanguageCapabilities;

    async fn activate(
        &self,
        context: LanguageActivationContext,
    ) -> Result<Box<dyn ActiveLanguage>, LanguageError>;
}
```

O metadado declara os **caracteres de gatilho** da linguagem — o que, ao ser
digitado, pede completação sozinho. Em Java é o ponto; em outra linguagem pode ser
`::` ou `->`. Quem sabe disso é a linguagem, e o editor pergunta: a alternativa
seria a shell decidir sobre a sintaxe de uma linguagem que ela não conhece.

```rust
pub struct LanguageMetadata {
    pub language_id: LanguageId,
    pub provider_id: ProviderId,
    pub display_name: String,
    pub extensions: Vec<String>,
    pub api_version: ApiVersion,
    pub trigger_characters: Vec<char>,
}
```

O contexto de ativação carrega, além da raiz do workspace, a **raiz do toolchain
escolhido na IDE**:

```rust
pub struct LanguageActivationContext {
    pub workspace_root: PathBuf,
    pub jdk_home: Option<PathBuf>,
}
```

A biblioteca padrão que a completação conhece vem dessa instalação, não de uma
variável de ambiente: trocar de JDK pelo menu tem que trocar as classes que a
completação enxerga. Como o provider indexa a biblioteca padrão na ativação,
mudar a escolha exige derrubar os providers ativos — o host expõe `set_jdk_home`,
que informa se houve troca, e `reactivate`, que faz a próxima requisição subir um
provider novo com o contexto atual. As rotas de documento permanecem, mas os
documentos abertos não: o provider novo nasce sem nenhum, e quem sincroniza
precisa reabri-los.

## Instância ativa de linguagem

```rust
#[async_trait::async_trait]
pub trait ActiveLanguage: Send + Sync {
    fn language_id(&self) -> &LanguageId;

    async fn open_document(
        &self,
        document: DocumentSnapshot,
    ) -> Result<(), LanguageError>;

    async fn change_document(
        &self,
        change: DocumentChange,
    ) -> Result<(), LanguageError>;

    async fn close_document(
        &self,
        document_id: DocumentId,
    ) -> Result<(), LanguageError>;

    async fn diagnostics(
        &self,
        document_id: DocumentId,
    ) -> Result<Vec<Diagnostic>, LanguageError>;

    async fn completion(
        &self,
        request: CompletionRequest,
    ) -> Result<Vec<CompletionItem>, LanguageError>;

    async fn definition(
        &self,
        request: DefinitionRequest,
    ) -> Result<Vec<Location>, LanguageError>;

    async fn references(
        &self,
        request: ReferencesRequest,
    ) -> Result<Vec<Location>, LanguageError>;

    async fn shutdown(&self) -> Result<(), LanguageError>;
}
```

## Contrato do parser

```rust
pub trait SyntaxEngine: Send + Sync {
    fn parse(
        &self,
        document: &DocumentSnapshot,
        previous: Option<&SyntaxSnapshot>,
    ) -> Result<SyntaxSnapshot, SyntaxError>;
}
```

O resultado sintático público é neutro em relação à linguagem:

```rust
pub struct SyntaxSnapshot {
    pub document_id: DocumentId,
    pub version: u64,
    pub tree: SyntaxNode,
    pub outline: Vec<OutlineItem>,
    pub highlights: Vec<SyntaxHighlight>,
    pub imports: Vec<ImportItem>,
    pub diagnostics: Vec<Diagnostic>,
}
```

`SyntaxNode` usa nomes de nós como texto e intervalos genéricos. Highlights,
outline, imports e diagnósticos também usam tipos de `ide-domain`; nenhum tipo
do Tree-sitter ou específico de Java atravessa o contrato público.

`ActiveLanguage::syntax` retorna o snapshot correspondente à versão atual do
documento. O Language Host transporta essa operação pelo mesmo worker isolado
e pelo mesmo contexto cancelável das demais solicitações.

## Contratos semânticos

`SemanticSnapshot` contém símbolos e escopos associados à versão do documento.
Cada símbolo usa `SymbolKind`, `Location`, profundidade de escopo e um
`TypeDescriptor` opcional. Nenhum nó interno do parser atravessa esta API.

As operações semânticas do provider são:

```rust
async fn semantic(document_id: DocumentId) -> Result<SemanticSnapshot, LanguageError>;
async fn completion(request: CompletionRequest) -> Result<Vec<CompletionItem>, LanguageError>;
async fn definition(request: DefinitionRequest) -> Result<Vec<Location>, LanguageError>;
async fn references(request: ReferencesRequest) -> Result<Vec<Location>, LanguageError>;
```

`CompletionRequest`, `DefinitionRequest` e `ReferencesRequest` identificam o
documento e uma `TextPosition`. Referências permitem escolher se a declaração
também deve fazer parte do resultado.

## Contrato de análise semântica

```rust
#[async_trait::async_trait]
pub trait SemanticEngine: Send + Sync {
    async fn analyze(
        &self,
        request: SemanticAnalysisRequest,
    ) -> Result<SemanticSnapshot, SemanticError>;

    async fn resolve_symbol(
        &self,
        request: ResolveSymbolRequest,
    ) -> Result<Option<ResolvedSymbol>, SemanticError>;

    async fn infer_type(
        &self,
        request: TypeInferenceRequest,
    ) -> Result<Option<TypeDescriptor>, SemanticError>;
}
```

## Contrato de indexação

```rust
#[async_trait::async_trait]
pub trait SymbolIndex: Send + Sync {
    async fn upsert_document(
        &self,
        document: IndexedDocument,
    ) -> Result<(), IndexError>;

    async fn remove_document(
        &self,
        document_id: DocumentId,
    ) -> Result<(), IndexError>;

    async fn search_symbols(
        &self,
        query: SymbolQuery,
    ) -> Result<Vec<SymbolSearchResult>, IndexError>;

    async fn find_references(
        &self,
        symbol_id: SymbolId,
    ) -> Result<Vec<Location>, IndexError>;
}
```

## Contrato de toolchain

```rust
#[async_trait::async_trait]
pub trait ToolchainProvider: Send + Sync {
    fn toolchain_id(&self) -> ToolchainId;

    async fn detect(
        &self,
        context: DetectionContext,
    ) -> Result<Vec<ToolchainInstallation>, ToolchainError>;

    async fn validate(
        &self,
        installation: &ToolchainInstallation,
    ) -> Result<ToolchainValidation, ToolchainError>;
}
```

## Contrato de compilador

```rust
#[async_trait::async_trait]
pub trait CompilerAdapter: Send + Sync {
    fn supported_language(&self) -> LanguageId;

    async fn compile(
        &self,
        request: CompilationRequest,
    ) -> Result<CompilationResult, ToolchainError>;
}
```

## Contrato de runtime, interpretador e testes

O termo `RuntimeAdapter` deve ser usado para linguagens compiladas e interpretadas.

```rust
#[async_trait::async_trait]
pub trait RuntimeAdapter: Send + Sync {
    fn supported_language(&self) -> LanguageId;

    async fn run(
        &self,
        request: ExecutionRequest,
    ) -> Result<ExecutionResult, ToolchainError>;
}

#[async_trait::async_trait]
pub trait TestAdapter: Send + Sync {
    fn supported_language(&self) -> LanguageId;

    async fn run_tests(
        &self,
        request: TestRequest,
    ) -> Result<TestResult, ToolchainError>;
}
```

`CompilationRequest`, `ExecutionRequest` e `TestRequest` carregam explicitamente
a instalação selecionada, diretório de trabalho, classpath e argumentos. Os
resultados preservam código de saída, `stdout` e `stderr`, sem depender de texto
renderizado na interface.

## Contrato de build system

```rust
#[async_trait::async_trait]
pub trait BuildSystemAdapter: Send + Sync {
    fn build_system_id(&self) -> BuildSystemId;

    async fn detect_project(
        &self,
        root: &Path,
    ) -> Result<Option<ProjectDescriptor>, BuildError>;

    async fn import_project(
        &self,
        request: ProjectImportRequest,
    ) -> Result<ProjectModel, BuildError>;

    async fn execute(
        &self,
        request: BuildCommandRequest,
    ) -> Result<BuildCommandResult, BuildError>;
}
```

## Contrato de depuração

A depuração é a forma primária de integração com servidores e processos
externos. O contrato descreve uma sessão conectada a um alvo já em execução com
depuração habilitada; nada nele identifica um servidor, um container ou um
protocolo específico.

```rust
pub struct DebugTarget {
    pub host: String,
    pub port: u16,
}

pub struct DebugSessionRequest {
    pub target: DebugTarget,
    /// Raízes usadas para mapear as posições recebidas para arquivos do workspace.
    pub source_roots: Vec<PathBuf>,
    pub connect_timeout: Option<Duration>,
}

pub struct SourceBreakpoint {
    pub path: PathBuf,
    pub line: u32,
    pub condition: Option<String>,
}

pub struct ResolvedBreakpoint {
    pub id: BreakpointId,
    pub requested: SourceBreakpoint,
    /// Um breakpoint pode ser aceito, movido para outra linha ou recusado.
    pub verified_line: Option<u32>,
    pub message: Option<String>,
}

pub enum StepKind {
    Into,
    Over,
    Out,
}

pub enum StopReason {
    Breakpoint(BreakpointId),
    Step,
    Exception,
    Pause,
}

pub struct StackFrame {
    pub id: FrameId,
    pub name: String,
    /// Ausente quando o quadro não tem fonte no workspace.
    pub location: Option<Location>,
}

pub struct Variable {
    pub name: String,
    pub value: String,
    pub type_name: Option<String>,
    pub children: usize,
}

pub struct Variable {
    pub name: String,
    pub value: String,
    pub type_name: Option<String>,
    /// Indica que `DebugSession::expand` pode revelar campos deste valor.
    pub expandable: bool,
}

pub enum DebugEvent {
    Attached { description: String },
    Stopped { thread: ThreadId, reason: StopReason },
    Resumed { thread: ThreadId },
    Output { text: String },
    Detached { reason: Option<String> },
}

/// Destino dos eventos assíncronos. O adapter empurra; quem consome decide
/// como entregá-los à interface, sem que o contrato imponha um runtime.
pub trait DebugEventSink: Send + Sync {
    fn emit(&self, event: DebugEvent);
}

#[async_trait::async_trait]
pub trait DebugAdapter: Send + Sync {
    fn debug_adapter_id(&self) -> DebugAdapterId;

    fn supported_language(&self) -> LanguageId;

    async fn attach(
        &self,
        request: DebugSessionRequest,
        events: Arc<dyn DebugEventSink>,
    ) -> Result<Box<dyn DebugSession>, DebugError>;
}

#[async_trait::async_trait]
pub trait DebugSession: Send + Sync {
    /// Substitui o conjunto de breakpoints do arquivo informado.
    async fn set_breakpoints(
        &self,
        path: &Path,
        breakpoints: &[SourceBreakpoint],
    ) -> Result<Vec<ResolvedBreakpoint>, DebugError>;

    async fn threads(&self) -> Result<Vec<ThreadDescriptor>, DebugError>;

    async fn stack_trace(&self, thread: ThreadId) -> Result<Vec<StackFrame>, DebugError>;

    async fn variables(
        &self,
        thread: ThreadId,
        frame: FrameId,
    ) -> Result<Vec<Variable>, DebugError>;

    /// Campos de um valor já apresentado, endereçado pelo caminho da expressão.
    async fn expand(
        &self,
        thread: ThreadId,
        frame: FrameId,
        path: &str,
    ) -> Result<Vec<Variable>, DebugError>;

    async fn evaluate(
        &self,
        thread: ThreadId,
        frame: FrameId,
        expression: &str,
    ) -> Result<Variable, DebugError>;

    async fn step(&self, thread: ThreadId, kind: StepKind) -> Result<(), DebugError>;

    async fn resume(&self, thread: Option<ThreadId>) -> Result<(), DebugError>;

    async fn pause(&self, thread: ThreadId) -> Result<(), DebugError>;

    async fn detach(&self) -> Result<(), DebugError>;
}
```

Quadro e thread andam juntos porque um quadro só existe dentro da thread que o
empilhou; pedir variáveis sem dizer de qual thread não teria resposta única.

Regras específicas da depuração:

- a sessão apenas se conecta a um alvo já em execução; iniciar ou parar o
  servidor não faz parte deste contrato;
- inspecionar um valor nunca executa código no alvo: invocar métodos mudaria o
  estado do programa depurado e precisa ser uma decisão explícita do usuário,
  não um efeito colateral de olhar uma variável;
- o adapter nunca carrega bibliotecas do alvo no processo da IDE;
- posições chegam como `Location` do domínio, e o mapeamento entre o alvo e os
  arquivos do workspace é responsabilidade do adapter;
- perder a conexão produz `Detached` com motivo, e não derruba a IDE;
- todas as operações são canceláveis e executam fora da thread da interface.

## Regras

- nenhum contrato deve retornar tipos específicos de Java;
- nenhum contrato deve nomear um servidor, container ou protocolo concreto;
- erros devem ser tipados;
- contratos assíncronos devem aceitar cancelamento;
- operações longas devem informar progresso;
- implementações devem ser substituíveis;
- contratos públicos devem possuir versionamento.

## Navegação originada no editor

A camada de apresentação deve emitir uma solicitação neutra ao detectar
`Ctrl+Click` sobre um token:

```rust
pub struct NavigationRequest {
    pub document_id: DocumentId,
    pub byte_offset: usize,
    pub token: String,
}
```

O editor não resolve símbolos. O Language Host transforma essa solicitação em
`DefinitionRequest` para o provider ativo. O resultado volta como `Location` e
é aplicado pela operação genérica:

```rust
fn open_location(location: Location) -> Result<(), NavigationError>;
```

Na Fase 4, a aplicação converte a posição do `Ctrl+Click` em
`DefinitionRequest`, consulta o Language Host e abre a primeira localização
resolvida. Ao manter `Ctrl` pressionado sobre um span classificado como tipo
navegável, a apresentação usa o cursor de mão apontando como indicação visual.
A resolução continua fora da camada de UI.
