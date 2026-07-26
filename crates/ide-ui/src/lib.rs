#![doc = "Shell visual e interativo da IDE baseado no ERLibUi."]

use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    path::{Path, PathBuf},
};

use ide_domain::{
    CompletionItem, CompletionRequest, DocumentId, DocumentSnapshot, OutlineItem,
    SyntaxHighlightKind, SyntaxSnapshot, TextPosition as DomainTextPosition,
};
use ide_terminal::{ShellKind, TerminalSession};
use ide_text::EditorSession;
use ide_workspace::{FileNode, WorkspaceError};
use ui_api::{EventContext, LayoutContext, PaintContext, Widget};
use ui_components::{
    Button, ComboBox, ComboBoxItem, Icon, IconTint, MenuBar, MenuBarItem, MenuItem, ModalHost,
    TextInput,
};
use ui_core::{
    Color, ColorTokens, EventResult, FontId, KeyEvent, Modifiers, Point, PointerButton,
    PointerEvent, Rect, Size, Theme, UiEvent, WidgetAction, WidgetId,
};
use ui_render_api::{
    DrawTextCommand, FillCircleCommand, FillRectCommand, PaintCommand, StrokeCircleCommand,
    StrokeRectCommand,
};

const ACTIVITY_WIDTH: f32 = 48.0;
const SIDEBAR_WIDTH: f32 = 260.0;
const SIDEBAR_MIN_WIDTH: f32 = 160.0;
const SIDEBAR_RESIZE_HIT: f32 = 5.0;
const TITLE_HEIGHT: f32 = 36.0;
const TAB_HEIGHT: f32 = 38.0;
const EXPLORER_ROW_HEIGHT: f32 = 23.0;
const EXPLORER_TOP: f32 = 106.0;
const EDITOR_LINE_HEIGHT: f32 = 22.0;
const EDITOR_GUTTER: f32 = 55.0;
const TAB_WIDTH: f32 = 140.0;
const TERMINAL_DEFAULT_HEIGHT: f32 = 180.0;
const TERMINAL_MIN_HEIGHT: f32 = 120.0;
const TERMINAL_COLLAPSED_HEIGHT: f32 = 30.0;
const TERMINAL_RESIZE_HIT: f32 = 5.0;
const TERMINAL_CHAR_WIDTH: f32 = 8.4;
const DIALOG_ROW_HEIGHT: f32 = 34.0;
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

