mod debug;
mod run;

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{
        Arc,
        mpsc::{self, Receiver, Sender},
    },
    thread,
    time::{Duration, Instant, SystemTime},
};

use ide_build_api::{
    BuildCommandRequest, BuildSystemAdapter, BuildSystemRegistry, ProjectImportRequest,
};
use ide_core::{AppConfig, config_path, init_logging};
use ide_debug_api::{DebugEvent, StepKind};
use ide_domain::{
    DefinitionRequest, DocumentChange, DocumentId, DocumentSnapshot, ProviderId, TextPosition,
    TextRange,
};
use ide_language_host::LanguageHost;
use ide_process::{NativeProcessSupervisor, ProcessSupervisor};
use ide_project_model::{ProjectDescriptor, ProjectModel};
use ide_toolchain_api::{
    CompilationRequest, CompilerAdapter, DetectionContext, ExecutionRequest, RuntimeAdapter,
    TestAdapter, TestRequest, ToolchainProvider,
};
use ide_ui::{
    DebugRequest, DebugView, IdeShell, NavigationRequest, NewItemKind, NewItemRequest, SettingsPage,
};
use java_gradle_adapter::{GRADLE_BUILD_SYSTEM_ID, GradleAdapter};
use java_javac_adapter::JavaJavacAdapter;
use java_maven_adapter::{MAVEN_BUILD_SYSTEM_ID, MavenAdapter};
use java_toolchain::{ClasspathBuilder, JavaToolchainProvider, JavaToolchainSelection};
use language_java::{JAVA_PROVIDER_ID, JavaLanguageProvider};
use ui_core::{Modifiers, Point, Size, WindowId};
use ui_render_api::{FrameInfo, UiRenderer};
use ui_render_wgpu::WgpuRenderer;
use ui_window_api::WindowRequest;
use ui_window_winit::WinitWindow;
use winit::{
    application::ApplicationHandler,
    event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    keyboard::{Key, NamedKey},
    window::{CursorIcon, WindowId as WinitWindowId},
};

/// Janela de tempo em que dois cliques contam como um duplo.
///
/// É o mesmo intervalo padrão do Windows. Mais curto, quem clica devagar nunca
/// consegue selecionar a palavra; mais longo, dois cliques deliberados em pontos
/// diferentes viram um duplo por acidente.
const DOUBLE_CLICK_INTERVAL: Duration = Duration::from_millis(500);
/// Distância máxima entre os dois cliques, em pontos.
const DOUBLE_CLICK_SLACK: f32 = 4.0;

#[derive(Default)]
struct NativeIde {
    window: Option<WinitWindow>,
    renderer: Option<WgpuRenderer>,
    shell: Option<IdeShell>,
    startup_error: Option<String>,
    cursor: Point,
    /// Instante e posição do último clique simples, para detectar o duplo.
    last_click: Option<(Instant, Point)>,
    control_pressed: bool,
    shift_pressed: bool,
    navigation_requests: Vec<NavigationRequest>,
    language_host: Option<LanguageHost>,
    language_documents: HashMap<DocumentId, DocumentSnapshot>,
    java_toolchains: JavaToolchainSelection,
    java_adapter: Option<Arc<JavaJavacAdapter>>,
    build_systems: BuildSystemRegistry,
    project: Option<ImportedProject>,
    last_manifest_check: Option<Instant>,
    debugger: Option<debug::DebugController>,
    debug_view: DebugView,
    debug_thread: Option<ide_debug_api::ThreadId>,
    config: AppConfig,
    config_path: Option<PathBuf>,
    /// Abas já registradas na configuração, para gravar só quando mudarem.
    remembered_documents: Vec<PathBuf>,
    tool_events: Option<Receiver<ToolEvent>>,
    tool_sender: Option<Sender<ToolEvent>>,
}

/// Projeto importado do build system detectado na raiz do workspace.
struct ImportedProject {
    adapter: Arc<dyn BuildSystemAdapter>,
    descriptor: ProjectDescriptor,
    model: ProjectModel,
    manifest_modified: Option<SystemTime>,
}

struct ToolEvent {
    status: String,
    stdout: String,
    stderr: String,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum JavaTask {
    Compile,
    Run,
    Test,
}

impl NativeIde {
    fn initialize(&mut self, event_loop: &ActiveEventLoop) -> Result<(), String> {
        let request = WindowRequest {
            title: "ER IDE — Rust Native IDE".to_owned(),
            logical_size: Size::new(1280.0, 800.0),
        };
        let window = WinitWindow::create_hidden(event_loop, WindowId(1), &request)
            .map_err(|error| error.to_string())?;
        let renderer = pollster::block_on(WgpuRenderer::new(window.inner().clone()))
            .map_err(|error| error.to_string())?;
        self.config_path = config_path();
        if let Some(path) = self.config_path.as_ref() {
            match AppConfig::load(path) {
                Ok(config) => self.config = config,
                Err(error) => {
                    tracing::warn!(%error, path = %path.display(), "configuração ignorada");
                }
            }
        }
        let root = startup_root(&self.config, std::env::current_dir().ok())
            .ok_or_else(|| "não foi possível determinar o diretório do projeto".to_owned())?;
        let language_host = LanguageHost::new(&root);
        language_host
            .register(Arc::new(JavaLanguageProvider::new()))
            .map_err(|error| error.to_string())?;
        self.language_host = Some(language_host);
        let mut shell = IdeShell::open(&root).map_err(|error| error.to_string())?;
        // Os componentes medem o texto pela mesma fonte que vai desenhá-lo. Quem
        // constrói o mecanismo é a aplicação; a interface só recebe a porta.
        shell.set_text_metrics(Arc::new(ui_text_cosmic::CosmicTextEngine::new()));
        // Copiar e colar falam com o sistema, não com uma cópia interna. Sem área
        // de transferência no ambiente a IDE segue funcionando, e é a barra de
        // estado que conta isso quando alguém tenta copiar.
        match ui_clipboard_arboard::SystemClipboard::new() {
            Ok(clipboard) => shell.set_clipboard(Arc::new(clipboard)),
            Err(error) => tracing::warn!(%error, "área de transferência indisponível"),
        }
        // As abas do último uso voltam com o projeto: quem reabre a IDE espera
        // continuar de onde parou, e um arquivo que sumiu é ignorado em
        // silêncio, como um projeto inexistente.
        for document in self.config.workspace.resolved_documents(&root) {
            if let Err(error) = shell.open_file(&document) {
                tracing::warn!(%error, path = %document.display(), "aba não pôde ser reaberta");
            }
        }
        if let Some(active) = self.config.workspace.resolved_active_document(&root)
            && let Err(error) = shell.open_file(&active)
        {
            tracing::warn!(%error, path = %active.display(), "aba ativa não pôde ser restaurada");
        }
        self.remembered_documents = shell.open_document_paths();
        self.shell = Some(shell);
        let processes: Arc<dyn ProcessSupervisor> = Arc::new(NativeProcessSupervisor::default());
        self.java_adapter = Some(Arc::new(JavaJavacAdapter::new(processes.clone())));
        self.build_systems
            .register(Arc::new(MavenAdapter::new(processes.clone())));
        self.build_systems
            .register(Arc::new(GradleAdapter::new(processes)));
        let (tool_sender, tool_events) = mpsc::channel();
        self.tool_sender = Some(tool_sender);
        self.tool_events = Some(tool_events);
        self.detect_java_toolchains(&root);
        self.import_project(&root);
        if let Some(shell) = self.shell.as_mut() {
            shell.set_debug_target(&self.config.debug.host, self.config.debug.port);
        }
        self.debugger = debug::DebugController::start();
        self.renderer = Some(renderer);
        window
            .inner()
            .set_title(&format!("ER IDE — {}", root.display()));
        window.show();
        self.window = Some(window);
        // As abas restauradas já estão abertas, mas ninguém pediu o realce
        // delas: `sync_languages` só rodava dentro do tratamento de eventos, e
        // por isso o código aparecia sem cor até o primeiro clique.
        self.sync_languages();
        if let Some(window) = self.window.as_ref() {
            window.request_redraw();
        }
        Ok(())
    }

    fn render(&mut self) -> Result<(), String> {
        let window = self
            .window
            .as_ref()
            .ok_or_else(|| "window unavailable".to_owned())?;
        let size = window.logical_size();
        let shell = self
            .shell
            .as_mut()
            .ok_or_else(|| "shell unavailable".to_owned())?;
        let commands = shell.paint(size);
        let renderer = self
            .renderer
            .as_mut()
            .ok_or_else(|| "renderer unavailable".to_owned())?;
        renderer
            .begin_frame(FrameInfo {
                window_id: window.handle().id,
                logical_size: size,
                scale_factor: window.scale_factor(),
            })
            .map_err(|error| error.to_string())?;
        renderer
            .submit(&commands)
            .map_err(|error| error.to_string())?;
        renderer.end_frame().map_err(|error| error.to_string())
    }

