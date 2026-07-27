#![doc = "Shell visual e interativo da IDE baseado no ERLibUi."]

use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    hash::{DefaultHasher, Hash, Hasher},
    path::{Path, PathBuf},
    sync::Arc,
};

use ide_domain::{
    CompletionItem, CompletionRequest, DocumentId, DocumentSnapshot, OutlineItem,
    SyntaxHighlightKind, SyntaxSnapshot, TextPosition as DomainTextPosition,
};
use ide_terminal::{ShellKind, TerminalSession};
use ide_text::EditorSession;
use ui_editor::{
    CodeEditor, GutterMark, LineDecoration, SyntaxSpan, TextRange as EditorRange, TokenKind,
};
use ide_workspace::{FileNode, WorkspaceError};
use ui_api::{EventContext, LayoutContext, PaintContext, TextMetrics, Widget};
use ui_components::{
    Button, ComboBox, ComboBoxItem, Icon, IconTint, ListView, MenuBar, MenuBarItem, MenuItem,
    ModalHost, Scrollbar, ScrollbarOrientation, SplitOrientation, Splitter, StatusBar, TabItem,
    Tabs, TextInput, TreeItem, TreeView,
};
use ui_core::{
    Color, ColorTokens, CommandId, EventResult, FontId, KeyEvent, Modifiers, Point, PointerButton,
    PointerEvent, Rect, Size, Theme, UiEvent, WidgetAction, WidgetId,
};
use ui_render_api::{DrawTextCommand, FillRectCommand, PaintCommand, StrokeRectCommand};

const ACTIVITY_WIDTH: f32 = 48.0;
const SIDEBAR_WIDTH: f32 = 260.0;
const SIDEBAR_MIN_WIDTH: f32 = 160.0;
const TITLE_HEIGHT: f32 = 36.0;
const TAB_HEIGHT: f32 = 38.0;
const EXPLORER_ROW_HEIGHT: f32 = 23.0;
const EXPLORER_TOP: f32 = 106.0;
/// Métricas do editor: elas são do componente que desenha o código, e a IDE
/// as consulta para posicionar cursor, popup e cliques no mesmo lugar.
const EDITOR_LINE_HEIGHT: f32 = CodeEditor::line_height();
const EDITOR_GUTTER: f32 = CodeEditor::gutter_width();
/// Largura do caractere na fonte de código, definida pelo editor que a desenha.
const EDITOR_CHAR_WIDTH: f32 = CodeEditor::default_char_width();
const TAB_WIDTH: f32 = 140.0;
const TERMINAL_TAB_WIDTH: f32 = 110.0;
const TERMINAL_TAB_HEIGHT: f32 = 30.0;
const TERMINAL_DEFAULT_HEIGHT: f32 = 180.0;
const TERMINAL_MIN_HEIGHT: f32 = 120.0;
const TERMINAL_COLLAPSED_HEIGHT: f32 = 30.0;
const TERMINAL_CHAR_WIDTH: f32 = 8.4;
const DEBUG_PANEL_WIDTH: f32 = 320.0;
const DEBUG_ROW_HEIGHT: f32 = 21.0;
const MENU_BAR_ID: WidgetId = WidgetId(10_001);
const SETTINGS_MODAL_ID: WidgetId = WidgetId(10_002);
const JDK_COMBO_ID: WidgetId = WidgetId(10_003);
const JDK_BROWSE_ID: WidgetId = WidgetId(10_004);
const SETTINGS_CLOSE_ID: WidgetId = WidgetId(10_005);
const DEBUG_HOST_ID: WidgetId = WidgetId(10_006);
const DEBUG_PORT_ID: WidgetId = WidgetId(10_007);
const DEBUG_ATTACH_ID: WidgetId = WidgetId(10_008);
const STOP_BUTTON_ID: WidgetId = WidgetId(10_009);
const RUN_BUTTON_ID: WidgetId = WidgetId(10_010);
const DEBUG_BUTTON_ID: WidgetId = WidgetId(10_011);
const DEBUG_FRAMES_ID: WidgetId = WidgetId(10_012);
const DEBUG_VARIABLES_ID: WidgetId = WidgetId(10_013);
const STATUS_BAR_ID: WidgetId = WidgetId(10_014);
const EDITOR_TABS_ID: WidgetId = WidgetId(10_015);
const TERMINAL_TABS_ID: WidgetId = WidgetId(10_016);
const EDITOR_SCROLLBAR_ID: WidgetId = WidgetId(10_017);
const TERMINAL_SCROLLBAR_ID: WidgetId = WidgetId(10_018);
const EXPLORER_VERTICAL_SCROLLBAR_ID: WidgetId = WidgetId(10_019);
const EXPLORER_HORIZONTAL_SCROLLBAR_ID: WidgetId = WidgetId(10_021);
const EXPLORER_TREE_ID: WidgetId = WidgetId(10_020);
const SIDEBAR_SPLITTER_ID: WidgetId = WidgetId(10_022);
const TERMINAL_SPLITTER_ID: WidgetId = WidgetId(10_023);
const EDITOR_VIEW_ID: WidgetId = WidgetId(10_024);

/// Página ativa da janela de configurações.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum SettingsPage {
    #[default]
    Compiler,
    Debug,
}

/// Pedido da interface para a sessão de depuração.
///
/// A apresentação não conhece protocolo nem servidor: apenas descreve o que o
/// usuário pediu, e a aplicação traduz para a sessão ativa.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DebugRequest {
    Attach {
        host: String,
        port: u16,
    },
    /// Sobe a aplicação do projeto com depuração e conecta nela.
    RunAndAttach {
        host: String,
        port: u16,
    },
    Continue,
    Pause,
    StepOver,
    StepInto,
    StepOut,
    Detach,
    SelectFrame(usize),
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DebugFrameView {
    pub name: String,
    pub location: Option<(PathBuf, u32)>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DebugVariableView {
    pub name: String,
    pub value: String,
    pub type_name: Option<String>,
}

/// Estado da depuração apresentado pela interface.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DebugView {
    pub attached: bool,
    pub status: String,
    pub stopped_at: Option<(PathBuf, u32)>,
    pub frames: Vec<DebugFrameView>,
    pub selected_frame: usize,
    pub variables: Vec<DebugVariableView>,
}

impl DebugView {
    #[must_use]
    pub const fn is_stopped(&self) -> bool {
        self.stopped_at.is_some()
    }
}

#[derive(Clone, Copy)]
struct TextPosition {
    line: usize,
    column: usize,
}

#[derive(Clone, Copy)]
struct TerminalSelection {
    anchor: TextPosition,
    focus: TextPosition,
}

