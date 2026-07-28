#![doc = "Contratos de depuração independentes de linguagem, servidor e protocolo."]

use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use async_trait::async_trait;
use ide_domain::{LanguageId, Location};
use thiserror::Error;

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct DebugAdapterId(pub String);

macro_rules! numeric_id {
    ($name:ident) => {
        #[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub struct $name(pub u64);
    };
}

numeric_id!(BreakpointId);
numeric_id!(ThreadId);
numeric_id!(FrameId);

/// Processo já em execução, identificado apenas por host e porta.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DebugTarget {
    pub host: String,
    pub port: u16,
}

impl DebugTarget {
    #[must_use]
    pub fn new(host: impl Into<String>, port: u16) -> Self {
        Self {
            host: host.into(),
            port,
        }
    }

    #[must_use]
    pub fn address(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DebugSessionRequest {
    pub target: DebugTarget,
    /// Raízes usadas para mapear as posições recebidas do alvo em arquivos do
    /// workspace.
    pub source_roots: Vec<PathBuf>,
    pub connect_timeout: Option<Duration>,
}

impl DebugSessionRequest {
    #[must_use]
    pub fn new(target: DebugTarget) -> Self {
        Self {
            target,
            source_roots: Vec::new(),
            connect_timeout: Some(Duration::from_secs(10)),
        }
    }

    #[must_use]
    pub fn with_source_roots(mut self, roots: Vec<PathBuf>) -> Self {
        self.source_roots = roots;
        self
    }
}

/// Linha é 0-based, como todo o domínio.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceBreakpoint {
    pub path: PathBuf,
    pub line: u32,
    pub condition: Option<String>,
}

impl SourceBreakpoint {
    #[must_use]
    pub fn new(path: impl Into<PathBuf>, line: u32) -> Self {
        Self {
            path: path.into(),
            line,
            condition: None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedBreakpoint {
    pub id: BreakpointId,
    pub requested: SourceBreakpoint,
    /// Linha efetiva no alvo. `None` quando o breakpoint não pôde ser instalado.
    pub verified_line: Option<u32>,
    pub message: Option<String>,
}

impl ResolvedBreakpoint {
    #[must_use]
    pub const fn is_verified(&self) -> bool {
        self.verified_line.is_some()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StepKind {
    Into,
    Over,
    Out,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StopReason {
    Breakpoint(BreakpointId),
    Step,
    Exception(String),
    Pause,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ThreadDescriptor {
    pub id: ThreadId,
    pub name: String,
    pub suspended: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StackFrame {
    pub id: FrameId,
    pub name: String,
    /// Ausente quando o quadro não tem fonte correspondente no workspace.
    pub location: Option<Location>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Variable {
    pub name: String,
    pub value: String,
    pub type_name: Option<String>,
    /// Indica que `DebugSession::expand` pode revelar campos deste valor.
    pub expandable: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DebugEvent {
    Attached {
        description: String,
    },
    Stopped {
        thread: ThreadId,
        reason: StopReason,
    },
    Resumed {
        thread: ThreadId,
    },
    Output {
        text: String,
    },
    Detached {
        reason: Option<String>,
    },
}

/// Destino dos eventos assíncronos da sessão.
///
/// O adapter empurra eventos; quem consome decide como entregá-los à interface.
/// Isso mantém o contrato livre de qualquer runtime assíncrono específico.
pub trait DebugEventSink: Send + Sync {
    fn emit(&self, event: DebugEvent);
}

#[async_trait]
pub trait DebugAdapter: Send + Sync {
    fn debug_adapter_id(&self) -> DebugAdapterId;

    fn supported_language(&self) -> LanguageId;

    /// Conecta-se a um alvo já em execução. Iniciar ou parar o processo não faz
    /// parte deste contrato.
    async fn attach(
        &self,
        request: DebugSessionRequest,
        events: Arc<dyn DebugEventSink>,
    ) -> Result<Box<dyn DebugSession>, DebugError>;
}

#[async_trait]
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

#[derive(Debug, Error)]
pub enum DebugError {
    #[error("could not connect to the debug target: {0}")]
    Connect(String),
    #[error("debug protocol failed: {0}")]
    Protocol(String),
    #[error("the target reported an error: {0}")]
    Target(String),
    #[error("the thread is not suspended")]
    NotSuspended,
    #[error("operation is not supported: {0}")]
    Unsupported(String),
    #[error("a sessão de depuração terminou; a aplicação não está mais parada")]
    Detached,
}

/// Mapeia um arquivo do workspace para a raiz de código que o contém.
///
/// Adapters usam isso para traduzir posições do alvo em `Location` do domínio e
/// vice-versa, sem embutir convenção de diretório de nenhuma linguagem.
#[must_use]
pub fn relative_to_source_root<'a>(path: &'a Path, source_roots: &[PathBuf]) -> Option<&'a Path> {
    source_roots
        .iter()
        .filter_map(|root| path.strip_prefix(root).ok())
        .max_by_key(|relative| relative.components().count())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_root_mapping_prefers_the_most_specific_root() {
        let roots = vec![
            PathBuf::from("/w/app/src/main/java"),
            PathBuf::from("/w/app/target/generated-sources/annotations"),
        ];
        let relative = relative_to_source_root(
            Path::new("/w/app/src/main/java/com/example/Main.java"),
            &roots,
        );
        assert_eq!(relative, Some(Path::new("com/example/Main.java")));

        let generated = relative_to_source_root(
            Path::new("/w/app/target/generated-sources/annotations/com/example/Gen.java"),
            &roots,
        );
        assert_eq!(generated, Some(Path::new("com/example/Gen.java")));

        assert!(relative_to_source_root(Path::new("/other/Main.java"), &roots).is_none());
    }

    #[test]
    fn target_address_and_breakpoint_defaults_are_stable() {
        assert_eq!(
            DebugTarget::new("127.0.0.1", 8000).address(),
            "127.0.0.1:8000"
        );
        let breakpoint = SourceBreakpoint::new("/w/Main.java", 41);
        assert_eq!(breakpoint.line, 41, "linhas são 0-based como no domínio");
        assert!(breakpoint.condition.is_none());
        assert!(
            !ResolvedBreakpoint {
                id: BreakpointId(1),
                requested: breakpoint,
                verified_line: None,
                message: Some("classe não carregada".to_owned()),
            }
            .is_verified()
        );
    }
}