    fn choose_project(&mut self) {
        let Some(current) = self
            .shell
            .as_ref()
            .map(|shell| shell.workspace_path().to_path_buf())
        else {
            return;
        };
        let Some(folder) = rfd::FileDialog::new()
            .set_title("Abrir projeto")
            .set_directory(current)
            .pick_folder()
        else {
            return;
        };
        match IdeShell::open(&folder) {
            Ok(shell) => {
                if let Some(language_host) = &self.language_host {
                    if let Err(error) = pollster::block_on(language_host.shutdown())
                        .and_then(|()| language_host.set_workspace_root(&folder))
                        .and_then(|()| {
                            language_host.enable(&ProviderId(JAVA_PROVIDER_ID.to_owned()))
                        })
                    {
                        self.startup_error = Some(error.to_string());
                        return;
                    }
                }
                self.language_documents.clear();
                self.shell = Some(shell);
                self.remember_project(&folder);
                self.detect_java_toolchains(&folder);
                self.import_project(&folder);
                self.sync_languages();
                if let Some(window) = self.window.as_ref() {
                    window
                        .inner()
                        .set_title(&format!("ER IDE — {}", folder.display()));
                    window.request_redraw();
                }
            }
            Err(error) => {
                self.startup_error = Some(format!(
                    "failed to open project {}: {error}",
                    folder.display()
                ));
            }
        }
    }

    fn sync_languages(&mut self) {
        let Some(language_host) = &self.language_host else {
            return;
        };
        let snapshots = self
            .shell
            .as_ref()
            .map(IdeShell::document_snapshots)
            .unwrap_or_default();
        let open_ids = snapshots
            .iter()
            .map(|snapshot| snapshot.id)
            .collect::<std::collections::HashSet<_>>();
        let closed = self
            .language_documents
            .keys()
            .filter(|id| !open_ids.contains(id))
            .copied()
            .collect::<Vec<_>>();
        for document_id in closed {
            let _ = pollster::block_on(
                language_host.close_document(language_host.request_context(), document_id),
            );
            self.language_documents.remove(&document_id);
        }

        for snapshot in snapshots {
            if !snapshot
                .path
                .extension()
                .and_then(|value| value.to_str())
                .is_some_and(|extension| extension.eq_ignore_ascii_case("java"))
            {
                continue;
            }
            let result = match self.language_documents.get(&snapshot.id) {
                None => pollster::block_on(
                    language_host.open_document(language_host.request_context(), snapshot.clone()),
                )
                .map(|_| ()),
                Some(previous) if previous.version != snapshot.version => {
                    let change = document_change(previous, &snapshot);
                    pollster::block_on(
                        language_host.change_document(language_host.request_context(), change),
                    )
                }
                Some(_) => Ok(()),
            };
            if let Err(error) = result {
                tracing::warn!(%error, document_id = snapshot.id.0, "Java syntax update failed");
                continue;
            }
            let changed = self
                .language_documents
                .get(&snapshot.id)
                .is_none_or(|previous| previous.version != snapshot.version);
            self.language_documents
                .insert(snapshot.id, snapshot.clone());
            if changed {
                match pollster::block_on(
                    language_host.syntax(language_host.request_context(), snapshot.id),
                ) {
                    Ok(syntax) => {
                        if let Some(shell) = self.shell.as_mut() {
                            shell.set_syntax_snapshot(syntax);
                        }
                    }
                    Err(error) => {
                        tracing::warn!(
                            %error,
                            document_id = snapshot.id.0,
                            "Java syntax snapshot failed"
                        );
                    }
                }
            }
        }
    }

    fn navigate_to_definition(&mut self, request: NavigationRequest) {
        let Some(language_host) = &self.language_host else {
            return;
        };
        let Some(document) = self.language_documents.get(&request.document_id) else {
            return;
        };
        let definition = DefinitionRequest {
            document_id: request.document_id,
            position: position_at_offset(&document.text, request.byte_offset),
        };
        match pollster::block_on(
            language_host.definition(language_host.request_context(), definition),
        ) {
            Ok(locations) => {
                if let Some(location) = locations.first() {
                    if let Some(shell) = self.shell.as_mut()
                        && let Err(error) = shell.open_location(
                            &location.path,
                            location.range.start.line as usize,
                            location.range.start.column as usize,
                        )
                    {
                        shell.set_status_message(error);
                    }
                } else if let Some(shell) = self.shell.as_mut() {
                    shell.set_status_message(format!("Definition not found: {}", request.token));
                }
            }
            Err(error) => {
                if let Some(shell) = self.shell.as_mut() {
                    shell.set_status_message(error.to_string());
                }
            }
        }
    }

    fn request_completion(&mut self) {
        let Some(language_host) = &self.language_host else {
            return;
        };
        let Some(request) = self.shell.as_ref().and_then(IdeShell::completion_request) else {
            return;
        };
        match pollster::block_on(language_host.completion(language_host.request_context(), request))
        {
            Ok(items) => {
                if let Some(shell) = self.shell.as_mut() {
                    shell.set_completions(items);
                }
            }
            Err(error) => {
                if let Some(shell) = self.shell.as_mut() {
                    shell.set_status_message(error.to_string());
                }
            }
        }
    }

    fn detect_java_toolchains(&mut self, workspace_root: &Path) {
        let provider = JavaToolchainProvider::new();
        match pollster::block_on(provider.detect(DetectionContext {
            workspace_root: Some(workspace_root.to_path_buf()),
        })) {
            Ok(installations) => {
                self.java_toolchains = JavaToolchainSelection::new(installations);
                if let Some(shell) = self.shell.as_mut() {
                    if let Some(selected) = self.java_toolchains.selected() {
                        let label = selected
                            .version
                            .clone()
                            .unwrap_or_else(|| selected.home.to_string_lossy().into_owned());
                        shell.set_status_message(format!("JDK: {}", label));
                    } else {
                        shell.set_status_message(
                            "JDK not found; configure JAVA_HOME or install a JDK",
                        );
                    }
                }
            }
            Err(error) => {
                if let Some(shell) = self.shell.as_mut() {
                    shell.set_status_message(error.to_string());
                }
            }
        }
        self.apply_jdk_to_languages();
    }

    /// Rótulo de cada JDK detectado, na ordem da lista.
    fn jdk_labels(&self) -> Vec<String> {
        self.java_toolchains
            .installations()
            .iter()
            .map(|installation| {
                format!(
                    "{}  —  {}",
                    installation
                        .version
                        .as_deref()
                        .unwrap_or("versão desconhecida"),
                    installation.home.display()
                )
            })
            .collect()
    }

    fn open_jdk_selector(&mut self) {
        let selected = self.java_toolchains.selected().map(|value| &value.id);
        let selected_index = self
            .java_toolchains
            .installations()
            .iter()
            .position(|installation| Some(&installation.id) == selected)
            .unwrap_or(0);
        let items = self.jdk_labels();
        if let Some(shell) = self.shell.as_mut() {
            shell.open_settings_dialog(items, selected_index);
        }
    }

    fn choose_jdk_home(&mut self) {
        let initial_directory = self
            .java_toolchains
            .selected()
            .map_or_else(|| Path::new(".").to_path_buf(), |jdk| jdk.home.clone());
        let Some(folder) = rfd::FileDialog::new()
            .set_title("Selecionar pasta do JDK")
            .set_directory(initial_directory)
            .pick_folder()
        else {
            return;
        };
        match JavaToolchainProvider::installation_from_home(folder) {
            Ok(installation) => {
                let home = installation.home.clone();
                // A pasta apontada entra na lista e fica pendente: quem aplica é
                // o Salvar, como qualquer escolha feita na janela.
                let index = self.java_toolchains.add(installation);
                let labels = self.jdk_labels();
                if let Some(shell) = self.shell.as_mut() {
                    shell.set_jdk_options(labels, index);
                    shell.set_status_message(format!("JDK a salvar: {}", home.display()));
                }
            }
            Err(_) => {
                if let Some(shell) = self.shell.as_mut() {
                    shell.set_settings_message(
                        "Pasta inválida: selecione um JDK com bin/java, bin/javac e bin/jar.",
                    );
                }
            }
        }
    }

