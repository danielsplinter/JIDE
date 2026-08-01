//! Controllers de aplicação com dependências explícitas e sem conhecimento de Winit.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{
        Arc,
        mpsc::{Receiver, Sender},
    },
    thread,
    time::{Instant, SystemTime},
};

use ide_application::{
    ContributionRegistry, IdeEvent, TaskController as ApplicationTaskController,
    TaskExecutionContext, TaskId, ToolchainRegistry,
};
use ide_domain::{DocumentId, DocumentSnapshot, SyntaxSnapshot};
use ide_language_host::{LanguageHost, LanguageHostError};
use ide_project::{
    build::{BuildCommandRequest, BuildSystemAdapter, BuildSystemRegistry},
    model::{ProjectDescriptor, ProjectModel},
};
use ide_ui::DebugView;
use ide_workspace::{FileNode, WorkspaceError, WorkspaceService};
use tokio::sync::oneshot;
use ui_core::Point;
use ui_render_wgpu::WgpuRenderer;
use ui_window_winit::WinitWindow;

use crate::{
    bootstrap::default_goals,
    bridges::{ToolEvent, document_change},
    debug,
    window::ClickTracker,
};

#[derive(Default)]
pub(super) struct NativeWindowState {
    pub(super) window: Option<WinitWindow>,
    pub(super) renderer: Option<WgpuRenderer>,
    pub(super) cursor: Point,
    pub(super) click_tracker: ClickTracker,
    pub(super) control_pressed: bool,
    pub(super) shift_pressed: bool,
    /// Botão primário segurado, o que caracteriza um arrasto em curso.
    ///
    /// É o que autoriza o relógio a continuar rolando a seleção; sem essa
    /// guarda, uma soltura perdida — a janela some do foco no meio do gesto —
    /// deixaria a vista rolando sozinha para sempre.
    pub(super) primary_pressed: bool,
}

#[derive(Default)]
pub(super) struct WorkspaceController {
    pub(super) service: WorkspaceService,
}

impl WorkspaceController {
    /// Os arquivos de uma extensão sob a raiz, lidos do filesystem.
    pub(super) fn source_files(&self, root: &Path, extension: &str) -> Vec<std::path::PathBuf> {
        self.service.source_files(root, extension)
    }

    /// Os níveis até uma pasta, da raiz para ela.
    pub(super) fn scan_path(
        &self,
        root: &Path,
        target: &Path,
    ) -> Vec<(std::path::PathBuf, Vec<FileNode>)> {
        self.service.scan_path(root, target)
    }

    pub(super) fn scan(&self, root: &Path) -> Result<FileNode, WorkspaceError> {
        self.service.scan(root)
    }

    pub(super) fn read_document(&self, path: &Path) -> Result<String, WorkspaceError> {
        self.service.read_document(path)
    }

    /// Move um arquivo do workspace, para seguir um tipo renomeado.
    pub(super) fn rename_path(&self, from: &Path, to: &Path) -> Result<(), WorkspaceError> {
        self.service.rename_path(from, to)
    }

    pub(super) fn save_document(&self, path: &Path, text: &str) -> Result<(), WorkspaceError> {
        self.service.save_document(path, text)
    }

    pub(super) fn modified_at(&self, path: &Path) -> Option<SystemTime> {
        self.service.modified_at(path)
    }
}

#[derive(Default)]
pub(super) struct DocumentController {
    pub(super) language: HashMap<DocumentId, DocumentSnapshot>,
    pub(super) application: HashMap<DocumentId, DocumentSnapshot>,
    pub(super) remembered: Vec<PathBuf>,
}

impl DocumentController {
    pub(super) fn clear(&mut self) {
        self.language.clear();
        self.application.clear();
        self.remembered.clear();
    }

