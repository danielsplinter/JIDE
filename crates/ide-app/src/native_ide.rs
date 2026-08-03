use std::{
    path::{Path, PathBuf},
    sync::{Arc, mpsc},
    time::{Duration, Instant},
};

#[cfg(test)]
use crate::bootstrap::default_goals;
use crate::bootstrap::{java_source, project_sources, startup_root};
use crate::bridges::position_at_offset;
use crate::controllers::{
    DebugController as AppDebugController, DocumentController, ImportedProject, LanguageController,
    NativeWindowState, ProjectController, RuntimeState, TaskController as AppTaskController,
    TypeSearchOutcome, WorkspaceController,
};
use crate::ui_bridge::{UiAction, UiBridge};
use crate::{debug, java_contribution, run, style_contribution, typescript_contribution};
use ide_application::{
    ApplicationCommand, DebugRequest, IdeEvent, NavigationRequest, NewItemRequest,
    OpenDocumentRequest, RenameDocumentRequest, SaveDocumentRequest, SearchScope,
    TaskExecutionContext, TaskId,
};
use ide_core::{AppConfig, ToolRole, config_path};
use ide_debug_api::{DebugEvent, StepKind};
use ide_domain::{
    DefinitionRequest, DocumentId, DocumentSnapshot, LanguageId, ProviderId, SymbolKind,
    TextPosition, TextRange,
};
use ide_language_host::{LanguageHost, LanguageToolchainConfig};
use ide_process::{NativeProcessSupervisor, ProcessSupervisor};
use ide_project::{build::ProjectImportRequest, model::ProjectModel};
use ide_toolchain_api::DetectionContext;
use ide_ui::{
    ContentSearchHit, DebugView, IdeShell, SettingsPage, TYPE_SEARCH_LIMIT, TypeSearchHit,
};
use ide_workspace::FileNode;
#[cfg(test)]
use language_java::GRADLE_BUILD_SYSTEM_ID;
#[cfg(test)]
use language_java::MAVEN_BUILD_SYSTEM_ID;
use language_java::JAVA_PROVIDER_ID;
use ui_core::{Modifiers, Point, Size, WindowId};
use ui_render_api::{FrameInfo, UiRenderer};
use ui_render_wgpu::WgpuRenderer;
use ui_window_api::WindowRequest;
use ui_window_winit::WinitWindow;

/// Quanto tempo sem uso derruba um provider de linguagem.
///
/// Cinco minutos é longo o bastante para não punir quem alterna entre duas
/// linguagens no mesmo trabalho, e curto o bastante para devolver a memória de
/// uma linguagem que se parou de usar. Ver a fase 3b da `23`.
const LANGUAGE_IDLE_LIMIT: std::time::Duration = std::time::Duration::from_secs(300);

/// De quanto em quanto se pergunta se há provider ocioso.
///
/// O tique da janela é de 30 ms, e perguntar trinta vezes por segundo por algo
/// que muda em minutos seria trabalho por trabalho.
const SUSPENSION_CHECK: std::time::Duration = std::time::Duration::from_secs(10);

/// De quanto em quanto a memória é medida.
///
/// Consultar a tabela de processos custa; e o número que interessa muda em
/// segundos, não em milissegundos.
const MEMORY_CHECK: std::time::Duration = std::time::Duration::from_secs(5);

use winit::{
    application::ApplicationHandler,
    event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow},
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
pub(super) struct NativeIde {
    window: NativeWindowState,
    workspace: WorkspaceController,
    documents: DocumentController,
    languages: LanguageController,
    project: ProjectController,
    tasks: AppTaskController,
    debug: AppDebugController,
    ui: UiBridge,
    runtime: RuntimeState,
}

impl NativeIde {
    pub(super) fn take_startup_error(&mut self) -> Option<String> {
        self.runtime.startup_error.take()
    }

    fn initialize(&mut self, event_loop: &ActiveEventLoop) -> Result<(), String> {
        let request = WindowRequest {
            title: "ER IDE — Rust Native IDE".to_owned(),
            // O tamanho de restauração: é para ele que a janela volta quando
            // alguém desmaximiza.
            logical_size: Size::new(1280.0, 800.0),
            // Uma IDE abre ocupando a tela: editor, explorador, terminal e
            // painel de depuração convivem, e em janela pequena nenhum deles
            // cabe inteiro.
            maximized: true,
        };
        let window = WinitWindow::create_hidden(event_loop, WindowId(1), &request)
            .map_err(|error| error.to_string())?;
        let renderer = pollster::block_on(WgpuRenderer::new(window.inner().clone()))
            .map_err(|error| error.to_string())?;
        self.runtime.config_path = config_path();
        if let Some(path) = self.runtime.config_path.as_ref() {
            match AppConfig::load(path) {
                Ok(config) => self.runtime.config = config,
                Err(error) => {
                    tracing::warn!(%error, path = %path.display(), "configuração ignorada");
                }
            }
        }
        // O formato de ferramentas mudou na fase 0 da `23`. Sem esta tradução,
        // quem já usa a IDE abriria com as escolhas silenciosamente vazias — e
        // regravar sem migrar apagaria o arquivo antigo de vez.
        if crate::bootstrap::migrate_legacy_toolchains(&mut self.runtime.config)
            && let Some(path) = self.runtime.config_path.as_ref()
            && let Err(error) = self.runtime.config.save(path)
        {
            tracing::warn!(%error, "escolhas de ferramenta migradas não puderam ser gravadas");
        }
        self.ui
            .replace_event_bus(self.runtime.config.event_capacity.max(1));
        let root = startup_root(&self.runtime.config, std::env::current_dir().ok())
            .ok_or_else(|| "não foi possível determinar o diretório do projeto".to_owned())?;
        let nativo = Arc::new(NativeProcessSupervisor::default());
        // O supervisor concreto fica guardado para a medição: ele é quem sabe
        // quais processos externos existem, porque foi ele que os criou.
        self.runtime.processes = Some(Arc::clone(&nativo));
        let processes: Arc<dyn ProcessSupervisor> = nativo;
        let java = java_contribution::contribution(processes.clone());
        let language_host = LanguageHost::new(&root);
        language_host
            .register(java.provider.clone())
            .map_err(|error| error.to_string())?;
        self.languages.toolchains.register_contribution(&java);
        self.tasks
            .controller
            .register_contribution(&java)
            .map_err(|error| error.to_string())?;
        self.languages
            .contributions
            .register(java)
            .map_err(|error| error.to_string())?;
        // A segunda linguagem. Ela entra pelo mesmo caminho da primeira, e é
        // isso que a fase 1 da `23` vem provar: registrar uma linguagem nova não
        // exige tocar em nada acima daqui.
        let typescript = typescript_contribution::contribution(processes.clone());
        language_host
            .register(typescript.provider.clone())
            .map_err(|error| error.to_string())?;
        // O analisador externo entra ao lado do nativo, e não no lugar dele: são
        // dois providers para a mesma extensão, e a ordem entre eles é a
        // declarada logo abaixo.
        language_host
            .register(typescript_contribution::service_provider(processes.clone()))
            .map_err(|error| error.to_string())?;
        self.languages.toolchains.register_contribution(&typescript);
        self.tasks
            .controller
            .register_contribution(&typescript)
            .map_err(|error| error.to_string())?;
        // A ordem entre providers é **declarada**, e não herdada da ordenação
        // alfabética dos identificadores. Ver a fase 3b da `23`.
        language_host
            .configure_selection(
                typescript_contribution::language_id(),
                typescript_contribution::selection(),
            )
            .map_err(|error| error.to_string())?;
        self.languages
            .contributions
            .register(typescript)
            .map_err(|error| error.to_string())?;
        let estilo = style_contribution::contribution();
        language_host
            .register(estilo.provider.clone())
            .map_err(|error| error.to_string())?;
        self.languages
            .contributions
            .register(estilo)
            .map_err(|error| error.to_string())?;
        self.languages.host = Some(Arc::new(language_host));
        let tree = self
            .workspace
            .service
            .scan(&root)
            .map_err(|error| error.to_string())?;
        let mut shell = IdeShell::from_tree(tree);
        shell.set_ui_catalog(self.languages.contributions.ui_catalog());
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
        for document in self.runtime.config.workspace.resolved_documents(&root) {
            if let Err(error) = self.open_document_in_shell(&mut shell, &document, 0, 0) {
                tracing::warn!(%error, path = %document.display(), "aba não pôde ser reaberta");
            }
        }
        if let Some(active) = self
            .runtime
            .config
            .workspace
            .resolved_active_document(&root)
            && let Err(error) = self.open_document_in_shell(&mut shell, &active, 0, 0)
        {
            tracing::warn!(%error, path = %active.display(), "aba ativa não pôde ser restaurada");
        }
        self.documents.remembered = shell.open_document_paths();
        self.ui.shell = Some(shell);
        self.publish_event(IdeEvent::WorkspaceOpened { root: root.clone() });
        // A raiz precisa estar registrada **antes** de resolver ferramenta:
        // é ela que decide se vale a sobreposição do projeto ou o padrão.
        self.runtime.workspace_root = Some(root.clone());
        let secondary = self.tool_home(&java_contribution::language_id(), ToolRole::Secondary);
        java_contribution::register_build_systems(
            &mut self.project.build_systems,
            processes.clone(),
            secondary,
        );
        typescript_contribution::register_build_systems(&mut self.project.build_systems, processes);
        let (tool_sender, tool_events) = mpsc::channel();
        self.tasks.sender = Some(tool_sender);
        self.tasks.events = Some(tool_events);
        self.detect_all_toolchains(&root);
        self.detect_maven();
        self.import_project(&root);
        if let Some(shell) = self.ui.shell.as_mut() {
            shell.set_debug_target(
                &self.runtime.config.debug.host,
                self.runtime.config.debug.port,
            );
        }
        self.debug.session = self
            .languages
            .contributions
            .get(&java_contribution::language_id())
            .and_then(|contribution| contribution.debugger.clone())
            .and_then(debug::DebugController::start);
        self.window.renderer = Some(renderer);
        window
            .inner()
            .set_title(&format!("ER IDE — {}", root.display()));
        window.show();
        self.window.window = Some(window);
        // As abas restauradas já estão abertas, mas ninguém pediu o realce
        // delas. Isso fica para **depois do primeiro quadro**: ativar o provider
        // indexa o JDK e os fontes do projeto, e feito aqui deixava a janela já
        // visível em branco por mais de um segundo. Primeiro a IDE aparece
        // montada; o realce chega no quadro seguinte.
        self.runtime.languages_pending = true;
        if let Some(window) = self.window.window.as_ref() {
            window.request_redraw();
        }
        Ok(())
    }

    fn render(&mut self) -> Result<(), String> {
        let window = self
            .window
            .window
            .as_ref()
            .ok_or_else(|| "window unavailable".to_owned())?;
        let size = window.logical_size();
        let shell = self
            .ui
            .shell
            .as_mut()
            .ok_or_else(|| "shell unavailable".to_owned())?;
        let iniciado = Instant::now();
        let commands = shell.paint(size);
        let pintura = iniciado.elapsed();
        let renderer = self
            .window
            .renderer
            .as_mut()
            .ok_or_else(|| "renderer unavailable".to_owned())?;
        let marca = Instant::now();
        renderer
            .begin_frame(FrameInfo {
                window_id: window.handle().id,
                logical_size: size,
                scale_factor: window.scale_factor(),
            })
            .map_err(|error| error.to_string())?;
        let abertura = marca.elapsed();
        let marca = Instant::now();
        renderer
            .submit(&commands)
            .map_err(|error| error.to_string())?;
        let submissao = marca.elapsed();
        let marca = Instant::now();
        let resultado = renderer.end_frame().map_err(|error| error.to_string());
        if perf_enabled() {
            let metricas = renderer.metrics();
            eprintln!(
                "[perf] quadro: pintura {pintura:?} | begin_frame {abertura:?} | submit {submissao:?} | present {:?} | {} comandos | moldes reaproveitados {} de {} guardados",
                marca.elapsed(),
                commands.len(),
                metricas.text_cache_hits,
                metricas.text_cache_entries,
            );
        }
        resultado
    }

    fn publish_event(&self, event: IdeEvent) {
        if let Err(error) = self.ui.publish(event) {
            tracing::warn!(?error, "evento da aplicação descartado");
        }
    }

    fn drain_application_events(&self) {
        match self.ui.drain_events() {
            Ok(events) => {
                for event in events {
                    tracing::debug!(?event, "evento da aplicação");
                }
            }
            Err(error) => tracing::warn!(?error, "barramento de eventos indisponível"),
        }
    }

    fn open_document_in_shell(
        &self,
        shell: &mut IdeShell,
        path: &Path,
        line: usize,
        column: usize,
    ) -> Result<DocumentId, String> {
        let text = self
            .workspace
            .service
            .read_document(path)
            .map_err(|error| error.to_string())?;
        Ok(shell.show_location(path, text, line, column))
    }

    fn open_document(&mut self, request: OpenDocumentRequest) {
        let result = self.workspace.read_document(&request.path);
        let mut opened = false;
        if let Some(shell) = self.ui.shell.as_mut() {
            match result {
                Ok(text) => {
                    shell.show_location(&request.path, text, request.line, request.column);
                    opened = true;
                }
                Err(error) => shell.set_status_message(error.to_string()),
            }
        }
        // Abrir muda o conjunto de documentos entregue ao host. Cliques comuns
        // no editor não mudam texto nem abas e, portanto, não devem pagar esta
        // sincronização.
        if opened {
            self.sync_languages();
            if let Some(shell) = self.ui.shell.as_mut() {
                shell.set_status_message(format!("Opened {}", request.path.display()));
            }
        }
    }

    fn save_document(&mut self, request: SaveDocumentRequest) {
        let result = self.workspace.save_document(&request.path, &request.text);
        if result.is_ok()
            && let Some(language_host) = &self.languages.host
        {
            // O índice acompanha a gravação: a classe criada agora entra na
            // completação sem esperar a próxima ativação. Ver a fase 4 da `19`.
            let _ = pollster::block_on(language_host.file_changed(&request.path));
        }
        let Some(shell) = self.ui.shell.as_mut() else {
            return;
        };
        match result {
            Ok(()) => shell.document_saved(request.document_id, request.revision, &request.path),
            Err(error) => shell.set_status_message(error.to_string()),
        }
    }

