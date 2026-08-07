use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, mpsc},
    time::{Duration, Instant},
};

#[cfg(test)]
use crate::bootstrap::default_goals;
use crate::bootstrap::{java_source, project_sources, requested_root, startup_root};
use crate::bridges::position_at_offset;
use crate::splash;
use crate::controllers::{
    CompletionOutcome, DebugController as AppDebugController, DocumentController, GitDiffOutcome,
    ImportedProject, LanguageController, NavigationOutcome, NativeWindowState, ProjectController,
    RuntimeState, TaskController as AppTaskController, TypeSearchOutcome, WorkspaceController,
};
use ide_application::{GitRequest, RestoreTarget};
use crate::ui_bridge::{UiAction, UiBridge};
use crate::watching::{ConsumidorDeFontes, ConsumidorDeGit, MudancaNoDisco};
use crate::{
    debug, java_contribution, markup_contribution, run, style_contribution,
    typescript_contribution,
};

/// Quanto tempo de giro ainda é carga, e a partir de quando é suspeita.
///
/// Um monorepo grande leva de trinta a setenta segundos para o analisador
/// montar o projeto — medido. Meio minuto além disso não é lentidão: é sinal de
/// prontidão que não chegou, e o log passa a dizer de quem.
const PACIENCIA_COM_O_GIRO: std::time::Duration = std::time::Duration::from_secs(90);
use ide_application::{
    ApplicationCommand, DebugRequest, IdeEvent, NavigationRequest, NewItemRequest,
    OpenDocumentRequest, RenameDocumentRequest, SaveDocumentRequest, SearchScope,
    TaskExecutionContext, TaskId,
};
use ide_core::{AppConfig, ToolRole, config_path};
use ide_debug_api::{DebugEvent, StepKind};
use ide_domain::{
    CancellationToken, DefinitionRequest, DocumentId, DocumentSnapshot, LanguageId, ProviderId, SymbolKind,
    TextPosition, TextRange,
};
use ide_language_host::{LanguageHost, LanguageToolchainConfig};
use ide_process::{NativeProcessSupervisor, ProcessSupervisor};
use ide_project::{build::ProjectImportRequest, model::ProjectModel};
use ide_toolchain_api::DetectionContext;
use ide_ui::{
    ContentSearchHit, DebugView, IdeShell, SettingsPage, TYPE_SEARCH_LIMIT, TypeSearchHit,
    explorer_id,
};
use ide_workspace::FileNode;
#[cfg(test)]
use language_java::GRADLE_BUILD_SYSTEM_ID;
#[cfg(test)]
use language_java::MAVEN_BUILD_SYSTEM_ID;
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