    /// Cria o pacote e, se houver nome, o tipo dentro dele.
    ///
    /// O pacote sempre é criado: é o diretório onde o tipo vai morar, e criá-lo
    /// sozinho é o caso de uso do "Novo pacote". O gabarito Java mora aqui, no
    /// ponto onde a IDE já compõe as peças Java — o `ide-workspace` grava
    /// arquivo sem saber de linguagem, e a janela não escreve código.
    fn create_new_item(&mut self, request: NewItemRequest) {
        let directory = request
            .package
            .split('.')
            .filter(|segment| !segment.is_empty())
            .fold(request.source_root.clone(), |path, segment| {
                path.join(segment)
            });
        if let Err(error) = ide_workspace::create_directory(&directory) {
            if let Some(shell) = self.shell.as_mut() {
                shell.set_new_item_message(error.to_string());
            }
            return;
        }
        let created_file = if request.name.is_empty() {
            None
        } else {
            let path = directory.join(format!("{}.java", request.name));
            let source = java_source(&request, &request.name);
            if let Err(error) = ide_workspace::create_file(&path, &source) {
                if let Some(shell) = self.shell.as_mut() {
                    shell.set_new_item_message(error.to_string());
                }
                return;
            }
            Some(path)
        };
        if let Some(shell) = self.shell.as_mut() {
            shell.close_new_item_dialog();
            // A árvore precisa reler o disco: o que acabou de nascer não estava
            // na varredura anterior.
            if let Err(error) = shell.reload_workspace() {
                shell.set_status_message(error.to_string());
            }
            // O que nasceu dentro de uma pasta fechada não aparece: revelar o
            // caminho é parte de ter criado.
            shell.reveal_in_explorer(&directory);
            match &created_file {
                Some(path) => {
                    shell.set_status_message(format!("Criado {}", path.display()));
                    if let Err(error) = shell.open_file(path) {
                        shell.set_status_message(error);
                    }
                }
                None => shell.set_status_message(format!("Criado {}", directory.display())),
            }
        }
        self.sync_languages();
    }

    /// Informa ao host de linguagens qual JDK está escolhido.
    ///
    /// A biblioteca padrão que a completação conhece vem dessa instalação, então
    /// trocar de JDK derruba os providers ativos: eles indexaram o JDK anterior.
    /// Os documentos abertos são esquecidos de propósito — o provider novo nasce
    /// sem nenhum, e a próxima sincronização os reabre.
    fn apply_jdk_to_languages(&mut self) {
        let Some(language_host) = &self.language_host else {
            return;
        };
        let home = self
            .java_toolchains
            .selected()
            .map(|installation| installation.home.clone());
        match language_host.set_jdk_home(home) {
            Ok(true) => {
                if pollster::block_on(language_host.reactivate()).is_ok() {
                    self.language_documents.clear();
                }
            }
            Ok(false) => {}
            Err(error) => tracing::warn!(%error, "não foi possível registrar o JDK escolhido"),
        }
    }

    fn select_jdk(&mut self, index: usize) {
        let Some(installation) = self.java_toolchains.installations().get(index).cloned() else {
            return;
        };
        if let Err(error) = self.java_toolchains.select(&installation.id) {
            if let Some(shell) = self.shell.as_mut() {
                shell.set_status_message(error.to_string());
            }
            return;
        }
        if let Some(shell) = self.shell.as_mut() {
            shell.set_status_message(format!("Selected JDK: {}", installation.home.display()));
        }
        self.apply_jdk_to_languages();
    }

    /// Detecta o build system da raiz e importa módulos e dependências.
    ///
    /// A importação é nativa: nenhum processo externo é iniciado aqui, apenas os
    /// manifestos do projeto são lidos. O Maven ou o Gradle só executam quando o
    /// usuário pede um build.
    fn import_project(&mut self, root: &Path) {
        let previous = self.project.take();
        self.last_manifest_check = None;
        if let Some(shell) = self.shell.as_mut() {
            shell.set_project_summary(None);
        }
        if self.build_systems.is_empty() {
            return;
        }
        let detected = match pollster::block_on(self.build_systems.detect(root)) {
            Ok(detected) => detected,
            Err(error) => {
                if let Some(shell) = self.shell.as_mut() {
                    shell.set_status_message(format!("Falha ao detectar o projeto: {error}"));
                }
                return;
            }
        };
        let Some((adapter, descriptor)) = detected else {
            return;
        };
        let request = ProjectImportRequest::new(descriptor.clone())
            .with_java_home(self.java_toolchains.selected().map(|jdk| jdk.home.clone()));
        match pollster::block_on(adapter.import_project(request)) {
            Ok(model) => {
                let summary = model.summary();
                self.project = Some(ImportedProject {
                    adapter,
                    manifest_modified: modified_at(&descriptor.manifest),
                    descriptor,
                    model,
                });
                if let Some(shell) = self.shell.as_mut() {
                    shell.set_project_summary(Some(summary.clone()));
                    shell.set_status_message(format!("Projeto importado: {summary}"));
                }
            }
            Err(error) => {
                // O último modelo válido continua valendo, e o carimbo do
                // manifesto é atualizado para não repetir a falha a cada segundo.
                let summary = previous.map(|mut project| {
                    project.manifest_modified = modified_at(&descriptor.manifest);
                    let summary = project.model.summary();
                    self.project = Some(project);
                    summary
                });
                if let Some(shell) = self.shell.as_mut() {
                    shell.set_project_summary(summary);
                    shell.set_status_message(format!(
                        "Falha ao importar {}: {error}",
                        descriptor.manifest.display()
                    ));
                }
            }
        }
    }

    fn reimport_project(&mut self) {
        let Some(root) = self
            .shell
            .as_ref()
            .map(|shell| shell.workspace_path().to_path_buf())
        else {
            return;
        };
        self.import_project(&root);
        if self.project.is_none()
            && let Some(shell) = self.shell.as_mut()
        {
            shell.set_status_message("Nenhum projeto Maven ou Gradle na raiz do workspace");
        }
    }

    /// Reimporta quando o manifesto muda fora da IDE, sem varrer o disco a cada frame.
    fn watch_manifest(&mut self) -> bool {
        let Some(project) = self.project.as_ref() else {
            return false;
        };
        if self
            .last_manifest_check
            .is_some_and(|checked| checked.elapsed() < Duration::from_secs(1))
        {
            return false;
        }
        self.last_manifest_check = Some(Instant::now());
        let modified = modified_at(&project.descriptor.manifest);
        if modified == project.manifest_modified {
            return false;
        }
        let root = project.descriptor.root.clone();
        self.import_project(&root);
        if let Some(shell) = self.shell.as_mut() {
            shell.set_status_message("Manifesto alterado: projeto reimportado");
        }
        true
    }

    /// Guarda o projeto aberto para que a próxima inicialização o reabra.
    ///
    /// Falhar ao gravar não pode impedir o trabalho: o projeto continua aberto e
    /// o usuário só perde a reabertura automática, avisado na barra de status.
    fn remember_project(&mut self, root: &Path) {
        let Some(path) = self.config_path.clone() else {
            return;
        };
        if let Err(error) = self.config.remember_workspace(root, &path) {
            tracing::warn!(%error, path = %path.display(), "configuração não pôde ser gravada");
            if let Some(shell) = self.shell.as_mut() {
                shell.set_status_message(format!("Configuração não pôde ser gravada: {error}"));
            }
        }
    }

    /// Raízes de código enviadas ao depurador para mapear posições em arquivos.
    ///
    /// Com projeto importado valem as raízes declaradas — inclusive as geradas.
    /// Sem projeto, a raiz do workspace e os diretórios convencionais servem de
    /// aproximação para arquivos avulsos.
    fn debug_source_roots(&self) -> Vec<PathBuf> {
        if let Some(project) = self.project.as_ref() {
            let roots = project.model.source_roots();
            if !roots.is_empty() {
                return roots;
            }
        }
        let Some(workspace) = self
            .shell
            .as_ref()
            .map(|shell| shell.workspace_path().to_path_buf())
        else {
            return Vec::new();
        };
        vec![
            workspace.join("src").join("main").join("java"),
            workspace.join("src"),
            workspace,
        ]
    }

    /// Botão de executar: sobe a aplicação do projeto, sem depuração.
    fn run_application(&mut self) {
        let target = self.run_target();
        let command = run::run_command(
            self.config.run.command.as_deref(),
            target.as_ref(),
            run::RunMode::Plain,
        );
        let Some(command) = command else {
            if let Some(shell) = self.shell.as_mut() {
                shell.set_status_message(
                    "A IDE não sabe executar este projeto; defina `run.command` na configuração",
                );
            }
            return;
        };
        let result = self
            .shell
            .as_mut()
            .map(|shell| shell.run_in_terminal(&command));
        if let (Some(Err(error)), Some(shell)) = (result, self.shell.as_mut()) {
            shell.set_status_message(error);
        }
    }

    /// Botão de parar: interrompe a aplicação iniciada pela IDE.
    ///
    /// Uma sessão de depuração aberta é desconectada antes, para o depurador
    /// não ficar apontando para um processo que está terminando.
    fn stop_application(&mut self) {
        if self.debug_view.attached {
            self.send_debug(debug::DebugCommand::Detach);
        }
        let result = self.shell.as_mut().map(IdeShell::stop_application);
        if let (Some(Err(error)), Some(shell)) = (result, self.shell.as_mut()) {
            shell.set_status_message(error);
        }
    }