    fn reload_workspace(&mut self) {
        let Some(root) = self
            .ui
            .shell
            .as_ref()
            .map(|shell| shell.workspace_path().to_path_buf())
        else {
            return;
        };
        let result = self.workspace.scan(&root);
        let Some(shell) = self.ui.shell.as_mut() else {
            return;
        };
        match result {
            Ok(tree) => shell.replace_workspace_tree(tree),
            Err(error) => shell.set_status_message(error.to_string()),
        }
    }

    fn choose_project(&mut self) {
        let Some(current) = self
            .ui
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
        match self.workspace.scan(&folder) {
            Ok(tree) => {
                if let Some(language_host) = &self.languages.host {
                    if let Err(error) = pollster::block_on(language_host.shutdown())
                        .and_then(|()| language_host.set_workspace_root(&folder))
                        .and_then(|()| language_host.set_source_roots(Vec::new()).map(|_| ()))
                        .and_then(|()| {
                            language_host.enable(&ProviderId(JAVA_PROVIDER_ID.to_owned()))
                        })
                    {
                        self.runtime.startup_error = Some(error.to_string());
                        return;
                    }
                }
                self.documents.clear();
                let mut shell = IdeShell::from_tree(tree);
                shell.set_ui_catalog(self.languages.contributions.ui_catalog());
                self.ui.shell = Some(shell);
                self.publish_event(IdeEvent::WorkspaceOpened {
                    root: folder.clone(),
                });
                self.remember_project(&folder);
                self.detect_all_toolchains(&folder);
                self.import_project(&folder);
                self.sync_languages();
                if let Some(window) = self.window.window.as_ref() {
                    window
                        .inner()
                        .set_title(&format!("ER IDE — {}", folder.display()));
                    window.request_redraw();
                }
            }
            Err(error) => {
                self.runtime.startup_error = Some(format!(
                    "failed to open project {}: {error}",
                    folder.display()
                ));
            }
        }
    }

    fn sync_languages(&mut self) {
        let snapshots = self
            .ui
            .shell
            .as_ref()
            .map(IdeShell::document_snapshots)
            .unwrap_or_default();
        self.sync_document_events(&snapshots);
        let syntax = self
            .languages
            .synchronize_documents(&mut self.documents, &snapshots);
        if let Some(shell) = self.ui.shell.as_mut() {
            for snapshot in syntax {
                shell.set_syntax_snapshot(snapshot);
            }
        }
    }

    /// Espera o realce pendente e o instala.
    ///
    /// Só para testes: na janela, quem recolhe é o relógio, e esperar seria
    /// justamente o que se quer evitar.
    #[cfg(test)]
    fn settle_syntax(&mut self) {
        while self.languages.pending_syntax() > 0 {
            let realces = self.languages.collect_syntax();
            if realces.is_empty() {
                std::thread::yield_now();
                continue;
            }
            if let Some(shell) = self.ui.shell.as_mut() {
                for snapshot in realces {
                    shell.set_syntax_snapshot(snapshot);
                }
            }
        }
    }

    fn sync_document_events(&mut self, snapshots: &[DocumentSnapshot]) {
        for event in self.documents.synchronize_application(snapshots) {
            self.publish_event(event);
        }
    }

