//! Sessão de depuração conduzida fora da thread da interface.
//!
//! A janela nunca espera pelo alvo: ela envia pedidos por um canal e recebe
//! eventos já prontos para apresentação. Assim uma parada, um passo ou a queda
//! da conexão não travam o desenho nem a digitação.

use std::{
    path::PathBuf,
    sync::{
        Arc, Mutex,
        mpsc::{Receiver, Sender, TryRecvError, channel},
    },
    thread,
};

use ide_debug_api::{
    DebugAdapter, DebugEvent, DebugEventSink, DebugSession, DebugSessionRequest, DebugTarget,
    SourceBreakpoint, StepKind, StopReason, ThreadId,
};
use ide_ui::{DebugFrameView, DebugVariableView};
use java_debug_adapter::JavaDebugAdapter;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

pub(crate) enum DebugCommand {
    Attach {
        host: String,
        port: u16,
        source_roots: Vec<PathBuf>,
    },
    SetBreakpoints {
        path: PathBuf,
        lines: Vec<u32>,
    },
    Step(StepKind),
    Continue,
    Pause,
    Detach,
    /// Recarrega pilha e variáveis do quadro escolhido.
    Refresh {
        thread: ThreadId,
        frame: usize,
    },
}

pub(crate) enum DebugUiEvent {
    Session(DebugEvent),
    View {
        thread: ThreadId,
        frames: Vec<DebugFrameView>,
        variables: Vec<DebugVariableView>,
        selected: usize,
    },
    Status(String),
}

struct ChannelSink {
    events: Mutex<Sender<DebugUiEvent>>,
}

impl DebugEventSink for ChannelSink {
    fn emit(&self, event: DebugEvent) {
        if let Ok(events) = self.events.lock() {
            let _ = events.send(DebugUiEvent::Session(event));
        }
    }
}

pub(crate) struct DebugController {
    commands: UnboundedSender<DebugCommand>,
    events: Receiver<DebugUiEvent>,
}

impl DebugController {
    pub(crate) fn start() -> Option<Self> {
        let (commands, command_receiver) = unbounded_channel();
        let (event_sender, events) = channel();
        thread::Builder::new()
            .name("debug-session".to_owned())
            .spawn(move || {
                let runtime = match tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    Ok(runtime) => runtime,
                    Err(error) => {
                        let _ = event_sender.send(DebugUiEvent::Status(error.to_string()));
                        return;
                    }
                };
                runtime.block_on(worker(command_receiver, event_sender));
            })
            .ok()?;
        Some(Self { commands, events })
    }

    pub(crate) fn send(&self, command: DebugCommand) {
        let _ = self.commands.send(command);
    }

    pub(crate) fn poll(&self) -> Vec<DebugUiEvent> {
        let mut events = Vec::new();
        loop {
            match self.events.try_recv() {
                Ok(event) => events.push(event),
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => return events,
            }
        }
    }
}

async fn worker(mut commands: UnboundedReceiver<DebugCommand>, ui: Sender<DebugUiEvent>) {
    let adapter = JavaDebugAdapter::new();
    let sink: Arc<dyn DebugEventSink> = Arc::new(ChannelSink {
        events: Mutex::new(ui.clone()),
    });
    let mut session: Option<Box<dyn DebugSession>> = None;

    while let Some(command) = commands.recv().await {
        match command {
            DebugCommand::Attach {
                host,
                port,
                source_roots,
            } => {
                if let Some(previous) = session.take() {
                    let _ = previous.detach().await;
                }
                let request = DebugSessionRequest::new(DebugTarget::new(host.clone(), port))
                    .with_source_roots(source_roots);
                match adapter.attach(request, Arc::clone(&sink)).await {
                    Ok(attached) => session = Some(attached),
                    Err(error) => {
                        let _ = ui.send(DebugUiEvent::Status(format!(
                            "Falha ao conectar em {host}:{port}: {error}"
                        )));
                    }
                }
            }
            DebugCommand::SetBreakpoints { path, lines } => {
                let Some(active) = session.as_ref() else {
                    continue;
                };
                let breakpoints: Vec<SourceBreakpoint> = lines
                    .iter()
                    .map(|line| SourceBreakpoint::new(&path, *line))
                    .collect();
                match active.set_breakpoints(&path, &breakpoints).await {
                    Ok(resolved) => {
                        let verified = resolved.iter().filter(|bp| bp.is_verified()).count();
                        let pending = resolved.len() - verified;
                        let mut status = format!("Breakpoints: {verified} ativos");
                        if pending > 0 {
                            status.push_str(&format!(", {pending} aguardando a classe"));
                        }
                        let _ = ui.send(DebugUiEvent::Status(status));
                    }
                    Err(error) => {
                        let _ = ui.send(DebugUiEvent::Status(error.to_string()));
                    }
                }
            }
            DebugCommand::Step(kind) => {
                if let Some(active) = session.as_ref()
                    && let Some(thread) = stopped_thread(active.as_ref()).await
                    && let Err(error) = active.step(thread, kind).await
                {
                    let _ = ui.send(DebugUiEvent::Status(error.to_string()));
                }
            }
            DebugCommand::Continue => {
                if let Some(active) = session.as_ref()
                    && let Err(error) = active.resume(None).await
                {
                    let _ = ui.send(DebugUiEvent::Status(error.to_string()));
                }
            }
            DebugCommand::Pause => {
                if let Some(active) = session.as_ref() {
                    match main_thread(active.as_ref()).await {
                        Some(thread) => {
                            if let Err(error) = active.pause(thread).await {
                                let _ = ui.send(DebugUiEvent::Status(error.to_string()));
                            }
                        }
                        None => {
                            let _ = ui.send(DebugUiEvent::Status(
                                "Nenhuma thread em execução para pausar".to_owned(),
                            ));
                        }
                    }
                }
            }
            DebugCommand::Detach => {
                if let Some(active) = session.take() {
                    let _ = active.detach().await;
                }
            }
            DebugCommand::Refresh { thread, frame } => {
                let Some(active) = session.as_ref() else {
                    continue;
                };
                match collect_view(active.as_ref(), thread, frame).await {
                    Ok(event) => {
                        let _ = ui.send(event);
                    }
                    Err(error) => {
                        let _ = ui.send(DebugUiEvent::Status(error));
                    }
                }
            }
        }
    }
    if let Some(active) = session.take() {
        let _ = active.detach().await;
    }
}