    /// Botão de depurar: sobe a aplicação, quando necessário, e conecta.
    ///
    /// Se já existe algo escutando no alvo — servidor externo, contêiner,
    /// máquina remota — nada é iniciado e a IDE apenas conecta.
    fn run_and_attach(&mut self, host: &str, port: u16) {
        self.remember_debug_target(host, port);
        let source_roots = self.debug_source_roots();
        let already_running = debug::port_is_open(host, port);
        let mut attempts = 1;
        if !already_running {
            let target = self.run_target();
            let command = run::run_command(
                self.config.run.command.as_deref(),
                target.as_ref(),
                run::RunMode::Debug { host, port },
            );
            let Some(command) = command else {
                if let Some(shell) = self.shell.as_mut() {
                    shell.set_status_message(
                        "Nada escutando no alvo e a IDE não sabe subir este projeto; \
                         defina `debug.command` na configuração ou inicie a aplicação",
                    );
                }
                return;
            };
            let started = self
                .shell
                .as_mut()
                .map(|shell| shell.run_in_terminal(&command));
            match started {
                Some(Ok(())) => {
                    attempts = 120;
                    if let Some(shell) = self.shell.as_mut() {
                        shell.set_status_message(format!(
                            "Subindo a aplicação e aguardando {host}:{port}"
                        ));
                    }
                }
                Some(Err(error)) => {
                    if let Some(shell) = self.shell.as_mut() {
                        shell.set_status_message(error);
                    }
                    return;
                }
                None => return,
            }
        }
        self.send_debug(debug::DebugCommand::Attach {
            host: host.to_owned(),
            port,
            source_roots,
            attempts,
        });
    }

    /// Descreve o projeto importado para a montagem do comando de execução.
    fn run_target(&self) -> Option<run::RunTarget<'_>> {
        let project = self.project.as_ref()?;
        Some(run::RunTarget {
            build_system: project.descriptor.build_system.0.as_str(),
            wrapper: project
                .descriptor
                .wrapper
                .as_ref()
                .map(|wrapper| wrapper.to_string_lossy().into_owned()),
            spring_boot: project.model.declares_plugin("spring-boot-maven-plugin")
                || project.model.declares_plugin("org.springframework.boot"),
        })
    }

    fn remember_debug_target(&mut self, host: &str, port: u16) {
        if self.config.debug.host == host && self.config.debug.port == port {
            return;
        }
        let Some(path) = self.config_path.clone() else {
            return;
        };
        if let Err(error) = self.config.remember_debug_target(host, port, &path) {
            tracing::warn!(%error, "alvo de depuração não pôde ser gravado");
        }
    }

    fn handle_debug_requests(&mut self) {
        let requests = self
            .shell
            .as_mut()
            .map(IdeShell::take_debug_requests)
            .unwrap_or_default();
        for request in requests {
            match request {
                DebugRequest::Attach { host, port } => {
                    self.remember_debug_target(&host, port);
                    let source_roots = self.debug_source_roots();
                    self.send_debug(debug::DebugCommand::Attach {
                        host,
                        port,
                        source_roots,
                        attempts: 1,
                    });
                }
                DebugRequest::RunAndAttach { host, port } => self.run_and_attach(&host, port),
                DebugRequest::Continue => self.send_debug(debug::DebugCommand::Continue),
                DebugRequest::Pause => self.send_debug(debug::DebugCommand::Pause),
                DebugRequest::StepOver => {
                    self.send_debug(debug::DebugCommand::Step(StepKind::Over));
                }
                DebugRequest::StepInto => {
                    self.send_debug(debug::DebugCommand::Step(StepKind::Into));
                }
                DebugRequest::StepOut => {
                    self.send_debug(debug::DebugCommand::Step(StepKind::Out));
                }
                DebugRequest::Detach => self.send_debug(debug::DebugCommand::Detach),
                DebugRequest::SelectFrame(index) => {
                    if let Some(thread) = self.debug_thread {
                        self.send_debug(debug::DebugCommand::Refresh {
                            thread,
                            frame: index,
                        });
                    }
                }
                DebugRequest::ExpandInspection(path) => {
                    if let Some(thread) = self.debug_thread {
                        self.send_debug(debug::DebugCommand::ExpandInspection {
                            thread,
                            frame: self.debug_view.selected_frame,
                            path,
                        });
                    }
                }
                DebugRequest::Evaluate(expression) => {
                    // A avaliação acontece no quadro que o usuário está olhando:
                    // o mesmo nome vale coisas diferentes em quadros diferentes.
                    if let Some(thread) = self.debug_thread {
                        self.send_debug(debug::DebugCommand::Evaluate {
                            thread,
                            frame: self.debug_view.selected_frame,
                            expression,
                        });
                    } else if let Some(shell) = self.shell.as_mut() {
                        shell.set_status_message("Nenhuma thread parada para inspecionar");
                    }
                }
            }
        }
        if self.shell.as_mut().is_some_and(IdeShell::take_run_request) {
            self.run_application();
        }
        if self.shell.as_mut().is_some_and(IdeShell::take_stop_request) {
            self.stop_application();
        }
        if let Some(path) = self
            .shell
            .as_mut()
            .and_then(IdeShell::take_breakpoints_dirty)
        {
            let lines = self
                .shell
                .as_ref()
                .map(|shell| shell.breakpoints_for(&path))
                .unwrap_or_default();
            self.send_debug(debug::DebugCommand::SetBreakpoints { path, lines });
        }
        self.remember_documents();
    }

    /// Mantém as abas abertas registradas na configuração.
    ///
    /// O conjunto é comparado a cada quadro em vez de sinalizado em cada ponto
    /// que abre ou fecha uma aba: comparar alguns caminhos é barato, e nenhum
    /// caminho novo pode esquecer de avisar.
    fn remember_documents(&mut self) {
        let Some(shell) = self.shell.as_ref() else {
            return;
        };
        let open = shell.open_document_paths();
        let active = shell.active_document_path();
        if open == self.remembered_documents
            && active.as_deref() == self.config.workspace.active_document.as_deref()
        {
            return;
        }
        let Some(path) = self.config_path.clone() else {
            self.remembered_documents = open;
            return;
        };
        if let Err(error) = self
            .config
            .remember_documents(&open, active.as_deref(), &path)
        {
            tracing::warn!(%error, path = %path.display(), "abas não puderam ser gravadas");
        }
        self.remembered_documents = open;
    }

    fn send_debug(&self, command: debug::DebugCommand) {
        if let Some(debugger) = self.debugger.as_ref() {
            debugger.send(command);
        }
    }

    /// Aplica os eventos da sessão ao estado apresentado pela interface.
    fn drain_debug_events(&mut self) -> bool {
        let events = self
            .debugger
            .as_ref()
            .map(debug::DebugController::poll)
            .unwrap_or_default();
        if events.is_empty() {
            return false;
        }
        for event in events {
            match event {
                debug::DebugUiEvent::Session(DebugEvent::Attached { description }) => {
                    self.debug_view = DebugView {
                        attached: true,
                        status: format!("Conectado a {description}"),
                        ..DebugView::default()
                    };
                }
                debug::DebugUiEvent::Session(DebugEvent::Stopped { thread, reason }) => {
                    self.debug_thread = Some(thread);
                    self.debug_view.attached = true;
                    self.debug_view.status = debug::stop_reason_label(&reason);
                    self.send_debug(debug::DebugCommand::Refresh { thread, frame: 0 });
                }
                debug::DebugUiEvent::Session(DebugEvent::Resumed { .. }) => {
                    self.debug_view.status = "Em execução".to_owned();
                    self.debug_view.stopped_at = None;
                    self.debug_view.frames.clear();
                    self.debug_view.variables.clear();
                }
                debug::DebugUiEvent::Session(DebugEvent::Detached { reason }) => {
                    self.debug_thread = None;
                    self.debug_view = DebugView::default();
                    if let Some(shell) = self.shell.as_mut() {
                        shell.set_status_message(
                            reason.unwrap_or_else(|| "Depuração encerrada".to_owned()),
                        );
                    }
                }
                debug::DebugUiEvent::Session(DebugEvent::Output { text }) => {
                    if let Some(shell) = self.shell.as_mut() {
                        shell.append_tool_output(&text, false);
                    }
                }
                debug::DebugUiEvent::View {
                    thread,
                    frames,
                    variables,
                    selected,
                } => {
                    self.debug_thread = Some(thread);
                    self.debug_view.attached = true;
                    self.debug_view.stopped_at = debug::first_location(&frames);
                    self.debug_view.frames = frames;
                    self.debug_view.variables = variables;
                    self.debug_view.selected_frame = selected;
                }
                debug::DebugUiEvent::Breakpoints { path, verified } => {
                    if let Some(shell) = self.shell.as_mut() {
                        shell.set_verified_breakpoints(&path, &verified);
                    }
                }
                debug::DebugUiEvent::Inspection {
                    expression,
                    value,
                    fields,
                } => {
                    if let Some(shell) = self.shell.as_mut() {
                        shell.show_inspection(expression, value, fields);
                    }
                }
                debug::DebugUiEvent::InspectionFields { path, fields } => {
                    if let Some(shell) = self.shell.as_mut() {
                        shell.add_inspection_fields(&path, fields);
                    }
                }
                debug::DebugUiEvent::Status(status) => {
                    if let Some(shell) = self.shell.as_mut() {
                        shell.set_status_message(status);
                    }
                }
            }
        }
        if let Some(shell) = self.shell.as_mut() {
            shell.set_debug_view(self.debug_view.clone());
        }
        true
    }