    pub(super) fn synchronize_application(
        &mut self,
        snapshots: &[DocumentSnapshot],
    ) -> Vec<IdeEvent> {
        let open_ids = snapshots
            .iter()
            .map(|snapshot| snapshot.id)
            .collect::<std::collections::HashSet<_>>();
        let closed = self
            .application
            .keys()
            .filter(|id| !open_ids.contains(id))
            .copied()
            .collect::<Vec<_>>();
        let mut events = Vec::new();
        for document_id in closed {
            self.application.remove(&document_id);
            events.push(IdeEvent::DocumentClosed(document_id));
        }
        for snapshot in snapshots {
            let event = match self.application.get(&snapshot.id) {
                None => Some(IdeEvent::DocumentOpened {
                    document_id: snapshot.id,
                    path: snapshot.path.clone(),
                }),
                Some(previous) if previous.version != snapshot.version => {
                    Some(IdeEvent::DocumentChanged {
                        document_id: snapshot.id,
                        version: snapshot.version,
                    })
                }
                Some(_) => None,
            };
            self.application.insert(snapshot.id, snapshot.clone());
            events.extend(event);
        }
        events
    }
}

#[derive(Default)]
pub(super) struct LanguageController {
    pub(super) host: Option<LanguageHost>,
    pub(super) contributions: ContributionRegistry,
    pub(super) toolchains: ToolchainRegistry,
    /// Realces pedidos e ainda não entregues.
    ///
    /// Guardá-los é o que permite não esperar: a tecla posta o pedido, e o
    /// resultado é colhido quando o provider terminar.
    pending_syntax: Vec<oneshot::Receiver<Result<SyntaxSnapshot, LanguageHostError>>>,
}

impl LanguageController {
    /// Recolhe os realces que já chegaram do provider, sem esperar por nenhum.
    ///
    /// A análise roda na thread do provider desde sempre; o que a punha no meio
    /// da digitação era esperar por ela. Agora a tecla posta o pedido e o
    /// resultado é colhido aqui, um ou dois quadros depois — o editor já ignora
    /// realce de revisão vencida, então chegar tarde não desenha nada errado.
    pub(super) fn collect_syntax(&mut self) -> Vec<SyntaxSnapshot> {
        let mut prontos = Vec::new();
        self.pending_syntax.retain_mut(|receiver| {
            match receiver.try_recv() {
                Ok(Ok(snapshot)) => {
                    prontos.push(snapshot);
                    false
                }
                Ok(Err(error)) => {
                    tracing::warn!(%error, "syntax snapshot failed");
                    false
                }
                // Vazio significa que o provider ainda está analisando.
                Err(oneshot::error::TryRecvError::Empty) => true,
                Err(oneshot::error::TryRecvError::Closed) => false,
            }
        });
        prontos
    }

    /// Quantos realces ainda estão sendo esperados.
    ///
    /// Só os testes precisam disso: é como se afirma que a tecla deixou a
    /// análise pendente em vez de esperar por ela.
    #[cfg(test)]
    pub(super) fn pending_syntax(&self) -> usize {
        self.pending_syntax.len()
    }

    pub(super) fn synchronize_documents(
        &mut self,
        documents: &mut DocumentController,
        snapshots: &[DocumentSnapshot],
    ) -> Vec<SyntaxSnapshot> {
        let Some(host) = self.host.as_ref() else {
            return Vec::new();
        };
        let open_ids = snapshots
            .iter()
            .map(|snapshot| snapshot.id)
            .collect::<std::collections::HashSet<_>>();
        let closed = documents
            .language
            .keys()
            .filter(|id| !open_ids.contains(id))
            .copied()
            .collect::<Vec<_>>();
        for document_id in closed {
            let _ = pollster::block_on(host.close_document(host.request_context(), document_id));
            documents.language.remove(&document_id);
        }

        let mut syntax = Vec::new();
        for snapshot in snapshots {
            if !snapshot
                .path
                .extension()
                .and_then(|value| value.to_str())
                .is_some_and(|extension| extension.eq_ignore_ascii_case("java"))
            {
                continue;
            }
            let changed = documents
                .language
                .get(&snapshot.id)
                .is_none_or(|previous| previous.version != snapshot.version);
            // Abrir é raro e o resto depende dele: esse ainda espera. A mudança,
            // que é o que acontece a cada tecla, apenas entra na fila.
            let result = match documents.language.get(&snapshot.id) {
                None => {
                    pollster::block_on(host.open_document(host.request_context(), snapshot.clone()))
                        .map(|_| ())
                }
                Some(previous) if previous.version != snapshot.version => {
                    let change = document_change(previous, snapshot);
                    host.post_change_document(host.request_context(), change)
                        .map(|_| ())
                }
                Some(_) => Ok(()),
            };
            if let Err(error) = result {
                // A fila cheia deixa o documento como estava do lado do provider,
                // e é por isso que o registro **não** avança: a próxima
                // sincronização calcula a diferença do mesmo ponto e tenta de
                // novo, com um pedaço maior.
                tracing::warn!(%error, document_id = snapshot.id.0, "syntax update failed");
                continue;
            }
            documents.language.insert(snapshot.id, snapshot.clone());
            if changed {
                match host.post_syntax(host.request_context(), snapshot.id) {
                    Ok(receiver) => self.pending_syntax.push(receiver),
                    Err(error) => {
                        tracing::warn!(%error, document_id = snapshot.id.0, "syntax snapshot failed");
                    }
                }
            }
        }
        // O que já estava pronto de rodadas anteriores entra agora.
        syntax.extend(self.collect_syntax());
        syntax
    }
}

