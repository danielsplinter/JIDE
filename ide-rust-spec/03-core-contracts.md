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
    ) -> Result<CompilationResult, CompilationError>;
}
```

## Contrato de runtime ou interpretador

O termo `RuntimeAdapter` deve ser usado para linguagens compiladas e interpretadas.

```rust
#[async_trait::async_trait]
pub trait RuntimeAdapter: Send + Sync {
    fn runtime_id(&self) -> RuntimeId;

    fn supported_languages(&self) -> Vec<LanguageId>;

    async fn run(
        &self,
        request: RunRequest,
    ) -> Result<RunningProcess, RuntimeError>;

    async fn stop(
        &self,
        process_id: ProcessId,
    ) -> Result<(), RuntimeError>;
}
```

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

```rust
#[async_trait::async_trait]
pub trait DebugAdapter: Send + Sync {
    fn debug_adapter_id(&self) -> DebugAdapterId;

    async fn start_session(
        &self,
        request: DebugSessionRequest,
    ) -> Result<Box<dyn DebugSession>, DebugError>;
}
```

## Regras

- nenhum contrato deve retornar tipos específicos de Java;
- erros devem ser tipados;
- contratos assíncronos devem aceitar cancelamento;
- operações longas devem informar progresso;
- implementações devem ser substituíveis;
- contratos públicos devem possuir versionamento.