    /// Executa o build do sistema detectado, fora da thread da interface.
    fn start_project_build(&mut self) {
        let Some(project) = self.project.as_ref() else {
            if let Some(shell) = self.shell.as_mut() {
                shell.set_status_message("Nenhum projeto Maven ou Gradle importado");
            }
            return;
        };
        let Some(sender) = self.tool_sender.clone() else {
            return;
        };
        let adapter = project.adapter.clone();
        let request = BuildCommandRequest::new(
            project.descriptor.clone(),
            default_goals(&project.descriptor),
        )
        .with_java_home(self.java_toolchains.selected().map(|jdk| jdk.home.clone()));
        let label = format!(
            "[{}] {}",
            project.descriptor.build_system.label(),
            request.goals.join(" ")
        );
        let Some(shell) = self.shell.as_mut() else {
            return;
        };
        shell.append_tool_output(&format!("{label}..."), false);
        shell.set_status_message(format!("{label} em execução"));
        let spawn = thread::Builder::new()
            .name("project-build".to_owned())
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
                let event = match runtime.block_on(adapter.execute(request)) {
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
                        status: format!("{label} falhou"),
                        stdout: String::new(),
                        stderr: error.to_string(),
                    },
                };
                let _ = sender.send(event);
            });
        if let Err(error) = spawn {
            shell.set_status_message(error.to_string());
        }
    }

    fn start_java_task(&mut self, task: JavaTask) {
        let Some(adapter) = self.java_adapter.clone() else {
            return;
        };
        let Some(installation) = self.java_toolchains.selected().cloned() else {
            if let Some(shell) = self.shell.as_mut() {
                shell.set_status_message("No JDK selected");
            }
            return;
        };
        let Some(sender) = self.tool_sender.clone() else {
            return;
        };
        let model = self.project.as_ref().map(|project| project.model.clone());
        let Some(shell) = self.shell.as_mut() else {
            return;
        };
        let workspace = shell.workspace_path().to_path_buf();
        let source_files = project_sources(shell.java_source_files(), model.as_ref());
        if source_files.is_empty() {
            shell.set_status_message("No Java source files found");
            return;
        }
        let active = shell
            .document_snapshots()
            .into_iter()
            .find(|snapshot| Some(snapshot.id) == shell.active_document());
        let main_class = active.as_ref().and_then(main_class_name);
        if task != JavaTask::Compile && main_class.is_none() {
            shell.set_status_message("Open a Java file before running or testing");
            return;
        }
        let output_directory = workspace.join(".er-ide").join("classes");
        let mut builder = ClasspathBuilder::new().workspace_defaults(&workspace, &output_directory);
        for entry in model
            .as_ref()
            .map(ProjectModel::classpath_entries)
            .unwrap_or_default()
        {
            builder = builder.with_entry(entry);
        }
        let classpath = builder.build();
        let compilation = CompilationRequest {
            installation: installation.clone(),
            source_files,
            output_directory: output_directory.clone(),
            classpath: classpath.clone(),
            source_level: Some("8".to_owned()),
            target_level: Some("8".to_owned()),
            additional_args: Vec::new(),
            working_directory: workspace.clone(),
        };
        shell.append_tool_output(
            match task {
                JavaTask::Compile => "[Java] Compiling workspace...",
                JavaTask::Run => "[Java] Compiling before run...",
                JavaTask::Test => "[Java] Compiling before test...",
            },
            false,
        );
        shell.set_status_message("Java task running");
        let spawn = thread::Builder::new()
            .name("java-build-run".to_owned())
            .spawn(move || {
                let runtime = match tokio::runtime::Builder::new_current_thread()
                    .enable_time()
                    .build()
                {
                    Ok(runtime) => runtime,
                    Err(error) => {
                        let _ = sender.send(ToolEvent {
                            status: "Java task failed".to_owned(),
                            stdout: String::new(),
                            stderr: error.to_string(),
                        });
                        return;
                    }
                };
                let result = runtime.block_on(async {
                    if task == JavaTask::Test {
                        let tested = adapter
                            .run_tests(TestRequest {
                                compilation,
                                test_classes: vec![main_class.unwrap_or_default()],
                                args: Vec::new(),
                            })
                            .await?;
                        let mut stdout = tested.compilation.stdout;
                        let mut stderr = tested.compilation.stderr;
                        let mut success = tested.compilation.success;
                        for case in tested.cases {
                            stdout.push_str(&case.execution.stdout);
                            stderr.push_str(&case.execution.stderr);
                            success &= case.execution.success;
                        }
                        return Ok::<ToolEvent, ide_toolchain_api::ToolchainError>(ToolEvent {
                            status: if success {
                                "Java test completed".to_owned()
                            } else {
                                "Java test failed".to_owned()
                            },
                            stdout,
                            stderr,
                        });
                    }
                    let compiled = adapter.compile(compilation).await?;
                    let mut stdout = compiled.stdout;
                    let mut stderr = compiled.stderr;
                    if !compiled.success || task == JavaTask::Compile {
                        return Ok::<ToolEvent, ide_toolchain_api::ToolchainError>(ToolEvent {
                            status: if compiled.success {
                                "Java compilation completed".to_owned()
                            } else {
                                format!("Java compilation failed ({})", compiled.exit_code)
                            },
                            stdout,
                            stderr,
                        });
                    }
                    let mut run_classpath = classpath;
                    if !run_classpath.entries.contains(&output_directory) {
                        run_classpath.entries.insert(0, output_directory);
                    }
                    let executed = adapter
                        .run(ExecutionRequest {
                            installation,
                            main_class: main_class.unwrap_or_default(),
                            classpath: run_classpath,
                            args: Vec::new(),
                            jvm_args: Vec::new(),
                            working_directory: workspace,
                        })
                        .await?;
                    stdout.push_str(&executed.stdout);
                    stderr.push_str(&executed.stderr);
                    Ok(ToolEvent {
                        status: if executed.success {
                            "Java execution completed".to_owned()
                        } else {
                            format!("Java execution failed ({})", executed.exit_code)
                        },
                        stdout,
                        stderr,
                    })
                });
                let event = result.unwrap_or_else(|error| ToolEvent {
                    status: "Java task failed".to_owned(),
                    stdout: String::new(),
                    stderr: error.to_string(),
                });
                let _ = sender.send(event);
            });
        if let Err(error) = spawn {
            shell.set_status_message(error.to_string());
        }
    }
}

/// Projeto a abrir na inicialização.
///
/// O último projeto usado tem prioridade sobre o diretório atual: quem abriu a
/// IDE pela segunda vez espera continuar de onde parou. Sem registro válido —
/// primeira execução, ou pasta que sumiu — vale o diretório atual.
fn startup_root(config: &AppConfig, current_directory: Option<PathBuf>) -> Option<PathBuf> {
    config.resolved_project().or(current_directory)
}

/// Metas equivalentes a "compilar o projeto" em cada build system.
fn default_goals(descriptor: &ProjectDescriptor) -> Vec<String> {
    match descriptor.build_system.0.as_str() {
        GRADLE_BUILD_SYSTEM_ID => vec!["classes".to_owned()],
        MAVEN_BUILD_SYSTEM_ID => vec!["compile".to_owned()],
        _ => vec!["build".to_owned()],
    }
}

/// Restringe as fontes às raízes do projeto importado, incluindo o código gerado.
///
/// Sem projeto — ou quando nenhuma fonte está sob as raízes declaradas — vale a
/// varredura completa do workspace usada desde a Fase 5.
fn project_sources(files: Vec<PathBuf>, model: Option<&ProjectModel>) -> Vec<PathBuf> {
    let Some(model) = model else {
        return files;
    };
    let filtered: Vec<PathBuf> = files
        .iter()
        .filter(|path| model.contains_source(path))
        .cloned()
        .collect();
    if filtered.is_empty() { files } else { filtered }
}

fn modified_at(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .ok()
}

fn main_class_name(document: &DocumentSnapshot) -> Option<String> {
    if !document
        .path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("java"))
    {
        return None;
    }
    let class = document.path.file_stem()?.to_str()?;
    let package = document.text.lines().find_map(|line| {
        line.trim()
            .strip_prefix("package ")
            .and_then(|value| value.strip_suffix(';'))
            .map(str::trim)
    });
    Some(package.map_or_else(|| class.to_owned(), |package| format!("{package}.{class}")))
}