pub(super) struct ImportedProject {
    pub(super) adapter: Arc<dyn BuildSystemAdapter>,
    pub(super) descriptor: ProjectDescriptor,
    pub(super) model: ProjectModel,
    pub(super) manifest_modified: Option<SystemTime>,
}

/// Instalações do Maven encontradas e a escolhida.
///
/// Fica junto do projeto porque é dele que o Maven serve: compilar, importar e
/// executar passam pelo mesmo executável.
#[derive(Default)]
pub(super) struct MavenController {
    pub(super) installations: Vec<java_maven_adapter::MavenInstallation>,
    pub(super) selected: Option<usize>,
}

impl MavenController {
    /// Casa do Maven escolhido, se houver escolha válida.
    pub(super) fn home(&self) -> Option<PathBuf> {
        self.selected
            .and_then(|index| self.installations.get(index))
            .map(|instalacao| instalacao.home.clone())
    }

    /// Rótulos para a lista da janela de configurações.
    pub(super) fn labels(&self) -> Vec<String> {
        self.installations
            .iter()
            .map(java_maven_adapter::MavenInstallation::label)
            .collect()
    }

    /// Põe uma instalação na lista, sem repetir, e a deixa escolhida.
    pub(super) fn adopt(&mut self, instalacao: java_maven_adapter::MavenInstallation) -> usize {
        let indice = self
            .installations
            .iter()
            .position(|outra| outra.home == instalacao.home)
            .unwrap_or_else(|| {
                self.installations.push(instalacao);
                self.installations.len() - 1
            });
        self.selected = Some(indice);
        indice
    }
}

#[derive(Default)]
pub(super) struct ProjectController {
    pub(super) build_systems: BuildSystemRegistry,
    /// Instalações do Maven e a escolhida.
    pub(super) maven: MavenController,
    pub(super) imported: Option<ImportedProject>,
    pub(super) last_manifest_check: Option<Instant>,
}

impl ProjectController {
    pub(super) fn reset_import(&mut self) -> Option<ImportedProject> {
        self.last_manifest_check = None;
        self.imported.take()
    }

    pub(super) fn build_plan(
        &self,
        java_home: Option<&Path>,
    ) -> Option<(Arc<dyn BuildSystemAdapter>, BuildCommandRequest, String)> {
        let project = self.imported.as_ref()?;
        let mut request = BuildCommandRequest::new(
            project.descriptor.clone(),
            default_goals(&project.descriptor),
        );
        if let Some(home) = java_home {
            request =
                request.with_environment_variable("JAVA_HOME", home.to_string_lossy().into_owned());
        }
        let label = format!(
            "[{}] {}",
            project.descriptor.build_system.label(),
            request.goals.join(" ")
        );
        Some((project.adapter.clone(), request, label))
    }
}

#[derive(Default)]
pub(super) struct TaskController {
    pub(super) controller: ApplicationTaskController,
    pub(super) events: Option<Receiver<ToolEvent>>,
    pub(super) sender: Option<Sender<ToolEvent>>,
}