/// Qual barra de rolagem está sob o gesto.
///
/// O deslocamento da pegada pertence ao componente; aqui só se registra qual
/// deles está sendo arrastado, para que o mesmo movimento não role duas áreas.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ScrollTarget {
    Editor,
    Terminal,
    ExplorerHorizontal,
    ExplorerVertical,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ShellFocus {
    #[default]
    None,
    Explorer,
    Editor,
    Search,
    Terminal,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NavigationRequest {
    pub document_id: DocumentId,
    pub byte_offset: usize,
    pub token: String,
}

struct TerminalTab {
    session: TerminalSession,
    scroll_line: usize,
    follow_output: bool,
}

struct SettingsDialog {
    message: Option<String>,
}

pub struct IdeShell {
    workspace_name: String,
    workspace: FileNode,
    /// O Explorer é a `TreeView` da biblioteca. Ela é reconstruída só quando a
    /// árvore ou a expansão mudam — o caminho oposto ao das abas, porque
    /// remontar milhares de nós a cada quadro custaria caro.
    explorer_tree: TreeView,
    expanded: HashSet<PathBuf>,
    editor: EditorSession,
    cursor_offset: usize,
    focus: ShellFocus,
    search_query: String,
    terminals: Vec<TerminalTab>,
    active_terminal: usize,
    editor_scroll_line: usize,
    explorer_scroll_x: f32,
    explorer_scroll_line: usize,
    sidebar_width: f32,
    terminal_height: f32,
    terminal_last_height: f32,
    terminal_minimized: bool,
    /// Medição de texto oferecida pela aplicação.
    ///
    /// Sem ela os componentes estimam por contagem de caracteres; com ela cada
    /// um pergunta à fonte que vai desenhar. A IDE não constrói o mecanismo:
    /// ela recebe a porta e a repassa aos widgets.
    text_metrics: Option<Arc<dyn TextMetrics>>,
    /// Linha alcançada pela última navegação, com o cursor que ela deixou.
    ///
    /// O destaque vale enquanto o cursor continuar onde a navegação o pôs. Não
    /// há o que limpar: assim que o usuário clica ou digita, o cursor muda e o
    /// destaque deixa de valer sozinho — nenhum caminho novo precisa lembrar de
    /// apagá-lo.
    navigated: Option<(usize, usize)>,
    /// Linha que precisa aparecer no próximo quadro.
    ///
    /// Quem navega não conhece a altura do editor; ela só existe na pintura, que
    /// é onde a revelação acontece.
    pending_reveal: Option<usize>,
    /// Cópia do documento ativo para desenho, com a revisão que a originou.
    ///
    /// O texto continua sendo do `EditorSession`; o editor da biblioteca é
    /// quem o desenha, e é reconstruído só quando a revisão muda.
    editor_view: Option<(DocumentId, u64, CodeEditor)>,
    /// Divisores redimensionáveis do layout, com limites em pontos.
    ///
    /// Eles guardam o arraste entre um evento e o seguinte; a posição e os
    /// limites são reconciliados com o tamanho da janela a cada uso.
    sidebar_splitter: Splitter,
    terminal_splitter: Splitter,
    scrollbar_drag: Option<ScrollTarget>,
    /// As quatro barras de rolagem da janela, como widgets da biblioteca.
    ///
    /// Elas guardam o estado do arraste entre um evento e o seguinte; faixa,
    /// trilha e deslocamento são reconciliados com o conteúdo a cada uso.
    editor_scrollbar: Scrollbar,
    terminal_scrollbar: Scrollbar,
    explorer_vertical_scrollbar: Scrollbar,
    explorer_horizontal_scrollbar: Scrollbar,
    terminal_selection: Option<TerminalSelection>,
    terminal_selecting: bool,
    menu_bar: MenuBar,
    settings_modal: ModalHost,
    jdk_combo: ComboBox,
    jdk_browse_button: Button,
    settings_close_button: Button,
    open_project_requested: bool,
    open_settings_requested: bool,
    build_project_requested: bool,
    reimport_project_requested: bool,
    run_requested: bool,
    stop_requested: bool,
    /// Aba de terminal em que a aplicação foi iniciada pela IDE.
    running_terminal: Option<usize>,
    project_summary: Option<String>,
    browse_jdk_requested: bool,
    pending_navigation: Option<NavigationRequest>,
    status_message: String,
    syntax_snapshots: HashMap<DocumentId, SyntaxSnapshot>,
    completion_items: Vec<CompletionItem>,
    completion_selected: usize,
    settings_dialog: Option<SettingsDialog>,
    settings_jdk_result: Option<usize>,
    settings_page: SettingsPage,
    settings_focus: Option<WidgetId>,
    stop_button: Button,
    run_button: Button,
    debug_button: Button,
    debug_host: TextInput,
    debug_port: TextInput,
    debug_attach_button: Button,
    theme: Theme,
    breakpoints: BTreeMap<PathBuf, BTreeSet<u32>>,
    /// Linhas que o alvo confirmou, por arquivo.
    verified_breakpoints: BTreeMap<PathBuf, BTreeSet<u32>>,
    breakpoints_dirty: Option<PathBuf>,
    debug: DebugView,
    /// Última posição do ponteiro, entregue às abas reconstruídas a cada quadro
    /// para que o botão de fechar apareça sob o cursor.
    pointer: Point,
    /// Pilha de chamadas e variáveis são listas da biblioteca: rolagem,
    /// seleção, recorte e acessibilidade não se reimplementam aqui.
    debug_frames: ListView,
    debug_variables: ListView,
    debug_requests: Vec<DebugRequest>,
}

impl IdeShell {
    pub fn open(root: &Path) -> Result<Self, WorkspaceError> {
        let workspace = FileNode::scan(root)?;
        Ok(Self::from_tree(workspace))
    }

    pub fn from_tree(workspace: FileNode) -> Self {
        let workspace_name = workspace
            .path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("workspace")
            .to_owned();
        let mut expanded = HashSet::new();
        expanded.insert(workspace.path.clone());
        let terminal_root = if workspace.path.is_dir() {
            workspace.path.clone()
        } else {
            PathBuf::from(".")
        };
        let terminals = TerminalSession::discover_profiles()
            .into_iter()
            .filter_map(|profile| {
                TerminalSession::new(terminal_root.clone(), 2_000, profile)
                    .ok()
                    .map(|session| TerminalTab {
                        session,
                        scroll_line: 0,
                        follow_output: true,
                    })
            })
            .collect();
        let explorer_tree = TreeView::new(EXPLORER_TREE_ID, explorer_items(&workspace))
            .with_row_height(EXPLORER_ROW_HEIGHT);
        let mut shell = Self {
            workspace_name,
            workspace,
            explorer_tree,
            expanded,
            editor: EditorSession::default(),
            cursor_offset: 0,
            focus: ShellFocus::None,
            search_query: String::new(),
            terminals,
            active_terminal: 0,
            editor_scroll_line: 0,
            explorer_scroll_x: 0.0,
            explorer_scroll_line: 0,
            sidebar_width: SIDEBAR_WIDTH,
            terminal_height: TERMINAL_DEFAULT_HEIGHT,
            terminal_last_height: TERMINAL_DEFAULT_HEIGHT,
            terminal_minimized: false,
            text_metrics: None,
            navigated: None,
            pending_reveal: None,
            editor_view: None,
            sidebar_splitter: Splitter::new(SIDEBAR_SPLITTER_ID, SplitOrientation::Horizontal),
            terminal_splitter: Splitter::new(TERMINAL_SPLITTER_ID, SplitOrientation::Vertical),
            scrollbar_drag: None,
            editor_scrollbar: Scrollbar::new(EDITOR_SCROLLBAR_ID, ScrollbarOrientation::Vertical),
            terminal_scrollbar: Scrollbar::new(
                TERMINAL_SCROLLBAR_ID,
                ScrollbarOrientation::Vertical,
            ),
            explorer_vertical_scrollbar: Scrollbar::new(
                EXPLORER_VERTICAL_SCROLLBAR_ID,
                ScrollbarOrientation::Vertical,
            ),
            explorer_horizontal_scrollbar: Scrollbar::new(
                EXPLORER_HORIZONTAL_SCROLLBAR_ID,
                ScrollbarOrientation::Horizontal,
            ),
            terminal_selection: None,
            terminal_selecting: false,
            menu_bar: MenuBar::new(
                MENU_BAR_ID,
                vec![
                    MenuBarItem::menu("Arquivo", vec![MenuItem::new("Projeto...", "file.project")]),
                    MenuBarItem::menu(
                        "Projeto",
                        vec![
                            MenuItem::new("Compilar projeto", "project.build"),
                            MenuItem::new("Reimportar projeto", "project.reimport"),
                            MenuItem::new("Executar aplicação", "project.run"),
                            MenuItem::new("Parar aplicação", "project.stop"),
                        ],
                    ),
                    MenuBarItem::menu(
                        "Depurar",
                        vec![
                            MenuItem::new("Conectar...", "debug.connect"),
                            MenuItem::new("Continuar", "debug.continue"),
                            MenuItem::new("Pausar", "debug.pause"),
                            MenuItem::new("Passo sobre", "debug.over"),
                            MenuItem::new("Entrar", "debug.into"),
                            MenuItem::new("Sair", "debug.out"),
                            MenuItem::new("Desconectar", "debug.detach"),
                        ],
                    ),
                    MenuBarItem::command("Configurações", "settings.open"),
                ],
            ),
            settings_modal: ModalHost::new(
                SETTINGS_MODAL_ID,
                "Configurações",
                Size::new(780.0, 460.0),
            ),
            jdk_combo: ComboBox::new(JDK_COMBO_ID, Vec::new()).with_command_prefix("jdk.select."),
            jdk_browse_button: Button::new(JDK_BROWSE_ID, "Procurar...").with_command("jdk.browse"),
            settings_close_button: Button::new(SETTINGS_CLOSE_ID, "Fechar")
                .with_command("settings.close"),
            open_project_requested: false,
            open_settings_requested: false,
            build_project_requested: false,
            reimport_project_requested: false,
            run_requested: false,
            stop_requested: false,
            running_terminal: None,
            project_summary: None,
            browse_jdk_requested: false,
            pending_navigation: None,
            status_message: "Ready".to_owned(),
            syntax_snapshots: HashMap::new(),
            completion_items: Vec::new(),
            completion_selected: 0,
            settings_dialog: None,
            settings_jdk_result: None,
            settings_page: SettingsPage::default(),
            settings_focus: None,
            stop_button: Button::icon(STOP_BUTTON_ID, Icon::Stop, "Parar aplicação")
                .with_tint(IconTint::Muted)
                .with_command("project.stop"),
            run_button: Button::icon(RUN_BUTTON_ID, Icon::Play, "Executar aplicação")
                .with_tint(IconTint::Success)
                .with_command("project.run"),
            debug_button: Button::icon(DEBUG_BUTTON_ID, Icon::Bug, "Executar com depuração")
                .with_tint(IconTint::Muted)
                .with_command("debug.run"),
            debug_host: TextInput::new(DEBUG_HOST_ID, "127.0.0.1").with_placeholder("host"),
            debug_port: TextInput::new(DEBUG_PORT_ID, "8000").with_placeholder("porta"),
            debug_attach_button: Button::new(DEBUG_ATTACH_ID, "Conectar")
                .with_command("debug.attach"),
            theme: Theme::default(),
            breakpoints: BTreeMap::new(),
            verified_breakpoints: BTreeMap::new(),
            breakpoints_dirty: None,
            debug: DebugView::default(),
            pointer: Point::new(-1.0, -1.0),
            debug_frames: ListView::new(DEBUG_FRAMES_ID, Vec::<String>::new())
                .with_row_height(DEBUG_ROW_HEIGHT),
            debug_variables: ListView::new(DEBUG_VARIABLES_ID, Vec::<String>::new())
                .with_row_height(DEBUG_ROW_HEIGHT),
            debug_requests: Vec::new(),
        };
        shell.sync_explorer_tree();
        shell
    }

    /// Recebe o mecanismo que mede o texto que a janela desenha.
    pub fn set_text_metrics(&mut self, metrics: Arc<dyn TextMetrics>) {
        self.text_metrics = Some(metrics);
    }

    /// Contexto de pintura com a medição disponível, quando houver.
    fn paint_context(&self) -> PaintContext {
        let context = PaintContext::with_theme(self.theme);
        match self.text_metrics.as_ref() {
            Some(metrics) => context.measuring(Arc::clone(metrics)),
            None => context,
        }
    }

    /// Contexto de layout com a medição disponível, quando houver.
    fn layout_context(&self) -> LayoutContext {
        match self.text_metrics.as_ref() {
            Some(metrics) => LayoutContext::with_text_metrics(Arc::clone(metrics)),
            None => LayoutContext::default(),
        }
    }

    pub fn open_file(&mut self, path: &Path) -> Result<DocumentId, String> {
        let id = self.editor.open(path).map_err(|error| error.to_string())?;
        self.cursor_offset = 0;
        self.focus = ShellFocus::Editor;
        self.status_message = format!("Opened {}", path.display());
        Ok(id)
    }

    pub const fn focus(&self) -> ShellFocus {
        self.focus
    }
    pub const fn active_document(&self) -> Option<DocumentId> {
        self.editor.active_id()
    }
    pub fn active_text(&self) -> Option<&str> {
        self.editor.active().map(|document| document.buffer.text())
    }
    pub fn document_snapshots(&self) -> Vec<DocumentSnapshot> {
        self.editor
            .tabs()
            .map(|document| DocumentSnapshot {
                id: document.id,
                path: document.path.clone(),
                version: document.buffer.revision(),
                text: document.buffer.text().to_owned(),
            })
            .collect()
    }
    pub fn set_syntax_snapshot(&mut self, snapshot: SyntaxSnapshot) {
        if self.editor.document(snapshot.document_id).is_none() {
            return;
        }
        let error_count = snapshot.diagnostics.len();
        let symbol_count = count_outline(&snapshot.outline);
        let import_count = snapshot.imports.len();
        self.status_message = format!(
            "Java: {error_count} error(s), {symbol_count} symbol(s), {import_count} import(s)"
        );
        self.syntax_snapshots.insert(snapshot.document_id, snapshot);
    }
    pub fn syntax_snapshot(&self, document_id: DocumentId) -> Option<&SyntaxSnapshot> {
        self.syntax_snapshots.get(&document_id)
    }
    pub fn active_outline(&self) -> &[OutlineItem] {
        self.active_document()
            .and_then(|id| self.syntax_snapshots.get(&id))
            .map_or(&[], |snapshot| snapshot.outline.as_slice())
    }
    pub fn completion_request(&self) -> Option<CompletionRequest> {
        let document = self.editor.active()?;
        let (line, column) = line_column(document.buffer.text(), self.cursor_offset);
        Some(CompletionRequest {
            document_id: document.id,
            position: DomainTextPosition {
                line: line as u32,
                column: column as u32,
            },
            prefix: identifier_prefix(document.buffer.text(), self.cursor_offset),
        })
    }
    pub fn set_completions(&mut self, items: Vec<CompletionItem>) {
        self.completion_items = items;
        self.completion_selected = 0;
    }
    /// Mensagem atual da barra de estado.
    #[must_use]
    pub fn status_message(&self) -> &str {
        &self.status_message
    }

    pub fn set_status_message(&mut self, message: impl Into<String>) {
        self.status_message = message.into();
    }
    pub fn open_settings_dialog(&mut self, jdk_items: Vec<String>, selected_jdk: usize) {
        self.jdk_combo.set_items(
            jdk_items
                .into_iter()
                .enumerate()
                .map(|(index, label)| ComboBoxItem::new(label, index.to_string()))
                .collect(),
        );
        self.jdk_combo.set_selected(selected_jdk);
        self.settings_modal.open();
        self.settings_dialog = Some(SettingsDialog { message: None });
        self.settings_jdk_result = None;
        self.browse_jdk_requested = false;
    }
    pub const fn settings_dialog_open(&self) -> bool {
        self.settings_modal.is_open()
    }
    /// Troca o tema da interface.
    ///
    /// O tema vem da ERLibUi e vale para tudo — inclusive para os componentes da
    /// biblioteca, que o recebem pelo contexto de pintura. A IDE não guarda cor
    /// própria.
    pub fn set_theme(&mut self, theme: Theme) {
        self.theme = theme;
    }

    #[must_use]
    pub const fn theme(&self) -> &Theme {
        &self.theme
    }

    /// Alvo de depuração apresentado na janela e usado pelo botão de depurar.
    pub fn set_debug_target(&mut self, host: &str, port: u16) {
        self.debug_host.set_value(host);
        self.debug_port.set_value(port.to_string());
    }

    #[must_use]
    pub fn debug_target(&self) -> Option<(String, u16)> {
        let host = self.debug_host.value().trim().to_owned();
        let port = self.debug_port.value().trim().parse::<u16>().ok()?;
        (!host.is_empty() && port > 0).then_some((host, port))
    }

    /// Executa um comando na aba de terminal ativa, como se o usuário digitasse.
    pub fn run_in_terminal(&mut self, command: &str) -> Result<(), String> {
        let active = self.active_terminal;
        let Some(tab) = self.terminals.get_mut(active) else {
            return Err("Nenhum terminal disponível".to_owned());
        };
        tab.session
            .run(command)
            .map_err(|error| error.to_string())?;
        tab.follow_output = true;
        self.terminal_minimized = false;
        self.running_terminal = Some(active);
        Ok(())
    }

    /// Interrompe a aplicação iniciada pela IDE, como um `Ctrl+C` do usuário.
    pub fn stop_application(&mut self) -> Result<(), String> {
        let Some(index) = self.running_terminal else {
            return Err("Nenhuma aplicação iniciada pela IDE".to_owned());
        };
        let Some(tab) = self.terminals.get_mut(index) else {
            self.running_terminal = None;
            return Err("O terminal da aplicação não existe mais".to_owned());
        };
        tab.session.interrupt().map_err(|error| error.to_string())?;
        tab.follow_output = true;
        self.active_terminal = index;
        self.running_terminal = None;
        self.status_message = "Aplicação interrompida".to_owned();
        Ok(())
    }

    /// Indica que a IDE iniciou uma aplicação e ainda não a interrompeu.
    #[must_use]
    pub const fn application_running(&self) -> bool {
        self.running_terminal.is_some()
    }

    /// Escolhe a página apresentada pela janela de configurações.
    ///
    /// Abrir a janela pelo menu mantém a última página usada; atalhos que
    /// prometem uma página específica precisam declará-la.
    pub fn set_settings_page(&mut self, page: SettingsPage) {
        self.settings_page = page;
        self.settings_focus = None;
    }
    #[must_use]
    pub const fn settings_page(&self) -> SettingsPage {
        self.settings_page
    }
    pub fn take_settings_jdk_result(&mut self) -> Option<usize> {
        self.settings_jdk_result.take()
    }
    pub fn take_browse_jdk_request(&mut self) -> bool {
        std::mem::take(&mut self.browse_jdk_requested)
    }
    pub fn set_settings_message(&mut self, message: impl Into<String>) {
        if let Some(dialog) = self.settings_dialog.as_mut() {
            dialog.message = Some(message.into());
        }
    }
    pub fn append_tool_output(&mut self, text: &str, is_error: bool) {
        let active = self.active_terminal;
        self.terminals[active]
            .session
            .append_external_output(text, is_error);
        self.terminals[active].follow_output = true;
        self.terminals[active].scroll_line = self.terminals[active].session.line_count();
    }
    pub fn java_source_files(&self) -> Vec<PathBuf> {
        fn collect(node: &FileNode, output: &mut Vec<PathBuf>) {
            if node.is_directory {
                for child in &node.children {
                    collect(child, output);
                }
            } else if node
                .path
                .extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| extension.eq_ignore_ascii_case("java"))
            {
                output.push(node.path.clone());
            }
        }
        let mut files = Vec::new();
        collect(&self.workspace, &mut files);
        files
    }
    pub fn navigation_hover(&self, point: Point, size: Size, control: bool) -> bool {
        if !control {
            return false;
        }
        let geometry = self.geometry(size);
        let editor_x = ACTIVITY_WIDTH + self.sidebar_width(size);
        if point.x < editor_x + EDITOR_GUTTER
            || point.y < geometry.content_top
            || point.y >= geometry.editor_bottom
        {
            return false;
        }
        let Some(document) = self.editor.active() else {
            return false;
        };
        let offset = self.offset_at_point(point, editor_x, geometry.content_top);
        if token_at(document.buffer.text(), offset).is_none() {
            return false;
        }
        let (line, column) = line_column(document.buffer.text(), offset);
        self.syntax_snapshots
            .get(&document.id)
            .filter(|snapshot| snapshot.version == document.buffer.revision())
            .is_some_and(|snapshot| {
                snapshot.highlights.iter().any(|highlight| {
                    is_navigable(highlight.kind) && position_in_range(line, column, highlight.range)
                })
            })
    }
    pub fn tab_count(&self) -> usize {
        self.editor.tabs().count()
    }

    /// Caminhos das abas abertas, na ordem em que aparecem.
    ///
    /// Documentos criados em memória não têm arquivo por trás e ficam de fora:
    /// registrar um caminho que não existe só produziria uma aba impossível de
    /// reabrir.
    #[must_use]
    pub fn open_document_paths(&self) -> Vec<PathBuf> {
        self.editor
            .tabs()
            .filter(|document| document.path.is_file())
            .map(|document| document.path.clone())
            .collect()
    }

    /// Caminho do documento em foco.
    #[must_use]
    pub fn active_document_path(&self) -> Option<PathBuf> {
        self.editor
            .active()
            .map(|document| document.path.clone())
            .filter(|path| path.is_file())
    }
    pub fn is_expanded(&self, path: &Path) -> bool {
        self.expanded.contains(path)
    }
    pub fn selected_shell(&self) -> ShellKind {
        self.active_terminal().selected_profile().kind
    }
    pub const fn editor_scroll_line(&self) -> usize {
        self.editor_scroll_line
    }
    pub fn terminal_scroll_line(&self) -> usize {
        self.terminals[self.active_terminal].scroll_line
    }
    pub const fn active_terminal_index(&self) -> usize {
        self.active_terminal
    }
    pub const fn terminal_height(&self) -> f32 {
        self.terminal_height
    }
    pub const fn terminal_minimized(&self) -> bool {
        self.terminal_minimized
    }
    pub const fn terminal_resizing(&self) -> bool {
        self.terminal_splitter.is_dragging()
    }
    pub const fn sidebar_resizing(&self) -> bool {
        self.sidebar_splitter.is_dragging()
    }
    pub fn active_terminal_lines(&self) -> impl Iterator<Item = &str> {
        self.active_terminal()
            .lines()
            .map(|line| line.text.as_str())
    }
    pub fn take_navigation_request(&mut self) -> Option<NavigationRequest> {
        self.pending_navigation.take()
    }
    pub fn take_open_project_request(&mut self) -> bool {
        std::mem::take(&mut self.open_project_requested)
    }
    /// Alterna o breakpoint de uma linha e marca o arquivo para sincronização.
    pub fn toggle_breakpoint(&mut self, path: &Path, line: u32) {
        let lines = self.breakpoints.entry(path.to_path_buf()).or_default();
        if !lines.remove(&line) {
            lines.insert(line);
        }
        if lines.is_empty() {
            self.breakpoints.remove(path);
        }
        self.breakpoints_dirty = Some(path.to_path_buf());
        self.status_message = format!("Breakpoints: {}", self.breakpoint_count());
    }

    /// Alterna o breakpoint na linha do cursor do documento ativo.
    pub fn toggle_breakpoint_at_cursor(&mut self) {
        let Some(document) = self.editor.active() else {
            return;
        };
        let path = document.path.clone();
        let (line, _) = line_column(document.buffer.text(), self.cursor_offset);
        self.toggle_breakpoint(&path, line as u32);
    }

    #[must_use]
    pub fn breakpoints_for(&self, path: &Path) -> Vec<u32> {
        self.breakpoints
            .get(path)
            .map(|lines| lines.iter().copied().collect())
            .unwrap_or_default()
    }

    #[must_use]
    pub fn breakpoint_count(&self) -> usize {
        self.breakpoints.values().map(BTreeSet::len).sum()
    }

    /// Registra quais linhas o alvo confirmou, para diferenciá-las na calha.
    pub fn set_verified_breakpoints(&mut self, path: &Path, lines: &[u32]) {
        if lines.is_empty() {
            self.verified_breakpoints.remove(path);
        } else {
            self.verified_breakpoints
                .insert(path.to_path_buf(), lines.iter().copied().collect());
        }
    }

    #[must_use]
    pub fn breakpoint_is_verified(&self, path: &Path, line: u32) -> bool {
        self.verified_breakpoints
            .get(path)
            .is_some_and(|lines| lines.contains(&line))
    }

    /// Arquivo cujos breakpoints mudaram desde a última consulta.
    pub fn take_breakpoints_dirty(&mut self) -> Option<PathBuf> {
        self.breakpoints_dirty.take()
    }

    pub fn take_debug_requests(&mut self) -> Vec<DebugRequest> {
        std::mem::take(&mut self.debug_requests)
    }

    pub fn request_debug(&mut self, request: DebugRequest) {
        self.debug_requests.push(request);
    }

    /// Substitui o estado de depuração apresentado.
    pub fn set_debug_view(&mut self, view: DebugView) {
        if let Some((path, line)) = view.stopped_at.clone()
            && self.debug.stopped_at.as_ref() != Some(&(path.clone(), line))
        {
            let _ = self.open_location(&path, line as usize, 0);
        }
        self.debug = view;
        self.debug_frames.set_items(
            self.debug
                .frames
                .iter()
                .map(|frame| match &frame.location {
                    Some((_, line)) => format!("{}:{}", frame.name, line + 1),
                    None => frame.name.clone(),
                })
                .collect::<Vec<_>>(),
        );
        self.debug_frames.set_selected(Some(self.debug.selected_frame));
        self.debug_variables.set_items(
            self.debug
                .variables
                .iter()
                .map(|variable| format!("{} = {}", variable.name, variable.value))
                .collect::<Vec<_>>(),
        );
    }

    #[must_use]
    pub const fn debug_view(&self) -> &DebugView {
        &self.debug
    }

    #[must_use]
    pub const fn debug_attached(&self) -> bool {
        self.debug.attached
    }

    pub fn take_build_project_request(&mut self) -> bool {
        std::mem::take(&mut self.build_project_requested)
    }
    pub fn take_reimport_project_request(&mut self) -> bool {
        std::mem::take(&mut self.reimport_project_requested)
    }
    /// Pedido de executar a aplicação, sem depuração.
    pub fn take_run_request(&mut self) -> bool {
        std::mem::take(&mut self.run_requested)
    }
    /// Pedido de interromper a aplicação iniciada pela IDE.
    pub fn take_stop_request(&mut self) -> bool {
        std::mem::take(&mut self.stop_requested)
    }
    /// Resumo do projeto importado, apresentado na barra de status.
    pub fn set_project_summary(&mut self, summary: Option<String>) {
        self.project_summary = summary;
    }
    pub fn project_summary(&self) -> Option<&str> {
        self.project_summary.as_deref()
    }
    pub fn take_open_settings_request(&mut self) -> bool {
        std::mem::take(&mut self.open_settings_requested)
    }
    pub fn workspace_path(&self) -> &Path {
        &self.workspace.path
    }
    pub fn active_terminal_input(&self) -> &str {
        self.active_terminal().input()
    }

    fn active_terminal(&self) -> &TerminalSession {
        &self.terminals[self.active_terminal].session
    }

    fn active_terminal_mut(&mut self) -> &mut TerminalSession {
        &mut self.terminals[self.active_terminal].session
    }

    pub fn update_terminals(&mut self, size: Size) -> bool {
        let geo = self.geometry(size);
        let rows = ((geo.terminal_height - 62.0) / EDITOR_LINE_HEIGHT).max(1.0) as u16;
        let mut changed = false;
        for terminal in &mut self.terminals {
            let received = terminal.session.drain_output();
            changed |= received > 0;
            if received > 0 && terminal.follow_output {
                terminal.scroll_line = terminal.session.line_count().saturating_sub(rows as usize);
            }
        }
        changed
    }

    fn geometry(&self, size: Size) -> Geometry {
        let mut geometry = geometry(
            size,
            if self.terminal_minimized {
                TERMINAL_COLLAPSED_HEIGHT
            } else {
                self.terminal_height
            },
            self.sidebar_width(size),
        );
        // O painel de depuração ocupa a direita do editor enquanto há sessão,
        // em vez de cobrir o código.
        if self.debug.attached {
            geometry.editor_width =
                (geometry.editor_width - DEBUG_PANEL_WIDTH).max(EDITOR_GUTTER * 2.0);
        }
        geometry
    }

    /// Abas do editor montadas a partir dos documentos abertos.
    ///
    /// O widget é reconstruído a cada uso porque a verdade são os documentos, e
    /// não uma cópia deles: assim nenhuma abertura, gravação ou fechamento
    /// precisa lembrar de sincronizar a barra de abas.
    fn editor_tabs(&self) -> Tabs {
        let items = self
            .editor
            .tabs()
            .map(|document| {
                let title = document
                    .path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("?");
                TabItem::new(document.id.0, title)
                    .closable()
                    .modified(document.buffer.is_dirty())
            })
            .collect();
        let mut tabs = Tabs::new(EDITOR_TABS_ID, items)
            .with_tab_width(TAB_WIDTH)
            .with_pointer(self.pointer);
        if let Some(active) = self.editor.active_id() {
            tabs.set_active_id(active.0);
        }
        tabs
    }

    fn editor_tabs_rect(&self, size: Size) -> Rect {
        let geo = self.geometry(size);
        Rect::new(
            ACTIVITY_WIDTH + self.sidebar_width(size),
            TITLE_HEIGHT,
            geo.editor_width,
            TAB_HEIGHT,
        )
    }

    /// Abas do painel de terminal, uma por perfil aberto. Terminais não fecham
    /// pela aba: eles pertencem à janela enquanto ela existir.
    fn terminal_tabs(&self) -> Tabs {
        let items = self
            .terminals
            .iter()
            .enumerate()
            .map(|(index, terminal)| {
                TabItem::new(index as u64, terminal.session.selected_profile().kind.label())
            })
            .collect();
        let mut tabs = Tabs::new(TERMINAL_TABS_ID, items).with_tab_width(TERMINAL_TAB_WIDTH);
        tabs.set_active(self.active_terminal);
        tabs
    }

    fn terminal_tabs_rect(&self, size: Size) -> Rect {
        let geo = self.geometry(size);
        Rect::new(
            ACTIVITY_WIDTH + self.sidebar_width(size),
            geo.editor_bottom,
            geo.editor_width,
            TERMINAL_TAB_HEIGHT,
        )
    }

    /// Trilha, faixa e deslocamento de uma barra, na unidade do seu conteúdo.
    ///
    /// As barras verticais contam linhas e a horizontal conta pontos de largura:
    /// para o componente é a mesma aritmética, e é por isso que uma só barra
    /// serve às quatro áreas.
    fn scrollbar_range(&self, target: ScrollTarget, size: Size) -> (Rect, f32, f32, f32) {
        match target {
            ScrollTarget::Editor => (
                self.editor_scrollbar_rect(size),
                self.active_text().map_or(0, |text| text.lines().count()) as f32,
                self.editor_visible_lines(size) as f32,
                self.editor_scroll_line as f32,
            ),
            ScrollTarget::Terminal => {
                let active = &self.terminals[self.active_terminal];
                (
                    self.terminal_scrollbar_rect(size),
                    active.session.line_count() as f32,
                    self.terminal_visible_lines(size) as f32,
                    active.scroll_line as f32,
                )
            }
            ScrollTarget::ExplorerVertical => (
                self.explorer_vertical_scrollbar_rect(size),
                self.visible_entries().len() as f32,
                self.explorer_visible_lines(size) as f32,
                self.explorer_scroll_line as f32,
            ),
            ScrollTarget::ExplorerHorizontal => {
                let track = self.explorer_horizontal_scrollbar_rect(size);
                (
                    track,
                    self.explorer_content_width(size),
                    (track.size.width - 28.0).max(1.0),
                    self.explorer_scroll_x,
                )
            }
        }
    }

    fn scrollbar_mut(&mut self, target: ScrollTarget) -> &mut Scrollbar {
        match target {
            ScrollTarget::Editor => &mut self.editor_scrollbar,
            ScrollTarget::Terminal => &mut self.terminal_scrollbar,
            ScrollTarget::ExplorerVertical => &mut self.explorer_vertical_scrollbar,
            ScrollTarget::ExplorerHorizontal => &mut self.explorer_horizontal_scrollbar,
        }
    }

    /// Alinha a barra ao conteúdo atual antes de entregar-lhe um evento.
    fn sync_scrollbar(&mut self, target: ScrollTarget, size: Size) {
        let (track, content, viewport, offset) = self.scrollbar_range(target, size);
        let context = self.layout_context();
        let bar = self.scrollbar_mut(target);
        bar.layout(&context, track);
        bar.set_range(content, viewport);
        bar.set_offset(offset);
    }

    /// Traz de volta o deslocamento que a barra escolheu.
    fn apply_scrollbar(&mut self, target: ScrollTarget) {
        let offset = self.scrollbar_mut(target).offset();
        match target {
            ScrollTarget::Editor => self.editor_scroll_line = offset.round().max(0.0) as usize,
            ScrollTarget::Terminal => {
                let maximum = self.terminal_scrollbar.max_offset();
                let active = self.active_terminal;
                self.terminals[active].scroll_line = offset.round().max(0.0) as usize;
                // Chegar ao fim volta a acompanhar a saída; parar no meio é
                // pedir para ficar onde está.
                self.terminals[active].follow_output = offset >= maximum;
            }
            ScrollTarget::ExplorerVertical => {
                self.explorer_scroll_line = offset.round().max(0.0) as usize;
            }
            ScrollTarget::ExplorerHorizontal => self.explorer_scroll_x = offset.max(0.0),
        }
    }

    /// Entrega o clique à barra cuja trilha o contém.
    fn scrollbar_pointer_down(&mut self, point: Point, size: Size) -> bool {
        for target in [
            ScrollTarget::Terminal,
            ScrollTarget::Editor,
            ScrollTarget::ExplorerHorizontal,
            ScrollTarget::ExplorerVertical,
        ] {
            if target == ScrollTarget::Terminal && self.terminal_minimized {
                continue;
            }
            let (track, ..) = self.scrollbar_range(target, size);
            if !track.contains(point) {
                continue;
            }
            self.sync_scrollbar(target, size);
            let handled = self.scrollbar_mut(target).event(
                &mut EventContext::default(),
                &UiEvent::PointerDown(primary_pointer(point)),
            );
            if matches!(handled, EventResult::Handled) {
                self.apply_scrollbar(target);
                self.scrollbar_drag = Some(target);
            }
            // A trilha consome o clique mesmo sem indicador: ali não há
            // conteúdo para reagir embaixo.
            return true;
        }
        false
    }

    /// Desenha uma barra com o componente da biblioteca.
    fn paint_scrollbar(&self, target: ScrollTarget, size: Size) -> Vec<PaintCommand> {
        let (track, content, viewport, offset) = self.scrollbar_range(target, size);
        let orientation = match target {
            ScrollTarget::ExplorerHorizontal => ScrollbarOrientation::Horizontal,
            _ => ScrollbarOrientation::Vertical,
        };
        let mut bar = Scrollbar::new(WidgetId(0), orientation).with_range(content, viewport);
        bar.layout(&self.layout_context(), track);
        bar.set_offset(offset);
        let mut paint = self.paint_context();
        bar.paint(&mut paint);
        paint.into_commands()
    }

    /// Área da árvore de arquivos.
    fn explorer_tree_rect(&self, size: Size) -> Rect {
        let geo = self.geometry(size);
        Rect::new(
            ACTIVITY_WIDTH,
            EXPLORER_TOP,
            self.sidebar_width(size),
            (geo.content_bottom - 12.0 - EXPLORER_TOP).max(0.0),
        )
    }

    /// Espelha na árvore o que a IDE considera expandido.
    ///
    /// A expansão continua sendo do shell porque ela é indexada por caminho e
    /// serve a mais gente do que ao desenho; a árvore recebe as identidades
    /// correspondentes.
    fn sync_explorer_tree(&mut self) {
        let ids: Vec<u64> = self.expanded.iter().map(|path| explorer_id(path)).collect();
        self.explorer_tree.set_expanded(ids);
    }

    /// Posiciona a árvore de acordo com as barras de rolagem da janela.
    fn explorer_tree_for(&self, size: Size) -> TreeView {
        let mut tree = self.explorer_tree.clone();
        tree.layout(&self.layout_context(), self.explorer_tree_rect(size));
        tree.set_scroll_offset(Point::new(
            self.explorer_scroll_x,
            self.explorer_scroll_line as f32 * EXPLORER_ROW_HEIGHT,
        ));
        tree
    }

    fn explorer_path_for(&self, id: u64) -> Option<(PathBuf, bool)> {
        fn visit(node: &FileNode, id: u64) -> Option<(PathBuf, bool)> {
            if explorer_id(&node.path) == id {
                return Some((node.path.clone(), node.is_directory));
            }
            node.children.iter().find_map(|child| visit(child, id))
        }
        visit(&self.workspace, id)
    }

    /// Divisor da barra lateral posicionado pelo layout atual.
    ///
    /// A barra lateral é limitada pela largura mínima dela e pela do editor; o
    /// terminal, pela altura mínima dele e pelo espaço que o editor precisa
    /// manter. São limites em pontos, não proporções.
    fn sidebar_splitter_for(&self, size: Size) -> Splitter {
        let geometry = self.geometry(size);
        let mut splitter = self.sidebar_splitter.clone();
        splitter.layout(
            &self.layout_context(),
            Rect::new(
                0.0,
                TITLE_HEIGHT,
                size.width,
                (geometry.content_bottom - TITLE_HEIGHT).max(0.0),
            ),
        );
        splitter.set_range(
            ACTIVITY_WIDTH + SIDEBAR_MIN_WIDTH,
            ACTIVITY_WIDTH + (size.width - 320.0).max(SIDEBAR_MIN_WIDTH),
        );
        splitter.set_position(ACTIVITY_WIDTH + self.sidebar_width(size));
        splitter
    }

    fn terminal_splitter_for(&self, size: Size) -> Splitter {
        let geometry = self.geometry(size);
        let editor_x = ACTIVITY_WIDTH + self.sidebar_width(size);
        let maximum =
            (geometry.content_bottom - geometry.content_top - 100.0).max(TERMINAL_MIN_HEIGHT);
        let mut splitter = self.terminal_splitter.clone();
        splitter.layout(
            &self.layout_context(),
            Rect::new(
                editor_x,
                geometry.content_top,
                (size.width - editor_x).max(0.0),
                (geometry.content_bottom - geometry.content_top).max(0.0),
            ),
        );
        splitter.set_range(
            geometry.content_bottom - maximum,
            geometry.content_bottom - TERMINAL_MIN_HEIGHT,
        );
        splitter.set_position(geometry.editor_bottom);
        splitter
    }

    fn sync_splitters(&mut self, size: Size) {
        self.sidebar_splitter = self.sidebar_splitter_for(size);
        self.terminal_splitter = self.terminal_splitter_for(size);
    }

    /// Traz de volta o tamanho que cada divisor definiu.
    fn apply_splitters(&mut self, size: Size) {
        let content_bottom = self.geometry(size).content_bottom;
        if self.sidebar_splitter.is_dragging() {
            self.sidebar_width = self.sidebar_splitter.position() - ACTIVITY_WIDTH;
        }
        if self.terminal_splitter.is_dragging() {
            self.terminal_height = content_bottom - self.terminal_splitter.position();
            self.terminal_last_height = self.terminal_height;
        }
    }

    /// Entrega o clique ao divisor cujo alvo o contém.
    fn splitter_pointer_down(&mut self, point: Point, size: Size) -> bool {
        self.sync_splitters(size);
        let event = UiEvent::PointerDown(primary_pointer(point));
        let mut context = EventContext::default();
        if matches!(
            self.sidebar_splitter.event(&mut context, &event),
            EventResult::Handled
        ) {
            return true;
        }
        !self.terminal_minimized
            && matches!(
                self.terminal_splitter.event(&mut context, &event),
                EventResult::Handled
            )
    }

    /// Linha da calha sob o ponteiro, respondida pelo próprio editor.
    fn gutter_line_at(&mut self, point: Point, size: Size) -> Option<usize> {
        self.refresh_editor_view(size);
        self.editor_view
            .as_ref()
            .and_then(|(_, _, editor)| editor.gutter_line_at(point))
    }

    fn editor_view_rect(&self, size: Size) -> Rect {
        let geometry = self.geometry(size);
        Rect::new(
            ACTIVITY_WIDTH + self.sidebar_width(size),
            geometry.content_top,
            geometry.editor_width,
            geometry.editor_height,
        )
    }

    /// Mantém a cópia de desenho alinhada ao documento, ao realce sintático, aos
    /// pontos de parada e à rolagem.
    ///
    /// O texto só é copiado quando a revisão muda; o resto é barato e vai a cada
    /// quadro, porque muda por fora do editor — o depurador pára em outra linha,
    /// o analisador entrega outro realce.
    fn refresh_editor_view(&mut self, size: Size) {
        let Some(document) = self.editor.active() else {
            self.editor_view = None;
            return;
        };
        let (id, revision) = (document.id, document.buffer.revision());
        let stale = !matches!(&self.editor_view, Some((view_id, view_revision, _))
            if *view_id == id && *view_revision == revision);
        if stale {
            let editor = CodeEditor::new(EDITOR_VIEW_ID, document.buffer.text());
            self.editor_view = Some((id, revision, editor));
        }
        let path = document.path.clone();
        let syntax = self.editor_syntax(id, revision);
        let decorations = self.editor_decorations(&path);
        let focused = self.focus == ShellFocus::Editor;
        // A IDE conta bytes e o editor conta caracteres: sem converter, o cursor
        // sairia do lugar no primeiro acento do arquivo.
        let text = document.buffer.text();
        let cursor = text
            .get(..self.cursor_offset.min(text.len()))
            .unwrap_or(text)
            .chars()
            .count();
        let scroll_line = self.editor_scroll_line;
        let reveal = self.pending_reveal;
        let bounds = self.editor_view_rect(size);
        let context = self.layout_context();
        let Some((_, _, editor)) = self.editor_view.as_mut() else {
            return;
        };
        editor.layout(&context, bounds);
        editor.set_syntax(syntax);
        editor.set_decorations(decorations);
        editor.set_focused(focused);
        editor.set_cursor(cursor);
        editor.set_scroll_line(scroll_line);
        if let Some(line) = reveal {
            editor.reveal_line(line);
        }
        let scrolled = editor.scroll_line();
        self.editor_scroll_line = scrolled;
        self.pending_reveal = None;
    }

    /// Converte o realce da IDE, que fala em linha e coluna, para os intervalos
    /// absolutos que o editor da biblioteca usa.
    fn editor_syntax(&self, id: DocumentId, revision: u64) -> Vec<SyntaxSpan> {
        let Some(snapshot) = self
            .syntax_snapshots
            .get(&id)
            .filter(|snapshot| snapshot.version == revision)
        else {
            return Vec::new();
        };
        let Some((_, _, editor)) = self.editor_view.as_ref() else {
            return Vec::new();
        };
        let buffer = editor.buffer();
        snapshot
            .highlights
            .iter()
            .map(|highlight| SyntaxSpan {
                range: EditorRange::new(
                    buffer.offset(
                        highlight.range.start.line as usize,
                        highlight.range.start.column as usize,
                    ),
                    buffer.offset(
                        highlight.range.end.line as usize,
                        highlight.range.end.column as usize,
                    ),
                ),
                token_kind: token_kind_for(highlight.kind),
            })
            .collect()
    }

    /// Pontos de parada e a linha em que a execução parou.
    fn editor_decorations(&self, path: &Path) -> Vec<LineDecoration> {
        let mut decorations: Vec<LineDecoration> = self
            .breakpoints
            .get(path)
            .into_iter()
            .flatten()
            .map(|line| {
                // Confirmado pelo alvo é um disco; ainda não registrado — sem
                // sessão, ou classe não carregada — é só o contorno.
                let mark = if self.breakpoint_is_verified(path, *line) {
                    GutterMark::Breakpoint
                } else {
                    GutterMark::PendingBreakpoint
                };
                LineDecoration::mark(*line as usize, mark)
            })
            .collect();
        let destacar = |decorations: &mut Vec<LineDecoration>, line: usize| {
            match decorations.iter_mut().find(|item| item.line == line) {
                Some(existing) => *existing = existing.with_highlight(),
                None => decorations.push(LineDecoration::highlight(line)),
            }
        };
        if let Some((_, line)) = self
            .debug
            .stopped_at
            .as_ref()
            .filter(|(stopped, _)| stopped == path)
        {
            destacar(&mut decorations, *line as usize);
        }
        // Destino da última navegação, enquanto o cursor não sair de lá.
        if let Some((line, cursor)) = self.navigated
            && cursor == self.cursor_offset
        {
            destacar(&mut decorations, line);
        }
        decorations
    }

    fn debug_panel_rect(&self, size: Size) -> Rect {
        let geometry = self.geometry(size);
        let x = ACTIVITY_WIDTH + self.sidebar_width(size) + geometry.editor_width;
        Rect::new(
            x,
            geometry.content_top,
            (size.width - x).max(0.0),
            geometry.editor_height,
        )
    }

    fn paint_debug_panel(&self, size: Size, colors: ColorTokens) -> Vec<PaintCommand> {
        let geometry = debug_panel_geometry(self.debug_panel_rect(size), self.debug.frames.len());
        let panel = geometry.panel;
        let mut commands = vec![
            fill(panel, colors.surface),
            fill(
                Rect::new(panel.origin.x, panel.origin.y, 1.0, panel.size.height),
                colors.border,
            ),
            PaintCommand::PushClip(panel),
            label(
                &self.debug.status,
                Point::new(panel.origin.x + 12.0, panel.origin.y + 10.0),
                if self.debug.is_stopped() {
                    colors.accent
                } else {
                    colors.text
                },
                13.0,
            ),
        ];

        for (rect, (title, _)) in geometry.buttons.iter().zip(DEBUG_BUTTONS) {
            commands.push(fill(*rect, colors.elevated));
            commands.push(stroke(*rect, colors.border));
            commands.push(label(
                title,
                Point::new(rect.origin.x + 8.0, rect.origin.y + 7.0),
                if self.debug.is_stopped() {
                    colors.text
                } else {
                    colors.muted_text
                },
                12.0,
            ));
        }

        commands.push(label(
            "Pilha de chamadas",
            Point::new(panel.origin.x + 12.0, geometry.frames.origin.y - 20.0),
            colors.muted_text,
            12.0,
        ));
        let mut frames = self.debug_frames.clone();
        frames.layout(&self.layout_context(), geometry.frames);
        let mut variables = self.debug_variables.clone();
        variables.layout(&self.layout_context(), geometry.variables);
        let mut lists = self.paint_context();
        frames.paint(&mut lists);

        commands.push(label(
            "Variáveis",
            Point::new(panel.origin.x + 12.0, geometry.variables.origin.y - 20.0),
            colors.muted_text,
            12.0,
        ));
        variables.paint(&mut lists);
        commands.extend(lists.into_commands());
        commands.push(PaintCommand::PopClip);
        commands
    }

    fn debug_panel_pointer_down(&mut self, point: Point, size: Size) {
        let geometry = debug_panel_geometry(self.debug_panel_rect(size), self.debug.frames.len());
        for (rect, (_, request)) in geometry.buttons.iter().zip(DEBUG_BUTTONS) {
            if rect.contains(point) {
                self.debug_requests.push(request);
                return;
            }
        }
        // A lista resolve qual linha foi clicada; a IDE só reage à escolha.
        // Clicar fora das linhas é ignorado por ela, e é isso que distingue uma
        // escolha de um clique no vazio do painel.
        self.debug_frames.layout(&self.layout_context(), geometry.frames);
        let result = self.debug_frames.event(
            &mut EventContext::default(),
            &UiEvent::PointerDown(primary_pointer(point)),
        );
        if matches!(result, EventResult::Ignored) {
            return;
        }
        let Some(row) = self.debug_frames.selected() else {
            return;
        };
        self.debug.selected_frame = row;
        self.debug_requests.push(DebugRequest::SelectFrame(row));
        if let Some((path, line)) = self
            .debug
            .frames
            .get(row)
            .and_then(|frame| frame.location.clone())
        {
            let _ = self.open_location(&path, line as usize, 0);
        }
    }

    fn sidebar_width(&self, size: Size) -> f32 {
        self.sidebar_width.clamp(
            SIDEBAR_MIN_WIDTH,
            (size.width - 320.0).max(SIDEBAR_MIN_WIDTH),
        )
    }

    pub fn toggle_search(&mut self) {
        if self.focus == ShellFocus::Search {
            self.search_query.clear();
            self.focus = ShellFocus::Editor;
        } else {
            self.focus = ShellFocus::Search;
        }
    }

    pub fn escape(&mut self) {
        if self.settings_modal.is_open() {
            self.settings_modal.close();
            self.settings_dialog = None;
            return;
        }
        if !self.completion_items.is_empty() {
            self.completion_items.clear();
            return;
        }
        if self.focus == ShellFocus::Search {
            self.search_query.clear();
            self.focus = ShellFocus::Editor;
        }
    }

    pub fn pointer_down(&mut self, point: Point, size: Size) {
        self.pointer_down_with_modifiers(point, size, false);
    }

    pub fn pointer_down_with_modifiers(&mut self, point: Point, size: Size, control: bool) {
        if self.settings_modal.is_open() {
            self.settings_dialog_pointer_down(point, size);
            return;
        }
        if point.y < TITLE_HEIGHT && self.action_buttons_pointer_down(point, size) {
            return;
        }
        self.menu_bar.layout(
            &self.layout_context(),
            Rect::new(82.0, 0.0, (size.width - 82.0).max(0.0), TITLE_HEIGHT),
        );
        let mut menu_context = EventContext::default();
        let menu_result = self.menu_bar.event(
            &mut menu_context,
            &UiEvent::PointerDown(primary_pointer(point)),
        );
        match menu_result {
            EventResult::Action(WidgetAction::Command(command)) if command.0 == "file.project" => {
                self.open_project_requested = true;
                self.status_message = "Select a project folder".to_owned();
                return;
            }
            EventResult::Action(WidgetAction::Command(command)) if command.0 == "settings.open" => {
                self.open_settings_requested = true;
                return;
            }
            EventResult::Action(WidgetAction::Command(command)) if command.0 == "project.build" => {
                self.build_project_requested = true;
                return;
            }
            EventResult::Action(WidgetAction::Command(command))
                if command.0 == "project.reimport" =>
            {
                self.reimport_project_requested = true;
                return;
            }
            EventResult::Action(WidgetAction::Command(command)) if command.0 == "project.run" => {
                self.run_requested = true;
                self.status_message = "Executando a aplicação".to_owned();
                return;
            }
            EventResult::Action(WidgetAction::Command(command)) if command.0 == "project.stop" => {
                self.stop_requested = true;
                return;
            }
            EventResult::Action(WidgetAction::Command(command)) if command.0 == "debug.connect" => {
                self.settings_page = SettingsPage::Debug;
                self.open_settings_requested = true;
                return;
            }
            EventResult::Action(WidgetAction::Command(command))
                if command.0.starts_with("debug.") =>
            {
                if let Some(request) = debug_request_for(&command.0) {
                    self.debug_requests.push(request);
                }
                return;
            }
            EventResult::Handled | EventResult::Action(_) => return,
            EventResult::Ignored => {}
        }
        if point.y < TITLE_HEIGHT {
            return;
        }
        let sidebar = self.sidebar_width(size);
        let editor_x = ACTIVITY_WIDTH + sidebar;
        let geometry = self.geometry(size);
        let toggle = Rect::new(size.width - 30.0, geometry.editor_bottom + 4.0, 22.0, 22.0);
        if toggle.contains(point) {
            if self.terminal_minimized {
                self.terminal_minimized = false;
                self.terminal_height = self.terminal_last_height;
            } else {
                self.terminal_last_height = self.terminal_height;
                self.terminal_minimized = true;
            }
            return;
        }
        if self.scrollbar_pointer_down(point, size) {
            return;
        }
        if self.splitter_pointer_down(point, size) {
            return;
        }
        if point.y >= TITLE_HEIGHT && point.y < TITLE_HEIGHT + TAB_HEIGHT && point.x >= editor_x {
            // Quem decide entre ativar e fechar é o componente; a IDE traduz o
            // comando recebido para o documento correspondente.
            self.pointer = point;
            let mut tabs = self.editor_tabs();
            tabs.layout(&self.layout_context(), self.editor_tabs_rect(size));
            match tab_command(&mut tabs, point) {
                Some(TabCommand::Select(id)) => {
                    let _ = self.editor.activate(DocumentId(id));
                    self.cursor_offset = 0;
                    self.focus = ShellFocus::Editor;
                }
                Some(TabCommand::Close(id)) => {
                    let id = DocumentId(id);
                    if self.editor.close(id).is_ok() {
                        self.syntax_snapshots.remove(&id);
                        self.cursor_offset = self.active_text().map_or(0, str::len);
                        self.status_message = "Tab closed".to_owned();
                    }
                }
                None => {}
            }
            return;
        }
        if point.x >= ACTIVITY_WIDTH && point.x < editor_x && point.y >= EXPLORER_TOP {
            // Qual nó foi clicado é a árvore quem sabe: o recuo, o deslocamento
            // horizontal e a virtualização são dela.
            let mut tree = self.explorer_tree_for(size);
            tree.event(
                &mut EventContext::default(),
                &UiEvent::PointerDown(primary_pointer(point)),
            );
            let entry = tree.selected().and_then(|id| self.explorer_path_for(id));
            if let Some((path, is_directory)) = entry {
                self.focus = ShellFocus::Explorer;
                self.explorer_tree.set_selected(Some(explorer_id(&path)));
                if is_directory {
                    if !self.expanded.remove(&path) {
                        self.expanded.insert(path);
                    }
                    self.sync_explorer_tree();
                } else if let Err(error) = self.open_file(&path) {
                    self.status_message = error;
                }
            }
            return;
        }
        if self.debug.attached
            && point.x >= editor_x + geometry.editor_width
            && point.y >= geometry.content_top
            && point.y < geometry.editor_bottom
        {
            self.debug_panel_pointer_down(point, size);
            return;
        }
        // A calha é a área de pontos de parada e não posiciona o cursor. Qual
        // linha foi clicada é o editor quem responde.
        if let Some(line) = self.gutter_line_at(point, size) {
            if let Some(path) = self.editor.active().map(|document| document.path.clone()) {
                self.toggle_breakpoint(&path, line as u32);
            }
            return;
        }
        if point.x >= editor_x
            && point.x < editor_x + geometry.editor_width
            && point.y >= geometry.content_top
            && point.y < geometry.editor_bottom
        {
            self.focus = ShellFocus::Editor;
            self.cursor_offset = self.offset_at_point(point, editor_x, geometry.content_top);
            if control
                && let (Some(document_id), Some(token)) = (
                    self.editor.active_id(),
                    self.active_text()
                        .and_then(|text| token_at(text, self.cursor_offset)),
                )
            {
                self.status_message = format!("Go to definition: {token}");
                self.pending_navigation = Some(NavigationRequest {
                    document_id,
                    byte_offset: self.cursor_offset,
                    token,
                });
            }
        } else if point.x >= editor_x && point.y >= geometry.editor_bottom {
            self.focus = ShellFocus::Terminal;
            if point.y < geometry.editor_bottom + TERMINAL_TAB_HEIGHT {
                let mut tabs = self.terminal_tabs();
                tabs.layout(&self.layout_context(), self.terminal_tabs_rect(size));
                if let Some(TabCommand::Select(index)) = tab_command(&mut tabs, point) {
                    self.active_terminal = index as usize;
                    self.status_message = format!(
                        "Terminal: {}",
                        self.active_terminal().selected_profile().kind.label()
                    );
                }
            } else if point.y >= geometry.editor_bottom + 60.0 {
                let position = self.terminal_position_at(point, size);
                self.terminal_selection = Some(TerminalSelection {
                    anchor: position,
                    focus: position,
                });
                self.terminal_selecting = true;
            }
        }
    }

    pub fn open_location(
        &mut self,
        path: &Path,
        line: usize,
        column: usize,
    ) -> Result<DocumentId, String> {
        let id = self.open_file(path)?;
        let text = self.active_text().unwrap_or_default();
        self.cursor_offset = offset_for_line_column(text, line, column);
        // Sem revelar a linha, a navegação move o cursor para fora da área
        // visível e parece que nada aconteceu.
        self.pending_reveal = Some(line);
        self.navigated = Some((line, self.cursor_offset));
        self.focus = ShellFocus::Editor;
        self.status_message = format!("Definition: {}:{}:{}", path.display(), line + 1, column + 1);
        Ok(id)
    }

    pub fn pointer_move(&mut self, point: Point, size: Size) -> bool {
        self.pointer = point;
        if self.settings_modal.is_open() {
            return false;
        }
        if let Some(target) = self.scrollbar_drag {
            self.sync_scrollbar(target, size);
            self.scrollbar_mut(target).event(
                &mut EventContext::default(),
                &UiEvent::PointerMove(primary_pointer(point)),
            );
            self.apply_scrollbar(target);
            return true;
        }
        if self.terminal_selecting {
            let position = self.terminal_position_at(point, size);
            if let Some(selection) = self.terminal_selection.as_mut() {
                selection.focus = position;
            }
            return true;
        }
        // O movimento vai sempre aos divisores: mesmo parados, eles precisam
        // saber que o ponteiro passou por cima para se destacar.
        let dragging = self.sidebar_resizing() || self.terminal_resizing();
        self.sync_splitters(size);
        let event = UiEvent::PointerMove(primary_pointer(point));
        self.sidebar_splitter
            .event(&mut EventContext::default(), &event);
        self.terminal_splitter
            .event(&mut EventContext::default(), &event);
        if dragging {
            self.apply_splitters(size);
            return true;
        }
        // Parado, o retorno diz se o ponteiro está sobre o divisor do terminal,
        // para a janela trocar o cursor.
        !self.terminal_minimized && self.terminal_splitter.hit_area().contains(point)
    }

    pub fn pointer_up(&mut self) {
        let event = UiEvent::PointerUp(primary_pointer(Point::ZERO));
        self.sidebar_splitter
            .event(&mut EventContext::default(), &event);
        self.terminal_splitter
            .event(&mut EventContext::default(), &event);
        // A barra também precisa saber que o gesto acabou: ela é quem guarda o
        // ponto da pegada.
        if let Some(target) = self.scrollbar_drag.take() {
            self.scrollbar_mut(target).event(
                &mut EventContext::default(),
                &UiEvent::PointerUp(primary_pointer(Point::ZERO)),
            );
        }
        self.terminal_selecting = false;
    }

    pub fn scroll(&mut self, point: Point, delta_lines: isize, size: Size) {
        if self.settings_modal.is_open() {
            return;
        }
        let geo = self.geometry(size);
        if point.x >= ACTIVITY_WIDTH
            && point.x < ACTIVITY_WIDTH + self.sidebar_width(size)
            && point.y >= EXPLORER_TOP - EXPLORER_ROW_HEIGHT
            && point.y < geo.content_bottom
        {
            let max = self
                .visible_entries()
                .len()
                .saturating_sub(self.explorer_visible_lines(size));
            self.explorer_scroll_line = self
                .explorer_scroll_line
                .saturating_add_signed(delta_lines)
                .min(max);
        } else if point.y >= geo.content_top && point.y < geo.editor_bottom {
            let total = self.active_text().map_or(0, |text| text.lines().count());
            let visible = (geo.editor_height / EDITOR_LINE_HEIGHT).floor().max(1.0) as usize;
            let max = total.saturating_sub(visible);
            self.editor_scroll_line = self
                .editor_scroll_line
                .saturating_add_signed(delta_lines)
                .min(max);
        } else if point.y >= geo.editor_bottom && point.y < geo.content_bottom {
            let visible = ((geo.terminal_height - 62.0) / EDITOR_LINE_HEIGHT)
                .floor()
                .max(1.0) as usize;
            let active = self.active_terminal;
            let max = self.terminals[active]
                .session
                .line_count()
                .saturating_sub(visible);
            self.terminals[active].scroll_line = self.terminals[active]
                .scroll_line
                .saturating_add_signed(delta_lines)
                .min(max);
            self.terminals[active].follow_output = self.terminals[active].scroll_line >= max;
        }
    }

    fn editor_visible_lines(&self, size: Size) -> usize {
        (self.geometry(size).editor_height / EDITOR_LINE_HEIGHT)
            .floor()
            .max(1.0) as usize
    }

    fn terminal_visible_lines(&self, size: Size) -> usize {
        ((self.geometry(size).terminal_height - 62.0) / EDITOR_LINE_HEIGHT)
            .floor()
            .max(1.0) as usize
    }

    fn explorer_visible_lines(&self, size: Size) -> usize {
        let geo = self.geometry(size);
        ((geo.content_bottom - 12.0 - EXPLORER_TOP) / EXPLORER_ROW_HEIGHT)
            .floor()
            .max(1.0) as usize
    }

    fn editor_scrollbar_rect(&self, size: Size) -> Rect {
        let geo = self.geometry(size);
        let editor_x = ACTIVITY_WIDTH + self.sidebar_width(size);
        Rect::new(
            editor_x + geo.editor_width - 10.0,
            geo.content_top,
            10.0,
            geo.editor_height,
        )
    }

    fn terminal_scrollbar_rect(&self, size: Size) -> Rect {
        let geo = self.geometry(size);
        let editor_x = ACTIVITY_WIDTH + self.sidebar_width(size);
        Rect::new(
            editor_x + geo.editor_width - 10.0,
            geo.editor_bottom + 60.0,
            10.0,
            (geo.terminal_height - 60.0).max(0.0),
        )
    }

    fn explorer_horizontal_scrollbar_rect(&self, size: Size) -> Rect {
        let geo = self.geometry(size);
        Rect::new(
            ACTIVITY_WIDTH,
            geo.content_bottom - 12.0,
            self.sidebar_width(size),
            12.0,
        )
    }

    fn explorer_vertical_scrollbar_rect(&self, size: Size) -> Rect {
        let geo = self.geometry(size);
        Rect::new(
            ACTIVITY_WIDTH + self.sidebar_width(size) - 16.0,
            EXPLORER_TOP - EXPLORER_ROW_HEIGHT,
            10.0,
            (geo.content_bottom - 12.0 - EXPLORER_TOP + EXPLORER_ROW_HEIGHT).max(0.0),
        )
    }

    /// Largura do conteúdo da árvore, medida pela árvore já posicionada.
    ///
    /// A instância persistente nunca passa por `layout` — quem é posicionada é a
    /// cópia usada para desenhar —, e é ela quem conhece a medida.
    fn explorer_content_width(&self, size: Size) -> f32 {
        self.explorer_tree_for(size).content_size().width
    }


    fn terminal_position_at(&self, point: Point, size: Size) -> TextPosition {
        let geo = self.geometry(size);
        let editor_x = ACTIVITY_WIDTH + self.sidebar_width(size);
        let visible = self.terminal_visible_lines(size);
        let active = &self.terminals[self.active_terminal];
        let max = active.session.line_count().saturating_sub(visible);
        let first = active.scroll_line.min(max);
        let row = ((point.y - (geo.editor_bottom + 68.0)) / EDITOR_LINE_HEIGHT)
            .floor()
            .max(0.0) as usize;
        let line = (first + row).min(active.session.line_count().saturating_sub(1));
        let line_length = active
            .session
            .lines()
            .nth(line)
            .map_or(0, |value| value.text.chars().count());
        let column = ((point.x - (editor_x + 14.0)) / TERMINAL_CHAR_WIDTH)
            .round()
            .max(0.0) as usize;
        TextPosition {
            line,
            column: column.min(line_length),
        }
    }

    pub fn selected_terminal_text(&self) -> String {
        let Some(selection) = self.terminal_selection else {
            return String::new();
        };
        let (start, end) = ordered_selection(selection);
        self.active_terminal()
            .lines()
            .enumerate()
            .filter_map(|(line_index, line)| {
                if line_index < start.line || line_index > end.line {
                    return None;
                }
                let from = if line_index == start.line {
                    start.column
                } else {
                    0
                };
                let to = if line_index == end.line {
                    end.column
                } else {
                    line.text.chars().count()
                };
                Some(
                    line.text
                        .chars()
                        .skip(from)
                        .take(to.saturating_sub(from))
                        .collect::<String>(),
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn text_input(&mut self, text: &str) {
        if self.settings_modal.is_open() {
            let _ = self.settings_text_input(text);
            return;
        }
        match self.focus {
            ShellFocus::Editor => self.edit_active(text),
            ShellFocus::Search => self.search_query.push_str(text),
            ShellFocus::Terminal => self.active_terminal_mut().input_mut().push_str(text),
            _ => {}
        }
    }

    pub fn key_down(&mut self, key: &str) {
        if self.settings_modal.is_open() {
            if self.settings_key_down(key) {
                return;
            }
            let event = UiEvent::KeyDown(KeyEvent {
                logical_key: key.to_owned(),
                repeat: false,
                modifiers: Modifiers::default(),
            });
            let mut context = EventContext::default();
            let result = self.jdk_combo.event(&mut context, &event);
            if !self.handle_settings_action(result) {
                let _ = self.settings_modal.event(&mut context, &event);
                if !self.settings_modal.is_open() {
                    self.settings_dialog = None;
                }
            }
            return;
        }
        if !self.completion_items.is_empty() {
            match key.to_ascii_lowercase().as_str() {
                "arrowdown" => {
                    self.completion_selected =
                        (self.completion_selected + 1).min(self.completion_items.len() - 1);
                    return;
                }
                "arrowup" => {
                    self.completion_selected = self.completion_selected.saturating_sub(1);
                    return;
                }
                "enter" => {
                    self.accept_completion();
                    return;
                }
                _ => {}
            }
        }
        if key.eq_ignore_ascii_case("backspace") {
            match self.focus {
                ShellFocus::Editor => self.backspace(),
                ShellFocus::Search => {
                    self.search_query.pop();
                }
                ShellFocus::Terminal => {
                    self.active_terminal_mut().input_mut().pop();
                }
                _ => {}
            }
        } else if self.focus == ShellFocus::Terminal && key.eq_ignore_ascii_case("enter") {
            match self.active_terminal_mut().submit() {
                Ok(()) => self.status_message = "Command sent to terminal".to_owned(),
                Err(error) => self.status_message = error.to_string(),
            }
            let active = self.active_terminal;
            self.terminals[active].scroll_line = self.terminals[active]
                .session
                .line_count()
                .saturating_sub(1);
        } else if self.focus == ShellFocus::Editor {
            match key.to_ascii_lowercase().as_str() {
                "enter" => self.edit_active("\n"),
                "arrowleft" => {
                    self.cursor_offset = previous_boundary(
                        self.active_text().unwrap_or_default(),
                        self.cursor_offset,
                    )
                }
                "arrowright" => {
                    self.cursor_offset =
                        next_boundary(self.active_text().unwrap_or_default(), self.cursor_offset)
                }
                _ => {}
            }
        }
    }

    fn edit_active(&mut self, text: &str) {
        self.completion_items.clear();
        if let Some(document) = self.editor.active_mut() {
            let cursor = self.cursor_offset.min(document.buffer.text().len());
            if document.buffer.replace(cursor..cursor, text).is_ok() {
                self.cursor_offset = cursor + text.len();
                self.status_message = "Modified".to_owned();
            }
        }
    }

    fn backspace(&mut self) {
        self.completion_items.clear();
        if let Some(document) = self.editor.active_mut() {
            let previous = previous_boundary(document.buffer.text(), self.cursor_offset);
            if previous < self.cursor_offset
                && document
                    .buffer
                    .replace(previous..self.cursor_offset, "")
                    .is_ok()
            {
                self.cursor_offset = previous;
                self.status_message = "Modified".to_owned();
            }
        }
    }

    fn accept_completion(&mut self) {
        let Some(item) = self.completion_items.get(self.completion_selected).cloned() else {
            return;
        };
        if let Some(document) = self.editor.active_mut() {
            let cursor = self.cursor_offset.min(document.buffer.text().len());
            let prefix = identifier_prefix(document.buffer.text(), cursor);
            let start = cursor.saturating_sub(prefix.len());
            if document.buffer.replace(start..cursor, &item.label).is_ok() {
                self.cursor_offset = start + item.label.len();
                self.status_message = format!("Completed {}", item.label);
            }
        }
        self.completion_items.clear();
    }

    fn offset_at_point(&self, point: Point, editor_x: f32, editor_top: f32) -> usize {
        let Some(text) = self.active_text() else {
            return 0;
        };
        let line_index = self.editor_scroll_line
            + ((point.y - editor_top) / EDITOR_LINE_HEIGHT).floor().max(0.0) as usize;
        let column = ((point.x - editor_x - EDITOR_GUTTER) / EDITOR_CHAR_WIDTH)
            .round()
            .max(0.0) as usize;
        let mut offset = 0;
        for (index, line) in text.split('\n').enumerate() {
            if index == line_index {
                return offset + byte_at_column(line, column);
            }
            offset += line.len() + 1;
        }
        text.len()
    }

    fn visible_entries(&self) -> Vec<(usize, &FileNode)> {
        fn visit<'a>(
            node: &'a FileNode,
            depth: usize,
            expanded: &HashSet<PathBuf>,
            output: &mut Vec<(usize, &'a FileNode)>,
        ) {
            if depth > 0 {
                output.push((depth - 1, node));
            }
            if node.is_directory && expanded.contains(&node.path) {
                for child in &node.children {
                    visit(child, depth + 1, expanded, output);
                }
            }
        }
        let mut output = Vec::new();
        visit(&self.workspace, 0, &self.expanded, &mut output);
        output
    }

    /// Desenha o quadro.
    ///
    /// Pintar exige acesso mutável porque o shell mantém widgets com estado
    /// próprio — o editor guarda uma cópia do documento ativo, reconstruída
    /// quando o texto muda. Deixar essa reconciliação para os manipuladores de evento faria
    /// cada esquecimento virar um quadro desatualizado.
    pub fn paint(&mut self, size: Size) -> Vec<PaintCommand> {
        let sidebar = self.sidebar_width(size);
        let editor_x = ACTIVITY_WIDTH + sidebar;
        let geo = self.geometry(size);
        let colors = self.theme.colors;
        let mut commands = vec![
            fill(
                Rect::new(0.0, 0.0, size.width, size.height),
                colors.background,
            ),
            fill(
                Rect::new(0.0, 0.0, size.width, TITLE_HEIGHT),
                colors.elevated,
            ),
            fill(
                Rect::new(
                    0.0,
                    TITLE_HEIGHT,
                    ACTIVITY_WIDTH,
                    geo.content_bottom - TITLE_HEIGHT,
                ),
                colors.elevated,
            ),
            fill(
                Rect::new(
                    ACTIVITY_WIDTH,
                    TITLE_HEIGHT,
                    sidebar,
                    geo.content_bottom - TITLE_HEIGHT,
                ),
                colors.surface,
            ),
            fill(
                Rect::new(editor_x, TITLE_HEIGHT, geo.editor_width, TAB_HEIGHT),
                colors.elevated,
            ),
            fill(
                Rect::new(
                    editor_x,
                    geo.editor_bottom,
                    geo.editor_width,
                    geo.terminal_height,
                ),
                colors.surface,
            ),
            stroke(
                Rect::new(
                    editor_x,
                    geo.editor_bottom,
                    geo.editor_width,
                    geo.terminal_height,
                ),
                colors.border,
            ),
            label("ER IDE", Point::new(14.0, 9.0), colors.text, 16.0),
            label(
                "EXPLORER",
                Point::new(ACTIVITY_WIDTH + 14.0, TITLE_HEIGHT + 14.0),
                colors.muted_text,
                12.0,
            ),
            label(
                &self.workspace_name,
                Point::new(ACTIVITY_WIDTH + 14.0, TITLE_HEIGHT + 42.0),
                colors.text,
                14.0,
            ),
            label(
                "⌕",
                Point::new(15.0, TITLE_HEIGHT + 18.0),
                colors.text,
                22.0,
            ),
            label(
                "▣",
                Point::new(15.0, TITLE_HEIGHT + 62.0),
                colors.text,
                20.0,
            ),
        ];
        commands.push(fill(
            Rect::new(size.width - 30.0, geo.editor_bottom + 4.0, 22.0, 22.0),
            colors.elevated,
        ));
        commands.push(stroke(
            Rect::new(size.width - 30.0, geo.editor_bottom + 4.0, 22.0, 22.0),
            colors.border,
        ));
        commands.push(label(
            if self.terminal_minimized { "^" } else { "v" },
            Point::new(size.width - 24.0, geo.editor_bottom + 7.0),
            colors.text,
            14.0,
        ));
        commands.push(PaintCommand::PushClip(Rect::new(
            ACTIVITY_WIDTH,
            EXPLORER_TOP - EXPLORER_ROW_HEIGHT,
            self.sidebar_width(size),
            (geo.content_bottom - EXPLORER_TOP + EXPLORER_ROW_HEIGHT - 12.0).max(0.0),
        )));
        // A árvore é um componente: recuo, marcador de expansão, virtualização,
        // seleção e deslocamento horizontal pertencem a ela.
        let tree = self.explorer_tree_for(size);
        let mut tree_paint = self.paint_context();
        tree.paint(&mut tree_paint);
        commands.extend(tree_paint.into_commands());
        commands.push(PaintCommand::PopClip);
        commands.extend(self.paint_scrollbar(ScrollTarget::ExplorerHorizontal, size));
        commands.extend(self.paint_scrollbar(ScrollTarget::ExplorerVertical, size));
        // Os divisores se desenham: a linha é a mesma borda de antes, mas agora
        // ela se destaca sob o ponteiro, que é o que revela que dá para arrastar.
        let mut splitters = self.paint_context();
        self.sidebar_splitter_for(size).paint(&mut splitters);
        if !self.terminal_minimized {
            self.terminal_splitter_for(size).paint(&mut splitters);
        }
        commands.extend(splitters.into_commands());
        // As abas são um componente: largura, faixa da aba ativa, corte do
        // título, ponto de alterado e botão de fechar pertencem a ele.
        let mut editor_tabs = self.editor_tabs();
        editor_tabs.layout(&self.layout_context(), self.editor_tabs_rect(size));
        let mut tabs_paint = self.paint_context();
        editor_tabs.paint(&mut tabs_paint);
        commands.extend(tabs_paint.into_commands());
        commands.push(PaintCommand::PushClip(Rect::new(
            editor_x,
            geo.content_top,
            geo.editor_width,
            geo.editor_height,
        )));
        if self.editor.active().is_some() {
            // O editor da biblioteca desenha calha, números, realce, marcas de
            // ponto de parada, linha em execução e cursor. A IDE entrega o
            // texto, o realce e as decorações.
            self.refresh_editor_view(size);
            if let Some((_, _, editor)) = self.editor_view.as_ref() {
                let mut editor_paint = self.paint_context();
                editor.paint(&mut editor_paint);
                commands.extend(editor_paint.into_commands());
            }
            commands.extend(self.paint_scrollbar(ScrollTarget::Editor, size));
        } else {
            commands.push(label(
                "Select a file in Explorer",
                Point::new(editor_x + 55.0, geo.content_top + 30.0),
                colors.muted_text,
                16.0,
            ));
        }
        commands.push(PaintCommand::PopClip);
        if self.debug.attached {
            commands.extend(self.paint_debug_panel(size, colors));
        }
        if !self.terminal_minimized {
            let mut terminal_tabs = self.terminal_tabs();
            terminal_tabs.layout(&self.layout_context(), self.terminal_tabs_rect(size));
            let mut terminal_tabs_paint = self.paint_context();
            terminal_tabs.paint(&mut terminal_tabs_paint);
            commands.extend(terminal_tabs_paint.into_commands());
            commands.push(fill(
                Rect::new(editor_x, geo.editor_bottom + 30.0, geo.editor_width, 30.0),
                colors.background,
            ));
            let active_terminal = &self.terminals[self.active_terminal];
            commands.push(label(
                &format!(
                    "{} {}",
                    active_terminal.session.prompt(),
                    active_terminal.session.input()
                ),
                Point::new(editor_x + 14.0, geo.editor_bottom + 38.0),
                colors.text,
                14.0,
            ));
            let terminal_visible = ((geo.terminal_height - 62.0) / EDITOR_LINE_HEIGHT)
                .floor()
                .max(1.0) as usize;
            let terminal_offset = active_terminal.scroll_line.min(
                active_terminal
                    .session
                    .line_count()
                    .saturating_sub(terminal_visible),
            );
            for (index, line) in active_terminal
                .session
                .lines()
                .skip(terminal_offset)
                .take(terminal_visible)
                .enumerate()
            {
                let absolute_line = terminal_offset + index;
                if let Some((start, end)) =
                    selection_columns(self.terminal_selection, absolute_line, &line.text)
                {
                    commands.push(fill(
                        Rect::new(
                            editor_x + 14.0 + start as f32 * TERMINAL_CHAR_WIDTH,
                            geo.editor_bottom + 66.0 + index as f32 * EDITOR_LINE_HEIGHT,
                            (end.saturating_sub(start) as f32 * TERMINAL_CHAR_WIDTH).max(2.0),
                            EDITOR_LINE_HEIGHT,
                        ),
                        colors.selection,
                    ));
                }
                commands.push(label(
                    &line.text,
                    Point::new(
                        editor_x + 14.0,
                        geo.editor_bottom + 68.0 + index as f32 * EDITOR_LINE_HEIGHT,
                    ),
                    if line.is_error {
                        colors.danger
                    } else {
                        colors.muted_text
                    },
                    14.0,
                ));
            }
            commands.extend(self.paint_scrollbar(ScrollTarget::Terminal, size));
        } else {
            commands.push(label(
                "Terminal",
                Point::new(editor_x + 10.0, geo.editor_bottom + 8.0),
                colors.text,
                13.0,
            ));
        }
        if !self.completion_items.is_empty()
            && self.focus == ShellFocus::Editor
            && let Some(text) = self.active_text()
        {
            let (line, column) = line_column(text, self.cursor_offset);
            let popup_x = (editor_x + EDITOR_GUTTER + column as f32 * EDITOR_CHAR_WIDTH)
                .min(size.width - 270.0)
                .max(editor_x + EDITOR_GUTTER);
            let popup_y = (geo.content_top
                + 36.0
                + line.saturating_sub(self.editor_scroll_line) as f32 * EDITOR_LINE_HEIGHT)
                .min(geo.editor_bottom - 190.0);
            let visible = self.completion_items.len().min(8);
            let popup = Rect::new(popup_x, popup_y, 260.0, visible as f32 * 24.0 + 8.0);
            commands.push(fill(popup, colors.elevated));
            commands.push(stroke(popup, colors.border));
            for (index, item) in self.completion_items.iter().take(visible).enumerate() {
                if index == self.completion_selected {
                    commands.push(fill(
                        Rect::new(
                            popup_x + 2.0,
                            popup_y + 4.0 + index as f32 * 24.0,
                            256.0,
                            23.0,
                        ),
                        colors.surface,
                    ));
                }
                commands.push(label(
                    &item.label,
                    Point::new(popup_x + 10.0, popup_y + 8.0 + index as f32 * 24.0),
                    if index == self.completion_selected {
                        colors.accent
                    } else {
                        colors.text
                    },
                    14.0,
                ));
            }
        }
        if self.focus == ShellFocus::Search {
            let width = 380.0_f32.min((geo.editor_width - 24.0).max(100.0));
            commands.push(fill(
                Rect::new(
                    size.width - width - 12.0,
                    geo.content_top + 12.0,
                    width,
                    42.0,
                ),
                colors.elevated,
            ));
            commands.push(stroke(
                Rect::new(
                    size.width - width - 12.0,
                    geo.content_top + 12.0,
                    width,
                    42.0,
                ),
                colors.accent,
            ));
            commands.push(label(
                &format!("Search: {}", self.search_query),
                Point::new(size.width - width, geo.content_top + 24.0),
                colors.text,
                14.0,
            ));
        }
        let position = self
            .active_text()
            .map(|text| line_column(text, self.cursor_offset))
            .unwrap_or((0, 0));
        // A barra de estado é da biblioteca: superfície, borda, alinhamento e
        // recorte vêm de lá. A IDE só diz o que cada segmento informa — a
        // mensagem da última ação à esquerda, e à direita o que o usuário
        // procura sempre no mesmo lugar.
        let mut status_bar = StatusBar::new(STATUS_BAR_ID).with_leading([&self.status_message]);
        let mut trailing = vec![
            "UTF-8".to_owned(),
            format!("Ln {}, Col {}", position.0 + 1, position.1 + 1),
        ];
        if let Some(summary) = self.project_summary.as_deref() {
            trailing.push(summary.to_owned());
        }
        status_bar.set_trailing(trailing);
        status_bar.layout(
            &self.layout_context(),
            Rect::new(
                0.0,
                geo.content_bottom,
                size.width,
                size.height - geo.content_bottom,
            ),
        );
        let mut status_paint = self.paint_context();
        status_bar.paint(&mut status_paint);
        commands.extend(status_paint.into_commands());
        let mut menu_bar = self.menu_bar.clone();
        menu_bar.layout(
            &self.layout_context(),
            Rect::new(82.0, 0.0, (size.width - 82.0).max(0.0), TITLE_HEIGHT),
        );
        let mut menu_paint = self.paint_context();
        menu_bar.paint(&mut menu_paint);
        commands.extend(menu_paint.into_commands());
        // Os botões de ação são widgets da biblioteca: a IDE define papel e
        // posição, e o desenho do ícone e o tema vêm de lá.
        let rects = action_button_rects(size);
        let mut stop = self.stop_button.clone();
        stop.set_tint(if self.application_running() {
            IconTint::Danger
        } else {
            IconTint::Muted
        });
        stop.set_disabled(!self.application_running());
        let mut debug = self.debug_button.clone();
        debug.set_tint(if self.debug.attached {
            IconTint::Accent
        } else {
            IconTint::Muted
        });
        let mut run = self.run_button.clone();
        let mut actions = self.paint_context();
        for (button, rect) in [
            (&mut stop, rects[0]),
            (&mut run, rects[1]),
            (&mut debug, rects[2]),
        ] {
            button.layout(&self.layout_context(), rect);
            button.paint(&mut actions);
        }
        commands.extend(actions.into_commands());
        if self.settings_modal.is_open() {
            let mut modal = self.settings_modal.clone();
            modal.layout(&self.layout_context(), Rect::new(0.0, 0.0, size.width, size.height));
            let geometry = settings_dialog_geometry(modal.panel_bounds());
            let mut modal_paint = self.paint_context();
            modal.paint(&mut modal_paint);
            commands.extend(modal_paint.into_commands());
            commands.push(fill(geometry.sidebar, colors.surface));
            for (option, title, active) in [
                (
                    geometry.compiler_option,
                    "Compilador e VM",
                    self.settings_page == SettingsPage::Compiler,
                ),
                (
                    geometry.debug_option,
                    "Depuração",
                    self.settings_page == SettingsPage::Debug,
                ),
            ] {
                if active {
                    commands.push(fill(option, colors.elevated));
                    commands.push(fill(
                        Rect::new(option.origin.x, option.origin.y, 3.0, option.size.height),
                        colors.accent,
                    ));
                }
                commands.push(label(
                    title,
                    Point::new(option.origin.x + 14.0, option.origin.y + 11.0),
                    if active {
                        colors.text
                    } else {
                        colors.muted_text
                    },
                    14.0,
                ));
            }
            let mut component_paint = self.paint_context();
            match self.settings_page {
                SettingsPage::Compiler => {
                    commands.push(label(
                        "Compilador e VM",
                        Point::new(geometry.combo.origin.x, geometry.combo.origin.y - 34.0),
                        colors.text,
                        17.0,
                    ));
                    commands.push(label(
                        "JDK",
                        Point::new(geometry.combo.origin.x, geometry.combo.origin.y - 16.0),
                        colors.muted_text,
                        13.0,
                    ));
                    let mut combo = self.jdk_combo.clone();
                    combo.layout(&self.layout_context(), geometry.combo);
                    combo.paint(&mut component_paint);
                    let mut browse = self.jdk_browse_button.clone();
                    browse.layout(&self.layout_context(), geometry.browse);
                    browse.paint(&mut component_paint);
                }
                SettingsPage::Debug => {
                    commands.extend(self.paint_debug_settings(&geometry, colors));
                    let mut host = self.debug_host.clone();
                    host.layout(&self.layout_context(), geometry.debug_host);
                    host.paint(&mut component_paint);
                    let mut port = self.debug_port.clone();
                    port.layout(&self.layout_context(), geometry.debug_port);
                    port.paint(&mut component_paint);
                    let mut attach = self.debug_attach_button.clone();
                    attach.layout(&self.layout_context(), geometry.debug_attach);
                    attach.paint(&mut component_paint);
                }
            }
            let mut close = self.settings_close_button.clone();
            close.layout(&self.layout_context(), geometry.close);
            close.paint(&mut component_paint);
            commands.extend(component_paint.into_commands());
            if let Some(message) = self
                .settings_dialog
                .as_ref()
                .and_then(|dialog| dialog.message.as_ref())
            {
                commands.push(label(
                    message,
                    Point::new(geometry.combo.origin.x, geometry.combo.origin.y + 54.0),
                    colors.danger,
                    13.0,
                ));
            }
        }
        commands
    }

    fn settings_dialog_pointer_down(&mut self, point: Point, size: Size) {
        if !self.settings_modal.is_open() {
            return;
        }
        self.settings_modal
            .layout(&self.layout_context(), Rect::new(0.0, 0.0, size.width, size.height));
        let geometry = settings_dialog_geometry(self.settings_modal.panel_bounds());
        if geometry.compiler_option.contains(point) {
            self.settings_page = SettingsPage::Compiler;
            self.settings_focus = None;
            return;
        }
        if geometry.debug_option.contains(point) {
            self.settings_page = SettingsPage::Debug;
            self.settings_focus = None;
            return;
        }
        if self.settings_page == SettingsPage::Debug {
            self.debug_page_pointer_down(point, &geometry);
            return;
        }
        self.jdk_combo.layout(&self.layout_context(), geometry.combo);
        self.jdk_browse_button
            .layout(&self.layout_context(), geometry.browse);
        self.settings_close_button
            .layout(&self.layout_context(), geometry.close);
        let event = UiEvent::PointerDown(primary_pointer(point));
        let mut context = EventContext::default();
        let combo_result = self.jdk_combo.event(&mut context, &event);
        let combo_consumed = !matches!(combo_result, EventResult::Ignored);
        if self.handle_settings_action(combo_result) || combo_consumed {
            return;
        }
        let browse_result = click_widget(&mut self.jdk_browse_button, point);
        if self.handle_settings_action(browse_result) {
            return;
        }
        let close_result = click_widget(&mut self.settings_close_button, point);
        if self.handle_settings_action(close_result) {
            return;
        }
        let _ = self.settings_modal.event(&mut context, &event);
    }

    fn paint_debug_settings(
        &self,
        geometry: &SettingsDialogGeometry,
        colors: ColorTokens,
    ) -> Vec<PaintCommand> {
        let origin = geometry.debug_host.origin;
        let mut commands = vec![
            label(
                "Depuração",
                Point::new(origin.x, origin.y - 34.0),
                colors.text,
                17.0,
            ),
            label(
                "Host e porta de depuração do processo em execução",
                Point::new(origin.x, origin.y - 16.0),
                colors.muted_text,
                13.0,
            ),
            label(
                "Inicie o servidor com -agentlib:jdwp=transport=dt_socket,server=y,suspend=n,address=*:8000",
                Point::new(origin.x, geometry.debug_attach.origin.y + 48.0),
                colors.muted_text,
                12.0,
            ),
            label(
                "Vale para qualquer processo Java: servidor, container ou ferramenta.",
                Point::new(origin.x, geometry.debug_attach.origin.y + 66.0),
                colors.muted_text,
                12.0,
            ),
        ];
        for (rect, id) in [
            (geometry.debug_host, DEBUG_HOST_ID),
            (geometry.debug_port, DEBUG_PORT_ID),
        ] {
            if self.settings_focus == Some(id) {
                commands.push(stroke(rect, colors.accent));
            }
        }
        commands
    }

    fn debug_page_pointer_down(&mut self, point: Point, geometry: &SettingsDialogGeometry) {
        if geometry.debug_host.contains(point) {
            self.settings_focus = Some(DEBUG_HOST_ID);
            return;
        }
        if geometry.debug_port.contains(point) {
            self.settings_focus = Some(DEBUG_PORT_ID);
            return;
        }
        if geometry.debug_attach.contains(point) {
            // O foco é preservado para o usuário corrigir um valor recusado.
            self.attach_debug_target();
            return;
        }
        self.settings_focus = None;
        self.settings_close_button
            .layout(&self.layout_context(), geometry.close);
        let close_result = click_widget(&mut self.settings_close_button, point);
        let _ = self.handle_settings_action(close_result);
    }

    /// Botões de ação da barra, na ordem em que aparecem.
    #[must_use]
    pub fn action_buttons(&self) -> [&Button; 3] {
        [&self.stop_button, &self.run_button, &self.debug_button]
    }

    /// Roteia o clique para os botões de ação, que são widgets da biblioteca.
    fn action_buttons_pointer_down(&mut self, point: Point, size: Size) -> bool {
        let rects = action_button_rects(size);
        self.stop_button.layout(&self.layout_context(), rects[0]);
        self.run_button.layout(&self.layout_context(), rects[1]);
        self.debug_button.layout(&self.layout_context(), rects[2]);
        let commands = [
            click_widget(&mut self.stop_button, point),
            click_widget(&mut self.run_button, point),
            click_widget(&mut self.debug_button, point),
        ];
        for result in commands {
            if let EventResult::Action(WidgetAction::Command(command)) = result {
                match command.0.as_str() {
                    "project.stop" => self.stop_requested = true,
                    "project.run" => {
                        self.run_requested = true;
                        self.status_message = "Executando a aplicação".to_owned();
                    }
                    "debug.run" => self.request_run_and_attach(),
                    _ => continue,
                }
                return true;
            }
        }
        false
    }

    /// Botão de depurar: sobe a aplicação e conecta, com o alvo configurado.
    fn request_run_and_attach(&mut self) {
        if self.debug.attached {
            self.status_message = "Depuração já conectada".to_owned();
            return;
        }
        match self.debug_target() {
            Some((host, port)) => {
                self.debug_requests
                    .push(DebugRequest::RunAndAttach { host, port });
            }
            None => {
                self.settings_page = SettingsPage::Debug;
                self.open_settings_requested = true;
                self.status_message = "Informe um host e uma porta de depuração válidos".to_owned();
            }
        }
    }

    /// Valida host e porta antes de pedir a conexão à aplicação.
    fn attach_debug_target(&mut self) {
        let host = self.debug_host.value().trim().to_owned();
        let port = self.debug_port.value().trim().parse::<u16>().ok();
        match (host.is_empty(), port) {
            (false, Some(port)) if port > 0 => {
                self.debug_requests.push(DebugRequest::Attach {
                    host: host.clone(),
                    port,
                });
                self.settings_modal.close();
                self.settings_dialog = None;
                self.settings_focus = None;
                self.status_message = format!("Conectando ao alvo de depuração {host}:{port}");
            }
            _ => {
                self.set_settings_message("Informe um host e uma porta de depuração válidos.");
            }
        }
    }

    /// Digitação enquanto a página de depuração está em foco.
    fn settings_text_input(&mut self, text: &str) -> bool {
        let Some(focus) = self.settings_focus else {
            return false;
        };
        let input = if focus == DEBUG_HOST_ID {
            &mut self.debug_host
        } else {
            &mut self.debug_port
        };
        let mut value = input.value().to_owned();
        value.push_str(text);
        input.set_value(value);
        true
    }

    fn settings_key_down(&mut self, key: &str) -> bool {
        let Some(focus) = self.settings_focus else {
            return false;
        };
        match key {
            "Backspace" => {
                let input = if focus == DEBUG_HOST_ID {
                    &mut self.debug_host
                } else {
                    &mut self.debug_port
                };
                let mut value = input.value().to_owned();
                value.pop();
                input.set_value(value);
                true
            }
            "Enter" => {
                self.attach_debug_target();
                true
            }
            _ => false,
        }
    }

    fn handle_settings_action(&mut self, result: EventResult) -> bool {
        let EventResult::Action(WidgetAction::Command(command)) = result else {
            return false;
        };
        if let Some(index) = command
            .0
            .strip_prefix("jdk.select.")
            .and_then(|value| value.parse::<usize>().ok())
        {
            self.settings_jdk_result = Some(index);
            return true;
        }
        match command.0.as_str() {
            "jdk.browse" => {
                self.browse_jdk_requested = true;
                true
            }
            "settings.close" => {
                self.settings_modal.close();
                self.settings_dialog = None;
                true
            }
            _ => false,
        }
    }
}

struct SettingsDialogGeometry {
    sidebar: Rect,
    compiler_option: Rect,
    debug_option: Rect,
    combo: Rect,
    browse: Rect,
    close: Rect,
    debug_host: Rect,
    debug_port: Rect,
    debug_attach: Rect,
}

fn primary_pointer(point: Point) -> PointerEvent {
    PointerEvent {
        position: point,
        button: Some(PointerButton::Primary),
    }
}

fn click_widget(widget: &mut dyn Widget, point: Point) -> EventResult {
    let mut context = EventContext::default();
    let pointer = primary_pointer(point);
    let _ = widget.event(&mut context, &UiEvent::PointerDown(pointer));
    widget.event(&mut context, &UiEvent::PointerUp(pointer))
}

fn settings_dialog_geometry(dialog: Rect) -> SettingsDialogGeometry {
    let sidebar = Rect::new(
        dialog.origin.x,
        dialog.origin.y + 52.0,
        210.0,
        dialog.size.height - 52.0,
    );
    let compiler_option = Rect::new(sidebar.origin.x, sidebar.origin.y + 12.0, 210.0, 42.0);
    let combo = Rect::new(
        sidebar.origin.x + sidebar.size.width + 28.0,
        dialog.origin.y + 126.0,
        (dialog.size.width - sidebar.size.width - 178.0).max(190.0),
        36.0,
    );
    let browse = Rect::new(
        combo.origin.x + combo.size.width + 10.0,
        combo.origin.y,
        112.0,
        36.0,
    );
    let close = Rect::new(
        dialog.origin.x + dialog.size.width - 104.0,
        dialog.origin.y + dialog.size.height - 48.0,
        88.0,
        34.0,
    );
    let debug_option = Rect::new(
        compiler_option.origin.x,
        compiler_option.origin.y + compiler_option.size.height + 4.0,
        compiler_option.size.width,
        compiler_option.size.height,
    );
    let debug_host = Rect::new(combo.origin.x, combo.origin.y, 220.0, 36.0);
    let debug_port = Rect::new(
        debug_host.origin.x + debug_host.size.width + 12.0,
        debug_host.origin.y,
        96.0,
        36.0,
    );
    let debug_attach = Rect::new(
        debug_host.origin.x,
        debug_host.origin.y + debug_host.size.height + 20.0,
        120.0,
        34.0,
    );
    SettingsDialogGeometry {
        sidebar,
        compiler_option,
        debug_option,
        combo,
        browse,
        close,
        debug_host,
        debug_port,
        debug_attach,
    }
}

struct Geometry {
    content_top: f32,
    content_bottom: f32,
    editor_bottom: f32,
    editor_width: f32,
    editor_height: f32,
    terminal_height: f32,
}

/// Barra de ações no canto direito da barra de menus: parar, executar, depurar.
///
/// A ordem e o desenho dos ícones pertencem à ERLibUi; aqui só existe a posição
/// dos botões na janela.
fn action_button_rects(size: Size) -> [Rect; 3] {
    const SIDE: f32 = 28.0;
    const GAP: f32 = 2.0;
    let top = (TITLE_HEIGHT - SIDE) / 2.0;
    let first = (size.width - 10.0 - SIDE * 3.0 - GAP * 2.0).max(0.0);
    [0.0, 1.0, 2.0].map(|index| Rect::new(first + index * (SIDE + GAP), top, SIDE, SIDE))
}

/// Comandos do menu `Depurar` que viram pedidos diretos à sessão.
fn debug_request_for(command: &str) -> Option<DebugRequest> {
    Some(match command {
        "debug.continue" => DebugRequest::Continue,
        "debug.pause" => DebugRequest::Pause,
        "debug.over" => DebugRequest::StepOver,
        "debug.into" => DebugRequest::StepInto,
        "debug.out" => DebugRequest::StepOut,
        "debug.detach" => DebugRequest::Detach,
        _ => return None,
    })
}

/// Botões do painel, na ordem em que aparecem.
const DEBUG_BUTTONS: [(&str, DebugRequest); 5] = [
    ("Cont.", DebugRequest::Continue),
    ("Sobre", DebugRequest::StepOver),
    ("Entrar", DebugRequest::StepInto),
    ("Sair", DebugRequest::StepOut),
    ("Fim", DebugRequest::Detach),
];

struct DebugPanelGeometry {
    panel: Rect,
    buttons: Vec<Rect>,
    /// Área das listas, já no formato que os widgets da biblioteca recebem.
    frames: Rect,
    variables: Rect,
}

fn debug_panel_geometry(panel: Rect, frame_count: usize) -> DebugPanelGeometry {
    let button_width = (panel.size.width - 20.0) / DEBUG_BUTTONS.len() as f32;
    let buttons = (0..DEBUG_BUTTONS.len())
        .map(|index| {
            Rect::new(
                panel.origin.x + 10.0 + index as f32 * button_width,
                panel.origin.y + 34.0,
                button_width - 4.0,
                26.0,
            )
        })
        .collect();
    let frames_top = panel.origin.y + 86.0;
    let visible_frames = frame_count.clamp(1, 8) as f32;
    let frames_height = visible_frames * DEBUG_ROW_HEIGHT;
    let list_x = panel.origin.x + 6.0;
    let list_width = (panel.size.width - 12.0).max(0.0);
    let variables_top = frames_top + frames_height + 30.0;
    DebugPanelGeometry {
        panel,
        buttons,
        frames: Rect::new(list_x, frames_top, list_width, frames_height),
        variables: Rect::new(
            list_x,
            variables_top,
            list_width,
            (panel.origin.y + panel.size.height - variables_top).max(0.0),
        ),
    }
}

fn geometry(size: Size, requested_terminal_height: f32, sidebar_width: f32) -> Geometry {
    let content_top = TITLE_HEIGHT + TAB_HEIGHT;
    // O rodapé é a barra de estado da biblioteca; a altura é dela.
    let content_bottom = size.height - StatusBar::HEIGHT;
    let terminal_height = requested_terminal_height
        .min((content_bottom - content_top - 100.0).max(TERMINAL_COLLAPSED_HEIGHT));
    let editor_height = (content_bottom - content_top - terminal_height).max(0.0);
    Geometry {
        content_top,
        content_bottom,
        editor_bottom: content_top + editor_height,
        editor_width: (size.width - ACTIVITY_WIDTH - sidebar_width).max(0.0),
        editor_height,
        terminal_height,
    }
}

fn previous_boundary(text: &str, cursor: usize) -> usize {
    text.get(..cursor.min(text.len()))
        .and_then(|prefix| prefix.char_indices().next_back().map(|(index, _)| index))
        .unwrap_or(0)
}
fn next_boundary(text: &str, cursor: usize) -> usize {
    text.get(cursor..)
        .and_then(|suffix| suffix.chars().next())
        .map_or(cursor, |value| cursor + value.len_utf8())
        .min(text.len())
}
fn byte_at_column(text: &str, column: usize) -> usize {
    text.char_indices()
        .nth(column)
        .map_or(text.len(), |(index, _)| index)
}
fn line_column(text: &str, cursor: usize) -> (usize, usize) {
    let prefix = &text[..cursor.min(text.len())];
    let line = prefix.bytes().filter(|byte| *byte == b'\n').count();
    let column = prefix
        .rsplit('\n')
        .next()
        .unwrap_or_default()
        .chars()
        .count();
    (line, column)
}

fn offset_for_line_column(text: &str, target_line: usize, target_column: usize) -> usize {
    let mut offset = 0;
    for (line, value) in text.split('\n').enumerate() {
        if line == target_line {
            return offset + byte_at_column(value, target_column);
        }
        offset += value.len() + 1;
    }
    text.len()
}

fn token_at(text: &str, offset: usize) -> Option<String> {
    if text.is_empty() {
        return None;
    }
    let offset = offset.min(text.len());
    let mut start = offset;
    while start > 0 {
        let previous = previous_boundary(text, start);
        let character = text[previous..start].chars().next()?;
        if !is_identifier_character(character) {
            break;
        }
        start = previous;
    }
    let mut end = offset;
    while end < text.len() {
        let next = next_boundary(text, end);
        let character = text[end..next].chars().next()?;
        if !is_identifier_character(character) {
            break;
        }
        end = next;
    }
    (start < end).then(|| text[start..end].to_owned())
}

fn identifier_prefix(text: &str, offset: usize) -> String {
    let offset = offset.min(text.len());
    let mut start = offset;
    while start > 0 {
        let previous = previous_boundary(text, start);
        let Some(character) = text[previous..start].chars().next() else {
            break;
        };
        if !is_identifier_character(character) {
            break;
        }
        start = previous;
    }
    text[start..offset].to_owned()
}

fn position_in_range(line: usize, column: usize, range: ide_domain::TextRange) -> bool {
    let start = (range.start.line as usize, range.start.column as usize);
    let end = (range.end.line as usize, range.end.column as usize);
    (line, column) >= start && (line, column) < end
}

fn is_identifier_character(character: char) -> bool {
    character == '_' || character.is_alphanumeric()
}

fn fill(rect: Rect, color: Color) -> PaintCommand {
    PaintCommand::FillRect(FillRectCommand { rect, color })
}
fn stroke(rect: Rect, color: Color) -> PaintCommand {
    PaintCommand::StrokeRect(StrokeRectCommand {
        rect,
        color,
        width: 1.0,
    })
}
fn label(text: &str, origin: Point, color: Color, size: f32) -> PaintCommand {
    PaintCommand::DrawText(DrawTextCommand {
        font_id: FontId(0),
        text: text.to_owned(),
        origin,
        color,
        size,
    })
}

/// Se um token realçado pode levar a uma definição.
///
/// O cursor precisa concordar com o clique. Enquanto só `Type` acendia a mão, o
/// clique navegava em método, campo e variável sem que nada na tela dissesse que
/// era possível — e o usuário concluía, com razão, que ali não funcionava.
///
/// Palavra-chave, literal, comentário e operador ficam de fora: nenhum deles
/// declara nada, e uma mão sobre cada palavra do arquivo não informa coisa
/// alguma.
const fn is_navigable(kind: SyntaxHighlightKind) -> bool {
    matches!(
        kind,
        SyntaxHighlightKind::Type
            | SyntaxHighlightKind::Function
            | SyntaxHighlightKind::Field
            | SyntaxHighlightKind::Variable
            | SyntaxHighlightKind::Annotation
    )
}

/// Papel do realce da IDE no vocabulário do editor da biblioteca.
const fn token_kind_for(kind: SyntaxHighlightKind) -> TokenKind {
    match kind {
        SyntaxHighlightKind::Keyword | SyntaxHighlightKind::Operator => TokenKind::Keyword,
        SyntaxHighlightKind::Type => TokenKind::Type,
        SyntaxHighlightKind::Function => TokenKind::Function,
        SyntaxHighlightKind::String => TokenKind::String,
        SyntaxHighlightKind::Number => TokenKind::Number,
        SyntaxHighlightKind::Comment => TokenKind::Comment,
        // Anotação e nomes comuns não têm token próprio no editor: seguem o
        // texto, que é o que o tema define para código sem classificação.
        SyntaxHighlightKind::Annotation
        | SyntaxHighlightKind::Field
        | SyntaxHighlightKind::Variable => TokenKind::Plain,
    }
}

/// Identidade estável de um nó do Explorer.
///
/// A árvore da biblioteca identifica nós por número e o Explorer os identifica
/// por caminho; o caminho é o que sobrevive a uma releitura do disco, então ele
/// é a origem do número.
fn explorer_id(path: &Path) -> u64 {
    let mut hasher = DefaultHasher::new();
    path.hash(&mut hasher);
    hasher.finish()
}

/// Converte a árvore de arquivos em itens da biblioteca.
fn explorer_items(node: &FileNode) -> Vec<TreeItem> {
    node.children
        .iter()
        .map(|child| {
            TreeItem::new(
                explorer_id(&child.path),
                child
                    .path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("?"),
                explorer_items(child),
            )
        })
        .collect()
}

/// O que uma barra de abas pediu para um clique.
enum TabCommand {
    Select(u64),
    Close(u64),
}

/// Entrega o clique ao componente de abas e traduz o comando emitido.
///
/// Um clique é pressionar e soltar; a interface da IDE só encaminha o
/// pressionar, então os dois eventos vão juntos.
fn tab_command(tabs: &mut Tabs, point: Point) -> Option<TabCommand> {
    let mut context = EventContext::default();
    let event = UiEvent::PointerDown(primary_pointer(point));
    tabs.event(&mut context, &event);
    let result = tabs.event(&mut context, &UiEvent::PointerUp(primary_pointer(point)));
    let EventResult::Action(WidgetAction::Command(CommandId(command))) = result else {
        return None;
    };
    if let Some(id) = command.strip_prefix("tabs.close.") {
        return id.parse().ok().map(TabCommand::Close);
    }
    command
        .strip_prefix("tabs.select.")
        .and_then(|id| id.parse().ok())
        .map(TabCommand::Select)
}

fn ordered_selection(selection: TerminalSelection) -> (TextPosition, TextPosition) {
    if (selection.anchor.line, selection.anchor.column)
        <= (selection.focus.line, selection.focus.column)
    {
        (selection.anchor, selection.focus)
    } else {
        (selection.focus, selection.anchor)
    }
}

fn selection_columns(
    selection: Option<TerminalSelection>,
    line: usize,
    text: &str,
) -> Option<(usize, usize)> {
    let (start, end) = ordered_selection(selection?);
    if line < start.line || line > end.line {
        return None;
    }
    let length = text.chars().count();
    let from = if line == start.line {
        start.column.min(length)
    } else {
        0
    };
    let to = if line == end.line {
        end.column.min(length)
    } else {
        length
    };
    (to > from).then_some((from, to))
}
fn count_outline(items: &[OutlineItem]) -> usize {
    items
        .iter()
        .map(|item| 1 + count_outline(&item.children))
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_shell() -> IdeShell {
        let root = PathBuf::from("workspace");
        let directory = root.join("src");
        IdeShell::from_tree(FileNode {
            path: root,
            is_directory: true,
            children: vec![FileNode {
                path: directory,
                is_directory: true,
                children: Vec::new(),
            }],
        })
    }

    /// O Explorer desenha pela árvore da biblioteca, e a rolagem horizontal
    /// desloca as linhas em vez de cortá-las.
    #[test]
    fn the_explorer_paints_through_the_tree_and_slides_horizontally() {
        let root = PathBuf::from("workspace");
        let mut shell = IdeShell::from_tree(FileNode {
            path: root.clone(),
            is_directory: true,
            children: vec![FileNode {
                path: root.join("um_arquivo_de_nome_bem_longo_para_exceder_o_painel.rs"),
                is_directory: false,
                children: Vec::new(),
            }],
        });
        let size = Size::new(1280.0, 800.0);
        let origin_of = |shell: &mut IdeShell| {
            shell
                .paint(size)
                .iter()
                .find_map(|command| match command {
                    PaintCommand::DrawText(text) if text.text.contains("um_arquivo") => {
                        Some(text.origin.x)
                    }
                    _ => None,
                })
                .unwrap_or_default()
        };
        let before = origin_of(&mut shell);
        assert!(before > 0.0, "o Explorer precisa desenhar o arquivo");

        shell.explorer_scroll_x = 20.0;
        assert!(
            (before - origin_of(&mut shell) - 20.0).abs() < 0.1,
            "a linha desliza com a rolagem horizontal"
        );
    }

    #[test]
    fn explorer_click_toggles_directory() {
        let mut shell = test_shell();
        let directory = PathBuf::from("workspace").join("src");
        assert!(!shell.is_expanded(&directory));
        shell.pointer_down(
            Point::new(80.0, EXPLORER_TOP + 2.0),
            Size::new(1280.0, 800.0),
        );
        assert!(shell.is_expanded(&directory));
    }

    fn shell_with_java_file() -> (IdeShell, PathBuf) {
        let mut shell = test_shell();
        let path = PathBuf::from("Main.java");
        shell.editor.open_memory(
            "Main.java",
            "class Main {\n  void run() {\n    int total = 1;\n  }\n}",
        );
        (shell, path)
    }

    /// A calha mostra a diferença entre pedido e confirmado, e a linha parada
    /// é destacada inteira.
    #[test]
    fn the_gutter_shows_pending_and_confirmed_breakpoints_and_the_stopped_line() {
        let mut shell = test_shell();
        shell.editor.open_memory("A.java", "um\ndois\ntres\nquatro");
        let path = PathBuf::from("A.java");
        let size = Size::new(1280.0, 800.0);
        shell.toggle_breakpoint(&path, 1);
        // Só as marcas da calha interessam; a janela desenha outros círculos.
        let gutter = ACTIVITY_WIDTH + SIDEBAR_WIDTH + EDITOR_GUTTER;
        let circles = |shell: &mut IdeShell| {
            shell
                .paint(size)
                .iter()
                .fold((0, 0), |(filled, outlined), command| match command {
                    PaintCommand::FillCircle(circle) if circle.center.x < gutter => {
                        (filled + 1, outlined)
                    }
                    PaintCommand::StrokeCircle(circle) if circle.center.x < gutter => {
                        (filled, outlined + 1)
                    }
                    _ => (filled, outlined),
                })
        };
        assert_eq!(circles(&mut shell), (0, 1), "sem sessão, o ponto é pendente");

        shell.set_verified_breakpoints(&path, &[1]);
        assert_eq!(circles(&mut shell), (1, 0), "confirmado vira disco");

        shell.set_debug_view(DebugView {
            attached: true,
            stopped_at: Some((path, 2)),
            ..DebugView::default()
        });
        let highlight = Theme::default().colors.highlight;
        assert!(
            shell.paint(size).iter().any(|command| matches!(
                command,
                PaintCommand::FillRect(fill) if fill.color == highlight
            )),
            "a linha em execução é destacada"
        );
    }

    #[test]
    fn clicking_the_gutter_toggles_a_breakpoint_and_marks_the_file() {
        let (mut shell, path) = shell_with_java_file();
        let size = Size::new(1280.0, 800.0);
        let geometry = shell.geometry(size);
        let editor_x = ACTIVITY_WIDTH + shell.sidebar_width(size);
        // Terceira linha visível, dentro da calha.
        let point = Point::new(
            editor_x + 20.0,
            geometry.content_top + 15.0 + 2.0 * EDITOR_LINE_HEIGHT + 2.0,
        );

        shell.pointer_down(point, size);
        assert_eq!(shell.breakpoints_for(&path), vec![2]);
        assert_eq!(
            shell.take_breakpoints_dirty().as_deref(),
            Some(path.as_path())
        );
        assert_eq!(shell.breakpoint_count(), 1);
        assert!(
            shell
                .paint(size)
                .iter()
                .any(|command| matches!(command, PaintCommand::StrokeCircle(_))),
            "sem confirmação do alvo, o marcador aparece apenas como contorno"
        );

        shell.set_verified_breakpoints(&path, &[2]);
        assert!(shell.breakpoint_is_verified(&path, 2));
        assert!(
            shell
                .paint(size)
                .iter()
                .any(|command| matches!(command, PaintCommand::FillCircle(_))),
            "confirmado pelo alvo, o marcador fica cheio"
        );

        shell.pointer_down(point, size);
        assert!(shell.breakpoints_for(&path).is_empty());
        assert_eq!(shell.breakpoint_count(), 0);
    }

    #[test]
    fn toggling_from_the_keyboard_uses_the_cursor_line() {
        let (mut shell, path) = shell_with_java_file();
        shell.cursor_offset = 20; // segunda linha
        shell.toggle_breakpoint_at_cursor();
        assert_eq!(shell.breakpoints_for(&path), vec![1]);
    }

    #[test]
    fn debug_panel_shows_stack_and_variables_and_selects_a_frame() {
        let (mut shell, path) = shell_with_java_file();
        let size = Size::new(1280.0, 800.0);
        shell.set_debug_view(DebugView {
            attached: true,
            status: "Parado em Main.run".to_owned(),
            stopped_at: Some((path.clone(), 2)),
            frames: vec![
                DebugFrameView {
                    name: "Main.run".to_owned(),
                    location: Some((path.clone(), 2)),
                },
                DebugFrameView {
                    name: "Main.main".to_owned(),
                    location: Some((path, 3)),
                },
            ],
            selected_frame: 0,
            variables: vec![DebugVariableView {
                name: "total".to_owned(),
                value: "1".to_owned(),
                type_name: None,
            }],
        });

        let texts: Vec<String> = shell
            .paint(size)
            .iter()
            .filter_map(|command| match command {
                PaintCommand::DrawText(text) => Some(text.text.clone()),
                _ => None,
            })
            .collect();
        assert!(texts.iter().any(|text| text == "Parado em Main.run"));
        assert!(texts.iter().any(|text| text.starts_with("Main.run:3")));
        assert!(texts.iter().any(|text| text == "total = 1"));
        assert!(texts.iter().any(|text| text == "Pilha de chamadas"));

        let panel = debug_panel_geometry(shell.debug_panel_rect(size), 2);
        shell.pointer_down(
            Point::new(
                panel.panel.origin.x + 40.0,
                panel.frames.origin.y + DEBUG_ROW_HEIGHT + 2.0,
            ),
            size,
        );
        assert_eq!(
            shell.take_debug_requests(),
            vec![DebugRequest::SelectFrame(1)]
        );
        assert_eq!(shell.debug_view().selected_frame, 1);

        // Clicar abaixo do último quadro não é escolha de quadro nenhum.
        shell.pointer_down(
            Point::new(
                panel.panel.origin.x + 40.0,
                panel.frames.origin.y + panel.frames.size.height + 6.0,
            ),
            size,
        );
        assert_eq!(shell.take_debug_requests(), vec![]);
        assert_eq!(shell.debug_view().selected_frame, 1);
    }

    /// A barra de estado informa em segmentos: a mensagem à esquerda, e o que
    /// se procura sempre no mesmo lugar ancorado à direita.
    #[test]
    fn the_status_bar_reports_message_and_position_in_separate_segments() {
        let mut shell = test_shell();
        shell.set_status_message("Compilação concluída");
        let size = Size::new(1_000.0, 700.0);
        let texts: Vec<String> = shell
            .paint(size)
            .iter()
            .filter_map(|command| match command {
                PaintCommand::DrawText(text) => Some(text.text.clone()),
                _ => None,
            })
            .collect();
        assert!(texts.iter().any(|text| text == "Compilação concluída"));
        assert!(texts.iter().any(|text| text == "UTF-8"));
        assert!(texts.iter().any(|text| text == "Ln 1, Col 1"));
    }

    #[test]
    fn debug_panel_buttons_and_menu_emit_session_requests() {
        let (mut shell, _) = shell_with_java_file();
        let size = Size::new(1280.0, 800.0);
        shell.set_debug_view(DebugView {
            attached: true,
            status: "Parado".to_owned(),
            ..DebugView::default()
        });

        let panel = debug_panel_geometry(shell.debug_panel_rect(size), 0);
        let button = panel.buttons[1];
        shell.pointer_down(
            Point::new(button.origin.x + 4.0, button.origin.y + 4.0),
            size,
        );
        assert_eq!(shell.take_debug_requests(), vec![DebugRequest::StepOver]);

        // Menu `Depurar` → `Continuar`.
        shell.pointer_down(Point::new(280.0, 10.0), size);
        shell.pointer_down(Point::new(280.0, TITLE_HEIGHT + 38.0), size);
        assert_eq!(shell.take_debug_requests(), vec![DebugRequest::Continue]);
    }

    #[test]
    fn the_action_buttons_are_library_widgets_with_accessible_names() {
        let shell = test_shell();
        let mut context = PaintContext::with_theme(*shell.theme());
        let mut accessibility = ui_api::AccessibilityContext::default();
        for (button, rect) in shell
            .action_buttons()
            .into_iter()
            .zip(action_button_rects(Size::new(1_280.0, 800.0)))
        {
            let mut button = button.clone();
            button.layout(&LayoutContext::default(), rect);
            button.paint(&mut context);
            button.accessibility(&mut accessibility);
        }
        let names: Vec<&str> = accessibility
            .nodes()
            .iter()
            .map(|node| node.label.as_str())
            .collect();
        assert_eq!(
            names,
            vec![
                "Parar aplicação",
                "Executar aplicação",
                "Executar com depuração"
            ],
            "um ícone não é legível: quem o expõe é a biblioteca"
        );
    }

    #[test]
    fn the_play_button_requests_a_plain_run() {
        let mut shell = test_shell();
        let size = Size::new(1_280.0, 800.0);
        let [_, run, debug] = action_button_rects(size);
        assert!(
            run.origin.x + run.size.width <= debug.origin.x,
            "o play fica à esquerda do inseto, sem sobrepor"
        );
        let colors = Theme::default().colors;
        assert!(
            shell
                .paint(size)
                .iter()
                .filter(|command| matches!(command, PaintCommand::FillRect(rect)
                    if rect.color == colors.success && run.contains(rect.rect.origin)))
                .count()
                >= 5,
            "o triângulo de play é desenhado com a cor de ação da paleta"
        );

        shell.pointer_down(Point::new(run.origin.x + 6.0, run.origin.y + 6.0), size);
        assert!(shell.take_run_request());
        assert!(!shell.take_run_request(), "o pedido é consumido uma vez");
        assert!(
            shell.take_debug_requests().is_empty(),
            "executar sem depuração não abre sessão"
        );
    }

    #[test]
    fn the_stop_button_sits_left_of_play_and_only_acts_after_a_run() {
        let mut shell = test_shell();
        let size = Size::new(1_280.0, 800.0);
        let [stop, run, _] = action_button_rects(size);
        assert!(
            stop.origin.x + stop.size.width <= run.origin.x,
            "a ordem é parar, executar, depurar"
        );

        let colors = Theme::default().colors;
        let icon_color = |shell: &mut IdeShell| {
            shell.paint(size).iter().find_map(|command| match command {
                PaintCommand::FillRect(rect)
                    if stop.contains(rect.rect.origin) && rect.rect.size.width < 20.0 =>
                {
                    Some(rect.color)
                }
                _ => None,
            })
        };
        assert_eq!(
            icon_color(&mut shell),
            Some(colors.muted_text),
            "sem aplicação iniciada, o ícone fica apagado"
        );
        assert!(!shell.application_running());

        shell.pointer_down(Point::new(stop.origin.x + 6.0, stop.origin.y + 6.0), size);
        assert!(shell.take_stop_request());
        assert!(
            shell.stop_application().is_err(),
            "sem aplicação iniciada não há o que interromper"
        );
    }

    #[test]
    fn the_project_menu_also_runs_the_application() {
        let mut shell = test_shell();
        let size = Size::new(1_000.0, 700.0);
        shell.pointer_down(Point::new(200.0, 10.0), size);
        shell.pointer_down(Point::new(200.0, TITLE_HEIGHT + 66.0), size);
        assert!(shell.take_run_request());
        assert!(!shell.take_build_project_request());
        assert!(!shell.take_reimport_project_request());
    }

    #[test]
    fn the_bug_button_runs_and_attaches_with_the_configured_target() {
        let mut shell = test_shell();
        let size = Size::new(1_280.0, 800.0);
        shell.set_debug_target("10.0.0.20", 8787);

        let button = action_button_rects(size)[2];
        assert!(
            button.origin.x + button.size.width < size.width && button.origin.x > size.width - 60.0,
            "o botão fica no canto direito da barra de menus"
        );
        assert!(
            shell
                .paint(size)
                .iter()
                .filter(|command| matches!(command, PaintCommand::FillCircle(circle)
                    if button.contains(circle.center)))
                .count()
                >= 2,
            "o ícone desenha corpo e cabeça do inseto dentro do botão"
        );

        shell.pointer_down(
            Point::new(button.origin.x + 6.0, button.origin.y + 6.0),
            size,
        );
        assert_eq!(
            shell.take_debug_requests(),
            vec![DebugRequest::RunAndAttach {
                host: "10.0.0.20".to_owned(),
                port: 8787,
            }]
        );
    }

    #[test]
    fn the_bug_button_asks_for_a_target_when_it_is_invalid() {
        let mut shell = test_shell();
        let size = Size::new(1_280.0, 800.0);
        shell.set_debug_target("", 0);

        let button = action_button_rects(size)[2];
        shell.pointer_down(
            Point::new(button.origin.x + 6.0, button.origin.y + 6.0),
            size,
        );

        assert!(shell.take_debug_requests().is_empty());
        assert!(
            shell.take_open_settings_request(),
            "sem alvo válido, o botão abre a página de depuração"
        );
        assert_eq!(shell.settings_page(), SettingsPage::Debug);
    }

    #[test]
    fn the_debug_menu_opens_the_settings_window_on_the_debug_page() {
        let mut shell = test_shell();
        let size = Size::new(1_000.0, 700.0);
        assert_eq!(shell.settings_page(), SettingsPage::Compiler);

        // Menu `Depurar` → `Conectar...`.
        shell.pointer_down(Point::new(280.0, 10.0), size);
        shell.pointer_down(Point::new(280.0, TITLE_HEIGHT + 10.0), size);
        assert!(shell.take_open_settings_request());
        assert_eq!(shell.settings_page(), SettingsPage::Debug);

        shell.open_settings_dialog(vec!["JDK 17".to_owned()], 0);
        let texts: Vec<String> = shell
            .paint(size)
            .iter()
            .filter_map(|command| match command {
                PaintCommand::DrawText(text) => Some(text.text.clone()),
                _ => None,
            })
            .collect();
        assert!(texts.iter().any(|text| text.contains("Host e porta")));
        assert!(!texts.iter().any(|text| text == "JDK"));

        // O atalho do compilador troca a página de volta.
        shell.set_settings_page(SettingsPage::Compiler);
        let texts: Vec<String> = shell
            .paint(size)
            .iter()
            .filter_map(|command| match command {
                PaintCommand::DrawText(text) => Some(text.text.clone()),
                _ => None,
            })
            .collect();
        assert!(texts.iter().any(|text| text == "JDK"));
    }

    #[test]
    fn debug_settings_page_validates_the_target_before_connecting() {
        let mut shell = test_shell();
        let size = Size::new(1_000.0, 700.0);
        shell.open_settings_dialog(vec!["JDK 8".to_owned()], 0);
        shell
            .settings_modal
            .layout(&LayoutContext::default(), Rect::new(0.0, 0.0, size.width, size.height));
        let geometry = settings_dialog_geometry(shell.settings_modal.panel_bounds());

        shell.pointer_down(
            Point::new(
                geometry.debug_option.origin.x + 20.0,
                geometry.debug_option.origin.y + 10.0,
            ),
            size,
        );
        let texts: Vec<String> = shell
            .paint(size)
            .iter()
            .filter_map(|command| match command {
                PaintCommand::DrawText(text) => Some(text.text.clone()),
                _ => None,
            })
            .collect();
        assert!(texts.iter().any(|text| text == "Depuração"));
        assert!(texts.iter().any(|text| text.contains("agentlib:jdwp")));

        shell.pointer_down(
            Point::new(
                geometry.debug_port.origin.x + 10.0,
                geometry.debug_port.origin.y + 10.0,
            ),
            size,
        );
        shell.key_down("Backspace");
        shell.key_down("Backspace");
        shell.key_down("Backspace");
        shell.key_down("Backspace");
        shell.text_input("porta");
        shell.pointer_down(
            Point::new(
                geometry.debug_attach.origin.x + 10.0,
                geometry.debug_attach.origin.y + 10.0,
            ),
            size,
        );
        assert!(
            shell.take_debug_requests().is_empty(),
            "porta inválida não conecta"
        );
        assert!(shell.settings_dialog_open());

        shell.key_down("Backspace");
        shell.key_down("Backspace");
        shell.key_down("Backspace");
        shell.key_down("Backspace");
        shell.key_down("Backspace");
        shell.text_input("5005");
        shell.key_down("Enter");
        assert_eq!(
            shell.take_debug_requests(),
            vec![DebugRequest::Attach {
                host: "127.0.0.1".to_owned(),
                port: 5005,
            }]
        );
        assert!(!shell.settings_dialog_open());
    }

    #[test]
    fn java_syntax_snapshot_drives_highlighting_and_outline() {
        let mut shell = test_shell();
        let document_id = shell
            .editor
            .open_memory("Example.java", "public class Example {}");
        shell.set_syntax_snapshot(ide_domain::SyntaxSnapshot {
            document_id,
            version: 0,
            tree: ide_domain::SyntaxNode {
                kind: "program".to_owned(),
                range: ide_domain::TextRange::default(),
                has_error: false,
                children: Vec::new(),
            },
            outline: vec![ide_domain::OutlineItem {
                name: "Example".to_owned(),
                kind: ide_domain::OutlineKind::Class,
                range: ide_domain::TextRange::default(),
                children: Vec::new(),
            }],
            highlights: vec![ide_domain::SyntaxHighlight {
                range: ide_domain::TextRange {
                    start: ide_domain::TextPosition { line: 0, column: 0 },
                    end: ide_domain::TextPosition { line: 0, column: 6 },
                },
                kind: SyntaxHighlightKind::Keyword,
            }],
            imports: Vec::new(),
            diagnostics: Vec::new(),
        });

        assert_eq!(shell.active_outline()[0].name, "Example");
        let colors = Theme::default().colors;
        assert!(shell.paint(Size::new(1280.0, 800.0)).iter().any(|command| {
            matches!(
                command,
                PaintCommand::DrawText(text)
                    if text.text == "public" && text.color == colors.syntax_keyword
            )
        }));
    }

    #[test]
    fn completion_popup_can_apply_selected_item() {
        let mut shell = test_shell();
        shell.editor.open_memory("Example.java", "Exa");
        shell.focus = ShellFocus::Editor;
        shell.cursor_offset = 3;
        shell.set_completions(vec![CompletionItem {
            label: "Example".to_owned(),
            detail: Some("class".to_owned()),
            kind: ide_domain::CompletionKind::Class,
        }]);
        assert!(shell.paint(Size::new(1280.0, 800.0)).iter().any(|command| {
            matches!(command, PaintCommand::DrawText(text) if text.text == "Example")
        }));
        shell.key_down("Enter");
        assert_eq!(shell.active_text(), Some("Example"));
    }

    /// A mão acende no que dá para navegar, não só em tipo.
    ///
    /// Enquanto só `Type` acendia, o clique navegava em método, campo e variável
    /// sem que nada na tela dissesse que era possível.
    #[test]
    fn the_navigation_cursor_agrees_with_what_the_click_resolves() {
        let mut shell = test_shell();
        //                                    0         1         2         3
        //                                    0123456789012345678901234567890
        let document_id = shell.editor.open_memory("A.java", "void metodo() { int x = y; }");
        let realce = |coluna_inicial: u32, coluna_final: u32, kind| ide_domain::SyntaxHighlight {
            range: ide_domain::TextRange {
                start: ide_domain::TextPosition {
                    line: 0,
                    column: coluna_inicial,
                },
                end: ide_domain::TextPosition {
                    line: 0,
                    column: coluna_final,
                },
            },
            kind,
        };
        shell.set_syntax_snapshot(ide_domain::SyntaxSnapshot {
            document_id,
            version: 0,
            tree: ide_domain::SyntaxNode {
                kind: "program".to_owned(),
                range: ide_domain::TextRange::default(),
                has_error: false,
                children: Vec::new(),
            },
            outline: Vec::new(),
            highlights: vec![
                realce(0, 4, SyntaxHighlightKind::Keyword),
                realce(5, 11, SyntaxHighlightKind::Function),
                realce(20, 21, SyntaxHighlightKind::Variable),
                realce(24, 25, SyntaxHighlightKind::Field),
            ],
            imports: Vec::new(),
            diagnostics: Vec::new(),
        });

        let size = Size::new(1280.0, 800.0);
        let editor_x = ACTIVITY_WIDTH + shell.sidebar_width(size);
        let sobre = |coluna: f32| {
            Point::new(
                editor_x + EDITOR_GUTTER + coluna * EDITOR_CHAR_WIDTH,
                shell.geometry(size).content_top + 15.0,
            )
        };
        assert!(shell.navigation_hover(sobre(7.0), size, true), "método");
        assert!(shell.navigation_hover(sobre(20.0), size, true), "variável");
        assert!(shell.navigation_hover(sobre(24.0), size, true), "campo");
        assert!(
            !shell.navigation_hover(sobre(1.0), size, true),
            "palavra-chave não leva a lugar nenhum"
        );
    }

    #[test]
    fn control_hover_over_java_type_uses_navigation_cursor_state() {
        let mut shell = test_shell();
        let document_id = shell.editor.open_memory("Example.java", "class Example {}");
        shell.set_syntax_snapshot(ide_domain::SyntaxSnapshot {
            document_id,
            version: 0,
            tree: ide_domain::SyntaxNode {
                kind: "program".to_owned(),
                range: ide_domain::TextRange::default(),
                has_error: false,
                children: Vec::new(),
            },
            outline: Vec::new(),
            highlights: vec![ide_domain::SyntaxHighlight {
                range: ide_domain::TextRange {
                    start: ide_domain::TextPosition { line: 0, column: 6 },
                    end: ide_domain::TextPosition {
                        line: 0,
                        column: 13,
                    },
                },
                kind: SyntaxHighlightKind::Type,
            }],
            imports: Vec::new(),
            diagnostics: Vec::new(),
        });
        let size = Size::new(1280.0, 800.0);
        let editor_x = ACTIVITY_WIDTH + shell.sidebar_width(size);
        let point = Point::new(
            editor_x + EDITOR_GUTTER + 8.0 * EDITOR_CHAR_WIDTH,
            shell.geometry(size).content_top + 15.0,
        );
        assert!(!shell.navigation_hover(point, size, false));
        assert!(shell.navigation_hover(point, size, true));
    }

    #[test]
    fn java_tool_output_is_appended_to_terminal() {
        let mut shell = test_shell();
        shell.append_tool_output("compile ok\nruntime failure", true);
        let lines = shell.active_terminal_lines().collect::<Vec<_>>();
        assert!(lines.contains(&"compile ok"));
        assert!(lines.contains(&"runtime failure"));
    }

    #[test]
    fn explorer_horizontal_scrollbar_keeps_long_names_inside_sidebar() {
        let mut shell = IdeShell::from_tree(FileNode {
            path: PathBuf::from("workspace"),
            is_directory: true,
            children: vec![FileNode {
                path: PathBuf::from("workspace")
                    .join("a_very_long_project_filename_that_must_not_overflow_into_the_editor.rs"),
                is_directory: false,
                children: Vec::new(),
            }],
        });
        let size = Size::new(1280.0, 800.0);
        let track = shell.explorer_horizontal_scrollbar_rect(size);
        // O nome não cabe na largura visível, então há o que rolar.
        let (_, content, viewport, _) = shell.scrollbar_range(ScrollTarget::ExplorerHorizontal, size);
        assert!(content > viewport);
        shell.pointer_down(
            Point::new(
                track.origin.x + track.size.width - 1.0,
                track.origin.y + 5.0,
            ),
            size,
        );
        assert!(shell.explorer_scroll_x > 0.0);
        let rendered = shell.paint(size);
        assert!(rendered.iter().any(|command| {
            matches!(
                command,
                PaintCommand::PushClip(rect)
                    if rect.origin.x == ACTIVITY_WIDTH
                        && rect.size.width == shell.sidebar_width(size)
            )
        }));
    }

    #[test]
    fn file_project_menu_requests_a_folder_picker() {
        let mut shell = test_shell();
        let size = Size::new(1280.0, 800.0);
        shell.pointer_down(Point::new(100.0, 15.0), size);
        let menu_is_visible = shell.paint(size).into_iter().any(|command| {
            matches!(
                command,
                PaintCommand::DrawText(command) if command.text == "Projeto..."
            )
        });
        assert!(menu_is_visible);
        shell.pointer_down(Point::new(110.0, TITLE_HEIGHT + 15.0), size);
        assert!(shell.take_open_project_request());
        assert!(!shell.take_open_project_request());
    }

    #[test]
    fn tab_click_changes_active_document_and_typing_edits_it() {
        let mut shell = test_shell();
        let first = shell.editor.open_memory("first.rs", "one");
        let second = shell.editor.open_memory("second.rs", "two");
        assert_eq!(shell.active_document(), Some(second));
        let editor_x = ACTIVITY_WIDTH + SIDEBAR_WIDTH;
        shell.pointer_down(
            Point::new(editor_x + 10.0, TITLE_HEIGHT + 10.0),
            Size::new(1280.0, 800.0),
        );
        assert_eq!(shell.active_document(), Some(first));
        shell.pointer_down(
            Point::new(editor_x + EDITOR_GUTTER, TITLE_HEIGHT + TAB_HEIGHT + 15.0),
            Size::new(1280.0, 800.0),
        );
        shell.text_input("X");
        assert_eq!(shell.active_text(), Some("Xone"));
    }

    #[test]
    fn tab_close_button_removes_only_the_clicked_document() {
        let mut shell = test_shell();
        let first = shell.editor.open_memory("first.rs", "one");
        shell.editor.open_memory("second.rs", "two");
        let size = Size::new(1280.0, 800.0);
        let editor_x = ACTIVITY_WIDTH + SIDEBAR_WIDTH;
        shell.pointer_down(
            Point::new(
                editor_x + TAB_WIDTH * 2.0 - 15.0,
                TITLE_HEIGHT + TAB_HEIGHT / 2.0,
            ),
            size,
        );
        assert_eq!(shell.tab_count(), 1);
        assert_eq!(shell.active_document(), Some(first));
    }

    /// Documento alterado e não gravado é sinalizado na aba.
    #[test]
    fn an_unsaved_document_is_marked_on_its_tab() {
        let mut shell = test_shell();
        shell.editor.open_memory("first.rs", "one");
        let size = Size::new(1280.0, 800.0);
        let marks = |shell: &mut IdeShell| {
            shell
                .paint(size)
                .iter()
                .filter_map(|command| match command {
                    PaintCommand::DrawText(text) => Some(text.text.clone()),
                    _ => None,
                })
                .any(|text| text == "●")
        };
        assert!(!marks(&mut shell), "documento intacto não é marcado");

        shell.focus = ShellFocus::Editor;
        shell.edit_active("x");
        assert!(marks(&mut shell), "documento alterado é marcado");
    }

    #[test]
    fn long_tab_titles_are_clipped_and_ellipsized_before_close_button() {
        let mut shell = test_shell();
        shell
            .editor
            .open_memory("ExplosionEffectManager.ts", "content");
        let rendered = shell.paint(Size::new(1280.0, 800.0));
        let texts = rendered
            .iter()
            .filter_map(|command| match command {
                PaintCommand::DrawText(command) => Some(command.text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>();
        // O nome inteiro não cabe: aparece encurtado, com a marca do corte, e o
        // desenho fica contido na faixa das abas.
        assert!(!texts.contains(&"ExplosionEffectManager.ts"));
        assert!(
            texts
                .iter()
                .any(|text| text.starts_with("Explosion") && text.ends_with('…')),
            "esperava um título encurtado, veio {texts:?}"
        );
        let tabs = shell.editor_tabs_rect(Size::new(1280.0, 800.0));
        assert!(rendered.iter().any(|command| {
            matches!(command, PaintCommand::PushClip(rect) if *rect == tabs)
        }));
    }

    /// O divisor é desenhado no lugar certo desde o primeiro quadro, antes de
    /// qualquer evento de ponteiro, e se destaca quando o ponteiro se aproxima.
    #[test]
    fn the_sidebar_divider_is_painted_in_place_and_highlights_under_the_pointer() {
        let mut shell = test_shell();
        let size = Size::new(1280.0, 800.0);
        let divider_color = |shell: &mut IdeShell| {
            let x = ACTIVITY_WIDTH + shell.sidebar_width(size);
            shell
                .paint(size)
                .iter()
                .find_map(|command| match command {
                    PaintCommand::FillRect(fill)
                        if (fill.rect.origin.x - x).abs() < 0.01
                            && fill.rect.size.width == Splitter::THICKNESS =>
                    {
                        Some(fill.color)
                    }
                    _ => None,
                })
        };
        assert_eq!(divider_color(&mut shell), Some(shell.theme().colors.border));

        shell.pointer_move(Point::new(ACTIVITY_WIDTH + SIDEBAR_WIDTH, 300.0), size);
        assert_eq!(divider_color(&mut shell), Some(shell.theme().colors.accent));
    }

    #[test]
    fn sidebar_border_resizes_explorer_editor_and_terminal_widths_together() {
        let mut shell = test_shell();
        let size = Size::new(1280.0, 800.0);
        let before = shell.geometry(size).editor_width;
        let border = ACTIVITY_WIDTH + SIDEBAR_WIDTH;
        shell.pointer_down(Point::new(border, 300.0), size);
        assert!(shell.sidebar_resizing());
        shell.pointer_move(Point::new(border + 80.0, 300.0), size);
        shell.pointer_up();
        assert_eq!(shell.sidebar_width(size), SIDEBAR_WIDTH + 80.0);
        assert_eq!(shell.geometry(size).editor_width, before - 80.0);
        assert!(!shell.sidebar_resizing());
    }

    #[test]
    fn explorer_vertical_scrollbar_and_wheel_reach_later_entries() {
        let children = (0..80)
            .map(|index| FileNode {
                path: PathBuf::from("workspace").join(format!("file_{index:03}.rs")),
                is_directory: false,
                children: Vec::new(),
            })
            .collect();
        let mut shell = IdeShell::from_tree(FileNode {
            path: PathBuf::from("workspace"),
            is_directory: true,
            children,
        });
        let size = Size::new(1280.0, 800.0);
        let track = shell.explorer_vertical_scrollbar_rect(size);
        shell.scroll(
            Point::new(ACTIVITY_WIDTH + 40.0, EXPLORER_TOP + 40.0),
            5,
            size,
        );
        assert_eq!(shell.explorer_scroll_line, 5);
        shell.pointer_down(
            Point::new(
                track.origin.x + 5.0,
                track.origin.y + track.size.height - 1.0,
            ),
            size,
        );
        assert!(shell.explorer_scroll_line > 5);
    }

    #[test]
    fn editor_wheel_scrolls_and_terminal_profile_is_selectable() {
        let mut shell = test_shell();
        shell.editor.open_memory(
            "long.rs",
            (0..100)
                .map(|line| format!("line {line}"))
                .collect::<Vec<_>>()
                .join("\n"),
        );
        let size = Size::new(1280.0, 800.0);
        let editor_x = ACTIVITY_WIDTH + SIDEBAR_WIDTH;
        shell.scroll(Point::new(editor_x + 100.0, 200.0), 8, size);
        assert_eq!(shell.editor_scroll_line(), 8);
        let terminal_y = shell.geometry(size).editor_bottom + 10.0;
        shell.pointer_down(Point::new(editor_x + 115.0, terminal_y), size);
        assert_eq!(shell.selected_shell(), ShellKind::Cmd);
    }

    #[test]
    fn terminal_tabs_keep_input_and_content_isolated() {
        let mut shell = test_shell();
        let size = Size::new(1280.0, 800.0);
        let editor_x = ACTIVITY_WIDTH + SIDEBAR_WIDTH;
        let terminal_y = shell.geometry(size).editor_bottom + 10.0;

        shell.pointer_down(Point::new(editor_x + 10.0, terminal_y), size);
        shell.text_input("Get-Location");
        assert_eq!(shell.active_terminal_input(), "Get-Location");

        shell.pointer_down(Point::new(editor_x + 115.0, terminal_y), size);
        assert_eq!(shell.active_terminal_index(), 1);
        assert_eq!(shell.active_terminal_input(), "");
        shell.text_input("dir");
        assert_eq!(shell.active_terminal_input(), "dir");
        let rendered = shell
            .paint(size)
            .into_iter()
            .filter_map(|command| match command {
                PaintCommand::DrawText(command) => Some(command.text),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert!(rendered.iter().any(|text| text.ends_with("> dir")));
        assert!(!rendered.iter().any(|text| text.contains("Get-Location")));

        shell.pointer_down(Point::new(editor_x + 10.0, terminal_y), size);
        assert_eq!(shell.active_terminal_index(), 0);
        assert_eq!(shell.active_terminal_input(), "Get-Location");
    }

    #[cfg(windows)]
    #[test]
    fn terminal_input_is_above_command_and_output() {
        let mut shell = test_shell();
        let size = Size::new(1280.0, 800.0);
        let editor_x = ACTIVITY_WIDTH + SIDEBAR_WIDTH;
        let terminal_y = shell.geometry(size).editor_bottom + 10.0;
        shell.pointer_down(Point::new(editor_x + 10.0, terminal_y), size);
        shell.text_input("Write-Output RESULT_BELOW");
        shell.key_down("Enter");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while std::time::Instant::now() < deadline {
            shell.update_terminals(size);
            if shell
                .active_terminal_lines()
                .any(|line| line.contains("RESULT_BELOW"))
            {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        assert!(
            shell
                .active_terminal_lines()
                .any(|line| line.contains("RESULT_BELOW"))
        );

        let geo = shell.geometry(size);
        let input_y = geo.editor_bottom + 38.0;
        let first_output_y = geo.editor_bottom + 68.0;
        assert!(first_output_y > input_y);
    }

    /// Clicar no fim da trilha do editor leva ao fim do documento, e arrastar
    /// de volta traz o conteúdo junto.
    #[test]
    fn the_editor_scrollbar_maps_click_and_drag_to_content_offsets() {
        let mut shell = test_shell();
        let text = (0..200)
            .map(|line| format!("linha {line}"))
            .collect::<Vec<_>>()
            .join("
");
        shell.editor.open_memory("longo.rs", &text);
        let size = Size::new(1280.0, 800.0);
        let track = shell.editor_scrollbar_rect(size);
        let visible = shell.editor_visible_lines(size);

        shell.pointer_down(
            Point::new(track.origin.x + 5.0, track.origin.y + track.size.height),
            size,
        );
        assert_eq!(shell.editor_scroll_line, 200 - visible);

        shell.pointer_move(Point::new(track.origin.x + 5.0, track.origin.y), size);
        assert_eq!(shell.editor_scroll_line, 0);

        shell.pointer_up();
        shell.pointer_move(
            Point::new(track.origin.x + 5.0, track.origin.y + track.size.height),
            size,
        );
        assert_eq!(shell.editor_scroll_line, 0, "soltar encerra o arraste");
    }

    #[test]
    fn terminal_selection_supports_forward_and_reverse_drag() {
        let forward = TerminalSelection {
            anchor: TextPosition { line: 2, column: 1 },
            focus: TextPosition { line: 2, column: 4 },
        };
        let reverse = TerminalSelection {
            anchor: forward.focus,
            focus: forward.anchor,
        };
        assert_eq!(selection_columns(Some(forward), 2, "abcdef"), Some((1, 4)));
        assert_eq!(selection_columns(Some(reverse), 2, "abcdef"), Some((1, 4)));
    }

    #[cfg(windows)]
    #[test]
    fn terminal_wheel_and_scrollbar_change_the_visible_offset() {
        let mut shell = test_shell();
        let size = Size::new(1280.0, 800.0);
        let editor_x = ACTIVITY_WIDTH + SIDEBAR_WIDTH;
        let terminal_y = shell.geometry(size).editor_bottom + 10.0;
        shell.pointer_down(Point::new(editor_x + 10.0, terminal_y), size);
        shell.text_input("1..80 | ForEach-Object { Write-Output \"scroll-$_\" }");
        shell.key_down("Enter");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while std::time::Instant::now() < deadline {
            shell.update_terminals(size);
            if shell.active_terminal().line_count() >= 80 {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        let active = shell.active_terminal;
        let bottom = shell.terminals[active].scroll_line;
        assert!(bottom > 0);

        let content_point = Point::new(editor_x + 100.0, shell.geometry(size).editor_bottom + 90.0);
        shell.scroll(content_point, -5, size);
        assert!(shell.terminals[active].scroll_line < bottom);

        let track = shell.terminal_scrollbar_rect(size);
        shell.pointer_down(Point::new(track.origin.x + 5.0, track.origin.y + 1.0), size);
        assert_eq!(shell.terminals[active].scroll_line, 0);
        shell.pointer_move(
            Point::new(track.origin.x + 5.0, track.origin.y + track.size.height),
            size,
        );
        assert!(shell.terminals[active].scroll_line > 0);
        shell.pointer_up();
    }

    #[cfg(windows)]
    #[test]
    fn vertically_resizing_terminal_never_changes_its_content() {
        let mut shell = test_shell();
        let size = Size::new(1280.0, 800.0);
        let editor_x = ACTIVITY_WIDTH + SIDEBAR_WIDTH;
        let terminal_header = shell.geometry(size).editor_bottom + 10.0;
        shell.pointer_down(Point::new(editor_x + 10.0, terminal_header), size);
        shell.text_input("Write-Output RESIZE_STABLE");
        shell.key_down("Enter");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while std::time::Instant::now() < deadline {
            shell.update_terminals(size);
            if shell
                .active_terminal_lines()
                .any(|line| line.trim() == "RESIZE_STABLE")
            {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        std::thread::sleep(std::time::Duration::from_millis(150));
        shell.update_terminals(size);
        let before = shell
            .active_terminal_lines()
            .map(str::to_owned)
            .collect::<Vec<_>>();

        let border = shell.geometry(size).editor_bottom;
        shell.pointer_down(Point::new(editor_x + 200.0, border), size);
        for y in [border - 20.0, border - 60.0, border - 100.0, border - 40.0] {
            shell.pointer_move(Point::new(editor_x + 200.0, y), size);
            shell.update_terminals(size);
        }
        shell.pointer_up();
        std::thread::sleep(std::time::Duration::from_millis(150));
        shell.update_terminals(size);

        let after = shell
            .active_terminal_lines()
            .map(str::to_owned)
            .collect::<Vec<_>>();
        assert_eq!(after, before);
    }

    #[test]
    fn terminal_button_minimizes_and_restores_previous_height() {
        let mut shell = test_shell();
        let size = Size::new(1280.0, 800.0);
        let original = shell.terminal_height();
        let toggle = Point::new(size.width - 20.0, shell.geometry(size).editor_bottom + 12.0);
        shell.pointer_down(toggle, size);
        assert!(shell.terminal_minimized());
        assert_eq!(
            shell.geometry(size).terminal_height,
            TERMINAL_COLLAPSED_HEIGHT
        );

        let restore = Point::new(size.width - 20.0, shell.geometry(size).editor_bottom + 12.0);
        shell.pointer_down(restore, size);
        assert!(!shell.terminal_minimized());
        assert_eq!(shell.terminal_height(), original);
    }

    #[test]
    fn dragging_terminal_top_border_changes_height_with_limits() {
        let mut shell = test_shell();
        let size = Size::new(1280.0, 800.0);
        let editor_x = ACTIVITY_WIDTH + SIDEBAR_WIDTH;
        let border_y = shell.geometry(size).editor_bottom;
        shell.pointer_down(Point::new(editor_x + 100.0, border_y), size);
        assert!(shell.terminal_resizing());
        assert!(shell.pointer_move(Point::new(editor_x + 100.0, border_y - 70.0), size));
        assert_eq!(shell.terminal_height(), TERMINAL_DEFAULT_HEIGHT + 70.0);
        shell.pointer_move(Point::new(editor_x + 100.0, size.height), size);
        assert_eq!(shell.terminal_height(), TERMINAL_MIN_HEIGHT);
        shell.pointer_up();
        assert!(!shell.terminal_resizing());
    }

    #[test]
    fn control_click_emits_language_neutral_navigation_request() {
        let mut shell = test_shell();
        let document_id = shell.editor.open_memory("main.rs", "fn target() {}\n");
        let size = Size::new(1280.0, 800.0);
        let editor_x = ACTIVITY_WIDTH + SIDEBAR_WIDTH;
        let target_x = editor_x + EDITOR_GUTTER + 5.0 * EDITOR_CHAR_WIDTH;
        shell.pointer_down_with_modifiers(
            Point::new(target_x, TITLE_HEIGHT + TAB_HEIGHT + 15.0),
            size,
            true,
        );
        assert_eq!(
            shell.take_navigation_request(),
            Some(NavigationRequest {
                document_id,
                byte_offset: 5,
                token: "target".to_owned(),
            })
        );
    }

    /// Ir para a definição rola o editor até o destino.
    ///
    /// Antes o cursor era movido e mais nada: um método declarado abaixo da área
    /// visível continuava fora da tela, e a navegação parecia não ter acontecido.
    #[test]
    fn going_to_a_definition_scrolls_the_target_line_into_view() {
        let mut shell = test_shell();
        let texto = (0..200)
            .map(|linha| format!("linha {linha}"))
            .collect::<Vec<_>>()
            .join("
");
        shell.editor.open_memory("Longo.java", &texto);
        let size = Size::new(1280.0, 800.0);
        let visiveis = shell.editor_visible_lines(size);
        assert!(120 > visiveis, "o destino precisa estar fora da tela");
        assert_eq!(shell.editor_scroll_line(), 0);

        assert!(shell.open_location(Path::new("Longo.java"), 120, 0).is_ok());
        // A revelação acontece na pintura, que é onde a altura do editor existe.
        let _ = shell.paint(size);

        let topo = shell.editor_scroll_line();
        assert!(
            topo <= 120 && 120 < topo + visiveis,
            "a linha 120 precisa ficar visível; topo={topo}, visíveis={visiveis}"
        );
        // E rolou o mínimo necessário, não saltou para o fim do arquivo.
        assert!(topo > 0);
    }

    /// A linha de destino fica destacada até o cursor sair de lá.
    #[test]
    fn the_navigated_line_is_highlighted_until_the_cursor_moves() {
        let mut shell = test_shell();
        let texto = (0..60)
            .map(|linha| format!("linha {linha}"))
            .collect::<Vec<_>>()
            .join("
");
        shell.editor.open_memory("Longo.java", &texto);
        let size = Size::new(1280.0, 800.0);
        let destaque = Theme::default().colors.highlight;
        let destacadas = |shell: &mut IdeShell| {
            shell
                .paint(size)
                .iter()
                .filter(|command| {
                    matches!(command, PaintCommand::FillRect(fill) if fill.color == destaque)
                })
                .count()
        };
        assert_eq!(destacadas(&mut shell), 0, "sem navegação, nada destacado");

        assert!(shell.open_location(Path::new("Longo.java"), 30, 0).is_ok());
        assert_eq!(destacadas(&mut shell), 1, "o destino fica destacado");

        // Clicar em outro lugar tira o destaque, sem ninguém precisar apagá-lo.
        let editor_x = ACTIVITY_WIDTH + SIDEBAR_WIDTH;
        shell.pointer_down(
            Point::new(
                editor_x + EDITOR_GUTTER + 2.0 * EDITOR_CHAR_WIDTH,
                TITLE_HEIGHT + TAB_HEIGHT + 3.0 * EDITOR_LINE_HEIGHT + 5.0,
            ),
            size,
        );
        assert_eq!(destacadas(&mut shell), 0, "mover o cursor encerra o destaque");
    }

    #[test]
    fn open_location_opens_file_and_positions_cursor() {
        let mut shell = test_shell();
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
        assert!(shell.open_location(&path, 1, 3).is_ok());
        let position = line_column(shell.active_text().unwrap_or_default(), shell.cursor_offset);
        assert_eq!(position, (1, 3));
        assert_eq!(shell.focus(), ShellFocus::Editor);
    }


    #[test]
    fn project_menu_requests_build_and_reimport() {
        let mut shell = test_shell();
        let size = Size::new(1_000.0, 700.0);

        shell.pointer_down(Point::new(200.0, 10.0), size);
        shell.pointer_down(Point::new(200.0, TITLE_HEIGHT + 10.0), size);
        assert!(shell.take_build_project_request());
        assert!(!shell.take_reimport_project_request());

        shell.pointer_down(Point::new(200.0, 10.0), size);
        shell.pointer_down(Point::new(200.0, TITLE_HEIGHT + 38.0), size);
        assert!(shell.take_reimport_project_request());
        assert!(!shell.take_build_project_request());
    }

    /// A interface não define cor própria: todas vêm do tema da ERLibUi.
    ///
    /// Sem esta trava, uma cor solta volta a aparecer na primeira pressa — foi
    /// o que aconteceu com a barra de status e com o marcador de breakpoint.
    #[test]
    fn the_interface_does_not_hardcode_colors() {
        let source = include_str!("lib.rs");
        let tests_start = source
            .find("\nmod tests {")
            .unwrap_or_else(|| panic!("módulo de testes não encontrado"));
        for (number, line) in source[..tests_start].lines().enumerate() {
            assert!(
                !line.contains("Color::rgba"),
                "linha {} usa cor fixa; use um token de `Theme`: {}",
                number + 1,
                line.trim()
            );
        }
    }

    #[test]
    fn the_theme_comes_from_the_library_and_reaches_its_components() {
        let mut shell = test_shell();
        let size = Size::new(1_000.0, 700.0);
        assert_eq!(shell.theme(), &Theme::dark(), "o tema padrão é o da lib");

        let dark: Vec<Color> = shell
            .paint(size)
            .iter()
            .filter_map(|command| match command {
                PaintCommand::DrawText(text) => Some(text.color),
                _ => None,
            })
            .collect();

        shell.set_theme(Theme::high_contrast());
        let contrast: Vec<Color> = shell
            .paint(size)
            .iter()
            .filter_map(|command| match command {
                PaintCommand::DrawText(text) => Some(text.color),
                _ => None,
            })
            .collect();

        assert_ne!(dark, contrast, "trocar o tema muda o que é pintado");
        assert!(
            contrast.contains(&Theme::high_contrast().colors.text),
            "o texto usa o token do tema ativo"
        );
        // A barra de menus é um componente da lib: ela recebe o tema pelo
        // contexto de pintura, sem a IDE redesenhá-la.
        assert!(
            shell.paint(size).iter().any(|command| matches!(
                command,
                PaintCommand::DrawText(text)
                    if text.text == "Arquivo"
                        && text.color == Theme::high_contrast().colors.text
            )),
            "os componentes da biblioteca seguem o tema da aplicação"
        );
    }

    #[test]
    fn status_bar_uses_palette_colors_with_readable_contrast() {
        let mut shell = test_shell();
        shell.set_status_message("Pronto");
        let size = Size::new(1_000.0, 700.0);
        let colors = Theme::default().colors;
        let geometry = shell.geometry(size);
        let commands = shell.paint(size);

        let background = commands.iter().find_map(|command| match command {
            PaintCommand::FillRect(rect)
                if rect.rect.origin.y == geometry.content_bottom && rect.rect.size.height > 1.0 =>
            {
                Some(rect.color)
            }
            _ => None,
        });
        assert_eq!(
            background,
            Some(colors.surface),
            "a barra usa a superfície da paleta, não a cor de destaque"
        );

        let text_color = commands.iter().find_map(|command| match command {
            PaintCommand::DrawText(text) if text.text.starts_with("Pronto") => Some(text.color),
            _ => None,
        });
        assert_eq!(
            text_color,
            Some(colors.text),
            "o texto usa a cor de texto da paleta, não branco puro"
        );
        assert!(
            contrast_ratio(colors.text, colors.surface) >= 7.0,
            "texto e fundo da barra precisam de contraste confortável"
        );
    }

    /// Razão de contraste WCAG entre duas cores opacas.
    fn contrast_ratio(first: Color, second: Color) -> f32 {
        fn luminance(color: Color) -> f32 {
            fn channel(value: f32) -> f32 {
                if value <= 0.03928 {
                    value / 12.92
                } else {
                    ((value + 0.055) / 1.055).powf(2.4)
                }
            }
            0.2126 * channel(color.red)
                + 0.7152 * channel(color.green)
                + 0.0722 * channel(color.blue)
        }
        let first = luminance(first);
        let second = luminance(second);
        (first.max(second) + 0.05) / (first.min(second) + 0.05)
    }

    #[test]
    fn status_bar_shows_the_imported_project_summary() {
        let mut shell = test_shell();
        shell.set_project_summary(Some("Maven • demo • 2 módulo(s)".to_owned()));
        assert_eq!(shell.project_summary(), Some("Maven • demo • 2 módulo(s)"));
        assert!(
            shell
                .paint(Size::new(1_000.0, 700.0))
                .iter()
                .any(|command| matches!(
                    command,
                    PaintCommand::DrawText(text) if text.text.contains("Maven • demo • 2 módulo(s)")
                ))
        );
    }

    #[test]
    fn settings_menu_opens_compiler_and_vm_page() {
        let mut shell = test_shell();
        let size = Size::new(1_000.0, 700.0);
        shell.pointer_down(Point::new(340.0, 10.0), size);
        assert!(shell.take_open_settings_request());

        shell.open_settings_dialog(vec!["JDK 8".to_owned(), "JDK 17".to_owned()], 0);
        assert!(shell.settings_dialog_open());
        let paint = shell.paint(size);
        let labels = paint
            .iter()
            .filter_map(|command| match command {
                PaintCommand::DrawText(text) => Some(text.text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert!(labels.contains(&"Configurações"));
        assert!(labels.contains(&"Compilador e VM"));
        assert!(labels.contains(&"JDK 8"));
        assert!(labels.contains(&"Procurar..."));
        assert!(labels.contains(&"PowerShell"));
        assert!(labels.contains(&"ER IDE"));
        assert!(
            paint
                .iter()
                .any(|command| matches!(command, PaintCommand::LayerBreak))
        );
    }

    #[test]
    fn settings_jdk_combo_and_browse_button_emit_requests() {
        let mut shell = test_shell();
        let size = Size::new(1_000.0, 700.0);
        shell.open_settings_dialog(vec!["JDK 8".to_owned(), "JDK 17".to_owned()], 0);
        shell
            .settings_modal
            .layout(&LayoutContext::default(), Rect::new(0.0, 0.0, size.width, size.height));
        let geometry = settings_dialog_geometry(shell.settings_modal.panel_bounds());
        shell.pointer_down(
            Point::new(
                geometry.combo.origin.x + 10.0,
                geometry.combo.origin.y + 10.0,
            ),
            size,
        );
        shell.pointer_down(
            Point::new(
                geometry.combo.origin.x + 10.0,
                geometry.combo.origin.y + geometry.combo.size.height + 28.0 + 5.0,
            ),
            size,
        );
        assert_eq!(shell.take_settings_jdk_result(), Some(1));

        shell.pointer_down(
            Point::new(
                geometry.browse.origin.x + 10.0,
                geometry.browse.origin.y + 10.0,
            ),
            size,
        );
        assert!(shell.take_browse_jdk_request());
    }
}