impl ApplicationHandler for NativeIde {
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        event_loop.set_control_flow(ControlFlow::WaitUntil(
            std::time::Instant::now() + std::time::Duration::from_millis(30),
        ));
        let mut changed = false;
        if let Some(events) = &self.tool_events {
            while let Ok(event) = events.try_recv() {
                if let Some(shell) = self.shell.as_mut() {
                    if !event.stdout.is_empty() {
                        shell.append_tool_output(&event.stdout, false);
                    }
                    if !event.stderr.is_empty() {
                        shell.append_tool_output(&event.stderr, true);
                    }
                    shell.set_status_message(event.status);
                    changed = true;
                }
            }
        }
        changed |= self.watch_manifest();
        changed |= self.drain_debug_events();
        self.handle_debug_requests();
        if let (Some(window), Some(shell)) = (self.window.as_ref(), self.shell.as_mut()) {
            changed |= shell.update_terminals(window.logical_size());
            if changed {
                window.request_redraw();
            }
        }
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_none()
            && let Err(error) = self.initialize(event_loop)
        {
            self.startup_error = Some(error);
            event_loop.exit();
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        id: WinitWindowId,
        event: WindowEvent,
    ) {
        let Some(window) = self.window.as_ref() else {
            return;
        };
        if window.inner().id() != id {
            return;
        }
        let mut sync_languages = false;
        let mut pending_definition = None;
        let mut completion_requested = false;
        let mut build_requested = false;
        let mut project_build_requested = false;
        let mut run_requested = false;
        let mut test_requested = false;
        let mut select_jdk_requested = false;
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let Some(renderer) = self.renderer.as_mut() {
                    renderer.resize(size.width, size.height);
                }
                window.request_redraw();
            }
            WindowEvent::ScaleFactorChanged { .. } => {
                let size = window.inner().inner_size();
                if let Some(renderer) = self.renderer.as_mut() {
                    renderer.resize(size.width, size.height);
                }
                window.request_redraw();
            }
            WindowEvent::CursorMoved { position, .. } => {
                let logical = position.to_logical::<f32>(window.scale_factor());
                self.cursor = Point::new(logical.x, logical.y);
                let redraw = self
                    .shell
                    .as_mut()
                    .is_some_and(|shell| shell.pointer_move(self.cursor, window.logical_size()));
                let resizing = self.shell.as_ref().is_some_and(IdeShell::terminal_resizing);
                let sidebar_resizing = self.shell.as_ref().is_some_and(IdeShell::sidebar_resizing);
                let navigation_hover = self.shell.as_ref().is_some_and(|shell| {
                    shell.navigation_hover(self.cursor, window.logical_size(), self.control_pressed)
                });
                window.inner().set_cursor(if sidebar_resizing {
                    CursorIcon::EwResize
                } else if resizing {
                    CursorIcon::NsResize
                } else if navigation_hover {
                    CursorIcon::Pointer
                } else {
                    CursorIcon::Default
                });
                if redraw {
                    window.request_redraw();
                }
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => {
                // O winit não entrega duplo clique: quem decide é o app, por
                // tempo entre os dois cliques e distância entre eles. Sem a
                // distância, mover o ponteiro entre cliques distantes ainda
                // contaria como duplo.
                let now = Instant::now();
                let double = self.last_click.is_some_and(|(instant, point)| {
                    now.duration_since(instant) <= DOUBLE_CLICK_INTERVAL
                        && (point.x - self.cursor.x).abs() <= DOUBLE_CLICK_SLACK
                        && (point.y - self.cursor.y).abs() <= DOUBLE_CLICK_SLACK
                });
                // Um duplo clique encerra a contagem: três cliques seguidos não
                // são dois duplos.
                self.last_click = (!double).then_some((now, self.cursor));
                if double {
                    if let Some(shell) = self.shell.as_mut() {
                        shell.select_word_at_point(self.cursor, window.logical_size());
                    }
                    window.request_redraw();
                    return;
                }
                if let Some(shell) = self.shell.as_mut() {
                    shell.pointer_down_with_modifiers(
                        self.cursor,
                        window.logical_size(),
                        self.control_pressed,
                    );
                    if let Some(request) = shell.take_navigation_request() {
                        tracing::info!(
                            token = request.token,
                            byte_offset = request.byte_offset,
                            "definition navigation requested"
                        );
                        self.navigation_requests.push(request.clone());
                        pending_definition = Some(request);
                    }
                }
                sync_languages = true;
                let open_project = self
                    .shell
                    .as_mut()
                    .is_some_and(IdeShell::take_open_project_request);
                window.request_redraw();
                if open_project {
                    self.choose_project();
                }
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Right,
                ..
            } => {
                if let Some(shell) = self.shell.as_mut() {
                    shell.secondary_pointer_down(self.cursor, window.logical_size());
                }
                window.request_redraw();
            }
            WindowEvent::MouseInput {
                state: ElementState::Released,
                button: MouseButton::Left,
                ..
            } => {
                if let Some(shell) = self.shell.as_mut() {
                    shell.pointer_up();
                }
                window.request_redraw();
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let lines = match delta {
                    MouseScrollDelta::LineDelta(_, y) => -y.round() as isize * 3,
                    MouseScrollDelta::PixelDelta(position) => -(position.y / 22.0).round() as isize,
                };
                if let Some(shell) = self.shell.as_mut() {
                    shell.scroll(self.cursor, lines, window.logical_size());
                }
                window.request_redraw();
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                self.control_pressed = modifiers.state().control_key();
                self.shift_pressed = modifiers.state().shift_key();
                let navigation_hover = self.shell.as_ref().is_some_and(|shell| {
                    shell.navigation_hover(self.cursor, window.logical_size(), self.control_pressed)
                });
                let resizing = self.shell.as_ref().is_some_and(IdeShell::terminal_resizing);
                let sidebar_resizing = self.shell.as_ref().is_some_and(IdeShell::sidebar_resizing);
                window.inner().set_cursor(if sidebar_resizing {
                    CursorIcon::EwResize
                } else if resizing {
                    CursorIcon::NsResize
                } else if navigation_hover {
                    CursorIcon::Pointer
                } else {
                    CursorIcon::Default
                });
            }
            WindowEvent::KeyboardInput { event, .. } if event.state.is_pressed() => {
                if self
                    .shell
                    .as_ref()
                    .is_some_and(IdeShell::settings_dialog_open)
                {
                    match &event.logical_key {
                        Key::Named(NamedKey::Escape) => {
                            if let Some(shell) = self.shell.as_mut() {
                                shell.escape();
                            }
                        }
                        Key::Named(NamedKey::Enter) => {
                            if let Some(shell) = self.shell.as_mut() {
                                shell.key_down("Enter");
                            }
                        }
                        Key::Named(NamedKey::ArrowDown) => {
                            if let Some(shell) = self.shell.as_mut() {
                                shell.key_down("ArrowDown");
                            }
                        }
                        Key::Named(NamedKey::ArrowUp) => {
                            if let Some(shell) = self.shell.as_mut() {
                                shell.key_down("ArrowUp");
                            }
                        }
                        _ => {}
                    }
                } else if self.control_pressed
                    && self.shift_pressed
                    && matches!(&event.logical_key, Key::Character(value) if value.eq_ignore_ascii_case("j"))
                {
                    select_jdk_requested = true;
                } else if self.control_pressed
                    && self.shift_pressed
                    && matches!(&event.logical_key, Key::Character(value) if value.eq_ignore_ascii_case("t"))
                {
                    test_requested = true;
                } else if self.control_pressed
                    && self.shift_pressed
                    && matches!(&event.logical_key, Key::Character(value) if value.eq_ignore_ascii_case("b"))
                {
                    project_build_requested = true;
                } else if self.control_pressed
                    && matches!(&event.logical_key, Key::Character(value) if value.eq_ignore_ascii_case("b"))
                {
                    build_requested = true;
                } else if self.control_pressed
                    && matches!(&event.logical_key, Key::Character(value) if value.eq_ignore_ascii_case("c"))
                {
                    if let Some(shell) = self.shell.as_mut() {
                        shell.copy_selection();
                    }
                } else if self.control_pressed
                    && matches!(&event.logical_key, Key::Character(value) if value.eq_ignore_ascii_case("v"))
                {
                    if let Some(shell) = self.shell.as_mut() {
                        shell.paste_clipboard();
                    }
                } else if self.control_pressed
                    && matches!(&event.logical_key, Key::Character(value) if value.eq_ignore_ascii_case("s"))
                {
                    // Salvar é do shell, que é dono da sessão do editor.
                    if let Some(shell) = self.shell.as_mut() {
                        shell.save_active_document();
                    }
                } else if self.control_pressed && event.text.as_deref() == Some(" ") {
                    completion_requested = true;
                } else {
                    match &event.logical_key {
                        Key::Named(NamedKey::Escape) => {
                            if let Some(shell) = self.shell.as_mut() {
                                shell.escape();
                            }
                        }
                        Key::Named(NamedKey::F3) => {
                            if let Some(shell) = self.shell.as_mut() {
                                shell.toggle_search();
                            }
                        }
                        Key::Named(NamedKey::F5) => {
                            run_requested = true;
                        }
                        Key::Named(NamedKey::F8) => {
                            if let Some(shell) = self.shell.as_mut() {
                                shell.request_debug(DebugRequest::Continue);
                            }
                        }
                        Key::Named(NamedKey::F9) => {
                            if let Some(shell) = self.shell.as_mut() {
                                shell.toggle_breakpoint_at_cursor();
                            }
                        }
                        Key::Named(NamedKey::F10) => {
                            if let Some(shell) = self.shell.as_mut() {
                                shell.request_debug(DebugRequest::StepOver);
                            }
                        }
                        Key::Named(NamedKey::F11) => {
                            let request = if self.shift_pressed {
                                DebugRequest::StepOut
                            } else {
                                DebugRequest::StepInto
                            };
                            if let Some(shell) = self.shell.as_mut() {
                                shell.request_debug(request);
                            }
                        }
                        Key::Named(NamedKey::Backspace) => {
                            if let Some(shell) = self.shell.as_mut() {
                                shell.key_down("Backspace");
                            }
                        }
                        Key::Named(NamedKey::Enter) => {
                            if let Some(shell) = self.shell.as_mut() {
                                shell.key_down("Enter");
                            }
                        }
                        // Tab precisa de um braço próprio: o texto que o sistema
                        // entrega para ela é `\t`, e o caminho genérico abaixo
                        // descarta caracteres de controle.
                        Key::Named(NamedKey::Tab) => {
                            let modifiers = Modifiers {
                                shift: self.shift_pressed,
                                control: self.control_pressed,
                                ..Modifiers::default()
                            };
                            if let Some(shell) = self.shell.as_mut() {
                                shell.key_down_with_modifiers("Tab", modifiers);
                            }
                        }
                        Key::Named(NamedKey::ArrowLeft) => {
                            if let Some(shell) = self.shell.as_mut() {
                                shell.key_down("ArrowLeft");
                            }
                        }
                        Key::Named(NamedKey::ArrowRight) => {
                            if let Some(shell) = self.shell.as_mut() {
                                shell.key_down("ArrowRight");
                            }
                        }
                        Key::Named(NamedKey::ArrowDown) => {
                            if let Some(shell) = self.shell.as_mut() {
                                shell.key_down("ArrowDown");
                            }
                        }
                        Key::Named(NamedKey::ArrowUp) => {
                            if let Some(shell) = self.shell.as_mut() {
                                shell.key_down("ArrowUp");
                            }
                        }
                        // Nem todo teclado entrega Tab como tecla nomeada; em
                        // alguns layouts ela chega só como o texto `\t`, que o
                        // filtro de caracteres de controle abaixo descartaria.
                        _ if event.text.as_deref() == Some("\t") => {
                            let modifiers = Modifiers {
                                shift: self.shift_pressed,
                                control: self.control_pressed,
                                ..Modifiers::default()
                            };
                            if let Some(shell) = self.shell.as_mut() {
                                shell.key_down_with_modifiers("Tab", modifiers);
                            }
                        }
                        _ => {
                            if let Some(text) = event.text
                                && !text.chars().any(char::is_control)
                                && let Some(shell) = self.shell.as_mut()
                            {
                                // Alguns caracteres pedem completação sozinhos —
                                // em Java, o ponto. Quais são é a linguagem
                                // quem diz; o editor só pergunta.
                                if let (Some(document_id), Some(host)) =
                                    (shell.active_document(), self.language_host.as_ref())
                                {
                                    let triggers = host.trigger_characters(document_id);
                                    completion_requested |=
                                        text.chars().any(|typed| triggers.contains(&typed));
                                }
                                shell.text_input(&text);
                            }
                        }
                    }
                }
                sync_languages = true;
                window.request_redraw();
            }
            WindowEvent::RedrawRequested => {
                if let Err(error) = self.render() {
                    self.startup_error = Some(error);
                    event_loop.exit();
                }
            }
            _ => {}
        }
        if sync_languages {
            self.sync_languages();
        }
        if let Some(request) = pending_definition {
            self.navigate_to_definition(request);
            self.sync_languages();
        }
        if completion_requested {
            self.request_completion();
        }
        if select_jdk_requested {
            // `Ctrl+Shift+J` promete a página do compilador, qualquer que tenha
            // sido a última página aberta.
            if let Some(shell) = self.shell.as_mut() {
                shell.set_settings_page(SettingsPage::Compiler);
            }
            self.open_jdk_selector();
        }
        let open_settings = self
            .shell
            .as_mut()
            .is_some_and(IdeShell::take_open_settings_request);
        if open_settings {
            self.open_jdk_selector();
        }
        if let Some(request) = self.shell.as_mut().and_then(IdeShell::take_new_item_request) {
            self.create_new_item(request);
        }
        let configured_jdk = self
            .shell
            .as_mut()
            .and_then(IdeShell::take_settings_jdk_result);
        if let Some(index) = configured_jdk {
            self.select_jdk(index);
        }
        let browse_jdk = self
            .shell
            .as_mut()
            .is_some_and(IdeShell::take_browse_jdk_request);
        if browse_jdk {
            self.choose_jdk_home();
        }
        if self
            .shell
            .as_mut()
            .is_some_and(IdeShell::take_reimport_project_request)
        {
            self.reimport_project();
        }
        if project_build_requested
            || self
                .shell
                .as_mut()
                .is_some_and(IdeShell::take_build_project_request)
        {
            self.start_project_build();
        }
        self.handle_debug_requests();
        if build_requested {
            self.start_java_task(JavaTask::Compile);
        }
        if run_requested {
            self.start_java_task(JavaTask::Run);
        }
        if test_requested {
            self.start_java_task(JavaTask::Test);
        }
    }
}

