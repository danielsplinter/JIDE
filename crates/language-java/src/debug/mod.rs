#![doc = "Adapter de depuração para processos Java em execução."]
#![doc = ""]
#![doc = "Conecta-se pela porta de depuração da JVM, seja qual for o servidor,"]
#![doc = "container ou ferramenta que a exponha. É o único módulo que conhece o"]
#![doc = "protocolo; tudo acima dele trabalha com os contratos de `ide-debug-api`."]

mod connection;
mod resolve;
mod session;
mod values;
mod wire;

use std::sync::Arc;

use async_trait::async_trait;
use ide_debug_api::{
    DebugAdapter, DebugAdapterId, DebugError, DebugEvent, DebugEventSink, DebugSession,
    DebugSessionRequest, FrameId, ResolvedBreakpoint, SourceBreakpoint, StackFrame, StepKind,
    ThreadDescriptor, ThreadId, Variable,
};
use ide_domain::LanguageId;

use crate::debug::{connection::Connection, session::Session};

pub const JAVA_DEBUG_ADAPTER_ID: &str = "java-jdwp";

#[derive(Default)]
pub struct JavaDebugAdapter;

impl JavaDebugAdapter {
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl DebugAdapter for JavaDebugAdapter {
    fn debug_adapter_id(&self) -> DebugAdapterId {
        DebugAdapterId(JAVA_DEBUG_ADAPTER_ID.to_owned())
    }

    fn supported_language(&self) -> LanguageId {
        LanguageId("java".to_owned())
    }

    async fn attach(
        &self,
        request: DebugSessionRequest,
        events: Arc<dyn DebugEventSink>,
    ) -> Result<Box<dyn DebugSession>, DebugError> {
        let (connection, mut composites) =
            Connection::attach(&request.target.address(), request.connect_timeout).await?;
        let session = Arc::new(Session::new(
            connection,
            request.source_roots,
            Arc::clone(&events),
        ));
        let description = session.version().await.unwrap_or_else(|_| "JVM".to_owned());
        events.emit(DebugEvent::Attached { description });

        let pump = Arc::clone(&session);
        tokio::spawn(async move {
            while let Some(payload) = composites.recv().await {
                pump.handle_composite(payload).await;
            }
            pump.notify_detached(Some("a conexão de depuração foi encerrada".to_owned()));
        });
        Ok(Box::new(SessionHandle { session }))
    }
}

/// Alça compartilhada com a tarefa que bombeia os eventos do alvo.
struct SessionHandle {
    session: Arc<Session>,
}

#[async_trait]
impl DebugSession for SessionHandle {
    async fn set_breakpoints(
        &self,
        path: &std::path::Path,
        breakpoints: &[SourceBreakpoint],
    ) -> Result<Vec<ResolvedBreakpoint>, DebugError> {
        self.session.set_breakpoints(path, breakpoints).await
    }

    async fn threads(&self) -> Result<Vec<ThreadDescriptor>, DebugError> {
        self.session.threads().await
    }

    async fn stack_trace(&self, thread: ThreadId) -> Result<Vec<StackFrame>, DebugError> {
        self.session.stack_trace(thread).await
    }

    async fn variables(
        &self,
        thread: ThreadId,
        frame: FrameId,
    ) -> Result<Vec<Variable>, DebugError> {
        self.session.variables(thread, frame).await
    }

    async fn expand(
        &self,
        thread: ThreadId,
        frame: FrameId,
        path: &str,
    ) -> Result<Vec<Variable>, DebugError> {
        self.session.expand(thread, frame, path).await
    }

    async fn evaluate(
        &self,
        thread: ThreadId,
        frame: FrameId,
        expression: &str,
    ) -> Result<Variable, DebugError> {
        self.session.evaluate(thread, frame, expression).await
    }

    async fn step(&self, thread: ThreadId, kind: StepKind) -> Result<(), DebugError> {
        self.session.step(thread, kind).await
    }

    async fn resume(&self, thread: Option<ThreadId>) -> Result<(), DebugError> {
        self.session.resume(thread).await
    }

    async fn pause(&self, thread: ThreadId) -> Result<(), DebugError> {
        self.session.pause(thread).await
    }

    async fn detach(&self) -> Result<(), DebugError> {
        self.session.detach().await
    }
}