/// Thread a pausar: `main` quando existir, senão a primeira em execução.
///
/// Pausar uma thread arbitrária de um servidor não ajudaria ninguém; a thread
/// principal é a escolha previsível para uma aplicação de linha de comando, e
/// paradas em servidores vêm de breakpoints, não de pausa manual.
async fn main_thread(session: &dyn DebugSession) -> Option<ThreadId> {
    let threads = session.threads().await.ok()?;
    let running: Vec<_> = threads
        .into_iter()
        .filter(|thread| !thread.suspended)
        .collect();
    running
        .iter()
        .find(|thread| thread.name == "main")
        .or_else(|| running.first())
        .map(|thread| thread.id)
}

/// Thread suspensa em que os passos devem ser aplicados.
async fn stopped_thread(session: &dyn DebugSession) -> Option<ThreadId> {
    session
        .threads()
        .await
        .ok()?
        .into_iter()
        .find(|thread| thread.suspended)
        .map(|thread| thread.id)
}

async fn collect_view(
    session: &dyn DebugSession,
    thread: ThreadId,
    frame: usize,
) -> Result<DebugUiEvent, String> {
    let frames = session
        .stack_trace(thread)
        .await
        .map_err(|error| error.to_string())?;
    let selected = frame.min(frames.len().saturating_sub(1));
    let variables = match frames.get(selected) {
        Some(current) => session
            .variables(thread, current.id)
            .await
            .unwrap_or_default(),
        None => Vec::new(),
    };
    Ok(DebugUiEvent::View {
        thread,
        frames: frames
            .iter()
            .map(|frame| DebugFrameView {
                name: frame.name.clone(),
                location: frame
                    .location
                    .as_ref()
                    .map(|location| (location.path.clone(), location.range.start.line)),
            })
            .collect(),
        variables: variables
            .into_iter()
            .map(|variable| DebugVariableView {
                name: variable.name,
                value: variable.value,
                type_name: variable.type_name,
            })
            .collect(),
        selected,
    })
}

/// Texto curto para a barra de status a partir do motivo da parada.
pub(crate) fn stop_reason_label(reason: &StopReason) -> String {
    match reason {
        StopReason::Breakpoint(_) => "Parado no breakpoint".to_owned(),
        StopReason::Step => "Parado após o passo".to_owned(),
        StopReason::Exception(exception) => format!("Parado na exceção {exception}"),
        StopReason::Pause => "Pausado".to_owned(),
    }
}

/// Quadro do topo, usado para posicionar o editor quando a execução para.
pub(crate) fn first_location(frames: &[DebugFrameView]) -> Option<(PathBuf, u32)> {
    frames.iter().find_map(|frame| frame.location.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stop_reasons_become_readable_status_messages() {
        assert_eq!(
            stop_reason_label(&StopReason::Breakpoint(ide_debug_api::BreakpointId(1))),
            "Parado no breakpoint"
        );
        assert_eq!(
            stop_reason_label(&StopReason::Exception("NullPointerException".to_owned())),
            "Parado na exceção NullPointerException"
        );
        assert_eq!(stop_reason_label(&StopReason::Pause), "Pausado");
    }

    #[test]
    fn the_first_frame_with_source_positions_the_editor() {
        let frames = vec![
            DebugFrameView {
                name: "Thread.sleep".to_owned(),
                location: None,
            },
            DebugFrameView {
                name: "Main.run".to_owned(),
                location: Some((PathBuf::from("/w/Main.java"), 12)),
            },
        ];
        assert_eq!(
            first_location(&frames),
            Some((PathBuf::from("/w/Main.java"), 12))
        );
        assert!(first_location(&[]).is_none());
    }
}