fn document_change(previous: &DocumentSnapshot, current: &DocumentSnapshot) -> DocumentChange {
    let prefix = common_prefix_boundary(&previous.text, &current.text);
    let suffix = common_suffix_boundary(&previous.text, &current.text, prefix);
    let old_end = previous.text.len().saturating_sub(suffix);
    let new_end = current.text.len().saturating_sub(suffix);
    DocumentChange {
        document_id: current.id,
        version: current.version,
        range: Some(TextRange {
            start: position_at_offset(&previous.text, prefix),
            end: position_at_offset(&previous.text, old_end),
        }),
        text: current.text[prefix..new_end].to_owned(),
    }
}

fn common_prefix_boundary(left: &str, right: &str) -> usize {
    left.char_indices()
        .zip(right.char_indices())
        .take_while(|((_, left), (_, right))| left == right)
        .map(|((index, character), _)| index + character.len_utf8())
        .last()
        .unwrap_or(0)
}

fn common_suffix_boundary(left: &str, right: &str, prefix: usize) -> usize {
    left[prefix..]
        .chars()
        .rev()
        .zip(right[prefix..].chars().rev())
        .take_while(|(left, right)| left == right)
        .map(|(character, _)| character.len_utf8())
        .sum()
}

fn position_at_offset(text: &str, offset: usize) -> TextPosition {
    let prefix = &text[..offset.min(text.len())];
    let line = prefix.bytes().filter(|byte| *byte == b'\n').count();
    let column = prefix
        .rsplit_once('\n')
        .map_or(prefix, |(_, line)| line)
        .chars()
        .count();
    TextPosition {
        line: line as u32,
        column: column as u32,
    }
}

