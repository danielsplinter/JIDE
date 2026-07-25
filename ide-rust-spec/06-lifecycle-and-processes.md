# 06 — Ciclo de Vida e Processos

## Objetivo

Evitar que todos os componentes permaneçam carregados durante toda a sessão.

## Serviços

Cada serviço deverá possuir ciclo de vida explícito:

```rust
#[async_trait::async_trait]
pub trait ManagedService: Send + Sync {
    async fn start(&self) -> Result<(), ServiceError>;
    async fn suspend(&self) -> Result<(), ServiceError>;
    async fn resume(&self) -> Result<(), ServiceError>;
    async fn stop(&self) -> Result<(), ServiceError>;
    fn health(&self) -> ServiceHealth;
}
```

## Políticas de ativação

Exemplos:

- ativar Java ao abrir projeto Java;
- ativar parser ao abrir arquivo;
- ativar semântica após parsing;
- ativar debugger somente ao iniciar depuração;
- ativar Maven somente ao importar ou executar build;
- suspender indexação após ociosidade;
- descarregar provider quando nenhum workspace o utiliza.

## Process Supervisor

```rust
#[async_trait::async_trait]
pub trait ProcessSupervisor: Send + Sync {
    async fn spawn(
        &self,
        request: ProcessRequest,
    ) -> Result<ProcessHandle, ProcessError>;

    async fn terminate(
        &self,
        process_id: ProcessId,
    ) -> Result<(), ProcessError>;

    async fn status(
        &self,
        process_id: ProcessId,
    ) -> Result<ProcessStatus, ProcessError>;
}
```

## Isolamento recomendado

Processo principal:

```text
IDE UI
Application Core
Document Model
Command Router
```

Processos auxiliares:

```text
Language Worker
Index Worker
Plugin Host
Build Worker
Debug Worker
```

## Comunicação

Preferir mensagens tipadas:

```rust
pub enum LanguageWorkerRequest {
    OpenDocument(OpenDocumentRequest),
    ChangeDocument(ChangeDocumentRequest),
    Completion(CompletionRequest),
    Diagnostics(DiagnosticsRequest),
    Shutdown,
}
```

Evitar compartilhamento de estruturas mutáveis entre processos.

## Cancelamento

Toda operação longa deve aceitar:

```rust
pub struct RequestContext {
    pub request_id: RequestId,
    pub cancellation: CancellationToken,
    pub deadline: Option<Instant>,
}
```

## Backpressure

O sistema deve evitar filas ilimitadas.

Exemplo:

```text
Usuário digita 20 caracteres rapidamente
    ↓
19 análises intermediárias são canceladas
    ↓
somente o snapshot mais recente é analisado
```