#[derive(Clone, Copy)]
enum ScrollbarDrag {
    Editor { pointer_offset: f32 },
    Terminal { pointer_offset: f32 },
    ExplorerHorizontal { pointer_offset: f32 },
    ExplorerVertical { pointer_offset: f32 },
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

struct SelectionDialog {
    title: String,
    items: Vec<String>,
    selected: usize,
}

struct SettingsDialog {
    message: Option<String>,
}

pub struct IdeShell {
    workspace_name: String,
    workspace: FileNode,
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
    terminal_resizing: bool,
    sidebar_resizing: bool,
    scrollbar_drag: Option<ScrollbarDrag>,
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
    selection_dialog: Option<SelectionDialog>,
    selection_result: Option<usize>,
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
        Self {
            workspace_name,
            workspace,
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
            terminal_resizing: false,
            sidebar_resizing: false,
            scrollbar_drag: None,
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
            selection_dialog: None,
            selection_result: None,
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
            debug_requests: Vec::new(),
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
    pub fn set_status_message(&mut self, message: impl Into<String>) {
        self.status_message = message.into();
    }
    pub fn open_selection_dialog(
        &mut self,
        title: impl Into<String>,
        items: Vec<String>,
        selected: usize,
    ) {
        self.selection_dialog = Some(SelectionDialog {
            title: title.into(),
            selected: selected.min(items.len().saturating_sub(1)),
            items,
        });
        self.selection_result = None;
    }
    pub fn take_selection_result(&mut self) -> Option<usize> {
        self.selection_result.take()
    }
    pub const fn selection_dialog_open(&self) -> bool {
        self.selection_dialog.is_some()
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
                    highlight.kind == SyntaxHighlightKind::Type
                        && position_in_range(line, column, highlight.range)
                })
            })
    }
    pub fn tab_count(&self) -> usize {
        self.editor.tabs().count()
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
        self.terminal_resizing
    }
    pub const fn sidebar_resizing(&self) -> bool {
        self.sidebar_resizing
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
            Point::new(panel.origin.x + 12.0, geometry.frames_top - 20.0),
            colors.muted_text,
            12.0,
        ));
        for (index, frame) in self.debug.frames.iter().take(8).enumerate() {
            let y = geometry.frames_top + index as f32 * DEBUG_ROW_HEIGHT;
            if index == self.debug.selected_frame {
                commands.push(fill(
                    Rect::new(
                        panel.origin.x + 6.0,
                        y - 2.0,
                        panel.size.width - 12.0,
                        DEBUG_ROW_HEIGHT,
                    ),
                    colors.elevated,
                ));
            }
            let line = frame
                .location
                .as_ref()
                .map(|(_, line)| format!(":{}", line + 1))
                .unwrap_or_default();
            commands.push(label(
                &ellipsize(&format!("{}{line}", frame.name), 38),
                Point::new(panel.origin.x + 12.0, y),
                colors.text,
                12.0,
            ));
        }

        commands.push(label(
            "Variáveis",
            Point::new(panel.origin.x + 12.0, geometry.variables_top - 20.0),
            colors.muted_text,
            12.0,
        ));
        let rows = ((panel.origin.y + panel.size.height - geometry.variables_top)
            / DEBUG_ROW_HEIGHT)
            .max(0.0) as usize;
        for (index, variable) in self.debug.variables.iter().take(rows).enumerate() {
            let y = geometry.variables_top + index as f32 * DEBUG_ROW_HEIGHT;
            commands.push(label(
                &ellipsize(&format!("{} = {}", variable.name, variable.value), 38),
                Point::new(panel.origin.x + 12.0, y),
                colors.text,
                12.0,
            ));
        }
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
        if point.y >= geometry.frames_top && point.y < geometry.frames_top + geometry.frames_height
        {
            let row = ((point.y - geometry.frames_top) / DEBUG_ROW_HEIGHT).floor() as usize;
            if row < self.debug.frames.len() {
                self.debug.selected_frame = row;
                self.debug_requests.push(DebugRequest::SelectFrame(row));
                if let Some((path, line)) = self.debug.frames[row].location.clone() {
                    let _ = self.open_location(&path, line as usize, 0);
                }
            }
        }
    }

    /// Linha do documento sob o ponteiro, considerando a rolagem atual.
    fn line_at_point(&self, point: Point, editor_top: f32) -> usize {
        self.editor_scroll_line
            + ((point.y - editor_top - 15.0) / EDITOR_LINE_HEIGHT)
                .floor()
                .max(0.0) as usize
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
        if self.selection_dialog.take().is_some() {
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
        if self.selection_dialog.is_some() {
            self.selection_dialog_pointer_down(point, size);
            return;
        }
        if point.y < TITLE_HEIGHT && self.action_buttons_pointer_down(point, size) {
            return;
        }
        self.menu_bar.layout(
            &LayoutContext,
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
        let terminal_track = self.terminal_scrollbar_rect(size);
        if !self.terminal_minimized && terminal_track.contains(point) {
            let active = self.active_terminal;
            let metrics = scrollbar_metrics(
                terminal_track,
                self.terminals[active].session.line_count(),
                self.terminal_visible_lines(size),
                self.terminals[active].scroll_line,
            );
            if let Some(metrics) = metrics {
                let pointer_offset = if metrics.thumb.contains(point) {
                    point.y - metrics.thumb.origin.y
                } else {
                    metrics.thumb.size.height / 2.0
                };
                self.terminals[active].scroll_line =
                    offset_from_scrollbar(point.y - pointer_offset, metrics);
                self.terminals[active].follow_output =
                    self.terminals[active].scroll_line >= metrics.max_offset;
                self.scrollbar_drag = Some(ScrollbarDrag::Terminal { pointer_offset });
            }
            return;
        }
        let editor_track = self.editor_scrollbar_rect(size);
        if editor_track.contains(point) {
            let total = self.active_text().map_or(0, |text| text.lines().count());
            let visible = self.editor_visible_lines(size);
            if let Some(metrics) =
                scrollbar_metrics(editor_track, total, visible, self.editor_scroll_line)
            {
                let pointer_offset = if metrics.thumb.contains(point) {
                    point.y - metrics.thumb.origin.y
                } else {
                    metrics.thumb.size.height / 2.0
                };
                self.editor_scroll_line = offset_from_scrollbar(point.y - pointer_offset, metrics);
                self.scrollbar_drag = Some(ScrollbarDrag::Editor { pointer_offset });
            }
            return;
        }
        let explorer_track = self.explorer_horizontal_scrollbar_rect(size);
        if explorer_track.contains(point) {
            if let Some(metrics) = self.explorer_horizontal_metrics(size) {
                let pointer_offset = if metrics.thumb.contains(point) {
                    point.x - metrics.thumb.origin.x
                } else {
                    metrics.thumb.size.width / 2.0
                };
                self.explorer_scroll_x =
                    offset_from_horizontal_scrollbar(point.x - pointer_offset, metrics);
                self.scrollbar_drag = Some(ScrollbarDrag::ExplorerHorizontal { pointer_offset });
            }
            return;
        }
        if (point.x - editor_x).abs() <= SIDEBAR_RESIZE_HIT
            && point.y >= TITLE_HEIGHT
            && point.y < geometry.content_bottom
        {
            self.sidebar_resizing = true;
            return;
        }
        let explorer_vertical_track = self.explorer_vertical_scrollbar_rect(size);
        if explorer_vertical_track.contains(point) {
            let total = self.visible_entries().len();
            let visible = self.explorer_visible_lines(size);
            if let Some(metrics) = scrollbar_metrics(
                explorer_vertical_track,
                total,
                visible,
                self.explorer_scroll_line,
            ) {
                let pointer_offset = if metrics.thumb.contains(point) {
                    point.y - metrics.thumb.origin.y
                } else {
                    metrics.thumb.size.height / 2.0
                };
                self.explorer_scroll_line =
                    offset_from_scrollbar(point.y - pointer_offset, metrics);
                self.scrollbar_drag = Some(ScrollbarDrag::ExplorerVertical { pointer_offset });
            }
            return;
        }
        if !self.terminal_minimized
            && (point.y - geometry.editor_bottom).abs() <= TERMINAL_RESIZE_HIT
            && point.x >= editor_x
        {
            self.terminal_resizing = true;
            return;
        }
        if point.y >= TITLE_HEIGHT && point.y < TITLE_HEIGHT + TAB_HEIGHT && point.x >= editor_x {
            let index = ((point.x - editor_x) / TAB_WIDTH).floor() as usize;
            let tab = self.editor.tabs().nth(index).map(|document| document.id);
            if let Some(id) = tab {
                let within_tab = (point.x - editor_x) - index as f32 * TAB_WIDTH;
                if within_tab >= TAB_WIDTH - 30.0 {
                    if self.editor.close(id).is_ok() {
                        self.syntax_snapshots.remove(&id);
                        self.cursor_offset = self.active_text().map_or(0, str::len);
                        self.status_message = "Tab closed".to_owned();
                    }
                } else {
                    let _ = self.editor.activate(id);
                    self.cursor_offset = 0;
                    self.focus = ShellFocus::Editor;
                }
            }
            return;
        }
        if point.x >= ACTIVITY_WIDTH && point.x < editor_x && point.y >= EXPLORER_TOP {
            let row = self.explorer_scroll_line
                + ((point.y - EXPLORER_TOP) / EXPLORER_ROW_HEIGHT).floor() as usize;
            let entry = self
                .visible_entries()
                .get(row)
                .map(|(_, node)| (node.path.clone(), node.is_directory));
            if let Some((path, is_directory)) = entry {
                self.focus = ShellFocus::Explorer;
                if is_directory {
                    if !self.expanded.remove(&path) {
                        self.expanded.insert(path);
                    }
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
        if point.x >= editor_x
            && point.x < editor_x + EDITOR_GUTTER
            && point.y >= geometry.content_top
            && point.y < geometry.editor_bottom
        {
            // A calha é a área de breakpoints, e não posiciona o cursor.
            let line = self.line_at_point(point, geometry.content_top);
            if let Some(path) = self.editor.active().map(|document| document.path.clone())
                && self
                    .active_text()
                    .is_some_and(|text| line < text.lines().count().max(1))
            {
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
            if point.y < geometry.editor_bottom + 30.0 {
                let index = ((point.x - editor_x) / 110.0).floor().max(0.0) as usize;
                if index < self.terminals.len() {
                    self.active_terminal = index;
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
        self.focus = ShellFocus::Editor;
        self.status_message = format!("Definition: {}:{}:{}", path.display(), line + 1, column + 1);
        Ok(id)
    }

    pub fn pointer_move(&mut self, point: Point, size: Size) -> bool {
        if self.settings_modal.is_open() || self.selection_dialog.is_some() {
            return false;
        }
        let geometry = self.geometry(size);
        if let Some(drag) = self.scrollbar_drag {
            match drag {
                ScrollbarDrag::Editor { pointer_offset } => {
                    let track = self.editor_scrollbar_rect(size);
                    let total = self.active_text().map_or(0, |text| text.lines().count());
                    if let Some(metrics) = scrollbar_metrics(
                        track,
                        total,
                        self.editor_visible_lines(size),
                        self.editor_scroll_line,
                    ) {
                        self.editor_scroll_line =
                            offset_from_scrollbar(point.y - pointer_offset, metrics);
                    }
                }
                ScrollbarDrag::Terminal { pointer_offset } => {
                    let track = self.terminal_scrollbar_rect(size);
                    let active = self.active_terminal;
                    if let Some(metrics) = scrollbar_metrics(
                        track,
                        self.terminals[active].session.line_count(),
                        self.terminal_visible_lines(size),
                        self.terminals[active].scroll_line,
                    ) {
                        self.terminals[active].scroll_line =
                            offset_from_scrollbar(point.y - pointer_offset, metrics);
                        self.terminals[active].follow_output =
                            self.terminals[active].scroll_line >= metrics.max_offset;
                    }
                }
                ScrollbarDrag::ExplorerHorizontal { pointer_offset } => {
                    if let Some(metrics) = self.explorer_horizontal_metrics(size) {
                        self.explorer_scroll_x =
                            offset_from_horizontal_scrollbar(point.x - pointer_offset, metrics);
                    }
                }
                ScrollbarDrag::ExplorerVertical { pointer_offset } => {
                    let track = self.explorer_vertical_scrollbar_rect(size);
                    if let Some(metrics) = scrollbar_metrics(
                        track,
                        self.visible_entries().len(),
                        self.explorer_visible_lines(size),
                        self.explorer_scroll_line,
                    ) {
                        self.explorer_scroll_line =
                            offset_from_scrollbar(point.y - pointer_offset, metrics);
                    }
                }
            }
            return true;
        }
        if self.terminal_selecting {
            let position = self.terminal_position_at(point, size);
            if let Some(selection) = self.terminal_selection.as_mut() {
                selection.focus = position;
            }
            return true;
        }
        if self.terminal_resizing {
            let max_height =
                (geometry.content_bottom - geometry.content_top - 100.0).max(TERMINAL_MIN_HEIGHT);
            self.terminal_height =
                (geometry.content_bottom - point.y).clamp(TERMINAL_MIN_HEIGHT, max_height);
            self.terminal_last_height = self.terminal_height;
            return true;
        }
        if self.sidebar_resizing {
            self.sidebar_width = (point.x - ACTIVITY_WIDTH).clamp(
                SIDEBAR_MIN_WIDTH,
                (size.width - 320.0).max(SIDEBAR_MIN_WIDTH),
            );
            return true;
        }
        !self.terminal_minimized
            && (point.y - geometry.editor_bottom).abs() <= TERMINAL_RESIZE_HIT
            && point.x >= ACTIVITY_WIDTH + self.sidebar_width(size)
    }

    pub fn pointer_up(&mut self) {
        self.terminal_resizing = false;
        self.sidebar_resizing = false;
        self.scrollbar_drag = None;
        self.terminal_selecting = false;
    }

    pub fn scroll(&mut self, point: Point, delta_lines: isize, size: Size) {
        if self.settings_modal.is_open() || self.selection_dialog.is_some() {
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

    fn explorer_content_width(&self) -> f32 {
        self.visible_entries()
            .into_iter()
            .map(|(depth, node)| {
                let name = node
                    .path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("?");
                28.0 + (depth * 2 + name.chars().count() + 2) as f32 * TERMINAL_CHAR_WIDTH
            })
            .fold(0.0, f32::max)
    }

    fn explorer_horizontal_metrics(&self, size: Size) -> Option<HorizontalScrollbarMetrics> {
        let track = self.explorer_horizontal_scrollbar_rect(size);
        horizontal_scrollbar_metrics(
            track,
            self.explorer_content_width(),
            (track.size.width - 28.0).max(1.0),
            self.explorer_scroll_x,
        )
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
        if self.selection_dialog.is_some() {
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
        if let Some(dialog) = self.selection_dialog.as_mut() {
            match key.to_ascii_lowercase().as_str() {
                "arrowdown" if !dialog.items.is_empty() => {
                    dialog.selected = (dialog.selected + 1).min(dialog.items.len() - 1);
                }
                "arrowup" => dialog.selected = dialog.selected.saturating_sub(1),
                "enter" if !dialog.items.is_empty() => {
                    self.selection_result = Some(dialog.selected);
                    self.selection_dialog = None;
                }
                _ => {}
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
            + ((point.y - editor_top - 15.0) / EDITOR_LINE_HEIGHT)
                .floor()
                .max(0.0) as usize;
        let column = ((point.x - editor_x - EDITOR_GUTTER) / 8.4)
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

    pub fn paint(&self, size: Size) -> Vec<PaintCommand> {
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
            // A barra de status usa a mesma superfície dos demais painéis,
            // separada por uma linha de borda. Em destaque, ela competia com o
            // conteúdo e deixava o texto com contraste baixo demais para ler.
            fill(
                Rect::new(0.0, geo.content_bottom, size.width, 24.0),
                colors.surface,
            ),
            fill(
                Rect::new(0.0, geo.content_bottom, size.width, 1.0),
                colors.border,
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
        let explorer_visible = self.explorer_visible_lines(size);
        let explorer_total = self.visible_entries().len();
        let explorer_offset = self
            .explorer_scroll_line
            .min(explorer_total.saturating_sub(explorer_visible));
        for (index, (depth, node)) in self
            .visible_entries()
            .into_iter()
            .skip(explorer_offset)
            .take(explorer_visible)
            .enumerate()
        {
            let name = node
                .path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("?");
            let marker = if node.is_directory {
                if self.expanded.contains(&node.path) {
                    "▾"
                } else {
                    "▸"
                }
            } else {
                " "
            };
            commands.push(label(
                &format!("{}{} {}", "  ".repeat(depth), marker, name),
                Point::new(
                    ACTIVITY_WIDTH + 14.0 - self.explorer_scroll_x,
                    EXPLORER_TOP + index as f32 * EXPLORER_ROW_HEIGHT,
                ),
                colors.text,
                14.0,
            ));
        }
        commands.push(PaintCommand::PopClip);
        commands.extend(horizontal_scrollbar(
            self.explorer_horizontal_scrollbar_rect(size),
            self.explorer_content_width(),
            (self.sidebar_width(size) - 28.0).max(1.0),
            self.explorer_scroll_x,
            colors,
        ));
        commands.extend(scrollbar(
            self.explorer_vertical_scrollbar_rect(size),
            explorer_total,
            explorer_visible,
            explorer_offset,
            colors,
        ));
        commands.push(fill(
            Rect::new(
                editor_x - 1.0,
                TITLE_HEIGHT,
                1.0,
                geo.content_bottom - TITLE_HEIGHT,
            ),
            colors.border,
        ));
        commands.push(PaintCommand::PushClip(Rect::new(
            editor_x,
            TITLE_HEIGHT,
            geo.editor_width,
            TAB_HEIGHT,
        )));
        for (index, document) in self.editor.tabs().enumerate() {
            let x = editor_x + index as f32 * TAB_WIDTH;
            if Some(document.id) == self.editor.active_id() {
                commands.push(fill(
                    Rect::new(x, TITLE_HEIGHT, TAB_WIDTH, TAB_HEIGHT),
                    colors.background,
                ));
                commands.push(fill(
                    Rect::new(x, TITLE_HEIGHT, TAB_WIDTH, 2.0),
                    colors.accent,
                ));
            }
            let mut title = document
                .path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("?")
                .to_owned();
            if document.buffer.is_dirty() {
                title.push_str(" ●");
            }
            let title = ellipsize(&title, 13);
            commands.push(PaintCommand::PushClip(Rect::new(
                x + 8.0,
                TITLE_HEIGHT,
                TAB_WIDTH - 38.0,
                TAB_HEIGHT,
            )));
            commands.push(label(
                &title,
                Point::new(x + 14.0, TITLE_HEIGHT + 11.0),
                colors.text,
                14.0,
            ));
            commands.push(PaintCommand::PopClip);
            commands.push(label(
                "x",
                Point::new(x + TAB_WIDTH - 22.0, TITLE_HEIGHT + 10.0),
                colors.muted_text,
                14.0,
            ));
        }
        commands.push(PaintCommand::PopClip);
        commands.push(PaintCommand::PushClip(Rect::new(
            editor_x,
            geo.content_top,
            geo.editor_width,
            geo.editor_height,
        )));
        // A calha é a faixa de breakpoints e precisa ser visível: sem contraste,
        // não há como saber onde clicar para marcar uma linha.
        commands.push(fill(
            Rect::new(editor_x, geo.content_top, EDITOR_GUTTER, geo.editor_height),
            colors.background,
        ));
        commands.push(fill(
            Rect::new(
                editor_x + EDITOR_GUTTER - 1.0,
                geo.content_top,
                1.0,
                geo.editor_height,
            ),
            colors.border,
        ));
        if let Some(text) = self.active_text() {
            let visible = (geo.editor_height / EDITOR_LINE_HEIGHT).ceil() as usize;
            let active_path = self
                .editor
                .active()
                .map(|document| document.path.clone())
                .unwrap_or_default();
            let breakpoints = self.breakpoints.get(&active_path);
            let stopped_line = self
                .debug
                .stopped_at
                .as_ref()
                .filter(|(path, _)| path == &active_path)
                .map(|(_, line)| *line);
            for (index, line) in text
                .lines()
                .skip(self.editor_scroll_line)
                .take(visible)
                .enumerate()
            {
                let y = geo.content_top + 15.0 + index as f32 * EDITOR_LINE_HEIGHT;
                let document_line = (index + self.editor_scroll_line) as u32;
                if stopped_line == Some(document_line) {
                    commands.push(fill(
                        Rect::new(editor_x, y - 4.0, geo.editor_width, EDITOR_LINE_HEIGHT),
                        Color::rgba(0.24, 0.20, 0.06, 1.0),
                    ));
                }
                if breakpoints.is_some_and(|lines| lines.contains(&document_line)) {
                    let center = Point::new(editor_x + 42.0, y + 5.0);
                    // Confirmado pelo alvo: círculo cheio. Ainda não registrado
                    // — sem sessão, ou classe não carregada — apenas o contorno.
                    if self.breakpoint_is_verified(&active_path, document_line) {
                        commands.push(PaintCommand::FillCircle(FillCircleCommand {
                            center,
                            radius: 5.0,
                            color: colors.danger,
                        }));
                    } else {
                        commands.push(PaintCommand::StrokeCircle(StrokeCircleCommand {
                            center,
                            radius: 4.5,
                            color: colors.danger,
                            width: 1.6,
                        }));
                    }
                }
                commands.push(label(
                    &(index + self.editor_scroll_line + 1).to_string(),
                    Point::new(editor_x + 12.0, y),
                    colors.muted_text,
                    13.0,
                ));
                if let Some(snapshot) = self
                    .active_document()
                    .and_then(|id| self.syntax_snapshots.get(&id))
                    .filter(|snapshot| {
                        self.editor
                            .document(snapshot.document_id)
                            .is_some_and(|document| document.buffer.revision() == snapshot.version)
                    })
                {
                    commands.extend(highlighted_line(
                        line,
                        index + self.editor_scroll_line,
                        Point::new(editor_x + EDITOR_GUTTER, y),
                        snapshot,
                        colors,
                    ));
                } else {
                    commands.push(label(
                        line,
                        Point::new(editor_x + EDITOR_GUTTER, y),
                        syntax_color(line, colors.text, colors.accent, colors.muted_text),
                        15.0,
                    ));
                }
            }
            if self.focus == ShellFocus::Editor {
                let (line, column) = line_column(text, self.cursor_offset);
                if line >= self.editor_scroll_line && line < self.editor_scroll_line + visible {
                    commands.push(fill(
                        Rect::new(
                            editor_x + EDITOR_GUTTER + column as f32 * 8.4,
                            geo.content_top
                                + 14.0
                                + (line - self.editor_scroll_line) as f32 * EDITOR_LINE_HEIGHT,
                            2.0,
                            18.0,
                        ),
                        colors.text,
                    ));
                }
            }
            commands.extend(scrollbar(
                Rect::new(
                    editor_x + geo.editor_width - 10.0,
                    geo.content_top,
                    10.0,
                    geo.editor_height,
                ),
                text.lines().count(),
                visible,
                self.editor_scroll_line,
                colors,
            ));
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
            for (index, terminal) in self.terminals.iter().enumerate() {
                let profile = terminal.session.selected_profile();
                let x = editor_x + index as f32 * 110.0;
                if index == self.active_terminal {
                    commands.push(fill(
                        Rect::new(x, geo.editor_bottom, 110.0, 30.0),
                        colors.elevated,
                    ));
                    commands.push(fill(
                        Rect::new(x, geo.editor_bottom, 110.0, 2.0),
                        colors.accent,
                    ));
                }
                commands.push(label(
                    profile.kind.label(),
                    Point::new(x + 10.0, geo.editor_bottom + 8.0),
                    colors.text,
                    13.0,
                ));
            }
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
                        Color::rgba(0.22, 0.42, 0.72, 0.65),
                    ));
                }
                commands.push(label(
                    &line.text,
                    Point::new(
                        editor_x + 14.0,
                        geo.editor_bottom + 68.0 + index as f32 * EDITOR_LINE_HEIGHT,
                    ),
                    if line.is_error {
                        Color::rgba(0.95, 0.40, 0.42, 1.0)
                    } else {
                        colors.muted_text
                    },
                    14.0,
                ));
            }
            commands.extend(scrollbar(
                Rect::new(
                    editor_x + geo.editor_width - 10.0,
                    geo.editor_bottom + 60.0,
                    10.0,
                    geo.terminal_height - 60.0,
                ),
                active_terminal.session.line_count(),
                terminal_visible,
                terminal_offset,
                colors,
            ));
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
            let popup_x = (editor_x + EDITOR_GUTTER + column as f32 * 8.4)
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
        commands.push(label(
            &format!(
                "{}  •  UTF-8  •  Ln {}, Col {}{}",
                self.status_message,
                position.0 + 1,
                position.1 + 1,
                self.project_summary
                    .as_deref()
                    .map(|summary| format!("  •  {summary}"))
                    .unwrap_or_default()
            ),
            Point::new(12.0, geo.content_bottom + 5.0),
            colors.text,
            12.0,
        ));
        let mut menu_bar = self.menu_bar.clone();
        menu_bar.layout(
            &LayoutContext,
            Rect::new(82.0, 0.0, (size.width - 82.0).max(0.0), TITLE_HEIGHT),
        );
        let mut menu_paint = PaintContext::with_theme(self.theme);
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
        let mut actions = PaintContext::with_theme(self.theme);
        for (button, rect) in [
            (&mut stop, rects[0]),
            (&mut run, rects[1]),
            (&mut debug, rects[2]),
        ] {
            button.layout(&LayoutContext, rect);
            button.paint(&mut actions);
        }
        commands.extend(actions.into_commands());
        if self.settings_modal.is_open() {
            let mut modal = self.settings_modal.clone();
            modal.layout(&LayoutContext, Rect::new(0.0, 0.0, size.width, size.height));
            let geometry = settings_dialog_geometry(modal.panel_bounds());
            let mut modal_paint = PaintContext::with_theme(self.theme);
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
            let mut component_paint = PaintContext::with_theme(self.theme);
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
                    combo.layout(&LayoutContext, geometry.combo);
                    combo.paint(&mut component_paint);
                    let mut browse = self.jdk_browse_button.clone();
                    browse.layout(&LayoutContext, geometry.browse);
                    browse.paint(&mut component_paint);
                }
                SettingsPage::Debug => {
                    commands.extend(self.paint_debug_settings(&geometry, colors));
                    let mut host = self.debug_host.clone();
                    host.layout(&LayoutContext, geometry.debug_host);
                    host.paint(&mut component_paint);
                    let mut port = self.debug_port.clone();
                    port.layout(&LayoutContext, geometry.debug_port);
                    port.paint(&mut component_paint);
                    let mut attach = self.debug_attach_button.clone();
                    attach.layout(&LayoutContext, geometry.debug_attach);
                    attach.paint(&mut component_paint);
                }
            }
            let mut close = self.settings_close_button.clone();
            close.layout(&LayoutContext, geometry.close);
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
                    Color::rgba(0.95, 0.55, 0.42, 1.0),
                    13.0,
                ));
            }
        }
        if let Some(dialog) = &self.selection_dialog {
            let (dialog_rect, rows_top, buttons_top) =
                selection_dialog_geometry(size, dialog.items.len());
            commands.push(fill(
                Rect::new(0.0, 0.0, size.width, size.height),
                Color::rgba(0.0, 0.0, 0.0, 0.55),
            ));
            commands.push(fill(dialog_rect, colors.elevated));
            commands.push(stroke(dialog_rect, colors.accent));
            commands.push(label(
                &dialog.title,
                Point::new(dialog_rect.origin.x + 20.0, dialog_rect.origin.y + 18.0),
                colors.text,
                18.0,
            ));
            if dialog.items.is_empty() {
                commands.push(label(
                    "Nenhum JDK detectado. Configure JAVA_HOME.",
                    Point::new(dialog_rect.origin.x + 20.0, rows_top + 8.0),
                    colors.muted_text,
                    14.0,
                ));
            }
            for (index, item) in dialog.items.iter().enumerate() {
                let row = Rect::new(
                    dialog_rect.origin.x + 16.0,
                    rows_top + index as f32 * DIALOG_ROW_HEIGHT,
                    dialog_rect.size.width - 32.0,
                    DIALOG_ROW_HEIGHT - 2.0,
                );
                if index == dialog.selected {
                    commands.push(fill(row, colors.surface));
                    commands.push(stroke(row, colors.accent));
                }
                commands.push(label(
                    item,
                    Point::new(row.origin.x + 10.0, row.origin.y + 8.0),
                    colors.text,
                    14.0,
                ));
            }
            let cancel = selection_dialog_cancel_rect(dialog_rect, buttons_top);
            let confirm = selection_dialog_confirm_rect(dialog_rect, buttons_top);
            commands.push(fill(cancel, colors.surface));
            commands.push(stroke(cancel, colors.border));
            commands.push(label(
                "Cancelar",
                Point::new(cancel.origin.x + 22.0, cancel.origin.y + 8.0),
                colors.text,
                14.0,
            ));
            commands.push(fill(
                confirm,
                if dialog.items.is_empty() {
                    colors.border
                } else {
                    colors.accent
                },
            ));
            commands.push(label(
                "Selecionar",
                Point::new(confirm.origin.x + 18.0, confirm.origin.y + 8.0),
                colors.text,
                14.0,
            ));
        }
        commands
    }

    fn selection_dialog_pointer_down(&mut self, point: Point, size: Size) {
        let Some(dialog) = self.selection_dialog.as_mut() else {
            return;
        };
        let (dialog_rect, rows_top, buttons_top) =
            selection_dialog_geometry(size, dialog.items.len());
        let cancel = selection_dialog_cancel_rect(dialog_rect, buttons_top);
        if cancel.contains(point) {
            self.selection_dialog = None;
            return;
        }
        let confirm = selection_dialog_confirm_rect(dialog_rect, buttons_top);
        if confirm.contains(point) && !dialog.items.is_empty() {
            self.selection_result = Some(dialog.selected);
            self.selection_dialog = None;
            return;
        }
        if point.x >= dialog_rect.origin.x + 16.0
            && point.x < dialog_rect.origin.x + dialog_rect.size.width - 16.0
            && point.y >= rows_top
        {
            let index = ((point.y - rows_top) / DIALOG_ROW_HEIGHT).floor() as usize;
            if index < dialog.items.len() {
                dialog.selected = index;
            }
        }
    }

    fn settings_dialog_pointer_down(&mut self, point: Point, size: Size) {
        if !self.settings_modal.is_open() {
            return;
        }
        self.settings_modal
            .layout(&LayoutContext, Rect::new(0.0, 0.0, size.width, size.height));
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
        self.jdk_combo.layout(&LayoutContext, geometry.combo);
        self.jdk_browse_button
            .layout(&LayoutContext, geometry.browse);
        self.settings_close_button
            .layout(&LayoutContext, geometry.close);
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
            .layout(&LayoutContext, geometry.close);
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
        self.stop_button.layout(&LayoutContext, rects[0]);
        self.run_button.layout(&LayoutContext, rects[1]);
        self.debug_button.layout(&LayoutContext, rects[2]);
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

fn selection_dialog_geometry(size: Size, item_count: usize) -> (Rect, f32, f32) {
    let rows_height = item_count.max(1) as f32 * DIALOG_ROW_HEIGHT;
    let width = 620.0_f32.min((size.width - 40.0).max(320.0));
    let height = 112.0 + rows_height;
    let rect = Rect::new(
        ((size.width - width) / 2.0).max(0.0),
        ((size.height - height) / 2.0).max(0.0),
        width,
        height,
    );
    let rows_top = rect.origin.y + 52.0;
    let buttons_top = rect.origin.y + rect.size.height - 44.0;
    (rect, rows_top, buttons_top)
}

fn selection_dialog_cancel_rect(dialog: Rect, buttons_top: f32) -> Rect {
    Rect::new(
        dialog.origin.x + dialog.size.width - 224.0,
        buttons_top,
        96.0,
        32.0,
    )
}

fn selection_dialog_confirm_rect(dialog: Rect, buttons_top: f32) -> Rect {
    Rect::new(
        dialog.origin.x + dialog.size.width - 118.0,
        buttons_top,
        102.0,
        32.0,
    )
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
    frames_top: f32,
    frames_height: f32,
    variables_top: f32,
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
    DebugPanelGeometry {
        panel,
        buttons,
        frames_top,
        frames_height,
        variables_top: frames_top + frames_height + 30.0,
    }
}

fn geometry(size: Size, requested_terminal_height: f32, sidebar_width: f32) -> Geometry {
    let content_top = TITLE_HEIGHT + TAB_HEIGHT;
    let content_bottom = size.height - 24.0;
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

fn ellipsize(text: &str, max_characters: usize) -> String {
    if text.chars().count() <= max_characters {
        return text.to_owned();
    }
    let visible = max_characters.saturating_sub(1);
    let mut shortened = text.chars().take(visible).collect::<String>();
    shortened.push('…');
    shortened
}

fn scrollbar(
    track: Rect,
    total: usize,
    visible: usize,
    offset: usize,
    colors: ColorTokens,
) -> Vec<PaintCommand> {
    let mut commands = vec![fill(track, colors.elevated)];
    let Some(metrics) = scrollbar_metrics(track, total, visible, offset) else {
        return commands;
    };
    commands.push(fill(
        Rect::new(
            metrics.thumb.origin.x + 2.0,
            metrics.thumb.origin.y,
            track.size.width - 4.0,
            metrics.thumb.size.height,
        ),
        colors.muted_text,
    ));
    commands
}

#[derive(Clone, Copy)]
struct ScrollbarMetrics {
    track: Rect,
    thumb: Rect,
    max_offset: usize,
}

fn scrollbar_metrics(
    track: Rect,
    total: usize,
    visible: usize,
    offset: usize,
) -> Option<ScrollbarMetrics> {
    if total <= visible || total == 0 || track.size.height <= 0.0 {
        return None;
    }
    let ratio = visible as f32 / total as f32;
    let thumb_height = (track.size.height * ratio).max(24.0).min(track.size.height);
    let max_offset = total.saturating_sub(visible).max(1);
    let travel = (track.size.height - thumb_height).max(0.0);
    let y = track.origin.y + travel * offset.min(max_offset) as f32 / max_offset as f32;
    Some(ScrollbarMetrics {
        track,
        thumb: Rect::new(track.origin.x, y, track.size.width, thumb_height),
        max_offset,
    })
}

fn offset_from_scrollbar(thumb_y: f32, metrics: ScrollbarMetrics) -> usize {
    let travel = (metrics.track.size.height - metrics.thumb.size.height).max(0.0);
    if travel == 0.0 {
        return 0;
    }
    let position = (thumb_y - metrics.track.origin.y).clamp(0.0, travel);
    (position / travel * metrics.max_offset as f32).round() as usize
}

#[derive(Clone, Copy)]
struct HorizontalScrollbarMetrics {
    track: Rect,
    thumb: Rect,
    max_offset: f32,
}

fn horizontal_scrollbar_metrics(
    track: Rect,
    total_width: f32,
    visible_width: f32,
    offset: f32,
) -> Option<HorizontalScrollbarMetrics> {
    if total_width <= visible_width || total_width <= 0.0 || track.size.width <= 0.0 {
        return None;
    }
    let thumb_width = (track.size.width * visible_width / total_width)
        .max(24.0)
        .min(track.size.width);
    let max_offset = (total_width - visible_width).max(1.0);
    let travel = (track.size.width - thumb_width).max(0.0);
    let x = track.origin.x + travel * offset.clamp(0.0, max_offset) / max_offset;
    Some(HorizontalScrollbarMetrics {
        track,
        thumb: Rect::new(x, track.origin.y, thumb_width, track.size.height),
        max_offset,
    })
}

fn offset_from_horizontal_scrollbar(thumb_x: f32, metrics: HorizontalScrollbarMetrics) -> f32 {
    let travel = (metrics.track.size.width - metrics.thumb.size.width).max(0.0);
    if travel == 0.0 {
        return 0.0;
    }
    let position = (thumb_x - metrics.track.origin.x).clamp(0.0, travel);
    position / travel * metrics.max_offset
}

fn horizontal_scrollbar(
    track: Rect,
    total_width: f32,
    visible_width: f32,
    offset: f32,
    colors: ColorTokens,
) -> Vec<PaintCommand> {
    let mut commands = vec![fill(track, colors.elevated)];
    let Some(metrics) = horizontal_scrollbar_metrics(track, total_width, visible_width, offset)
    else {
        return commands;
    };
    commands.push(fill(
        Rect::new(
            metrics.thumb.origin.x,
            metrics.thumb.origin.y + 2.0,
            metrics.thumb.size.width,
            (metrics.thumb.size.height - 4.0).max(2.0),
        ),
        colors.muted_text,
    ));
    commands
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
fn syntax_color(line: &str, plain: Color, keyword: Color, muted: Color) -> Color {
    let trimmed = line.trim_start();
    if trimmed.starts_with("//") {
        muted
    } else if ["use ", "fn ", "let ", "pub ", "struct ", "impl "]
        .iter()
        .any(|prefix| trimmed.starts_with(prefix))
    {
        keyword
    } else {
        plain
    }
}

fn highlighted_line(
    line: &str,
    line_index: usize,
    origin: Point,
    snapshot: &SyntaxSnapshot,
    colors: ColorTokens,
) -> Vec<PaintCommand> {
    let line_length = line.chars().count();
    let mut spans = snapshot
        .highlights
        .iter()
        .filter_map(|highlight| {
            let start_line = highlight.range.start.line as usize;
            let end_line = highlight.range.end.line as usize;
            if line_index < start_line || line_index > end_line {
                return None;
            }
            let start = if line_index == start_line {
                highlight.range.start.column as usize
            } else {
                0
            };
            let end = if line_index == end_line {
                highlight.range.end.column as usize
            } else {
                line_length
            };
            (start < end).then_some((start.min(line_length), end.min(line_length), highlight.kind))
        })
        .collect::<Vec<_>>();
    spans.sort_by_key(|(start, end, _)| (*start, *end));

    let mut commands = Vec::new();
    let mut column = 0;
    for (start, end, kind) in spans {
        let start = start.max(column);
        if start > column {
            push_line_segment(&mut commands, line, column, start, origin, colors.text);
        }
        if end > start {
            push_line_segment(
                &mut commands,
                line,
                start,
                end,
                origin,
                syntax_highlight_color(kind, colors),
            );
            column = end;
        }
    }
    if column < line_length {
        push_line_segment(
            &mut commands,
            line,
            column,
            line_length,
            origin,
            colors.text,
        );
    }
    if line_length == 0 {
        commands.push(label("", origin, colors.text, 15.0));
    }
    commands
}

fn push_line_segment(
    commands: &mut Vec<PaintCommand>,
    line: &str,
    start: usize,
    end: usize,
    origin: Point,
    color: Color,
) {
    let text = line
        .chars()
        .skip(start)
        .take(end.saturating_sub(start))
        .collect::<String>();
    if !text.is_empty() {
        commands.push(label(
            &text,
            Point::new(origin.x + start as f32 * 8.4, origin.y),
            color,
            15.0,
        ));
    }
}

fn syntax_highlight_color(kind: SyntaxHighlightKind, colors: ColorTokens) -> Color {
    match kind {
        SyntaxHighlightKind::Keyword | SyntaxHighlightKind::Operator => colors.accent,
        SyntaxHighlightKind::Type => colors.syntax_type,
        SyntaxHighlightKind::Function => colors.syntax_function,
        SyntaxHighlightKind::String => colors.syntax_string,
        SyntaxHighlightKind::Number => colors.syntax_number,
        SyntaxHighlightKind::Comment => colors.muted_text,
        SyntaxHighlightKind::Annotation => colors.syntax_annotation,
        SyntaxHighlightKind::Field | SyntaxHighlightKind::Variable => colors.text,
    }
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
                panel.frames_top + DEBUG_ROW_HEIGHT,
            ),
            size,
        );
        assert_eq!(
            shell.take_debug_requests(),
            vec![DebugRequest::SelectFrame(1)]
        );
        assert_eq!(shell.debug_view().selected_frame, 1);
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
            button.layout(&LayoutContext, rect);
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
        let icon_color = |shell: &IdeShell| {
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
            icon_color(&shell),
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
            .layout(&LayoutContext, Rect::new(0.0, 0.0, size.width, size.height));
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
                    if text.text == "public" && text.color == colors.accent
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
            editor_x + EDITOR_GUTTER + 8.0 * 8.4,
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
        assert!(shell.explorer_horizontal_metrics(size).is_some());
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
        assert!(texts.contains(&"ExplosionEff…"));
        assert!(!texts.contains(&"ExplosionEffectManager.ts"));
        assert!(rendered.iter().any(|command| {
            matches!(
                command,
                PaintCommand::PushClip(rect) if rect.size.width == TAB_WIDTH - 38.0
            )
        }));
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

    #[test]
    fn scrollbar_maps_click_and_drag_to_content_offsets() {
        let track = Rect::new(100.0, 20.0, 10.0, 200.0);
        let metrics = match scrollbar_metrics(track, 100, 10, 0) {
            Some(metrics) => metrics,
            None => panic!("scrollbar metrics unavailable"),
        };
        assert_eq!(offset_from_scrollbar(track.origin.y, metrics), 0);
        assert_eq!(
            offset_from_scrollbar(track.origin.y + track.size.height, metrics),
            90
        );
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
        let target_x = editor_x + EDITOR_GUTTER + 5.0 * 8.4;
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
    fn selection_dialog_supports_keyboard_confirmation_and_cancel() {
        let mut shell = test_shell();
        shell.open_selection_dialog(
            "Selecionar JDK",
            vec!["JDK 8".to_owned(), "JDK 17".to_owned()],
            0,
        );
        assert!(shell.selection_dialog_open());
        assert!(
            shell
                .paint(Size::new(1_000.0, 700.0))
                .iter()
                .any(|command| {
                    matches!(
                        command,
                        PaintCommand::DrawText(text) if text.text == "Selecionar JDK"
                    )
                })
        );

        shell.key_down("ArrowDown");
        shell.key_down("Enter");
        assert!(!shell.selection_dialog_open());
        assert_eq!(shell.take_selection_result(), Some(1));

        shell.open_selection_dialog("Selecionar JDK", vec!["JDK 17".to_owned()], 0);
        shell.escape();
        assert!(!shell.selection_dialog_open());
        assert_eq!(shell.take_selection_result(), None);
    }

    #[test]
    fn selection_dialog_supports_mouse_selection_and_confirmation() {
        let mut shell = test_shell();
        let size = Size::new(1_000.0, 700.0);
        shell.open_selection_dialog(
            "Selecionar JDK",
            vec!["JDK 8".to_owned(), "JDK 17".to_owned()],
            0,
        );
        let (dialog, rows_top, buttons_top) = selection_dialog_geometry(size, 2);
        shell.pointer_down(
            Point::new(dialog.origin.x + 30.0, rows_top + DIALOG_ROW_HEIGHT + 5.0),
            size,
        );
        let confirm = selection_dialog_confirm_rect(dialog, buttons_top);
        shell.pointer_down(
            Point::new(confirm.origin.x + 10.0, confirm.origin.y + 10.0),
            size,
        );

        assert_eq!(shell.take_selection_result(), Some(1));
        assert!(!shell.selection_dialog_open());
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
            .layout(&LayoutContext, Rect::new(0.0, 0.0, size.width, size.height));
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