/// Gabarito do arquivo Java criado.
///
/// A declaração de pacote vem do caminho, que é a regra da linguagem: o
/// diretório espelha o pacote, e um arquivo sem `package` na pasta errada não
/// compila. Um pacote raiz não declara nada.
fn java_source(request: &NewItemRequest, name: &str) -> String {
    let declaration = if request.package.is_empty() {
        String::new()
    } else {
        format!("package {};\n\n", request.package)
    };
    let keyword = match request.kind {
        NewItemKind::Interface => "interface",
        // Um pacote criado com nome de tipo produz uma classe: é o tipo mais
        // comum, e quem quer interface pede interface no menu.
        NewItemKind::Class | NewItemKind::Package => "class",
    };
    format!("{declaration}public {keyword} {name} {{\n}}\n")
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    init_logging("info")?;
    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(ControlFlow::Wait);
    let mut app = NativeIde::default();
    event_loop.run_app(&mut app)?;
    if let Some(error) = app.startup_error {
        return Err(error.into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use ide_project_model::{BuildSystemId, ModuleId, ProjectModule, SourceRoots};

    use super::*;

    /// Ctrl+clique encontra a definição em outro arquivo do projeto, para
    /// qualquer forma de declarar um tipo.
    ///
    /// `record` não estava no índice: navegar até um DTO — a forma mais comum
    /// de declarar um no Java moderno — não encontrava nada, enquanto classes e
    /// interfaces funcionavam.
    #[test]
    fn navigation_finds_definitions_declared_in_other_files() {
        let root = std::env::temp_dir().join(format!("er-ide-nav-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let pacote = root.join("src");
        assert!(std::fs::create_dir_all(&pacote).is_ok());
        assert!(std::fs::write(pacote.join("Pedido.java"), "public record Pedido(String v) {}
").is_ok());
        assert!(std::fs::write(pacote.join("Servico.java"), "public interface Servico {}
").is_ok());
        assert!(std::fs::write(pacote.join("Estado.java"), "public enum Estado { ATIVO }
").is_ok());
        assert!(std::fs::write(pacote.join("Ajuda.java"), "public class Ajuda {}
").is_ok());
        let uso = pacote.join("Uso.java");
        let texto = "public class Uso { void f() { Pedido p; Servico s; Estado e; Ajuda a; } }
";
        assert!(std::fs::write(&uso, texto).is_ok());

        let language_host = LanguageHost::new(&root);
        assert!(
            language_host
                .register(Arc::new(JavaLanguageProvider::new()))
                .is_ok()
        );
        let mut ide = NativeIde {
            language_host: Some(language_host),
            shell: Some(match IdeShell::open(&root) {
                Ok(shell) => shell,
                Err(error) => panic!("projeto não abriu: {error}"),
            }),
            ..NativeIde::default()
        };
        let document_id = match ide.shell.as_mut().map(|shell| shell.open_file(&uso)) {
            Some(Ok(id)) => id,
            _ => panic!("documento não abriu"),
        };
        ide.sync_languages();

        for (token, arquivo) in [
            ("Pedido", "Pedido.java"),
            ("Servico", "Servico.java"),
            ("Estado", "Estado.java"),
            ("Ajuda", "Ajuda.java"),
        ] {
            let byte_offset = texto.find(token).unwrap_or_default();
            ide.navigate_to_definition(NavigationRequest {
                document_id,
                byte_offset,
                token: token.to_owned(),
            });
            let mensagem = ide
                .shell
                .as_ref()
                .map(|shell| shell.status_message().to_owned())
                .unwrap_or_default();
            assert!(
                mensagem.contains(arquivo),
                "{token} deveria levar a {arquivo}, veio: {mensagem}"
            );
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    /// O código já aparece colorido no primeiro quadro.
    ///
    /// O realce era pedido só dentro do tratamento de eventos, então o texto
    /// ficava sem cor até o primeiro clique ou troca de aba.
    #[test]
    fn the_code_is_highlighted_before_the_first_interaction() {
        let root = std::env::temp_dir().join(format!("er-ide-realce-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        assert!(std::fs::create_dir_all(&root).is_ok());
        let file = root.join("Exemplo.java");
        assert!(std::fs::write(&file, "public class Exemplo {}").is_ok());

        let language_host = LanguageHost::new(&root);
        assert!(
            language_host
                .register(Arc::new(JavaLanguageProvider::new()))
                .is_ok()
        );

        let mut ide = NativeIde {
            language_host: Some(language_host),
            shell: Some(match IdeShell::open(&root) {
                Ok(shell) => shell,
                Err(error) => panic!("projeto não abriu: {error}"),
            }),
            ..NativeIde::default()
        };
        if let Some(shell) = ide.shell.as_mut() {
            assert!(shell.open_file(&file).is_ok());
        }

        let keyword_colored = |ide: &mut NativeIde| {
            let colors = ui_core::Theme::default().colors;
            ide.shell
                .as_mut()
                .map(|shell| shell.paint(Size::new(1_280.0, 800.0)))
                .unwrap_or_default()
                .iter()
                .any(|command| {
                    matches!(
                        command,
                        ui_render_api::PaintCommand::DrawText(text)
                            if text.text == "public" && text.color == colors.syntax_keyword
                    )
                })
        };
        assert!(
            !keyword_colored(&mut ide),
            "sem o realce pedido, o texto sai sem cor"
        );

        // É isto que a inicialização passou a fazer.
        ide.sync_languages();
        assert!(
            keyword_colored(&mut ide),
            "depois de pedir o realce, o código aparece colorido"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    /// Ciclo completo: abrir abas, gravar, reabrir a IDE e encontrá-las de volta.
    #[test]
    fn the_open_tabs_come_back_with_the_project() {
        let root = std::env::temp_dir().join(format!("er-ide-abas-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let project = root.join("projeto");
        assert!(std::fs::create_dir_all(&project).is_ok());
        let first = project.join("Primeiro.java");
        let second = project.join("Segundo.java");
        assert!(std::fs::write(&first, "class Primeiro {}").is_ok());
        assert!(std::fs::write(&second, "class Segundo {}").is_ok());
        let config_file = root.join("config.toml");

        // Sessão de trabalho: dois arquivos abertos pelo Explorer.
        let mut shell = match IdeShell::open(&project) {
            Ok(shell) => shell,
            Err(error) => panic!("projeto não abriu: {error}"),
        };
        assert!(shell.open_file(&first).is_ok());
        assert!(shell.open_file(&second).is_ok());

        let mut config = AppConfig::default();
        assert!(config.remember_workspace(&project, &config_file).is_ok());
        assert!(
            config
                .remember_documents(
                    &shell.open_document_paths(),
                    shell.active_document_path().as_deref(),
                    &config_file,
                )
                .is_ok()
        );

        // Nova inicialização: a mesma restauração que `initialize` faz.
        let reopened = match AppConfig::load(&config_file) {
            Ok(config) => config,
            Err(error) => panic!("releitura falhou: {error}"),
        };
        let mut restored = match IdeShell::open(&project) {
            Ok(shell) => shell,
            Err(error) => panic!("projeto não reabriu: {error}"),
        };
        assert_eq!(restored.tab_count(), 0, "a IDE abre sem abas");
        for document in reopened.workspace.resolved_documents(&project) {
            assert!(restored.open_file(&document).is_ok());
        }
        assert_eq!(restored.open_document_paths(), vec![first, second.clone()]);
        assert_eq!(restored.active_document_path(), Some(second));

        let _ = std::fs::remove_dir_all(&root);
    }

    fn maven_descriptor() -> ProjectDescriptor {
        ProjectDescriptor {
            build_system: BuildSystemId(MAVEN_BUILD_SYSTEM_ID.to_owned()),
            root: PathBuf::from("/w"),
            manifest: PathBuf::from("/w/pom.xml"),
            name: None,
            wrapper: None,
        }
    }

    fn model_with_roots() -> ProjectModel {
        let mut source_roots = SourceRoots::default();
        source_roots.push_main(PathBuf::from("/w/app/src/main/java"));
        source_roots.push_generated(PathBuf::from("/w/app/target/generated-sources/annotations"));
        let mut model = ProjectModel::new(
            BuildSystemId(MAVEN_BUILD_SYSTEM_ID.to_owned()),
            "/w",
            "demo",
        );
        model.modules.push(ProjectModule {
            id: ModuleId("app".to_owned()),
            name: "app".to_owned(),
            root: PathBuf::from("/w/app"),
            manifest: PathBuf::from("/w/app/pom.xml"),
            coordinates: None,
            packaging: "jar".to_owned(),
            source_roots,
            dependencies: Vec::new(),
            output_directory: PathBuf::from("/w/app/target/classes"),
            test_output_directory: PathBuf::from("/w/app/target/test-classes"),
            children: Vec::new(),
            plugins: Vec::new(),
        });
        model
    }

    #[test]
    fn project_sources_keep_generated_code_and_drop_files_outside_the_model() {
        let files = vec![
            PathBuf::from("/w/app/src/main/java/Main.java"),
            PathBuf::from("/w/app/target/generated-sources/annotations/Generated.java"),
            PathBuf::from("/w/scripts/Helper.java"),
        ];
        let model = model_with_roots();

        let filtered = project_sources(files.clone(), Some(&model));
        assert_eq!(filtered.len(), 2);
        assert!(filtered.contains(&PathBuf::from(
            "/w/app/target/generated-sources/annotations/Generated.java"
        )));
        assert!(!filtered.contains(&PathBuf::from("/w/scripts/Helper.java")));
    }

    #[test]
    fn project_sources_fall_back_to_the_workspace_scan() {
        let files = vec![PathBuf::from("/w/scripts/Helper.java")];
        assert_eq!(project_sources(files.clone(), None), files);
        assert_eq!(
            project_sources(files.clone(), Some(&model_with_roots())),
            files,
            "um projeto sem fontes sob suas raízes não deve zerar a compilação"
        );
    }

    #[test]
    fn startup_reopens_the_last_project_and_falls_back_to_the_current_directory() {
        let root = std::env::temp_dir().join(format!("er-ide-startup-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let project = root.join("projeto");
        assert!(std::fs::create_dir_all(&project).is_ok());
        let current = PathBuf::from("/w/atual");

        let mut config = AppConfig::default();
        config.workspace.last_path = Some(project.clone());
        assert_eq!(
            startup_root(&config, Some(current.clone())),
            Some(project.clone()),
            "o último projeto tem prioridade sobre o diretório atual"
        );

        config.workspace.last_path = Some(root.join("removido"));
        assert_eq!(
            startup_root(&config, Some(current.clone())),
            Some(current.clone()),
            "uma pasta que sumiu não impede a IDE de abrir"
        );

        assert_eq!(
            startup_root(&AppConfig::default(), Some(current)),
            Some(PathBuf::from("/w/atual")),
            "sem registro, vale o diretório atual"
        );
        assert!(startup_root(&AppConfig::default(), None).is_none());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn default_goals_compile_main_sources_in_each_build_system() {
        assert_eq!(
            default_goals(&maven_descriptor()),
            vec!["compile".to_owned()]
        );
        let gradle = ProjectDescriptor {
            build_system: BuildSystemId(GRADLE_BUILD_SYSTEM_ID.to_owned()),
            ..maven_descriptor()
        };
        assert_eq!(default_goals(&gradle), vec!["classes".to_owned()]);
    }
}