impl TaskController {
    fn spawn(
        &self,
        name: &str,
        label: String,
        work: impl FnOnce(&tokio::runtime::Runtime) -> ToolEvent + Send + 'static,
    ) -> Result<(), String> {
        let sender = self
            .sender
            .clone()
            .ok_or_else(|| "canal de ferramentas indisponível".to_owned())?;
        thread::Builder::new()
            .name(name.to_owned())
            .spawn(move || {
                let runtime = match tokio::runtime::Builder::new_current_thread()
                    .enable_time()
                    .build()
                {
                    Ok(runtime) => runtime,
                    Err(error) => {
                        let _ = sender.send(ToolEvent {
                            status: format!("{label} falhou"),
                            stdout: String::new(),
                            stderr: error.to_string(),
                        });
                        return;
                    }
                };
                let _ = sender.send(work(&runtime));
            })
            .map(|_| ())
            .map_err(|error| error.to_string())
    }

    pub(super) fn execute_build(
        &self,
        adapter: Arc<dyn BuildSystemAdapter>,
        request: BuildCommandRequest,
        label: String,
    ) -> Result<(), String> {
        let error_label = label.clone();
        self.spawn("project-build", label.clone(), move |runtime| match runtime
            .block_on(adapter.execute(request))
        {
            Ok(result) => ToolEvent {
                status: if result.success {
                    format!("{label} concluído")
                } else {
                    format!("{label} falhou ({})", result.exit_code)
                },
                stdout: format!("{}\n{}", result.command_line, result.stdout),
                stderr: result.stderr,
            },
            Err(error) => ToolEvent {
                status: format!("{error_label} falhou"),
                stdout: String::new(),
                stderr: error.to_string(),
            },
        })
    }

    pub(super) fn execute_task(
        &self,
        task_id: TaskId,
        context: TaskExecutionContext,
        label: String,
    ) -> Result<(), String> {
        let controller = self.controller.clone();
        let error_label = label.clone();
        self.spawn("language-task", label, move |runtime| {
            match runtime.block_on(controller.execute(&task_id, context)) {
                Ok(result) => ToolEvent {
                    status: result.status,
                    stdout: result.stdout,
                    stderr: result.stderr,
                },
                Err(error) => ToolEvent {
                    status: format!("{error_label} falhou"),
                    stdout: String::new(),
                    stderr: error.to_string(),
                },
            }
        })
    }
}

#[derive(Default)]
pub(super) struct DebugController {
    pub(super) session: Option<debug::DebugController>,
    pub(super) view: DebugView,
    pub(super) thread: Option<ide_debug_api::ThreadId>,
}

#[derive(Default)]
pub(super) struct RuntimeState {
    pub(super) startup_error: Option<String>,
    pub(super) config: ide_core::AppConfig,
    pub(super) config_path: Option<PathBuf>,
    /// A primeira sincronização de linguagens ainda não aconteceu.
    ///
    /// Ela ativa o provider, que indexa o JDK e os fontes do projeto — mais de
    /// um segundo. Feita antes do primeiro quadro, deixava a janela já visível
    /// em branco todo esse tempo. Depois do primeiro quadro, o usuário vê a IDE
    /// montada e o realce chega em seguida.
    pub(super) languages_pending: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use ide_domain::{DocumentId, DocumentSnapshot};

    fn snapshot(id: u64, version: u64) -> DocumentSnapshot {
        DocumentSnapshot {
            id: DocumentId(id),
            path: PathBuf::from(format!("{id}.java")),
            version,
            text: String::new(),
        }
    }

    #[test]
    fn document_controller_reports_open_change_and_close_without_a_window() {
        let mut controller = DocumentController::default();
        assert!(matches!(
            controller
                .synchronize_application(&[snapshot(1, 0)])
                .as_slice(),
            [IdeEvent::DocumentOpened { .. }]
        ));
        assert!(matches!(
            controller
                .synchronize_application(&[snapshot(1, 1)])
                .as_slice(),
            [IdeEvent::DocumentChanged { version: 1, .. }]
        ));
        assert!(matches!(
            controller.synchronize_application(&[]).as_slice(),
            [IdeEvent::DocumentClosed(DocumentId(1))]
        ));
    }
}
