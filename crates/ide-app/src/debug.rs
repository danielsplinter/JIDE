//! Sessão de depuração conduzida fora da thread da interface.
//!
//! A janela nunca espera pelo alvo: ela envia pedidos por um canal e recebe
//! eventos já prontos para apresentação. Assim uma parada, um passo ou a queda
//! da conexão não travam o desenho nem a digitação.

use std::{
    collections::HashMap,
    net::{TcpStream, ToSocketAddrs},
    path::PathBuf,
    sync::{
        Arc, Mutex,
        mpsc::{Receiver, Sender, TryRecvError, channel},
    },
    thread,
    time::Duration,
};

use ide_debug_api::{
    DebugAdapter, DebugEvent, DebugEventSink, DebugSession, DebugSessionRequest, DebugTarget,
    SourceBreakpoint, StepKind, StopReason, ThreadId,
};
use ide_ui::{DebugFrameView, DebugVariableView};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

pub(crate) enum DebugCommand {
    Attach {
        host: String,
        port: u16,
        source_roots: Vec<PathBuf>,
        /// Tentativas de conexão, uma por segundo. Subir uma aplicação leva
        /// tempo, e a porta só abre quando a JVM já está de pé.
        attempts: u32,
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
    /// Avalia uma expressão no quadro escolhido e relata o valor.
    Evaluate {
        thread: ThreadId,
        frame: usize,
        expression: String,
    },
    /// Revela os campos de um valor já inspecionado.
    ExpandInspection {
        thread: ThreadId,
        frame: usize,
        path: String,
    },
}

pub(crate) enum DebugUiEvent {
    Session(DebugEvent),
    /// Linhas que o alvo confirmou para um arquivo, na ordem pedida.
    Breakpoints {
        path: PathBuf,
        verified: Vec<u32>,
    },
    View {
        thread: ThreadId,
        frames: Vec<DebugFrameView>,
        variables: Vec<DebugVariableView>,
        selected: usize,
    },
    /// Resultado de uma inspeção: o valor pedido e os campos do primeiro nível.
    Inspection {
        expression: String,
        value: DebugVariableView,
        fields: Vec<DebugVariableView>,
    },
    /// Campos revelados para um caminho já presente na árvore de inspeção.
    InspectionFields {
        path: String,
        fields: Vec<DebugVariableView>,
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
    pub(crate) fn start(adapter: Arc<dyn DebugAdapter>) -> Option<Self> {
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
                runtime.block_on(worker(adapter, command_receiver, event_sender));
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

async fn worker(
    adapter: Arc<dyn DebugAdapter>,
    mut commands: UnboundedReceiver<DebugCommand>,
    ui: Sender<DebugUiEvent>,
) {
    let sink: Arc<dyn DebugEventSink> = Arc::new(ChannelSink {
        events: Mutex::new(ui.clone()),
    });
    let mut session: Option<Box<dyn DebugSession>> = None;
    // O worker guarda os breakpoints marcados, com ou sem sessão. Marcar antes
    // de conectar é o fluxo normal — a aplicação ainda está subindo — e o pedido
    // não pode se perder por chegar cedo.
    let mut breakpoints: HashMap<PathBuf, Vec<u32>> = HashMap::new();

    while let Some(command) = commands.recv().await {
        match command {
            DebugCommand::Attach {
                host,
                port,
                source_roots,
                attempts,
            } => {
                if let Some(previous) = session.take() {
                    let _ = previous.detach().await;
                }
                let attempts = attempts.max(1);
                let mut last_error = None;
                for attempt in 0..attempts {
                    let request = DebugSessionRequest::new(DebugTarget::new(host.clone(), port))
                        .with_source_roots(source_roots.clone());
                    match adapter.attach(request, Arc::clone(&sink)).await {
                        Ok(attached) => {
                            session = Some(attached);
                            break;
                        }
                        Err(error) => {
                            last_error = Some(error);
                            if attempt + 1 < attempts {
                                tokio::time::sleep(Duration::from_secs(1)).await;
                            }
                        }
                    }
                }
                match (&session, last_error) {
                    (Some(active), _) => {
                        // Tudo que já estava marcado entra na sessão nova.
                        for (path, lines) in &breakpoints {
                            apply_breakpoints(active.as_ref(), path, lines, &ui).await;
                        }
                    }
                    (None, Some(error)) => {
                        let _ = ui.send(DebugUiEvent::Status(format!(
                            "Falha ao conectar em {host}:{port}: {error}"
                        )));
                    }
                    (None, None) => {}
                }
            }
            DebugCommand::SetBreakpoints { path, lines } => {
                if lines.is_empty() {
                    breakpoints.remove(&path);
                } else {
                    breakpoints.insert(path.clone(), lines.clone());
                }
                match session.as_ref() {
                    Some(active) => apply_breakpoints(active.as_ref(), &path, &lines, &ui).await,
                    None => {
                        let total: usize = breakpoints.values().map(Vec::len).sum();
                        let _ = ui.send(DebugUiEvent::Status(format!(
                            "{total} breakpoint(s) marcados; serão registrados ao conectar"
                        )));
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
            DebugCommand::Evaluate {
                thread,
                frame,
                expression,
            } => {
                let Some(active) = session.as_ref() else {
                    continue;
                };
                let event =
                    match evaluate_in_frame(active.as_ref(), thread, frame, &expression).await {
                        Ok((value, fields)) => DebugUiEvent::Inspection {
                            expression,
                            value,
                            fields,
                        },
                        Err(error) => DebugUiEvent::Status(error),
                    };
                let _ = ui.send(event);
            }
            DebugCommand::ExpandInspection {
                thread,
                frame,
                path,
            } => {
                let Some(active) = session.as_ref() else {
                    continue;
                };
                let event = match expand_inspection(active.as_ref(), thread, frame, &path).await {
                    Ok(fields) => DebugUiEvent::InspectionFields { path, fields },
                    Err(error) => DebugUiEvent::Status(error),
                };
                let _ = ui.send(event);
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

/// Avalia uma expressão no quadro pedido e descreve o resultado.
///
/// O quadro chega por índice, que é como a interface o conhece; o identificador
/// que o alvo entende sai da pilha atual, porque ela pode ter mudado desde a
/// última leitura.
async fn evaluate_in_frame(
    session: &dyn DebugSession,
    thread: ThreadId,
    frame: usize,
    expression: &str,
) -> Result<(DebugVariableView, Vec<DebugVariableView>), String> {
    let frames = session
        .stack_trace(thread)
        .await
        .map_err(|error| error.to_string())?;
    let selected = frames
        .get(frame.min(frames.len().saturating_sub(1)))
        .ok_or_else(|| "Nenhum quadro para inspecionar".to_owned())?;
    let value = session
        .evaluate(thread, selected.id, expression)
        .await
        .map_err(|error| format!("{expression}: {error}"))?;
    // O primeiro nível vem junto: pedir para inspecionar um objeto e receber só
    // ele fechado seria uma resposta pela metade. Os níveis seguintes esperam a
    // expansão, porque o grafo de um objeto pode ser fundo e cíclico.
    let fields = if value.expandable {
        expand_fields(session, thread, selected.id, expression).await
    } else {
        Vec::new()
    };
    Ok((variable_view(expression, &value), fields))
}

/// Campos de um valor endereçado pelo caminho, no quadro pedido.
async fn expand_inspection(
    session: &dyn DebugSession,
    thread: ThreadId,
    frame: usize,
    path: &str,
) -> Result<Vec<DebugVariableView>, String> {
    let frames = session
        .stack_trace(thread)
        .await
        .map_err(|error| error.to_string())?;
    let selected = frames
        .get(frame.min(frames.len().saturating_sub(1)))
        .ok_or_else(|| "Nenhum quadro para inspecionar".to_owned())?;
    Ok(expand_fields(session, thread, selected.id, path).await)
}

async fn expand_fields(
    session: &dyn DebugSession,
    thread: ThreadId,
    frame: ide_debug_api::FrameId,
    path: &str,
) -> Vec<DebugVariableView> {
    session
        .expand(thread, frame, path)
        .await
        .unwrap_or_default()
        .iter()
        .map(|field| variable_view(&field.name, field))
        .collect()
}

fn variable_view(name: &str, value: &ide_debug_api::Variable) -> DebugVariableView {
    DebugVariableView {
        name: name.to_owned(),
        value: value.value.clone(),
        type_name: value.type_name.clone(),
        expandable: value.expandable,
    }
}

/// Registra os breakpoints de um arquivo e informa o resultado à interface.
async fn apply_breakpoints(
    session: &dyn DebugSession,
    path: &std::path::Path,
    lines: &[u32],
    ui: &Sender<DebugUiEvent>,
) {
    let requested: Vec<SourceBreakpoint> = lines
        .iter()
        .map(|line| SourceBreakpoint::new(path, *line))
        .collect();
    match session.set_breakpoints(path, &requested).await {
        Ok(resolved) => {
            let verified: Vec<u32> = resolved
                .iter()
                .filter_map(|breakpoint| breakpoint.verified_line)
                .collect();
            let pending = resolved.len() - verified.len();
            let mut status = format!("Breakpoints: {} ativos", verified.len());
            if pending > 0 {
                status.push_str(&format!(", {pending} aguardando a classe carregar"));
            }
            let _ = ui.send(DebugUiEvent::Breakpoints {
                path: path.to_path_buf(),
                verified,
            });
            let _ = ui.send(DebugUiEvent::Status(status));
        }
        Err(error) => {
            let _ = ui.send(DebugUiEvent::Status(error.to_string()));
        }
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
                expandable: variable.expandable,
            })
            .collect(),
        selected,
    })
}

/// Verifica rapidamente se já existe algo escutando no alvo.
///
/// Serve para não subir uma segunda instância quando o servidor já está de pé —
/// o caso de um contêiner, de uma máquina remota ou de uma aplicação que o
/// usuário mesmo iniciou.
pub(crate) fn port_is_open(host: &str, port: u16) -> bool {
    let Ok(addresses) = (host, port).to_socket_addrs() else {
        return false;
    };
    addresses
        .into_iter()
        .any(|address| TcpStream::connect_timeout(&address, Duration::from_millis(200)).is_ok())
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