/// Até quando insistir com o índice pelas espécies do Explorer.
///
/// Ele indexa depois de subir, e antes disso responde vazio — vazio que não se
/// distingue de "este projeto não tem tipos". Noventa segundos é o mesmo fôlego
/// que a IDE já dá ao giro de carregamento; passado isso, o Explorer fica sem
/// crachá, que é melhor do que uma thread esperando para sempre.
const ESPERA_DO_INDICE: std::time::Duration = std::time::Duration::from_secs(90);

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
            ..WindowRequest::default()
        };
        let window = WinitWindow::create_hidden(event_loop, WindowId(1), &request)
            .map_err(|error| error.to_string())?;
        let renderer = pollster::block_on(WgpuRenderer::new(window.inner().clone()))
            .map_err(|error| error.to_string())?;
        // A tela de abertura vem **antes** de tudo o que demora. Daqui até o
        // `window.show()` lá embaixo há registro de linguagens, varredura de
        // disco, detecção de ferramenta e importação de projeto — e a janela da
        // IDE fica oculta o tempo inteiro, sem sinal nenhum de que ela está
        // subindo. A tela de abertura não acelera nada; ela responde.
        //
        // Ela vive nesta variável e morre logo antes de a IDE aparecer: fechar
        // é soltar a janela, e é por isso que ela é um valor e não um campo.
        let splash = splash::abrir(event_loop);
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
        let root = startup_root(
            requested_root(),
            &self.runtime.config,
            std::env::current_dir().ok(),
        )
            .ok_or_else(|| "não foi possível determinar o diretório do projeto".to_owned())?;
        let nativo = Arc::new(NativeProcessSupervisor::default());
        // O supervisor concreto fica guardado para a medição: ele é quem sabe
        // quais processos externos existem, porque foi ele que os criou.
        self.runtime.processes = Some(Arc::clone(&nativo));
        let processes: Arc<dyn ProcessSupervisor> = nativo;
        let java = java_contribution::contribution(processes.clone());
        // A postura vem da configuração, e o padrão é subir junto.
        let language_host = LanguageHost::with_config(
            &root,
            ide_language_host::LanguageHostConfig {
                eager_providers: self.runtime.config.eager_language_providers,
                ..ide_language_host::LanguageHostConfig::default()
            },
        );
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
        // Construído uma vez e usado nos dois lugares: o descritor precisa
        // reclamar as extensões que o provider vai responder, senão elas não
        // chegam ao host.
        let plugins: Vec<Arc<dyn language_typescript::AnalyzerPluginSource>> =
            vec![Arc::new(crate::angular_contribution::analyzer_plugin())];
        let typescript = typescript_contribution::contribution(processes.clone(), &plugins);
        language_host
            .register(typescript.provider.clone())
            .map_err(|error| error.to_string())?;
        // O analisador externo entra ao lado do nativo, e não no lugar dele: são
        // dois providers para a mesma extensão, e a ordem entre eles é a
        // declarada logo abaixo.
        language_host
            .register(typescript_contribution::service_provider(
                processes.clone(),
                plugins,
            ))
            .map_err(|error| error.to_string())?;
        // Quem prepara coisa que não é da IDE não segura o giro de carregamento.
        self.languages
            .alheios
            .extend(typescript_contribution::analisadores_externos());
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
        // A marcação entra ao lado do analisador, e não no lugar dele: num
        // projeto Angular o mesmo `.html` é atendido pelos dois, cada um com as
        // capacidades que tem.
        let marcacao = markup_contribution::contribution();
        language_host
            .register(marcacao.provider.clone())
            .map_err(|error| error.to_string())?;
        self.languages
            .contributions
            .register(marcacao)
            .map_err(|error| error.to_string())?;
        let estilo = style_contribution::contribution();
        language_host
            .register(estilo.provider.clone())
            .map_err(|error| error.to_string())?;
        self.languages
            .contributions
            .register(estilo)
            .map_err(|error| error.to_string())?;
        // Desligar o que a configuração pede vem **depois** de registrar tudo:
        // um provider precisa existir para poder ser tirado de serviço, e é por
        // continuar listado que a IDE consegue dizer que ele está desligado em
        // vez de a pergunta apenas não achar nada.
        for provider_id in &self.runtime.config.disabled_providers {
            let provider_id = ProviderId(provider_id.clone());
            match pollster::block_on(language_host.disable(&provider_id)) {
                Ok(()) => tracing::info!(
                    provider = provider_id.0,
                    "provider desligado pela configuração"
                ),
                // Identificador que não existe não é erro fatal: a configuração é
                // escrita à mão, e um nome errado não pode impedir a IDE de abrir.
                Err(error) => tracing::warn!(
                    provider = provider_id.0,
                    %error,
                    "a configuração pede desligar um provider que não está registrado"
                ),
            }
        }
        self.languages.host = Some(Arc::new(language_host));
        let tree = self
            .workspace
            .service
            .scan(&root)
            .map_err(|error| error.to_string())?;
        let mut shell = IdeShell::from_tree(tree);
        // Os componentes medem o texto pela mesma fonte que vai desenhá-lo. Quem
        // constrói o mecanismo é a aplicação; a interface só recebe a porta.
        self.ui.text_metrics = Some(Arc::new(ui_text_cosmic::CosmicTextEngine::new()));
        // Copiar e colar falam com o sistema, não com uma cópia interna. Sem área
        // de transferência no ambiente a IDE segue funcionando, e é a barra de
        // estado que conta isso quando alguém tenta copiar.
        match ui_clipboard_arboard::SystemClipboard::new() {
            Ok(clipboard) => self.ui.clipboard = Some(Arc::new(clipboard)),
            Err(error) => tracing::warn!(%error, "área de transferência indisponível"),
        }
        self.equip_shell(&mut shell);
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
        self.register_build_systems();
        // O projeto da abertura também é um recente. Antes só entravam os
        // abertos pelo menu, e quem sobe a IDE por linha de comando numa pasta
        // nunca a encontrava depois na lista — justo quem mais alterna.
        //
        // **Depois de registrar os sistemas de build**: é por eles que se sabe
        // em que linguagem a pasta é reconhecida, e antes daqui a resposta seria
        // sempre "nenhuma".
        self.remember_project(&root);
        let (tool_sender, tool_events) = mpsc::channel();
        self.tasks.sender = Some(tool_sender);
        self.tasks.events = Some(tool_events);
        self.detect_all_toolchains(&root);
        self.detect_maven();
        self.import_project(&root);
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
        // **A espera pela tela de abertura não é ociosa: ela é o carregamento.**
        //
        // Perguntar ao índice que espécie cada arquivo declara é o que ativa as
        // linguagens do projeto — e uma linguagem cujo analisador é um processo
        // externo sobe esse processo aqui, atrás da marca. Subir Node no
        // Windows, com antivírus no caminho, leva segundos; esses segundos ou
        // passam agora, escondidos, ou passam depois, com a IDE aberta e sem
        // responder.
        //
        // Vem **depois** do `import_project` de propósito: é ele que registra as
        // raízes de fontes, e registrá-las derruba os workers ativos. Aquecer
        // antes seria aquecer o que vai ser jogado fora.
        self.window.window = Some(window);
        // As linguagens do projeto sobem agora, atrás da marca — a mesma
        // preparação que a troca de projeto faz, pela mesma função.
        self.prepare_project_languages();
        match splash {
            // **O arranque acaba aqui, e devolve o controle ao laço.** Quem
            // conta o tempo da marca é o `about_to_wait`, girando a cada trinta
            // milissegundos: bloquear aqui deixaria a janela da tela de abertura
            // sem processar mensagens, e o sistema a trocaria pela janela
            // fantasma dele — com moldura, título e "não respondendo".
            Some(splash) => self.window.splash = Some(splash),
            // Sem tela de abertura, a IDE aparece agora, como antes dela existir.
            None => self.mostrar_a_ide(),
        }
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

    /// Mostra a janela da IDE, já desenhada.
    ///
    /// Mostrar primeiro e pintar depois dá ao sistema uma janela sem conteúdo,
    /// e o que ele desenha é a moldura em volta de um retângulo vazio. O
    /// primeiro quadro vai para a superfície com a janela ainda oculta; quando
    /// ela aparece, aparece pronta.
    fn mostrar_a_ide(&mut self) {
        if let Err(error) = self.render() {
            tracing::warn!(%error, "o primeiro quadro não foi desenhado antes de mostrar");
        }
        if let Some(window) = self.window.window.as_ref() {
            window.show();
        }
    }

    /// Fecha a tela de abertura quando o tempo dela acaba, e abre a IDE.
    ///
    /// Chamado a cada volta do laço de eventos. É esta volta que mantém a
    /// janela da tela de abertura respondendo — foi por não existir que o
    /// sistema a declarava travada e punha a janela fantasma dele no lugar.
    ///
    /// A IDE aparece **antes** de a marca sair: fechá-la primeiro abriria um vão
    /// em que nenhuma das duas está visível, e o que aparece nesse vão é a área
    /// de trabalho.
    fn advance_splash(&mut self) {
        if !self
            .window
            .splash
            .as_ref()
            .is_some_and(crate::splash::Splash::terminou)
        {
            return;
        }
        self.mostrar_a_ide();
        self.window.splash = None;
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
            // A margem do arquivo aberto mostra o que mudou desde o commit. É a
            // diferença **sem** a comparação lado a lado: quem abre um arquivo
            // quer ver o arquivo, e não uma tela dividida que ninguém pediu.
            self.pedir_diferenca(request.path.clone(), false, false);
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
        let gravou = result.is_ok();
        if let Some(shell) = self.ui.shell.as_mut() {
            match result {
                Ok(()) => {
                    shell.document_saved(request.document_id, request.revision, &request.path);
                }
                Err(error) => shell.set_status_message(error.to_string()),
            }
        }
        if gravou {
            // **A margem acompanha a gravação.** O observador do disco vê o
            // arquivo mudar 300 ms depois, e esperar por ele deixaria a marca do
            // que se acabou de escrever chegando tarde — para quem gravou, ela
            // simplesmente não apareceu.
            self.pedir_diferenca(request.path, false, false);
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
        self.open_another_project(&folder);
    }

    /// Abre um projeto escolhido em "Arquivo → Recentes".
    ///
    /// A pasta é conferida na hora do clique, e não só quando o menu foi
    /// montado: entre uma coisa e outra alguém pode ter renomeado, movido ou
    /// desconectado o disco. Abrir a mesma pasta de novo não faz nada — seria
    /// derrubar o analisador e reabrir tudo para chegar onde já se estava.
    fn open_recent_project(&mut self, path: &Path) {
        if !path.is_dir() {
            self.report_to_shell(&format!("{} não está mais disponível", path.display()));
            self.publish_recent_projects();
            return;
        }
        let atual = self
            .ui
            .shell
            .as_ref()
            .is_some_and(|shell| shell.workspace_path() == path);
        if atual {
            return;
        }
        self.open_another_project(path);
    }

    /// Manda para a tela os projetos recentes que ainda existem.
    ///
    /// Chamado depois de gravar a configuração, que é quando a lista muda, e
    /// também quando um recente se revela ausente: nos dois casos o menu montado
    /// deixou de descrever o que existe.
    fn publish_recent_projects(&mut self) {
        // O identificador vira nome de exibição aqui: no arquivo fica o que não
        // muda, e na tela o que se lê. Uma linguagem que deixou de ser
        // registrada perde o nome, e o projeto dela fica solto em vez de sumir.
        let recentes = self
            .runtime
            .config
            .workspace
            .resolved_recent_projects()
            .into_iter()
            .map(|recente| ide_ui::RecentProject {
                language: recente.language.and_then(|id| {
                    self.languages
                        .contributions
                        .get(&LanguageId(id))
                        .map(|contribuicao| contribuicao.descriptor.display_name.clone())
                }),
                path: recente.path,
            })
            .collect();
        if let Some(shell) = self.ui.shell.as_mut() {
            shell.set_recent_projects(recentes);
        }
    }

    /// Em que linguagem **esta pasta** é reconhecida.
    ///
    /// Sai do sistema de build detectado, e não de uma tabela de manifestos: a
    /// aplicação não sabe o nome de linguagem nenhuma, e quem diz de quem é o
    /// manifesto encontrado é a contribuição que registrou aquele sistema de
    /// build.
    ///
    /// # Detecta, e não pergunta ao projeto importado
    ///
    /// Ler `project.imported` respondia sobre **o projeto anterior**: ele só é
    /// trocado dentro da importação, que roda depois do primeiro quadro. Abrir
    /// um projeto Java vindo de um TypeScript o registrava como TypeScript, e a
    /// correção só chegava se a importação desse certo — um Maven sem JDK
    /// deixava a etiqueta errada para sempre.
    ///
    /// A detecção é só procurar manifesto em disco; a importação é que roda
    /// processo externo. Perguntar aqui custa pouco e responde sobre a pasta
    /// certa mesmo quando a importação depois falha.
    ///
    /// `None` para uma pasta que não é projeto de ninguém.
    fn detected_language(&self, root: &Path) -> Option<String> {
        let build_system = self.detected_build_system(root)?;
        self.languages
            .contributions
            .language_for_build_system(&build_system)
            .map(|descriptor| descriptor.language_id.0.clone())
    }

    /// O sistema de build da raiz, ou o da pasta que ela apenas embrulha.
    ///
    /// **Descer é necessário**: quem clona um repositório dentro de uma pasta de
    /// mesmo nome abre `.../camel-main`, e o `pom.xml` está em
    /// `.../camel-main/camel-main`. Procurar só na raiz não achava manifesto
    /// nenhum, e o projeto caía em "Outras" — foi assim que um projeto Java
    /// deixou de ser reconhecido como Java.
    ///
    /// A condição é a mesma que a árvore já usa para mostrar o conteúdo em vez
    /// da porta seguinte: "um filho só, e ele é pasta". Nada aqui sabe o que é
    /// um projeto de linguagem nenhuma.
    fn detected_build_system(&self, root: &Path) -> Option<String> {
        let mut atual = root.to_path_buf();
        for _ in 0..ide_workspace::PROFUNDIDADE_DA_CADEIA {
            if let Some((_, descriptor)) =
                pollster::block_on(self.project.build_systems.detect(&atual))
                    .ok()
                    .flatten()
            {
                return Some(descriptor.build_system.0);
            }
            atual = self.workspace.service.only_child_directory(&atual)?;
        }
        None
    }

    /// Abre outra janela da IDE sobre o mesmo projeto.
    ///
    /// # Outro processo, e não outra janela do mesmo
    ///
    /// Uma segunda janela dentro deste processo dividiria o analisador, o
    /// índice e o supervisor de processos com a primeira — e dividir é onde
    /// mora a maior parte dos defeitos que esta sessão caçou. Um processo novo
    /// nasce com tudo próprio, e as duas janelas só compartilham o disco.
    ///
    /// O preço é honesto e vale dizer: o projeto é indexado duas vezes, e são
    /// dois analisadores externos. Quem duplica quer duas janelas de verdade.
    ///
    /// # A raiz vai como argumento
    ///
    /// Não como "o que a configuração disser": ela guarda o **último** projeto,
    /// e esta janela pode trocar de projeto antes de a outra terminar de subir.
    /// O menu diz "duplicar", e duplicar é abrir o que está aqui.
    fn duplicate_workspace(&mut self) {
        let Some(raiz) = self.runtime.workspace_root.clone() else {
            return;
        };
        let executavel = match std::env::current_exe() {
            Ok(caminho) => caminho,
            Err(error) => {
                self.report_to_shell(&format!("não foi possível duplicar: {error}"));
                return;
            }
        };
        // Sem esperar por ela: a janela nova tem vida própria, e ficar com o
        // filho pendurado faria esta janela segurar um processo que ela não
        // controla mais.
        match std::process::Command::new(executavel)
            .arg(&raiz)
            .current_dir(&raiz)
            .spawn()
        {
            Ok(_) => {
                tracing::info!(raiz = %raiz.display(), "outra janela da IDE foi aberta");
                self.report_to_shell("Outra janela deste projeto está abrindo");
            }
            Err(error) => {
                tracing::warn!(%error, "a segunda janela não subiu");
                self.report_to_shell(&format!("não foi possível duplicar: {error}"));
            }
        }
    }

    /// Diz na barra de estado, quando há shell para dizer.
    fn report_to_shell(&mut self, mensagem: &str) {
        if let Some(shell) = self.ui.shell.as_mut() {
            shell.set_status_message(mensagem.to_owned());
        }
    }

    /// Troca o projeto aberto sem fechar a IDE.
    ///
    /// Separada do diálogo do sistema porque é **aqui** que estava o defeito, e
    /// uma janela modal não cabe num teste. O que ela faz é o mesmo que abrir a
    /// IDE faz — e era a distância entre as duas que produzia os defeitos: cada
    /// coisa que a abertura passou a fazer ao longo do tempo precisava ser
    /// lembrada aqui de novo, e três delas não foram.
    fn open_another_project(&mut self, folder: &Path) {
        let tree = match self.workspace.scan(folder) {
            Ok(tree) => tree,
            Err(error) => {
                // Uma pasta que não abre **não pode derrubar a IDE**.
                // `startup_error` é fatal no `bootstrap`, e o projeto que já
                // está aberto continua bom: escolher a pasta errada é engano de
                // um clique, e não motivo para fechar o que se estava fazendo.
                if let Some(shell) = self.ui.shell.as_mut() {
                    shell.set_status_message(format!("{} não abriu: {error}", folder.display()));
                }
                return;
            }
        };
        self.reset_languages_for(folder);
        self.documents.clear();
        let mut shell = IdeShell::from_tree(tree);
        self.equip_shell(&mut shell);
        self.ui.shell = Some(shell);
        // A raiz decide qual ferramenta vale — ela ficava na do projeto
        // anterior, e com ela o `tsconfig`, o JDK e o Maven do outro projeto.
        self.runtime.workspace_root = Some(folder.to_path_buf());
        self.documents.remembered = self
            .ui
            .shell
            .as_ref()
            .map(IdeShell::open_document_paths)
            .unwrap_or_default();
        self.publish_event(IdeEvent::WorkspaceOpened {
            root: folder.to_path_buf(),
        });
        // Nesta ordem: quem diz em que linguagem a pasta é reconhecida são os
        // sistemas de build, e eles são registrados de novo a cada troca.
        self.register_build_systems();
        self.remember_project(folder);
        // Detectar ferramenta e importar o projeto rodam processos externos e
        // **esperam por eles** — o Maven chega a baixar dependências. Na
        // abertura isso acontece antes de a janela existir, e por isso ninguém
        // via; aqui a janela está na tela, e o mesmo trabalho é congelamento.
        // Fica para depois do primeiro quadro, como o realce já fica.
        self.runtime.project_pending = Some(folder.to_path_buf());
        self.runtime.languages_pending = true;
        self.observar_o_disco(folder);
        // As margens pedidas eram do projeto anterior: um caminho que se repete
        // entre dois projetos diferentes é outro arquivo.
        self.runtime.margens_pedidas.clear();
        // O branch e a contagem entram na barra de estado assim que houver
        // resposta. É a única pergunta ao Git que a IDE faz sozinha: as outras
        // saem de quem abre o gerenciador.
        self.refresh_git();
        if let Some(window) = self.window.window.as_ref() {
            window
                .inner()
                .set_title(&format!("ER IDE — {}", folder.display()));
            window.request_redraw();
        }
    }

    /// Começa a observar o projeto, com os consumidores registrados.
    ///
    /// **Um registro no sistema operacional, dois interessados.** Trocar de
    /// projeto solta o observador anterior, e com ele o registro: o `Drop` do
    /// campo é o que desliga.
    fn observar_o_disco(&mut self, raiz: &Path) {
        let (envio, recepcao) = std::sync::mpsc::channel();
        let Some(observador) = ide_watch::FileWatcher::iniciar(raiz, Vec::new()) else {
            // Sem observador a IDE volta a ser o que era, com o índice
            // envelhecendo até a próxima abertura. É degradação, e não falha.
            self.runtime.observador = None;
            self.runtime.mudancas = None;
            return;
        };
        observador.registrar(std::sync::Arc::new(ConsumidorDeFontes {
            aviso: envio.clone(),
        }));
        observador.registrar(std::sync::Arc::new(ConsumidorDeGit { aviso: envio }));
        self.runtime.observador = Some(observador);
        self.runtime.mudancas = Some(recepcao);
    }

    /// Reage ao que mudou no disco, no quadro seguinte ao aviso.
    ///
    /// É o critério da fase 4: rodar `git checkout` no terminal integrado
    /// atualiza a IDE inteira **sem ação do usuário**.
    fn collect_disk_changes(&mut self) -> bool {
        let Some(recepcao) = self.runtime.mudancas.as_ref() else {
            return false;
        };
        let mut fontes: Vec<PathBuf> = Vec::new();
        let mut repositorio = false;
        // Tudo o que chegou até agora, de uma vez: dois avisos do mesmo tipo no
        // mesmo quadro são o mesmo trabalho feito duas vezes.
        while let Ok(mudanca) = recepcao.try_recv() {
            match mudanca {
                MudancaNoDisco::Fontes(lote) => fontes.extend(lote),
                MudancaNoDisco::Repositorio => repositorio = true,
            }
        }
        if fontes.is_empty() && !repositorio {
            return false;
        }
        if repositorio {
            self.refresh_git();
        }
        // A margem do arquivo aberto acompanha o que mudou no disco: trocar de
        // branch reescreve o arquivo debaixo do editor, e a marca de antes
        // passaria a falar de um texto que não está mais lá.
        if let Some(aberto) = self
            .ui
            .shell
            .as_ref()
            .and_then(IdeShell::active_document_path)
            && fontes.contains(&aberto)
        {
            self.pedir_diferenca(aberto, false, false);
        }
        if !fontes.is_empty()
            && let Some(host) = self.languages.host.as_ref().map(Arc::clone)
        {
            // Fora da thread da interface: um `checkout` traz milhares de
            // arquivos, e reindexar um por um aqui pararia a janela pelo tempo
            // todo. A resposta não interessa — o índice é lido de onde já está.
            std::thread::spawn(move || {
                for caminho in fontes {
                    let _ = pollster::block_on(host.file_changed(&caminho));
                }
            });
        }
        true
    }

    /// Aponta as linguagens para a raiz nova, sem desligar nenhuma.
    ///
    /// # O que estava errado
    ///
    /// Antes daqui, trocar de projeto chamava `shutdown()` — que **desabilita**
    /// cada provider que tinha worker de pé e apaga as rotas dele — e depois
    /// reabilitava **um**, pelo nome, escrito no código. O resultado: as
    /// linguagens que estavam em uso eram exatamente as que morriam, ninguém
    /// respondia o pedido de realce, e o texto abria sem cor.
    ///
    /// Era também uma linguagem citada num caminho que vale para todas — e a
    /// IDE não pode se prender ao que um projeto de teste usa.
    ///
    /// # O que ela faz
    ///
    /// Solta os workers e devolve cada provider ao **registro**, de onde ele
    /// sobe sozinho na primeira pergunta, já com a raiz nova. Nenhuma linguagem
    /// é nomeada, e por isso a próxima também vai funcionar.
    fn reset_languages_for(&mut self, folder: &Path) {
        let Some(language_host) = self.languages.host.as_ref().map(Arc::clone) else {
            return;
        };
        if let Err(error) = language_host
            .set_workspace_root(folder)
            .and_then(|()| language_host.set_source_roots(Vec::new()).map(|_| ()))
        {
            tracing::warn!(%error, "a raiz nova não chegou às linguagens");
            return;
        }
        // `reset_for_new_project`, e não `detach_workers`: além de soltar quem
        // está ativo, ele devolve ao registro **quem falhou**. Um provider
        // morre por causa do projeto que estava aberto — abrir um projeto Java
        // derruba o analisador de TypeScript, porque ali não há o que ele
        // precisa. Guardar essa morte contra o projeto seguinte era julgar o
        // novo pelo que aconteceu com o velho, e foi assim que abrir um projeto
        // Angular depois de um Java não coloria nada.
        match language_host.reset_for_new_project() {
            Ok(soltos) if !soltos.is_empty() => {
                // Esperar cada worker morrer é o que congelava a janela: a fila
                // dele atende um pedido por vez, e o encerramento fica atrás de
                // uma preparação que pode levar dois minutos. Os providers já
                // voltaram ao registro; o que sobrou aqui é limpeza, e limpeza
                // não precisa de quem está desenhando.
                std::thread::spawn(move || {
                    if let Err(error) = pollster::block_on(soltos.shutdown()) {
                        tracing::warn!(%error, "worker do projeto anterior não encerrou");
                    }
                });
            }
            Ok(_) => {}
            Err(error) => tracing::warn!(%error, "não foi possível soltar os workers"),
        }
        self.documents.language.clear();
        // E tudo o que esta camada guardava por documento vai junto: a sessão
        // nova recomeça os identificadores do zero, e o que sobrasse aqui
        // passaria a falar do arquivo errado. Ver `forget_previous_project`.
        self.languages.forget_previous_project();
    }

    /// Põe no shell tudo o que ele precisa da aplicação.
    ///
    /// Uma função só, usada pela abertura e pela troca de projeto. Era a
    /// distância entre as duas que fazia o shell da troca nascer sem medida de
    /// texto — com o cursor caindo longe do clique — e sem área de
    /// transferência, com copiar e colar mortos até fechar a IDE.
    fn equip_shell(&mut self, shell: &mut IdeShell) {
        shell.set_ui_catalog(self.languages.contributions.ui_catalog());
        // Clonados por valor, e não por referência: é na passagem que o tipo
        // concreto vira o objeto de trait que o shell pede.
        if let Some(metrics) = self.ui.text_metrics.clone() {
            shell.set_text_metrics(metrics);
        }
        if let Some(clipboard) = self.ui.clipboard.clone() {
            shell.set_clipboard(clipboard);
        }
        shell.set_debug_target(
            &self.runtime.config.debug.host,
            self.runtime.config.debug.port,
        );
    }

    /// Registra os sistemas de build com a ferramenta que a **raiz atual** manda.
    ///
    /// Precisa acontecer de novo a cada troca de projeto: o caminho da
    /// ferramenta secundária é resolvido pela raiz, e o registro ficava com o do
    /// projeto anterior.
    fn register_build_systems(&mut self) {
        let Some(nativo) = self.runtime.processes.clone() else {
            return;
        };
        let processes: Arc<dyn ProcessSupervisor> = nativo;
        let secondary = self.tool_home(&java_contribution::language_id(), ToolRole::Secondary);
        java_contribution::register_build_systems(
            &mut self.project.build_systems,
            processes.clone(),
            secondary,
        );
        typescript_contribution::register_build_systems(&mut self.project.build_systems, processes);
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
        self.install_syntax(syntax);
        self.collect_declaration_kinds();
    }

    /// Entrega ao shell o realce que chegou. `true` quando chegou alguma coisa.
    ///
    /// Um lugar só, e não dois: instalar realce acontecia aqui e no laço de
    /// quadros, e nenhum dos dois dizia nada. Quando alguém relata que o código
    /// não fica colorido, a diferença entre "não foi pedido", "não voltou" e
    /// "voltou vazio" é toda a diagnose — e ela não existia em lugar nenhum.
    fn install_syntax(&mut self, snapshots: Vec<ide_domain::SyntaxSnapshot>) -> bool {
        if snapshots.is_empty() {
            return false;
        }
        let Some(shell) = self.ui.shell.as_mut() else {
            return false;
        };
        for snapshot in snapshots {
            tracing::debug!(
                document_id = snapshot.document_id.0,
                version = snapshot.version,
                realces = snapshot.highlights.len(),
                "realce instalado"
            );
            shell.set_syntax_snapshot(snapshot);
        }
        true
    }

    /// Pergunta ao índice que espécie de tipo cada arquivo declara.
    ///
    /// O Explorer só conhece caminho e pasta; classe, interface e enumeração
    /// exigem saber o que está **dentro** do arquivo. A consulta com filtro
    /// vazio devolve todo tipo do projeto com o caminho e a espécie — é uma
    /// varredura do índice, e por isso vai para uma thread própria.
    ///
    /// O mapa é chaveado pela mesma identidade que a árvore usa, e não pelo
    /// caminho: a entrada cai de cerca de duzentos bytes para trinta e dois, e
    /// é exatamente a chave pela qual a árvore pergunta.
    fn request_declaration_kinds(&mut self) {
        let Some(host) = self.languages.host.as_ref().map(Arc::clone) else {
            return;
        };
        // Uma extensão por linguagem viva, como a busca por tipo já faz. A
        // aplicação não escolhe linguagem: ela pergunta a todas.
        let extensoes: Vec<String> = self
            .languages
            .contributions
            .iter()
            .flat_map(|contribution| contribution.descriptor.extensions.clone())
            .collect();
        if extensoes.is_empty() {
            return;
        }
        let (sender, receiver) = std::sync::mpsc::channel();
        let cancel = self.languages.declaration_kinds.start(receiver);
        std::thread::spawn(move || {
            let mut mapa: HashMap<u64, ide_domain::SymbolKind> = HashMap::new();
            // **Um prazo para o laço inteiro, e não um por extensão.** Com um
            // por extensão, cada linguagem sem tipos no projeto segurava o
            // envio por noventa segundos, e o mapa só sai no fim: quatro
            // extensões quietas adiavam o primeiro crachá em seis minutos.
            let limite = Instant::now() + ESPERA_DO_INDICE;
            for extensao in extensoes {
                if cancel.is_cancelled() {
                    return;
                }
                let perguntar = || {
                    let mut contexto = host.request_context();
                    contexto.cancellation = cancel.clone();
                    // Sem limite: a pergunta é sobre o projeto inteiro, e um
                    // limite daria crachá a uns arquivos e não a outros, sem
                    // critério que alguém pudesse explicar.
                    //
                    // `ok()` e não `unwrap_or_default()`: **erro não é vazio**.
                    // Uma linguagem que não sabe responder por busca de tipo dá
                    // erro, e insistir com ela seria esperar por uma resposta
                    // que nunca vem — que é o que segurava o mapa.
                    pollster::block_on(host.workspace_types(
                        contexto,
                        &extensao,
                        String::new(),
                        usize::MAX,
                    ))
                    .ok()
                };
                // **A primeira pergunta é o que sobe o provider**, e a resposta
                // dela chega antes de o índice existir: vazia. Foi assim que o
                // crachá nunca apareceu — perguntava-se uma vez, cedo demais, e
                // não se perguntava de novo.
                //
                // Insistir é a saída honesta enquanto `workspace_types` não
                // souber dizer "ainda não sei": hoje ele responde vazio tanto
                // para isso quanto para "não há tipos aqui", e os dois se
                // parecem. É a mesma ambiguidade que a `25` separou no ponto, e
                // que aqui continua de pé. `preparing()` não serve como sinal —
                // ele é falso durante a indexação deste provider, e foi medido.
                //
                // Só se insiste **enquanto nada foi achado ainda**: depois que
                // uma linguagem respondeu, o índice está de pé, e o vazio das
                // outras quer dizer o que parece.
                let mut achados = perguntar();
                while achados.as_deref().is_some_and(<[_]>::is_empty)
                    && mapa.is_empty()
                    && !cancel.is_cancelled()
                    && Instant::now() < limite
                {
                    std::thread::sleep(Duration::from_millis(100));
                    achados = perguntar();
                }
                for symbol in achados.unwrap_or_default() {
                    mapa.insert(explorer_id(&symbol.location.path), symbol.kind);
                }
            }
            let _ = sender.send(mapa);
        });
    }

    /// Recolhe o mapa pronto e o entrega ao shell.
    fn collect_declaration_kinds(&mut self) {
        let Some(mapa) = self.languages.declaration_kinds.collect() else {
            return;
        };
        if let Some(shell) = self.ui.shell.as_mut() {
            shell.set_declaration_kinds(mapa);
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

    /// Vai para a declaração, fora da thread da interface.
    ///
    /// # Por que isto não pode ser síncrono
    ///
    /// Perguntar ao analisador externo custa o que ele demorar. Enquanto ele
    /// monta o projeto — trinta segundos num monorepo — ele não responde nada, e
    /// o pedido espera o prazo inteiro. Feito na chamada, isso é a janela parada
    /// a cada clique.
    ///
    /// Antes da fase 5 da `25` isso não aparecia: o analisador subia junto com o
    /// projeto e já estava quente quando alguém clicava. Subir sob demanda tirou
    /// a espera do começo e a pôs no primeiro clique — e revelou que a chamada
    /// sempre esteve no lugar errado.
    ///
    /// É o mesmo defeito da busca textual e da busca por tipo, pela terceira e
    /// quarta vez: trabalho que não cabe num quadro, feito dentro do quadro.
    fn navigate_to_definition(&mut self, request: NavigationRequest) {
        let Some(host) = self.languages.host.as_ref().map(Arc::clone) else {
            return;
        };
        let Some(document) = self.documents.language.get(&request.document_id) else {
            return;
        };
        let definition = DefinitionRequest {
            document_id: request.document_id,
            position: position_at_offset(&document.text, request.byte_offset),
        };
        let (sender, receiver) = std::sync::mpsc::channel();
        let _cancel = self.languages.navigation.start(receiver);
        let token = request.token.clone();
        std::thread::spawn(move || {
            let resposta = pollster::block_on(host.definition(host.request_context(), definition));
            let saida = match resposta {
                Ok(locations) => NavigationOutcome {
                    token,
                    location: locations.first().cloned(),
                    failure: None,
                },
                Err(error) => NavigationOutcome {
                    token,
                    location: None,
                    failure: Some(error.to_string()),
                },
            };
            let _ = sender.send(saida);
        });
        // **Sem aviso de que se está procurando.** Havia um `Procurando X…` na
        // barra, e ele saiu pelo mesmo motivo que o giro do clique tinha saído
        // antes: quase todo `Ctrl+clique` é respondido em milissegundos pelo
        // índice, então o aviso aparecia e sumia antes de ser lido. Uma piscada
        // a cada clique é pior do que nenhum aviso.
        //
        // O que sobra é a resposta — o arquivo abre, ou a barra diz que não
        // achou. Quem espera de verdade, porque o analisador está montando o
        // projeto, **fica sem aviso, e isso é a decisão** e não uma pendência:
        // avisar foi tentado de duas formas, o giro e o texto, e as duas foram
        // retiradas depois de usadas. Duas tentativas retiradas são evidência.
        //
        // O cancelamento continua existindo por baixo — o `SearchController`
        // sabe cancelar —, e falta só o gesto. Uma terceira tentativa começa
        // sabendo que as duas primeiras piscaram.
    }

    /// Recolhe o resultado da navegação, se já chegou.
    fn collect_navigation(&mut self) -> bool {
        let Some(resultado) = self.languages.navigation.collect() else {
            return false;
        };
        if let Some(location) = resultado.location {
            self.open_document(OpenDocumentRequest::new(&location.path).at(
                location.range.start.line as usize,
                location.range.start.column as usize,
            ));
            return true;
        }
        if let Some(shell) = self.ui.shell.as_mut() {
            // Falhar e não achar são coisas diferentes, e a mensagem diz qual foi.
            shell.set_status_message(resultado.failure.unwrap_or_else(|| {
                format!("Definition not found: {}", resultado.token)
            }));
        }
        true
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

        let raiz = self.runtime.workspace_root.clone();

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
            let mut encontrados = refine_type_hits(encontrados, &query, raiz.as_deref());
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
    }

    /// Sobe, fora da thread da interface, quem a última pergunta mandou acordar.
    ///
    /// **Criar processo não cabe num quadro.** O analisador externo é acordado
    /// pela primeira pergunta que o índice não soube — e subir um processo Node
    /// no Windows, com antivírus no caminho, leva o tempo que leva. Feito na
    /// chamada, isso acontecia na thread da interface e a janela parava: é o
    /// mesmo defeito que a busca textual e a busca por tipo já tiveram aqui.
    /// Sobe os analisadores externos **em paralelo** com a tela de abertura.
    ///
    /// # Quem decide se o projeto precisa
    ///
    /// O próprio analisador. Perguntar "este projeto é de TypeScript?" aqui
    /// exigiria que este módulo soubesse o que é um `tsconfig` — e ele não pode
    /// saber, porque a IDE serve qualquer linguagem. A lista de quem é externo
    /// vem da raiz de composição, em `alheios`, e a tentativa de subir **é** a
    /// pergunta: num projeto que não precisa dele, ele responde que não é o caso
    /// e nenhum processo nasce.
    ///
    /// # Por que falhar aqui não pode ser definitivo
    ///
    /// Um provider que falha fica fora de serviço até a próxima troca de
    /// projeto. Isso é certo quando ele morreu atendendo; é errado quando ele
    /// só não estava pronto ainda — as dependências ainda não instaladas, por
    /// exemplo. Antes deste aquecimento, quem o acordava era a primeira pergunta
    /// que o índice não soubesse responder, e essa porta precisa continuar
    /// aberta. Por isso a falha aqui **devolve o provider ao registro**.
    /// Põe as linguagens do projeto de pé, em duas frentes e fora desta thread.
    ///
    /// Uma função só, e chamada **pelos dois caminhos** — abrir a IDE e trocar
    /// de projeto pelo menu. Eram dois trechos parecidos, e o da troca não
    /// aquecia o analisador externo: abrir um projeto Angular depois de um Java
    /// deixava o Node para a primeira pergunta que o índice não soubesse, com
    /// os segundos de subida acontecendo com a IDE aberta na frente de quem
    /// esperava.
    ///
    /// # Ordem
    ///
    /// Só vale **depois** do `import_project`: é ele que registra as raízes de
    /// fontes, e registrá-las derruba os workers ativos. Aquecer antes é
    /// aquecer o que vai ser jogado fora.
    fn prepare_project_languages(&mut self) {
        // O índice do projeto, que responde os crachás e ativa as linguagens
        // nativas...
        self.request_declaration_kinds();
        // ...e o analisador externo, o que custa um processo Node.
        self.warm_external_analyzers();
    }

    fn warm_external_analyzers(&self) {
        let Some(host) = self.languages.host.as_ref() else {
            return;
        };
        let alheios = self.languages.alheios.clone();
        if alheios.is_empty() {
            return;
        }
        let host = Arc::clone(host);
        std::thread::Builder::new()
            .name("language-warmup".to_owned())
            .spawn(move || {
                for provider_id in alheios {
                    match host.activate_provider(&provider_id) {
                        Ok(()) => tracing::info!(
                            provider = provider_id.0,
                            "analisador externo subiu atrás da tela de abertura"
                        ),
                        Err(error) => {
                            tracing::info!(
                                provider = provider_id.0,
                                %error,
                                "o analisador externo não subiu agora; a primeira \
                                 pergunta que o índice não souber tenta de novo"
                            );
                            let _ = host.enable(&provider_id);
                        }
                    }
                }
            })
            .ok();
    }

    fn wake_pending_providers(&mut self) {
        let Some(host) = self.languages.host.as_ref() else {
            return;
        };
        let pendentes = host.take_pending_activation();
        if pendentes.is_empty() {
            return;
        }
        let host = Arc::clone(host);
        std::thread::Builder::new()
            .name("language-wake".to_owned())
            .spawn(move || {
                for provider_id in pendentes {
                    match host.activate_provider(&provider_id) {
                        Ok(()) => tracing::info!(
                            provider = provider_id.0,
                            "provider acordado pela pergunta"
                        ),
                        Err(error) => tracing::warn!(
                            provider = provider_id.0,
                            %error,
                            "o provider acordado não subiu"
                        ),
                    }
                }
            })
            .ok();
    }

    /// Adianta o giro do carregamento do projeto, e pede o quadro seguinte.
    ///
    /// **A IDE nao sabe o que esta sendo preparado.** Ela pergunta ao host se
    /// alguma linguagem ainda prepara o projeto, e o host pergunta ao sinal que a
    /// linguagem entregou na ativacao. Um analisador externo montando o projeto e
    /// um indice sendo construido viram aqui a mesma frase.
    ///
    /// O relogio e daqui, como o do giro da busca: o componente nao tem relogio,
    /// e a janela tambem nao.
    fn advance_project_loading(&mut self) -> bool {
        // **O giro relata o que a IDE prepara, e não o que ela espera.**
        //
        // Medido no monorepo de referência: a preparação do analisador externo
        // leva de 28 a 76 s, e nesse intervalo inteiro realce, estrutura, busca
        // por nome e navegação **já respondem**, do índice e do provider
        // nativo. Girar durante tudo isso está tecnicamente certo e
        // praticamente mentindo — quem olha conclui que deve esperar.
        //
        // Quem é "alheio" vem da composição, e não daqui: ver a fase 6 da `25`.
        //
        // **E a navegação não gira aqui.** Chegou a girar, e foi retirado depois
        // de experimentado: uma animação no meio da tela a cada `Ctrl+clique`
        // aparece e some rápido demais na maioria dos cliques, e o que era para
        // ser aviso vira piscada. Quem espera pela navegação continua com a
        // mensagem na barra de estado, que não pisca.
        let alheios = &self.languages.alheios;
        let preparando = self.languages.host.as_ref().is_some_and(|host| {
            host.preparing_providers()
                .iter()
                .any(|provider| !alheios.contains(provider))
        });
        if preparando {
            let inicio = *self
                .languages
                .preparing_since
                .get_or_insert_with(std::time::Instant::now);
            // Um giro que passa de meio minuto não é carga: é sinal que não
            // chegou. Dizer **quem** o segura tira a procura do palpite — e o
            // palpite já custou caro nesta base.
            if preparando
                && inicio.elapsed() >= PACIENCIA_COM_O_GIRO
                && !self.languages.reclamou_do_giro
            {
                self.languages.reclamou_do_giro = true;
                if let Some(host) = self.languages.host.as_ref() {
                    tracing::warn!(
                        preparando = ?host.preparing_providers(),
                        "o carregamento passou de {:?} e alguém ainda não ficou pronto",
                        PACIENCIA_COM_O_GIRO
                    );
                }
            }
            let fase = inicio.elapsed().as_secs_f32().fract();
            if let Some(shell) = self.ui.shell.as_mut() {
                shell.set_project_loading(Some(fase));
            }
            return true;
        }
        // Terminou: apaga o giro e pede **um** quadro a mais, para a tela ficar
        // sem ele. Sem esse ultimo quadro, o giro parado continuaria desenhado
        // ate o proximo evento, e um giro parado parece a IDE travada.
        self.languages.reclamou_do_giro = false;
        let estava = self.languages.preparing_since.take().is_some();
        if estava && let Some(shell) = self.ui.shell.as_mut() {
            shell.set_project_loading(None);
        }
        estava
    }

    /// Adianta o giro da janela de busca, e pede o quadro seguinte.
    ///
    /// **Devolver `true` é o que mantém a animação viva.** O laço já acorda a
    /// cada 30 ms, mas só redesenha quando alguma coisa mudou; sem isto o giro
    /// desenharia um quadro e congelaria — que é pior do que não ter giro nenhum,
    /// porque um giro parado parece a IDE travada.
    fn advance_search_spinner(&mut self) -> bool {
        let fase = self
            .workspace
            .search
            .spinner_phase()
            .or_else(|| self.languages.type_search.spinner_phase())
            .or_else(|| self.languages.referencias.spinner_phase());
        let Some(shell) = self.ui.shell.as_mut() else {
            return false;
        };
        shell.set_search_progress(fase);
        fase.is_some()
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
    }

    /// Quem usa o nome sob o cursor, fora da thread da interface.
    ///
    /// Mesmo pedido da navegação — documento, posição e nome —, porque é a mesma
    /// pergunta virada do avesso. O resultado cai no painel da busca por
    /// conteúdo: uma lista de arquivo e linha é exatamente a forma dele.
    fn find_references(&mut self, request: NavigationRequest) {
        let Some(host) = self.languages.host.as_ref().map(Arc::clone) else {
            return;
        };
        let Some(document) = self.documents.language.get(&request.document_id) else {
            return;
        };
        let pedido = ide_domain::ReferencesRequest {
            document_id: request.document_id,
            position: position_at_offset(&document.text, request.byte_offset),
            include_declaration: true,
        };
        let (sender, receiver) = std::sync::mpsc::channel();
        let _cancel = self.languages.referencias.start(receiver);
        std::thread::spawn(move || {
            let achadas = pollster::block_on(host.references(host.request_context(), pedido))
                .unwrap_or_default();
            let _ = sender.send(achadas);
        });
        if let Some(shell) = self.ui.shell.as_mut() {
            // **A janela abre agora, e não quando a resposta chega.** Procurar
            // pode levar segundos; abrir só no fim mostra o resultado sem ter
            // mostrado a procura, e quem perguntou fica sem saber se a IDE
            // ouviu. O giro vem do mesmo lugar das outras buscas.
            shell.open_content_search();
            // O campo mostra de que nome esta lista fala. Vazio, a janela
            // pareceria uma busca que ninguém pediu.
            shell.set_search_query(request.token.clone());
            shell.set_content_search_results(Vec::new());
            shell.set_status_message(format!("Procurando usos de {}…", request.token));
        }
    }

    /// Pergunta ao repositório o que mostrar, fora da thread da interface.
    ///
    /// Uma pergunta por vez: o `SearchController` guarda só a última, e a
    /// resposta de uma que ficou pelo caminho é descartada em vez de chegar
    /// depois da atual — que é a família de defeito que a `21` nomeou, a
    /// resposta velha parecida com a certa.
    fn refresh_git(&mut self) {
        let Some(raiz) = self
            .ui
            .shell
            .as_ref()
            .map(|shell| shell.workspace_root().to_path_buf())
        else {
            return;
        };
        let (sender, receiver) = std::sync::mpsc::channel();
        let cancel = self.languages.git.start(receiver);
        std::thread::spawn(move || {
            let _ = sender.send(retrato_do_repositorio(&raiz, &cancel));
        });
    }

    /// O que a tela pediu ao repositório.
    ///
    /// As escritas vão para uma thread e voltam pela mesma porta do retrato: a
    /// tela já mandou o `Refresh` junto do pedido, e é ele que refaz a lista.
    fn handle_git(&mut self, request: GitRequest) {
        match request {
            GitRequest::Refresh => self.refresh_git(),
            GitRequest::ShowDiff { path, staged } => self.pedir_diferenca(path, staged, true),
            GitRequest::Commit { message, amend } => self.commitar(message, amend),
            GitRequest::LoadHistory { ja_carregados } => self.pedir_historico(ja_carregados),
            GitRequest::SwitchBranch(nome) => self.mexer_na_branch("switch", nome),
            GitRequest::CreateBranch(nome) => self.mexer_na_branch("create", nome),
            GitRequest::Merge(nome) => self.mexer_na_branch("merge", nome),
            GitRequest::ContinueOperation => self.mexer_na_branch("continue", String::new()),
            GitRequest::AbortOperation => self.mexer_na_branch("abort", String::new()),
            GitRequest::Fetch => self.mexer_na_branch("fetch", String::new()),
            GitRequest::Pull => self.mexer_na_branch("pull", String::new()),
            GitRequest::Push => self.mexer_na_branch("push", String::new()),
            GitRequest::RestoreLine { path, from, target } => {
                self.devolver_a_linha(&path, from, target);
            }
            GitRequest::Stash => self.mexer_na_branch("stash", String::new()),
            GitRequest::StashPop(indice) => self.mexer_na_branch("pop", indice.to_string()),
            GitRequest::Stage(path) => self.escrever_no_git("stage", path),
            GitRequest::Unstage(path) => self.escrever_no_git("unstage", path),
            GitRequest::Discard(path) => self.escrever_no_git("discard", path),
        }
    }

    /// Manda uma escrita para a thread, e conta o que ela respondeu.
    ///
    /// **Sem cancelamento**, como o contrato da `22` diz: cancelar uma escrita
    /// pela metade deixaria o repositório num estado que ninguém pediu.
    fn escrever_no_git(&mut self, acao: &'static str, path: PathBuf) {
        let Some(raiz) = self
            .ui
            .shell
            .as_ref()
            .map(|shell| shell.workspace_root().to_path_buf())
        else {
            return;
        };
        let (sender, receiver) = std::sync::mpsc::channel();
        // Canal próprio, e não o das ferramentas: a saída de um build é texto
        // que vai para o painel, e isto é uma linha da barra de estado. Dividir
        // o canal faria uma escrita do Git aparecer como saída de compilação.
        let _cancel = self.languages.git_write.start(receiver);
        if let Some(shell) = self.ui.shell.as_mut() {
            shell.set_status_message(format!("Git: {acao}…"));
        }
        std::thread::spawn(move || {
            let resultado = escrever_no_repositorio(&raiz, acao, &path);
            let _ = sender.send(resultado.unwrap_or_else(|erro| format!("Git falhou: {erro}")));
        });
    }

    /// Grava o que está preparado, fora da thread da interface.
    fn commitar(&mut self, message: String, amend: bool) {
        let Some(raiz) = self
            .ui
            .shell
            .as_ref()
            .map(|shell| shell.workspace_root().to_path_buf())
        else {
            return;
        };
        let (sender, receiver) = std::sync::mpsc::channel();
        let _cancel = self.languages.git_write.start(receiver);
        if let Some(shell) = self.ui.shell.as_mut() {
            shell.set_status_message("Git: commitando…".to_owned());
        }
        std::thread::spawn(move || {
            let _ = sender.send(commitar_no_repositorio(&raiz, &message, amend));
        });
    }

    /// Pede uma página do histórico.
    ///
    /// `ja_carregados` é de onde a página começa: quem rola pede a seguinte, e
    /// o resultado é **acrescentado** ao que já está na tela. Zero recomeça, que
    /// é o que um commit novo exige.
    fn pedir_historico(&mut self, ja_carregados: usize) {
        let Some(raiz) = self
            .ui
            .shell
            .as_ref()
            .map(|shell| shell.workspace_root().to_path_buf())
        else {
            return;
        };
        let anteriores = if ja_carregados == 0 {
            Vec::new()
        } else {
            self.ui
                .shell
                .as_ref()
                .map(|shell| shell.git_view().commits.clone())
                .unwrap_or_default()
        };
        let (sender, receiver) = std::sync::mpsc::channel();
        let cancel = self.languages.git_history.start(receiver);
        std::thread::spawn(move || {
            let _ = sender.send(pagina_do_historico(&raiz, ja_carregados, anteriores, &cancel));
        });
    }

    /// Instala a página do histórico, se ela já chegou.
    fn collect_git_history(&mut self) -> bool {
        let Some(commits) = self.languages.git_history.collect() else {
            return false;
        };
        if let Some(shell) = self.ui.shell.as_mut() {
            let mut view = shell.git_view().clone();
            view.commits = commits;
            shell.set_git_view(view);
        }
        true
    }

    /// O que mexe em branch, fusão ou `stash`, fora da thread da interface.
    ///
    /// As seis coisas passam pela mesma porta porque têm a mesma forma — um
    /// nome, uma escrita, uma resposta curta — e porque **todas mudam o que
    /// está no disco debaixo do editor**. É por isso que a coleta delas
    /// recarrega o workspace.
    fn mexer_na_branch(&mut self, acao: &'static str, alvo: String) {
        let Some(raiz) = self
            .ui
            .shell
            .as_ref()
            .map(|shell| shell.workspace_root().to_path_buf())
        else {
            return;
        };
        let (sender, receiver) = std::sync::mpsc::channel();
        let _cancel = self.languages.git_write.start(receiver);
        if let Some(shell) = self.ui.shell.as_mut() {
            shell.set_status_message(format!("Git: {acao}…"));
        }
        self.runtime.git_mexeu_no_disco = true;
        std::thread::spawn(move || {
            let _ = sender.send(mexer_no_repositorio(&raiz, acao, &alvo));
        });
    }

    /// Conta o que a escrita respondeu, se ela já respondeu.
    ///
    /// **E recarrega o que o disco mudou.** É o critério da fase 3: trocar de
    /// branch pela IDE atualiza o editor, o Explorer e o índice de símbolos. Um
    /// `checkout` reescreve milhares de arquivos, e uma IDE que continuasse
    /// mostrando o texto de antes estaria mentindo sobre o que está gravado.
    fn collect_git_write(&mut self) -> bool {
        let Some(mensagem) = self.languages.git_write.collect() else {
            return false;
        };
        if let Some(shell) = self.ui.shell.as_mut() {
            shell.set_status_message(mensagem);
        }
        if std::mem::take(&mut self.runtime.git_mexeu_no_disco) {
            self.reload_workspace();
            // O índice de símbolos vem junto: sem isto, a completação
            // continuaria oferecendo as classes da branch anterior.
            self.sync_languages();
        }
        true
    }

    /// Pede a diferença de um arquivo: as marcas da margem, e talvez a
    /// comparação lado a lado.
    fn pedir_diferenca(&mut self, path: PathBuf, staged: bool, comparar: bool) {
        let Some(raiz) = self
            .ui
            .shell
            .as_ref()
            .map(|shell| shell.workspace_root().to_path_buf())
        else {
            return;
        };
        let (sender, receiver) = std::sync::mpsc::channel();
        self.languages.git_diff.push(receiver);
        // Token novo a cada pedido, e nunca cancelado: o que ele evita é a
        // resposta que já não interessa, e aqui **toda** resposta interessa —
        // cada uma é a margem de um arquivo diferente.
        let cancel = CancellationToken::new();
        std::thread::spawn(move || {
            let _ = sender.send(diferenca_do_arquivo(&raiz, path, staged, comparar, &cancel));
        });
    }

    /// Leva uma linha do arquivo de então para o de agora, e grava.
    ///
    /// **É escrita em arquivo, e por isso acontece aqui.** A tela pede; quem
    /// toca o disco é a aplicação, como em todo o resto. O texto de então vem do
    /// que já está na tela — foi ele que a comparação mostrou, e é ele que quem
    /// clicou está mandando para o outro lado.
    ///
    /// A linha que não existe mais no arquivo de agora é **acrescentada no fim**
    /// em vez de recusada em silêncio: quem pediu a linha 40 de um arquivo que
    /// hoje tem 30 quer aquela linha de volta.
    fn devolver_a_linha(&mut self, path: &Path, from: usize, target: RestoreTarget) {
        let Some(texto) = self
            .ui
            .shell
            .as_ref()
            .and_then(|shell| shell.git_diff_line(from))
        else {
            return;
        };
        let atual = std::fs::read_to_string(path).unwrap_or_default();
        // **Trocar e inserir são coisas diferentes.** A linha que existe dos
        // dois lados é trocada; a que só existe no arquivo de então foi
        // *removida*, e devolvê-la é acrescentar uma linha — trocar a que está
        // naquela posição apagaria uma linha que ninguém mandou tocar.
        let novo = match target {
            RestoreTarget::Replace(linha) => ide_workspace::rewrite_line(&atual, linha, texto),
            RestoreTarget::Insert(linha) => ide_workspace::insert_line(&atual, linha, texto),
        };
        if std::fs::write(path, &novo).is_err() {
            if let Some(shell) = self.ui.shell.as_mut() {
                shell.set_status_message("Não foi possível gravar o arquivo".to_owned());
            }
            return;
        }
        let refrescado = if let Some(shell) = self.ui.shell.as_mut() {
            // O editor principal mostra este arquivo, e o arquivo mudou. Sem
            // isto ele ficaria com o texto de antes até alguém reabri-lo.
            let refrescado = shell.refresh_document(path, &novo);
            shell.set_status_message(format!("Linha {} devolvida", from + 1));
            refrescado
        } else {
            false
        };
        // E o realce vem atrás do texto. A revisão do documento subiu, e o
        // realce guardado é o da revisão anterior — a tela o descarta, de
        // propósito, porque colorir o texto novo com os trechos do velho pinta
        // as palavras erradas. Quem não pedir realce novo aqui deixa o arquivo
        // sem cor nenhuma: o realce do clique é pedido **durante** o clique, e
        // esta troca acontece depois dele, no laço de comandos.
        if refrescado {
            self.sync_languages();
        }
        // O arquivo mudou no disco: a comparação e a margem vêm de lá, e as
        // duas precisam ser refeitas. O observador também veria, 300 ms depois;
        // quem clicou não pode esperar por ele para ver o que acabou de fazer.
        self.pedir_diferenca(path.to_path_buf(), false, true);
    }

    /// Pede a margem do arquivo que está na frente, se ela ainda não foi pedida.
    ///
    /// Roda a cada quadro e custa uma consulta a um mapa. É o que faz a marca
    /// aparecer **em qualquer forma de chegar a um arquivo** — abrir, trocar de
    /// aba, voltar de uma navegação —, sem que cada uma delas precise lembrar de
    /// pedir.
    fn ask_missing_git_marks(&mut self) {
        let Some(caminho) = self
            .ui
            .shell
            .as_ref()
            .and_then(IdeShell::git_marks_missing)
        else {
            return;
        };
        if !self.runtime.margens_pedidas.insert(caminho.clone()) {
            return;
        }
        self.pedir_diferenca(caminho, false, false);
    }

    /// Instala as comparações e as marcas que já chegaram.
    ///
    /// Todas as que chegaram, e não a primeira: são respostas de arquivos
    /// diferentes, e guardar uma para o quadro seguinte faria a margem de um
    /// arquivo esperar pela de outro.
    fn collect_git_diff(&mut self) -> bool {
        let mut chegaram = Vec::new();
        self.languages.git_diff.retain(|receptor| match receptor.try_recv() {
            Ok(resultado) => {
                chegaram.push(resultado);
                false
            }
            // Vazio quer dizer que o `git` ainda está respondendo.
            Err(std::sync::mpsc::TryRecvError::Empty) => true,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => false,
        });
        if chegaram.is_empty() {
            return false;
        }
        for resultado in chegaram {
            self.instalar_diferenca(resultado);
        }
        true
    }

    /// Instala uma comparação que chegou.
    fn instalar_diferenca(&mut self, resultado: GitDiffOutcome) -> bool {
        let Some(shell) = self.ui.shell.as_mut() else {
            return false;
        };
        if let Some(erro) = resultado.error {
            shell.set_status_message(erro);
            return true;
        }
        shell.set_git_line_marks(resultado.path.clone(), resultado.marks.clone());
        if resultado.comparar {
            let atual = std::fs::read_to_string(&resultado.path).unwrap_or_default();
            // As marcas vão junto: elas tingem o lado direito da comparação, e
            // são as mesmas que a margem do editor usa — calculá-las duas vezes
            // daria duas respostas para a mesma pergunta.
            let comparacao = ide_ui::GitDiff {
                pairs: resultado.pairs,
                // O rótulo e o caminho são do shell, que sabe onde o projeto
                // começa; daqui vai o conteúdo.
                label: String::new(),
                path: std::path::PathBuf::new(),
                committed: resultado.committed,
                current: atual,
                marks: resultado.marks,
                removed: resultado.removed,
                added_spans: resultado.added_spans,
                removed_spans: resultado.removed_spans,
            };
            if !shell.abrir_comparacao(&resultado.path, comparacao) {
                shell.set_status_message("Não foi possível abrir a comparação".to_owned());
            }
        }
        true
    }

    /// Instala o retrato, se já chegou. Não espera por nada.
    fn collect_git(&mut self) -> bool {
        let Some(view) = self.languages.git.collect() else {
            return false;
        };
        if let Some(shell) = self.ui.shell.as_mut() {
            shell.set_git_view(view);
        }
        true
    }

    /// Recolhe os usos, se já chegaram. Não espera por nada.
    fn collect_references(&mut self) -> bool {
        let Some(achadas) = self.languages.referencias.collect() else {
            return false;
        };
        let quantos = achadas.len();
        // **A linha de código, e não o nome do arquivo.** Uma lista de usos sem
        // o texto em volta obriga a abrir um por um para saber qual interessa —
        // é o que a busca por conteúdo já entendeu, e o mesmo painel a mostra.
        let mut textos: std::collections::HashMap<PathBuf, String> =
            std::collections::HashMap::new();
        let itens: Vec<ContentSearchHit> = achadas
            .into_iter()
            .map(|local| {
                let conteudo = textos.entry(local.path.clone()).or_insert_with(|| {
                    std::fs::read_to_string(&local.path).unwrap_or_default()
                });
                let preview = conteudo
                    .lines()
                    .nth(local.range.start.line as usize)
                    .unwrap_or_default()
                    .trim()
                    .to_owned();
                ContentSearchHit {
                    preview,
                    location: local,
                }
            })
            .collect();
        if let Some(shell) = self.ui.shell.as_mut() {
            // A janela já está aberta desde o pedido; aqui só chega o conteúdo.
            shell.set_content_search_results(itens);
            shell.set_status_message(format!("{quantos} uso(s)"));
        }
        true
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

    /// O que uma tecla de texto faz, e se ela pede a lista de completação.
    ///
    /// # Por que isto saiu do tratador de eventos
    ///
    /// Ficava embutido no `match` da tecla, e por isso **nenhum teste o
    /// alcançava**: montar um evento de janela exige o winit inteiro. O
    /// resultado foi um defeito relatado três vezes por quem usa, com os dois
    /// providers já sondados e respondendo em separado — o que faltava testar
    /// era justamente isto, e não dava para testar.
    ///
    /// São três coisas, nesta ordem, e a ordem importa:
    ///
    /// 1. o caractere de disparo da linguagem — o ponto —, perguntado **antes**
    ///    de o texto entrar, porque quem responde é a posição de antes;
    /// 2. o filtro da lista já aberta, que a encurta a cada letra e a fecha no
    ///    que não é nome;
    /// 3. a abertura por nome, perguntada **depois** de o texto entrar, porque
    ///    o prefixo precisa incluir a letra recém-digitada.
    fn text_typed(&mut self, text: &str) -> bool {
        let disparo = {
            let Some(shell) = self.ui.shell.as_ref() else {
                return false;
            };
            // Só pergunta quando a tecla é **do editor**. Com a janela de busca
            // na frente, há documento ativo atrás dela e ele não recebe nada:
            // digitar um ponto na busca abria o menu de completação sobre uma
            // janela que não era a do editor.
            match (
                shell.text_reaches_editor(),
                shell.active_document(),
                self.languages.host.as_ref(),
            ) {
                (true, Some(document_id), Some(host)) => {
                    let triggers = host.trigger_characters(document_id);
                    text.chars().any(|typed| triggers.contains(&typed))
                }
                _ => false,
            }
        };
        let Some(shell) = self.ui.shell.as_mut() else {
            return false;
        };
        let seguindo = shell.completion_follow_up(text);
        shell.text_input(text);
        let abrindo = shell.text_reaches_editor() && shell.completion_opens_now();
        disparo || seguindo || abrindo
    }

    /// Pergunta o que mais muda no arquivo pelo item que acabou de ser aceito.
    ///
    /// # Por que aqui, e por que numa thread
    ///
    /// Escolher `HttpClient` numa lista escreve o nome — e deixa o arquivo sem
    /// o `import`, ou seja, sem compilar. Quem sabe qual `import` escrever é a
    /// linguagem, e perguntar a ela é falar com um processo: fazer isso no
    /// quadro seria a sétima vez do defeito que a guarda do `block_on` existe
    /// para impedir.
    ///
    /// A pergunta sai daqui e a resposta é recolhida em `collect_completion`,
    /// como as outras.
    fn ask_completion_edits(&mut self) {
        let Some(label) = self
            .ui
            .shell
            .as_mut()
            .and_then(IdeShell::completacao_aceita)
        else {
            return;
        };
        let (Some(host), Some(document_id)) = (
            self.languages.host.as_ref().map(Arc::clone),
            self.ui.shell.as_ref().and_then(IdeShell::active_document),
        ) else {
            return;
        };
        let (sender, receiver) = std::sync::mpsc::channel();
        let _cancel = self.languages.completion.start(receiver);
        std::thread::spawn(move || {
            let trocas =
                pollster::block_on(host.completion_edits(host.request_context(), document_id, label))
                    .unwrap_or_default();
            let _ = sender.send(CompletionOutcome::Trocas(trocas));
        });
    }

    /// Pede a lista de completação, fora da thread da interface.
    ///
    /// # Por que isto não pode ser síncrono
    ///
    /// **É o sexto lugar com o mesmo defeito**, e o mais bem escondido. Os cinco
    /// anteriores — a busca textual, a busca por tipo, acordar o provider, a
    /// navegação e abrir um documento — foram achados na fase 5 da `25`. Este
    /// escapou porque a completação quase sempre responde rápido: o índice
    /// alcança o que o projeto declara, em milissegundos.
    ///
    /// Ele apareceu com `let w: String[]`. `String` é do próprio TypeScript, não
    /// está no índice, e a pergunta desce para o analisador — cinco segundos de
    /// prazo, com a tela parada. As teclas seguintes não somem: ficam na fila da
    /// janela e aparecem juntas quando a resposta volta.
    fn request_completion(&mut self) {
        let Some(host) = self.languages.host.as_ref().map(Arc::clone) else {
            return;
        };
        // Com a inspeção aberta, a pergunta é sobre um tipo, e não sobre uma
        // posição num arquivo: ali não existe arquivo. São duas perguntas ao
        // host, e entre elas há uma escolha que só a tela sabe fazer — por isso
        // esta manda a primeira, e o laço de quadros manda a segunda.
        if let Some(shell) = self.ui.shell.as_ref()
            && let Some((text, offset)) = shell.inspection_member_context()
            && let Some(document_id) = shell.active_document()
        {
            let (sender, receiver) = std::sync::mpsc::channel();
            let _cancel = self.languages.completion.start(receiver);
            std::thread::spawn(move || {
                let access =
                    pollster::block_on(host.member_access(host.request_context(), document_id, text, offset))
                        .map_err(|error| error.to_string());
                let _ = sender.send(CompletionOutcome::AcessoNaInspecao {
                    document_id,
                    access,
                });
            });
            return;
        }
        let Some(pedido) = self
            .ui
            .shell
            .as_ref()
            .and_then(IdeShell::completion_request)
        else {
            return;
        };
        self.pedir_completacao_do_editor(pedido);
    }

    /// Manda a pergunta do editor para a thread, e guarda o canal.
    ///
    /// Está separada porque quem pergunta é a tecla **e** o recolhimento: uma
    /// resposta que venceu vira uma pergunta nova, feita da posição de agora.
    fn pedir_completacao_do_editor(&mut self, pedido: ide_domain::CompletionRequest) {
        let Some(host) = self.languages.host.as_ref().map(Arc::clone) else {
            return;
        };
        let (sender, receiver) = std::sync::mpsc::channel();
        let _cancel = self.languages.completion.start(receiver);
        let request = pedido.clone();
        if completacao_falante() {
            eprintln!(
                "[completação] pedida em {}:{} com prefixo {:?}",
                pedido.position.line + 1,
                pedido.position.column + 1,
                pedido.prefix
            );
        }
        std::thread::spawn(move || {
            let items = pollster::block_on(host.completion(host.request_context(), request))
                .map_err(|error| error.to_string());
            let _ = sender.send(CompletionOutcome::Editor { pedido, items });
        });
    }

    /// Recolhe a completação que já chegou, e descarta a que venceu.
    ///
    /// **Uma resposta atrasada não pode entrar na tela.** Quem digita `for`
    /// depois do ponto faz três perguntas, e as duas primeiras chegam quando o
    /// cursor já andou. Aplicá-las faria a lista piscar com o conteúdo de duas
    /// teclas atrás — pior do que a espera que esta correção removeu.
    ///
    /// A pergunta de agora é a comparação: `completion_request` sai do cursor
    /// onde ele está, então uma resposta serve exatamente quando a pergunta que
    /// a gerou continua sendo a que se faria hoje.
    ///
    /// # E o que venceu vira pergunta, em vez de virar nada
    ///
    /// Descartar e parar por aí deixaria a lista fechada para sempre no caso
    /// mais comum de todos: quem digita `.` e a letra seguinte sem pausa. A
    /// resposta do ponto chega vencida, e não haveria quem pedisse de novo — a
    /// letra não pede, porque só a lista **já aberta** dispara o pedido
    /// seguinte. Antes isto não aparecia porque a espera síncrona abria a lista
    /// antes de a letra ser processada; foi a correção que criou o buraco.
    ///
    /// Então a resposta vencida vira uma pergunta nova, feita de onde o cursor
    /// está. Ela converge: quem para de digitar recebe a resposta da própria
    /// posição, e quem continua faz uma pergunta por tecla, que é o que a lista
    /// aberta já fazia.
    fn collect_completion(&mut self) -> bool {
        let Some(resultado) = self.languages.completion.collect() else {
            return false;
        };
        match resultado {
            CompletionOutcome::Editor { pedido, items } => {
                let atual = self
                    .ui
                    .shell
                    .as_ref()
                    .and_then(IdeShell::completion_request);
                if atual.as_ref() != Some(&pedido) {
                    if completacao_falante() {
                        eprintln!(
                            "[completação] resposta vencida: era {:?}, agora é {:?}",
                            pedido.prefix,
                            atual.as_ref().map(|pedido| pedido.prefix.clone())
                        );
                    }
                    if let Some(atual) = atual {
                        self.pedir_completacao_do_editor(atual);
                    }
                    return false;
                }
                match items {
                    Ok(items) => {
                        if completacao_falante() {
                            eprintln!(
                                "[completação] {} itens para o prefixo {:?}",
                                items.len(),
                                pedido.prefix
                            );
                        }
                        if let Some(shell) = self.ui.shell.as_mut() {
                            shell.set_completions(items);
                        }
                    }
                    Err(error) => {
                        // **A recusa vai para o terminal, e não só para a
                        // barra.** Na barra ela pisca: a mensagem seguinte a
                        // substitui antes de alguém conseguir ler, e quem usa só
                        // sabe dizer que "piscou". Uma falha que não se consegue
                        // relatar é uma falha que não se conserta.
                        //
                        // `eprintln!`, e não `tracing`: esta aplicação não
                        // instala assinante nenhum, e todo `tracing::warn!` que
                        // ela já tem escreve no vazio. Descoberto ao tentar usar
                        // um — e é dívida anotada, não um argumento a favor.
                        eprintln!(
                            "[completação] recusada em {}:{} com prefixo {:?}: {error}",
                            pedido.position.line + 1,
                            pedido.position.column + 1,
                            pedido.prefix
                        );
                        if let Some(shell) = self.ui.shell.as_mut() {
                            shell.set_status_message(error);
                        }
                    }
                }
            }
            CompletionOutcome::AcessoNaInspecao {
                document_id,
                access,
            } => {
                let access = match access {
                    Ok(Some(access)) => access,
                    Ok(None) => return true,
                    Err(error) => {
                        if let Some(shell) = self.ui.shell.as_mut() {
                            shell.set_inspection_message(error);
                        }
                        return true;
                    }
                };
                let Some(host) = self.languages.host.as_ref().map(Arc::clone) else {
                    return true;
                };
                let (type_name, prefix) = self
                    .ui
                    .shell
                    .as_ref()
                    .map(|shell| shell.inspection_member_target(&access.receiver, access.prefix))
                    .unwrap_or_default();
                let (sender, receiver) = std::sync::mpsc::channel();
                let _cancel = self.languages.completion.start(receiver);
                std::thread::spawn(move || {
                    let items = pollster::block_on(host.type_members(
                        host.request_context(),
                        document_id,
                        type_name,
                        prefix,
                    ))
                    .map_err(|error| error.to_string());
                    let _ = sender.send(CompletionOutcome::MembrosNaInspecao(items));
                });
            }
            CompletionOutcome::Trocas(trocas) => {
                if trocas.is_empty() {
                    return false;
                }
                if let Some(shell) = self.ui.shell.as_mut() {
                    shell.aplicar_trocas(&trocas);
                    shell.set_status_message(format!(
                        "{} import{} acrescentado{}",
                        trocas.len(),
                        if trocas.len() == 1 { "" } else { "s" },
                        if trocas.len() == 1 { "" } else { "s" }
                    ));
                }
                // O texto mudou por fora da digitação: sem isto, o realce fica
                // com a cor de antes do `import`.
                self.sync_languages();
            }
            CompletionOutcome::MembrosNaInspecao(items) => {
                if let Some(shell) = self.ui.shell.as_mut() {
                    match items {
                        Ok(items) => shell.set_completions(items),
                        Err(error) => shell.set_inspection_message(error),
                    }
                }
            }
        }
        true
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
        let linguagem = self.detected_language(root);
        if let Err(error) = self
            .runtime
            .config
            .remember_workspace(root, linguagem, &path)
        {
            tracing::warn!(%error, path = %path.display(), "configuração não pôde ser gravada");
            if let Some(shell) = self.ui.shell.as_mut() {
                shell.set_status_message(format!("Configuração não pôde ser gravada: {error}"));
            }
        }
        // A lista em memória subiu o projeto para o topo mesmo que a gravação
        // tenha falhado; o menu mostra o que a sessão sabe, e não o que o disco
        // aceitou guardar.
        self.publish_recent_projects();
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
                UiAction::Git(request) => self.handle_git(request),
                UiAction::OpenProject => self.choose_project(),
                UiAction::DuplicateWorkspace => self.duplicate_workspace(),
                UiAction::OpenRecentProject(path) => self.open_recent_project(&path),
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
                UiAction::FindReferences(request) => self.find_references(request),
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

/// Pergunta ao repositório, e traduz a resposta para o que a tela mostra.
///
/// **Esta é a única função da IDE que nomeia o `ide-git`.** É a raiz de
/// composição, e é o lugar onde o domínio vira tela — do outro lado dela o shell
/// tem `GitView`, com `String` e `usize`, e nenhum tipo do domínio de Git.
///
/// O runtime é montado aqui porque o adapter fala com um processo, e processo
/// precisa do reator do tokio. É o mesmo desenho das ferramentas de build: uma
/// linha de execução própria, runtime `current_thread` dentro dela.
fn retrato_do_repositorio(raiz: &std::path::Path, cancel: &CancellationToken) -> ide_ui::GitView {
    let mut view = ide_ui::GitView::default();
    let repositorio = match ide_git::open(raiz) {
        Ok(repositorio) => repositorio,
        Err(ide_git::GitError::NotARepository) => {
            // Não é erro: a maioria das pastas não é repositório, e a janela diz
            // isso em vez de aparecer vazia.
            return view;
        }
        Err(erro) => {
            view.message = Some(erro.to_string());
            return view;
        }
    };
    let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    else {
        view.message = Some("Não foi possível falar com o Git".to_owned());
        return view;
    };
    match runtime.block_on(repositorio.working_tree().status(cancel)) {
        Ok(status) => {
            view.head = status.head.as_ref().map(ide_git::Head::label);
            view.changed = status.changed_files();
            view.staged = status.count(ide_git::FileState::Staged);
            view.modified = status.count(ide_git::FileState::Modified);
            view.untracked = status.count(ide_git::FileState::Untracked);
            view.entries = status
                .entries
                .iter()
                .map(|entrada| ide_ui::GitEntry {
                    label: entrada
                        .path
                        .strip_prefix(raiz)
                        .unwrap_or(&entrada.path)
                        .display()
                        .to_string()
                        .replace('\\', "/"),
                    path: entrada.path.clone(),
                    state: match entrada.state {
                        ide_git::FileState::Staged => ide_ui::GitFileState::Staged,
                        ide_git::FileState::Modified => ide_ui::GitFileState::Modified,
                        ide_git::FileState::Untracked => ide_ui::GitFileState::Untracked,
                        ide_git::FileState::Conflicted => ide_ui::GitFileState::Conflicted,
                    },
                })
                .collect();
        }
        Err(erro) => view.message = Some(erro.to_string()),
    }
    if let Ok(operacao) = runtime.block_on(repositorio.integration().pending(cancel)) {
        view.pending = operacao.map(|operacao| operacao.label().to_owned());
    }
    if let Ok(tags) = runtime.block_on(repositorio.tags().list(cancel)) {
        view.tags = tags;
    }
    if let Ok(remotas) = runtime.block_on(repositorio.remotes().remote_branches(cancel)) {
        view.remotes = remotas.into_iter().map(|branch| branch.0).collect();
    }
    if let Ok(guardados) = runtime.block_on(repositorio.working_tree().stash_list(cancel)) {
        view.stashes = guardados
            .into_iter()
            .map(|item| item.message)
            .collect();
    }
    if let Ok(branches) = runtime.block_on(repositorio.branches().local(cancel)) {
        view.branches = branches
            .into_iter()
            .map(|branch| ide_ui::BranchItem {
                name: branch.name.0,
                current: branch.current,
                ahead: branch.ahead,
                behind: branch.behind,
            })
            .collect();
    }
    view
}

/// Roda uma escrita do Git e devolve o que dizer na barra de estado.
///
/// Está aqui pelo mesmo motivo de `retrato_do_repositorio`: é a raiz de
/// composição, o único lugar que pode nomear o `ide-git`.
fn escrever_no_repositorio(
    raiz: &std::path::Path,
    acao: &str,
    path: &std::path::Path,
) -> Result<String, String> {
    let repositorio = ide_git::open(raiz).map_err(|erro| erro.to_string())?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|erro| erro.to_string())?;
    let arvore = repositorio.working_tree();
    let caminhos = vec![path.to_path_buf()];
    let (resultado, feito) = match acao {
        "stage" => (runtime.block_on(arvore.stage(&caminhos)), "preparado"),
        "unstage" => (runtime.block_on(arvore.unstage(&caminhos)), "despreparado"),
        _ => (runtime.block_on(arvore.discard(&caminhos)), "descartado"),
    };
    resultado.map_err(|erro| erro.to_string())?;
    let nome = path
        .file_name()
        .map(|nome| nome.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string());
    Ok(format!("{nome} {feito}"))
}

/// Grava o commit e devolve o que dizer na barra de estado.
fn commitar_no_repositorio(raiz: &std::path::Path, mensagem: &str, amend: bool) -> String {
    let resultado = ide_git::open(raiz).map_err(|erro| erro.to_string()).and_then(
        |repositorio| {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|erro| erro.to_string())?;
            runtime
                .block_on(repositorio.history().commit(mensagem, amend))
                .map_err(|erro| erro.to_string())
        },
    );
    match resultado {
        Ok(id) => format!("Commit {} gravado", id.short()),
        Err(erro) => format!("Git falhou: {erro}"),
    }
}

/// Troca de branch, funde, sai de uma operação ou mexe no `stash`.
///
/// Está aqui pelo mesmo motivo das outras: é a raiz de composição, o único lugar
/// que pode nomear o `ide-git`.
fn mexer_no_repositorio(raiz: &std::path::Path, acao: &str, alvo: &str) -> String {
    let resultado = ide_git::open(raiz)
        .map_err(|erro| erro.to_string())
        .and_then(|repositorio| {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|erro| erro.to_string())?;
            let branch = ide_git::BranchName(alvo.to_owned());
            let branches = repositorio.branches();
            let integracao = repositorio.integration();
            let arvore = repositorio.working_tree();
            match acao {
                "switch" => runtime
                    .block_on(branches.switch(&branch))
                    .map(|()| format!("Agora em {alvo}"))
                    .map_err(|erro| erro.to_string()),
                "create" => runtime
                    .block_on(branches.create(&branch))
                    .map(|()| format!("Branch {alvo} criada"))
                    .map_err(|erro| erro.to_string()),
                "merge" => runtime
                    .block_on(integracao.merge(&branch))
                    .map(|resultado| match resultado {
                        ide_git::MergeOutcome::Merged => format!("{alvo} fundida"),
                        ide_git::MergeOutcome::AlreadyUpToDate => {
                            format!("{alvo} já estava aqui")
                        }
                        // Conflito não é falha: é trabalho a fazer, e a tela
                        // mostra quais arquivos.
                        ide_git::MergeOutcome::Conflicted { paths } => {
                            format!("{} arquivo(s) em conflito", paths.len())
                        }
                    })
                    .map_err(|erro| erro.to_string()),
                "continue" => runtime
                    .block_on(integracao.continue_operation())
                    .map(|()| "Operação concluída".to_owned())
                    .map_err(|erro| erro.to_string()),
                "abort" => runtime
                    .block_on(integracao.abort())
                    .map(|()| "Operação abortada".to_owned())
                    .map_err(|erro| erro.to_string()),
                "fetch" | "pull" | "push" => {
                    let remotos = repositorio.remotes();
                    let resultado = match acao {
                        "fetch" => runtime.block_on(remotos.fetch()),
                        "pull" => runtime.block_on(remotos.pull()),
                        _ => runtime.block_on(remotos.push(false)),
                    };
                    resultado
                        .map(|()| match acao {
                            "fetch" => "Referências atualizadas".to_owned(),
                            "pull" => "Trazido do remoto".to_owned(),
                            _ => "Enviado ao remoto".to_owned(),
                        })
                        // **A falha de autenticação é a que precisa de frase
                        // própria.** Com `GIT_TERMINAL_PROMPT=0` o `git` falha
                        // rápido em vez de ficar pendurado esperando uma senha
                        // que ninguém vai digitar; o que sobra é dizer o que
                        // aconteceu, e não "falha na ferramenta Git".
                        .map_err(|erro| match erro {
                            ide_git::GitError::AuthenticationRequired { .. } => {
                                "O remoto pediu autenticação: configure a credencial do Git"
                                    .to_owned()
                            }
                            outro => outro.to_string(),
                        })
                }
                "stash" => runtime
                    .block_on(arvore.stash_push(""))
                    .map(|()| "Trabalho guardado".to_owned())
                    .map_err(|erro| erro.to_string()),
                _ => {
                    let indice = alvo.parse::<usize>().unwrap_or_default();
                    runtime
                        .block_on(arvore.stash_pop(indice))
                        .map(|()| "Trabalho devolvido".to_owned())
                        .map_err(|erro| erro.to_string())
                }
            }
        });
    match resultado {
        Ok(mensagem) => mensagem,
        Err(erro) => format!("Git falhou: {erro}"),
    }
}

/// Uma página do histórico, já com as faixas do grafo calculadas.
///
/// **A conta das faixas é feita aqui**, e não na tela: ela é aritmética sobre
/// pais e filhos, e a tela recebe o resultado. Quem desenha o ponto e o traço é
/// a ERLibUi. Ver a `22`.
fn pagina_do_historico(
    raiz: &std::path::Path,
    ja_carregados: usize,
    anteriores: Vec<ide_ui::CommitRow>,
    cancel: &CancellationToken,
) -> Vec<ide_ui::CommitRow> {
    let Ok(repositorio) = ide_git::open(raiz) else {
        return anteriores;
    };
    let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    else {
        return anteriores;
    };
    let pagina = runtime.block_on(repositorio.history().log(
        ja_carregados,
        ide_ui::PAGINA_DO_HISTORICO,
        cancel,
    ));
    let Ok(pagina) = pagina else {
        return anteriores;
    };
    // As faixas são calculadas sobre o histórico inteiro que está na tela, e não
    // sobre a página: uma faixa aberta na página anterior continua aberta, e
    // recomeçar a conta a cada página faria o traço saltar de coluna.
    let mut todos: Vec<ide_git::CommitSummary> = anteriores
        .iter()
        .map(|linha| ide_git::CommitSummary {
            id: ide_git::CommitId(linha.hash.clone()),
            summary: linha.summary.clone(),
            author: linha.author.clone(),
            date: linha.date.clone(),
            // Os pais só interessam por identidade, e a tela guarda a faixa e
            // não o hash deles: a conta é refeita com o que a página nova traz.
            parents: Vec::new(),
        })
        .collect();
    let inicio = todos.len();
    todos.extend(pagina);
    let faixas = ide_git::graph_rows(&todos);
    todos
        .into_iter()
        .zip(faixas)
        .enumerate()
        .map(|(indice, (commit, faixa))| {
            if indice < inicio {
                // O que já estava na tela mantém o que tinha: refazer a conta
                // sem os pais deles daria faixa zero para todo mundo.
                return anteriores[indice].clone();
            }
            ide_ui::CommitRow {
                hash: commit.id.0,
                summary: commit.summary,
                author: commit.author,
                date: commit.date,
                lane: faixa.lane,
                lanes: faixa.width,
                passing: faixa.passing,
                parents: faixa.parents,
            }
        })
        .collect()
}

/// A diferença de um arquivo: o texto de então e as linhas que mudaram.
fn diferenca_do_arquivo(
    raiz: &std::path::Path,
    path: PathBuf,
    staged: bool,
    comparar: bool,
    cancel: &CancellationToken,
) -> GitDiffOutcome {
    let mut saida = GitDiffOutcome {
        path: path.clone(),
        committed: String::new(),
        marks: Vec::new(),
        removed: Vec::new(),
        added_spans: Vec::new(),
        removed_spans: Vec::new(),
        pairs: Vec::new(),
        comparar,
        error: None,
    };
    let repositorio = match ide_git::open(raiz) {
        Ok(repositorio) => repositorio,
        // Fora de repositório não há diferença nenhuma, e isso não é erro: é a
        // resposta certa para a maioria das pastas.
        Err(_) => return saida,
    };
    let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    else {
        saida.error = Some("Não foi possível falar com o Git".to_owned());
        return saida;
    };
    let arvore = repositorio.working_tree();
    let lado = if staged {
        ide_git::DiffSide::Index
    } else {
        ide_git::DiffSide::WorkingTree
    };
    // A resposta fica guardada: as fileiras precisam dela **e** dos dois
    // comprimentos, e um deles só se sabe depois de buscar o texto de então.
    // Perguntar duas vezes ao Git a mesma coisa seria dobrar a espera.
    let mut comparacao = None;
    match runtime.block_on(arvore.diff(&path, lado, cancel)) {
        Ok(diff) => {
            saida.removed = diff.removed_lines();
            let trecho = |span: &ide_git::LineSpan| ide_ui::GitSpan {
                line: span.line,
                start: span.start,
                end: span.end,
            };
            saida.added_spans = diff.added_spans().iter().map(trecho).collect();
            saida.removed_spans = diff.removed_spans().iter().map(trecho).collect();
            saida.marks = diff
                .changed_lines()
                .into_iter()
                .map(|(linha, mudanca)| {
                    let mudanca = match mudanca {
                        ide_git::LineChange::Added => ide_ui::GitLineChange::Added,
                        ide_git::LineChange::Removed => ide_ui::GitLineChange::Removed,
                    };
                    (linha, mudanca)
                })
                .collect();
            comparacao = Some(diff);
        }
        Err(erro) => saida.error = Some(erro.to_string()),
    }
    if comparar {
        match runtime.block_on(arvore.committed_text(&path, cancel)) {
            Ok(texto) => saida.committed = texto,
            // Arquivo que nunca foi commitado não tem lado esquerdo, e comparar
            // com nada é comparar com o vazio — que é a verdade.
            Err(_) => saida.committed = String::new(),
        }
        // As fileiras: quem sabe emparelhar duas versões é o domínio, e os
        // comprimentos são de quem tem os dois textos.
        if let Some(diff) = comparacao {
            let de_agora = std::fs::read_to_string(&path).unwrap_or_default();
            saida.pairs = diff
                .aligned_lines(saida.committed.lines().count(), de_agora.lines().count())
                .into_iter()
                .map(|par| ide_ui::GitLinePair {
                    old: par.old,
                    new: par.new,
                })
                .collect();
        }
    }
    saida
}

impl ApplicationHandler for NativeIde {
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        event_loop.set_control_flow(ControlFlow::WaitUntil(
            std::time::Instant::now() + std::time::Duration::from_millis(30),
        ));
        // Antes de tudo: enquanto a marca está na tela, é esta volta que a
        // mantém respondendo, e é ela que decide quando a IDE aparece.
        self.advance_splash();
        let mut changed = self.collect_content_search();
        changed |= self.collect_type_search();
        changed |= self.advance_search_spinner();
        changed |= self.advance_project_loading();
        changed |= self.collect_references();
        changed |= self.collect_git();
        changed |= self.collect_git_diff();
        self.ask_missing_git_marks();
        changed |= self.collect_git_write();
        changed |= self.collect_git_history();
        changed |= self.collect_disk_changes();
        changed |= self.collect_navigation();
        changed |= self.collect_completion();
        // Aceitar um item escreve o nome; o `import` que ele exige é a pergunta
        // seguinte, e ela vale a pena mesmo quando a resposta é vazia — que é o
        // caso de quase todo item.
        self.ask_completion_edits();
        self.wake_pending_providers();
        self.suspend_idle_languages();
        self.measure_memory();
        // O realce vem da thread do provider e chega quando fica pronto: é aqui
        // que ele encontra a tela, sem que a tecla tenha esperado por ele.
        let realces = self.languages.collect_syntax();
        changed |= self.install_syntax(realces);
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
        // O texto que a tecla trouxe, tratado depois do `match`.
        let mut digitado: Option<String> = None;
        // A raiz recém-aberta cuja ferramenta e importação ficaram para depois
        // deste quadro. Ver `open_another_project`.
        let mut projeto_novo: Option<PathBuf> = None;
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
                // **Sobre o divisor, e não só arrastando.** Um divisor que só
                // anuncia que se move depois que alguém o moveu não anuncia
                // nada. Os três respondem a mesma pergunta; o que muda é a seta.
                let (cursor, logico) = (self.window.cursor, window.logical_size());
                let horizontal = self.ui.shell.as_ref().is_some_and(|shell| {
                    shell.sidebar_divider_hover(cursor, logico)
                        || shell.split_divider_hover(cursor, logico)
                        || shell.git_divider_hover(cursor)
                });
                let vertical = self
                    .ui
                    .shell
                    .as_ref()
                    .is_some_and(|shell| shell.terminal_divider_hover(cursor, logico));
                let navigation_hover = self.ui.shell.as_ref().is_some_and(|shell| {
                    shell.navigation_hover(
                        self.window.cursor,
                        window.logical_size(),
                        self.window.control_pressed,
                    )
                });
                window.inner().set_cursor(if horizontal {
                    CursorIcon::EwResize
                } else if vertical {
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
                // **Sobre o divisor, e não só arrastando.** Um divisor que só
                // anuncia que se move depois que alguém o moveu não anuncia
                // nada. Os três respondem a mesma pergunta; o que muda é a seta.
                let (cursor, logico) = (self.window.cursor, window.logical_size());
                let horizontal = self.ui.shell.as_ref().is_some_and(|shell| {
                    shell.sidebar_divider_hover(cursor, logico)
                        || shell.split_divider_hover(cursor, logico)
                });
                let vertical = self
                    .ui
                    .shell
                    .as_ref()
                    .is_some_and(|shell| shell.terminal_divider_hover(cursor, logico));
                window.inner().set_cursor(if horizontal {
                    CursorIcon::EwResize
                } else if vertical {
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
                            // O texto é **anotado** e tratado depois do `match`:
                            // a janela está emprestada aqui, e quem digita mexe
                            // no shell e pergunta ao host. Ver `text_typed`.
                            if let Some(text) = event.text
                                && !text.chars().any(char::is_control)
                            {
                                digitado = Some(text.to_string());
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
                // E pelo mesmo motivo, o projeto recém-trocado só é detectado e
                // importado agora: a árvore nova já está na tela.
                projeto_novo = self.runtime.project_pending.take();
            }
            _ => {}
        }
        if let Some(raiz) = projeto_novo {
            // A mesma sequência da abertura, e na mesma ordem: detectar a
            // ferramenta, achar o Maven, importar o projeto. O que a abertura
            // faz e a troca não fizesse ficaria com o valor do projeto
            // anterior — e cada um desses passos termina registrando algo nas
            // linguagens, que acabaram de esquecer tudo.
            self.detect_all_toolchains(&raiz);
            self.detect_maven();
            self.import_project(&raiz);
            // O projeto novo tem outras linguagens, e as do anterior acabaram
            // de ser esquecidas. A mesma preparação da abertura, pela mesma
            // função: era aqui que o analisador externo ficava de fora, e o
            // Node só subia na primeira pergunta que o índice não soubesse.
            self.prepare_project_languages();
            if let Some(window) = self.window.window.as_ref() {
                window.request_redraw();
            }
        }
        // A tecla é tratada aqui, e não dentro do `match`: ali a janela está
        // emprestada, e escrever mexe no shell e pergunta ao host. Ver
        // `text_typed` — foi tirar isto de dentro do tratador que tornou o
        // caminho testável.
        if let Some(texto) = digitado {
            completion_requested |= self.text_typed(&texto);
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

/// Se a completação deve contar o que faz, em `ERIDE_COMPLETACAO`.
///
/// # Por que atrás de uma chave, e por que ela existe
///
/// A recusa já vai para o terminal sempre — falha é rara e precisa ser
/// relatável. **Isto é outra coisa**: o ciclo inteiro, inclusive o que deu
/// certo, que é o que falta quando o sintoma é "não aconteceu nada".
///
/// Sem ele, um pedido que responde uma lista vazia é indistinguível de um
/// pedido que nunca foi feito — e quem usa só consegue dizer "não abriu". Foi
/// preciso pedir três vezes que alguém lesse a barra de estado para descobrir
/// que a informação não estava em lugar nenhum.
///
/// Ligado, ele diz três coisas: quando um pedido sai, quando uma resposta chega
/// vencida, e quantos itens vieram.
fn completacao_falante() -> bool {
    static LIGADO: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *LIGADO.get_or_init(|| std::env::var_os("ERIDE_COMPLETACAO").is_some())
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

/// Os pedaços de uma consulta, como quem digitou os pensou.
///
/// `federated-login-context` e `FederatedLoginContext` são a mesma pergunta feita
/// de dois jeitos, e viram os mesmos três pedaços. Separam pedaço: qualquer coisa
/// que não seja letra ou dígito, e a passagem de minúscula para maiúscula.
fn query_segments(query: &str) -> Vec<String> {
    let mut segmentos = Vec::new();
    let mut atual = String::new();
    let mut anterior_minuscula = false;
    for caractere in query.chars() {
        if !caractere.is_alphanumeric() {
            if !atual.is_empty() {
                segmentos.push(std::mem::take(&mut atual));
            }
            anterior_minuscula = false;
            continue;
        }
        if caractere.is_uppercase() && anterior_minuscula && !atual.is_empty() {
            segmentos.push(std::mem::take(&mut atual));
        }
        anterior_minuscula = caractere.is_lowercase() || caractere.is_numeric();
        atual.extend(caractere.to_lowercase());
    }
    if !atual.is_empty() {
        segmentos.push(atual);
    }
    segmentos
}

/// O nome reduzido ao que se compara: só letras e dígitos, em minúscula.
fn normalized_name(name: &str) -> String {
    name.chars()
        .filter(|caractere| caractere.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// Filtra e ordena o que as linguagens acharam, pelo que foi digitado.
///
/// # Por que isto precisa existir
///
/// O analisador de TypeScript responde a `federated-login-context` com **tudo o
/// que casa com qualquer pedaço**: `context`, `login`, `contextmenu`,
/// `AudioContext`, `CONTEXT_LOST_WEBGL` — metade vinda do `lib.dom.d.ts`, que não
/// é do projeto. O casamento exato, `FederatedLoginContext`, vinha na décima
/// segunda posição. Foi o que se viu na tela: procurando por um arquivo, vieram
/// os que só tinham `login` no nome.
///
/// A recall generosa do analisador é boa e fica: o que faltava era **exigir todos
/// os pedaços** e pôr na frente quem casa melhor.
///
/// # Language-neutral de propósito
///
/// Isto trabalha sobre nome e caminho, e não sabe de linguagem nenhuma. Uma
/// consulta de um pedaço só — `Pedido`, que é como se busca em Java — passa por
/// aqui sem perder nada: o filtro só morde quando há mais de um pedaço, que é
/// exatamente o caso em que o analisador solta o OR.
fn refine_type_hits(
    found: Vec<ide_domain::SemanticSymbol>,
    query: &str,
    root: Option<&Path>,
) -> Vec<ide_domain::SemanticSymbol> {
    let segmentos = query_segments(query);
    if segmentos.is_empty() {
        return found;
    }
    let inteira: String = segmentos.concat();
    let mut pontuados: Vec<_> = found
        .into_iter()
        .filter_map(|simbolo| {
            let nome = normalized_name(&simbolo.name);
            // **Todos** os pedaços, e não qualquer um. É a correção inteira.
            if !segmentos.iter().all(|pedaco| nome.contains(pedaco.as_str())) {
                return None;
            }
            let posicao = if nome == inteira {
                0
            } else if nome.starts_with(&inteira) {
                1
            } else if nome.contains(&inteira) {
                2
            } else {
                3
            };
            // Empate desfeito pelo que é do projeto: um tipo de dependência não
            // costuma ser o que se procurou.
            let de_fora = usize::from(
                !root.is_some_and(|raiz| simbolo.location.path.starts_with(raiz)),
            );
            Some((posicao, de_fora, nome.len(), simbolo))
        })
        .collect();
    pontuados.sort_by(|esquerda, direita| {
        (esquerda.0, esquerda.1, esquerda.2)
            .cmp(&(direita.0, direita.1, direita.2))
            .then_with(|| esquerda.3.name.cmp(&direita.3.name))
    });
    pontuados
        .into_iter()
        .map(|(_, _, _, simbolo)| simbolo)
        .collect()
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
        let fora = match snapshot.state {
            ide_language_api::ProviderState::Failed => "indisponível",
            // Desligado não é falha, e dizer "indisponível" faria procurar
            // defeito onde houve escolha.
            ide_language_api::ProviderState::Disabled => "desligado na configuração",
            _ => return None,
        };
        let cuida = snapshot.metadata.extensions.iter().any(|extensao| {
            relevantes
                .iter()
                .any(|aberta| aberta.eq_ignore_ascii_case(extensao))
        });
        if !cuida {
            return None;
        }
        let nome = snapshot.metadata.display_name;
        let motivo = snapshot.last_error.map_or_else(String::new, |detalhe| format!(": {detalhe}"));
        Some(format!(
            "{nome} {fora}, e a análise nativa não tem índice{motivo}"
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
mod tests;