    fn navigate_to_definition(&mut self, request: NavigationRequest) {
        let Some(language_host) = &self.languages.host else {
            return;
        };
        let Some(document) = self.documents.language.get(&request.document_id) else {
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
                    self.open_document(OpenDocumentRequest::new(&location.path).at(
                        location.range.start.line as usize,
                        location.range.start.column as usize,
                    ));
                } else if let Some(shell) = self.ui.shell.as_mut() {
                    shell.set_status_message(format!("Definition not found: {}", request.token));
                }
            }
            Err(error) => {
                if let Some(shell) = self.ui.shell.as_mut() {
                    shell.set_status_message(error.to_string());
                }
            }
        }
    }

    /// Responde à busca de tipo enquanto houver consulta esperando.
    ///
    /// A tela guarda o que foi digitado e pede uma vez; quem tem o provedor de
    /// linguagem é o app, e é aqui que a pergunta vira resposta.
    fn answer_type_search(&mut self, query: String) {
        let Some(host) = self.languages.host.as_ref().map(Arc::clone) else {
            return;
        };
        // A pergunta vai a **todas** as linguagens registradas, uma extensão de
        // cada. Ela dizia `java` à mão, e com uma linguagem só ninguém percebia:
        // num projeto TypeScript, o atalho de abrir um tipo pelo nome não achava
        // nada. Ver a fase 0 da `23`, que apontou o vazamento e não o corrigiu.
        let extensoes: Vec<String> = self
            .languages
            .contributions
            .iter()
            .filter_map(|contribution| contribution.descriptor.extensions.first().cloned())
            .collect();

        // De que linguagens se está falando agora. É o que impede a busca de
        // acusar o analisador de uma linguagem que nem está no projeto.
        let abertas: Vec<String> = self
            .documents
            .application
            .values()
            .filter_map(|documento| documento.path.extension()?.to_str().map(str::to_owned))
            .collect();

        let (sender, receiver) = std::sync::mpsc::channel();
        let cancel = self.languages.type_search.start(receiver);
        std::thread::spawn(move || {
            let mut encontrados = Vec::new();
            let mut ultimo_erro = None;
            for extensao in extensoes {
                if cancel.is_cancelled() {
                    return;
                }
                // O token vai **dentro** do contexto: assim a desistência chega
                // ao analisador, e não só a este laço.
                let mut contexto = host.request_context();
                contexto.cancellation = cancel.clone();
                match pollster::block_on(host.workspace_types(
                    contexto,
                    &extensao,
                    query.clone(),
                    TYPE_SEARCH_LIMIT,
                )) {
                    Ok(symbols) => encontrados.extend(symbols),
                    // Uma linguagem que não sabe responder não estraga a busca
                    // das outras: sem índice, a resposta dela é "nada", e nada é
                    // uma resposta.
                    Err(error) => ultimo_erro = Some(error.to_string()),
                }
            }
            encontrados.truncate(TYPE_SEARCH_LIMIT);
            // Vazio pode ser "não existe" ou "ninguém sabia responder", e os dois
            // se parecem na tela. Num projeto Angular sem `node_modules`, o
            // analisador externo não sobe, o provider nativo assume — e ele **não
            // tem índice**, então responde `Ok` com nada. Sem esta verificação, a
            // busca dizia "nenhum tipo encontrado" num projeto cheio de tipos, e
            // a causa não aparecia em lugar nenhum.
            let failure = encontrados
                .is_empty()
                .then(|| {
                    host.providers()
                        .ok()
                        .and_then(|providers| analisador_ausente(providers, &abertas))
                        .or(ultimo_erro)
                })
                .flatten();
            let _ = sender.send(TypeSearchOutcome {
                found: encontrados,
                failure,
            });
        });
        if let Some(shell) = self.ui.shell.as_mut() {
            shell.set_status_message("Procurando tipos…".to_owned());
        }
    }

    /// Recolhe o resultado da busca por tipo, se já chegou.
    ///
    /// **Perguntar ao analisador não cabe num quadro.** A busca rodava inline, e
    /// num projeto Angular de nove mil arquivos o `Ctrl+L` travava a janela — o
    /// mesmo defeito da busca textual, um atalho ao lado. A completação por `.`
    /// não travava porque ela pergunta sobre **um** arquivo; esta pergunta é
    /// sobre o projeto inteiro.
    fn collect_type_search(&mut self) -> bool {
        let Some(resultado) = self.languages.type_search.collect() else {
            return false;
        };
        let Some(shell) = self.ui.shell.as_mut() else {
            return false;
        };
        if resultado.found.is_empty() {
            shell.set_status_message(
                resultado
                    .failure
                    .unwrap_or_else(|| "Nenhum tipo encontrado".to_owned()),
            );
        } else {
            shell.set_status_message(format!("{} tipos", resultado.found.len()));
        }
        shell.set_type_search_results(
            resultado
                .found
                .into_iter()
                .filter_map(|symbol| {
                    Some(TypeSearchHit {
                        name: symbol.name,
                        kind: type_kind_label(symbol.kind)?.to_owned(),
                        location: symbol.location,
                    })
                })
                .collect(),
        );
        true
    }

    /// Responde à busca textual usando a árvore já carregada no Explorer.
    ///
    /// O serviço de workspace recebe raízes e extensões explícitas. Nenhuma
    /// convenção de linguagem fica embutida na busca.
    /// Dispara a busca textual numa thread própria.
    ///
    /// **O que estava errado não era a busca, era o lugar dela.** Ela rodava
    /// inline, e num projeto de 8 958 arquivos com o cache do sistema frio isso é
    /// mais de um minuto sem a janela responder — que foi o travamento relatado
    /// ao abrir um projeto Angular grande. O limite de resultados não salvava:
    /// ele para quando a lista enche, e uma consulta que não acha nada percorre
    /// tudo.
    ///
    /// O escopo continua explícito, e nenhuma convenção de linguagem fica
    /// embutida aqui.
    fn answer_content_search(&mut self, query: &str) {
        let mut source_roots = self
            .project
            .imported
            .as_ref()
            .map(|project| project.model.source_roots())
            .unwrap_or_default();
        if source_roots.is_empty()
            && let Some(shell) = self.ui.shell.as_ref()
        {
            let root_names = self.languages.contributions.ui_catalog().source_root_names;
            collect_named_source_roots(shell.workspace_tree(), &root_names, &mut source_roots);
        }
        let extensions = self
            .languages
            .contributions
            .iter()
            .flat_map(|contribution| contribution.descriptor.extensions.iter().cloned())
            .collect();
        let scope = SearchScope::new(source_roots, extensions);
        let Some(tree) = self.ui.shell.as_ref().map(|shell| shell.workspace_tree().clone()) else {
            return;
        };

        let (sender, receiver) = std::sync::mpsc::channel();
        let cancel = self.workspace.search.start(receiver);
        let mensagem = format!("Procurando “{query}”…");
        let query = query.to_owned();
        // O serviço é criado na thread: ele não guarda estado do projeto, e
        // mandá-lo para lá emprestado prenderia a IDE ao tempo da varredura.
        std::thread::spawn(move || {
            let achados = ide_workspace::WorkspaceService::native()
                .search_content(&tree, &scope, &query, TYPE_SEARCH_LIMIT, &cancel);
            // Erro de envio é a IDE ter fechado, ou outra busca ter tomado o
            // lugar desta. Nos dois casos não há o que fazer com o resultado.
            let _ = sender.send(achados);
        });
        if let Some(shell) = self.ui.shell.as_mut() {
            shell.set_status_message(mensagem);
        }
    }

    /// Recolhe o resultado da busca, se já chegou. Não espera por nada.
    fn collect_content_search(&mut self) -> bool {
        let Some(found) = self.workspace.search.collect() else {
            return false;
        };
        let quantos = found.len();
        if let Some(shell) = self.ui.shell.as_mut() {
            shell.set_content_search_results(
                found
                    .into_iter()
                    .map(|hit| ContentSearchHit {
                        preview: hit.preview,
                        location: ide_domain::Location {
                            path: hit.path,
                            range: TextRange {
                                start: TextPosition {
                                    line: hit.line.saturating_sub(1) as u32,
                                    column: hit.column.saturating_sub(1) as u32,
                                },
                                end: TextPosition {
                                    line: hit.line.saturating_sub(1) as u32,
                                    column: hit.column.saturating_sub(1) as u32,
                                },
                            },
                        },
                    })
                    .collect(),
            );
            // Nenhuma ocorrência precisa ser dito. Uma lista vazia e a mensagem
            // \u201cProcurando\u2026\u201d parada na barra ficariam iguais a uma busca que
            // nunca terminou.
            shell.set_status_message(if quantos == 0 {
                "Nenhuma ocorrência".to_owned()
            } else {
                format!("{quantos} ocorrências")
            });
        }
        true
    }

    /// Pede à linguagem o plano de acessores que a tela solicitou.
    fn answer_accessor_request(&mut self) {
        let Some(kind) = self
            .ui
            .shell
            .as_mut()
            .and_then(IdeShell::take_accessor_request)
        else {
            return;
        };
        let (Some(host), Some(shell)) = (self.languages.host.as_ref(), self.ui.shell.as_ref())
        else {
            return;
        };
        let (Some(document_id), Some(position)) =
            (shell.active_document(), shell.cursor_position())
        else {
            return;
        };
        let plano = pollster::block_on(host.accessor_plan(
            host.request_context(),
            document_id,
            position,
            kind,
        ));
        if let Some(shell) = self.ui.shell.as_mut() {
            match plano {
                Ok(plano) => shell.show_accessor_plan(kind, plano),
                Err(error) => shell.set_status_message(error.to_string()),
            }
        }
    }

    /// Responde ao pedido de renomear: onde o nome aparece no projeto.
    ///
    /// A pergunta é da linguagem que atende a extensão do arquivo. Sem provider
    /// para ela — um `.md`, por exemplo — a resposta é vazia, e renomear passa a
    /// ser só mover o arquivo, que continua sendo uma resposta útil.
    fn answer_rename_request(&mut self) {
        let Some(path) = self
            .ui
            .shell
            .as_mut()
            .and_then(IdeShell::take_rename_request)
        else {
            return;
        };
        let nome = path
            .file_stem()
            .and_then(|valor| valor.to_str())
            .unwrap_or_default()
            .to_owned();
        let extensao = path
            .extension()
            .and_then(|valor| valor.to_str())
            .unwrap_or_default()
            .to_owned();
        let referencias = self
            .languages
            .host
            .as_ref()
            .map(|host| {
                pollster::block_on(host.references_to_name(host.request_context(), &extensao, nome))
                    .unwrap_or_default()
            })
            .unwrap_or_default();
        if let Some(shell) = self.ui.shell.as_mut() {
            shell.show_rename(path, referencias);
        }
    }

    /// Renomeia o arquivo e reescreve tudo o que citava o nome antigo.
    ///
    /// Tudo ou nada: os conteúdos novos são calculados **antes** de qualquer
    /// gravação, e uma falha no meio desfaz o que já foi escrito. Meio caminho
    /// aqui é um projeto que não compila e um usuário sem saber onde parou.
    fn rename_document(&mut self, request: RenameDocumentRequest) {
        use ide_workspace::rewrite_occurrences;
        let mut originais: Vec<(PathBuf, String)> = Vec::new();
        let mut novos: Vec<(PathBuf, String)> = Vec::new();
        for arquivo in &request.occurrences {
            let Ok(texto) = self.workspace.read_document(&arquivo.path) else {
                continue;
            };
            let novo = rewrite_occurrences(
                &texto,
                &arquivo.ranges,
                &request.old_name,
                &request.new_name,
            );
            if novo != texto {
                originais.push((arquivo.path.clone(), texto));
                novos.push((arquivo.path.clone(), novo));
            }
        }

        let mut gravados: Vec<PathBuf> = Vec::new();
        for (caminho, conteudo) in &novos {
            if let Err(error) = self.workspace.save_document(caminho, conteudo) {
                // Desfaz o que já foi escrito: o projeto volta ao que era.
                for gravado in &gravados {
                    if let Some((_, original)) =
                        originais.iter().find(|(caminho, _)| caminho == gravado)
                    {
                        let _ = self.workspace.save_document(gravado, original);
                    }
                }
                if let Some(shell) = self.ui.shell.as_mut() {
                    shell.set_status_message(format!("Renomeação desfeita: {error}"));
                }
                return;
            }
            gravados.push(caminho.clone());
        }

        if let Err(error) = self.workspace.rename_path(&request.from, &request.to) {
            for gravado in &gravados {
                if let Some((_, original)) =
                    originais.iter().find(|(caminho, _)| caminho == gravado)
                {
                    let _ = self.workspace.save_document(gravado, original);
                }
            }
            if let Some(shell) = self.ui.shell.as_mut() {
                shell.set_status_message(format!("Renomeação desfeita: {error}"));
            }
            return;
        }

        // O índice da linguagem é montado na ativação e não é incremental: ele
        // ainda guarda o caminho e o nome antigos. Sem refazê-lo, a próxima
        // renomeação listaria um arquivo que não existe mais.
        if let Some(host) = self.languages.host.as_ref()
            && pollster::block_on(host.reactivate()).is_ok()
        {
            self.documents.language.clear();
        }
        // A aba do arquivo renomeado, se estiver aberta, precisa seguir o novo
        // caminho: sem isso a próxima gravação recriaria o arquivo antigo.
        if let Some(shell) = self.ui.shell.as_mut() {
            shell.follow_renamed_path(&request.from, &request.to);
            shell.set_status_message(format!(
                "{} renomeado para {} em {} arquivo(s)",
                request.old_name,
                request.new_name,
                gravados.len() + 1
            ));
        }
        self.reload_workspace();
    }

    /// Monta o construtor escolhido na janela e o entrega à tela.
    ///
    /// Separado do plano porque o texto depende da escolha: a tela sabe **quais**
    /// campos foram marcados, e só a linguagem sabe escrever o construtor.
    fn answer_constructor_request(&mut self) {
        let Some((fields, insert_at)) = self
            .ui
            .shell
            .as_mut()
            .and_then(IdeShell::take_constructor_request)
        else {
            return;
        };
        let (Some(host), Some(shell)) = (self.languages.host.as_ref(), self.ui.shell.as_ref())
        else {
            return;
        };
        let (Some(document_id), Some(position)) =
            (shell.active_document(), shell.cursor_position())
        else {
            return;
        };
        let fonte = pollster::block_on(host.constructor_source(
            host.request_context(),
            document_id,
            position,
            fields,
        ));
        let escreveu = match (fonte, self.ui.shell.as_mut()) {
            (Ok(fonte), Some(shell)) => shell.insert_constructor(fonte, insert_at),
            (Err(error), Some(shell)) => {
                shell.set_status_message(error.to_string());
                false
            }
            _ => false,
        };
        // O texto gerado entra depois da checagem de revisão do evento, então é
        // aqui que o realce novo precisa ser pedido: sem isso o construtor fica
        // sem cor até a primeira tecla.
        if escreveu {
            self.sync_languages();
            if let Some(window) = self.window.window.as_ref() {
                window.request_redraw();
            }
        }
    }

    fn request_completion(&mut self) {
        let Some(language_host) = &self.languages.host else {
            return;
        };
        // Com a inspeção aberta, a pergunta é sobre um tipo, e não sobre uma
        // posição num arquivo: ali não existe arquivo.
        if let Some(shell) = self.ui.shell.as_ref()
            && let Some((text, offset)) = shell.inspection_member_context()
            && let Some(document_id) = shell.active_document()
        {
            let access = match pollster::block_on(language_host.member_access(
                language_host.request_context(),
                document_id,
                text,
                offset,
            )) {
                Ok(Some(access)) => access,
                Ok(None) => return,
                Err(error) => {
                    if let Some(shell) = self.ui.shell.as_mut() {
                        shell.set_inspection_message(error.to_string());
                    }
                    return;
                }
            };
            let (type_name, prefix) = self
                .ui
                .shell
                .as_ref()
                .map(|shell| shell.inspection_member_target(&access.receiver, access.prefix))
                .unwrap_or_default();
            let answered = pollster::block_on(language_host.type_members(
                language_host.request_context(),
                document_id,
                type_name,
                prefix,
            ));
            if let Some(shell) = self.ui.shell.as_mut() {
                match answered {
                    Ok(items) => shell.set_completions(items),
                    Err(error) => shell.set_inspection_message(error.to_string()),
                }
            }
            return;
        }
        let Some(request) = self
            .ui
            .shell
            .as_ref()
            .and_then(IdeShell::completion_request)
        else {
            return;
        };
        match pollster::block_on(language_host.completion(language_host.request_context(), request))
        {
            Ok(items) => {
                if let Some(shell) = self.ui.shell.as_mut() {
                    shell.set_completions(items);
                }
            }
            Err(error) => {
                if let Some(shell) = self.ui.shell.as_mut() {
                    shell.set_status_message(error.to_string());
                }
            }
        }
    }

    /// Encontra as instalações do Maven e restaura a escolhida.
    ///
    /// O que estava gravado tem prioridade sobre o que foi detectado: a escolha
    /// do usuário vale mais do que a ordem em que a máquina responde. Um caminho
    /// que deixou de existir é ignorado, e a IDE volta ao primeiro encontrado.
    fn detect_maven(&mut self) {
        self.project.maven.installations = language_java::detect_maven_installations();
        let gravado = self.tool_home(&java_contribution::language_id(), ToolRole::Secondary);
        self.project.maven.selected = match gravado {
            Some(home) => match language_java::maven_installation_from_home(&home) {
                Some(instalacao) => Some(self.project.maven.adopt(instalacao)),
                None => (!self.project.maven.installations.is_empty()).then_some(0),
            },
            None => (!self.project.maven.installations.is_empty()).then_some(0),
        };
        if let Some(shell) = self.ui.shell.as_mut() {
            shell.set_secondary_tool_options(
                self.project.maven.labels(),
                self.project.maven.selected,
            );
        }
    }

    /// Abre o seletor de pasta para apontar uma instalação do Maven.
    fn choose_maven_home(&mut self) {
        let Some(home) = rfd::FileDialog::new()
            .set_title("Escolher instalação do Maven")
            .pick_folder()
        else {
            return;
        };
        let Some(instalacao) = language_java::maven_installation_from_home(&home) else {
            if let Some(shell) = self.ui.shell.as_mut() {
                shell.set_status_message(format!(
                    "{} não tem bin/mvn: não é uma instalação do Maven",
                    home.display()
                ));
            }
            return;
        };
        let indice = self.project.maven.adopt(instalacao);
        if let Some(shell) = self.ui.shell.as_mut() {
            shell.set_secondary_tool_options(self.project.maven.labels(), Some(indice));
        }
    }

    /// Aplica e grava o Maven escolhido na janela.
    fn select_maven(&mut self, index: usize) {
        let Some(instalacao) = self.project.maven.installations.get(index).cloned() else {
            return;
        };
        self.project.maven.selected = Some(index);
        self.remember_toolchains();
        // O adaptador guarda a instalação: sem refazer o registro, o build
        // continuaria chamando o Maven anterior.
        java_contribution::register_build_systems(
            &mut self.project.build_systems,
            Arc::new(NativeProcessSupervisor::default()),
            self.project.maven.home(),
        );
        if let Some(shell) = self.ui.shell.as_mut() {
            shell.set_status_message(format!("Maven: {}", instalacao.home.display()));
        }
    }

    /// Abre o seletor de pasta para a ferramenta de uma seção.
    ///
    /// A seção volta a ser uma linguagem aqui, pelo registro de contribuições —
    /// a tela nunca soube de qual linguagem falava, e é assim que deve ser.
    fn browse_tool(&mut self, section: &str, role: ToolRole) {
        let Some(language) = self.languages.contributions.language_for_section(section) else {
            tracing::warn!(section, "seção de configurações sem contribuição");
            return;
        };
        match role {
            ToolRole::Primary => self.choose_toolchain_home(&language),
            ToolRole::Secondary => self.browse_secondary_tool(&language),
        }
    }

    /// Aplica a instalação escolhida na lista de uma seção.
    fn select_tool(&mut self, section: &str, role: ToolRole, index: usize) {
        let Some(language) = self.languages.contributions.language_for_section(section) else {
            tracing::warn!(section, "seção de configurações sem contribuição");
            return;
        };
        match role {
            ToolRole::Primary => self.select_toolchain(&language, index),
            ToolRole::Secondary => self.select_secondary_tool(&language, index),
        }
    }

    /// A segunda ferramenta ainda é atendida por linguagem, uma a uma.
    ///
    /// A principal já é genérica — o registro de toolchains é por linguagem
    /// desde sempre. A segunda não é: o que existe é um controlador com detecção
    /// e rótulos próprios, e generalizá-lo é trabalho de quando houver uma
    /// segunda linguagem com segunda ferramenta.
    ///
    /// O que a fase 0 da `23` conserta é o comando **poder** distinguir: antes,
    /// o botão genérico chamava a ferramenta de Java direto, e com duas seções
    /// não haveria como dizer qual foi clicada.
    fn browse_secondary_tool(&mut self, language: &LanguageId) {
        if language == &java_contribution::language_id() {
            self.choose_maven_home();
            return;
        }
        tracing::warn!(language = language.0, "seção sem segunda ferramenta");
    }

    fn select_secondary_tool(&mut self, language: &LanguageId, index: usize) {
        if language == &java_contribution::language_id() {
            self.select_maven(index);
            return;
        }
        tracing::warn!(language = language.0, "seção sem segunda ferramenta");
    }

    /// Ferramenta em vigor para uma seção, resolvida no projeto aberto.
    ///
    /// A ordem — sobreposição do projeto, padrão global, nada — está em
    /// `ide-core`. Aqui só se diz de qual projeto se fala.
    fn tool_home(&self, language: &LanguageId, role: ToolRole) -> Option<PathBuf> {
        self.runtime
            .config
            .toolchains
            .resolved(self.runtime.workspace_root.as_deref(), &language.0, role)
            .map(|tool| tool.home)
    }

    /// Grava as ferramentas escolhidas no arquivo de configuração do usuário.
    ///
    /// As duas juntas, no mesmo arquivo: são a mesma decisão — com que
    /// ferramentas este usuário compila — e separá-las faria uma sobreviver ao
    /// reinício e a outra não.
    ///
    /// A escrita vai para a **sobreposição do projeto aberto**, porque foi ali
    /// que a escolha foi feita. Sem projeto aberto, vira padrão global.
    fn remember_toolchains(&mut self) {
        let Some(path) = self.runtime.config_path.clone() else {
            return;
        };
        let root = self.runtime.workspace_root.clone();
        let language = java_contribution::language_id();
        let primary = self.selected_jdk_home();
        let secondary = self.project.maven.home();
        self.runtime.config.toolchains.choose(
            root.as_deref(),
            &language.0,
            ToolRole::Primary,
            primary.as_deref(),
        );
        self.runtime.config.toolchains.choose(
            root.as_deref(),
            &language.0,
            ToolRole::Secondary,
            secondary.as_deref(),
        );
        if let Err(error) = self.runtime.config.save(&path) {
            tracing::warn!(%error, "configuração de ferramentas não pôde ser gravada");
        }
    }

    /// Casa da ferramenta principal de Java em uso, para gravar junto da segunda.
    fn selected_jdk_home(&self) -> Option<PathBuf> {
        self.languages
            .toolchains
            .selection(&java_contribution::language_id())
            .and_then(|selection| selection.selected())
            .map(|installation| installation.home.clone())
    }

    /// Detecta a ferramenta de **todas** as linguagens que declaram uma.
    ///
    /// Era chamada só para Java, e com uma linguagem só ninguém percebia. A
    /// lista sai das contribuições: uma linguagem nova entra sem que este laço
    /// mude, que é o que a fase 1 da `23` veio provar.
    fn detect_all_toolchains(&mut self, workspace_root: &Path) {
        let linguagens: Vec<LanguageId> = self
            .languages
            .contributions
            .iter()
            .filter(|contribution| contribution.toolchain.is_some())
            .map(|contribution| contribution.descriptor.language_id.clone())
            .collect();
        let mut resumo = Vec::new();
        for language_id in linguagens {
            if let Some(linha) = self.detect_toolchains(&language_id, workspace_root) {
                resumo.push(linha);
            }
        }
        if let Some(shell) = self.ui.shell.as_mut()
            && !resumo.is_empty()
        {
            // Uma mensagem só, com todas: uma por linguagem faria a última
            // apagar as outras, e quem abre veria a de quem chegou por último.
            shell.set_status_message(resumo.join("  ·  "));
        }
    }

    /// Detecta e devolve o que dizer na barra de estado, se houver o que dizer.
    fn detect_toolchains(
        &mut self,
        language_id: &LanguageId,
        workspace_root: &Path,
    ) -> Option<String> {
        let display_name = self
            .languages
            .contributions
            .get(language_id)
            .map(|contribution| contribution.descriptor.display_name.clone())
            .unwrap_or_else(|| language_id.0.clone());
        let detected = pollster::block_on(self.languages.toolchains.detect(
            language_id,
            DetectionContext {
                workspace_root: Some(workspace_root.to_path_buf()),
            },
        ));
        let resumo = match detected {
            Ok(_) => {
                // A escolha do usuário vence a detecção. Sem isto, ela era
                // gravada e ignorada na abertura seguinte — a ordem em que a
                // máquina responde decidia por ele.
                self.restore_chosen_toolchain(language_id);
                let selected = self
                    .languages
                    .toolchains
                    .selection(language_id)
                    .and_then(ide_application::ToolchainSelection::selected)
                    .map(|installation| {
                        installation
                            .version
                            .clone()
                            .unwrap_or_else(|| installation.home.to_string_lossy().into_owned())
                    });
                selected.map(|selected| format!("{display_name}: {selected}"))
            }
            Err(_) => {
                // Nenhuma instalação encontrada não é erro na abertura: o
                // projeto abre e é editável, e quem reclama é a tarefa na hora
                // de executar. Ver a fase 2 da `23`.
                self.restore_chosen_toolchain(language_id);
                None
            }
        };
        self.apply_selected_toolchain(language_id.clone());
        resumo
    }

    /// Repõe a instalação que o usuário escolheu para este projeto.
    ///
    /// A ordem — sobreposição do projeto, padrão global, detecção — está em
    /// `ide-core`. Aqui só se aplica o que ela responder.
    fn restore_chosen_toolchain(&mut self, language_id: &LanguageId) {
        let Some(home) = self.tool_home(language_id, ToolRole::Primary) else {
            return;
        };
        match pollster::block_on(
            self.languages
                .toolchains
                .add_from_home(language_id, home.clone()),
        ) {
            Ok(_) => {
                let escolhida = self
                    .languages
                    .toolchains
                    .selection(language_id)
                    .and_then(|selection| {
                        selection
                            .installations()
                            .iter()
                            .find(|installation| installation.home == home)
                            .map(|installation| installation.id.clone())
                    });
                if let Some(id) = escolhida
                    && let Some(selection) = self.languages.toolchains.selection_mut(language_id)
                    && selection.select(&id).is_err()
                {
                    tracing::warn!(language = language_id.0, "escolha gravada não pôde ser aplicada");
                }
            }
            Err(_) => {
                // O caminho gravado deixou de existir: a IDE volta a detectar,
                // em vez de recusar-se a abrir.
                tracing::warn!(
                    language = language_id.0,
                    home = %home.display(),
                    "instalação escolhida não existe mais"
                );
            }
        }
    }

    /// Como a contribuição chama a ferramenta principal desta linguagem.
    ///
    /// Vem da seção de configurações que ela declarou. A aplicação não sabe o
    /// que a ferramenta é — ela repete o rótulo que recebeu, como já faz com os
    /// nomes de tarefa e os modelos de arquivo.
    fn tool_caption(&self, language_id: &LanguageId) -> String {
        self.languages
            .contributions
            .get(language_id)
            .and_then(|contribution| contribution.settings_sections.first())
            .map(|section| section.field_caption.clone())
            .unwrap_or_else(|| "ferramenta".to_owned())
    }

    fn toolchain_labels(&self, language_id: &LanguageId) -> Vec<String> {
        self.languages
            .toolchains
            .selection(language_id)
            .map(ide_application::ToolchainSelection::installations)
            .unwrap_or_default()
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

    fn open_toolchain_selector(&mut self, language_id: &LanguageId) {
        let selected_index = self
            .languages
            .toolchains
            .selection(language_id)
            .map(|selection| {
                let selected = selection.selected().map(|value| &value.id);
                selection
                    .installations()
                    .iter()
                    .position(|installation| Some(&installation.id) == selected)
                    .unwrap_or(0)
            })
            .unwrap_or(0);
        let items = self.toolchain_labels(language_id);
        // O Maven é reposto antes de abrir: a janela guarda o que estava
        // escolhido para poder desfazer no Cancelar, e ela precisa disso pronto.
        let maven_items = self.project.maven.labels();
        let maven_selected = self.project.maven.selected;
        if let Some(shell) = self.ui.shell.as_mut() {
            shell.set_secondary_tool_options(maven_items, maven_selected);
            shell.open_settings_dialog(items, selected_index);
        }
    }

    fn choose_toolchain_home(&mut self, language_id: &LanguageId) {
        // O rótulo é o que a **contribuição** declarou na seção — "JDK" em Java,
        // "Node" em TypeScript. Escrevê-lo aqui faria a janela de uma linguagem
        // pedir a ferramenta de outra.
        let rotulo = self.tool_caption(language_id);
        let initial_directory = self
            .languages
            .toolchains
            .selection(language_id)
            .and_then(ide_application::ToolchainSelection::selected)
            .map_or_else(|| Path::new(".").to_path_buf(), |atual| atual.home.clone());
        let Some(folder) = rfd::FileDialog::new()
            .set_title(format!("Selecionar pasta: {rotulo}"))
            .set_directory(initial_directory)
            .pick_folder()
        else {
            return;
        };
        let home = folder.clone();
        match pollster::block_on(self.languages.toolchains.add_from_home(language_id, folder)) {
            Ok(index) => {
                // A pasta apontada entra na lista e fica pendente: quem aplica é
                // o Salvar, como qualquer escolha feita na janela.
                let labels = self.toolchain_labels(language_id);
                if let Some(shell) = self.ui.shell.as_mut() {
                    shell.set_toolchain_options(labels, index);
                    shell.set_status_message(format!("{rotulo} a salvar: {}", home.display()));
                }
            }
            Err(_) => {
                if let Some(shell) = self.ui.shell.as_mut() {
                    shell.set_settings_message(format!(
                        "Pasta inválida: ela não contém uma instalação de {rotulo}."
                    ));
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
        if let Err(error) = self.workspace.service.create_directory(&directory) {
            if let Some(shell) = self.ui.shell.as_mut() {
                shell.set_new_item_message(error.to_string());
            }
            return;
        }
        let created_file = if request.name.is_empty() {
            None
        } else {
            let path = directory.join(format!("{}.java", request.name));
            let source = java_source(&request, &request.name);
            if let Err(error) = self.workspace.service.create_file(&path, &source) {
                if let Some(shell) = self.ui.shell.as_mut() {
                    shell.set_new_item_message(error.to_string());
                }
                return;
            }
            Some(path)
        };
        if let Some(shell) = self.ui.shell.as_mut() {
            shell.close_new_item_dialog();
            // A árvore precisa reler o disco: o que acabou de nascer não estava
            // na varredura anterior.
            match self.workspace.scan(shell.workspace_path()) {
                Ok(tree) => shell.replace_workspace_tree(tree),
                Err(error) => shell.set_status_message(error.to_string()),
            }
            // O que nasceu dentro de uma pasta fechada não aparece: revelar o
            // caminho é parte de ter criado.
            shell.reveal_in_explorer(&directory);
            match &created_file {
                Some(path) => {
                    shell.set_status_message(format!("Criado {}", path.display()));
                    match self.workspace.read_document(path) {
                        Ok(text) => {
                            shell.show_document(path, text);
                        }
                        Err(error) => shell.set_status_message(error.to_string()),
                    }
                }
                None => shell.set_status_message(format!("Criado {}", directory.display())),
            }
        }
        self.sync_languages();
    }

    /// Informa ao host de linguagens qual instalação está escolhida.
    ///
    /// A biblioteca padrão que a completação conhece vem dessa instalação, então
    /// trocar de instalação derruba os providers ativos: eles indexaram a
    /// biblioteca padrão da anterior.
    /// Os documentos abertos são esquecidos de propósito — o provider novo nasce
    /// sem nenhum, e a próxima sincronização os reabre.
    fn apply_selected_toolchain(&mut self, language_id: LanguageId) {
        let Some(language_host) = &self.languages.host else {
            return;
        };
        let toolchain = self
            .languages
            .toolchains
            .selection(&language_id)
            .and_then(ide_application::ToolchainSelection::selected)
            .map(|installation| LanguageToolchainConfig {
                language_id: language_id.clone(),
                installation_root: installation.home.clone(),
                properties: Default::default(),
            });
        match language_host.set_toolchain(language_id, toolchain) {
            Ok(true) => {
                if pollster::block_on(language_host.reactivate()).is_ok() {
                    self.documents.language.clear();
                }
            }
            Ok(false) => {}
            Err(error) => {
                tracing::warn!(%error, "não foi possível registrar a instalação escolhida");
            }
        }
    }

    fn apply_project_roots_to_languages(&mut self) {
        let Some(language_host) = &self.languages.host else {
            return;
        };
        let roots = self
            .project
            .imported
            .as_ref()
            .map(|project| project.model.source_roots())
            .unwrap_or_default();
        match language_host.set_source_roots(roots) {
            Ok(true) => {
                if pollster::block_on(language_host.reactivate()).is_ok() {
                    self.documents.language.clear();
                }
            }
            Ok(false) => {}
            Err(error) => tracing::warn!(%error, "não foi possível registrar source roots"),
        }
    }

    fn select_toolchain(&mut self, language_id: &LanguageId, index: usize) {
        let Some(selection) = self.languages.toolchains.selection_mut(language_id) else {
            return;
        };
        let Some(installation) = selection.installations().get(index).cloned() else {
            return;
        };
        if let Err(error) = selection.select(&installation.id) {
            if let Some(shell) = self.ui.shell.as_mut() {
                shell.set_status_message(error.to_string());
            }
            return;
        }
        let rotulo = self.tool_caption(language_id);
        if let Some(shell) = self.ui.shell.as_mut() {
            shell.set_status_message(format!("{rotulo}: {}", installation.home.display()));
        }
        self.apply_selected_toolchain(language_id.clone());
        self.remember_toolchains();
    }

    /// Detecta o build system da raiz e importa módulos e dependências.
    ///
    /// A importação é nativa: nenhum processo externo é iniciado aqui, apenas os
    /// manifestos do projeto são lidos. O Maven ou o Gradle só executam quando o
    /// usuário pede um build.
    /// Publica as tarefas que o projeto aberto declara.
    ///
    /// Nem toda tarefa é conhecida na partida: as de npm são os `scripts` do
    /// `package.json`, e mudam de projeto para projeto. Declarar um conjunto
    /// fixo na contribuição seria adivinhar nomes — a tabela de compatibilidade
    /// que a `23` proíbe, com outro nome.
    /// Derruba os providers que ninguém usa há um tempo.
    ///
    /// Quem tem relógio é a aplicação; o host tem o estado. A conta é barata —
    /// olhar carimbo de tempo — e roda no mesmo tique de 30 ms que recolhe o
    /// realce, mas só a cada `SUSPENSION_CHECK`, porque não há por que perguntar
    /// trinta vezes por segundo.
    ///
    /// O que se ganha é o índice de uma linguagem que se parou de usar: a `20`
    /// mediu 103 MB só o de Java, e na fase 3c serão centenas de megabytes de um
    /// processo externo. Ver a fase 3b da `23`.
    fn suspend_idle_languages(&mut self) {
        let agora = std::time::Instant::now();
        if self
            .languages
            .last_suspension_check
            .is_some_and(|anterior| agora.duration_since(anterior) < SUSPENSION_CHECK)
        {
            return;
        }
        self.languages.last_suspension_check = Some(agora);
        let Some(host) = self.languages.host.as_ref() else {
            return;
        };
        match pollster::block_on(host.suspend_idle(LANGUAGE_IDLE_LIMIT)) {
            Ok(suspensos) => {
                for provider in suspensos {
                    tracing::info!(provider = provider.0, "provider suspenso por ociosidade");
                }
            }
            Err(error) => tracing::warn!(%error, "suspensão por ociosidade falhou"),
        }
    }

    /// Mede o que a IDE custa, somando o que roda fora dela.
    ///
    /// **São dois números, e o segundo é o que ninguém veria.** O analisador de
    /// TypeScript é um processo próprio: contabilmente separado, fisicamente a
    /// mesma RAM. No Windows, o Gerenciador de Tarefas ainda o soma sob a IDE.
    ///
    /// Sem isto, o teto de heap do analisador o derrubaria, o provider nativo
    /// assumiria, e a IDE ficaria **silenciosamente pior** — sem tipos, sem
    /// completação — sem dizer por quê. É a família de defeito que a `21`
    /// nomeou, e medir é o que a tira do escuro.
    fn measure_memory(&mut self) {
        let agora = std::time::Instant::now();
        if self
            .languages
            .last_memory_check
            .is_some_and(|anterior| agora.duration_since(anterior) < MEMORY_CHECK)
        {
            return;
        }
        self.languages.last_memory_check = Some(agora);
        self.notice_fallen_providers();
        let externos = self
            .runtime
            .processes
            .as_ref()
            .map(|supervisor| supervisor.live_conversations())
            .unwrap_or_default();
        let leitura = ide_core::MemoryMeter::read(&externos);
        // Só se anuncia quando muda de patamar: repetir o mesmo número a cada
        // cinco segundos apagaria a mensagem que estivesse na barra.
        if leitura != self.languages.memory {
            self.languages.memory = leitura;
            if let Some(shell) = self.ui.shell.as_mut() {
                shell.set_memory_usage(leitura.own_mb, leitura.external_mb);
            }
        }
    }

    /// Diz quando um analisador caiu, em vez de deixar a IDE piorar calada.
    ///
    /// O host já degrada sozinho: o provider que morre vira `Failed`, sai das
    /// rotas, e o nativo assume — o `.ts` continua colorido. **Esse é o
    /// problema.** Sem aviso, o que se vê é a completação por tipo simplesmente
    /// parar de existir, sem erro nenhum, e ninguém tem por onde começar.
    ///
    /// Cada queda se anuncia uma vez: repetir a cada verificação apagaria
    /// qualquer outra mensagem da barra para sempre.
    fn notice_fallen_providers(&mut self) {
        let Some(host) = self.languages.host.as_ref() else {
            return;
        };
        let Ok(providers) = host.providers() else {
            return;
        };
        let caidos: Vec<_> = providers
            .into_iter()
            .filter(|snapshot| snapshot.state == ide_language_api::ProviderState::Failed)
            .filter(|snapshot| {
                self.languages
                    .announced_failures
                    .insert(snapshot.metadata.provider_id.0.clone())
            })
            .collect();
        for snapshot in caidos {
            let nome = snapshot.metadata.display_name.clone();
            let motivo = snapshot.last_error.unwrap_or_else(|| "sem detalhe".to_owned());
            tracing::warn!(provider = %snapshot.metadata.provider_id.0, %motivo, "provider caiu");
            if let Some(shell) = self.ui.shell.as_mut() {
                shell.set_status_message(format!(
                    "{nome} parou; seguindo com a análise nativa ({motivo})"
                ));
            }
        }
    }

    fn refresh_project_tasks(&mut self, root: &Path) {
        let language = typescript_contribution::language_id();
        let tasks = typescript_contribution::project_tasks(root);
        self.tasks
            .controller
            .replace_language_tasks(&language, tasks.clone());
        self.languages
            .contributions
            .replace_language_tasks(&language, tasks);
        if let Some(shell) = self.ui.shell.as_mut() {
            shell.set_ui_catalog(self.languages.contributions.ui_catalog());
        }
    }

    fn import_project(&mut self, root: &Path) {
        let previous = self.project.reset_import();
        self.refresh_project_tasks(root);
        if let Some(shell) = self.ui.shell.as_mut() {
            shell.set_project_summary(None);
        }
        if self.project.build_systems.is_empty() {
            return;
        }
        let detected = match pollster::block_on(self.project.build_systems.detect(root)) {
            Ok(detected) => detected,
            Err(error) => {
                if let Some(shell) = self.ui.shell.as_mut() {
                    shell.set_status_message(format!("Falha ao detectar o projeto: {error}"));
                }
                return;
            }
        };
        let Some((adapter, descriptor)) = detected else {
            return;
        };
        let mut request = ProjectImportRequest::new(descriptor.clone());
        if let Some(jdk) = self
            .languages
            .toolchains
            .selection(&java_contribution::language_id())
            .and_then(ide_application::ToolchainSelection::selected)
        {
            request = request
                .with_environment_variable("JAVA_HOME", jdk.home.to_string_lossy().into_owned());
        }
        match pollster::block_on(adapter.import_project(request)) {
            Ok(model) => {
                let summary = model.summary();
                let event = IdeEvent::ProjectImported {
                    root: descriptor.root.clone(),
                    build_system: descriptor.build_system.0.clone(),
                };
                self.project.imported = Some(ImportedProject {
                    adapter,
                    manifest_modified: self.workspace.modified_at(&descriptor.manifest),
                    descriptor,
                    model,
                });
                self.apply_project_roots_to_languages();
                if let Some(shell) = self.ui.shell.as_mut() {
                    shell.set_project_summary(Some(summary.clone()));
                    shell.set_status_message(format!("Projeto importado: {summary}"));
                }
                self.publish_event(event);
            }
            Err(error) => {
                // O último modelo válido continua valendo, e o carimbo do
                // manifesto é atualizado para não repetir a falha a cada segundo.
                let summary = previous.map(|mut project| {
                    project.manifest_modified = self.workspace.modified_at(&descriptor.manifest);
                    let summary = project.model.summary();
                    self.project.imported = Some(project);
                    summary
                });
                if let Some(shell) = self.ui.shell.as_mut() {
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
            .ui
            .shell
            .as_ref()
            .map(|shell| shell.workspace_path().to_path_buf())
        else {
            return;
        };
        self.import_project(&root);
        if self.project.imported.is_none()
            && let Some(shell) = self.ui.shell.as_mut()
        {
            shell.set_status_message("Nenhum projeto Maven ou Gradle na raiz do workspace");
        }
    }

    /// Reimporta quando o manifesto muda fora da IDE, sem varrer o disco a cada frame.
    fn watch_manifest(&mut self) -> bool {
        let Some(project) = self.project.imported.as_ref() else {
            return false;
        };
        if self
            .project
            .last_manifest_check
            .is_some_and(|checked| checked.elapsed() < Duration::from_secs(1))
        {
            return false;
        }
        self.project.last_manifest_check = Some(Instant::now());
        let modified = self.workspace.modified_at(&project.descriptor.manifest);
        if modified == project.manifest_modified {
            return false;
        }
        let root = project.descriptor.root.clone();
        self.import_project(&root);
        if let Some(shell) = self.ui.shell.as_mut() {
            shell.set_status_message("Manifesto alterado: projeto reimportado");
        }
        true
    }

    /// Guarda o projeto aberto para que a próxima inicialização o reabra.
    ///
    /// Falhar ao gravar não pode impedir o trabalho: o projeto continua aberto e
    /// o usuário só perde a reabertura automática, avisado na barra de status.
    fn remember_project(&mut self, root: &Path) {
        let Some(path) = self.runtime.config_path.clone() else {
            return;
        };
        if let Err(error) = self.runtime.config.remember_workspace(root, &path) {
            tracing::warn!(%error, path = %path.display(), "configuração não pôde ser gravada");
            if let Some(shell) = self.ui.shell.as_mut() {
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
        if let Some(project) = self.project.imported.as_ref() {
            let roots = project.model.source_roots();
            if !roots.is_empty() {
                return roots;
            }
        }
        let Some(workspace) = self
            .ui
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
            self.runtime.config.run.command.as_deref(),
            target.as_ref(),
            run::RunMode::Plain,
        );
        let Some(command) = command else {
            if let Some(shell) = self.ui.shell.as_mut() {
                shell.set_status_message(
                    "A IDE não sabe executar este projeto; defina `run.command` na configuração",
                );
            }
            return;
        };
        let result = self
            .ui
            .shell
            .as_mut()
            .map(|shell| shell.run_in_terminal(&command));
        if let (Some(Err(error)), Some(shell)) = (result, self.ui.shell.as_mut()) {
            shell.set_status_message(error);
        }
    }

    /// Botão de parar: interrompe a aplicação iniciada pela IDE.
    ///
    /// Uma sessão de depuração aberta é desconectada antes, para o depurador
    /// não ficar apontando para um processo que está terminando.
    fn stop_application(&mut self) {
        if self.debug.view.attached {
            self.send_debug(debug::DebugCommand::Detach);
        }
        let result = self.ui.shell.as_mut().map(IdeShell::stop_application);
        if let (Some(Err(error)), Some(shell)) = (result, self.ui.shell.as_mut()) {
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
                self.runtime.config.run.command.as_deref(),
                target.as_ref(),
                run::RunMode::Debug { host, port },
            );
            let Some(command) = command else {
                if let Some(shell) = self.ui.shell.as_mut() {
                    shell.set_status_message(
                        "Nada escutando no alvo e a IDE não sabe subir este projeto; \
                         defina `debug.command` na configuração ou inicie a aplicação",
                    );
                }
                return;
            };
            let started = self
                .ui
                .shell
                .as_mut()
                .map(|shell| shell.run_in_terminal(&command));
            match started {
                Some(Ok(())) => {
                    attempts = 120;
                    if let Some(shell) = self.ui.shell.as_mut() {
                        shell.set_status_message(format!(
                            "Subindo a aplicação e aguardando {host}:{port}"
                        ));
                    }
                }
                Some(Err(error)) => {
                    if let Some(shell) = self.ui.shell.as_mut() {
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
        let project = self.project.imported.as_ref()?;
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
        if self.runtime.config.debug.host == host && self.runtime.config.debug.port == port {
            return;
        }
        let Some(path) = self.runtime.config_path.clone() else {
            return;
        };
        if let Err(error) = self.runtime.config.remember_debug_target(host, port, &path) {
            tracing::warn!(%error, "alvo de depuração não pôde ser gravado");
        }
    }

    fn handle_debug_request(&mut self, request: DebugRequest) {
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
                if let Some(thread) = self.debug.thread {
                    self.send_debug(debug::DebugCommand::Refresh {
                        thread,
                        frame: index,
                    });
                }
            }
            DebugRequest::ExpandInspection(path) => {
                if let Some(thread) = self.debug.thread {
                    self.send_debug(debug::DebugCommand::ExpandInspection {
                        thread,
                        frame: self.debug.view.selected_frame,
                        path,
                    });
                }
            }
            DebugRequest::Evaluate(expression) => {
                if let Some(thread) = self.debug.thread {
                    self.send_debug(debug::DebugCommand::Evaluate {
                        thread,
                        frame: self.debug.view.selected_frame,
                        expression,
                    });
                } else if let Some(shell) = self.ui.shell.as_mut() {
                    shell.set_status_message("Nenhuma thread parada para inspecionar");
                }
            }
        }
    }

    fn sync_breakpoints(&mut self, path: PathBuf) {
        let lines = self
            .ui
            .shell
            .as_ref()
            .map(|shell| shell.breakpoints_for(&path))
            .unwrap_or_default();
        self.send_debug(debug::DebugCommand::SetBreakpoints { path, lines });
    }

    fn dispatch_application_commands(&mut self, mut direct: Vec<ApplicationCommand>) {
        for command in self.ui.actions(std::mem::take(&mut direct)) {
            match command {
                UiAction::OpenDocument(request) => self.open_document(request),
                UiAction::SaveDocument(request) => self.save_document(request),
                UiAction::RenameDocument(request) => self.rename_document(request),
                UiAction::ReloadWorkspace => self.reload_workspace(),
                UiAction::LoadDirectory(path) => {
                    let raiz = self
                        .ui
                        .shell
                        .as_ref()
                        .map(|shell| shell.workspace_root().to_path_buf());
                    if let Some(raiz) = raiz {
                        let niveis = self.workspace.scan_path(&raiz, &path);
                        if let Some(shell) = self.ui.shell.as_mut() {
                            shell.insert_path_children(niveis);
                        }
                    }
                }
                UiAction::OpenProject => self.choose_project(),
                UiAction::OpenSettings => {
                    self.open_toolchain_selector(&java_contribution::language_id());
                }
                UiAction::OpenToolchainSettings => {
                    if let Some(shell) = self.ui.shell.as_mut() {
                        shell.set_settings_page(SettingsPage::Contribution(0));
                    }
                    self.open_toolchain_selector(&java_contribution::language_id());
                }
                UiAction::BrowseTool { section, role } => self.browse_tool(&section, role),
                UiAction::SelectTool {
                    section,
                    role,
                    index,
                } => self.select_tool(&section, role, index),
                UiAction::BuildProject => self.start_project_build(),
                UiAction::ReimportProject => self.reimport_project(),
                UiAction::RunProject => self.run_application(),
                UiAction::ExecuteTask(task_id) => self.start_task(task_id),
                UiAction::StopProject => self.stop_application(),
                UiAction::Navigate(request) => {
                    tracing::info!(
                        token = request.token,
                        byte_offset = request.byte_offset,
                        "definition navigation requested"
                    );
                    self.ui.remember_navigation(request.clone());
                    self.navigate_to_definition(request);
                    self.sync_languages();
                }
                UiAction::CreateItem(request) => self.create_new_item(request),
                UiAction::BreakpointsChanged(path) => self.sync_breakpoints(path),
                UiAction::Debug(request) => self.handle_debug_request(request),
                UiAction::SearchTypes(query) => self.answer_type_search(query),
                UiAction::SearchContent(query) => self.answer_content_search(&query),
            }
        }
        self.remember_documents();
    }

    /// Mantém as abas abertas registradas na configuração.
    ///
    /// O conjunto é comparado a cada quadro em vez de sinalizado em cada ponto
    /// que abre ou fecha uma aba: comparar alguns caminhos é barato, e nenhum
    /// caminho novo pode esquecer de avisar.
    fn remember_documents(&mut self) {
        let Some(shell) = self.ui.shell.as_ref() else {
            return;
        };
        let open = shell.open_document_paths();
        let active = shell.active_document_path();
        if open == self.documents.remembered
            && active.as_deref() == self.runtime.config.workspace.active_document.as_deref()
        {
            return;
        }
        let Some(path) = self.runtime.config_path.clone() else {
            self.documents.remembered = open;
            return;
        };
        if let Err(error) = self
            .runtime
            .config
            .remember_documents(&open, active.as_deref(), &path)
        {
            tracing::warn!(%error, path = %path.display(), "abas não puderam ser gravadas");
        }
        self.documents.remembered = open;
    }

    fn send_debug(&self, command: debug::DebugCommand) {
        if let Some(debugger) = self.debug.session.as_ref() {
            debugger.send(command);
        }
    }

    /// Aplica os eventos da sessão ao estado apresentado pela interface.
    fn drain_debug_events(&mut self) -> bool {
        let events = self
            .debug
            .session
            .as_ref()
            .map(debug::DebugController::poll)
            .unwrap_or_default();
        if events.is_empty() {
            return false;
        }
        for event in events {
            match event {
                debug::DebugUiEvent::Session(DebugEvent::Attached { description }) => {
                    self.debug.view = DebugView {
                        attached: true,
                        status: format!("Conectado a {description}"),
                        ..DebugView::default()
                    };
                }
                debug::DebugUiEvent::Session(DebugEvent::Stopped { thread, reason }) => {
                    self.debug.thread = Some(thread);
                    self.debug.view.attached = true;
                    self.debug.view.status = debug::stop_reason_label(&reason);
                    self.send_debug(debug::DebugCommand::Refresh { thread, frame: 0 });
                }
                debug::DebugUiEvent::Session(DebugEvent::Resumed { .. }) => {
                    self.debug.view.status = "Em execução".to_owned();
                    self.debug.view.stopped_at = None;
                    self.debug.view.frames.clear();
                    self.debug.view.variables.clear();
                }
                debug::DebugUiEvent::Session(DebugEvent::Detached { reason }) => {
                    self.debug.thread = None;
                    self.debug.view = DebugView::default();
                    if let Some(shell) = self.ui.shell.as_mut() {
                        shell.set_status_message(
                            reason.unwrap_or_else(|| "Depuração encerrada".to_owned()),
                        );
                    }
                }
                debug::DebugUiEvent::Session(DebugEvent::Output { text }) => {
                    if let Some(shell) = self.ui.shell.as_mut() {
                        shell.append_tool_output(&text, false);
                    }
                }
                debug::DebugUiEvent::View {
                    thread,
                    frames,
                    variables,
                    selected,
                } => {
                    self.debug.thread = Some(thread);
                    self.debug.view.attached = true;
                    self.debug.view.stopped_at = debug::first_location(&frames);
                    self.debug.view.frames = frames;
                    self.debug.view.variables = variables;
                    self.debug.view.selected_frame = selected;
                }
                debug::DebugUiEvent::Breakpoints { path, verified } => {
                    if let Some(shell) = self.ui.shell.as_mut() {
                        shell.set_verified_breakpoints(&path, &verified);
                    }
                }
                debug::DebugUiEvent::Inspection {
                    expression,
                    value,
                    fields,
                } => {
                    if let Some(shell) = self.ui.shell.as_mut() {
                        shell.inspection_result(expression, value, fields);
                    }
                }
                debug::DebugUiEvent::InspectionFields { path, fields } => {
                    if let Some(shell) = self.ui.shell.as_mut() {
                        shell.add_inspection_fields(&path, fields);
                    }
                }
                debug::DebugUiEvent::Status(status) => {
                    if let Some(shell) = self.ui.shell.as_mut() {
                        // Com a inspeção aberta, ela cobre a barra de estado: a
                        // resposta precisa aparecer dentro da janela.
                        shell.set_inspection_message(status.clone());
                        shell.set_status_message(status);
                    }
                }
            }
        }
        if let Some(shell) = self.ui.shell.as_mut() {
            shell.set_debug_view(self.debug.view.clone());
        }
        true
    }

    /// Executa o build do sistema detectado, fora da thread da interface.
    fn start_project_build(&mut self) {
        let java_home = self
            .languages
            .toolchains
            .selection(&java_contribution::language_id())
            .and_then(ide_application::ToolchainSelection::selected)
            .map(|jdk| jdk.home.as_path());
        let Some((adapter, request, label)) = self.project.build_plan(java_home) else {
            if let Some(shell) = self.ui.shell.as_mut() {
                shell.set_status_message("Nenhum projeto Maven ou Gradle importado");
            }
            return;
        };
        let Some(shell) = self.ui.shell.as_mut() else {
            return;
        };
        shell.append_tool_output(&format!("{label}..."), false);
        shell.set_status_message(format!("{label} em execução"));
        if let Err(error) = self.tasks.execute_build(adapter, request, label) {
            shell.set_status_message(error.to_string());
        }
    }

    fn start_task(&mut self, task_id: TaskId) {
        let Some((language_id, descriptor)) = self.tasks.controller.task(&task_id) else {
            if let Some(shell) = self.ui.shell.as_mut() {
                shell.set_status_message(format!("Tarefa não registrada: {}", task_id.0));
            }
            return;
        };
        let Some(contribution) = self.languages.contributions.get(&language_id) else {
            return;
        };
        let Some(installation) = self
            .languages
            .toolchains
            .selection(&language_id)
            .and_then(ide_application::ToolchainSelection::selected)
            .cloned()
        else {
            let rotulo = self.tool_caption(&language_id);
            if let Some(shell) = self.ui.shell.as_mut() {
                shell.set_status_message(format!("Nenhuma instalação de {rotulo} escolhida"));
            }
            return;
        };
        let model = self
            .project
            .imported
            .as_ref()
            .map(|project| project.model.clone());
        let Some(shell) = self.ui.shell.as_mut() else {
            return;
        };
        let workspace = shell.workspace_path().to_path_buf();
        let extension = contribution
            .descriptor
            .extensions
            .first()
            .map(String::as_str)
            .unwrap_or_default();
        // A lista vem do filesystem, e não da árvore do Explorer: desde a `19`
        // ela é rasa, e responderia só pelo que estivesse expandido.
        let raiz = shell.workspace_root().to_path_buf();
        let source_files = project_sources(
            self.workspace.source_files(&raiz, extension),
            model.as_ref(),
        );
        if source_files.is_empty() {
            shell.set_status_message(format!(
                "Nenhum arquivo-fonte encontrado para {}",
                contribution.descriptor.display_name
            ));
            return;
        }
        let active = shell
            .document_snapshots()
            .into_iter()
            .find(|snapshot| Some(snapshot.id) == shell.active_document());
        if descriptor.requires_active_document && active.is_none() {
            shell.set_status_message("Abra um arquivo antes de executar esta tarefa");
            return;
        }
        let context = TaskExecutionContext {
            workspace_root: workspace,
            source_files,
            active_document: active,
            library_paths: model
                .as_ref()
                .map(ProjectModel::library_paths)
                .unwrap_or_default(),
            installation,
        };
        let label = format!(
            "[{}] {}",
            contribution.descriptor.display_name, descriptor.title
        );
        shell.append_tool_output(&format!("{label}..."), false);
        shell.set_status_message(format!("{label} em execução"));
        if let Err(error) = self.tasks.execute_task(task_id, context, label) {
            shell.set_status_message(error.to_string());
        }
    }
}

fn collect_named_source_roots(node: &FileNode, names: &[String], output: &mut Vec<PathBuf>) {
    if node.is_directory
        && node
            .path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| {
                names
                    .iter()
                    .any(|candidate| candidate.eq_ignore_ascii_case(name))
            })
    {
        output.push(node.path.clone());
        return;
    }
    for child in &node.children {
        collect_named_source_roots(child, names, output);
    }
}

impl ApplicationHandler for NativeIde {
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        event_loop.set_control_flow(ControlFlow::WaitUntil(
            std::time::Instant::now() + std::time::Duration::from_millis(30),
        ));
        let mut changed = self.collect_content_search();
        changed |= self.collect_type_search();
        self.suspend_idle_languages();
        self.measure_memory();
        // O realce vem da thread do provider e chega quando fica pronto: é aqui
        // que ele encontra a tela, sem que a tecla tenha esperado por ele.
        let realces = self.languages.collect_syntax();
        if !realces.is_empty() {
            if let Some(shell) = self.ui.shell.as_mut() {
                for snapshot in realces {
                    shell.set_syntax_snapshot(snapshot);
                }
            }
            changed = true;
        }
        if let Some(events) = &self.tasks.events {
            while let Ok(event) = events.try_recv() {
                if let Some(shell) = self.ui.shell.as_mut() {
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
        self.dispatch_application_commands(Vec::new());
        self.drain_application_events();
        if let (Some(window), Some(shell)) = (self.window.window.as_ref(), self.ui.shell.as_mut()) {
            changed |= shell.update_terminals();
            // Um arrasto que saiu da janela não manda mais movimento nenhum, e
            // é justamente aí que a vista precisa continuar andando. O relógio
            // já bate por causa das ferramentas; o passo pega carona nele.
            if self.window.primary_pressed {
                changed |= shell.drag_autoscroll(window.logical_size());
            }
            if changed {
                window.request_redraw();
            }
        }
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.window.is_none()
            && let Err(error) = self.initialize(event_loop)
        {
            self.runtime.startup_error = Some(error);
            event_loop.exit();
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        id: WinitWindowId,
        event: WindowEvent,
    ) {
        let Some(window) = self.window.window.as_ref() else {
            return;
        };
        if window.inner().id() != id {
            return;
        }
        let medindo = perf_enabled();
        let etiqueta = medindo.then(|| event_label(&event));
        let iniciado = Instant::now();
        let mut sync_languages = false;
        let mut completion_requested = false;
        let mut direct_commands = Vec::new();
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let Some(renderer) = self.window.renderer.as_mut() {
                    renderer.resize(size.width, size.height);
                }
                window.request_redraw();
            }
            WindowEvent::ScaleFactorChanged { .. } => {
                let size = window.inner().inner_size();
                if let Some(renderer) = self.window.renderer.as_mut() {
                    renderer.resize(size.width, size.height);
                }
                window.request_redraw();
            }
            WindowEvent::CursorMoved { position, .. } => {
                let logical = position.to_logical::<f32>(window.scale_factor());
                self.window.cursor = Point::new(logical.x, logical.y);
                let redraw = self.ui.shell.as_mut().is_some_and(|shell| {
                    shell.pointer_move(self.window.cursor, window.logical_size())
                });
                let resizing = self
                    .ui
                    .shell
                    .as_ref()
                    .is_some_and(IdeShell::terminal_resizing);
                let sidebar_resizing = self
                    .ui
                    .shell
                    .as_ref()
                    .is_some_and(IdeShell::sidebar_resizing);
                let navigation_hover = self.ui.shell.as_ref().is_some_and(|shell| {
                    shell.navigation_hover(
                        self.window.cursor,
                        window.logical_size(),
                        self.window.control_pressed,
                    )
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
                self.window.primary_pressed = true;
                let now = Instant::now();
                let double = self.window.click_tracker.register(
                    now,
                    self.window.cursor,
                    DOUBLE_CLICK_INTERVAL,
                    DOUBLE_CLICK_SLACK,
                );
                if double {
                    if let Some(shell) = self.ui.shell.as_mut() {
                        shell.select_word_at_point(self.window.cursor, window.logical_size());
                    }
                    window.request_redraw();
                    return;
                }
                let tab_count = self.ui.shell.as_ref().map_or(0, IdeShell::tab_count);
                let revisao = self.ui.shell.as_ref().map_or(0, IdeShell::active_revision);
                if let Some(shell) = self.ui.shell.as_mut() {
                    shell.pointer_down_with_modifiers(
                        self.window.cursor,
                        window.logical_size(),
                        self.window.control_pressed,
                        self.window.shift_pressed,
                    );
                }
                // Abrir ou fechar aba muda o conjunto de documentos; um clique
                // que altera o texto — gerar acessores — muda o conteúdo. Os
                // dois pedem realce novo; mover o cursor, não.
                sync_languages = self.ui.shell.as_ref().is_some_and(|shell| {
                    shell.tab_count() != tab_count || shell.active_revision() != revisao
                });
                window.request_redraw();
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Right,
                ..
            } => {
                if let Some(shell) = self.ui.shell.as_mut() {
                    shell.secondary_pointer_down(self.window.cursor, window.logical_size());
                }
                window.request_redraw();
            }
            WindowEvent::MouseInput {
                state: ElementState::Released,
                button: MouseButton::Left,
                ..
            } => {
                self.window.primary_pressed = false;
                if let Some(shell) = self.ui.shell.as_mut() {
                    shell.pointer_up();
                }
                window.request_redraw();
            }
            // Perder o foco no meio de um arrasto — alt-tab, a janela sai da
            // frente — nunca traz a soltura. Encerrar o gesto aqui evita a
            // seleção que continua andando depois que o usuário já foi embora.
            WindowEvent::Focused(false) => {
                self.window.primary_pressed = false;
                if let Some(shell) = self.ui.shell.as_mut() {
                    shell.pointer_up();
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                // Sem arredondar: o delta em pixels de um touchpad ou de um mouse
                // de precisão vem em frações de linha, e arredondá-lo aqui
                // transformava um deslizar contínuo numa sucessão de saltos.
                let lines = match delta {
                    MouseScrollDelta::LineDelta(_, y) => -y * 3.0,
                    MouseScrollDelta::PixelDelta(position) => -(position.y as f32) / 22.0,
                };
                if let Some(shell) = self.ui.shell.as_mut() {
                    shell.scroll(self.window.cursor, lines, window.logical_size());
                }
                window.request_redraw();
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                let control_antes = self.window.control_pressed;
                self.window.control_pressed = modifiers.state().control_key();
                self.window.shift_pressed = modifiers.state().shift_key();
                self.window.alt_pressed = modifiers.state().alt_key();
                // Soltar o `Ctrl` é o que conclui a troca de abas. É o único
                // gesto da IDE em que a **soltura** de um modificador decide
                // algo, e por isso ele precisa de um aviso próprio.
                if control_antes
                    && !self.window.control_pressed
                    && let Some(shell) = self.ui.shell.as_mut()
                    && shell.release_control()
                {
                    window.request_redraw();
                }
                let navigation_hover = self.ui.shell.as_ref().is_some_and(|shell| {
                    shell.navigation_hover(
                        self.window.cursor,
                        window.logical_size(),
                        self.window.control_pressed,
                    )
                });
                let resizing = self
                    .ui
                    .shell
                    .as_ref()
                    .is_some_and(IdeShell::terminal_resizing);
                let sidebar_resizing = self
                    .ui
                    .shell
                    .as_ref()
                    .is_some_and(IdeShell::sidebar_resizing);
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
                    .ui
                    .shell
                    .as_ref()
                    .is_some_and(IdeShell::settings_dialog_open)
                {
                    match &event.logical_key {
                        Key::Named(NamedKey::Escape) => {
                            if let Some(shell) = self.ui.shell.as_mut() {
                                shell.escape();
                            }
                        }
                        Key::Named(NamedKey::Enter) => {
                            if let Some(shell) = self.ui.shell.as_mut() {
                                shell.key_down("Enter");
                            }
                        }
                        Key::Named(NamedKey::ArrowDown) => {
                            // Com os modificadores: `Shift` é o que
                            // distingue mover o cursor de estender a
                            // seleção, e sem ele a tecla chegava vazia.
                            if let Some(shell) = self.ui.shell.as_mut() {
                                shell.key_down_with_modifiers(
                                    "ArrowDown",
                                    Modifiers {
                                        shift: self.window.shift_pressed,
                                        control: self.window.control_pressed,
                                        ..Modifiers::default()
                                    },
                                );
                            }
                        }
                        Key::Named(NamedKey::ArrowUp) => {
                            // Com os modificadores: `Shift` é o que
                            // distingue mover o cursor de estender a
                            // seleção, e sem ele a tecla chegava vazia.
                            if let Some(shell) = self.ui.shell.as_mut() {
                                shell.key_down_with_modifiers(
                                    "ArrowUp",
                                    Modifiers {
                                        shift: self.window.shift_pressed,
                                        control: self.window.control_pressed,
                                        ..Modifiers::default()
                                    },
                                );
                            }
                        }
                        _ => {}
                    }
                } else if self.window.control_pressed
                    && self.window.shift_pressed
                    && matches!(&event.logical_key, Key::Character(value) if value.eq_ignore_ascii_case("j"))
                {
                    direct_commands.push(ApplicationCommand::OpenToolchainSettings);
                } else if self.window.control_pressed
                    && self.window.shift_pressed
                    && matches!(&event.logical_key, Key::Character(value) if value.eq_ignore_ascii_case("t"))
                {
                    direct_commands.push(ApplicationCommand::ExecuteTask(TaskId(
                        java_contribution::TEST_TASK_ID.to_owned(),
                    )));
                } else if self.window.control_pressed
                    && self.window.shift_pressed
                    && matches!(&event.logical_key, Key::Character(value) if value.eq_ignore_ascii_case("b"))
                {
                    direct_commands.push(ApplicationCommand::BuildProject);
                } else if self.window.control_pressed
                    && matches!(&event.logical_key, Key::Character(value) if value.eq_ignore_ascii_case("b"))
                {
                    direct_commands.push(ApplicationCommand::ExecuteTask(TaskId(
                        java_contribution::COMPILE_TASK_ID.to_owned(),
                    )));
                } else if self.window.control_pressed
                    && self.window.shift_pressed
                    && matches!(&event.logical_key, Key::Character(value) if value.eq_ignore_ascii_case("l"))
                {
                    if let Some(shell) = self.ui.shell.as_mut() {
                        shell.open_content_search();
                    }
                } else if self.window.control_pressed
                    && matches!(&event.logical_key, Key::Character(value) if value.eq_ignore_ascii_case("l"))
                {
                    if let Some(shell) = self.ui.shell.as_mut() {
                        shell.open_type_search();
                    }
                } else if self.window.control_pressed
                    && matches!(&event.logical_key, Key::Character(value) if value.eq_ignore_ascii_case("z"))
                {
                    // Desfazer e marcar ocorrências são do editor: o shell só
                    // encaminha a tecla com o modificador.
                    if let Some(shell) = self.ui.shell.as_mut() {
                        shell.key_down_with_modifiers(
                            "z",
                            Modifiers {
                                control: true,
                                ..Modifiers::default()
                            },
                        );
                    }
                } else if self.window.control_pressed
                    && matches!(&event.logical_key, Key::Character(value) if value.eq_ignore_ascii_case("f"))
                {
                    // A busca no arquivo aberto. Era `F3`, que ninguém procura
                    // primeiro; `Ctrl+F` é o que a mão faz sozinha.
                    if let Some(shell) = self.ui.shell.as_mut() {
                        shell.toggle_search();
                    }
                } else if self.window.control_pressed
                    && matches!(&event.logical_key, Key::Character(value) if value.eq_ignore_ascii_case("d"))
                {
                    if let Some(shell) = self.ui.shell.as_mut() {
                        shell.key_down_with_modifiers(
                            "d",
                            Modifiers {
                                control: true,
                                ..Modifiers::default()
                            },
                        );
                    }
                } else if self.window.control_pressed
                    && matches!(&event.logical_key, Key::Character(value) if value.eq_ignore_ascii_case("c"))
                {
                    if let Some(shell) = self.ui.shell.as_mut() {
                        shell.copy_selection();
                    }
                } else if self.window.control_pressed
                    && matches!(&event.logical_key, Key::Character(value) if value.eq_ignore_ascii_case("v"))
                {
                    if let Some(shell) = self.ui.shell.as_mut() {
                        shell.paste_clipboard();
                    }
                } else if self.window.control_pressed
                    && matches!(&event.logical_key, Key::Character(value) if value.eq_ignore_ascii_case("s"))
                {
                    // Salvar é do shell, que é dono da sessão do editor.
                    if let Some(shell) = self.ui.shell.as_mut() {
                        shell.request_save_active_document();
                    }
                } else if self.window.control_pressed && event.text.as_deref() == Some(" ") {
                    completion_requested = true;
                } else {
                    match &event.logical_key {
                        Key::Named(NamedKey::Escape) => {
                            if let Some(shell) = self.ui.shell.as_mut() {
                                shell.escape();
                            }
                        }
                        Key::Named(NamedKey::F5) => {
                            direct_commands.push(ApplicationCommand::ExecuteTask(TaskId(
                                java_contribution::RUN_TASK_ID.to_owned(),
                            )));
                        }
                        Key::Named(NamedKey::F8) => {
                            if let Some(shell) = self.ui.shell.as_mut() {
                                shell.request_debug(DebugRequest::Continue);
                            }
                        }
                        Key::Named(NamedKey::F9) => {
                            if let Some(shell) = self.ui.shell.as_mut() {
                                shell.toggle_breakpoint_at_cursor();
                            }
                        }
                        Key::Named(NamedKey::F10) => {
                            if let Some(shell) = self.ui.shell.as_mut() {
                                shell.request_debug(DebugRequest::StepOver);
                            }
                        }
                        Key::Named(NamedKey::F11) => {
                            let request = if self.window.shift_pressed {
                                DebugRequest::StepOut
                            } else {
                                DebugRequest::StepInto
                            };
                            if let Some(shell) = self.ui.shell.as_mut() {
                                shell.request_debug(request);
                            }
                        }
                        // `Delete` não produz texto, e o ramo geral só
                        // encaminha o que tem texto — sem este caso ela era
                        // descartada antes de chegar à janela.
                        Key::Named(NamedKey::Delete) => {
                            if let Some(shell) = self.ui.shell.as_mut() {
                                shell.key_down("Delete");
                            }
                        }
                        Key::Named(NamedKey::Backspace) => {
                            if let Some(shell) = self.ui.shell.as_mut() {
                                shell.key_down("Backspace");
                                // Apagar encurta o nome, e a lista volta a ter
                                // o que o prefixo menor alcança.
                                completion_requested |= shell.completion_open();
                            }
                        }
                        Key::Named(NamedKey::Enter) => {
                            if let Some(shell) = self.ui.shell.as_mut() {
                                shell.key_down("Enter");
                            }
                        }
                        // Tab precisa de um braço próprio: o texto que o sistema
                        // entrega para ela é `\t`, e o caminho genérico abaixo
                        // descarta caracteres de controle.
                        Key::Named(NamedKey::Tab) => {
                            let modifiers = Modifiers {
                                shift: self.window.shift_pressed,
                                control: self.window.control_pressed,
                                ..Modifiers::default()
                            };
                            if let Some(shell) = self.ui.shell.as_mut() {
                                shell.key_down_with_modifiers("Tab", modifiers);
                            }
                        }
                        Key::Named(NamedKey::ArrowLeft) => {
                            // Com os modificadores: `Shift` é o que
                            // distingue mover o cursor de estender a
                            // seleção, e sem ele a tecla chegava vazia.
                            if let Some(shell) = self.ui.shell.as_mut() {
                                shell.key_down_with_modifiers(
                                    "ArrowLeft",
                                    Modifiers {
                                        shift: self.window.shift_pressed,
                                        control: self.window.control_pressed,
                                        alt: self.window.alt_pressed,
                                        ..Modifiers::default()
                                    },
                                );
                            }
                        }
                        Key::Named(NamedKey::ArrowRight) => {
                            // Com os modificadores: `Shift` é o que
                            // distingue mover o cursor de estender a
                            // seleção, e sem ele a tecla chegava vazia.
                            if let Some(shell) = self.ui.shell.as_mut() {
                                shell.key_down_with_modifiers(
                                    "ArrowRight",
                                    Modifiers {
                                        shift: self.window.shift_pressed,
                                        control: self.window.control_pressed,
                                        alt: self.window.alt_pressed,
                                        ..Modifiers::default()
                                    },
                                );
                            }
                        }
                        Key::Named(NamedKey::ArrowDown) => {
                            // Com os modificadores: `Shift` é o que
                            // distingue mover o cursor de estender a
                            // seleção, e sem ele a tecla chegava vazia.
                            if let Some(shell) = self.ui.shell.as_mut() {
                                shell.key_down_with_modifiers(
                                    "ArrowDown",
                                    Modifiers {
                                        shift: self.window.shift_pressed,
                                        control: self.window.control_pressed,
                                        ..Modifiers::default()
                                    },
                                );
                            }
                        }
                        Key::Named(NamedKey::ArrowUp) => {
                            // Com os modificadores: `Shift` é o que
                            // distingue mover o cursor de estender a
                            // seleção, e sem ele a tecla chegava vazia.
                            if let Some(shell) = self.ui.shell.as_mut() {
                                shell.key_down_with_modifiers(
                                    "ArrowUp",
                                    Modifiers {
                                        shift: self.window.shift_pressed,
                                        control: self.window.control_pressed,
                                        ..Modifiers::default()
                                    },
                                );
                            }
                        }
                        // Nem todo teclado entrega Tab como tecla nomeada; em
                        // alguns layouts ela chega só como o texto `\t`, que o
                        // filtro de caracteres de controle abaixo descartaria.
                        _ if event.text.as_deref() == Some("\t") => {
                            let modifiers = Modifiers {
                                shift: self.window.shift_pressed,
                                control: self.window.control_pressed,
                                ..Modifiers::default()
                            };
                            if let Some(shell) = self.ui.shell.as_mut() {
                                shell.key_down_with_modifiers("Tab", modifiers);
                            }
                        }
                        _ => {
                            if let Some(text) = event.text
                                && !text.chars().any(char::is_control)
                                && let Some(shell) = self.ui.shell.as_mut()
                            {
                                // Alguns caracteres pedem completação sozinhos —
                                // em Java, o ponto. Quais são é a linguagem
                                // quem diz; o editor só pergunta.
                                if let (Some(document_id), Some(host)) =
                                    (shell.active_document(), self.languages.host.as_ref())
                                {
                                    let triggers = host.trigger_characters(document_id);
                                    completion_requested |=
                                        text.chars().any(|typed| triggers.contains(&typed));
                                }
                                // Com a lista aberta, cada letra digitada refaz o
                                // filtro, e o que não é nome fecha a lista.
                                completion_requested |= shell.completion_follow_up(&text);
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
                    self.runtime.startup_error = Some(error);
                    event_loop.exit();
                }
                // Com a IDE já desenhada, a espera da indexação acontece com o
                // usuário vendo a tela montada em vez de um retângulo vazio.
                sync_languages |= self.runtime.languages_pending;
                self.runtime.languages_pending = false;
            }
            _ => {}
        }
        let antes_da_linguagem = iniciado.elapsed();
        if sync_languages {
            self.sync_languages();
            // O realce que acabou de chegar precisa de um quadro para aparecer.
            if let Some(window) = self.window.window.as_ref() {
                window.request_redraw();
            }
        }
        let linguagem = iniciado.elapsed() - antes_da_linguagem;
        if completion_requested {
            self.request_completion();
        }
        let completacao = iniciado.elapsed() - antes_da_linguagem - linguagem;
        self.dispatch_application_commands(direct_commands);
        // O menu `Generate` só marca o pedido; quem tem a linguagem responde.
        self.answer_accessor_request();
        self.answer_constructor_request();
        self.answer_rename_request();
        self.drain_application_events();
        if let Some(etiqueta) = etiqueta
            && iniciado.elapsed() >= PERF_THRESHOLD
        {
            eprintln!(
                "[perf] {etiqueta}: total {:?} | evento {antes_da_linguagem:?} | linguagem {linguagem:?} | completação {completacao:?}",
                iniciado.elapsed()
            );
        }
    }
}

/// Medição de tempo ligada por `ERIDE_PERF`, para investigar travamento.
///
/// Fora dela o custo é uma leitura de `OnceLock`: medir não pode ser o motivo de
/// a janela ficar lenta.
fn perf_enabled() -> bool {
    static LIGADO: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *LIGADO.get_or_init(|| std::env::var_os("ERIDE_PERF").is_some())
}

/// Abaixo disto o evento não interessa: o alvo é o que trava, não o ruído.
const PERF_THRESHOLD: std::time::Duration = std::time::Duration::from_millis(4);

/// Nome curto do evento, para a linha de medição.
fn event_label(event: &WindowEvent) -> &'static str {
    match event {
        WindowEvent::RedrawRequested => "redraw",
        WindowEvent::KeyboardInput { .. } => "tecla",
        WindowEvent::CursorMoved { .. } => "ponteiro",
        WindowEvent::MouseInput { .. } => "clique",
        WindowEvent::MouseWheel { .. } => "roda",
        WindowEvent::Resized(_) => "redimensionar",
        WindowEvent::ModifiersChanged(_) => "modificadores",
        _ => "outro",
    }
}

/// A queixa de um analisador que não subiu, **da linguagem que está em uso**.
///
/// É a mesma informação que o aviso de queda dá, dita na hora em que a pergunta
/// falha em vez de cinco segundos depois na barra. A degradação da ADR-025 é para
/// a IDE continuar servindo, e não para ela ficar silenciosamente pior.
///
/// # Por que o filtro por extensão existe
///
/// Sem ele, esta função acusava o **primeiro** provider falho que encontrasse. Num
/// projeto Java o provider de TypeScript está falho quase sempre — não há
/// `node_modules` num projeto Java, e não deveria haver —, e uma busca Java que
/// legitimamente não achasse nada responderia "TypeScript indisponível". A
/// mensagem certa para o projeto errado é pior do que nenhuma mensagem.
///
/// Lista de extensões vazia devolve `None`: sem saber sobre o que se está
/// perguntando, calar é a resposta conservadora.
fn analisador_ausente(
    providers: Vec<ide_language_host::ProviderSnapshot>,
    relevantes: &[String],
) -> Option<String> {
    if relevantes.is_empty() {
        return None;
    }
    providers.into_iter().find_map(|snapshot| {
        if snapshot.state != ide_language_api::ProviderState::Failed {
            return None;
        }
        let cuida = snapshot.metadata.extensions.iter().any(|extensao| {
            relevantes
                .iter()
                .any(|aberta| aberta.eq_ignore_ascii_case(extensao))
        });
        if !cuida {
            return None;
        }
        let nome = snapshot.metadata.display_name;
        let motivo = snapshot
            .last_error
            .unwrap_or_else(|| "sem detalhe".to_owned());
        Some(format!(
            "{nome} indisponível, e a análise nativa não tem índice: {motivo}"
        ))
    })
}

/// Como o tipo é chamado na tela — o que o usuário escreveria.
///
/// Devolve `None` para o que não é tipo: a busca fala de classes, e um método
/// com o mesmo nome só confundiria a lista.
const fn type_kind_label(kind: SymbolKind) -> Option<&'static str> {
    Some(match kind {
        SymbolKind::Class => "classe",
        SymbolKind::Interface => "interface",
        SymbolKind::Record => "record",
        SymbolKind::Enum => "enum",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use ide_project::model::{
        BuildSystemId, ModuleId, ProjectDescriptor, ProjectModule, SourceRoots,
    };
    use ide_workspace::WorkspaceService;

    use super::*;

    fn test_shell(root: &Path) -> IdeShell {
        let service = WorkspaceService::native();
        match service.scan(root) {
            Ok(tree) => IdeShell::from_tree(tree),
            Err(error) => panic!("projeto não abriu: {error}"),
        }
    }

    fn open_test_document(shell: &mut IdeShell, path: &Path) -> DocumentId {
        let service = WorkspaceService::native();
        match service.read_document(path) {
            Ok(text) => shell.show_document(path, text),
            Err(error) => panic!("documento não abriu: {error}"),
        }
    }

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
        assert!(
            std::fs::write(
                pacote.join("Pedido.java"),
                "public record Pedido(String v) {}
"
            )
            .is_ok()
        );
        assert!(
            std::fs::write(
                pacote.join("Servico.java"),
                "public interface Servico {}
"
            )
            .is_ok()
        );
        assert!(
            std::fs::write(
                pacote.join("Estado.java"),
                "public enum Estado { ATIVO }
"
            )
            .is_ok()
        );
        assert!(
            std::fs::write(
                pacote.join("Ajuda.java"),
                "public class Ajuda {}
"
            )
            .is_ok()
        );
        let uso = pacote.join("Uso.java");
        let texto = "public class Uso { void f() { Pedido p; Servico s; Estado e; Ajuda a; } }
";
        assert!(std::fs::write(&uso, texto).is_ok());

        let language_host = LanguageHost::new(&root);
        let java = java_contribution::contribution(Arc::new(NativeProcessSupervisor::default()));
        assert!(language_host.register(java.provider.clone()).is_ok());
        let mut ide = NativeIde::default();
        // A contribuição, e não só o provider: é dela que sai a lista de
        // extensões que a sincronização consulta. Ver a fase 1b da `23`.
        assert!(ide.languages.contributions.register(java).is_ok());
        ide.languages.host = Some(Arc::new(language_host));
        ide.ui.shell = Some(test_shell(&root));
        let document_id = match ide.ui.shell.as_mut() {
            Some(shell) => open_test_document(shell, &uso),
            None => panic!("shell de teste ausente"),
        };
        ide.sync_languages();
        // Ativar não espera mais o índice: quem afirma a navegação pelo projeto
        // inteiro precisa dele pronto. Ver a fase 2 da `19`.
        if let Some(host) = &ide.languages.host {
            assert!(
                pollster::block_on(host.wait_until_indexed(std::time::Duration::from_secs(60)))
                    .unwrap_or(false),
                "o índice do projeto não ficou pronto a tempo"
            );
        }

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
                .ui
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

    /// Digitar não espera pela análise da linguagem.
    ///
    /// Era o que travava a digitação: a tecla ficava presa esperando o provider
    /// analisar o arquivo — mais de 400 ms num arquivo grande, e ainda 60 ms
    /// depois de a análise emagrecer. O provider sempre teve thread própria; o
    /// que faltava era não esperar por ela.
    #[test]
    fn typing_does_not_wait_for_the_language_analysis() {
        let root = std::env::temp_dir().join(format!("er-ide-async-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let pacote = root.join("src");
        assert!(std::fs::create_dir_all(&pacote).is_ok());
        // Grande o bastante para a análise não caber num quadro.
        let corpo: String = (0..3000)
            .map(|indice| {
                format!(
                    "    int metodo{indice}() {{ return {indice}; }}
"
                )
            })
            .collect();
        let alvo = pacote.join("Grande.java");
        assert!(
            std::fs::write(
                &alvo,
                format!(
                    "public class Grande {{
{corpo}}}
"
                )
            )
            .is_ok()
        );

        let language_host = LanguageHost::new(&root);
        let java = java_contribution::contribution(Arc::new(NativeProcessSupervisor::default()));
        assert!(language_host.register(java.provider.clone()).is_ok());
        let mut ide = NativeIde::default();
        // A contribuição, e não só o provider: é dela que sai a lista de
        // extensões que a sincronização consulta. Ver a fase 1b da `23`.
        assert!(ide.languages.contributions.register(java).is_ok());
        ide.languages.host = Some(Arc::new(language_host));
        ide.ui.shell = Some(test_shell(&root));
        if let Some(shell) = ide.ui.shell.as_mut() {
            open_test_document(shell, &alvo);
        }
        ide.sync_languages();
        ide.settle_syntax();

        // Uma tecla, e o tempo que ela custa no laço da janela.
        if let Some(shell) = ide.ui.shell.as_mut() {
            shell.text_input("a");
        }
        let inicio = std::time::Instant::now();
        ide.sync_languages();
        let gasto = inicio.elapsed();

        assert!(
            ide.languages.pending_syntax() > 0,
            "a tecla deixou a análise pendente em vez de esperar por ela"
        );
        // Folgado de propósito: o que se afirma é que a tecla não paga a análise,
        // e não quanto a máquina que roda o teste é rápida.
        assert!(
            gasto < std::time::Duration::from_millis(30),
            "a tecla custou {gasto:?}, como se ainda esperasse a análise"
        );

        // E o realce chega, ainda que depois.
        ide.settle_syntax();
        assert_eq!(ide.languages.pending_syntax(), 0);
        let realcado = ide
            .ui
            .shell
            .as_ref()
            .and_then(|shell| {
                shell
                    .syntax_snapshot(DocumentId(1))
                    .map(|s| s.highlights.len())
            })
            .unwrap_or_default();
        assert!(realcado > 0, "o realce chegou pela thread do provider");

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
        let java = java_contribution::contribution(Arc::new(NativeProcessSupervisor::default()));
        assert!(language_host.register(java.provider.clone()).is_ok());

        let mut ide = NativeIde::default();
        // A contribuição precisa estar registrada, e não só o provider: é dela
        // que sai a lista de extensões que a sincronização de documentos
        // consulta. Ver a fase 1b da `23`.
        assert!(ide.languages.contributions.register(java).is_ok());
        ide.languages.host = Some(Arc::new(language_host));
        ide.ui.shell = Some(test_shell(&root));
        if let Some(shell) = ide.ui.shell.as_mut() {
            open_test_document(shell, &file);
        }

        let keyword_colored = |ide: &mut NativeIde| {
            let colors = ui_core::Theme::default().colors;
            ide.ui
                .shell
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

        // É isto que a inicialização passou a fazer. O realce vem da thread do
        // provider, então o teste espera por ele; na janela, quem o recolhe é o
        // relógio, uns 30 ms depois — sem a digitação ter esperado.
        ide.sync_languages();
        ide.settle_syntax();
        assert!(
            keyword_colored(&mut ide),
            "depois de pedir o realce, o código aparece colorido"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    /// Um `.ts` aberto na IDE aparece colorido, como um `.java`.
    ///
    /// É o critério da fase 1 da `23` cobrado onde ele vale: no caminho da
    /// **aplicação**. O teste que deu a fase por cumprida montava um
    /// `LanguageHost` e falava com ele — e o host roteava certo o tempo todo. O
    /// que descartava o `.ts` era a sincronização de documentos, um nível acima,
    /// que perguntava se a extensão era `java` com a palavra escrita à mão.
    ///
    /// Testar a camada que se acabou de mexer e concluir sobre a de cima é o
    /// defeito que este teste existe para não deixar voltar. Ver a fase 1b.
    #[test]
    fn a_typescript_file_is_highlighted_through_the_application() {
        let root = std::env::temp_dir().join(format!("er-ide-ts-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        assert!(std::fs::create_dir_all(&root).is_ok());
        let file = root.join("pedido.ts");
        assert!(std::fs::write(&file, "export class Pedido {}").is_ok());

        let language_host = LanguageHost::new(&root);
        let typescript = typescript_contribution::contribution(Arc::new(
            NativeProcessSupervisor::default(),
        ));
        assert!(language_host.register(typescript.provider.clone()).is_ok());

        let mut ide = NativeIde::default();
        assert!(ide.languages.contributions.register(typescript).is_ok());
        ide.languages.host = Some(Arc::new(language_host));
        ide.ui.shell = Some(test_shell(&root));
        if let Some(shell) = ide.ui.shell.as_mut() {
            open_test_document(shell, &file);
        }

        let keyword_colored = |ide: &mut NativeIde| {
            let colors = ui_core::Theme::default().colors;
            ide.ui
                .shell
                .as_mut()
                .map(|shell| shell.paint(Size::new(1_280.0, 800.0)))
                .unwrap_or_default()
                .iter()
                .any(|command| {
                    matches!(
                        command,
                        ui_render_api::PaintCommand::DrawText(text)
                            if text.text == "export" && text.color == colors.syntax_keyword
                    )
                })
        };

        ide.sync_languages();
        ide.settle_syntax();
        assert!(
            keyword_colored(&mut ide),
            "um `.ts` precisa chegar ao provider e voltar colorido"
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
        let mut shell = test_shell(&project);
        open_test_document(&mut shell, &first);
        open_test_document(&mut shell, &second);

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
        let mut restored = test_shell(&project);
        assert_eq!(restored.tab_count(), 0, "a IDE abre sem abas");
        for document in reopened.workspace.resolved_documents(&project) {
            open_test_document(&mut restored, &document);
        }
        assert_eq!(restored.open_document_paths(), vec![first, second.clone()]);
        assert_eq!(restored.active_document_path(), Some(second));

        let _ = std::fs::remove_dir_all(&root);
    }

    /// O JDK e o Maven gravados no formato antigo sobrevivem à mudança.
    ///
    /// É o risco principal da fase 0 da `23`: mexer em configuração persistida.
    /// Uma migração malfeita apaga a escolha em silêncio, que é o pior jeito de
    /// falhar — quem abre a IDE não distingue "nunca escolhi" de "perdi".
    ///
    /// A tradução mora na raiz de composição porque saber que `jdk_home` era a
    /// ferramenta principal de Java é conhecimento de linguagem, e o núcleo não
    /// pode tê-lo.
    #[test]
    fn tools_chosen_in_the_old_format_survive_the_migration() {
        let root = std::env::temp_dir().join(format!("er-migracao-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let jdk = root.join("jdk-21");
        let maven = root.join("maven-3.9");
        assert!(std::fs::create_dir_all(&jdk).is_ok());
        assert!(std::fs::create_dir_all(&maven).is_ok());
        let config_file = root.join("config.toml");
        assert!(
            std::fs::write(
                &config_file,
                format!(
                    "[toolchains]\njdk_home = {:?}\nmaven_home = {:?}\n",
                    jdk.to_string_lossy(),
                    maven.to_string_lossy()
                ),
            )
            .is_ok()
        );

        let Ok(mut config) = AppConfig::load(&config_file) else {
            panic!("configuração antiga precisa ser lida");
        };
        assert!(
            crate::bootstrap::migrate_legacy_toolchains(&mut config),
            "havia escolhas antigas a migrar"
        );

        let language = java_contribution::JAVA_LANGUAGE_ID;
        assert_eq!(
            config
                .toolchains
                .resolved(None, language, ToolRole::Primary)
                .map(|tool| tool.home),
            Some(jdk),
            "o JDK escolhido não pode se perder na mudança de formato"
        );
        assert_eq!(
            config
                .toolchains
                .resolved(None, language, ToolRole::Secondary)
                .map(|tool| tool.home),
            Some(maven)
        );

        // Migrar é uma vez só: gravado no formato novo, não há mais o que traduzir.
        assert!(config.save(&config_file).is_ok());
        let Ok(mut relido) = AppConfig::load(&config_file) else {
            panic!("configuração migrada precisa ser relida");
        };
        assert!(!crate::bootstrap::migrate_legacy_toolchains(&mut relido));
        assert!(
            relido
                .toolchains
                .resolved(None, language, ToolRole::Primary)
                .is_some(),
            "a escolha migrada continua valendo depois de regravada"
        );

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

    fn snapshot_falho(
        provider: &str,
        nome: &str,
        extensoes: &[&str],
        estado: ide_language_api::ProviderState,
    ) -> ide_language_host::ProviderSnapshot {
        ide_language_host::ProviderSnapshot {
            metadata: ide_language_api::LanguageMetadata {
                language_id: LanguageId(provider.to_owned()),
                provider_id: ProviderId(provider.to_owned()),
                display_name: nome.to_owned(),
                extensions: extensoes.iter().map(|e| (*e).to_owned()).collect(),
                api_version: ide_language_api::LANGUAGE_API_VERSION,
                trigger_characters: Vec::new(),
            },
            capabilities: ide_language_api::LanguageCapabilities::empty(),
            state: estado,
            last_error: Some("faltou `npm install`".to_owned()),
        }
    }

    /// A queixa é da linguagem em uso, e não da primeira que estiver falha.
    ///
    /// **Num projeto Java o provider de TypeScript está falho quase sempre**, e
    /// deve estar: não há `node_modules` num projeto Java. Sem este filtro, uma
    /// busca Java que não achasse nada responderia "TypeScript indisponível" — a
    /// mensagem certa para o projeto errado, que é pior do que nenhuma.
    #[test]
    fn the_complaint_names_only_the_language_in_use() {
        let providers = vec![
            snapshot_falho(
                "typescript",
                "TypeScript",
                &["ts", "tsx"],
                ide_language_api::ProviderState::Failed,
            ),
            snapshot_falho(
                "java",
                "Java",
                &["java"],
                ide_language_api::ProviderState::Active,
            ),
        ];

        // Editando Java: o TypeScript falho não interessa a esta pergunta.
        assert_eq!(
            analisador_ausente(providers.clone(), &["java".to_owned()]),
            None,
            "uma busca Java não pode ser explicada por um analisador de TypeScript"
        );

        // Editando TypeScript: aí sim.
        let queixa = analisador_ausente(providers.clone(), &["ts".to_owned()]);
        assert!(
            queixa.is_some_and(|texto| texto.contains("TypeScript") && texto.contains("npm")),
            "a queixa precisa nomear a linguagem e dizer o que fazer"
        );

        // Sem arquivo aberto, calar é a resposta conservadora.
        assert_eq!(analisador_ausente(providers, &[]), None);
    }

    /// Uma linguagem sem analisador externo nenhum nunca produz queixa.
    ///
    /// É o caso de qualquer linguagem que só tenha provider nativo — hoje o
    /// realce de CSS, amanhã a próxima que entrar. Para elas, "nada encontrado"
    /// é a resposta inteira, e inventar uma causa seria mentir.
    #[test]
    fn a_language_without_an_external_analyzer_never_complains() {
        let providers = vec![snapshot_falho(
            "css",
            "CSS",
            &["css", "scss"],
            ide_language_api::ProviderState::Active,
        )];
        assert_eq!(analisador_ausente(providers, &["css".to_owned()]), None);
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
