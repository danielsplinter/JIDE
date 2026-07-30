//! Coordenação dos estados de feature da interface.

#[cfg(test)]
use crate::debugging::DebugFrameView;
use crate::debugging::{
    DebugPanelState, DebugVariableView, DebugView, InspectionFocus, InspectionNode, InspectionRun,
    InspectionState, InspectionView,
};
use crate::editor::{
    CachedSyntax, ConstructorRequest, EditorAction, EditorAreaState, EditorCapabilities, EditorPane,
    GenerateState, RenameState, SyntaxView,
};
use crate::explorer::{
    ExplorerState, id as explorer_id, is_source_root, items as explorer_items,
    visible_row as visible_tree_row,
};
use crate::search::{
    ContentSearchHit, NewItemDialog, SearchState, TypeSearchHit, WorkspaceSearchMode,
};
use crate::settings::{SettingsPage, SettingsState};
use crate::shell::{ShellCommandQueue, ShellFocus};
use crate::terminal::{
    ScrollTarget, TerminalPanelState, TerminalSelection, TerminalTab, TextPosition,
    ordered_selection, selection_columns,
};
use ide_application::{
    ApplicationCommand, DebugRequest, NavigationRequest, NewItemRequest, NewItemTemplate,
    FileOccurrences, OpenDocumentRequest, RenameDocumentRequest, SaveDocumentRequest, TaskId,
    UiContributionCatalog,
};
#[cfg(test)]
use ide_application::{NewItemTemplateId, SettingsSection, TaskDescriptor};

use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    hash::{DefaultHasher, Hash, Hasher},
    path::{Path, PathBuf},
    sync::Arc,
};

use crate::layout::{
    DEBUG_BUTTONS, Geometry, action_button_rects, debug_panel_geometry, shell_geometry as geometry,
};
use crate::menus::{
    MenuState, debug_request as debug_request_for, editor_entries as editor_menu_entries,
    explorer_entries as explorer_menu_entries,
};
#[cfg(test)]
use crate::search::search_display_path;
use crate::settings::SettingsDialog;
use ide_domain::{
    AccessorCandidate, AccessorKind, AccessorPlan, CompletionItem, CompletionRequest, DocumentId,
    DocumentSnapshot, Location, OutlineItem, OutlineKind,
    SyntaxHighlightKind, SyntaxSnapshot, TextPosition as DomainTextPosition,
    TextRange as DomainTextRange,
};
use ide_terminal::{ShellKind, TerminalSession};
use ide_workspace::{EditorSession, FileNode, TextBuffer, rewrite_occurrences};
use ui_api::{EventContext, LayoutContext, PaintContext, TextMetrics, Widget};
#[cfg(test)]
use ui_components::MenuEntry;
use ui_components::{
    Button, CellWidth, Checkbox, ComboBox, ComboBoxItem, ComposedCell, ComposedList, ComposedRow,
    ContextMenu, Icon, IconTint, Label, ListSelection, ListView,
    MenuBar, MenuBarItem, MenuItem, ModalHost, Popup, Scrollbar, ScrollbarOrientation,
    SplitOrientation, Splitter, StatusBar, TabItem, Tabs, TextInput, TreeItem, TreeView,
};
use ui_core::{
    Color, ColorTokens, CommandId, EventResult, FontId, KeyEvent, Modifiers, Point, PointerButton,
    PointerEvent, Rect, Size, TextInputEvent, Theme, UiEvent, WidgetAction, WidgetId,
};
use ui_editor::{CodeEditor, GutterMark, LineDecoration, TokenKind};
use ui_render_api::{DrawTextCommand, FillRectCommand, PaintCommand, StrokeRectCommand};
use ui_window_api::ClipboardService;

pub(super) const ACTIVITY_WIDTH: f32 = 48.0;
const SIDEBAR_WIDTH: f32 = 260.0;
const SIDEBAR_MIN_WIDTH: f32 = 160.0;
pub(super) const TITLE_HEIGHT: f32 = 36.0;
pub(super) const TAB_HEIGHT: f32 = 38.0;
const EXPLORER_ROW_HEIGHT: f32 = 23.0;
const EXPLORER_TOP: f32 = 106.0;
/// Métricas do editor: elas são do componente que desenha o código, e a IDE
/// as consulta para posicionar cursor, popup e cliques no mesmo lugar.
const EDITOR_LINE_HEIGHT: f32 = CodeEditor::line_height();
/// Largura de uma parada de tabulação, em colunas.
const EDITOR_GUTTER: f32 = CodeEditor::gutter_width();
/// Largura do caractere na fonte de código, definida pelo editor que a desenha.
const EDITOR_CHAR_WIDTH: f32 = CodeEditor::default_char_width();
const TAB_WIDTH: f32 = 140.0;
const TERMINAL_TAB_WIDTH: f32 = 110.0;
const TERMINAL_TAB_HEIGHT: f32 = 30.0;
const TERMINAL_DEFAULT_HEIGHT: f32 = 180.0;
const TERMINAL_MIN_HEIGHT: f32 = 120.0;
pub(super) const TERMINAL_COLLAPSED_HEIGHT: f32 = 30.0;
const TERMINAL_CHAR_WIDTH: f32 = 8.4;
const DEBUG_PANEL_WIDTH: f32 = 320.0;
pub(super) const DEBUG_ROW_HEIGHT: f32 = 21.0;
const MENU_BAR_ID: WidgetId = WidgetId(10_001);
const SETTINGS_MODAL_ID: WidgetId = WidgetId(10_002);
const TOOLCHAIN_COMBO_ID: WidgetId = WidgetId(10_003);
const TOOLCHAIN_BROWSE_ID: WidgetId = WidgetId(10_004);
const SECONDARY_TOOL_COMBO_ID: WidgetId = WidgetId(10_070);
const SECONDARY_TOOL_BROWSE_ID: WidgetId = WidgetId(10_071);
const SECONDARY_TOOL_CAPTION_ID: WidgetId = WidgetId(10_072);
const SETTINGS_CLOSE_ID: WidgetId = WidgetId(10_005);
const SETTINGS_SAVE_ID: WidgetId = WidgetId(10_030);
const SETTINGS_TITLE_ID: WidgetId = WidgetId(10_031);
const SETTINGS_CAPTION_ID: WidgetId = WidgetId(10_032);
const SETTINGS_MESSAGE_ID: WidgetId = WidgetId(10_033);
const SETTINGS_PAGES_ID: WidgetId = WidgetId(10_034);
/// Páginas da janela de configurações, na ordem em que aparecem.
const DEBUG_SETTINGS_TITLE: &str = "Depuração";
const SETTINGS_PAGE_ROW_HEIGHT: f32 = 42.0;
const RENAME_MODAL_ID: WidgetId = WidgetId(10_060);
const RENAME_INPUT_ID: WidgetId = WidgetId(10_061);
const RENAME_LIST_ID: WidgetId = WidgetId(10_062);
const RENAME_OK_ID: WidgetId = WidgetId(10_063);
const RENAME_CANCEL_ID: WidgetId = WidgetId(10_064);
const RENAME_NAME_CAPTION_ID: WidgetId = WidgetId(10_065);
const RENAME_LIST_CAPTION_ID: WidgetId = WidgetId(10_066);
/// A janela é larga porque a lista mostra caminho e contagem em cada linha, e
/// estreitá-la só empurraria o trabalho para a barra lateral.
const RENAME_PANEL_SIZE: Size = Size::new(720.0, 460.0);
const NEW_ITEM_MODAL_ID: WidgetId = WidgetId(10_035);
const NEW_ITEM_PACKAGE_ID: WidgetId = WidgetId(10_036);
const NEW_ITEM_NAME_ID: WidgetId = WidgetId(10_037);
const NEW_ITEM_CREATE_ID: WidgetId = WidgetId(10_038);
const NEW_ITEM_CANCEL_ID: WidgetId = WidgetId(10_039);
const NEW_ITEM_PACKAGE_CAPTION_ID: WidgetId = WidgetId(10_041);
const NEW_ITEM_NAME_CAPTION_ID: WidgetId = WidgetId(10_042);
const NEW_ITEM_MESSAGE_ID: WidgetId = WidgetId(10_043);
/// A janela é pequena de propósito: dois campos e duas ações.
const NEW_ITEM_PANEL_SIZE: Size = Size::new(460.0, 230.0);
const GENERATE_MODAL_ID: WidgetId = WidgetId(10_070);
const GENERATE_LIST_ID: WidgetId = WidgetId(10_071);
const GENERATE_ALL_ID: WidgetId = WidgetId(10_072);
const GENERATE_OK_ID: WidgetId = WidgetId(10_073);
const GENERATE_PANEL_SIZE: Size = Size::new(420.0, 380.0);
const GENERATE_ROW_HEIGHT: f32 = 28.0;
/// Faixa de ids das células da lista, para não colidir com outros componentes.
const GENERATE_CELL_BASE: u64 = 10_200;
const TYPE_SEARCH_MODAL_ID: WidgetId = WidgetId(10_060);
const TYPE_SEARCH_INPUT_ID: WidgetId = WidgetId(10_061);
const TYPE_SEARCH_LIST_ID: WidgetId = WidgetId(10_062);
/// Janela larga: o que ela mostra são caminhos, e caminho cortado não localiza.
const TYPE_SEARCH_PANEL_SIZE: Size = Size::new(760.0, 420.0);
const TYPE_SEARCH_ROW_HEIGHT: f32 = 26.0;
/// Linhas que cabem na lista de 302 pontos de altura, incluindo a última parcial.
const TYPE_SEARCH_VISIBLE_ROWS: usize = 12;
const INSPECTION_MODAL_ID: WidgetId = WidgetId(10_044);
const INSPECTION_TREE_ID: WidgetId = WidgetId(10_045);
const INSPECTION_CLOSE_ID: WidgetId = WidgetId(10_046);
const INSPECTION_NAME_ID: WidgetId = WidgetId(10_047);
const INSPECTION_TYPE_ID: WidgetId = WidgetId(10_048);
const INSPECTION_VALUE_ID: WidgetId = WidgetId(10_049);
const INSPECTION_EMPTY_ID: WidgetId = WidgetId(10_050);
const INSPECTION_RUN_ID: WidgetId = WidgetId(10_052);
const INSPECTION_SOURCE_CAPTION_ID: WidgetId = WidgetId(10_053);
const INSPECTION_MESSAGE_ID: WidgetId = WidgetId(10_054);
/// Largura média de caractere na fonte da mensagem, para saber onde cortar.
const INSPECTION_MESSAGE_CHAR_WIDTH: f32 = 6.6;
/// A janela é larga porque o valor de um objeto costuma ser longo.
const INSPECTION_PANEL_SIZE: Size = Size::new(720.0, 420.0);
const INSPECTION_ROW_HEIGHT: f32 = 26.0;
/// Fatia da janela ocupada pela lista, à esquerda.
const INSPECTION_LIST_FRACTION: f32 = 0.42;
/// Fatia do painel direito ocupada pelo detalhe; o resto é o editor.
const INSPECTION_DETAIL_FRACTION: f32 = 0.45;
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
const EDITOR_HORIZONTAL_SCROLLBAR_ID: WidgetId = WidgetId(10_063);
const TERMINAL_SCROLLBAR_ID: WidgetId = WidgetId(10_018);
const EXPLORER_VERTICAL_SCROLLBAR_ID: WidgetId = WidgetId(10_019);
const EXPLORER_HORIZONTAL_SCROLLBAR_ID: WidgetId = WidgetId(10_021);
const EXPLORER_TREE_ID: WidgetId = WidgetId(10_020);
const SIDEBAR_SPLITTER_ID: WidgetId = WidgetId(10_022);
const TERMINAL_SPLITTER_ID: WidgetId = WidgetId(10_023);
const EXPLORER_CONTEXT_MENU_ID: WidgetId = WidgetId(10_025);
const COMPLETION_POPUP_ID: WidgetId = WidgetId(10_026);
const COMPLETION_LIST_ID: WidgetId = WidgetId(10_027);
const SEARCH_POPUP_ID: WidgetId = WidgetId(10_028);
const SEARCH_INPUT_ID: WidgetId = WidgetId(10_029);
/// Linhas visíveis da lista de completação antes de precisar rolar.
const COMPLETION_VISIBLE_ROWS: usize = 8;
const COMPLETION_ROW_HEIGHT: f32 = 24.0;
const COMPLETION_POPUP_WIDTH: f32 = 260.0;
/// Folga entre a borda da superfície flutuante e a lista dentro dela.
///
/// Entra na conta da área ocupada, e é por isso que é uma constante: o clique
/// precisa acertar a mesma área que a pintura desenhou, incluindo a moldura.
const COMPLETION_POPUP_PADDING: f32 = 4.0;
const SEARCH_BOX_WIDTH: f32 = 380.0;
const SEARCH_BOX_HEIGHT: f32 = 42.0;

/// Serviços e estado visual compartilhados apenas pelo coordenador.
struct ShellContext {
    focus: ShellFocus,
    text_metrics: Option<Arc<dyn TextMetrics>>,
    clipboard: Option<Arc<dyn ClipboardService>>,
    theme: Theme,
    status_message: String,
    project_summary: Option<String>,
    pointer: Point,
    /// Tamanho da janela no último quadro.
    ///
    /// A soltura do ponteiro não recebe tamanho, e as janelas precisam dele
    /// para saber onde estão seus componentes.
    last_size: Size,
    scrollbar_drag: Option<ScrollTarget>,
}

/// Coordenador da interface. Cada feature é dona de seus widgets e seleção.
pub struct IdeShell {
    explorer: ExplorerState,
    editor_area: EditorAreaState,
    terminal: TerminalPanelState,
    search: SearchState,
    settings: SettingsState,
    debug_panel: DebugPanelState,
    menu: MenuState,
    catalog: UiContributionCatalog,
    context: ShellContext,
    commands: ShellCommandQueue,
}
impl IdeShell {
    #[cfg(test)]
    fn open(root: &Path) -> Result<Self, ide_workspace::WorkspaceError> {
        ide_workspace::WorkspaceService::native()
            .scan(root)
            .map(Self::from_tree)
    }

    #[cfg(test)]
    fn open_file(&mut self, path: &Path) -> Result<DocumentId, String> {
        if self
            .editor_area
            .session
            .tabs()
            .any(|document| document.path == path)
        {
            return Ok(self.show_document(path, String::new()));
        }
        let text = ide_workspace::WorkspaceService::native()
            .read_document(path)
            .map_err(|error| error.to_string())?;
        Ok(self.show_document(path, text))
    }

    #[cfg(test)]
    fn open_location(
        &mut self,
        path: &Path,
        line: usize,
        column: usize,
    ) -> Result<DocumentId, String> {
        if self
            .editor_area
            .session
            .tabs()
            .any(|document| document.path == path)
        {
            return Ok(self.show_location(path, String::new(), line, column));
        }
        let text = ide_workspace::WorkspaceService::native()
            .read_document(path)
            .map_err(|error| error.to_string())?;
        Ok(self.show_location(path, text, line, column))
    }

    #[cfg(test)]
    fn save_active_document(&mut self) -> bool {
        let Some(document) = self.editor_area.session.active() else {
            return false;
        };
        let id = document.id;
        let path = document.path.clone();
        let text = document.buffer.text().to_owned();
        let revision = document.buffer.revision();
        if ide_workspace::WorkspaceService::native()
            .save_document(&path, &text)
            .is_err()
        {
            return false;
        }
        self.document_saved(id, revision, &path);
        true
    }

    #[cfg(test)]
    fn reload_workspace(&mut self) -> Result<(), ide_workspace::WorkspaceError> {
        let tree = ide_workspace::WorkspaceService::native().scan(&self.explorer.workspace.path)?;
        self.replace_workspace_tree(tree);
        Ok(())
    }

    /// Raiz atualmente carregada no Explorer.
    #[must_use]
    pub fn workspace_root(&self) -> &Path {
        self.explorer.workspace_root()
    }

    /// Árvore já varrida que serviços de workspace podem consultar sem refazer
    /// I/O a cada tecla.
    #[must_use]
    pub fn workspace_tree(&self) -> &FileNode {
        self.explorer.workspace_tree()
    }

    /// Substitui a árvore já carregada pela camada de workspace.
    pub fn replace_workspace_tree(&mut self, workspace: FileNode) {
        self.explorer
            .replace_workspace(workspace, &self.catalog.source_root_names);
        // A `TreeView` guarda os itens dela: reler o disco sem repô-los deixava a
        // árvore desenhando a varredura anterior. `set_roots` preserva expansão e
        // seleção por identidade, então a posição do usuário não se perde.
        self.sync_explorer_tree();
    }

    /// Revisão do documento ativo, para quem precisa notar que o texto mudou.
    ///
    /// Um clique pode alterar o texto — gerar acessores, por exemplo —, e quem
    /// mantém o realce precisa perceber isso sem depender de uma tecla.
    #[must_use]
    pub fn active_revision(&self) -> u64 {
        self.editor_area
            .session
            .active()
            .map_or(0, |document| document.buffer.revision())
    }

    /// A aba ativa tem alteração ainda não gravada.
    ///
    /// É o que a marca na aba anuncia, e o que decide se fechar sem salvar
    /// perderia trabalho.
    #[must_use]
    pub fn active_document_modified(&self) -> bool {
        self.editor_area
            .session
            .active()
            .is_some_and(|document| document.buffer.is_dirty())
    }

    /// Solicita a gravação da aba ativa à camada de aplicação.
    pub fn request_save_active_document(&mut self) {
        let Some(document) = self.editor_area.session.active() else {
            self.context.status_message = "Nenhum documento aberto".to_owned();
            return;
        };
        if !document.is_persistent() {
            self.context.status_message =
                "Documento em memória não possui caminho para salvar".to_owned();
            return;
        }
        self.commands
            .push(ApplicationCommand::SaveDocument(SaveDocumentRequest {
                document_id: document.id,
                path: document.path.clone(),
                text: document.buffer.text().to_owned(),
                revision: document.buffer.revision(),
            }));
    }

    pub fn document_saved(&mut self, document_id: DocumentId, revision: u64, path: &Path) {
        if self
            .editor_area
            .session
            .mark_saved(document_id, revision)
            .is_ok()
        {
            self.context.status_message = format!("Salvo {}", path.display());
        }
    }

    /// Abre a pasta e todas as que levam até ela.
    ///
    /// Criar algo dentro de uma pasta fechada esconde o que acabou de nascer;
    /// revelar o caminho é o que faz o resultado aparecer.
    pub fn reveal_in_explorer(&mut self, path: &Path) {
        for ancestor in path.ancestors() {
            if ancestor.starts_with(&self.explorer.workspace.path) {
                self.explorer.expanded.insert(ancestor.to_path_buf());
            }
        }
        self.sync_explorer_tree();
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
        let terminal_root = if workspace.is_directory {
            workspace.path.clone()
        } else {
            PathBuf::from(".")
        };
        let terminals = TerminalSession::discover_profiles()
            .into_iter()
            .filter_map(|profile| {
                TerminalSession::new(terminal_root.clone(), 2_000, profile.clone())
                    .or_else(|_| TerminalSession::new(PathBuf::from("."), 2_000, profile))
                    .ok()
                    .map(|session| TerminalTab {
                        session,
                        scroll_line: 0,
                        follow_output: true,
                    })
            })
            .collect();
        let explorer_tree = TreeView::new(EXPLORER_TREE_ID, explorer_items(&workspace, &[]))
            .with_row_height(EXPLORER_ROW_HEIGHT);
        let mut shell = Self {
            explorer: ExplorerState {
                workspace_name,
                workspace,
                tree: explorer_tree,
                context_menu: ContextMenu::new(EXPLORER_CONTEXT_MENU_ID, Vec::new()),
                context_menu_target: None,
                context_menu_file: None,
                expanded,
                scroll_x: 0.0,
                scroll_line: 0,
                sidebar_width: SIDEBAR_WIDTH,
                splitter: Splitter::new(SIDEBAR_SPLITTER_ID, SplitOrientation::Horizontal),
                vertical_scrollbar: Scrollbar::new(
                    EXPLORER_VERTICAL_SCROLLBAR_ID,
                    ScrollbarOrientation::Vertical,
                ),
                horizontal_scrollbar: Scrollbar::new(
                    EXPLORER_HORIZONTAL_SCROLLBAR_ID,
                    ScrollbarOrientation::Horizontal,
                ),
            },
            editor_area: EditorAreaState {
                session: EditorSession::default(),
                pane: EditorPane::new(EditorCapabilities::full()),
                search_query: String::new(),
                navigated: None,
                scrollbar: Scrollbar::new(EDITOR_SCROLLBAR_ID, ScrollbarOrientation::Vertical),
                horizontal_scrollbar: Scrollbar::new(
                    EDITOR_HORIZONTAL_SCROLLBAR_ID,
                    ScrollbarOrientation::Horizontal,
                ),
                syntax_snapshots: HashMap::new(),
                syntax_spans: HashMap::new(),
                completion_items: Vec::new(),
                completion_selected: 0,
                generate: None,
                generate_pending: None,
                constructor_pending: None,
                rename: None,
                rename_pending: None,
                rename_modal: ModalHost::new(RENAME_MODAL_ID, "Renomear", RENAME_PANEL_SIZE),
                generate_modal: ModalHost::new(
                    GENERATE_MODAL_ID,
                    "Generate",
                    GENERATE_PANEL_SIZE,
                ),
            },
            terminal: TerminalPanelState {
                tabs: terminals,
                active: 0,
                height: TERMINAL_DEFAULT_HEIGHT,
                last_height: TERMINAL_DEFAULT_HEIGHT,
                minimized: false,
                splitter: Splitter::new(TERMINAL_SPLITTER_ID, SplitOrientation::Vertical),
                scrollbar: Scrollbar::new(TERMINAL_SCROLLBAR_ID, ScrollbarOrientation::Vertical),
                selection: None,
                selecting: false,
                running_terminal: None,
            },
            search: SearchState {
                modal: ModalHost::new(
                    TYPE_SEARCH_MODAL_ID,
                    "Ir para o tipo",
                    TYPE_SEARCH_PANEL_SIZE,
                ),
                mode: WorkspaceSearchMode::Types,
                query: String::new(),
                type_results: Vec::new(),
                content_results: Vec::new(),
                selected: 0,
                first_visible: 0,
                new_item_modal: ModalHost::new(NEW_ITEM_MODAL_ID, "", NEW_ITEM_PANEL_SIZE),
                new_item_dialog: None,
                new_item_package: TextInput::new(NEW_ITEM_PACKAGE_ID, String::new())
                    .with_placeholder("br.com.exemplo"),
                new_item_name: TextInput::new(NEW_ITEM_NAME_ID, String::new()),
                new_item_create_button: Button::new(NEW_ITEM_CREATE_ID, "Criar")
                    .with_command("new.create"),
                new_item_cancel_button: Button::new(NEW_ITEM_CANCEL_ID, "Cancelar")
                    .with_command("new.cancel"),
            },
            settings: SettingsState {
                modal: ModalHost::new(SETTINGS_MODAL_ID, "Configurações", Size::new(780.0, 460.0)),
                toolchain_combo: ComboBox::new(TOOLCHAIN_COMBO_ID, Vec::new())
                    .with_command_prefix("toolchain.select."),
                secondary_combo: ComboBox::new(SECONDARY_TOOL_COMBO_ID, Vec::new())
                    .with_command_prefix("tool.select."),
                secondary_browse_button: Button::new(SECONDARY_TOOL_BROWSE_ID, "Procurar...")
                    .with_command("tool.browse"),
                toolchain_browse_button: Button::new(TOOLCHAIN_BROWSE_ID, "Procurar...")
                    .with_command("toolchain.browse"),
                close_button: Button::new(SETTINGS_CLOSE_ID, "Cancelar")
                    .with_command("settings.cancel"),
                save_button: Button::new(SETTINGS_SAVE_ID, "Salvar").with_command("settings.save"),
                pages: ListView::new(SETTINGS_PAGES_ID, [DEBUG_SETTINGS_TITLE])
                    .with_row_height(SETTINGS_PAGE_ROW_HEIGHT)
                    .with_selection(ListSelection::Marker),
                dialog: None,
                page: SettingsPage::default(),
                focus: None,
                debug_host: TextInput::new(DEBUG_HOST_ID, "127.0.0.1").with_placeholder("host"),
                debug_port: TextInput::new(DEBUG_PORT_ID, "8000").with_placeholder("porta"),
                debug_attach_button: Button::new(DEBUG_ATTACH_ID, "Conectar")
                    .with_command("debug.attach"),
            },
            debug_panel: DebugPanelState {
                inspection: InspectionState {
                    modal: ModalHost::new(
                        INSPECTION_MODAL_ID,
                        "Inspecionar",
                        INSPECTION_PANEL_SIZE,
                    ),
                    view: None,
                    tree: TreeView::new(INSPECTION_TREE_ID, Vec::new())
                        .with_row_height(INSPECTION_ROW_HEIGHT),
                    close_button: Button::new(INSPECTION_CLOSE_ID, "Fechar")
                        .with_command("inspect.close"),
                    editor: EditorPane::new(EditorCapabilities::plain()),
                    source: TextBuffer::new(String::new()),
                    run_button: Button::new(INSPECTION_RUN_ID, "Executar")
                        .with_command("inspect.run"),
                    focus: InspectionFocus::Tree,
                    message: None,
                    run: None,
                },
                stop_button: Button::icon(STOP_BUTTON_ID, Icon::Stop, "Parar aplicação")
                    .with_tint(IconTint::Muted)
                    .with_command("project.stop"),
                run_button: Button::icon(RUN_BUTTON_ID, Icon::Play, "Executar aplicação")
                    .with_tint(IconTint::Success)
                    .with_command("project.run"),
                debug_button: Button::icon(DEBUG_BUTTON_ID, Icon::Bug, "Executar com depuração")
                    .with_tint(IconTint::Muted)
                    .with_command("debug.run"),
                breakpoints: BTreeMap::new(),
                verified_breakpoints: BTreeMap::new(),
                view: DebugView::default(),
                frames: ListView::new(DEBUG_FRAMES_ID, Vec::<String>::new())
                    .with_row_height(DEBUG_ROW_HEIGHT),
                variables: ListView::new(DEBUG_VARIABLES_ID, Vec::<String>::new())
                    .with_row_height(DEBUG_ROW_HEIGHT),
            },
            menu: MenuState {
                bar: MenuBar::new(
                    MENU_BAR_ID,
                    vec![
                        MenuBarItem::menu(
                            "Arquivo",
                            vec![
                                MenuItem::new("Projeto...", "file.project"),
                                MenuItem::new("Salvar", "file.save"),
                            ],
                        ),
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
            },
            catalog: UiContributionCatalog::default(),
            context: ShellContext {
                focus: ShellFocus::None,
                text_metrics: None,
                clipboard: None,
                theme: Theme::default(),
                status_message: "Ready".to_owned(),
                project_summary: None,
                pointer: Point::new(-1.0, -1.0),
                last_size: Size::new(1280.0, 800.0),
                scrollbar_drag: None,
            },
            commands: ShellCommandQueue::default(),
        };
        shell.sync_explorer_tree();
        shell
    }

    /// Recebe o mecanismo que mede o texto que a janela desenha.
    pub fn set_clipboard(&mut self, clipboard: Arc<dyn ClipboardService>) {
        self.context.clipboard = Some(clipboard);
    }

    pub fn set_text_metrics(&mut self, metrics: Arc<dyn TextMetrics>) {
        self.context.text_metrics = Some(metrics);
    }

    /// Contexto de pintura com a medição disponível, quando houver.
    fn paint_context(&self) -> PaintContext {
        let context = PaintContext::with_theme(self.context.theme);
        match self.context.text_metrics.as_ref() {
            Some(metrics) => context.measuring(Arc::clone(metrics)),
            None => context,
        }
    }

    /// Contexto de layout com a medição disponível, quando houver.
    fn layout_context(&self) -> LayoutContext {
        match self.context.text_metrics.as_ref() {
            Some(metrics) => LayoutContext::with_text_metrics(Arc::clone(metrics)),
            None => LayoutContext::default(),
        }
    }

    /// Apresenta um documento cujo conteúdo já foi carregado pelo workspace.
    pub fn show_document(&mut self, path: &Path, text: impl Into<String>) -> DocumentId {
        let id = self.editor_area.session.open(path, text);
        self.editor_area.pane.set_cursor(0);
        self.context.focus = ShellFocus::Editor;
        self.context.status_message = format!("Opened {}", path.display());
        self.sync_explorer_to_active();
        id
    }

    pub const fn focus(&self) -> ShellFocus {
        self.context.focus
    }
    pub const fn active_document(&self) -> Option<DocumentId> {
        self.editor_area.active_document()
    }
    pub fn active_text(&self) -> Option<&str> {
        self.editor_area.active_text()
    }
    pub fn document_snapshots(&self) -> Vec<DocumentSnapshot> {
        self.editor_area
            .session
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
        let Some(document) = self.editor_area.session.document(snapshot.document_id) else {
            return;
        };
        let spans = converted_syntax(document.buffer.text(), &snapshot);
        let error_count = snapshot.diagnostics.len();
        let symbol_count = count_outline(&snapshot.outline);
        let import_count = snapshot.imports.len();
        let language = self
            .catalog
            .language_names
            .first()
            .map_or("Análise", String::as_str);
        self.context.status_message = format!(
            "{language}: {error_count} error(s), {symbol_count} symbol(s), {import_count} import(s)"
        );
        self.editor_area.syntax_spans.insert(
            snapshot.document_id,
            CachedSyntax {
                version: snapshot.version,
                spans,
            },
        );
        self.editor_area
            .syntax_snapshots
            .insert(snapshot.document_id, snapshot);
    }
    pub fn syntax_snapshot(&self, document_id: DocumentId) -> Option<&SyntaxSnapshot> {
        self.editor_area.syntax_snapshots.get(&document_id)
    }
    pub fn active_outline(&self) -> &[OutlineItem] {
        self.active_document()
            .and_then(|id| self.editor_area.syntax_snapshots.get(&id))
            .map_or(&[], |snapshot| snapshot.outline.as_slice())
    }
    pub fn completion_request(&self) -> Option<CompletionRequest> {
        let document = self.editor_area.session.active()?;
        let (line, column) = line_column(document.buffer.text(), self.editor_area.pane.cursor());
        Some(CompletionRequest {
            document_id: document.id,
            position: DomainTextPosition {
                line: line as u32,
                column: column as u32,
            },
            prefix: identifier_prefix(document.buffer.text(), self.editor_area.pane.cursor()),
        })
    }
    pub fn set_completions(&mut self, items: Vec<CompletionItem>) {
        self.editor_area.completion_items = items;
        self.editor_area.completion_selected = 0;
    }

    /// A lista de completação está à mostra.
    #[must_use]
    pub fn completion_open(&self) -> bool {
        !self.editor_area.completion_items.is_empty()
    }

    pub fn clear_completions(&mut self) {
        self.editor_area.completion_items.clear();
        self.editor_area.completion_selected = 0;
    }

    /// O que o texto recém-digitado faz com a lista já aberta.
    ///
    /// Devolve `true` quando a lista precisa ser pedida de novo: cada letra
    /// digitada encurta o que ainda serve, e é isso que faz a lista **acompanhar
    /// o nome sendo escrito** em vez de congelar no que valia quando ela abriu.
    ///
    /// Um caractere que não faz parte de um nome encerra o nome, e a lista sai.
    /// Chamar com a lista fechada não abre nada: abrir é papel do caractere de
    /// disparo da linguagem, ou do `Ctrl+Space`.
    pub fn completion_follow_up(&mut self, typed: &str) -> bool {
        if !self.completion_open() {
            return false;
        }
        if !typed.is_empty() && typed.chars().all(is_identifier_character) {
            return true;
        }
        self.clear_completions();
        false
    }
    /// Mensagem atual da barra de estado.
    #[must_use]
    pub fn status_message(&self) -> &str {
        &self.context.status_message
    }

    pub fn set_status_message(&mut self, message: impl Into<String>) {
        self.context.status_message = message.into();
    }

    /// Instala o modelo visual agregado das contribuições de linguagem.
    ///
    /// Templates, páginas, raízes e tarefas deixam de ser convenções da UI:
    /// trocar o catálogo reconstrói os controles que apresentam esses dados.
    pub fn set_ui_catalog(&mut self, catalog: UiContributionCatalog) {
        let mut project_items = vec![
            MenuItem::new("Compilar projeto", "project.build"),
            MenuItem::new("Reimportar projeto", "project.reimport"),
            MenuItem::new("Executar aplicação", "project.run"),
            MenuItem::new("Parar aplicação", "project.stop"),
        ];
        project_items.extend(catalog.tasks.iter().map(|task| {
            MenuItem::new(
                task.title.clone(),
                CommandId(format!("task.execute.{}", task.id.0)),
            )
        }));
        self.menu.bar = MenuBar::new(
            MENU_BAR_ID,
            vec![
                MenuBarItem::menu(
                    "Arquivo",
                    vec![
                        MenuItem::new("Projeto...", "file.project"),
                        MenuItem::new("Salvar", "file.save"),
                    ],
                ),
                MenuBarItem::menu("Projeto", project_items),
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
        );
        let mut settings_titles = catalog
            .settings_sections
            .iter()
            .map(|section| section.title.clone())
            .collect::<Vec<_>>();
        settings_titles.push(DEBUG_SETTINGS_TITLE.to_owned());
        self.settings.pages = ListView::new(SETTINGS_PAGES_ID, settings_titles)
            .with_row_height(SETTINGS_PAGE_ROW_HEIGHT)
            .with_selection(ListSelection::Marker);
        if let Some(section) = catalog.settings_sections.first() {
            self.settings.toolchain_browse_button =
                Button::new(TOOLCHAIN_BROWSE_ID, section.browse_button_title.clone())
                    .with_command("toolchain.browse");
        }
        if let Some(task) = catalog.tasks.iter().find(|task| task.show_in_toolbar) {
            self.debug_panel.run_button =
                Button::icon(RUN_BUTTON_ID, Icon::Play, task.title.clone())
                    .with_tint(IconTint::Success)
                    .with_command(CommandId(format!("task.execute.{}", task.id.0)));
        }
        self.catalog = catalog;
        self.explorer.tree.set_roots(explorer_items(
            &self.explorer.workspace,
            &self.catalog.source_root_names,
        ));
    }

    #[must_use]
    pub fn ui_catalog(&self) -> &UiContributionCatalog {
        &self.catalog
    }
    pub fn open_settings_dialog(
        &mut self,
        toolchain_items: Vec<String>,
        selected_toolchain: usize,
    ) {
        self.settings.toolchain_combo.set_items(
            toolchain_items
                .into_iter()
                .enumerate()
                .map(|(index, label)| ComboBoxItem::new(label, index.to_string()))
                .collect(),
        );
        self.settings
            .toolchain_combo
            .set_selected(selected_toolchain);
        self.settings.modal.open();
        // Reabrir a janela recomeça a transação: o que ficou pendente de uma
        // abertura anterior foi descartado com ela.
        let segunda = (self.settings.secondary_combo.item_count() > 0)
            .then(|| self.settings.secondary_combo.selected_index());
        self.settings.dialog = Some(SettingsDialog {
            message: None,
            pending_toolchain: None,
            original_toolchain: Some(selected_toolchain),
            pending_secondary: None,
            original_secondary: segunda,
            original_debug_host: self.settings.debug_host.value().to_owned(),
            original_debug_port: self.settings.debug_port.value().to_owned(),
        });
    }

    /// Repõe a lista da segunda escolha da seção, com uma delas marcada.
    ///
    /// Mesma mecânica da primeira: o `Procurar...` põe o que foi apontado na
    /// lista e o deixa pendente, sem recomeçar a transação da janela.
    pub fn set_secondary_tool_options(&mut self, items: Vec<String>, selected: Option<usize>) {
        self.settings.secondary_combo.set_items(
            items
                .into_iter()
                .enumerate()
                .map(|(index, label)| ComboBoxItem::new(label, index.to_string()))
                .collect(),
        );
        if let Some(index) = selected {
            self.settings.secondary_combo.set_selected(index);
        }
        if let Some(dialog) = self.settings.dialog.as_mut() {
            dialog.pending_secondary = selected;
        }
    }

    /// Segunda ferramenta escolhida na janela, para quem for aplicar.
    #[must_use]
    pub fn selected_secondary_tool(&self) -> Option<usize> {
        (self.settings.secondary_combo.item_count() > 0)
            .then(|| self.settings.secondary_combo.selected_index())
    }

    /// Repõe a lista de toolchains e deixa uma delas escolhida, sem sair da transação.
    ///
    /// É o que o `Procurar...` precisa: a instalação apontada entra na lista e
    /// fica pendente como qualquer escolha feita no combo. Reabrir a janela
    /// recomeçaria a transação e apagaria o que já estava pendente.
    pub fn set_toolchain_options(&mut self, toolchain_items: Vec<String>, pending: usize) {
        self.settings.toolchain_combo.set_items(
            toolchain_items
                .into_iter()
                .enumerate()
                .map(|(index, label)| ComboBoxItem::new(label, index.to_string()))
                .collect(),
        );
        self.settings.toolchain_combo.set_selected(pending);
        if let Some(dialog) = self.settings.dialog.as_mut() {
            dialog.pending_toolchain = Some(pending);
            dialog.message = None;
        }
    }

    pub const fn settings_dialog_open(&self) -> bool {
        self.settings.is_open()
    }
    /// Troca o tema da interface.
    ///
    /// O tema vem da ERLibUi e vale para tudo — inclusive para os componentes da
    /// biblioteca, que o recebem pelo contexto de pintura. A IDE não guarda cor
    /// própria.
    pub fn set_theme(&mut self, theme: Theme) {
        self.context.theme = theme;
    }

    #[must_use]
    pub const fn theme(&self) -> &Theme {
        &self.context.theme
    }

    /// Alvo de depuração apresentado na janela e usado pelo botão de depurar.
    pub fn set_debug_target(&mut self, host: &str, port: u16) {
        self.settings.debug_host.set_value(host);
        self.settings.debug_port.set_value(port.to_string());
    }

    #[must_use]
    pub fn debug_target(&self) -> Option<(String, u16)> {
        let host = self.settings.debug_host.value().trim().to_owned();
        let port = self
            .settings
            .debug_port
            .value()
            .trim()
            .parse::<u16>()
            .ok()?;
        (!host.is_empty() && port > 0).then_some((host, port))
    }

    /// Executa um comando na aba de terminal ativa, como se o usuário digitasse.
    pub fn run_in_terminal(&mut self, command: &str) -> Result<(), String> {
        self.terminal.run(command)
    }

    /// Interrompe a aplicação iniciada pela IDE, como um `Ctrl+C` do usuário.
    pub fn stop_application(&mut self) -> Result<(), String> {
        let Some(index) = self.terminal.running_terminal else {
            return Err("Nenhuma aplicação iniciada pela IDE".to_owned());
        };
        let Some(tab) = self.terminal.tabs.get_mut(index) else {
            self.terminal.running_terminal = None;
            return Err("O terminal da aplicação não existe mais".to_owned());
        };
        tab.session.interrupt().map_err(|error| error.to_string())?;
        tab.follow_output = true;
        self.terminal.active = index;
        self.terminal.running_terminal = None;
        self.context.status_message = "Aplicação interrompida".to_owned();
        Ok(())
    }

    /// Indica que a IDE iniciou uma aplicação e ainda não a interrompeu.
    #[must_use]
    pub const fn application_running(&self) -> bool {
        self.terminal.running_terminal.is_some()
    }

    /// Escolhe a página apresentada pela janela de configurações.
    ///
    /// Abrir a janela pelo menu mantém a última página usada; atalhos que
    /// prometem uma página específica precisam declará-la.
    pub fn set_settings_page(&mut self, page: SettingsPage) {
        self.settings.set_page(page);
    }
    #[must_use]
    pub const fn settings_page(&self) -> SettingsPage {
        self.settings.page
    }
    /// Retira, em ordem, todas as intenções produzidas desde a última consulta.
    pub fn drain_application_commands(&mut self) -> Vec<ApplicationCommand> {
        self.commands.drain()
    }

    #[cfg(test)]
    fn take_test_command(
        &mut self,
        predicate: impl Fn(&ApplicationCommand) -> bool,
    ) -> Option<ApplicationCommand> {
        let index = self.commands.iter().position(predicate)?;
        Some(self.commands.remove(index))
    }

    #[cfg(test)]
    fn take_settings_jdk_result(&mut self) -> Option<usize> {
        match self
            .take_test_command(|command| matches!(command, ApplicationCommand::SelectToolchain(_)))
        {
            Some(ApplicationCommand::SelectToolchain(index)) => Some(index),
            _ => None,
        }
    }

    #[cfg(test)]
    fn take_browse_jdk_request(&mut self) -> bool {
        self.take_test_command(|command| matches!(command, ApplicationCommand::BrowseToolchain))
            .is_some()
    }

    #[cfg(test)]
    fn take_navigation_request(&mut self) -> Option<NavigationRequest> {
        match self.take_test_command(|command| matches!(command, ApplicationCommand::Navigate(_))) {
            Some(ApplicationCommand::Navigate(request)) => Some(request),
            _ => None,
        }
    }

    #[cfg(test)]
    fn take_open_project_request(&mut self) -> bool {
        self.take_test_command(|command| matches!(command, ApplicationCommand::OpenProject))
            .is_some()
    }

    #[cfg(test)]
    fn take_breakpoints_dirty(&mut self) -> Option<PathBuf> {
        match self.take_test_command(|command| {
            matches!(command, ApplicationCommand::BreakpointsChanged(_))
        }) {
            Some(ApplicationCommand::BreakpointsChanged(path)) => Some(path),
            _ => None,
        }
    }

    #[cfg(test)]
    fn take_debug_requests(&mut self) -> Vec<DebugRequest> {
        let mut requests = Vec::new();
        self.commands.retain(|command| {
            if let ApplicationCommand::Debug(request) = command {
                requests.push(request.clone());
                false
            } else {
                true
            }
        });
        requests
    }

    #[cfg(test)]
    fn take_build_project_request(&mut self) -> bool {
        self.take_test_command(|command| matches!(command, ApplicationCommand::BuildProject))
            .is_some()
    }

    #[cfg(test)]
    fn take_reimport_project_request(&mut self) -> bool {
        self.take_test_command(|command| matches!(command, ApplicationCommand::ReimportProject))
            .is_some()
    }

    #[cfg(test)]
    fn take_run_request(&mut self) -> bool {
        self.take_test_command(|command| matches!(command, ApplicationCommand::RunProject))
            .is_some()
    }

    #[cfg(test)]
    fn take_stop_request(&mut self) -> bool {
        self.take_test_command(|command| matches!(command, ApplicationCommand::StopProject))
            .is_some()
    }

    #[cfg(test)]
    fn take_open_settings_request(&mut self) -> bool {
        self.take_test_command(|command| matches!(command, ApplicationCommand::OpenSettings))
            .is_some()
    }

    #[cfg(test)]
    fn take_new_item_request(&mut self) -> Option<NewItemRequest> {
        match self.take_test_command(|command| matches!(command, ApplicationCommand::CreateItem(_)))
        {
            Some(ApplicationCommand::CreateItem(request)) => Some(request),
            _ => None,
        }
    }

    #[cfg(test)]
    fn take_type_search_request(&mut self) -> Option<String> {
        match self
            .take_test_command(|command| matches!(command, ApplicationCommand::SearchTypes(_)))
        {
            Some(ApplicationCommand::SearchTypes(query)) => Some(query),
            _ => None,
        }
    }

    #[cfg(test)]
    fn take_content_search_request(&mut self) -> Option<String> {
        match self
            .take_test_command(|command| matches!(command, ApplicationCommand::SearchContent(_)))
        {
            Some(ApplicationCommand::SearchContent(query)) => Some(query),
            _ => None,
        }
    }
    pub fn set_settings_message(&mut self, message: impl Into<String>) {
        if let Some(dialog) = self.settings.dialog.as_mut() {
            dialog.message = Some(message.into());
        }
    }
    pub fn append_tool_output(&mut self, text: &str, is_error: bool) {
        let active = self.terminal.active;
        self.terminal.tabs[active]
            .session
            .append_external_output(text, is_error);
        self.terminal.tabs[active].follow_output = true;
        self.terminal.tabs[active].scroll_line = self.terminal.tabs[active].session.line_count();
    }
    pub fn source_files(&self, expected_extension: &str) -> Vec<PathBuf> {
        fn collect(node: &FileNode, expected_extension: &str, output: &mut Vec<PathBuf>) {
            if node.is_directory {
                for child in &node.children {
                    collect(child, expected_extension, output);
                }
            } else if node
                .path
                .extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| extension.eq_ignore_ascii_case(expected_extension))
            {
                output.push(node.path.clone());
            }
        }
        let mut files = Vec::new();
        collect(&self.explorer.workspace, expected_extension, &mut files);
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
        let Some(document) = self.editor_area.session.active() else {
            return false;
        };
        let mut pane = self.editor_area.pane.clone();
        pane.set_bounds(Rect::new(
            editor_x,
            geometry.content_top,
            geometry.editor_width,
            geometry.editor_height,
        ));
        let offset = pane.offset_at_point(&document.buffer, point);
        if token_at(document.buffer.text(), offset).is_none() {
            return false;
        }
        let (line, column) = line_column(document.buffer.text(), offset);
        self.editor_area
            .syntax_snapshots
            .get(&document.id)
            .filter(|snapshot| snapshot.version == document.buffer.revision())
            .is_some_and(|snapshot| {
                snapshot.highlights.iter().any(|highlight| {
                    is_navigable(highlight.kind) && position_in_range(line, column, highlight.range)
                })
            })
    }
    pub fn tab_count(&self) -> usize {
        self.editor_area.session.tabs().count()
    }

    /// Caminhos das abas abertas, na ordem em que aparecem.
    ///
    /// Documentos criados em memória não têm arquivo por trás e ficam de fora:
    /// registrar um caminho que não existe só produziria uma aba impossível de
    /// reabrir.
    #[must_use]
    pub fn open_document_paths(&self) -> Vec<PathBuf> {
        self.editor_area
            .session
            .tabs()
            .filter(|document| document.is_persistent())
            .map(|document| document.path.clone())
            .collect()
    }

    /// Texto de um documento aberto, para quem vai gravá-lo.
    #[must_use]
    pub fn document_text(&self, document_id: DocumentId) -> Option<String> {
        self.editor_area
            .session
            .document(document_id)
            .map(|document| document.buffer.text().to_owned())
    }

    /// Faz a aba seguir o arquivo que mudou de nome.
    ///
    /// Sem isso a aba continuaria apontando para um caminho que não existe
    /// mais, e a próxima gravação recriaria o arquivo antigo.
    pub fn follow_renamed_path(&mut self, from: &Path, to: &Path) {
        let aberto = self
            .editor_area
            .session
            .tabs()
            .find(|documento| documento.path == from)
            .map(|documento| documento.id);
        if let Some(id) = aberto {
            self.editor_area.session.set_path(id, to.to_path_buf());
        }
    }

    /// Caminho do documento em foco.
    #[must_use]
    pub fn active_document_path(&self) -> Option<PathBuf> {
        self.editor_area
            .session
            .active()
            .filter(|document| document.is_persistent())
            .map(|document| document.path.clone())
    }
    pub fn is_expanded(&self, path: &Path) -> bool {
        self.explorer.is_expanded(path)
    }
    pub fn selected_shell(&self) -> ShellKind {
        self.terminal.selected_shell()
    }
    pub fn editor_scroll_line(&self) -> usize {
        self.editor_area.pane.scroll_line()
    }
    pub fn terminal_scroll_line(&self) -> usize {
        self.terminal.tabs[self.terminal.active].scroll_line
    }
    pub const fn active_terminal_index(&self) -> usize {
        self.terminal.active
    }
    pub const fn terminal_height(&self) -> f32 {
        self.terminal.height
    }
    pub const fn terminal_minimized(&self) -> bool {
        self.terminal.minimized
    }
    pub const fn terminal_resizing(&self) -> bool {
        self.terminal.splitter.is_dragging()
    }
    pub const fn sidebar_resizing(&self) -> bool {
        self.explorer.splitter.is_dragging()
    }
    pub fn active_terminal_lines(&self) -> impl Iterator<Item = &str> {
        self.terminal.active_lines()
    }
    /// Alterna o breakpoint de uma linha e marca o arquivo para sincronização.
    pub fn toggle_breakpoint(&mut self, path: &Path, line: u32) {
        let lines = self
            .debug_panel
            .breakpoints
            .entry(path.to_path_buf())
            .or_default();
        if !lines.remove(&line) {
            lines.insert(line);
        }
        if lines.is_empty() {
            self.debug_panel.breakpoints.remove(path);
        }
        self.commands
            .push(ApplicationCommand::BreakpointsChanged(path.to_path_buf()));
        self.context.status_message = format!("Breakpoints: {}", self.breakpoint_count());
    }

    /// Alterna o breakpoint na linha do cursor do documento ativo.
    pub fn toggle_breakpoint_at_cursor(&mut self) {
        let Some(document) = self.editor_area.session.active() else {
            return;
        };
        let path = document.path.clone();
        let (line, _) = line_column(document.buffer.text(), self.editor_area.pane.cursor());
        self.toggle_breakpoint(&path, line as u32);
    }

    #[must_use]
    pub fn breakpoints_for(&self, path: &Path) -> Vec<u32> {
        self.debug_panel.breakpoints_for(path)
    }

    #[must_use]
    pub fn breakpoint_count(&self) -> usize {
        self.debug_panel
            .breakpoints
            .values()
            .map(BTreeSet::len)
            .sum()
    }

    /// Registra quais linhas o alvo confirmou, para diferenciá-las na calha.
    pub fn set_verified_breakpoints(&mut self, path: &Path, lines: &[u32]) {
        if lines.is_empty() {
            self.debug_panel.verified_breakpoints.remove(path);
        } else {
            self.debug_panel
                .verified_breakpoints
                .insert(path.to_path_buf(), lines.iter().copied().collect());
        }
    }

    #[must_use]
    pub fn breakpoint_is_verified(&self, path: &Path, line: u32) -> bool {
        self.debug_panel
            .verified_breakpoints
            .get(path)
            .is_some_and(|lines| lines.contains(&line))
    }

    pub fn request_debug(&mut self, request: DebugRequest) {
        self.commands.push(ApplicationCommand::Debug(request));
    }

    /// Substitui o estado de depuração apresentado.
    pub fn set_debug_view(&mut self, view: DebugView) {
        if let Some((path, line)) = view.stopped_at.clone()
            && self.debug_panel.view.stopped_at.as_ref() != Some(&(path.clone(), line))
        {
            self.commands.push(ApplicationCommand::OpenDocument(
                OpenDocumentRequest::new(path).at(line as usize, 0),
            ));
        }
        self.debug_panel.view = view;
        self.debug_panel.frames.set_items(
            self.debug_panel
                .view
                .frames
                .iter()
                .map(|frame| match &frame.location {
                    Some((_, line)) => format!("{}:{}", frame.name, line + 1),
                    None => frame.name.clone(),
                })
                .collect::<Vec<_>>(),
        );
        self.debug_panel
            .frames
            .set_selected(Some(self.debug_panel.view.selected_frame));
        self.debug_panel.variables.set_items(
            self.debug_panel
                .view
                .variables
                .iter()
                .map(|variable| format!("{} = {}", variable.name, variable.value))
                .collect::<Vec<_>>(),
        );
    }

    #[must_use]
    pub const fn debug_view(&self) -> &DebugView {
        &self.debug_panel.view
    }

    #[must_use]
    pub const fn debug_attached(&self) -> bool {
        self.debug_panel.attached()
    }

    /// Resumo do projeto importado, apresentado na barra de status.
    pub fn set_project_summary(&mut self, summary: Option<String>) {
        self.context.project_summary = summary;
    }
    pub fn project_summary(&self) -> Option<&str> {
        self.context.project_summary.as_deref()
    }
    pub fn workspace_path(&self) -> &Path {
        &self.explorer.workspace.path
    }
    pub fn active_terminal_input(&self) -> &str {
        self.active_terminal().input()
    }

    fn active_terminal(&self) -> &TerminalSession {
        self.terminal.active()
    }

    fn active_terminal_mut(&mut self) -> &mut TerminalSession {
        self.terminal.active_session_mut()
    }

    pub fn update_terminals(&mut self, size: Size) -> bool {
        let geo = self.geometry(size);
        let rows = ((geo.terminal_height - 62.0) / EDITOR_LINE_HEIGHT).max(1.0) as u16;
        let mut changed = false;
        for terminal in &mut self.terminal.tabs {
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
            if self.terminal.minimized {
                TERMINAL_COLLAPSED_HEIGHT
            } else {
                self.terminal.height
            },
            self.sidebar_width(size),
        );
        // O painel de depuração ocupa a direita do editor enquanto há sessão,
        // em vez de cobrir o código.
        if self.debug_panel.view.attached {
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
            .editor_area
            .session
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
            .with_pointer(self.context.pointer);
        if let Some(active) = self.editor_area.session.active_id() {
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
            .terminal
            .tabs
            .iter()
            .enumerate()
            .map(|(index, terminal)| {
                TabItem::new(
                    index as u64,
                    terminal.session.selected_profile().kind.label(),
                )
            })
            .collect();
        let mut tabs = Tabs::new(TERMINAL_TABS_ID, items).with_tab_width(TERMINAL_TAB_WIDTH);
        tabs.set_active(self.terminal.active);
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
                self.editor_area.pane.scroll_offset() / EDITOR_LINE_HEIGHT,
            ),
            ScrollTarget::Terminal => {
                let active = &self.terminal.tabs[self.terminal.active];
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
                self.explorer.scroll_line as f32,
            ),
            ScrollTarget::EditorHorizontal => {
                let track = self.editor_horizontal_scrollbar_rect(size);
                (
                    track,
                    self.editor_area.pane.content_width(),
                    (track.size.width - 28.0).max(1.0),
                    self.editor_area.pane.scroll_x(),
                )
            }
            ScrollTarget::ExplorerHorizontal => {
                let track = self.explorer_horizontal_scrollbar_rect(size);
                (
                    track,
                    self.explorer_content_width(size),
                    (track.size.width - 28.0).max(1.0),
                    self.explorer.scroll_x,
                )
            }
        }
    }

    /// Onde o cursor está, em linha e coluna.
    ///
    /// É o que a linguagem precisa para saber de qual tipo se está falando.
    #[must_use]
    pub fn cursor_position(&self) -> Option<DomainTextPosition> {
        let text = self.active_text()?;
        let (line, column) = line_column(text, self.editor_area.pane.cursor());
        Some(DomainTextPosition {
            line: line as u32,
            column: column as u32,
        })
    }

    /// Pede à linguagem o plano de acessores. É o que o menu `Generate` aciona.
    pub fn request_accessors(&mut self, kind: AccessorKind) {
        self.editor_area.generate_pending = Some(kind);
    }

    /// Pedido que a tela quer ver respondido, se houver um esperando.
    pub fn take_accessor_request(&mut self) -> Option<AccessorKind> {
        self.editor_area.generate_pending.take()
    }

    /// Campos escolhidos para o construtor, se houver um pedido esperando.
    ///
    /// Devolve a lista e onde o texto entra; quem tem a linguagem monta o
    /// construtor e responde por [`IdeShell::insert_constructor`].
    pub fn take_constructor_request(&mut self) -> Option<(Vec<String>, DomainTextPosition)> {
        self.editor_area
            .constructor_pending
            .take()
            .map(|pedido| (pedido.fields, pedido.insert_at))
    }

    /// Escreve o construtor que a linguagem montou.
    ///
    /// `None` é o tipo já ter um construtor com a mesma assinatura: escrever
    /// outro não compilaria, e avisar é melhor do que entregar arquivo quebrado.
    ///
    /// Devolve se o documento mudou — é o que diz a quem chamou que o realce
    /// precisa ser pedido de novo, senão o código gerado fica sem cor até a
    /// primeira tecla.
    pub fn insert_constructor(
        &mut self,
        source: Option<String>,
        insert_at: DomainTextPosition,
    ) -> bool {
        let Some(source) = source else {
            self.set_status_message("Esse construtor já existe");
            return false;
        };
        let Some(document) = self.editor_area.session.active_mut() else {
            return false;
        };
        // A linha vem da linguagem, que sabe onde o tipo abre e fecha.
        let texto = document.buffer.text();
        let inicio = offset_of_line(texto, insert_at.line as usize);
        if document.buffer.replace(inicio..inicio, &source).is_err() {
            return false;
        }
        self.set_status_message("Construtor gerado");
        true
    }

    /// Abre a janela com o que a linguagem propôs gerar.
    ///
    /// Só entram os campos que ainda **não têm** o acessor: listar o que já
    /// existe daria a escolher algo que não seria escrito.
    pub fn show_accessor_plan(&mut self, kind: AccessorKind, plan: AccessorPlan) {
        // O construtor lista **todos** os campos: nenhum deles "já existe", e a
        // escolha é sobre quais entram por parâmetro. Os acessores listam só o
        // que falta, porque oferecer o que já existe daria a escolher algo que
        // não seria escrito.
        let candidates: Vec<AccessorCandidate> = match kind {
            AccessorKind::Constructor => plan.candidates,
            _ => plan
                .candidates
                .into_iter()
                .filter(|candidate| candidate.source.is_some())
                .collect(),
        };
        if candidates.is_empty() && kind != AccessorKind::Constructor {
            self.set_status_message("Todos os campos já têm esse acessor");
            return;
        }
        let checked = vec![false; candidates.len()];
        let list = generate_list(&candidates, &checked);
        self.editor_area.generate = Some(GenerateState {
            kind,
            checked,
            candidates,
            insert_at: plan.insert_at,
            list,
        });
        self.editor_area.generate_modal.set_title(match kind {
            AccessorKind::Getter => "Generate — Getter",
            AccessorKind::Setter => "Generate — Setter",
            AccessorKind::Both => "Generate — Getter and Setter",
            AccessorKind::Constructor => "Generate — Constructor",
        });
        self.editor_area.generate_modal.open();
    }

    #[must_use]
    pub fn generate_open(&self) -> bool {
        self.editor_area.generate_modal.is_open()
    }

    /// Campos oferecidos na janela, na ordem em que aparecem.
    #[must_use]
    pub fn generate_fields(&self) -> Vec<String> {
        self.editor_area
            .generate
            .as_ref()
            .map(|state| {
                state
                    .candidates
                    .iter()
                    .map(|candidate| candidate.field.clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn close_generate(&mut self) {
        self.editor_area.generate_modal.close();
        self.editor_area.generate = None;
    }

    /// Escreve os acessores escolhidos no documento.
    ///
    /// `todos` ignora a marcação — é o botão que gera tudo de uma vez, sem
    /// obrigar a marcar campo por campo quando se quer a classe inteira.
    pub fn apply_generate(&mut self, todos: bool) {
        let Some(state) = self.editor_area.generate.take() else {
            return;
        };
        // O construtor é um texto só, montado a partir do conjunto: quem monta é
        // a linguagem, e por isso ele é pedido agora, com a escolha na mão. Sem
        // nada marcado o pedido vai vazio, que é o construtor sem parâmetros.
        if state.kind == AccessorKind::Constructor {
            let fields = state
                .candidates
                .iter()
                .enumerate()
                .filter(|(index, _)| {
                    todos || state.checked.get(*index).copied().unwrap_or_default()
                })
                .map(|(_, candidate)| candidate.field.clone())
                .collect();
            self.editor_area.generate_modal.close();
            self.editor_area.constructor_pending = Some(ConstructorRequest {
                fields,
                insert_at: state.insert_at,
            });
            return;
        }
        let escolhidos: String = state
            .candidates
            .iter()
            .enumerate()
            .filter(|(index, _)| todos || state.checked.get(*index).copied().unwrap_or_default())
            .filter_map(|(_, candidate)| candidate.source.clone())
            .collect();
        self.editor_area.generate_modal.close();
        if escolhidos.is_empty() {
            self.set_status_message("Nenhum campo marcado");
            return;
        }
        let Some(document) = self.editor_area.session.active_mut() else {
            return;
        };
        // A linha vem da linguagem, que sabe onde o tipo fecha.
        let texto = document.buffer.text();
        let inicio = offset_of_line(texto, state.insert_at.line as usize);
        if document.buffer.replace(inicio..inicio, &escolhidos).is_ok() {
            let quantos = state
                .candidates
                .iter()
                .enumerate()
                .filter(|(index, candidate)| {
                    candidate.source.is_some()
                        && (todos || state.checked.get(*index).copied().unwrap_or_default())
                })
                .count();
            let nome = match state.kind {
                AccessorKind::Getter => "getter",
                AccessorKind::Setter => "setter",
                // O construtor não passa por aqui: ele tem caminho próprio.
                AccessorKind::Both | AccessorKind::Constructor => "acessor",
            };
            self.set_status_message(format!("{quantos} {nome}(s) gerado(s)"));
        }
    }

    /// Pede a renomeação do arquivo escolhido na árvore.
    ///
    /// O menu só marca; quem responde é a aplicação, que pergunta à linguagem
    /// onde o nome aparece no projeto — inclusive em arquivos fechados.
    pub fn request_rename(&mut self, path: PathBuf) {
        self.editor_area.rename_pending = Some(path);
    }

    /// Arquivo cuja renomeação a tela quer ver respondida, se houver um.
    pub fn take_rename_request(&mut self) -> Option<PathBuf> {
        self.editor_area.rename_pending.take()
    }

    /// Abre a janela de renomear com o arquivo e o que será reescrito junto.
    ///
    /// A lista mostra **os arquivos afetados**, com quantas ocorrências cada um
    /// tem: é o alcance da mudança, e é o que o usuário confirma ao clicar OK.
    pub fn show_rename(&mut self, path: PathBuf, references: Vec<Location>) {
        let old_name = path
            .file_stem()
            .and_then(|valor| valor.to_str())
            .unwrap_or_default()
            .to_owned();
        let mut por_arquivo: BTreeMap<PathBuf, Vec<DomainTextRange>> = BTreeMap::new();
        for location in references {
            por_arquivo
                .entry(location.path)
                .or_default()
                .push(location.range);
        }
        let occurrences: Vec<(PathBuf, Vec<DomainTextRange>)> = por_arquivo
            .into_iter()
            .map(|(caminho, mut ranges)| {
                // Do fim para o começo: trocar no começo moveria as seguintes.
                ranges.sort_by(|esquerda, direita| {
                    (direita.start.line, direita.start.column)
                        .cmp(&(esquerda.start.line, esquerda.start.column))
                });
                ranges.dedup();
                (caminho, ranges)
            })
            .collect();
        let rotulos: Vec<String> = occurrences
            .iter()
            .map(|(caminho, ranges)| rename_reference_label(caminho, ranges.len()))
            .collect();
        let mut input = TextInput::new(RENAME_INPUT_ID, old_name.clone());
        input.event(&mut EventContext::default(), &UiEvent::FocusGained);
        self.editor_area.rename = Some(RenameState {
            input,
            list: ListView::new(RENAME_LIST_ID, rotulos),
            path,
            old_name,
            occurrences,
        });
        self.editor_area.rename_modal.open();
    }

    #[must_use]
    pub fn rename_open(&self) -> bool {
        self.editor_area.rename_modal.is_open()
    }

    /// Nome que está no campo, que é o que a confirmação aplica.
    #[must_use]
    pub fn rename_name(&self) -> String {
        self.editor_area
            .rename
            .as_ref()
            .map(|state| state.input.value().to_owned())
            .unwrap_or_default()
    }

    /// Arquivos afetados, como aparecem na lista.
    #[must_use]
    pub fn rename_references(&self) -> Vec<String> {
        self.editor_area
            .rename
            .as_ref()
            .map(|state| {
                state
                    .occurrences
                    .iter()
                    .map(|(caminho, ranges)| rename_reference_label(caminho, ranges.len()))
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn cancel_rename(&mut self) {
        self.editor_area.rename_modal.close();
        self.editor_area.rename = None;
    }

    /// Confirma: manda renomear o arquivo e reescrever tudo o que o cita.
    ///
    /// A tela não escreve em disco nem em arquivo fechado — ela entrega o nome
    /// novo e a lista de onde trocar. Quem escreve é a aplicação, de uma vez só.
    pub fn apply_rename(&mut self) {
        let Some(state) = self.editor_area.rename.take() else {
            return;
        };
        self.editor_area.rename_modal.close();
        let novo = state.input.value().trim().to_owned();
        if novo.is_empty() || novo == state.old_name {
            self.set_status_message("Nome inalterado");
            return;
        }
        let extensao = state
            .path
            .extension()
            .and_then(|valor| valor.to_str())
            .map(|valor| format!(".{valor}"))
            .unwrap_or_default();
        let destino = state.path.with_file_name(format!("{novo}{extensao}"));
        let arquivos = state.occurrences.len();
        // O que está aberto é reescrito aqui, no buffer: a aba mantém cursor,
        // desfazer e alterações não salvas. Gravar por cima delas perderia
        // trabalho que a tela tem e o disco não.
        let mut fechados = Vec::new();
        for (caminho, ranges) in state.occurrences {
            let aberto = self
                .editor_area
                .session
                .tabs()
                .find(|documento| documento.path == caminho)
                .map(|documento| documento.id);
            match aberto {
                Some(id) => self.rewrite_open_document(id, &ranges, &state.old_name, &novo),
                None => fechados.push(FileOccurrences {
                    path: caminho,
                    ranges,
                }),
            }
        }
        self.commands
            .push(ApplicationCommand::RenameDocument(RenameDocumentRequest {
                from: state.path,
                to: destino,
                old_name: state.old_name,
                new_name: novo,
                occurrences: fechados,
            }));
        self.set_status_message(format!("Renomeando em {arquivos} arquivo(s)"));
    }

    /// Reescreve um documento aberto, mantendo a aba e o desfazer.
    fn rewrite_open_document(
        &mut self,
        document_id: DocumentId,
        ranges: &[DomainTextRange],
        antigo: &str,
        novo: &str,
    ) {
        let Some(document) = self.editor_area.session.document_mut(document_id) else {
            return;
        };
        let texto = rewrite_occurrences(document.buffer.text(), ranges, antigo, novo);
        let total = document.buffer.text().len();
        let _ = document.buffer.replace(0..total, &texto);
    }

    /// Roteia o clique dentro da janela de renomear.
    fn rename_pointer_down(&mut self, point: Point, size: Size) {
        self.editor_area.rename_modal.layout(
            &self.layout_context(),
            Rect::new(0.0, 0.0, size.width, size.height),
        );
        let geometry = rename_geometry(self.editor_area.rename_modal.panel_bounds());
        if geometry.ok.contains(point) {
            self.apply_rename();
            return;
        }
        if geometry.cancel.contains(point) {
            self.cancel_rename();
            return;
        }
        let contexto = self.layout_context();
        // O campo sabe onde o cursor cai dentro do texto: a medição é dele.
        if geometry.input.contains(point)
            && let Some(state) = self.editor_area.rename.as_mut()
        {
            state.input.layout(&contexto, geometry.input);
            state.input.event(
                &mut EventContext::default(),
                &UiEvent::PointerDown(primary_pointer(point)),
            );
            return;
        }
        if geometry.list.contains(point) {
            self.rename_list_event(&UiEvent::PointerDown(primary_pointer(point)), size);
        }
    }

    /// Entrega um evento de ponteiro à lista da janela de renomear.
    ///
    /// A lista é quem tem as barras, e é ela que decide se o gesto é de uma
    /// delas ou de uma linha.
    fn rename_list_event(&mut self, event: &UiEvent, size: Size) {
        self.editor_area.rename_modal.layout(
            &self.layout_context(),
            Rect::new(0.0, 0.0, size.width, size.height),
        );
        let geometry = rename_geometry(self.editor_area.rename_modal.panel_bounds());
        let contexto = self.layout_context();
        if let Some(state) = self.editor_area.rename.as_mut() {
            state.list.layout(&contexto, geometry.list);
            state.list.event(&mut EventContext::default(), event);
        }
    }

    /// Teclas da janela de renomear: `Enter` confirma, `Esc` desiste.
    fn rename_key(&mut self, key: &str, modifiers: Modifiers) -> bool {
        if !self.editor_area.rename_modal.is_open() {
            return false;
        }
        match key.to_ascii_lowercase().as_str() {
            "enter" => self.apply_rename(),
            "escape" => self.cancel_rename(),
            outra => {
                if let Some(state) = self.editor_area.rename.as_mut() {
                    state.input.event(
                        &mut EventContext::default(),
                        &UiEvent::KeyDown(KeyEvent {
                            logical_key: outra.to_owned(),
                            repeat: false,
                            modifiers,
                        }),
                    );
                }
            }
        }
        true
    }

    /// Texto digitado enquanto a janela de renomear está aberta.
    fn rename_text_input(&mut self, text: &str) -> bool {
        if !self.editor_area.rename_modal.is_open() {
            return false;
        }
        if let Some(state) = self.editor_area.rename.as_mut() {
            state.input.event(
                &mut EventContext::default(),
                &UiEvent::TextInput(TextInputEvent {
                    text: text.to_owned(),
                }),
            );
        }
        true
    }

    fn paint_rename(&mut self, commands: &mut Vec<PaintCommand>, size: Size) {
        if !self.editor_area.rename_modal.is_open() || self.editor_area.rename.is_none() {
            return;
        }
        let mut modal = self.editor_area.rename_modal.clone();
        modal.layout(
            &self.layout_context(),
            Rect::new(0.0, 0.0, size.width, size.height),
        );
        let geometry = rename_geometry(modal.panel_bounds());
        let mut paint = self.paint_context();
        modal.paint(&mut paint);
        let legenda = self
            .editor_area
            .rename
            .as_ref()
            .map(|state| match state.occurrences.len() {
                0 => "Nada mais no projeto usa este nome".to_owned(),
                1 => "1 arquivo será reescrito".to_owned(),
                quantos => format!("{quantos} arquivos serão reescritos"),
            })
            .unwrap_or_default();
        self.paint_settings_text(
            &mut paint,
            RENAME_NAME_CAPTION_ID,
            "Novo nome",
            Point::new(geometry.input.origin.x, geometry.input.origin.y - 18.0),
            13.0,
            IconTint::Muted,
        );
        self.paint_settings_text(
            &mut paint,
            RENAME_LIST_CAPTION_ID,
            &legenda,
            Point::new(geometry.list.origin.x, geometry.list.origin.y - 18.0),
            13.0,
            IconTint::Muted,
        );
        let contexto = self.layout_context();
        if let Some(state) = self.editor_area.rename.as_mut() {
            state.input.layout(&contexto, geometry.input);
            state.input.paint(&mut paint);
            state.list.layout(&contexto, geometry.list);
            state.list.paint(&mut paint);
        }
        let mut cancelar = Button::new(RENAME_CANCEL_ID, "Cancelar");
        cancelar.layout(&contexto, geometry.cancel);
        cancelar.paint(&mut paint);
        let mut ok = Button::new(RENAME_OK_ID, "OK");
        ok.layout(&contexto, geometry.ok);
        ok.paint(&mut paint);
        commands.extend(paint.into_commands());
    }

    /// Áreas da janela de geração: a lista e os dois botões.
    fn generate_geometry(&mut self, size: Size) -> (Rect, Rect, Rect) {
        self.editor_area.generate_modal.layout(
            &self.layout_context(),
            Rect::new(0.0, 0.0, size.width, size.height),
        );
        let panel = self.editor_area.generate_modal.panel_bounds();
        let botoes_y = panel.origin.y + panel.size.height - 52.0;
        let lista = Rect::new(
            panel.origin.x + 16.0,
            panel.origin.y + 56.0,
            panel.size.width - 32.0,
            botoes_y - (panel.origin.y + 56.0) - 12.0,
        );
        let ok = Rect::new(
            panel.origin.x + panel.size.width - 116.0,
            botoes_y,
            100.0,
            36.0,
        );
        let todos = Rect::new(ok.origin.x - 112.0, botoes_y, 100.0, 36.0);
        (lista, todos, ok)
    }

    fn generate_pointer_down(&mut self, point: Point, size: Size) {
        let (lista, todos, ok) = self.generate_geometry(size);
        if todos.contains(point) {
            self.apply_generate(true);
            return;
        }
        if ok.contains(point) {
            self.apply_generate(false);
            return;
        }
        if lista.contains(point) {
            // A lista trata a trilha e diz qual linha foi clicada; a marcação
            // é da tela, que é quem sabe o que está marcado.
            let contexto = self.layout_context();
            let Some(state) = self.editor_area.generate.as_mut() else {
                return;
            };
            state.list.layout(&contexto, lista);
            let antes = state.list.selected();
            state.list.event(
                &mut EventContext::default(),
                &UiEvent::PointerDown(primary_pointer(point)),
            );
            let agora = state.list.selected();
            if (agora != antes || !state.list.scrolls())
                && let Some(linha) = agora
                && let Some(marcado) = state.checked.get_mut(linha)
            {
                *marcado = !*marcado;
                state.list = generate_list(&state.candidates, &state.checked);
            }
            return;
        }
        if !self.editor_area.generate_modal.panel_bounds().contains(point) {
            self.close_generate();
        }
    }

    /// Desenha a janela de geração.
    ///
    /// A lista é a `ComposedList` da biblioteca, e **quem escolhe as células é
    /// esta tela**: uma caixa de marcação e o nome do campo.
    fn paint_generate(&mut self, commands: &mut Vec<PaintCommand>, size: Size) {
        if !self.editor_area.generate_modal.is_open() {
            return;
        }
        if self.editor_area.generate.is_none() {
            return;
        }
        let mut modal = self.editor_area.generate_modal.clone();
        modal.layout(
            &self.layout_context(),
            Rect::new(0.0, 0.0, size.width, size.height),
        );
        let mut paint = self.paint_context();
        modal.paint(&mut paint);
        let panel = modal.panel_bounds();
        let botoes_y = panel.origin.y + panel.size.height - 52.0;
        let lista_rect = Rect::new(
            panel.origin.x + 16.0,
            panel.origin.y + 56.0,
            panel.size.width - 32.0,
            botoes_y - (panel.origin.y + 56.0) - 12.0,
        );
        let contexto = self.layout_context();
        // A lista é a mesma entre quadros: recriá-la aqui jogaria fora a
        // rolagem e a deixaria sem receber evento nenhum.
        if let Some(state) = self.editor_area.generate.as_mut() {
            state.list.layout(&contexto, lista_rect);
            state.list.paint(&mut paint);
        }

        let ok_rect = Rect::new(
            panel.origin.x + panel.size.width - 116.0,
            botoes_y,
            100.0,
            36.0,
        );
        let todos_rect = Rect::new(ok_rect.origin.x - 112.0, botoes_y, 100.0, 36.0);
        let mut todos = Button::new(GENERATE_ALL_ID, "All");
        todos.layout(&self.layout_context(), todos_rect);
        todos.paint(&mut paint);
        let mut ok = Button::new(GENERATE_OK_ID, "OK");
        ok.layout(&self.layout_context(), ok_rect);
        ok.paint(&mut paint);
        commands.extend(paint.into_commands());
    }

    /// O ponto clicado cai dentro de uma classe, interface, enum ou anotação.
    ///
    /// A estrutura vem do outline que a linguagem já publica: perguntar de novo
    /// ao provider no meio de um clique seria uma ida síncrona à linguagem para
    /// decidir o que mostrar num menu.
    fn cursor_inside_type(&mut self, point: Point, size: Size) -> bool {
        self.place_focused_editor(size);
        let Some(buffer) = self
            .editor_area
            .session
            .active()
            .map(|document| &document.buffer)
        else {
            return false;
        };
        let offset = self.editor_area.pane.offset_at_point(buffer, point);
        let (line, column) = line_column(buffer.text(), offset);
        let posicao = DomainTextPosition {
            line: line as u32,
            column: column as u32,
        };
        self.active_outline()
            .iter()
            .any(|item| encloses_type(item, posicao))
    }

    /// Há linha passando da área visível, e portanto barra lateral.
    fn editor_scrolls_sideways(&self, size: Size) -> bool {
        self.editor_area.pane.content_width() > self.editor_view_rect(size).size.width
    }

    /// Trilha da barra lateral do editor, rente à borda de baixo da área.
    ///
    /// Ela para antes da barra vertical: duas trilhas cruzadas no canto
    /// disputariam o mesmo clique.
    fn editor_horizontal_scrollbar_rect(&self, size: Size) -> Rect {
        let geo = self.geometry(size);
        let editor_x = ACTIVITY_WIDTH + self.sidebar_width(size);
        Rect::new(
            editor_x,
            geo.editor_bottom - 10.0,
            (geo.editor_width - 10.0).max(0.0),
            10.0,
        )
    }

    fn scrollbar_mut(&mut self, target: ScrollTarget) -> &mut Scrollbar {
        match target {
            ScrollTarget::Editor => &mut self.editor_area.scrollbar,
            ScrollTarget::Terminal => &mut self.terminal.scrollbar,
            ScrollTarget::ExplorerVertical => &mut self.explorer.vertical_scrollbar,
            ScrollTarget::EditorHorizontal => &mut self.editor_area.horizontal_scrollbar,
            ScrollTarget::ExplorerHorizontal => &mut self.explorer.horizontal_scrollbar,
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
            // A barra também fala em pixels: arredondar para linha aqui
            // devolveria o salto que a rolagem contínua veio tirar.
            ScrollTarget::Editor => self
                .editor_area
                .pane
                .set_scroll_offset((offset * EDITOR_LINE_HEIGHT).max(0.0)),
            ScrollTarget::Terminal => {
                let maximum = self.terminal.scrollbar.max_offset();
                let active = self.terminal.active;
                self.terminal.tabs[active].scroll_line = offset.round().max(0.0) as usize;
                // Chegar ao fim volta a acompanhar a saída; parar no meio é
                // pedir para ficar onde está.
                self.terminal.tabs[active].follow_output = offset >= maximum;
            }
            ScrollTarget::ExplorerVertical => {
                self.explorer.scroll_line = offset.round().max(0.0) as usize;
            }
            ScrollTarget::EditorHorizontal => self.editor_area.pane.set_scroll_x(offset.max(0.0)),
            ScrollTarget::ExplorerHorizontal => self.explorer.scroll_x = offset.max(0.0),
        }
    }

    /// Entrega o clique à barra cuja trilha o contém.
    fn scrollbar_pointer_down(&mut self, point: Point, size: Size) -> bool {
        for target in [
            ScrollTarget::Terminal,
            ScrollTarget::Editor,
            ScrollTarget::EditorHorizontal,
            ScrollTarget::ExplorerHorizontal,
            ScrollTarget::ExplorerVertical,
        ] {
            if target == ScrollTarget::Terminal && self.terminal.minimized {
                continue;
            }
            // A barra lateral do editor só existe quando há linha passando da
            // área. Sem esta guarda ela tomaria o clique da borda do terminal,
            // que fica na mesma altura, sem sequer estar desenhada.
            if target == ScrollTarget::EditorHorizontal && !self.editor_scrolls_sideways(size) {
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
                self.context.scrollbar_drag = Some(target);
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
            ScrollTarget::EditorHorizontal | ScrollTarget::ExplorerHorizontal => {
                ScrollbarOrientation::Horizontal
            }
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
        let ids: Vec<u64> = self
            .explorer
            .expanded
            .iter()
            .map(|path| explorer_id(path))
            .collect();
        self.explorer.tree.set_expanded(ids);
    }

    /// Expande, seleciona e revela no Explorer o arquivo da aba ativa.
    ///
    /// A restauração abre as abas antes do primeiro frame. Fazer a reconciliação
    /// aqui deixa a árvore nascer no mesmo documento do editor, em vez de exigir
    /// que o usuário repita toda a navegação manualmente.
    fn sync_explorer_to_active(&mut self) {
        let Some(path) = self.active_document_path() else {
            self.explorer.tree.set_selected(None);
            return;
        };
        let target = explorer_id(&path);
        if self.explorer_path_for(target).is_none() {
            return;
        }

        for ancestor in path.ancestors().skip(1) {
            if !ancestor.starts_with(&self.explorer.workspace.path) {
                break;
            }
            self.explorer.expanded.insert(ancestor.to_path_buf());
        }
        self.sync_explorer_tree();
        self.explorer.tree.set_selected(Some(target));

        let expanded = self
            .explorer
            .expanded
            .iter()
            .map(|path| explorer_id(path))
            .collect::<HashSet<_>>();
        if let Some(row) = visible_tree_row(
            &explorer_items(&self.explorer.workspace, &self.catalog.source_root_names),
            &expanded,
            target,
        ) {
            // Duas linhas de contexto ajudam a reconhecer o pacote pai. A cópia
            // da TreeView usada na pintura limita o deslocamento no fim da lista.
            self.explorer.scroll_line = row.saturating_sub(2);
        }
    }

    /// Posiciona a árvore de acordo com as barras de rolagem da janela.
    fn explorer_tree_for(&self, size: Size) -> TreeView {
        let mut tree = self.explorer.tree.clone();
        tree.layout(&self.layout_context(), self.explorer_tree_rect(size));
        tree.set_scroll_offset(Point::new(
            self.explorer.scroll_x,
            self.explorer.scroll_line as f32 * EXPLORER_ROW_HEIGHT,
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
        visit(&self.explorer.workspace, id)
    }

    /// Divisor da barra lateral posicionado pelo layout atual.
    ///
    /// A barra lateral é limitada pela largura mínima dela e pela do editor; o
    /// terminal, pela altura mínima dele e pelo espaço que o editor precisa
    /// manter. São limites em pontos, não proporções.
    fn sidebar_splitter_for(&self, size: Size) -> Splitter {
        let geometry = self.geometry(size);
        let mut splitter = self.explorer.splitter.clone();
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
        let mut splitter = self.terminal.splitter.clone();
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
        self.explorer.splitter = self.sidebar_splitter_for(size);
        self.terminal.splitter = self.terminal_splitter_for(size);
    }

    /// Traz de volta o tamanho que cada divisor definiu.
    fn apply_splitters(&mut self, size: Size) {
        let content_bottom = self.geometry(size).content_bottom;
        if self.explorer.splitter.is_dragging() {
            self.explorer.sidebar_width = self.explorer.splitter.position() - ACTIVITY_WIDTH;
        }
        if self.terminal.splitter.is_dragging() {
            self.terminal.height = content_bottom - self.terminal.splitter.position();
            self.terminal.last_height = self.terminal.height;
        }
    }

    /// Entrega o clique ao divisor cujo alvo o contém.
    fn splitter_pointer_down(&mut self, point: Point, size: Size) -> bool {
        self.sync_splitters(size);
        let event = UiEvent::PointerDown(primary_pointer(point));
        let mut context = EventContext::default();
        if matches!(
            self.explorer.splitter.event(&mut context, &event),
            EventResult::Handled
        ) {
            return true;
        }
        !self.terminal.minimized
            && matches!(
                self.terminal.splitter.event(&mut context, &event),
                EventResult::Handled
            )
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

    /// Alinha o painel de edição ao documento, ao realce e às decorações.
    ///
    /// O texto vive no documento; o painel guarda cursor, seleção, rolagem e a
    /// cópia de desenho, e só refaz esta última quando a revisão muda.
    fn sync_editor_pane(&mut self, size: Size) {
        let Some(document) = self.editor_area.session.active() else {
            return;
        };
        let (id, revision, path) = (
            document.id,
            document.buffer.revision(),
            document.path.clone(),
        );
        let decorations = self.editor_decorations(&path);
        let focused = self.context.focus == ShellFocus::Editor;
        let bounds = self.editor_view_rect(size);
        let context = self.layout_context();
        let editor_area = &mut self.editor_area;
        editor_area.pane.set_bounds(bounds);
        // Qual documento o painel edita: trocar de aba precisa jogar fora a cópia
        // de desenho, o desfazer e as marcas, que falam do texto anterior.
        editor_area.pane.set_source(id.0);
        let Some(document) = editor_area.session.active() else {
            return;
        };
        let syntax = editor_area
            .syntax_spans
            .get(&id)
            .filter(|cached| cached.version == revision)
            .map(|cached| SyntaxView {
                version: cached.version,
                spans: &cached.spans,
            });
        editor_area
            .pane
            .sync(&context, &document.buffer, syntax, decorations, focused);
    }

    /// Pontos de parada e a linha em que a execução parou.
    fn editor_decorations(&self, path: &Path) -> Vec<LineDecoration> {
        let mut decorations: Vec<LineDecoration> = self
            .debug_panel
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
        let destacar = |decorations: &mut Vec<LineDecoration>, line: usize| match decorations
            .iter_mut()
            .find(|item| item.line == line)
        {
            Some(existing) => *existing = existing.with_highlight(),
            None => decorations.push(LineDecoration::highlight(line)),
        };
        if let Some((_, line)) = self
            .debug_panel
            .view
            .stopped_at
            .as_ref()
            .filter(|(stopped, _)| stopped == path)
        {
            destacar(&mut decorations, *line as usize);
        }
        // Destino da última navegação, enquanto o cursor não sair de lá.
        if let Some((line, cursor)) = self.editor_area.navigated
            && cursor == self.editor_area.pane.cursor()
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
        let geometry = debug_panel_geometry(
            self.debug_panel_rect(size),
            self.debug_panel.view.frames.len(),
        );
        let panel = geometry.panel;
        let mut commands = vec![
            fill(panel, colors.surface),
            fill(
                Rect::new(panel.origin.x, panel.origin.y, 1.0, panel.size.height),
                colors.border,
            ),
            PaintCommand::PushClip(panel),
            label(
                &self.debug_panel.view.status,
                Point::new(panel.origin.x + 12.0, panel.origin.y + 10.0),
                if self.debug_panel.view.is_stopped() {
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
                if self.debug_panel.view.is_stopped() {
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
        let mut frames = self.debug_panel.frames.clone();
        frames.layout(&self.layout_context(), geometry.frames);
        let mut variables = self.debug_panel.variables.clone();
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
        let geometry = debug_panel_geometry(
            self.debug_panel_rect(size),
            self.debug_panel.view.frames.len(),
        );
        for (rect, (_, request)) in geometry.buttons.iter().zip(DEBUG_BUTTONS) {
            if rect.contains(point) {
                self.commands.push(ApplicationCommand::Debug(request));
                return;
            }
        }
        // A lista resolve qual linha foi clicada; a IDE só reage à escolha.
        // Clicar fora das linhas é ignorado por ela, e é isso que distingue uma
        // escolha de um clique no vazio do painel.
        self.debug_panel
            .frames
            .layout(&self.layout_context(), geometry.frames);
        let result = self.debug_panel.frames.event(
            &mut EventContext::default(),
            &UiEvent::PointerDown(primary_pointer(point)),
        );
        if matches!(result, EventResult::Ignored) {
            return;
        }
        let Some(row) = self.debug_panel.frames.selected() else {
            return;
        };
        self.debug_panel.view.selected_frame = row;
        self.commands
            .push(ApplicationCommand::Debug(DebugRequest::SelectFrame(row)));
        if let Some((path, line)) = self
            .debug_panel
            .view
            .frames
            .get(row)
            .and_then(|frame| frame.location.clone())
        {
            self.commands.push(ApplicationCommand::OpenDocument(
                OpenDocumentRequest::new(path).at(line as usize, 0),
            ));
        }
    }

    fn sidebar_width(&self, size: Size) -> f32 {
        self.explorer.sidebar_width.clamp(
            SIDEBAR_MIN_WIDTH,
            (size.width - 320.0).max(SIDEBAR_MIN_WIDTH),
        )
    }

    pub fn toggle_search(&mut self) {
        if self.context.focus == ShellFocus::Search {
            self.editor_area.search_query.clear();
            self.context.focus = ShellFocus::Editor;
        } else {
            self.context.focus = ShellFocus::Search;
        }
    }

    pub fn escape(&mut self) {
        // O menu de contexto é o que está por cima de tudo: é ele que Esc
        // dispensa primeiro.
        if self.context_menu_key("Escape", Modifiers::default()) {
            return;
        }
        // A janela de renomear vem antes das outras porque é a que está por
        // cima quando está aberta.
        if self.editor_area.rename_modal.is_open() {
            self.cancel_rename();
            return;
        }
        if self.editor_area.generate_modal.is_open() {
            self.close_generate();
            return;
        }
        if self.search.modal.is_open() {
            self.close_type_search();
            return;
        }
        if self.debug_panel.inspection.modal.is_open() {
            self.close_inspection();
            return;
        }
        if self.search.new_item_modal.is_open() {
            self.close_new_item_dialog();
            return;
        }
        // Esc na janela de configurações é cancelar: fechar sem descartar o que
        // foi mexido salvaria pela porta dos fundos.
        if self.settings.modal.is_open() {
            self.cancel_settings();
            return;
        }
        if !self.editor_area.completion_items.is_empty() {
            self.editor_area.completion_items.clear();
            return;
        }
        // Desistir da edição múltipla vem antes de largar a busca: são as marcas
        // que estão na frente do usuário, e o texto volta ao que era.
        if let Some(document) = self.editor_area.session.active_mut()
            && self
                .editor_area
                .pane
                .cancel_occurrences(&mut document.buffer)
        {
            return;
        }
        if self.context.focus == ShellFocus::Search {
            self.editor_area.search_query.clear();
            self.context.focus = ShellFocus::Editor;
        }
    }

    pub fn pointer_down(&mut self, point: Point, size: Size) {
        self.pointer_down_with_modifiers(point, size, false, false);
    }

    /// Clique secundário: abre o menu de contexto sobre o item do Explorer.
    ///
    /// Fora do Explorer o clique só dispensa um menu aberto. Enquanto não
    /// houver menu para as outras áreas, abrir um vazio prometeria ações que
    /// não existem.
    pub fn secondary_pointer_down(&mut self, point: Point, size: Size) {
        self.explorer.context_menu.close();
        self.explorer.context_menu_target = None;
        // O clique secundário nunca escolhe da lista, então ele só a dispensa.
        self.clear_completions();
        if self.settings.modal.is_open() {
            return;
        }
        let geometry = self.geometry(size);
        let editor_x = ACTIVITY_WIDTH + self.sidebar_width(size);
        // No editor o menu fala do texto: copiar e colar.
        if point.x >= editor_x
            && point.x < editor_x + geometry.editor_width
            && point.y >= geometry.content_top
            && point.y < geometry.editor_bottom
        {
            self.context.focus = ShellFocus::Editor;
            let dentro_de_tipo = self.cursor_inside_type(point, size);
            self.explorer.context_menu.set_entries(editor_menu_entries(
                self.editor_area.pane.selection_range().is_some(),
                self.debug_panel.view.attached,
                dentro_de_tipo,
            ));
            self.explorer.context_menu.layout(
                &self.layout_context(),
                Rect::new(0.0, 0.0, size.width, size.height),
            );
            self.explorer.context_menu.open_at(point);
            return;
        }
        if point.x < ACTIVITY_WIDTH || point.x >= editor_x || point.y < EXPLORER_TOP {
            return;
        }
        // Qual nó está sob o ponteiro é a árvore quem sabe: recuo, deslocamento
        // horizontal e virtualização são dela.
        let mut tree = self.explorer_tree_for(size);
        tree.event(
            &mut EventContext::default(),
            &UiEvent::PointerDown(primary_pointer(point)),
        );
        let Some((path, is_directory)) = tree.selected().and_then(|id| self.explorer_path_for(id))
        else {
            return;
        };
        self.context.focus = ShellFocus::Explorer;
        self.explorer.tree.set_selected(Some(explorer_id(&path)));
        // O arquivo clicado, quando foi um: `target` abaixo é a pasta, porque é
        // nela que a criação acontece, mas renomear fala do arquivo.
        self.explorer.context_menu_file = (!is_directory).then(|| path.clone());
        // O alvo é o diretório: clicando em um arquivo, é na pasta dele que a
        // criação acontece.
        let target = if is_directory {
            path
        } else {
            path.parent().map(Path::to_path_buf).unwrap_or(path)
        };
        self.explorer
            .context_menu
            .set_entries(explorer_menu_entries(
                &target,
                &self.catalog.source_root_names,
                &self.catalog.new_item_templates,
                !is_directory,
            ));
        self.explorer.context_menu.layout(
            &self.layout_context(),
            Rect::new(0.0, 0.0, size.width, size.height),
        );
        self.explorer.context_menu.open_at(point);
        self.explorer.context_menu_target = Some(target);
    }

    pub fn context_menu_open(&self) -> bool {
        self.explorer.context_menu.is_open()
    }

    /// Entrega o evento ao menu aberto e trata o comando escolhido.
    ///
    /// Devolve `true` quando o menu consumiu o evento — é o sinal de que o
    /// clique ou a tecla não devem seguir para o que está embaixo dele.
    fn context_menu_event(&mut self, event: &UiEvent, size: Size) -> bool {
        if !self.explorer.context_menu.is_open() {
            return false;
        }
        self.explorer.context_menu.layout(
            &self.layout_context(),
            Rect::new(0.0, 0.0, size.width, size.height),
        );
        let mut context = EventContext::default();
        let result = self.explorer.context_menu.event(&mut context, event);
        if let EventResult::Action(WidgetAction::Command(command)) = &result {
            self.run_explorer_command(&command.0);
        }
        if !self.explorer.context_menu.is_open() {
            self.explorer.context_menu_target = None;
        }
        result != EventResult::Ignored
    }

    /// Entrega a tecla ao menu aberto.
    ///
    /// Separado do caminho do ponteiro porque navegar por teclado não depende
    /// de onde o menu foi desenhado, e assim não precisa do tamanho da janela.
    fn context_menu_key(&mut self, key: &str, modifiers: Modifiers) -> bool {
        if !self.explorer.context_menu.is_open() {
            return false;
        }
        let mut context = EventContext::default();
        let result = self.explorer.context_menu.event(
            &mut context,
            &UiEvent::KeyDown(KeyEvent {
                logical_key: key.to_owned(),
                repeat: false,
                modifiers,
            }),
        );
        if let EventResult::Action(WidgetAction::Command(command)) = &result {
            self.run_explorer_command(&command.0);
        }
        if !self.explorer.context_menu.is_open() {
            self.explorer.context_menu_target = None;
        }
        result != EventResult::Ignored
    }

    fn run_explorer_command(&mut self, command: &str) {
        match command {
            // A geração em si ainda não existe: o menu já escolhe o que gerar, e
            // dizer isso é melhor do que um clique que não faz nada.
            "editor.generate.getter" => {
                self.request_accessors(AccessorKind::Getter);
                return;
            }
            "editor.generate.setter" => {
                self.request_accessors(AccessorKind::Setter);
                return;
            }
            "editor.generate.accessors" => {
                self.request_accessors(AccessorKind::Both);
                return;
            }
            "editor.generate.constructor" => {
                self.request_accessors(AccessorKind::Constructor);
                return;
            }
            "editor.copy" => {
                self.copy_selection();
                return;
            }
            "editor.paste" => {
                self.paste_clipboard();
                return;
            }
            "debug.inspect" => {
                self.inspect_selection();
                return;
            }
            _ => {}
        }
        let Some(target) = self.explorer.context_menu_target.clone() else {
            return;
        };
        if command == "explorer.rename" {
            if let Some(arquivo) = self.explorer.context_menu_file.clone() {
                self.request_rename(arquivo);
            }
            return;
        }
        if command == "explorer.new.folder" {
            self.context.status_message = format!("Nova pasta em {}", target.display());
            return;
        }
        let Some(template_id) = command.strip_prefix("explorer.new.") else {
            return;
        };
        let Some(template) = self
            .catalog
            .new_item_templates
            .iter()
            .find(|template| template.id.as_str() == template_id)
            .cloned()
        else {
            return;
        };
        self.open_new_item_dialog(template, &target);
    }

    /// Abre a janela de criação com o pacote do alvo já preenchido.
    ///
    /// O pacote vem do caminho clicado, em notação de ponto: é o que o usuário vê
    /// no Explorer e o que ele vai editar para criar um pacote abaixo. Sem raiz de
    /// fontes não há pacote, e a janela não abre — o menu que oferece essas ações
    /// só aparece dentro dela.
    fn open_new_item_dialog(&mut self, template: NewItemTemplate, target: &Path) {
        let Some(source_root) = target
            .ancestors()
            .find(|ancestor| is_source_root(ancestor, &self.catalog.source_root_names))
            .map(Path::to_path_buf)
        else {
            self.context.status_message = "Fora de uma raiz de fontes registrada".to_owned();
            return;
        };
        let package = target
            .strip_prefix(&source_root)
            .map(|relative| {
                relative
                    .components()
                    .filter_map(|component| component.as_os_str().to_str())
                    .collect::<Vec<_>>()
                    .join(".")
            })
            .unwrap_or_default();
        self.search.new_item_package.set_value(package);
        self.search.new_item_name.set_value(String::new());
        self.search.new_item_modal.set_title(template.title.clone());
        self.search.new_item_modal.open();
        self.search.new_item_dialog = Some(NewItemDialog {
            template: template.clone(),
            source_root,
            message: None,
            naming: false,
        });
        // O pacote já vem preenchido, então o que falta digitar é o nome —
        // exceto ao criar pacote, em que o nome é justamente o que se edita.
        self.focus_new_item_field(!template.allows_empty_name);
    }

    pub const fn new_item_dialog_open(&self) -> bool {
        self.search.new_item_modal.is_open()
    }

    /// Relata o que impediu a criação, mantendo a janela aberta.
    pub fn set_new_item_message(&mut self, message: impl Into<String>) {
        if let Some(dialog) = self.search.new_item_dialog.as_mut() {
            dialog.message = Some(message.into());
        }
    }

    pub fn close_new_item_dialog(&mut self) {
        self.search.new_item_modal.close();
        self.search.new_item_dialog = None;
    }

    /// Monta o pedido a partir do que está nos campos.
    ///
    /// O pacote é obrigatório: sem ele não há onde criar. O nome é obrigatório
    /// para classe e interface, e opcional para pacote — é o que permite criar o
    /// pacote e a primeira classe dele num gesto só.
    fn submit_new_item(&mut self) {
        let Some(dialog) = self.search.new_item_dialog.as_ref() else {
            return;
        };
        let template_id = dialog.template.id.clone();
        let source_root = dialog.source_root.clone();
        let package = self.search.new_item_package.value().trim().to_owned();
        let name = self.search.new_item_name.value().trim().to_owned();
        if package.is_empty() {
            self.set_new_item_message("Informe o pacote.");
            return;
        }
        if name.is_empty() && !dialog.template.allows_empty_name {
            self.set_new_item_message("Informe o nome.");
            return;
        }
        self.commands
            .push(ApplicationCommand::CreateItem(NewItemRequest {
                template_id,
                package,
                name,
                source_root,
            }));
    }

    pub fn pointer_down_with_modifiers(
        &mut self,
        point: Point,
        size: Size,
        control: bool,
        shift: bool,
    ) {
        // O menu aberto tem a primeira palavra: escolher uma ação ou dispensá-lo
        // é o que este clique significa, e não o que está embaixo dele.
        if self.context_menu_event(&UiEvent::PointerDown(primary_pointer(point)), size) {
            return;
        }
        if self.editor_area.rename_modal.is_open() {
            self.rename_pointer_down(point, size);
            return;
        }
        if self.editor_area.generate_modal.is_open() {
            self.generate_pointer_down(point, size);
            return;
        }
        if self.search.modal.is_open() {
            self.type_search_pointer_down(point, size);
            return;
        }
        // A lista de completação vem antes do resto: ela está por cima, e clicar
        // em outro lugar significa desistir dela.
        if self.completion_pointer_down(point, size) {
            return;
        }
        if self.debug_panel.inspection.modal.is_open() {
            self.inspection_pointer_down(point, size);
            return;
        }
        if self.search.new_item_modal.is_open() {
            self.new_item_pointer_down(point, size);
            return;
        }
        if self.settings.modal.is_open() {
            self.settings_dialog_pointer_down(point, size);
            return;
        }
        if point.y < TITLE_HEIGHT && self.action_buttons_pointer_down(point, size) {
            return;
        }
        self.menu.bar.layout(
            &self.layout_context(),
            Rect::new(82.0, 0.0, (size.width - 82.0).max(0.0), TITLE_HEIGHT),
        );
        let mut menu_context = EventContext::default();
        let menu_result = self.menu.bar.event(
            &mut menu_context,
            &UiEvent::PointerDown(primary_pointer(point)),
        );
        match menu_result {
            EventResult::Action(WidgetAction::Command(command)) if command.0 == "file.project" => {
                self.commands.push(ApplicationCommand::OpenProject);
                self.context.status_message = "Select a project folder".to_owned();
                return;
            }
            EventResult::Action(WidgetAction::Command(command)) if command.0 == "file.save" => {
                self.request_save_active_document();
                return;
            }
            EventResult::Action(WidgetAction::Command(command)) if command.0 == "settings.open" => {
                self.commands.push(ApplicationCommand::OpenSettings);
                return;
            }
            EventResult::Action(WidgetAction::Command(command)) if command.0 == "project.build" => {
                self.commands.push(ApplicationCommand::BuildProject);
                return;
            }
            EventResult::Action(WidgetAction::Command(command))
                if command.0 == "project.reimport" =>
            {
                self.commands.push(ApplicationCommand::ReimportProject);
                return;
            }
            EventResult::Action(WidgetAction::Command(command)) if command.0 == "project.run" => {
                self.commands.push(ApplicationCommand::RunProject);
                self.context.status_message = "Executando a aplicação".to_owned();
                return;
            }
            EventResult::Action(WidgetAction::Command(command)) if command.0 == "project.stop" => {
                self.commands.push(ApplicationCommand::StopProject);
                return;
            }
            EventResult::Action(WidgetAction::Command(command))
                if command.0.starts_with("task.execute.") =>
            {
                if let Some(id) = command.0.strip_prefix("task.execute.") {
                    self.commands
                        .push(ApplicationCommand::ExecuteTask(TaskId(id.to_owned())));
                }
                return;
            }
            EventResult::Action(WidgetAction::Command(command)) if command.0 == "debug.connect" => {
                self.settings.page = SettingsPage::Debug;
                self.commands.push(ApplicationCommand::OpenSettings);
                return;
            }
            EventResult::Action(WidgetAction::Command(command))
                if command.0.starts_with("debug.") =>
            {
                if let Some(request) = debug_request_for(&command.0) {
                    self.commands.push(ApplicationCommand::Debug(request));
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
            if self.terminal.minimized {
                self.terminal.minimized = false;
                self.terminal.height = self.terminal.last_height;
            } else {
                self.terminal.last_height = self.terminal.height;
                self.terminal.minimized = true;
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
            self.context.pointer = point;
            let mut tabs = self.editor_tabs();
            tabs.layout(&self.layout_context(), self.editor_tabs_rect(size));
            match tab_command(&mut tabs, point) {
                Some(TabCommand::Select(id)) => {
                    let _ = self.editor_area.session.activate(DocumentId(id));
                    self.editor_area.pane.set_cursor(0);
                    self.context.focus = ShellFocus::Editor;
                    self.sync_explorer_to_active();
                }
                Some(TabCommand::Close(id)) => {
                    let id = DocumentId(id);
                    if self.editor_area.session.close(id).is_ok() {
                        self.editor_area.syntax_snapshots.remove(&id);
                        self.editor_area.syntax_spans.remove(&id);
                        self.editor_area
                            .pane
                            .set_cursor(self.active_text().map_or(0, str::len));
                        self.context.status_message = "Tab closed".to_owned();
                        self.sync_explorer_to_active();
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
                self.context.focus = ShellFocus::Explorer;
                self.explorer.tree.set_selected(Some(explorer_id(&path)));
                if is_directory {
                    if !self.explorer.expanded.remove(&path) {
                        self.explorer.expanded.insert(path);
                    }
                    self.sync_explorer_tree();
                } else {
                    self.commands
                        .push(ApplicationCommand::OpenDocument(OpenDocumentRequest::new(
                            path,
                        )));
                }
            }
            return;
        }
        if self.debug_panel.view.attached
            && point.x >= editor_x + geometry.editor_width
            && point.y >= geometry.content_top
            && point.y < geometry.editor_bottom
        {
            self.debug_panel_pointer_down(point, size);
            return;
        }
        if point.x >= editor_x
            && point.x < editor_x + geometry.editor_width
            && point.y >= geometry.content_top
            && point.y < geometry.editor_bottom
        {
            self.context.focus = ShellFocus::Editor;
            // O painel cuida de cursor, âncora e calha; o shell só reage ao que
            // ele pede.
            let bounds = self.editor_view_rect(size);
            self.editor_area.pane.set_bounds(bounds);
            let Some(document) = self.editor_area.session.active() else {
                return;
            };
            let action = self
                .editor_area
                .pane
                .pointer_down(&document.buffer, point, control, shift);
            self.handle_editor_action(action);
        } else if point.x >= editor_x && point.y >= geometry.editor_bottom {
            self.context.focus = ShellFocus::Terminal;
            if point.y < geometry.editor_bottom + TERMINAL_TAB_HEIGHT {
                let mut tabs = self.terminal_tabs();
                tabs.layout(&self.layout_context(), self.terminal_tabs_rect(size));
                if let Some(TabCommand::Select(index)) = tab_command(&mut tabs, point) {
                    self.terminal.active = index as usize;
                    self.context.status_message = format!(
                        "Terminal: {}",
                        self.active_terminal().selected_profile().kind.label()
                    );
                }
            } else if point.y >= geometry.editor_bottom + 60.0 {
                let position = self.terminal_position_at(point, size);
                self.terminal.selection = Some(TerminalSelection {
                    anchor: position,
                    focus: position,
                });
                self.terminal.selecting = true;
            }
        }
    }

    /// Executa o que o painel de edição pediu.
    ///
    /// O painel edita texto; navegar até uma definição, marcar breakpoint,
    /// gravar e abrir menu são coisas que só o shell tem como fazer.
    fn handle_editor_action(&mut self, action: EditorAction) {
        match action {
            EditorAction::Navigate(offset) => {
                if let (Some(document_id), Some(token)) = (
                    self.editor_area.session.active_id(),
                    self.active_text().and_then(|text| token_at(text, offset)),
                ) {
                    self.context.status_message = format!("Go to definition: {token}");
                    self.commands
                        .push(ApplicationCommand::Navigate(NavigationRequest {
                            document_id,
                            byte_offset: offset,
                            token,
                        }));
                }
            }
            EditorAction::ToggleBreakpoint(line) => {
                if let Some(path) = self
                    .editor_area
                    .session
                    .active()
                    .map(|document| document.path.clone())
                {
                    self.toggle_breakpoint(&path, line as u32);
                }
            }
            EditorAction::Save => {
                self.request_save_active_document();
            }
            EditorAction::ContextMenu(_) | EditorAction::None => {}
        }
    }

    pub fn show_location(
        &mut self,
        path: &Path,
        text: impl Into<String>,
        line: usize,
        column: usize,
    ) -> DocumentId {
        let id = self.show_document(path, text);
        let text = self.active_text().unwrap_or_default();
        self.editor_area
            .pane
            .set_cursor(offset_for_line_column(text, line, column));
        // Sem revelar a linha, a navegação move o cursor para fora da área
        // visível e parece que nada aconteceu.
        self.editor_area.pane.reveal_line(line);
        self.editor_area.navigated = Some((line, self.editor_area.pane.cursor()));
        self.context.focus = ShellFocus::Editor;
        self.context.status_message =
            format!("Definition: {}:{}:{}", path.display(), line + 1, column + 1);
        id
    }

    pub fn pointer_move(&mut self, point: Point, size: Size) -> bool {
        self.context.pointer = point;
        // Com o menu aberto, o destaque acompanha o ponteiro dentro dele.
        if self.explorer.context_menu.is_open() {
            return self.context_menu_event(&UiEvent::PointerMove(primary_pointer(point)), size);
        }
        // Arrastar a barra da janela de renomear precisa do movimento: só o
        // clique chegando à lista, o indicador é pego e nunca anda.
        if self.editor_area.rename_modal.is_open() {
            self.rename_list_event(&UiEvent::PointerMove(primary_pointer(point)), size);
            return true;
        }
        if self.settings.modal.is_open() {
            return false;
        }
        // Com a inspeção aberta, o gesto é dela: o resto da janela está atrás do
        // painel, e arrastar sobre o que não se vê seria o gesto indo parar no
        // lugar errado.
        let inspecting = self.debug_panel.inspection.modal.is_open();
        if !inspecting && let Some(target) = self.context.scrollbar_drag {
            self.sync_scrollbar(target, size);
            self.scrollbar_mut(target).event(
                &mut EventContext::default(),
                &UiEvent::PointerMove(primary_pointer(point)),
            );
            self.apply_scrollbar(target);
            return true;
        }
        // O arraste no editor é do painel, que sabe se um gesto começou nele.
        self.place_focused_editor(size);
        if let Some((pane, buffer)) = self.focused_editor()
            && pane.pointer_move(buffer, point)
        {
            return true;
        }
        if inspecting {
            return false;
        }
        if self.terminal.selecting {
            let position = self.terminal_position_at(point, size);
            if let Some(selection) = self.terminal.selection.as_mut() {
                selection.focus = position;
            }
            return true;
        }
        // O movimento vai sempre aos divisores: mesmo parados, eles precisam
        // saber que o ponteiro passou por cima para se destacar.
        let dragging = self.sidebar_resizing() || self.terminal_resizing();
        self.sync_splitters(size);
        let event = UiEvent::PointerMove(primary_pointer(point));
        self.explorer
            .splitter
            .event(&mut EventContext::default(), &event);
        self.terminal
            .splitter
            .event(&mut EventContext::default(), &event);
        if dragging {
            self.apply_splitters(size);
            return true;
        }
        // Parado, o retorno diz se o ponteiro está sobre o divisor do terminal,
        // para a janela trocar o cursor.
        !self.terminal.minimized && self.terminal.splitter.hit_area().contains(point)
    }

    /// Continua a rolagem de um arrasto que saiu da área visível do editor.
    ///
    /// A janela chama isto a cada tique do relógio, porque um arrasto parado
    /// fora da borda não gera evento nenhum — e ainda assim deve seguir
    /// rolando e marcando. Devolve se algo mudou, para a janela redesenhar
    /// só quando há o que mostrar.
    pub fn drag_autoscroll(&mut self, size: Size) -> bool {
        self.place_focused_editor(size);
        self.focused_editor()
            .is_some_and(|(pane, buffer)| pane.drag_autoscroll(buffer))
    }

    pub fn pointer_up(&mut self) {
        // A soltura encerra o arrasto da barra: sem ela a lista continuaria
        // achando que o gesto está em curso e seguiria o ponteiro.
        if self.editor_area.rename_modal.is_open() {
            let size = self.context.last_size;
            self.rename_list_event(&UiEvent::PointerUp(primary_pointer(Point::ZERO)), size);
        }
        // Encerrar o gesto é do painel, que sabe se ele virou seleção.
        self.editor_area.pane.pointer_up();
        self.debug_panel.inspection.editor.pointer_up();
        let event = UiEvent::PointerUp(primary_pointer(Point::ZERO));
        self.explorer
            .splitter
            .event(&mut EventContext::default(), &event);
        self.terminal
            .splitter
            .event(&mut EventContext::default(), &event);
        // A barra também precisa saber que o gesto acabou: ela é quem guarda o
        // ponto da pegada.
        if let Some(target) = self.context.scrollbar_drag.take() {
            self.scrollbar_mut(target).event(
                &mut EventContext::default(),
                &UiEvent::PointerUp(primary_pointer(Point::ZERO)),
            );
        }
        self.terminal.selecting = false;
    }

    pub fn scroll(&mut self, point: Point, delta_lines: f32, size: Size) {
        // A janela de renomear cobre tudo: a roda ali é da lista dela, e nunca
        // do editor atrás — rolar o que está coberto é mexer no que não se vê.
        if self.editor_area.rename_modal.is_open() {
            self.editor_area.rename_modal.layout(
                &self.layout_context(),
                Rect::new(0.0, 0.0, size.width, size.height),
            );
            let geometry = rename_geometry(self.editor_area.rename_modal.panel_bounds());
            let contexto = self.layout_context();
            if let Some(state) = self.editor_area.rename.as_mut() {
                state.list.layout(&contexto, geometry.list);
                state.list.event(
                    &mut EventContext::default(),
                    &UiEvent::Scroll(ui_core::ScrollEvent {
                        position: point,
                        delta_x: 0.0,
                        delta_y: delta_lines * GENERATE_ROW_HEIGHT,
                    }),
                );
            }
            return;
        }
        // A janela de geração cobre tudo: a roda ali é dela.
        if self.editor_area.generate_modal.is_open() {
            let (lista, ..) = self.generate_geometry(size);
            let contexto = self.layout_context();
            if let Some(state) = self.editor_area.generate.as_mut() {
                state.list.layout(&contexto, lista);
                state.list.event(
                    &mut EventContext::default(),
                    &UiEvent::Scroll(ui_core::ScrollEvent {
                        position: point,
                        delta_x: 0.0,
                        delta_y: delta_lines * GENERATE_ROW_HEIGHT,
                    }),
                );
            }
            return;
        }
        if self.search.modal.is_open() {
            self.type_search_scroll(point, delta_lines, size);
            return;
        }
        if self.settings.modal.is_open() {
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
            // Explorer e terminal são listas de linhas inteiras: aqui a
            // fração não tem onde aparecer, e o passo volta a ser em linhas.
            self.explorer.scroll_line = self
                .explorer
                .scroll_line
                .saturating_add_signed(delta_lines.round() as isize)
                .min(max);
        } else if point.y >= geo.content_top && point.y < geo.editor_bottom {
            // Em pixels, e por uma fração de linha a cada passo: rolar de linha
            // inteira faz o texto saltar, que é o que se sente como travado.
            let total = self.active_text().map_or(0, |text| text.lines().count());
            let visible = (geo.editor_height / EDITOR_LINE_HEIGHT).floor().max(1.0) as usize;
            let maximo = total.saturating_sub(visible) as f32 * EDITOR_LINE_HEIGHT;
            let passo = delta_lines * EDITOR_LINE_HEIGHT;
            let destino =
                (self.editor_area.pane.scroll_offset() + passo).clamp(0.0, maximo.max(0.0));
            self.editor_area.pane.set_scroll_offset(destino);
        } else if point.y >= geo.editor_bottom && point.y < geo.content_bottom {
            let visible = ((geo.terminal_height - 62.0) / EDITOR_LINE_HEIGHT)
                .floor()
                .max(1.0) as usize;
            let active = self.terminal.active;
            let max = self.terminal.tabs[active]
                .session
                .line_count()
                .saturating_sub(visible);
            self.terminal.tabs[active].scroll_line = self.terminal.tabs[active]
                .scroll_line
                .saturating_add_signed(delta_lines.round() as isize)
                .min(max);
            self.terminal.tabs[active].follow_output =
                self.terminal.tabs[active].scroll_line >= max;
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
        let active = &self.terminal.tabs[self.terminal.active];
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
        let Some(selection) = self.terminal.selection else {
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
        if self.type_search_text_input(text) {
            return;
        }
        if self.inspection_text_input(text) {
            return;
        }
        if self.rename_text_input(text) {
            return;
        }
        if self.new_item_text_input(text) {
            return;
        }
        if self.settings.modal.is_open() {
            let _ = self.settings_text_input(text);
            return;
        }
        match self.context.focus {
            ShellFocus::Editor => self.edit_active(text),
            ShellFocus::Search => self.editor_area.search_query.push_str(text),
            ShellFocus::Terminal => self.active_terminal_mut().input_mut().push_str(text),
            _ => {}
        }
    }

    pub fn key_down(&mut self, key: &str) {
        self.key_down_with_modifiers(key, Modifiers::default());
    }

    /// Tecla com os modificadores que o sistema entregou.
    ///
    /// `Tab` e `Shift+Tab` são a mesma tecla lógica com sentidos opostos, então
    /// o estado das teclas modificadoras precisa chegar aqui — o nome da tecla
    /// sozinho não diz se é para indentar ou recolher.
    pub fn key_down_with_modifiers(&mut self, key: &str, modifiers: Modifiers) {
        if self.context_menu_key(key, modifiers) {
            return;
        }
        // A busca de tipo cobre a janela: enquanto ela está aberta, as teclas são
        // dela.
        if self.type_search_key(key) {
            return;
        }
        if self.inspection_key(key, modifiers) {
            return;
        }
        if self.new_item_key(key) {
            return;
        }
        if self.settings.modal.is_open() {
            if self.settings_key_down(key) {
                return;
            }
            let event = UiEvent::KeyDown(KeyEvent {
                logical_key: key.to_owned(),
                repeat: false,
                modifiers,
            });
            let mut context = EventContext::default();
            let result = self.settings.toolchain_combo.event(&mut context, &event);
            let atendida = self.handle_settings_action(result);
            let result = if atendida {
                EventResult::Handled
            } else {
                // Com a segunda lista aberta, as setas e o Enter são dela.
                self.settings.secondary_combo.event(&mut context, &event)
            };
            if !self.handle_settings_action(result) {
                let _ = self.settings.modal.event(&mut context, &event);
                // A janela fechada por dentro do componente — Esc no
                // `ModalHost` — também é cancelamento.
                if !self.settings.modal.is_open() {
                    self.cancel_settings();
                }
            }
            return;
        }
        if self.rename_key(key, modifiers) {
            return;
        }
        if self.completion_key(key) {
            return;
        }
        if key.eq_ignore_ascii_case("backspace") {
            match self.context.focus {
                ShellFocus::Editor => self.backspace(),
                ShellFocus::Search => {
                    self.editor_area.search_query.pop();
                }
                ShellFocus::Terminal => {
                    self.active_terminal_mut().input_mut().pop();
                }
                _ => {}
            }
        } else if self.context.focus == ShellFocus::Terminal && key.eq_ignore_ascii_case("enter") {
            match self.active_terminal_mut().submit() {
                Ok(()) => self.context.status_message = "Command sent to terminal".to_owned(),
                Err(error) => self.context.status_message = error.to_string(),
            }
            let active = self.terminal.active;
            self.terminal.tabs[active].scroll_line = self.terminal.tabs[active]
                .session
                .line_count()
                .saturating_sub(1);
        } else if self.context.focus == ShellFocus::Editor {
            // Edição, seleção e movimento são do painel. O shell cuida do que
            // sobra: a lista de completação, a marca de modificado e as ações
            // que o painel não tem como executar.
            self.editor_area.completion_items.clear();
            let Some(document) = self.editor_area.session.active_mut() else {
                return;
            };
            let before = document.buffer.revision();
            let action = self.editor_area.pane.key(
                &mut document.buffer,
                key,
                modifiers.shift,
                modifiers.control,
            );
            if document.buffer.revision() != before {
                self.context.status_message = "Modified".to_owned();
            }
            self.handle_editor_action(action);
        }
    }

    /// Seleciona a palavra sob o ponteiro. É o que o duplo clique pede.
    pub fn select_word_at_point(&mut self, point: Point, size: Size) {
        self.place_focused_editor(size);
        let inside = self
            .focused_editor_ref()
            .is_some_and(|(pane, _)| pane.bounds().contains(point));
        if !inside {
            return;
        }
        // O gesto também leva o foco, como no clique simples.
        if self.debug_panel.inspection.modal.is_open() {
            self.debug_panel.inspection.focus = InspectionFocus::Source;
        } else {
            self.context.focus = ShellFocus::Editor;
        }
        if let Some((pane, buffer)) = self.focused_editor() {
            pane.select_word_at(buffer, point);
        }
    }

    /// Área do campo e da lista dentro do painel da busca.
    ///
    /// Um lugar só, para o clique acertar o que foi desenhado.
    fn type_search_geometry(&mut self, size: Size) -> (Rect, Rect) {
        self.search.modal.layout(
            &self.layout_context(),
            Rect::new(0.0, 0.0, size.width, size.height),
        );
        let panel = self.search.modal.panel_bounds();
        let input = Rect::new(
            panel.origin.x + 16.0,
            panel.origin.y + 56.0,
            panel.size.width - 32.0,
            34.0,
        );
        let list = Rect::new(
            panel.origin.x + 16.0,
            input.origin.y + input.size.height + 12.0,
            panel.size.width - 32.0,
            (panel.origin.y + panel.size.height - 16.0)
                - (input.origin.y + input.size.height + 12.0),
        );
        (input, list)
    }

    fn type_search_pointer_down(&mut self, point: Point, size: Size) {
        let (_, list) = self.type_search_geometry(size);
        if !list.contains(point) {
            // Clicar fora da lista não escolhe nada, e clicar fora do painel
            // dispensa a janela.
            if !self.search.modal.panel_bounds().contains(point) {
                self.close_type_search();
            }
            return;
        }
        let row = self.search.first_visible
            + ((point.y - list.origin.y) / TYPE_SEARCH_ROW_HEIGHT)
                .floor()
                .max(0.0) as usize;
        if row < self.workspace_search_result_len() {
            self.search.selected = row;
            self.open_selected_type();
        }
    }

    fn type_search_scroll(&mut self, point: Point, delta_lines: f32, size: Size) {
        let (_, list) = self.type_search_geometry(size);
        if !list.contains(point) {
            return;
        }
        self.search.first_visible = self
            .search
            .first_visible
            .saturating_add_signed(delta_lines.round() as isize)
            .min(self.type_search_max_first_visible());
    }

    fn type_search_max_first_visible(&self) -> usize {
        self.workspace_search_result_len()
            .saturating_sub(TYPE_SEARCH_VISIBLE_ROWS)
    }

    fn workspace_search_result_len(&self) -> usize {
        self.search.result_len()
    }

    fn reveal_type_search_selection(&mut self) {
        if self.search.selected < self.search.first_visible {
            self.search.first_visible = self.search.selected;
        } else if self.search.selected >= self.search.first_visible + TYPE_SEARCH_VISIBLE_ROWS {
            self.search.first_visible = self.search.selected + 1 - TYPE_SEARCH_VISIBLE_ROWS;
        }
        self.search.first_visible = self
            .search
            .first_visible
            .min(self.type_search_max_first_visible());
    }

    /// Abre a busca de tipo por nome. É o que `Ctrl+L` pede.
    pub fn open_type_search(&mut self) {
        self.search.reset(WorkspaceSearchMode::Types);
        self.search.modal.set_title("Ir para o tipo");
        // A consulta vazia nasce mostrando os tipos existentes.
        self.commands
            .push(ApplicationCommand::SearchTypes(String::new()));
        self.search.modal.open();
    }

    /// Abre a mesma janela da busca de tipos no modo de conteúdo.
    ///
    /// A consulta vazia não é enviada: ao contrário de uma lista de tipos, cada
    /// linha vazia de cada arquivo não é um resultado útil.
    pub fn open_content_search(&mut self) {
        self.search.reset(WorkspaceSearchMode::Content);
        let title = self.catalog.language_names.first().map_or_else(
            || "Buscar conteúdo".to_owned(),
            |language| format!("Buscar conteúdo em {language}"),
        );
        self.search.modal.set_title(title);
        self.search.modal.open();
    }

    #[must_use]
    pub fn type_search_open(&self) -> bool {
        self.search.modal.is_open()
    }

    pub fn close_type_search(&mut self) {
        self.search.modal.close();
    }

    /// Entrega o que a linguagem encontrou.
    pub fn set_type_search_results(&mut self, results: Vec<TypeSearchHit>) {
        self.search.type_results = results;
        self.search.content_results.clear();
        self.search.selected = 0;
        self.search.first_visible = 0;
    }

    /// Entrega as ocorrências encontradas dentro do escopo fornecido pela aplicação.
    pub fn set_content_search_results(&mut self, results: Vec<ContentSearchHit>) {
        self.search.content_results = results;
        self.search.type_results.clear();
        self.search.selected = 0;
        self.search.first_visible = 0;
    }

    #[must_use]
    pub fn type_search_results(&self) -> &[TypeSearchHit] {
        &self.search.type_results
    }

    /// Digitação na busca de tipo. Devolve `true` quando consumiu.
    fn type_search_text_input(&mut self, text: &str) -> bool {
        if !self.search.modal.is_open() {
            return false;
        }
        self.search.query.push_str(text);
        let command = match self.search.mode {
            WorkspaceSearchMode::Types => {
                ApplicationCommand::SearchTypes(self.search.query.clone())
            }
            WorkspaceSearchMode::Content => {
                ApplicationCommand::SearchContent(self.search.query.clone())
            }
        };
        self.commands.push(command);
        true
    }

    /// Tecla na busca de tipo. Devolve `true` quando consumiu.
    fn type_search_key(&mut self, key: &str) -> bool {
        if !self.search.modal.is_open() {
            return false;
        }
        match key.to_ascii_lowercase().as_str() {
            "backspace" => {
                self.search.query.pop();
                let command = match self.search.mode {
                    WorkspaceSearchMode::Types => {
                        ApplicationCommand::SearchTypes(self.search.query.clone())
                    }
                    WorkspaceSearchMode::Content => {
                        ApplicationCommand::SearchContent(self.search.query.clone())
                    }
                };
                self.commands.push(command);
            }
            "arrowdown" => {
                self.search.selected = (self.search.selected + 1)
                    .min(self.workspace_search_result_len().saturating_sub(1));
                self.reveal_type_search_selection();
            }
            "arrowup" => {
                self.search.selected = self.search.selected.saturating_sub(1);
                self.reveal_type_search_selection();
            }
            "enter" => self.open_selected_type(),
            "escape" => self.close_type_search(),
            _ => {}
        }
        true
    }

    /// Abre o item destacado no editor principal e fecha a janela.
    fn open_selected_type(&mut self) {
        let location = match self.search.mode {
            WorkspaceSearchMode::Types => self
                .search
                .type_results
                .get(self.search.selected)
                .map(|hit| hit.location.clone()),
            WorkspaceSearchMode::Content => self
                .search
                .content_results
                .get(self.search.selected)
                .map(|hit| hit.location.clone()),
        };
        let Some(location) = location else {
            return;
        };
        self.close_type_search();
        self.commands.push(ApplicationCommand::OpenDocument(
            OpenDocumentRequest::new(location.path).at(
                location.range.start.line as usize,
                location.range.start.column as usize,
            ),
        ));
    }

    /// Recebe uma avaliação vinda do depurador e decide o que ela significa.
    ///
    /// Abrir a inspeção e executar código dentro dela chegam pelo mesmo evento,
    /// mas pedem coisas opostas: a primeira monta a árvore, a segunda **não pode
    /// desmontá-la**. Quem executa `m.setId(4L)` quer continuar olhando `m`, e não
    /// trocar a árvore pelo `void` que o método devolveu.
    pub fn inspection_result(
        &mut self,
        expression: String,
        value: DebugVariableView,
        fields: Vec<DebugVariableView>,
    ) {
        if self
            .debug_panel
            .inspection
            .run
            .as_ref()
            .is_some_and(|run| run.current == expression)
        {
            self.advance_inspection_run(&expression, &value.value);
            return;
        }
        // A mesma raiz chegando de novo é a releitura pedida depois de uma
        // execução: os valores mudam, o que estava aberto continua aberto.
        let same_root = self
            .debug_panel
            .inspection
            .view
            .as_ref()
            .is_some_and(|inspection| inspection.expression == expression);
        if same_root && self.debug_panel.inspection.modal.is_open() {
            self.refresh_inspection(value, fields);
            return;
        }
        self.show_inspection(expression, value, fields);
    }

    /// Uma instrução terminou: manda a próxima, ou fecha a execução.
    fn advance_inspection_run(&mut self, expression: &str, value: &str) {
        let Some(run) = self.debug_panel.inspection.run.as_mut() else {
            return;
        };
        let total = run.total;
        if run.remaining.is_empty() {
            self.debug_panel.inspection.run = None;
            // Com uma instrução só, o relato é o retorno dela; com várias, o que
            // interessa é que todas passaram e o que a última respondeu.
            self.debug_panel.inspection.message = Some(if total > 1 {
                format!("{total} instruções executadas — {expression} → {value}")
            } else {
                format!("{expression} → {value}")
            });
            self.reload_inspection();
            return;
        }
        let next = run.remaining.remove(0);
        run.position += 1;
        run.current.clone_from(&next);
        let position = run.position;
        self.context.status_message = format!("Executando {next} ({position} de {total})");
        self.commands
            .push(ApplicationCommand::Debug(DebugRequest::Evaluate(next)));
    }

    /// Pede ao depurador o valor atual da raiz da árvore.
    fn reload_inspection(&mut self) {
        let Some(inspection) = self.debug_panel.inspection.view.as_ref() else {
            return;
        };
        self.commands
            .push(ApplicationCommand::Debug(DebugRequest::Evaluate(
                inspection.expression.clone(),
            )));
    }

    /// Troca os valores da árvore sem mexer no que está aberto nem no que está
    /// selecionado.
    fn refresh_inspection(&mut self, value: DebugVariableView, fields: Vec<DebugVariableView>) {
        let Some(inspection) = self.debug_panel.inspection.view.as_mut() else {
            return;
        };
        let expression = inspection.expression.clone();
        inspection.root.variable = value;
        inspection.root.loaded = true;
        inspection.root.children = fields
            .into_iter()
            .map(|field| InspectionNode::new(format!("{expression}.{}", field.name), field))
            .collect();
        // Só a raiz veio com campos; os níveis abertos abaixo dela precisam ser
        // relidos, ou mostrariam o valor de antes da execução.
        let deeper: Vec<String> = inspection
            .expanded
            .iter()
            .filter(|path| **path != expression)
            .cloned()
            .collect();
        self.sync_inspection_tree();
        for path in deeper {
            self.commands
                .push(ApplicationCommand::Debug(DebugRequest::ExpandInspection(
                    path,
                )));
        }
    }

    /// Abre a janela de inspeção com o valor avaliado e seus campos.
    pub fn show_inspection(
        &mut self,
        expression: impl Into<String>,
        value: DebugVariableView,
        fields: Vec<DebugVariableView>,
    ) {
        let expression = expression.into();
        let mut root = InspectionNode::new(expression.clone(), value);
        root.loaded = true;
        root.children = fields
            .into_iter()
            .map(|field| InspectionNode::new(format!("{expression}.{}", field.name), field))
            .collect();
        // A raiz nasce aberta quando tem campos: quem manda inspecionar um objeto
        // quer ver o que há dentro, não um triângulo para clicar.
        let mut expanded = HashSet::new();
        if !root.children.is_empty() {
            expanded.insert(expression.clone());
        }
        self.debug_panel
            .inspection
            .modal
            .set_title(format!("Inspecionar — {expression}"));
        self.debug_panel.inspection.modal.open();
        self.debug_panel.inspection.view = Some(InspectionView {
            selected: expression.clone(),
            expression,
            root,
            expanded,
        });
        self.sync_inspection_tree();
    }

    /// Acrescenta os campos que o alvo revelou para um caminho.
    pub fn add_inspection_fields(&mut self, path: &str, fields: Vec<DebugVariableView>) {
        let Some(inspection) = self.debug_panel.inspection.view.as_mut() else {
            return;
        };
        let Some(node) = inspection.root.find_mut(path) else {
            return;
        };
        node.loaded = true;
        node.children = fields
            .into_iter()
            .map(|field| InspectionNode::new(format!("{path}.{}", field.name), field))
            .collect();
        // Sem campos não há o que abrir; manter aberto deixaria um triângulo
        // apontando para nada.
        if node.children.is_empty() {
            inspection.expanded.remove(path);
        }
        self.sync_inspection_tree();
    }

    /// Reconstrói os itens da árvore a partir dos nós carregados.
    fn sync_inspection_tree(&mut self) {
        let Some(inspection) = self.debug_panel.inspection.view.as_ref() else {
            return;
        };
        let roots = vec![inspection_items(&inspection.root)];
        let expanded: Vec<u64> = inspection
            .expanded
            .iter()
            .map(|path| inspection_id(path))
            .collect();
        let selected = inspection_id(&inspection.selected);
        self.debug_panel.inspection.tree.set_roots(roots);
        self.debug_panel.inspection.tree.set_expanded(expanded);
        self.debug_panel
            .inspection
            .tree
            .set_selected(Some(selected));
    }

    /// Relata na janela o que a última execução respondeu.
    ///
    /// Enquanto a janela está aberta ela cobre a barra de estado, então é aqui
    /// que a resposta precisa aparecer.
    pub fn set_inspection_message(&mut self, message: impl Into<String>) {
        let message = message.into();
        // A avaliação responde com o valor ou com este relato, nunca com os dois:
        // chegar aqui encerra a execução que estava em curso.
        let interrupted = self.debug_panel.inspection.run.take();
        if !self.debug_panel.inspection.modal.is_open() {
            return;
        }
        let Some(run) = interrupted else {
            self.debug_panel.inspection.message = Some(message);
            return;
        };
        // As instruções seguintes não rodam: cada uma esperava o estado que a
        // anterior deixaria, e ele não existe.
        self.debug_panel.inspection.message = Some(if run.total > 1 {
            format!(
                "parou na instrução {} de {}: {message}",
                run.position, run.total
            )
        } else {
            message
        });
        // O que rodou antes da falha teve efeito, e a árvore precisa mostrá-lo.
        // Só aqui: relê a cada relato qualquer e uma releitura que falhasse
        // pediria outra, sem fim.
        if run.position > 1 {
            self.reload_inspection();
        }
    }

    pub const fn inspection_open(&self) -> bool {
        self.debug_panel.inspection.modal.is_open()
    }

    /// Texto do editor de expressões da inspeção.
    #[must_use]
    pub fn inspection_source(&self) -> &str {
        self.debug_panel.inspection.source.text()
    }

    /// Executa o que está escrito no editor, no quadro atual.
    pub fn run_inspection_source(&mut self) {
        if !self.debug_panel.view.attached {
            self.debug_panel.inspection.message =
                Some("A sessão de depuração terminou; reconecte para executar".to_owned());
            return;
        }
        let mut statements = inspection_statements(self.debug_panel.inspection.source.text());
        if statements.is_empty() {
            self.context.status_message = "Escreva a expressão a executar".to_owned();
            return;
        }
        let total = statements.len();
        let first = statements.remove(0);
        self.context.status_message = if total > 1 {
            format!("Executando {first} (1 de {total})")
        } else {
            format!("Executando {first}")
        };
        self.debug_panel.inspection.message = None;
        self.debug_panel.inspection.run = Some(InspectionRun {
            current: first.clone(),
            remaining: statements,
            position: 1,
            total,
        });
        self.commands
            .push(ApplicationCommand::Debug(DebugRequest::Evaluate(first)));
    }

    /// Painel de edição que está na frente, com o texto que ele edita.
    ///
    /// A janela de inspeção cobre o editor principal, então **qual é "o editor"
    /// depende do que está na frente**. Responder isso num lugar só é o que
    /// impede cada gesto — arraste, duplo clique, copiar, colar — de escolher por
    /// conta própria e um dia escolher diferente do vizinho.
    fn focused_editor(&mut self) -> Option<(&mut EditorPane, &mut TextBuffer)> {
        if self.debug_panel.inspection.modal.is_open() {
            return Some((
                &mut self.debug_panel.inspection.editor,
                &mut self.debug_panel.inspection.source,
            ));
        }
        let document = self.editor_area.session.active_mut()?;
        Some((&mut self.editor_area.pane, &mut document.buffer))
    }

    /// Tipo e filtro pedidos pelo ponto digitado no editor da inspeção.
    ///
    /// Ali não existe arquivo, então a completação comum — que descobre o tipo
    /// pela declaração — não tem de onde partir. O tipo do receptor vem de duas
    /// origens, nesta ordem: o que está parado no depurador, que é a única fonte
    /// para uma variável de quadro como `m`; e, se o nome não for uma delas, o
    /// próprio texto digitado, que passa a ser lido como nome de tipo. É o que
    /// faz uma classe alheia ao código depurado ser reconhecida como as outras —
    /// quem responde pelos membros é o índice do projeto, não o processo.
    #[must_use]
    pub fn inspection_member_context(&self) -> Option<(String, usize)> {
        if !self.debug_panel.inspection.modal.is_open() {
            return None;
        }
        Some((
            self.debug_panel.inspection.source.text().to_owned(),
            self.debug_panel.inspection.editor.cursor(),
        ))
    }

    #[must_use]
    pub fn inspection_member_target(&self, receiver: &str, prefix: String) -> (String, String) {
        let type_name = self
            .debug_type_of(receiver)
            .unwrap_or_else(|| receiver.to_owned());
        (type_name, prefix)
    }

    /// Tipo em execução de um nome visível no quadro parado.
    ///
    /// A árvore de inspeção vem primeiro porque é o objeto que o usuário mandou
    /// inspecionar; as variáveis do quadro cobrem o resto do que está no escopo.
    fn debug_type_of(&self, name: &str) -> Option<String> {
        if let Some(inspection) = self.debug_panel.inspection.view.as_ref() {
            if inspection.expression == name {
                return inspection.root.variable.type_name.clone();
            }
            if let Some(field) = inspection
                .root
                .children
                .iter()
                .find(|child| child.variable.name == name)
            {
                return field.variable.type_name.clone();
            }
        }
        self.debug_panel
            .view
            .variables
            .iter()
            .find(|variable| variable.name == name)
            .and_then(|variable| variable.type_name.clone())
    }

    /// Canto onde a lista de completação nasce, seja qual for o editor da frente.
    ///
    /// Um lugar só, porque a pintura e o clique precisam concordar: se o desenho
    /// e o teste de acerto calculassem a área cada um por si, clicar na borda da
    /// lista faria uma coisa e ver a lista mostraria outra.
    fn completion_anchor(&self, size: Size) -> Option<Point> {
        if self.editor_area.completion_items.is_empty() {
            return None;
        }
        if self.debug_panel.inspection.modal.is_open() {
            return self.inspection_completion_anchor();
        }
        if self.context.focus != ShellFocus::Editor {
            return None;
        }
        let text = self.active_text()?;
        let geo = self.geometry(size);
        let editor_x = ACTIVITY_WIDTH + self.sidebar_width(size);
        let (line, column) = line_column(text, self.editor_area.pane.cursor());
        Some(Point::new(
            (editor_x + EDITOR_GUTTER + column as f32 * EDITOR_CHAR_WIDTH)
                .min(size.width - 270.0)
                .max(editor_x + EDITOR_GUTTER),
            (geo.content_top
                + 36.0
                + line.saturating_sub(self.editor_area.pane.scroll_line()) as f32
                    * EDITOR_LINE_HEIGHT)
                .min(geo.editor_bottom - 190.0),
        ))
    }

    /// Área ocupada pela lista de completação na tela.
    fn completion_rect(&self, size: Size) -> Option<Rect> {
        let anchor = self.completion_anchor(size)?;
        let rows = self
            .editor_area
            .completion_items
            .len()
            .min(COMPLETION_VISIBLE_ROWS);
        Some(Rect::new(
            anchor.x,
            anchor.y,
            COMPLETION_POPUP_WIDTH + COMPLETION_POPUP_PADDING * 2.0,
            rows as f32 * COMPLETION_ROW_HEIGHT + COMPLETION_POPUP_PADDING * 2.0,
        ))
    }

    /// Clique com a lista aberta. Devolve `true` quando ela consumiu o clique.
    ///
    /// Fora dela, o clique a dispensa: o usuário foi olhar outra coisa, e uma
    /// lista que sobrevive a isso fica pairando sobre um cursor que já se moveu.
    /// Dentro dela, escolhe a linha — e precisa consumir o clique de qualquer
    /// forma, ou ele atravessaria a lista e moveria o cursor no editor de baixo.
    fn completion_pointer_down(&mut self, point: Point, size: Size) -> bool {
        let Some(rect) = self.completion_rect(size) else {
            return false;
        };
        if !rect.contains(point) {
            self.clear_completions();
            return false;
        }
        let row = ((point.y - rect.origin.y - COMPLETION_POPUP_PADDING) / COMPLETION_ROW_HEIGHT)
            .floor()
            .max(0.0) as usize;
        if row < self.editor_area.completion_items.len() {
            self.editor_area.completion_selected = row;
            self.accept_completion();
        }
        true
    }

    /// Canto onde a lista nasce dentro da janela de inspeção.
    fn inspection_completion_anchor(&self) -> Option<Point> {
        if self.editor_area.completion_items.is_empty() {
            return None;
        }
        let bounds = self.debug_panel.inspection.editor.bounds();
        let (line, column) = line_column(
            self.debug_panel.inspection.source.text(),
            self.debug_panel.inspection.editor.cursor(),
        );
        Some(Point::new(
            (bounds.origin.x + EDITOR_GUTTER + column as f32 * EDITOR_CHAR_WIDTH)
                .min(bounds.origin.x + bounds.size.width - COMPLETION_POPUP_WIDTH)
                .max(bounds.origin.x),
            bounds.origin.y
                + (line.saturating_sub(self.debug_panel.inspection.editor.scroll_line()) + 1)
                    as f32
                    * EDITOR_LINE_HEIGHT,
        ))
    }

    /// O mesmo, para quem só precisa ler.
    fn focused_editor_ref(&self) -> Option<(&EditorPane, &TextBuffer)> {
        if self.debug_panel.inspection.modal.is_open() {
            return Some((
                &self.debug_panel.inspection.editor,
                &self.debug_panel.inspection.source,
            ));
        }
        let document = self.editor_area.session.active()?;
        Some((&self.editor_area.pane, &document.buffer))
    }

    /// Põe o painel da frente na área que ele ocupa agora.
    ///
    /// Converter ponto em posição do texto depende de saber onde o painel está, e
    /// as duas áreas mudam com o tamanho da janela.
    fn place_focused_editor(&mut self, size: Size) {
        if self.debug_panel.inspection.modal.is_open() {
            self.layout_inspection_editor(size);
            return;
        }
        let bounds = self.editor_view_rect(size);
        self.editor_area.pane.set_bounds(bounds);
    }

    /// Digitação dentro da janela de inspeção. Devolve `true` quando consumiu.
    fn inspection_text_input(&mut self, text: &str) -> bool {
        if !self.debug_panel.inspection.modal.is_open()
            || self.debug_panel.inspection.focus != InspectionFocus::Source
        {
            return false;
        }
        self.debug_panel
            .inspection
            .editor
            .insert(&mut self.debug_panel.inspection.source, text);
        true
    }

    /// Tecla dentro da janela de inspeção. Devolve `true` quando consumiu.
    fn inspection_key(&mut self, key: &str, modifiers: Modifiers) -> bool {
        if !self.debug_panel.inspection.modal.is_open()
            || self.debug_panel.inspection.focus != InspectionFocus::Source
        {
            return false;
        }
        // Ctrl+Enter executa: a mão já está no teclado, escrevendo a expressão.
        if modifiers.control && key.eq_ignore_ascii_case("enter") {
            self.run_inspection_source();
            return true;
        }
        // A lista aberta tem precedência sobre o texto, como no editor da janela.
        if self.completion_key(key) {
            return true;
        }
        self.debug_panel.inspection.editor.key(
            &mut self.debug_panel.inspection.source,
            key,
            modifiers.shift,
            modifiers.control,
        );
        true
    }

    /// Expressão que está sendo inspecionada.
    #[must_use]
    pub fn inspected_expression(&self) -> Option<&str> {
        self.debug_panel
            .inspection
            .view
            .as_ref()
            .map(|inspection| inspection.expression.as_str())
    }

    pub fn close_inspection(&mut self) {
        self.debug_panel.inspection.message = None;
        self.debug_panel.inspection.run = None;
        self.debug_panel.inspection.modal.close();
        self.debug_panel.inspection.view = None;
    }

    /// Entrada destacada na árvore, que é a detalhada no painel direito.
    fn inspection_selected(&self) -> Option<&DebugVariableView> {
        let inspection = self.debug_panel.inspection.view.as_ref()?;
        inspection
            .root
            .find(&inspection.selected)
            .map(|node| &node.variable)
    }

    /// Pede a avaliação do trecho marcado no quadro atual da depuração.
    fn inspect_selection(&mut self) {
        let Some(range) = self.editor_area.pane.selection_range() else {
            return;
        };
        let Some(expression) = self
            .active_text()
            .and_then(|text| text.get(range))
            .map(str::trim)
            .filter(|expression| !expression.is_empty())
            .map(str::to_owned)
        else {
            return;
        };
        self.context.status_message = format!("Inspecionando {expression}");
        self.commands
            .push(ApplicationCommand::Debug(DebugRequest::Evaluate(
                expression,
            )));
    }

    /// Copia o trecho selecionado para a área de transferência do sistema.
    pub fn copy_selection(&mut self) -> bool {
        let selected = self
            .focused_editor_ref()
            .and_then(|(pane, buffer)| pane.selected_text(buffer))
            .map(str::to_owned);
        let Some(text) = selected else {
            self.context.status_message = "Nada selecionado".to_owned();
            return false;
        };
        let Some(clipboard) = self.context.clipboard.as_ref() else {
            self.context.status_message = "Área de transferência indisponível".to_owned();
            return false;
        };
        match clipboard.set_text(&text) {
            Ok(()) => {
                self.context.status_message =
                    format!("Copiado {} caractere(s)", text.chars().count());
                true
            }
            Err(error) => {
                self.context.status_message = error.to_string();
                false
            }
        }
    }

    /// Cola o conteúdo da área de transferência no cursor.
    ///
    /// Havendo trecho selecionado, ele é substituído — colar sobre uma seleção é
    /// trocar aquele texto, e é o que qualquer editor faz.
    pub fn paste_clipboard(&mut self) -> bool {
        let Some(clipboard) = self.context.clipboard.as_ref() else {
            self.context.status_message = "Área de transferência indisponível".to_owned();
            return false;
        };
        match clipboard.get_text() {
            Ok(Some(text)) if !text.is_empty() => {
                self.edit_focused(&text);
                true
            }
            Ok(_) => {
                self.context.status_message = "Área de transferência vazia".to_owned();
                false
            }
            Err(error) => {
                self.context.status_message = error.to_string();
                false
            }
        }
    }

    /// Escreve no painel que está na frente.
    ///
    /// Marcar o documento como modificado e fechar o autocomplete são efeitos do
    /// editor de arquivos; o rascunho da inspeção não tem nenhum dos dois.
    fn edit_focused(&mut self, text: &str) {
        if self.debug_panel.inspection.modal.is_open() {
            self.debug_panel
                .inspection
                .editor
                .insert(&mut self.debug_panel.inspection.source, text);
            return;
        }
        self.edit_active(text);
    }

    /// Escreve no documento ativo pelo painel de edição.
    fn edit_active(&mut self, text: &str) {
        self.editor_area.completion_items.clear();
        let Some(document) = self.editor_area.session.active_mut() else {
            return;
        };
        if self.editor_area.pane.insert(&mut document.buffer, text) {
            self.context.status_message = "Modified".to_owned();
        }
    }

    fn backspace(&mut self) {
        self.editor_area.completion_items.clear();
        let Some(document) = self.editor_area.session.active_mut() else {
            return;
        };
        let before = document.buffer.revision();
        self.editor_area
            .pane
            .key(&mut document.buffer, "backspace", false, false);
        if document.buffer.revision() != before {
            self.context.status_message = "Modified".to_owned();
        }
    }

    /// Tecla dirigida à lista de completação aberta. `true` quando ela consumiu.
    ///
    /// Com a lista à mostra, as setas andam nela e não no texto — é o que se
    /// espera de qualquer editor, e vale igual nos dois editores da IDE.
    fn completion_key(&mut self, key: &str) -> bool {
        if self.editor_area.completion_items.is_empty() {
            return false;
        }
        match key.to_ascii_lowercase().as_str() {
            "arrowdown" => {
                self.editor_area.completion_selected = (self.editor_area.completion_selected + 1)
                    .min(self.editor_area.completion_items.len() - 1);
                true
            }
            "arrowup" => {
                self.editor_area.completion_selected =
                    self.editor_area.completion_selected.saturating_sub(1);
                true
            }
            "enter" | "tab" => {
                self.accept_completion();
                true
            }
            "escape" => {
                self.editor_area.completion_items.clear();
                true
            }
            _ => false,
        }
    }

    fn accept_completion(&mut self) {
        let Some(item) = self
            .editor_area
            .completion_items
            .get(self.editor_area.completion_selected)
            .cloned()
        else {
            return;
        };
        // Trocar o que já foi digitado pelo item escolhido é a mesma operação nos
        // dois editores; o que muda é qual deles está na frente.
        if let Some((pane, buffer)) = self.focused_editor() {
            let cursor = pane.cursor().min(buffer.text().len());
            let prefix = identifier_prefix(buffer.text(), cursor);
            let start = cursor.saturating_sub(prefix.len());
            if buffer.replace(start..cursor, &item.label).is_ok() {
                pane.set_cursor(start + item.label.len());
                self.context.status_message = format!("Completed {}", item.label);
            }
        }
        self.editor_area.completion_items.clear();
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
        visit(
            &self.explorer.workspace,
            0,
            &self.explorer.expanded,
            &mut output,
        );
        output
    }

    /// Desenha o quadro.
    ///
    /// Pintar exige acesso mutável porque o shell mantém widgets com estado
    /// próprio — o editor guarda uma cópia do documento ativo, reconstruída
    /// quando o texto muda. Deixar essa reconciliação para os manipuladores de evento faria
    /// cada esquecimento virar um quadro desatualizado.
    pub fn paint(&mut self, size: Size) -> Vec<PaintCommand> {
        self.context.last_size = size;
        let sidebar = self.sidebar_width(size);
        let editor_x = ACTIVITY_WIDTH + sidebar;
        let geo = self.geometry(size);
        let colors = self.context.theme.colors;
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
                &self.explorer.workspace_name,
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
            if self.terminal.minimized { "^" } else { "v" },
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
        if !self.terminal.minimized {
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
        if self.editor_area.session.active().is_some() {
            // O editor da biblioteca desenha calha, números, realce, marcas de
            // ponto de parada, linha em execução e cursor. A IDE entrega o
            // texto, o realce e as decorações.
            self.sync_editor_pane(size);
            let mut editor_paint = self.paint_context();
            self.editor_area.pane.paint(&mut editor_paint);
            commands.extend(editor_paint.into_commands());
            commands.extend(self.paint_scrollbar(ScrollTarget::Editor, size));
            // A barra lateral só aparece quando há linha passando da área: uma
            // trilha permanente ocuparia altura útil sem servir para nada.
            if self.editor_scrolls_sideways(size) {
                commands.extend(self.paint_scrollbar(ScrollTarget::EditorHorizontal, size));
            }
        } else {
            commands.push(label(
                "Select a file in Explorer",
                Point::new(editor_x + 55.0, geo.content_top + 30.0),
                colors.muted_text,
                16.0,
            ));
        }
        commands.push(PaintCommand::PopClip);
        if self.debug_panel.view.attached {
            commands.extend(self.paint_debug_panel(size, colors));
        }
        if !self.terminal.minimized {
            let mut terminal_tabs = self.terminal_tabs();
            terminal_tabs.layout(&self.layout_context(), self.terminal_tabs_rect(size));
            let mut terminal_tabs_paint = self.paint_context();
            terminal_tabs.paint(&mut terminal_tabs_paint);
            commands.extend(terminal_tabs_paint.into_commands());
            commands.push(fill(
                Rect::new(editor_x, geo.editor_bottom + 30.0, geo.editor_width, 30.0),
                colors.background,
            ));
            let active_terminal = &self.terminal.tabs[self.terminal.active];
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
                    selection_columns(self.terminal.selection, absolute_line, &line.text)
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
        if !self.debug_panel.inspection.modal.is_open()
            && let Some(anchor) = self.completion_anchor(size)
        {
            self.paint_completion(&mut commands, size, anchor);
        }
        if self.context.focus == ShellFocus::Search {
            // Também da biblioteca: a caixa é um campo de texto sobre uma
            // superfície flutuante, e a IDE só escolhe o canto e o conteúdo.
            let width = SEARCH_BOX_WIDTH.min((geo.editor_width - 24.0).max(100.0));
            let mut surface = Popup::new(SEARCH_POPUP_ID).with_padding(6.0);
            surface.set_content_size(Size::new(width - 12.0, SEARCH_BOX_HEIGHT - 12.0));
            surface.layout(
                &self.layout_context(),
                Rect::new(0.0, 0.0, size.width, size.height),
            );
            surface.open_at(Point::new(
                size.width - width - 12.0,
                geo.content_top + 12.0,
            ));
            let mut search_paint = self.paint_context();
            surface.paint(&mut search_paint);
            if let Some(content) = surface.content_rect() {
                let mut field = TextInput::new(SEARCH_INPUT_ID, &self.editor_area.search_query)
                    .with_placeholder("Buscar no arquivo");
                // A busca só aparece quando tem o foco do shell, então o campo
                // é desenhado no estado focado — é ali que o cursor está.
                field.event(&mut EventContext::default(), &UiEvent::FocusGained);
                field.layout(&self.layout_context(), content);
                field.paint(&mut search_paint);
            }
            commands.extend(search_paint.into_commands());
        }
        let position = self
            .active_text()
            .map(|text| line_column(text, self.editor_area.pane.cursor()))
            .unwrap_or((0, 0));
        // A barra de estado é da biblioteca: superfície, borda, alinhamento e
        // recorte vêm de lá. A IDE só diz o que cada segmento informa — a
        // mensagem da última ação à esquerda, e à direita o que o usuário
        // procura sempre no mesmo lugar.
        let mut status_bar =
            StatusBar::new(STATUS_BAR_ID).with_leading([&self.context.status_message]);
        let mut trailing = vec![
            "UTF-8".to_owned(),
            format!("Ln {}, Col {}", position.0 + 1, position.1 + 1),
        ];
        if let Some(summary) = self.context.project_summary.as_deref() {
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
        let mut menu_bar = self.menu.bar.clone();
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
        let mut stop = self.debug_panel.stop_button.clone();
        stop.set_tint(if self.application_running() {
            IconTint::Danger
        } else {
            IconTint::Muted
        });
        stop.set_disabled(!self.application_running());
        let mut debug = self.debug_panel.debug_button.clone();
        debug.set_tint(if self.debug_panel.view.attached {
            IconTint::Accent
        } else {
            IconTint::Muted
        });
        let mut run = self.debug_panel.run_button.clone();
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
        if self.settings.modal.is_open() {
            let mut modal = self.settings.modal.clone();
            modal.layout(
                &self.layout_context(),
                Rect::new(0.0, 0.0, size.width, size.height),
            );
            let geometry = settings_dialog_geometry(modal.panel_bounds());
            let mut modal_paint = self.paint_context();
            modal.paint(&mut modal_paint);
            commands.extend(modal_paint.into_commands());
            commands.push(fill(geometry.sidebar, colors.surface));
            let mut component_paint = self.paint_context();
            // A navegação entre páginas é a `ListView` da biblioteca, no estilo
            // de marcador: a linha ativa ganha a barra de destaque e continua
            // sendo um rótulo para ler.
            let pages = self.settings_pages_for(&geometry);
            pages.paint(&mut component_paint);
            match self.settings.page {
                SettingsPage::Contribution(index) => {
                    let section = self.catalog.settings_sections.get(index);
                    // Título e legenda são `Label` da biblioteca: tamanho e cor
                    // vêm do tema, não de números escritos aqui.
                    self.paint_settings_text(
                        &mut component_paint,
                        SETTINGS_TITLE_ID,
                        section.map_or("Ferramenta", |section| section.title.as_str()),
                        Point::new(geometry.combo.origin.x, geometry.combo.origin.y - 34.0),
                        17.0,
                        IconTint::Text,
                    );
                    self.paint_settings_text(
                        &mut component_paint,
                        SETTINGS_CAPTION_ID,
                        section.map_or("Toolchain", |section| section.field_caption.as_str()),
                        Point::new(geometry.combo.origin.x, geometry.combo.origin.y - 16.0),
                        13.0,
                        IconTint::Muted,
                    );
                    let mut combo = self.settings.toolchain_combo.clone();
                    combo.layout(&self.layout_context(), geometry.combo);
                    combo.paint(&mut component_paint);
                    let mut browse = self.settings.toolchain_browse_button.clone();
                    browse.layout(&self.layout_context(), geometry.browse);
                    browse.paint(&mut component_paint);
                    // A segunda escolha só existe se a seção declarar uma: a
                    // tela desenha o que lhe dizem, sem saber o que é.
                    if let Some(caption) = self
                        .catalog
                        .settings_sections
                        .first()
                        .and_then(|section| section.secondary_caption.clone())
                    {
                        self.paint_settings_text(
                            &mut component_paint,
                            SECONDARY_TOOL_CAPTION_ID,
                            &caption,
                            Point::new(
                                geometry.secondary_combo.origin.x,
                                geometry.secondary_combo.origin.y - 16.0,
                            ),
                            13.0,
                            IconTint::Muted,
                        );
                        let mut combo = self.settings.secondary_combo.clone();
                        combo.layout(&self.layout_context(), geometry.secondary_combo);
                        combo.paint(&mut component_paint);
                        let mut browse = self.settings.secondary_browse_button.clone();
                        browse.layout(&self.layout_context(), geometry.secondary_browse);
                        browse.paint(&mut component_paint);
                    }
                }
                SettingsPage::Debug => {
                    commands.extend(self.paint_debug_settings(&geometry, colors));
                    let mut host = self.settings.debug_host.clone();
                    host.layout(&self.layout_context(), geometry.debug_host);
                    host.paint(&mut component_paint);
                    let mut port = self.settings.debug_port.clone();
                    port.layout(&self.layout_context(), geometry.debug_port);
                    port.paint(&mut component_paint);
                    let mut attach = self.settings.debug_attach_button.clone();
                    attach.layout(&self.layout_context(), geometry.debug_attach);
                    attach.paint(&mut component_paint);
                }
            }
            let mut close = self.settings.close_button.clone();
            close.layout(&self.layout_context(), geometry.close);
            close.paint(&mut component_paint);
            let mut save = self.settings.save_button.clone();
            save.layout(&self.layout_context(), geometry.save);
            save.paint(&mut component_paint);
            if let Some(message) = self
                .settings
                .dialog
                .as_ref()
                .and_then(|dialog| dialog.message.as_ref())
            {
                self.paint_settings_text(
                    &mut component_paint,
                    SETTINGS_MESSAGE_ID,
                    message,
                    Point::new(geometry.combo.origin.x, geometry.combo.origin.y + 54.0),
                    13.0,
                    IconTint::Danger,
                );
            }
            commands.extend(component_paint.into_commands());
        }
        // A janela de criação cobre o conteúdo, e o menu de contexto cobre ela.
        self.paint_new_item_dialog(&mut commands, size);
        self.paint_generate(&mut commands, size);
        self.paint_rename(&mut commands, size);
        self.paint_type_search(&mut commands, size);
        self.paint_inspection(&mut commands, size);
        // Depois da janela de inspeção, ou a lista ficaria atrás dela.
        if self.debug_panel.inspection.modal.is_open()
            && let Some(anchor) = self.inspection_completion_anchor()
        {
            self.paint_completion(&mut commands, size, anchor);
        }
        // O menu de contexto é desenhado por último: ele cobre tudo, inclusive
        // o painel de onde foi aberto.
        if self.explorer.context_menu.is_open() {
            let mut menu = self.explorer.context_menu.clone();
            menu.layout(
                &self.layout_context(),
                Rect::new(0.0, 0.0, size.width, size.height),
            );
            let mut menu_paint = self.paint_context();
            menu.paint(&mut menu_paint);
            commands.extend(menu_paint.into_commands());
        }
        commands
    }

    /// Desenha a janela de inspeção: lista à esquerda, detalhe à direita.
    ///
    /// A lista mostra o valor pedido e os campos dele; o painel direito descreve
    /// a entrada destacada. O valor completo cabe ali, e não numa linha de lista,
    /// que o recorte cortaria justamente onde está a informação.
    /// Desenha a lista de completação ancorada num ponto da tela.
    ///
    /// Superfície flutuante e lista são da biblioteca: a IDE só diz onde ancorar,
    /// o que listar e o que está selecionado. Quem chama decide o ponto, e é isso
    /// que permite a mesma lista servir ao editor da janela e ao do depurador,
    /// que estão em lugares diferentes.
    fn paint_completion(&self, commands: &mut Vec<PaintCommand>, size: Size, anchor: Point) {
        if self.editor_area.completion_items.is_empty() {
            return;
        }
        let visible = self
            .editor_area
            .completion_items
            .len()
            .min(COMPLETION_VISIBLE_ROWS);
        let mut surface = Popup::new(COMPLETION_POPUP_ID).with_padding(COMPLETION_POPUP_PADDING);
        surface.set_content_size(Size::new(
            COMPLETION_POPUP_WIDTH,
            visible as f32 * COMPLETION_ROW_HEIGHT,
        ));
        surface.layout(
            &self.layout_context(),
            Rect::new(0.0, 0.0, size.width, size.height),
        );
        surface.open_at(anchor);
        let mut popup_paint = self.paint_context();
        surface.paint(&mut popup_paint);
        if let Some(content) = surface.content_rect() {
            let mut list = ListView::new(
                COMPLETION_LIST_ID,
                self.editor_area
                    .completion_items
                    .iter()
                    .map(|item| item.label.clone())
                    .collect::<Vec<_>>(),
            )
            // Sem cor própria: o texto do tema é o escolhido para se ler sobre a
            // superfície, e é o mesmo em toda a interface.
            .with_row_height(COMPLETION_ROW_HEIGHT);
            list.set_selected(Some(self.editor_area.completion_selected));
            list.layout(&self.layout_context(), content);
            list.paint(&mut popup_paint);
        }
        commands.extend(popup_paint.into_commands());
    }

    fn paint_inspection(&self, commands: &mut Vec<PaintCommand>, size: Size) {
        let Some(inspection) = self.debug_panel.inspection.view.as_ref() else {
            return;
        };
        let mut modal = self.debug_panel.inspection.modal.clone();
        modal.layout(
            &self.layout_context(),
            Rect::new(0.0, 0.0, size.width, size.height),
        );
        let geometry = inspection_geometry(modal.panel_bounds());
        let mut paint = self.paint_context();
        modal.paint(&mut paint);

        let mut tree = self.debug_panel.inspection.tree.clone();
        tree.set_selected(Some(inspection_id(&inspection.selected)));
        tree.layout(&self.layout_context(), geometry.list);
        tree.paint(&mut paint);

        let detail = geometry.detail;
        match self.inspection_selected() {
            Some(entry) => {
                self.paint_settings_text(
                    &mut paint,
                    INSPECTION_NAME_ID,
                    &entry.name,
                    Point::new(detail.origin.x, detail.origin.y),
                    17.0,
                    IconTint::Text,
                );
                self.paint_settings_text(
                    &mut paint,
                    INSPECTION_TYPE_ID,
                    entry.type_name.as_deref().unwrap_or("tipo desconhecido"),
                    Point::new(detail.origin.x, detail.origin.y + 26.0),
                    13.0,
                    IconTint::Muted,
                );
                self.paint_settings_text(
                    &mut paint,
                    INSPECTION_VALUE_ID,
                    &entry.value,
                    Point::new(detail.origin.x, detail.origin.y + 56.0),
                    14.0,
                    IconTint::Text,
                );
            }
            None => self.paint_settings_text(
                &mut paint,
                INSPECTION_EMPTY_ID,
                "Sem valor para mostrar",
                Point::new(detail.origin.x, detail.origin.y),
                14.0,
                IconTint::Muted,
            ),
        }

        // O editor de expressões é o mesmo painel da janela principal, com os
        // comportamentos de arquivo desligados.
        self.paint_settings_text(
            &mut paint,
            INSPECTION_SOURCE_CAPTION_ID,
            "Código a executar no quadro atual",
            Point::new(geometry.source.origin.x, geometry.source.origin.y - 16.0),
            13.0,
            IconTint::Muted,
        );
        let mut editor = self.debug_panel.inspection.editor.clone();
        editor.set_bounds(geometry.source);
        editor.sync(
            &self.layout_context(),
            &self.debug_panel.inspection.source,
            None,
            Vec::new(),
            true,
        );
        editor.paint(&mut paint);

        if let Some(message) = self.debug_panel.inspection.message.as_ref() {
            // A mensagem tem a linha inteira, acima dos botões: dividir a linha
            // com eles a fazia passar por baixo do Executar, ilegível justamente
            // quando é ela que explica por que o clique não fez nada.
            self.paint_settings_text(
                &mut paint,
                INSPECTION_MESSAGE_ID,
                &clipped_message(message, geometry.message.size.width),
                geometry.message.origin,
                13.0,
                IconTint::Danger,
            );
        }
        let mut run = self.debug_panel.inspection.run_button.clone();
        // Sem sessão viva não há quadro onde executar: o botão apagado diz isso
        // antes do clique, em vez de a mensagem dizer depois.
        run.set_disabled(!self.debug_panel.view.attached);
        run.layout(&self.layout_context(), geometry.run);
        run.paint(&mut paint);
        let mut close = self.debug_panel.inspection.close_button.clone();
        close.layout(&self.layout_context(), geometry.close);
        close.paint(&mut paint);
        commands.extend(paint.into_commands());
    }

    /// Roteia o clique dentro da janela de inspeção.
    /// Põe o painel de edição da inspeção na área que ele ocupa agora.
    ///
    /// O painel só sabe converter ponto em posição do texto depois de saber onde
    /// está, e a janela é centrada: a área muda com o tamanho da tela.
    fn layout_inspection_editor(&mut self, size: Size) -> InspectionGeometry {
        self.debug_panel.inspection.modal.layout(
            &self.layout_context(),
            Rect::new(0.0, 0.0, size.width, size.height),
        );
        let geometry = inspection_geometry(self.debug_panel.inspection.modal.panel_bounds());
        self.debug_panel
            .inspection
            .editor
            .set_bounds(geometry.source);
        geometry
    }

    fn inspection_pointer_down(&mut self, point: Point, size: Size) {
        let geometry = self.layout_inspection_editor(size);
        if geometry.close.contains(point) {
            self.close_inspection();
            return;
        }
        if geometry.run.contains(point) {
            self.run_inspection_source();
            return;
        }
        if geometry.source.contains(point) {
            // O editor cuida do próprio cursor e da própria seleção.
            self.debug_panel.inspection.editor.pointer_down(
                &self.debug_panel.inspection.source,
                point,
                false,
                false,
            );
            self.debug_panel.inspection.focus = InspectionFocus::Source;
            return;
        }
        self.debug_panel.inspection.focus = InspectionFocus::Tree;
        if !geometry.list.contains(point) {
            return;
        }
        // Qual nó foi clicado é a árvore quem sabe: recuo, marcador de expansão e
        // rolagem são dela.
        let mut tree = self.debug_panel.inspection.tree.clone();
        tree.layout(&self.layout_context(), geometry.list);
        tree.event(
            &mut EventContext::default(),
            &UiEvent::PointerDown(primary_pointer(point)),
        );
        let Some(id) = tree.selected() else {
            return;
        };
        let Some(inspection) = self.debug_panel.inspection.view.as_mut() else {
            return;
        };
        let Some(path) = inspection_path_of(&inspection.root, id) else {
            return;
        };
        inspection.selected = path.clone();
        let expandable = inspection
            .root
            .find(&path)
            .is_some_and(|node| node.variable.expandable);
        if expandable {
            // Clicar em um valor com campos abre e fecha, como no Explorer.
            if !inspection.expanded.remove(&path) {
                inspection.expanded.insert(path.clone());
                let pending = inspection.root.find(&path).is_some_and(|node| !node.loaded);
                if pending {
                    // Os campos só são pedidos ao abrir: perguntar por tudo de
                    // uma vez percorreria o grafo inteiro do objeto.
                    self.commands
                        .push(ApplicationCommand::Debug(DebugRequest::ExpandInspection(
                            path,
                        )));
                }
            }
        }
        self.sync_inspection_tree();
    }

    /// Desenha a janela de criação por cima de tudo.
    ///
    /// Moldura, véu e título são do `ModalHost`; os campos, os botões e as
    /// legendas são componentes da biblioteca. A IDE diz onde e o que.
    /// Desenha a busca de tipo: campo em cima, resultados embaixo.
    ///
    /// Janela, campo e lista são da biblioteca; a IDE diz o que cada um mostra.
    fn paint_type_search(&self, commands: &mut Vec<PaintCommand>, size: Size) {
        if !self.search.modal.is_open() {
            return;
        }
        let mut modal = self.search.modal.clone();
        modal.layout(
            &self.layout_context(),
            Rect::new(0.0, 0.0, size.width, size.height),
        );
        let mut paint = self.paint_context();
        modal.paint(&mut paint);
        let panel = modal.panel_bounds();
        let input = Rect::new(
            panel.origin.x + 16.0,
            panel.origin.y + 56.0,
            panel.size.width - 32.0,
            34.0,
        );
        let placeholder = match self.search.mode {
            WorkspaceSearchMode::Types => "Nome da classe, interface, record ou enum",
            WorkspaceSearchMode::Content => "Texto nos arquivos do escopo do projeto",
        };
        let mut field =
            TextInput::new(TYPE_SEARCH_INPUT_ID, &self.search.query).with_placeholder(placeholder);
        field.event(&mut EventContext::default(), &UiEvent::FocusGained);
        field.layout(&self.layout_context(), input);
        field.paint(&mut paint);

        let list_rect = Rect::new(
            panel.origin.x + 16.0,
            input.origin.y + input.size.height + 12.0,
            panel.size.width - 32.0,
            (panel.origin.y + panel.size.height - 16.0)
                - (input.origin.y + input.size.height + 12.0),
        );
        let labels = match self.search.mode {
            WorkspaceSearchMode::Types => self
                .search
                .type_results
                .iter()
                .skip(self.search.first_visible)
                .take(TYPE_SEARCH_VISIBLE_ROWS)
                .map(|hit| hit.label(&self.catalog.source_root_names))
                .collect::<Vec<_>>(),
            WorkspaceSearchMode::Content => self
                .search
                .content_results
                .iter()
                .skip(self.search.first_visible)
                .take(TYPE_SEARCH_VISIBLE_ROWS)
                .map(|hit| hit.label(&self.catalog.source_root_names))
                .collect::<Vec<_>>(),
        };
        let mut list =
            ListView::new(TYPE_SEARCH_LIST_ID, labels).with_row_height(TYPE_SEARCH_ROW_HEIGHT);
        list.set_selected(self.search.selected.checked_sub(self.search.first_visible));
        list.layout(&self.layout_context(), list_rect);
        list.paint(&mut paint);
        commands.extend(paint.into_commands());
    }

    fn paint_new_item_dialog(&self, commands: &mut Vec<PaintCommand>, size: Size) {
        let Some(dialog) = self.search.new_item_dialog.as_ref() else {
            return;
        };
        let mut modal = self.search.new_item_modal.clone();
        // O painel se centraliza na área que recebe no layout. Sem esse layout a
        // área é zero, e a janela nasce no canto superior esquerdo.
        modal.layout(
            &self.layout_context(),
            Rect::new(0.0, 0.0, size.width, size.height),
        );
        let panel = modal.panel_bounds();
        let geometry = new_item_geometry(panel);
        let mut paint = self.paint_context();
        // O título é do `ModalHost`, que já o desenha: escrever outro por cima
        // era o que aparecia duplicado.
        modal.paint(&mut paint);
        self.paint_settings_text(
            &mut paint,
            NEW_ITEM_PACKAGE_CAPTION_ID,
            "Pacote",
            Point::new(geometry.package.origin.x, geometry.package.origin.y - 18.0),
            13.0,
            IconTint::Muted,
        );
        self.paint_settings_text(
            &mut paint,
            NEW_ITEM_NAME_CAPTION_ID,
            &dialog.template.name_caption,
            Point::new(geometry.name.origin.x, geometry.name.origin.y - 18.0),
            13.0,
            IconTint::Muted,
        );
        for (field, rect) in [
            (&self.search.new_item_package, geometry.package),
            (&self.search.new_item_name, geometry.name),
        ] {
            // O foco já está no campo de verdade; aqui é só desenhar.
            let mut field = field.clone();
            field.layout(&self.layout_context(), rect);
            field.paint(&mut paint);
        }
        for (button, rect) in [
            (&self.search.new_item_cancel_button, geometry.cancel),
            (&self.search.new_item_create_button, geometry.create),
        ] {
            let mut button = button.clone();
            button.layout(&self.layout_context(), rect);
            button.paint(&mut paint);
        }
        if let Some(message) = dialog.message.as_ref() {
            self.paint_settings_text(
                &mut paint,
                NEW_ITEM_MESSAGE_ID,
                message,
                Point::new(geometry.name.origin.x, geometry.name.origin.y + 44.0),
                13.0,
                IconTint::Danger,
            );
        }
        commands.extend(paint.into_commands());
    }

    /// Roteia o clique dentro da janela de criação.
    fn new_item_pointer_down(&mut self, point: Point, size: Size) {
        self.search.new_item_modal.layout(
            &self.layout_context(),
            Rect::new(0.0, 0.0, size.width, size.height),
        );
        let geometry = new_item_geometry(self.search.new_item_modal.panel_bounds());
        // O clique vai ao campo: onde o cursor fica dentro do texto é ele quem
        // sabe, porque a medição da fonte é dele.
        for (naming, rect) in [(false, geometry.package), (true, geometry.name)] {
            if !rect.contains(point) {
                continue;
            }
            let context = self.layout_context();
            let field = if naming {
                &mut self.search.new_item_name
            } else {
                &mut self.search.new_item_package
            };
            field.layout(&context, rect);
            field.event(
                &mut EventContext::default(),
                &UiEvent::PointerDown(primary_pointer(point)),
            );
            self.focus_new_item_field(naming);
            return;
        }
        if geometry.create.contains(point) {
            self.submit_new_item();
            return;
        }
        if geometry.cancel.contains(point) {
            self.close_new_item_dialog();
        }
    }

    /// Move o foco entre os dois campos.
    ///
    /// O foco fica nos campos de verdade, e não num clone da pintura: é ele que
    /// decide onde a digitação entra e onde o cursor aparece.
    fn focus_new_item_field(&mut self, naming: bool) {
        if let Some(dialog) = self.search.new_item_dialog.as_mut() {
            dialog.naming = naming;
        }
        let mut context = EventContext::default();
        let (focused, blurred) = if naming {
            (
                &mut self.search.new_item_name,
                &mut self.search.new_item_package,
            )
        } else {
            (
                &mut self.search.new_item_package,
                &mut self.search.new_item_name,
            )
        };
        focused.event(&mut context, &UiEvent::FocusGained);
        blurred.event(&mut context, &UiEvent::FocusLost);
    }

    /// O campo que está recebendo o que for digitado.
    fn new_item_field(&mut self) -> Option<&mut TextInput> {
        let naming = self.search.new_item_dialog.as_ref()?.naming;
        Some(if naming {
            &mut self.search.new_item_name
        } else {
            &mut self.search.new_item_package
        })
    }

    /// Tecla dentro da janela de criação. Devolve `true` quando a consumiu.
    fn new_item_key(&mut self, key: &str) -> bool {
        if !self.search.new_item_modal.is_open() {
            return false;
        }
        let Some(naming) = self
            .search
            .new_item_dialog
            .as_ref()
            .map(|dialog| dialog.naming)
        else {
            return false;
        };
        match key.to_ascii_lowercase().as_str() {
            "enter" => self.submit_new_item(),
            "escape" => self.close_new_item_dialog(),
            "tab" => self.focus_new_item_field(!naming),
            // Apagar e mover o cursor são do campo: ele conhece as fronteiras de
            // caractere e a posição atual.
            _ => {
                let event = UiEvent::KeyDown(KeyEvent {
                    logical_key: key.to_owned(),
                    repeat: false,
                    modifiers: Modifiers::default(),
                });
                if let Some(field) = self.new_item_field() {
                    field.event(&mut EventContext::default(), &event);
                }
            }
        }
        true
    }

    /// Texto digitado na janela de criação. Devolve `true` quando o consumiu.
    ///
    /// O texto entra pelo componente, e não por concatenação: é assim que ele
    /// aparece onde o cursor está, inclusive depois de um clique no meio do
    /// caminho já digitado.
    fn new_item_text_input(&mut self, text: &str) -> bool {
        if !self.search.new_item_modal.is_open() {
            return false;
        }
        let event = UiEvent::TextInput(TextInputEvent {
            text: text.to_owned(),
        });
        if let Some(field) = self.new_item_field() {
            field.event(&mut EventContext::default(), &event);
        }
        if let Some(dialog) = self.search.new_item_dialog.as_mut() {
            // Digitar é corrigir: a mensagem do erro anterior sai de cena.
            dialog.message = None;
        }
        true
    }

    /// Lista de páginas posicionada, com a página atual selecionada.
    ///
    /// A área ocupada é a das duas linhas, não a barra inteira: a lista responde
    /// pelo que ela desenha, e o resto da barra é fundo do painel.
    fn settings_pages_for(&self, geometry: &SettingsDialogGeometry) -> ListView {
        let mut pages = self.settings.pages.clone();
        pages.set_selected(Some(match self.settings.page {
            SettingsPage::Contribution(index) => index,
            SettingsPage::Debug => self.catalog.settings_sections.len(),
        }));
        pages.layout(
            &self.layout_context(),
            settings_pages_rect(geometry, self.catalog.settings_sections.len() + 1),
        );
        pages
    }

    /// Desenha um texto da janela de configurações com a `Label` da biblioteca.
    ///
    /// A IDE escolhe o papel — título, legenda, mensagem de erro — e a posição.
    /// Cor e desenho vêm do componente, e é assim que o tema alcança este texto.
    fn paint_settings_text(
        &self,
        context: &mut PaintContext,
        id: WidgetId,
        text: &str,
        origin: Point,
        font_size: f32,
        tone: IconTint,
    ) {
        let mut label = Label::new(id, text)
            .with_font_size(font_size)
            .with_tone(tone);
        label.layout(
            &self.layout_context(),
            Rect::new(origin.x, origin.y, 0.0, 0.0),
        );
        label.paint(context);
    }

    fn settings_dialog_pointer_down(&mut self, point: Point, size: Size) {
        if !self.settings.modal.is_open() {
            return;
        }
        self.settings.modal.layout(
            &self.layout_context(),
            Rect::new(0.0, 0.0, size.width, size.height),
        );
        let geometry = settings_dialog_geometry(self.settings.modal.panel_bounds());
        // Qual página foi clicada é a lista quem sabe: altura de linha e rolagem
        // são dela.
        let mut pages = self.settings_pages_for(&geometry);
        pages.event(
            &mut EventContext::default(),
            &UiEvent::PointerDown(primary_pointer(point)),
        );
        if let Some(page) = pages.selected().map(|index| {
            if index < self.catalog.settings_sections.len() {
                SettingsPage::Contribution(index)
            } else {
                SettingsPage::Debug
            }
        }) && settings_pages_rect(&geometry, self.catalog.settings_sections.len() + 1)
            .contains(point)
        {
            self.settings.page = page;
            self.settings.focus = None;
            return;
        }
        if self.settings.page == SettingsPage::Debug {
            self.debug_page_pointer_down(point, &geometry);
            return;
        }
        self.settings
            .toolchain_combo
            .layout(&self.layout_context(), geometry.combo);
        self.settings
            .toolchain_browse_button
            .layout(&self.layout_context(), geometry.browse);
        // A segunda escolha da seção recebe o clique pelo mesmo caminho: sem
        // isso ela é desenhada e nunca respondia.
        self.settings
            .secondary_combo
            .layout(&self.layout_context(), geometry.secondary_combo);
        self.settings
            .secondary_browse_button
            .layout(&self.layout_context(), geometry.secondary_browse);
        self.settings
            .close_button
            .layout(&self.layout_context(), geometry.close);
        self.settings
            .save_button
            .layout(&self.layout_context(), geometry.save);
        let event = UiEvent::PointerDown(primary_pointer(point));
        let mut context = EventContext::default();
        let combo_result = self.settings.toolchain_combo.event(&mut context, &event);
        let combo_consumed = !matches!(combo_result, EventResult::Ignored);
        if self.handle_settings_action(combo_result) || combo_consumed {
            return;
        }
        let browse_result = click_widget(&mut self.settings.toolchain_browse_button, point);
        if self.handle_settings_action(browse_result) {
            return;
        }
        let segunda = self.settings.secondary_combo.event(&mut context, &event);
        let segunda_consumiu = !matches!(segunda, EventResult::Ignored);
        if self.handle_settings_action(segunda) || segunda_consumiu {
            return;
        }
        let segunda_browse = click_widget(&mut self.settings.secondary_browse_button, point);
        if self.handle_settings_action(segunda_browse) {
            return;
        }
        let close_result = click_widget(&mut self.settings.close_button, point);
        if self.handle_settings_action(close_result) {
            return;
        }
        let save_result = click_widget(&mut self.settings.save_button, point);
        if self.handle_settings_action(save_result) {
            return;
        }
        let _ = self.settings.modal.event(&mut context, &event);
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
                "Vale para qualquer processo depurado: servidor, container ou ferramenta.",
                Point::new(origin.x, geometry.debug_attach.origin.y + 66.0),
                colors.muted_text,
                12.0,
            ),
        ];
        for (rect, id) in [
            (geometry.debug_host, DEBUG_HOST_ID),
            (geometry.debug_port, DEBUG_PORT_ID),
        ] {
            if self.settings.focus == Some(id) {
                commands.push(stroke(rect, colors.accent));
            }
        }
        commands
    }

    fn debug_page_pointer_down(&mut self, point: Point, geometry: &SettingsDialogGeometry) {
        if geometry.debug_host.contains(point) {
            self.settings.focus = Some(DEBUG_HOST_ID);
            return;
        }
        if geometry.debug_port.contains(point) {
            self.settings.focus = Some(DEBUG_PORT_ID);
            return;
        }
        if geometry.debug_attach.contains(point) {
            // O foco é preservado para o usuário corrigir um valor recusado.
            self.attach_debug_target();
            return;
        }
        self.settings.focus = None;
        self.settings
            .close_button
            .layout(&self.layout_context(), geometry.close);
        let close_result = click_widget(&mut self.settings.close_button, point);
        if self.handle_settings_action(close_result) {
            return;
        }
        self.settings
            .save_button
            .layout(&self.layout_context(), geometry.save);
        let save_result = click_widget(&mut self.settings.save_button, point);
        let _ = self.handle_settings_action(save_result);
    }

    /// Botões de ação da barra, na ordem em que aparecem.
    #[must_use]
    pub fn action_buttons(&self) -> [&Button; 3] {
        [
            &self.debug_panel.stop_button,
            &self.debug_panel.run_button,
            &self.debug_panel.debug_button,
        ]
    }

    /// Roteia o clique para os botões de ação, que são widgets da biblioteca.
    fn action_buttons_pointer_down(&mut self, point: Point, size: Size) -> bool {
        let rects = action_button_rects(size);
        self.debug_panel
            .stop_button
            .layout(&self.layout_context(), rects[0]);
        self.debug_panel
            .run_button
            .layout(&self.layout_context(), rects[1]);
        self.debug_panel
            .debug_button
            .layout(&self.layout_context(), rects[2]);
        let commands = [
            click_widget(&mut self.debug_panel.stop_button, point),
            click_widget(&mut self.debug_panel.run_button, point),
            click_widget(&mut self.debug_panel.debug_button, point),
        ];
        for result in commands {
            if let EventResult::Action(WidgetAction::Command(command)) = result {
                match command.0.as_str() {
                    "project.stop" => self.commands.push(ApplicationCommand::StopProject),
                    "project.run" => {
                        self.commands.push(ApplicationCommand::RunProject);
                        self.context.status_message = "Executando a aplicação".to_owned();
                    }
                    task if task.starts_with("task.execute.") => {
                        if let Some(id) = task.strip_prefix("task.execute.") {
                            self.commands
                                .push(ApplicationCommand::ExecuteTask(TaskId(id.to_owned())));
                        }
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
        if self.debug_panel.view.attached {
            self.context.status_message = "Depuração já conectada".to_owned();
            return;
        }
        match self.debug_target() {
            Some((host, port)) => {
                self.commands
                    .push(ApplicationCommand::Debug(DebugRequest::RunAndAttach {
                        host,
                        port,
                    }));
            }
            None => {
                self.settings.page = SettingsPage::Debug;
                self.commands.push(ApplicationCommand::OpenSettings);
                self.context.status_message =
                    "Informe um host e uma porta de depuração válidos".to_owned();
            }
        }
    }

    /// Valida host e porta antes de pedir a conexão à aplicação.
    fn attach_debug_target(&mut self) {
        let host = self.settings.debug_host.value().trim().to_owned();
        let port = self.settings.debug_port.value().trim().parse::<u16>().ok();
        match (host.is_empty(), port) {
            (false, Some(port)) if port > 0 => {
                self.commands
                    .push(ApplicationCommand::Debug(DebugRequest::Attach {
                        host: host.clone(),
                        port,
                    }));
                self.settings.modal.close();
                self.settings.dialog = None;
                self.settings.focus = None;
                self.context.status_message =
                    format!("Conectando ao alvo de depuração {host}:{port}");
            }
            _ => {
                self.set_settings_message("Informe um host e uma porta de depuração válidos.");
            }
        }
    }

    /// Digitação enquanto a página de depuração está em foco.
    fn settings_text_input(&mut self, text: &str) -> bool {
        let Some(focus) = self.settings.focus else {
            return false;
        };
        let input = if focus == DEBUG_HOST_ID {
            &mut self.settings.debug_host
        } else {
            &mut self.settings.debug_port
        };
        let mut value = input.value().to_owned();
        value.push_str(text);
        input.set_value(value);
        true
    }

    fn settings_key_down(&mut self, key: &str) -> bool {
        let Some(focus) = self.settings.focus else {
            return false;
        };
        match key {
            "Backspace" => {
                let input = if focus == DEBUG_HOST_ID {
                    &mut self.settings.debug_host
                } else {
                    &mut self.settings.debug_port
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
            .strip_prefix("toolchain.select.")
            .and_then(|value| value.parse::<usize>().ok())
        {
            // A escolha fica pendente: quem aplica é o Salvar.
            if let Some(dialog) = self.settings.dialog.as_mut() {
                dialog.pending_toolchain = Some(index);
            }
            return true;
        }
        if let Some(index) = command
            .0
            .strip_prefix("tool.select.")
            .and_then(|value| value.parse::<usize>().ok())
        {
            // Como na primeira escolha: fica pendente, e quem aplica é o Salvar.
            if let Some(dialog) = self.settings.dialog.as_mut() {
                dialog.pending_secondary = Some(index);
            }
            return true;
        }
        match command.0.as_str() {
            "toolchain.browse" => {
                self.commands.push(ApplicationCommand::BrowseToolchain);
                true
            }
            "tool.browse" => {
                self.commands.push(ApplicationCommand::BrowseSecondaryTool);
                true
            }
            "settings.save" => {
                self.save_settings();
                true
            }
            "settings.cancel" => {
                self.cancel_settings();
                true
            }
            _ => false,
        }
    }

    /// Aplica o que foi mexido e fecha.
    ///
    /// Só o que mudou sai daqui: sem escolha pendente, salvar não reaplica a toolchain
    /// que já estava valendo — reaplicar derrubaria o provider de linguagem e
    /// reindexaria a biblioteca padrão por nada.
    fn save_settings(&mut self) {
        if let Some(dialog) = self.settings.dialog.as_ref()
            && let Some(index) = dialog.pending_toolchain
            && dialog.original_toolchain != Some(index)
        {
            self.commands
                .push(ApplicationCommand::SelectToolchain(index));
        }
        if let Some(dialog) = self.settings.dialog.as_ref()
            && let Some(index) = dialog.pending_secondary
            && dialog.original_secondary != Some(index)
        {
            self.commands
                .push(ApplicationCommand::SelectSecondaryTool(index));
        }
        self.settings.modal.close();
        self.settings.dialog = None;
    }

    /// Descarta tudo o que foi mexido e fecha.
    fn cancel_settings(&mut self) {
        if let Some(dialog) = self.settings.dialog.take() {
            if let Some(original) = dialog.original_toolchain {
                self.settings.toolchain_combo.set_selected(original);
            }
            if let Some(original) = dialog.original_secondary {
                self.settings.secondary_combo.set_selected(original);
            }
            self.settings
                .debug_host
                .set_value(dialog.original_debug_host);
            self.settings
                .debug_port
                .set_value(dialog.original_debug_port);
        }
        self.settings.modal.close();
    }
}

struct SettingsDialogGeometry {
    sidebar: Rect,
    /// Primeira linha da navegação; as demais seguem por altura de linha.
    compiler_option: Rect,
    combo: Rect,
    secondary_combo: Rect,
    secondary_browse: Rect,
    browse: Rect,
    close: Rect,
    save: Rect,
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

/// Rótulo de uma entrada na lista da inspeção.
fn inspection_label(entry: &DebugVariableView) -> String {
    match entry.type_name.as_deref() {
        Some(type_name) => format!("{} = ({type_name}) {}", entry.name, entry.value),
        None => format!("{} = {}", entry.name, entry.value),
    }
}

/// Caminho do nó com aquela identidade, procurando em profundidade.
fn inspection_path_of(node: &InspectionNode, id: u64) -> Option<String> {
    if inspection_id(&node.path) == id {
        return Some(node.path.clone());
    }
    node.children
        .iter()
        .find_map(|child| inspection_path_of(child, id))
}

/// Identidade de um nó da árvore, derivada do caminho.
///
/// O caminho sobrevive à chegada de campos novos; um índice mudaria a cada
/// expansão, e a árvore perderia o que estava aberto.
fn inspection_id(path: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    path.hash(&mut hasher);
    hasher.finish()
}

/// Separa o texto do editor nas instruções que o compõem.
///
/// O `;` e a quebra de linha terminam uma instrução, mas **não dentro de aspas**:
/// `setNome("a; b")` é uma instrução só, e partir no ponto e vírgula do meio
/// entregaria duas metades sem sentido ao alvo.
fn inspection_statements(source: &str) -> Vec<String> {
    let mut statements = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    let mut escaped = false;
    for character in source.chars() {
        if quoted {
            current.push(character);
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == '"' {
                quoted = false;
            }
            continue;
        }
        match character {
            '"' => {
                quoted = true;
                current.push(character);
            }
            ';' | '\n' | '\r' => {
                if !current.trim().is_empty() {
                    statements.push(current.trim().to_owned());
                }
                current.clear();
            }
            _ => current.push(character),
        }
    }
    if !current.trim().is_empty() {
        statements.push(current.trim().to_owned());
    }
    statements
}

/// Converte um nó carregado em item da biblioteca.
///
/// Um valor expansível ainda sem campos recebe um filho de espera: é o que faz a
/// árvore desenhar o triângulo antes de o alvo ter respondido, e é onde o clique
/// de expansão acontece.
fn inspection_items(node: &InspectionNode) -> TreeItem {
    let children = if node.children.is_empty() && node.variable.expandable && !node.loaded {
        vec![TreeItem::new(
            inspection_id(&format!("{}.\u{2026}", node.path)),
            "carregando…",
            Vec::new(),
        )]
    } else {
        node.children.iter().map(inspection_items).collect()
    };
    TreeItem::new(
        inspection_id(&node.path),
        inspection_label(&node.variable),
        children,
    )
}

/// Os dois painéis da janela de inspeção e os botões.
struct InspectionGeometry {
    list: Rect,
    detail: Rect,
    /// Editor de expressões, na parte de baixo do painel direito.
    source: Rect,
    /// Linha da resposta da última execução, acima dos botões.
    message: Rect,
    run: Rect,
    close: Rect,
}

fn inspection_geometry(panel: Rect) -> InspectionGeometry {
    let top = panel.origin.y + 56.0;
    let height = (panel.size.height - 112.0).max(80.0);
    let list_width = (panel.size.width - 32.0) * INSPECTION_LIST_FRACTION;
    let list = Rect::new(panel.origin.x + 16.0, top, list_width, height);
    let right_x = list.origin.x + list.size.width + 16.0;
    let right_width = (panel.size.width - list_width - 48.0).max(80.0);
    // O detalhe fica em cima e o editor embaixo: o valor é o que se lê, o código
    // é o que se escreve.
    let detail_height = (height * INSPECTION_DETAIL_FRACTION).max(60.0);
    let detail = Rect::new(right_x, top, right_width, detail_height);
    let source = Rect::new(
        right_x,
        top + detail_height + 18.0,
        right_width,
        (height - detail_height - 18.0).max(60.0),
    );
    let close = Rect::new(
        panel.origin.x + panel.size.width - 104.0,
        panel.origin.y + panel.size.height - 48.0,
        88.0,
        34.0,
    );
    let run = Rect::new(close.origin.x - 108.0, close.origin.y, 98.0, 34.0);
    let message = Rect::new(
        panel.origin.x + 16.0,
        close.origin.y - 22.0,
        (panel.size.width - 32.0).max(80.0),
        18.0,
    );
    InspectionGeometry {
        list,
        detail,
        source,
        message,
        run,
        close,
    }
}

/// Encurta a mensagem para caber na largura disponível.
///
/// Sem isso ela sai pela borda da janela e o fim — que costuma ser a causa —
/// desaparece.
fn clipped_message(message: &str, width: f32) -> String {
    let limit = (width / INSPECTION_MESSAGE_CHAR_WIDTH).floor().max(8.0) as usize;
    if message.chars().count() <= limit {
        return message.to_owned();
    }
    let head: String = message.chars().take(limit.saturating_sub(1)).collect();
    format!("{head}…")
}

/// Onde cada peça da janela de criação fica dentro do painel.
struct NewItemGeometry {
    package: Rect,
    name: Rect,
    create: Rect,
    cancel: Rect,
}

/// Áreas da janela de renomear: campo, lista e os dois botões.
struct RenameGeometry {
    input: Rect,
    list: Rect,
    cancel: Rect,
    ok: Rect,
}

fn rename_geometry(panel: Rect) -> RenameGeometry {
    let largura = (panel.size.width - 48.0).max(120.0);
    let input = Rect::new(
        panel.origin.x + 24.0,
        panel.origin.y + 76.0,
        largura,
        34.0,
    );
    let ok = Rect::new(
        panel.origin.x + panel.size.width - 104.0,
        panel.origin.y + panel.size.height - 48.0,
        88.0,
        34.0,
    );
    let cancel = Rect::new(ok.origin.x - 98.0, ok.origin.y, 88.0, 34.0);
    let topo_lista = input.origin.y + input.size.height + 34.0;
    let list = Rect::new(
        input.origin.x,
        topo_lista,
        largura,
        (ok.origin.y - topo_lista - 16.0).max(40.0),
    );
    RenameGeometry {
        input,
        list,
        cancel,
        ok,
    }
}

/// Como um arquivo afetado aparece na lista.
///
/// O nome do arquivo, a pasta — dois pacotes podem ter arquivos homônimos — e
/// quantas ocorrências serão trocadas ali, que é o que dá a dimensão da mudança
/// antes de confirmá-la.
fn rename_reference_label(path: &Path, ocorrencias: usize) -> String {
    let nome = path
        .file_name()
        .and_then(|valor| valor.to_str())
        .unwrap_or_default();
    let pasta = path
        .parent()
        .and_then(|pai| pai.to_str())
        .filter(|pai| !pai.is_empty());
    match pasta {
        Some(pasta) => format!("{nome}  ({pasta})  —  {ocorrencias}"),
        None => format!("{nome}  —  {ocorrencias}"),
    }
}

fn new_item_geometry(panel: Rect) -> NewItemGeometry {
    let field_width = (panel.size.width - 48.0).max(120.0);
    let package = Rect::new(
        panel.origin.x + 24.0,
        panel.origin.y + 76.0,
        field_width,
        34.0,
    );
    let name = Rect::new(package.origin.x, package.origin.y + 64.0, field_width, 34.0);
    // Criar à direita, encostado na borda, como o Salvar das Configurações.
    let create = Rect::new(
        panel.origin.x + panel.size.width - 104.0,
        panel.origin.y + panel.size.height - 48.0,
        88.0,
        34.0,
    );
    let cancel = Rect::new(create.origin.x - 98.0, create.origin.y, 88.0, 34.0);
    NewItemGeometry {
        package,
        name,
        create,
        cancel,
    }
}

/// Área que a lista de páginas ocupa: as linhas, e não a barra inteira.
fn settings_pages_rect(geometry: &SettingsDialogGeometry, page_count: usize) -> Rect {
    Rect::new(
        geometry.compiler_option.origin.x,
        geometry.compiler_option.origin.y,
        geometry.compiler_option.size.width,
        SETTINGS_PAGE_ROW_HEIGHT * page_count as f32,
    )
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
    // O Maven vem logo abaixo do JDK, com a mesma largura: são a mesma escolha
    // feita duas vezes, e alinhá-las é o que deixa isso evidente.
    let secondary_combo = Rect::new(
        combo.origin.x,
        combo.origin.y + combo.size.height + 46.0,
        combo.size.width,
        combo.size.height,
    );
    let secondary_browse = Rect::new(
        secondary_combo.origin.x + secondary_combo.size.width + 10.0,
        secondary_combo.origin.y,
        browse.size.width,
        browse.size.height,
    );
    // Salvar à direita, encostado na borda, e Cancelar à esquerda dele: a ação
    // que confirma fica no canto que a leitura alcança por último.
    let save = Rect::new(
        dialog.origin.x + dialog.size.width - 104.0,
        dialog.origin.y + dialog.size.height - 48.0,
        88.0,
        34.0,
    );
    let close = Rect::new(save.origin.x - 98.0, save.origin.y, 88.0, save.size.height);
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
        combo,
        secondary_combo,
        secondary_browse,
        browse,
        close,
        save,
        debug_host,
        debug_port,
        debug_attach,
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
/// Converte todos os realces em uma única passagem pelo texto.
///
/// O snapshot fala em linha/coluna e o editor em caracteres absolutos. Manter
/// início e tamanho de cada linha transforma cada extremo de token em consulta
/// O(1); percorrer desde a primeira linha para cada token tornava a pintura
/// quadrática em classes grandes.
fn converted_syntax(text: &str, snapshot: &SyntaxSnapshot) -> Vec<(usize, usize, TokenKind)> {
    let mut starts = Vec::new();
    let mut lengths = Vec::new();
    let mut offset = 0;
    for line in text.split('\n') {
        starts.push(offset);
        let length = line.chars().count();
        lengths.push(length);
        offset += length + 1;
    }
    let position = |position: DomainTextPosition| {
        let line = (position.line as usize).min(starts.len().saturating_sub(1));
        starts.get(line).copied().unwrap_or_default()
            + (position.column as usize).min(lengths.get(line).copied().unwrap_or_default())
    };
    snapshot
        .highlights
        .iter()
        .map(|highlight| {
            (
                position(highlight.range.start),
                position(highlight.range.end),
                token_kind_for(highlight.kind),
            )
        })
        .collect()
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

/// O item do outline é um tipo e contém a posição — ou algum filho dele é.
///
/// A busca desce a árvore porque classes aninhadas existem, e estar dentro de
/// uma interna continua sendo estar dentro de um tipo.
fn encloses_type(item: &OutlineItem, position: DomainTextPosition) -> bool {
    let dentro = position_in_range(position.line as usize, position.column as usize, item.range);
    let tipo = matches!(
        item.kind,
        OutlineKind::Class | OutlineKind::Interface | OutlineKind::Enum | OutlineKind::Annotation
    );
    (dentro && tipo)
        || item
            .children
            .iter()
            .any(|child| encloses_type(child, position))
}

fn is_identifier_character(character: char) -> bool {
    character == '_' || character.is_alphanumeric()
}

/// Monta a lista da janela de geração.
///
/// **Esta tela é quem escolhe as células**: uma caixa de marcação e o nome do
/// campo. A biblioteca não decide nada disso.
fn generate_list(candidates: &[AccessorCandidate], checked: &[bool]) -> ComposedList {
    let linhas: Vec<ComposedRow> = candidates
        .iter()
        .enumerate()
        .map(|(index, candidate)| {
            let base = GENERATE_CELL_BASE + index as u64 * 2;
            ComposedRow::new(vec![
                ComposedCell::new(
                    Box::new(Checkbox::new(
                        WidgetId(base),
                        "",
                        checked.get(index).copied().unwrap_or_default(),
                    )),
                    CellWidth::Fixed(30.0),
                ),
                ComposedCell::new(
                    Box::new(Label::new(WidgetId(base + 1), candidate.field.clone())),
                    CellWidth::Fill,
                ),
            ])
        })
        .collect();
    ComposedList::new(GENERATE_LIST_ID, linhas).with_row_height(GENERATE_ROW_HEIGHT)
}

/// Deslocamento em bytes onde uma linha começa.
///
/// Além da última, devolve o fim do texto: inserir depois do fim é acrescentar,
/// e é o que se quer quando o tipo fecha na última linha.
fn offset_of_line(text: &str, line: usize) -> usize {
    let mut offset = 0;
    for (index, value) in text.split('\n').enumerate() {
        if index == line {
            return offset;
        }
        offset += value.len() + 1;
    }
    text.len()
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

fn count_outline(items: &[OutlineItem]) -> usize {
    items
        .iter()
        .map(|item| 1 + count_outline(&item.children))
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn java_source_roots() -> Vec<String> {
        vec!["java".to_owned()]
    }

    fn java_catalog() -> UiContributionCatalog {
        UiContributionCatalog {
            language_names: vec!["Java".to_owned()],
            source_root_names: java_source_roots(),
            new_item_templates: vec![
                NewItemTemplate {
                    id: NewItemTemplateId::new("java.package"),
                    title: "Novo pacote".to_owned(),
                    name_caption: "Classe (opcional)".to_owned(),
                    file_extension: None,
                    allows_empty_name: true,
                },
                NewItemTemplate {
                    id: NewItemTemplateId::new("java.class"),
                    title: "Nova classe".to_owned(),
                    name_caption: "Nome da classe".to_owned(),
                    file_extension: Some("java".to_owned()),
                    allows_empty_name: false,
                },
                NewItemTemplate {
                    id: NewItemTemplateId::new("java.interface"),
                    title: "Nova interface".to_owned(),
                    name_caption: "Nome da interface".to_owned(),
                    file_extension: Some("java".to_owned()),
                    allows_empty_name: false,
                },
            ],
            settings_sections: vec![SettingsSection {
                id: "java.compiler-vm".to_owned(),
                title: "Compilador e VM".to_owned(),
                field_caption: "JDK".to_owned(),
                browse_button_title: "Procurar...".to_owned(),
                secondary_caption: None,
            }],
            tasks: vec![TaskDescriptor {
                id: TaskId("java.run".to_owned()),
                title: "Executar".to_owned(),
                requires_active_document: true,
                show_in_toolbar: true,
            }],
        }
    }

    fn fake_catalog() -> UiContributionCatalog {
        UiContributionCatalog {
            language_names: vec!["Fake".to_owned()],
            source_root_names: vec!["src".to_owned()],
            new_item_templates: vec![NewItemTemplate {
                id: NewItemTemplateId::new("fake.module"),
                title: "Novo módulo fake".to_owned(),
                name_caption: "Nome do módulo".to_owned(),
                file_extension: Some("fake".to_owned()),
                allows_empty_name: false,
            }],
            settings_sections: vec![SettingsSection {
                id: "fake.runtime".to_owned(),
                title: "Runtime fake".to_owned(),
                field_caption: "Runtime".to_owned(),
                browse_button_title: "Localizar...".to_owned(),
                secondary_caption: None,
            }],
            tasks: vec![TaskDescriptor {
                id: TaskId("fake.run".to_owned()),
                title: "Executar fake".to_owned(),
                requires_active_document: false,
                show_in_toolbar: true,
            }],
        }
    }

    fn open_java_settings(shell: &mut IdeShell, items: Vec<String>, selected: usize) {
        shell.set_ui_catalog(java_catalog());
        shell.open_settings_dialog(items, selected);
    }

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

        shell.explorer.scroll_x = 20.0;
        assert!(
            (before - origin_of(&mut shell) - 20.0).abs() < 0.1,
            "a linha desliza com a rolagem horizontal"
        );
    }

    fn dir(path: &str, children: Vec<FileNode>) -> FileNode {
        FileNode {
            path: PathBuf::from(path),
            is_directory: true,
            children,
        }
    }

    fn file(path: &str) -> FileNode {
        FileNode {
            path: PathBuf::from(path),
            is_directory: false,
            children: Vec::new(),
        }
    }

    fn labels(items: &[TreeItem]) -> Vec<&str> {
        items.iter().map(|item| item.label.as_str()).collect()
    }

    /// Projeto Maven com a cadeia de pacote que a captura mostra.
    fn maven_project() -> FileNode {
        dir(
            "demo",
            vec![dir(
                "demo/src",
                vec![dir(
                    "demo/src/main",
                    vec![dir(
                        "demo/src/main/java",
                        vec![dir(
                            "demo/src/main/java/br",
                            vec![dir(
                                "demo/src/main/java/br/com",
                                vec![dir(
                                    "demo/src/main/java/br/com/exemplo",
                                    vec![dir(
                                        "demo/src/main/java/br/com/exemplo/endpoints",
                                        vec![
                                            dir(
                                                "demo/src/main/java/br/com/exemplo/endpoints/controller",
                                                Vec::new(),
                                            ),
                                            file(
                                                "demo/src/main/java/br/com/exemplo/endpoints/App.java",
                                            ),
                                        ],
                                    )],
                                )],
                            )],
                        )],
                    )],
                )],
            )],
        )
    }

    /// `br`, `com` e `exemplo` só existem porque o diretório espelha o pacote:
    /// viram uma linha só, e `src`, `main` e `java` continuam separados porque
    /// não são pacotes.
    #[test]
    fn explorer_joins_single_child_java_packages_into_one_row() {
        let items = explorer_items(&maven_project(), &java_source_roots());
        let src = &items[0];
        assert_eq!(labels(&items), vec!["src"]);
        let main = &src.children[0];
        assert_eq!(labels(&src.children), vec!["main"]);
        let java = &main.children[0];
        assert_eq!(labels(&main.children), vec!["java"]);
        assert_eq!(labels(&java.children), vec!["br.com.exemplo.endpoints"]);
        assert_eq!(
            labels(&java.children[0].children),
            vec!["controller", "App.java"]
        );
    }

    /// O nó comprimido responde pelo diretório final da cadeia — é assim que o
    /// clique continua resolvendo para um caminho que existe.
    #[test]
    fn a_joined_package_keeps_the_identity_of_the_deepest_directory() {
        let items = explorer_items(&maven_project(), &java_source_roots());
        let package = &items[0].children[0].children[0].children[0];
        assert_eq!(
            package.id,
            explorer_id(Path::new("demo/src/main/java/br/com/exemplo/endpoints"))
        );
    }

    /// Um arquivo ao lado do subdiretório interrompe a cadeia: `br` passa a ter
    /// conteúdo próprio e merece a linha dele.
    #[test]
    fn a_file_beside_the_subdirectory_stops_the_chain() {
        let tree = dir(
            "demo/src/main/java",
            vec![dir(
                "demo/src/main/java/br",
                vec![
                    dir("demo/src/main/java/br/com", Vec::new()),
                    file("demo/src/main/java/br/leiame.md"),
                ],
            )],
        );
        assert_eq!(
            labels(&explorer_items(&tree, &java_source_roots())),
            vec!["br"]
        );
    }

    /// Fora de uma raiz de fontes não há pacote, e juntar nomes com ponto diria
    /// algo que não é verdade sobre aquelas pastas.
    #[test]
    fn directories_outside_a_source_root_are_left_alone() {
        let tree = dir(
            "demo",
            vec![dir(
                "demo/docs",
                vec![dir("demo/docs/adr", vec![file("demo/docs/adr/0001.md")])],
            )],
        );
        let items = explorer_items(&tree, &java_source_roots());
        assert_eq!(labels(&items), vec!["docs"]);
        assert_eq!(labels(&items[0].children), vec!["adr"]);
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
        shell.editor_area.session.open_memory(
            "Main.java",
            "class Main {\n  void run() {\n    int total = 1;\n  }\n}",
        );
        (shell, path)
    }

    /// Tab escreve espaços até a próxima parada de tabulação. A partir da
    /// coluna 2 são dois espaços, não quatro — o texto alinha com a grade que o
    /// editor desenha.
    #[test]
    fn tab_indents_the_editor_to_the_next_stop() {
        let mut shell = test_shell();
        shell.editor_area.session.open_memory("Example.java", "ab");
        shell.context.focus = ShellFocus::Editor;
        shell.editor_area.pane.set_cursor(2);
        shell.key_down("Tab");
        assert_eq!(shell.active_text(), Some("ab  "));
        assert_eq!(shell.editor_area.pane.cursor(), 4);
    }

    /// Shift+Tab recolhe a margem da linha inteira, com o cursor no meio do
    /// código, e o cursor acompanha o deslocamento.
    #[test]
    fn shift_tab_unindents_the_current_line() {
        let mut shell = test_shell();
        shell
            .editor_area
            .session
            .open_memory("Example.java", "class A {\n    int valor;\n}");
        shell.context.focus = ShellFocus::Editor;
        shell.editor_area.pane.set_cursor(14);
        shell.key_down_with_modifiers(
            "Tab",
            Modifiers {
                shift: true,
                ..Modifiers::default()
            },
        );
        assert_eq!(shell.active_text(), Some("class A {\nint valor;\n}"));
        assert_eq!(shell.editor_area.pane.cursor(), 10);
    }

    fn entry_labels(entries: &[MenuEntry]) -> Vec<String> {
        entries
            .iter()
            .map(|entry| match entry {
                MenuEntry::Item(item) => item.label.clone(),
                MenuEntry::Submenu { label, .. } => label.clone(),
                MenuEntry::Separator => "—".to_owned(),
            })
            .collect()
    }

    /// Dentro da raiz de fontes o que se cria é pacote e tipo. A própria pasta
    /// `java` conta como dentro: é a raiz onde o primeiro pacote nasce.
    #[test]
    fn inside_the_java_source_root_the_menu_offers_packages_and_types() {
        for target in [
            "demo/src/main/java",
            "demo/src/main/java/br",
            "demo/src/test/java/br/com/exemplo",
        ] {
            assert_eq!(
                entry_labels(&explorer_menu_entries(
                    Path::new(target),
                    &java_source_roots(),
                    &java_catalog().new_item_templates,
                    false,
                )),
                vec!["Novo pacote", "—", "Nova classe", "Nova interface"],
                "alvo {target}"
            );
        }
    }

    /// Fora dela não há pacote nem classe: resta a pasta.
    #[test]
    fn outside_the_source_root_the_menu_offers_a_folder() {
        for target in ["demo", "demo/docs", "demo/src/main/resources"] {
            assert_eq!(
                entry_labels(&explorer_menu_entries(
                    Path::new(target),
                    &java_source_roots(),
                    &java_catalog().new_item_templates,
                    false,
                )),
                vec!["Nova pasta"],
                "alvo {target}"
            );
        }
    }

    /// O clique secundário abre o menu sobre a linha apontada, e a escolha
    /// relata o diretório onde a ação aconteceria.
    #[test]
    fn the_secondary_click_on_the_explorer_opens_the_menu_for_that_row() {
        let mut shell = IdeShell::from_tree(maven_project());
        let size = Size::new(1280.0, 800.0);
        let row = Point::new(80.0, EXPLORER_TOP + 2.0);
        shell.secondary_pointer_down(row, size);
        assert!(shell.context_menu_open());
        assert_eq!(
            shell.explorer.context_menu_target,
            Some(PathBuf::from("demo/src"))
        );
        assert_eq!(
            entry_labels(shell.explorer.context_menu.entries()),
            vec!["Nova pasta"]
        );
    }

    /// Clicando em um arquivo, o alvo é a pasta dele: criar dentro de um
    /// arquivo não quer dizer nada.
    #[test]
    fn a_file_hands_the_menu_over_to_its_directory() {
        let tree = dir(
            "demo",
            vec![dir(
                "demo/src/main/java",
                vec![file("demo/src/main/java/App.java")],
            )],
        );
        let mut shell = IdeShell::from_tree(tree);
        shell.set_ui_catalog(java_catalog());
        let size = Size::new(1280.0, 800.0);
        shell
            .explorer
            .expanded
            .insert(PathBuf::from("demo/src/main/java"));
        shell.sync_explorer_tree();
        shell.secondary_pointer_down(
            Point::new(80.0, EXPLORER_TOP + EXPLORER_ROW_HEIGHT + 2.0),
            size,
        );
        assert_eq!(
            shell.explorer.context_menu_target,
            Some(PathBuf::from("demo/src/main/java"))
        );
        // A criação é na pasta do arquivo; renomear é do arquivo clicado, e por
        // isso as duas coisas convivem no mesmo menu.
        assert_eq!(
            entry_labels(shell.explorer.context_menu.entries()),
            vec![
                "Novo pacote",
                "—",
                "Nova classe",
                "Nova interface",
                "—",
                "Renomear"
            ]
        );
        assert_eq!(
            shell.explorer.context_menu_file,
            Some(PathBuf::from("demo/src/main/java/App.java")),
            "renomear precisa do arquivo, e não da pasta"
        );
    }

    /// Esc dispensa o menu antes de qualquer outra coisa que Esc faria.
    #[test]
    fn escape_dismisses_the_context_menu_first() {
        let mut shell = IdeShell::from_tree(maven_project());
        let size = Size::new(1280.0, 800.0);
        shell.context.focus = ShellFocus::Search;
        shell.editor_area.search_query = "consulta".to_owned();
        shell.secondary_pointer_down(Point::new(80.0, EXPLORER_TOP + 2.0), size);
        shell.escape();
        assert!(!shell.context_menu_open());
        assert_eq!(shell.editor_area.search_query, "consulta");
    }

    /// A calha mostra a diferença entre pedido e confirmado, e a linha parada
    /// é destacada inteira.
    #[test]
    fn the_gutter_shows_pending_and_confirmed_breakpoints_and_the_stopped_line() {
        let mut shell = test_shell();
        shell
            .editor_area
            .session
            .open_memory("A.java", "um\ndois\ntres\nquatro");
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
        assert_eq!(
            circles(&mut shell),
            (0, 1),
            "sem sessão, o ponto é pendente"
        );

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
        shell.editor_area.pane.set_cursor(20); // segunda linha
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
                expandable: false,
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
    fn contribution_catalog_generates_templates_settings_and_task_button() {
        let mut shell = test_shell();
        shell.set_ui_catalog(fake_catalog());
        assert_eq!(
            entry_labels(&explorer_menu_entries(
                Path::new("workspace/src"),
                &shell.catalog.source_root_names,
                &shell.catalog.new_item_templates,
                false,
            )),
            vec!["Novo módulo fake"]
        );
        open_java_settings(&mut shell, vec!["Fake SDK".to_owned()], 0);
        shell.set_ui_catalog(fake_catalog());
        let texts = shell
            .paint(Size::new(1_000.0, 700.0))
            .into_iter()
            .filter_map(|command| match command {
                PaintCommand::DrawText(text) => Some(text.text),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert!(texts.iter().any(|text| text == "Runtime fake"));
        assert!(texts.iter().any(|text| text == "Runtime"));
        shell.escape();

        let size = Size::new(1_000.0, 700.0);
        let run = action_button_rects(size)[1];
        shell.pointer_down(Point::new(run.origin.x + 2.0, run.origin.y + 2.0), size);
        assert!(
            shell
                .drain_application_commands()
                .contains(&ApplicationCommand::ExecuteTask(TaskId(
                    "fake.run".to_owned()
                )))
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
        assert_eq!(shell.settings_page(), SettingsPage::Contribution(0));

        // Menu `Depurar` → `Conectar...`.
        shell.pointer_down(Point::new(280.0, 10.0), size);
        shell.pointer_down(Point::new(280.0, TITLE_HEIGHT + 10.0), size);
        assert!(shell.take_open_settings_request());
        assert_eq!(shell.settings_page(), SettingsPage::Debug);

        open_java_settings(&mut shell, vec!["JDK 17".to_owned()], 0);
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
        shell.set_settings_page(SettingsPage::Contribution(0));
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
        open_java_settings(&mut shell, vec!["JDK 8".to_owned()], 0);
        shell.settings.modal.layout(
            &LayoutContext::default(),
            Rect::new(0.0, 0.0, size.width, size.height),
        );
        let geometry = settings_dialog_geometry(shell.settings.modal.panel_bounds());

        // Segunda linha da navegação é a página de Depuração.
        shell.pointer_down(
            Point::new(
                geometry.compiler_option.origin.x + 20.0,
                geometry.compiler_option.origin.y + SETTINGS_PAGE_ROW_HEIGHT + 10.0,
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
            .editor_area
            .session
            .open_memory("Example.java", "public class Example {}");
        shell.set_syntax_snapshot(ide_domain::SyntaxSnapshot {
            document_id,
            version: 0,
            outline: vec![ide_domain::OutlineItem {
                name: "Example".to_owned(),
                kind: ide_domain::OutlineKind::Class,
                range: ide_domain::TextRange::default(),
                name_range: ide_domain::TextRange::default(),
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
        let cached = shell
            .editor_area
            .syntax_spans
            .get(&document_id)
            .unwrap_or_else(|| panic!("o realce convertido precisa ser cacheado"));
        assert_eq!(cached.spans, vec![(0, 6, TokenKind::Keyword)]);
        let cached_pointer = cached.spans.as_ptr();
        let colors = Theme::default().colors;
        assert!(shell.paint(Size::new(1280.0, 800.0)).iter().any(|command| {
            matches!(
                command,
                PaintCommand::DrawText(text)
                    if text.text == "public" && text.color == colors.syntax_keyword
            )
        }));
        let _ = shell.paint(Size::new(1280.0, 800.0));
        assert_eq!(
            shell.editor_area.syntax_spans[&document_id].spans.as_ptr(),
            cached_pointer,
            "quadros seguintes devem reutilizar o realce convertido"
        );
    }

    #[test]
    fn completion_popup_can_apply_selected_item() {
        let mut shell = test_shell();
        shell.editor_area.session.open_memory("Example.java", "Exa");
        shell.context.focus = ShellFocus::Editor;
        shell.editor_area.pane.set_cursor(3);
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
        let document_id = shell
            .editor_area
            .session
            .open_memory("A.java", "void metodo() { int x = y; }");
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
        let document_id = shell
            .editor_area
            .session
            .open_memory("Example.java", "class Example {}");
        shell.set_syntax_snapshot(ide_domain::SyntaxSnapshot {
            document_id,
            version: 0,
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
        let (_, content, viewport, _) =
            shell.scrollbar_range(ScrollTarget::ExplorerHorizontal, size);
        assert!(content > viewport);
        shell.pointer_down(
            Point::new(
                track.origin.x + track.size.width - 1.0,
                track.origin.y + 5.0,
            ),
            size,
        );
        assert!(shell.explorer.scroll_x > 0.0);
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
        let first = shell.editor_area.session.open_memory("first.rs", "one");
        let second = shell.editor_area.session.open_memory("second.rs", "two");
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
    fn active_file_expands_selects_and_scrolls_the_explorer() {
        static NEXT_PROJECT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let sequence = NEXT_PROJECT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "er-ide-explorer-active-{}-{sequence}",
            std::process::id()
        ));
        let package = root
            .join("src")
            .join("main")
            .join("java")
            .join("br")
            .join("com")
            .join("exemplo")
            .join("controller");
        assert!(std::fs::create_dir_all(&package).is_ok());
        for index in 0..12 {
            assert!(std::fs::write(root.join(format!("Anterior{index:02}.txt")), "x").is_ok());
        }
        let first = package.join("PrimeiroController.java");
        let second = package.join("SegundoController.java");
        assert!(std::fs::write(&first, "class PrimeiroController {}").is_ok());
        assert!(std::fs::write(&second, "class SegundoController {}").is_ok());

        let mut shell = match IdeShell::open(&root) {
            Ok(shell) => shell,
            Err(error) => panic!("projeto não abriu: {error}"),
        };
        assert!(shell.open_file(&first).is_ok());
        assert!(shell.open_file(&second).is_ok());
        assert_eq!(
            shell.explorer.tree.selected(),
            Some(explorer_id(&second)),
            "a última aba restaurada deve nascer selecionada no Explorer"
        );
        for ancestor in second.ancestors().skip(1).take_while(|path| *path != root) {
            assert!(
                shell.explorer.expanded.contains(ancestor),
                "{} deveria estar expandido",
                ancestor.display()
            );
        }
        assert!(
            shell.explorer.scroll_line > 0,
            "o arquivo ativo precisa ser revelado mesmo abaixo do primeiro viewport"
        );

        let editor_x = ACTIVITY_WIDTH + SIDEBAR_WIDTH;
        shell.pointer_down(
            Point::new(editor_x + 10.0, TITLE_HEIGHT + 10.0),
            Size::new(1280.0, 800.0),
        );
        assert_eq!(shell.active_document_path(), Some(first.clone()));
        assert_eq!(
            shell.explorer.tree.selected(),
            Some(explorer_id(&first)),
            "trocar de aba também precisa trocar a seleção da árvore"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn tab_close_button_removes_only_the_clicked_document() {
        let mut shell = test_shell();
        let first = shell.editor_area.session.open_memory("first.rs", "one");
        shell.editor_area.session.open_memory("second.rs", "two");
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
        shell.editor_area.session.open_memory("first.rs", "one");
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

        shell.context.focus = ShellFocus::Editor;
        shell.edit_active("x");
        assert!(marks(&mut shell), "documento alterado é marcado");
    }

    #[test]
    fn long_tab_titles_are_clipped_and_ellipsized_before_close_button() {
        let mut shell = test_shell();
        shell
            .editor_area
            .session
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
        assert!(
            rendered.iter().any(|command| {
                matches!(command, PaintCommand::PushClip(rect) if *rect == tabs)
            })
        );
    }

    /// O divisor é desenhado no lugar certo desde o primeiro quadro, antes de
    /// qualquer evento de ponteiro, e se destaca quando o ponteiro se aproxima.
    #[test]
    fn the_sidebar_divider_is_painted_in_place_and_highlights_under_the_pointer() {
        let mut shell = test_shell();
        let size = Size::new(1280.0, 800.0);
        let divider_color = |shell: &mut IdeShell| {
            let x = ACTIVITY_WIDTH + shell.sidebar_width(size);
            shell.paint(size).iter().find_map(|command| match command {
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
            5.0,
            size,
        );
        assert_eq!(shell.explorer.scroll_line, 5);
        shell.pointer_down(
            Point::new(
                track.origin.x + 5.0,
                track.origin.y + track.size.height - 1.0,
            ),
            size,
        );
        assert!(shell.explorer.scroll_line > 5);
    }

    #[test]
    fn editor_wheel_scrolls_and_terminal_profile_is_selectable() {
        let mut shell = test_shell();
        shell.editor_area.session.open_memory(
            "long.rs",
            (0..100)
                .map(|line| format!("line {line}"))
                .collect::<Vec<_>>()
                .join("\n"),
        );
        let size = Size::new(1280.0, 800.0);
        let editor_x = ACTIVITY_WIDTH + SIDEBAR_WIDTH;
        shell.scroll(Point::new(editor_x + 100.0, 200.0), 8.0, size);
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
            .join(
                "
",
            );
        shell.editor_area.session.open_memory("longo.rs", &text);
        let size = Size::new(1280.0, 800.0);
        let track = shell.editor_scrollbar_rect(size);
        let visible = shell.editor_visible_lines(size);

        shell.pointer_down(
            Point::new(track.origin.x + 5.0, track.origin.y + track.size.height),
            size,
        );
        assert_eq!(shell.editor_scroll_line(), 200 - visible);

        shell.pointer_move(Point::new(track.origin.x + 5.0, track.origin.y), size);
        assert_eq!(shell.editor_scroll_line(), 0);

        shell.pointer_up();
        shell.pointer_move(
            Point::new(track.origin.x + 5.0, track.origin.y + track.size.height),
            size,
        );
        assert_eq!(shell.editor_scroll_line(), 0, "soltar encerra o arraste");
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
        let active = shell.terminal.active;
        let bottom = shell.terminal.tabs[active].scroll_line;
        assert!(bottom > 0);

        let content_point = Point::new(editor_x + 100.0, shell.geometry(size).editor_bottom + 90.0);
        shell.scroll(content_point, -5.0, size);
        assert!(shell.terminal.tabs[active].scroll_line < bottom);

        let track = shell.terminal_scrollbar_rect(size);
        shell.pointer_down(Point::new(track.origin.x + 5.0, track.origin.y + 1.0), size);
        assert_eq!(shell.terminal.tabs[active].scroll_line, 0);
        shell.pointer_move(
            Point::new(track.origin.x + 5.0, track.origin.y + track.size.height),
            size,
        );
        assert!(shell.terminal.tabs[active].scroll_line > 0);
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
        let document_id = shell
            .editor_area
            .session
            .open_memory("main.rs", "fn target() {}\n");
        let size = Size::new(1280.0, 800.0);
        let editor_x = ACTIVITY_WIDTH + SIDEBAR_WIDTH;
        let target_x = editor_x + EDITOR_GUTTER + 5.0 * EDITOR_CHAR_WIDTH;
        shell.pointer_down_with_modifiers(
            Point::new(target_x, TITLE_HEIGHT + TAB_HEIGHT + 15.0),
            size,
            true,
            false,
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
            .join(
                "
",
            );
        shell.editor_area.session.open_memory("Longo.java", &texto);
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
            .join(
                "
",
            );
        shell.editor_area.session.open_memory("Longo.java", &texto);
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
        assert_eq!(
            destacadas(&mut shell),
            0,
            "mover o cursor encerra o destaque"
        );
    }

    #[test]
    fn open_location_opens_file_and_positions_cursor() {
        let mut shell = test_shell();
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
        assert!(shell.open_location(&path, 1, 3).is_ok());
        let position = line_column(
            shell.active_text().unwrap_or_default(),
            shell.editor_area.pane.cursor(),
        );
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
        let source = include_str!("ide_shell.rs");
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

        open_java_settings(&mut shell, vec!["JDK 8".to_owned(), "JDK 17".to_owned()], 0);
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
        open_java_settings(&mut shell, vec!["JDK 8".to_owned(), "JDK 17".to_owned()], 0);
        shell.settings.modal.layout(
            &LayoutContext::default(),
            Rect::new(0.0, 0.0, size.width, size.height),
        );
        let geometry = settings_dialog_geometry(shell.settings.modal.panel_bounds());
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
        // A escolha fica pendente: a janela é uma transação.
        assert_eq!(shell.take_settings_jdk_result(), None);

        shell.pointer_down(
            Point::new(
                geometry.browse.origin.x + 10.0,
                geometry.browse.origin.y + 10.0,
            ),
            size,
        );
        assert!(shell.take_browse_jdk_request());
    }

    /// Salvar aplica o que foi escolhido e fecha.
    #[test]
    fn saving_the_settings_applies_the_chosen_jdk() {
        let mut shell = test_shell();
        let size = Size::new(1_000.0, 700.0);
        open_java_settings(&mut shell, vec!["JDK 8".to_owned(), "JDK 17".to_owned()], 0);
        let geometry = open_settings_geometry(&mut shell, size);
        choose_second_jdk(&mut shell, &geometry, size);
        shell.pointer_down(
            Point::new(geometry.save.origin.x + 10.0, geometry.save.origin.y + 10.0),
            size,
        );
        assert_eq!(shell.take_settings_jdk_result(), Some(1));
        assert!(!shell.settings_dialog_open());
    }

    /// Cancelar descarta o que foi mexido, e o combo volta ao que estava.
    #[test]
    fn cancelling_the_settings_discards_every_change() {
        let mut shell = test_shell();
        let size = Size::new(1_000.0, 700.0);
        open_java_settings(&mut shell, vec!["JDK 8".to_owned(), "JDK 17".to_owned()], 0);
        let geometry = open_settings_geometry(&mut shell, size);
        choose_second_jdk(&mut shell, &geometry, size);
        shell.pointer_down(
            Point::new(
                geometry.close.origin.x + 10.0,
                geometry.close.origin.y + 10.0,
            ),
            size,
        );
        assert_eq!(shell.take_settings_jdk_result(), None);
        assert!(!shell.settings_dialog_open());
        assert_eq!(shell.settings.toolchain_combo.selected_index(), 0);
    }

    /// Projeto Maven com um pacote já criado, para o menu agir sobre ele.
    fn shell_with_package() -> IdeShell {
        let mut shell = IdeShell::from_tree(dir(
            "demo",
            vec![dir(
                "demo/src/main/java",
                vec![dir(
                    "demo/src/main/java/br",
                    vec![dir("demo/src/main/java/br/com", Vec::new())],
                )],
            )],
        ));
        shell.set_ui_catalog(java_catalog());
        shell
    }

    /// O menu abre a janela com o pacote do alvo já preenchido.
    ///
    /// Quem clicou com o botão direito sobre um pacote não deveria ter que
    /// digitar de novo onde está.
    #[test]
    fn the_new_item_dialog_opens_with_the_clicked_package() {
        let mut shell = shell_with_package();
        shell.explorer.context_menu_target = Some(PathBuf::from("demo/src/main/java/br/com"));
        shell.run_explorer_command("explorer.new.java.class");
        assert!(shell.new_item_dialog_open());
        assert_eq!(shell.search.new_item_package.value(), "br.com");
        assert_eq!(shell.search.new_item_name.value(), "");
    }

    /// A mesma janela serve as três ações, mudando só o título e a legenda.
    #[test]
    fn the_three_menu_actions_share_one_window() {
        let mut shell = shell_with_package();
        shell.explorer.context_menu_target = Some(PathBuf::from("demo/src/main/java/br/com"));
        for (command, title) in [
            ("explorer.new.java.package", "Novo pacote"),
            ("explorer.new.java.class", "Nova classe"),
            ("explorer.new.java.interface", "Nova interface"),
        ] {
            shell.run_explorer_command(command);
            let actual_title = shell
                .search
                .new_item_dialog
                .as_ref()
                .map(|dialog| dialog.template.title.as_str());
            assert_eq!(actual_title, Some(title));
            assert_eq!(shell.search.new_item_package.value(), "br.com");
        }
    }

    /// Enter com só o pacote pede o pacote; o nome fica vazio.
    #[test]
    fn enter_with_only_the_package_asks_for_the_package() {
        let mut shell = shell_with_package();
        shell.explorer.context_menu_target = Some(PathBuf::from("demo/src/main/java/br/com"));
        shell.run_explorer_command("explorer.new.java.package");
        // O foco começa no pacote, com o cursor no fim do que veio preenchido.
        shell.text_input(".exemplo");
        shell.key_down("Enter");
        let request = shell.take_new_item_request();
        assert_eq!(
            request,
            Some(NewItemRequest {
                template_id: NewItemTemplateId::new("java.package"),
                package: "br.com.exemplo".to_owned(),
                name: String::new(),
                source_root: PathBuf::from("demo/src/main/java"),
            })
        );
    }

    /// Prepara um shell com arquivo aberto e foco no editor.
    fn shell_editing(text: &str) -> IdeShell {
        let mut shell = test_shell();
        shell.editor_area.session.open_memory("Pedido.java", text);
        shell.context.focus = ShellFocus::Editor;
        shell.editor_area.pane.set_cursor(0);
        shell
    }

    fn accessor_plan_para_teste() -> AccessorPlan {
        let candidato = |campo: &str, fonte: Option<&str>| AccessorCandidate {
            field: campo.to_owned(),
            source: fonte.map(str::to_owned),
        };
        AccessorPlan {
            candidates: vec![
                candidato("id", Some("\n    public Long getId() {\n        return id;\n    }\n")),
                // Já tem getter: não deve nem aparecer na janela.
                candidato("nome", None),
                candidato(
                    "ativo",
                    Some("\n    public boolean isAtivo() {\n        return ativo;\n    }\n"),
                ),
            ],
            insert_at: DomainTextPosition { line: 2, column: 0 },
        }
    }

    /// A janela lista só o que falta, e `OK` gera o que foi marcado.
    #[test]
    fn the_generate_window_lists_what_is_missing_and_writes_what_was_checked() {
        let mut shell = shell_editing("class Matricula {\n    private Long id;\n}\n");
        let size = Size::new(1280.0, 800.0);
        shell.show_accessor_plan(AccessorKind::Getter, accessor_plan_para_teste());
        assert!(shell.generate_open());
        assert_eq!(
            shell.generate_fields(),
            vec!["id", "ativo"],
            "o campo que já tem getter não é oferecido"
        );

        // Os nomes aparecem na janela.
        let texts = painted_texts(&mut shell, size);
        assert!(texts.iter().any(|text| text == "id"), "{texts:?}");
        assert!(texts.iter().any(|text| text == "All"), "{texts:?}");
        assert!(texts.iter().any(|text| text == "OK"), "{texts:?}");

        // Marcar a segunda linha e confirmar gera só ela.
        let (lista, _, ok) = shell.generate_geometry(size);
        shell.pointer_down(
            Point::new(lista.origin.x + 40.0, lista.origin.y + GENERATE_ROW_HEIGHT + 4.0),
            size,
        );
        shell.pointer_down(
            Point::new(ok.origin.x + 10.0, ok.origin.y + 10.0),
            size,
        );
        assert!(!shell.generate_open(), "confirmar fecha a janela");
        let texto = shell.active_text().unwrap_or_default();
        assert!(texto.contains("isAtivo"), "o marcado foi gerado: {texto}");
        assert!(!texto.contains("getId"), "o não marcado ficou de fora");
    }

    /// `All` gera todos, esteja marcado ou não.
    #[test]
    fn the_all_button_ignores_the_checkboxes() {
        let mut shell = shell_editing("class Matricula {\n    private Long id;\n}\n");
        let size = Size::new(1280.0, 800.0);
        shell.show_accessor_plan(AccessorKind::Getter, accessor_plan_para_teste());
        let (_, todos, _) = shell.generate_geometry(size);
        shell.pointer_down(
            Point::new(todos.origin.x + 10.0, todos.origin.y + 10.0),
            size,
        );
        let texto = shell.active_text().unwrap_or_default();
        assert!(texto.contains("getId"), "{texto}");
        assert!(texto.contains("isAtivo"), "{texto}");
        assert!(!shell.generate_open());
    }

    /// A lista da janela sobrevive aos quadros: roda e clique funcionam.
    ///
    /// Recriá-la a cada pintura jogava fora a rolagem e a deixava sem receber
    /// evento nenhum — a barra não se movia e o clique não chegava.
    #[test]
    fn the_generate_list_keeps_its_scroll_between_frames() {
        let mut shell = shell_editing("class Muitos {}\n");
        let size = Size::new(1280.0, 800.0);
        let candidatos: Vec<AccessorCandidate> = (0..40)
            .map(|index| AccessorCandidate {
                field: format!("campo{index}"),
                source: Some(format!("\n    public int getCampo{index}() {{ return 0; }}\n")),
            })
            .collect();
        shell.show_accessor_plan(AccessorKind::Getter, AccessorPlan {
            candidates: candidatos,
            insert_at: DomainTextPosition { line: 1, column: 0 },
        });
        let _ = shell.paint(size);

        let rolagem = |shell: &IdeShell| {
            shell
                .editor_area
                .generate
                .as_ref()
                .map_or(0.0, |state| state.list.scroll_offset())
        };
        assert_eq!(rolagem(&shell), 0.0);

        // A roda rola a lista da janela.
        let (lista, ..) = shell.generate_geometry(size);
        shell.scroll(
            Point::new(lista.origin.x + 40.0, lista.origin.y + 40.0),
            5.0,
            size,
        );
        let apos_roda = rolagem(&shell);
        assert!(apos_roda > 0.0, "a roda precisa mover a lista");

        // Pintar de novo não desfaz a rolagem.
        let _ = shell.paint(size);
        assert_eq!(rolagem(&shell), apos_roda, "a lista sobrevive ao quadro");

        // E o clique numa linha visível continua marcando.
        shell.pointer_down(
            Point::new(lista.origin.x + 40.0, lista.origin.y + 4.0),
            size,
        );
        let marcados = shell
            .editor_area
            .generate
            .as_ref()
            .map_or(0, |state| state.checked.iter().filter(|item| **item).count());
        assert_eq!(marcados, 1, "clicar numa linha marca uma");
    }

    /// O `Shift` das setas chega ao editor pela shell.
    ///
    /// O defeito estava no despacho: as setas eram enviadas com modificadores
    /// vazios, e o editor — que sempre soube estender — nunca via o `Shift`.
    #[test]
    fn shift_arrows_reach_the_editor_through_the_shell() {
        let mut shell = shell_editing("primeiro\nsegundo");
        shell.editor_area.pane.set_cursor(0);
        let com_shift = Modifiers {
            shift: true,
            ..Modifiers::default()
        };
        shell.key_down_with_modifiers("ArrowRight", com_shift);
        shell.key_down_with_modifiers("ArrowRight", com_shift);
        assert_eq!(
            shell.editor_area.pane.selection_range(),
            Some(0..2),
            "as setas com Shift precisam marcar"
        );
        shell.key_down_with_modifiers("ArrowDown", com_shift);
        assert!(
            shell
                .editor_area
                .pane
                .selection_range()
                .is_some_and(|range| range.end > 2),
            "a seleção cresce pela mesma âncora"
        );
        shell.key_down("ArrowRight");
        assert_eq!(shell.editor_area.pane.selection_range(), None);
    }

    /// O `Ctrl` das setas laterais também precisa atravessar o shell.
    ///
    /// É a mesma costura do `Shift`: o painel sabe saltar de palavra em palavra,
    /// mas de nada adianta se o modificador se perder no caminho.
    #[test]
    fn control_arrows_reach_the_editor_through_the_shell() {
        let mut shell = shell_editing("int total = valor;");
        shell.editor_area.pane.set_cursor(0);
        let com_control = Modifiers {
            control: true,
            ..Modifiers::default()
        };
        shell.key_down_with_modifiers("ArrowRight", com_control);
        assert_eq!(shell.editor_area.pane.cursor(), 3, "parou no fim de `int`");
        shell.key_down_with_modifiers("ArrowRight", com_control);
        assert_eq!(shell.editor_area.pane.cursor(), 9, "e no fim de `total`");
        shell.key_down_with_modifiers("ArrowLeft", com_control);
        assert_eq!(shell.editor_area.pane.cursor(), 4, "voltou ao começo dele");
    }

    /// Gerar muda a revisão do documento, que é o que pede realce novo.
    ///
    /// Sem isso o código gerado aparecia sem cor até a primeira tecla: o realce
    /// é invalidado pela revisão, e ninguém pedia um novo depois do clique.
    #[test]
    fn generating_changes_the_revision_so_the_highlight_is_asked_again() {
        let mut shell = shell_editing("class Matricula {\n}\n");
        let size = Size::new(1280.0, 800.0);
        let antes = shell.active_revision();
        shell.show_accessor_plan(AccessorKind::Getter, AccessorPlan {
            candidates: vec![AccessorCandidate {
                field: "id".to_owned(),
                source: Some("\n    public Long getId() { return id; }\n".to_owned()),
            }],
            insert_at: DomainTextPosition { line: 1, column: 0 },
        });
        let (_, todos, _) = shell.generate_geometry(size);
        shell.pointer_down(
            Point::new(todos.origin.x + 10.0, todos.origin.y + 10.0),
            size,
        );
        assert!(
            shell.active_revision() > antes,
            "escrever no documento precisa avançar a revisão"
        );
    }

    /// Os três itens do menu usam a mesma janela.
    ///
    /// Getter, setter e o par diferem no que a linguagem escreve, não na tela:
    /// duplicar a janela daria duas cópias que divergiriam na primeira correção.
    #[test]
    fn the_three_generate_options_share_one_window() {
        for kind in [
            AccessorKind::Getter,
            AccessorKind::Setter,
            AccessorKind::Both,
        ] {
            let mut shell = shell_editing("class Matricula {\n}\n");
            let size = Size::new(1280.0, 800.0);
            let fonte = match kind {
                AccessorKind::Getter => "\n    public Long getId() { return id; }\n",
                AccessorKind::Setter => "\n    public void setId(Long id) {}\n",
                AccessorKind::Both => {
                    "\n    public Long getId() { return id; }\n    public void setId(Long id) {}\n"
                }
                // O construtor não gera texto por campo, e por isso tem teste
                // próprio: aqui ele não teria o que comparar.
                AccessorKind::Constructor => continue,
            };
            shell.show_accessor_plan(kind, AccessorPlan {
                candidates: vec![AccessorCandidate {
                    field: "id".to_owned(),
                    source: Some(fonte.to_owned()),
                }],
                insert_at: DomainTextPosition { line: 1, column: 0 },
            });
            assert!(shell.generate_open(), "a mesma janela abre para {kind:?}");
            assert_eq!(shell.generate_fields(), vec!["id"]);

            let (_, todos, _) = shell.generate_geometry(size);
            shell.pointer_down(
                Point::new(todos.origin.x + 10.0, todos.origin.y + 10.0),
                size,
            );
            let texto = shell.active_text().unwrap_or_default();
            match kind {
                AccessorKind::Getter => assert!(texto.contains("getId"), "{texto}"),
                AccessorKind::Setter => assert!(texto.contains("setId"), "{texto}"),
                AccessorKind::Both => {
                    assert!(texto.contains("getId"), "o par gera os dois: {texto}");
                    assert!(texto.contains("setId"), "o par gera os dois: {texto}");
                }
                AccessorKind::Constructor => unreachable!("descartado acima"),
            }
        }
    }

    /// Clicar e arrastar a barra da janela rola a lista.
    ///
    /// O widget já sabia arrastar; o que faltava era a janela entregar o
    /// **movimento** e a **soltura** a ele — só o clique chegava, e o indicador
    /// era pego e nunca andava.
    #[test]
    fn dragging_the_rename_scrollbar_scrolls_the_list() {
        let mut shell = shell_editing("public class Pedido {
}
");
        let size = Size::new(1280.0, 800.0);
        let local = |arquivo: &str, linha: u32| Location {
            path: PathBuf::from(arquivo),
            range: DomainTextRange {
                start: DomainTextPosition { line: linha, column: 0 },
                end: DomainTextPosition { line: linha, column: 6 },
            },
        };
        shell.show_rename(
            PathBuf::from("src/Pedido.java"),
            (0..60)
                .map(|indice| local(&format!("src/Arquivo{indice}.java"), indice))
                .collect(),
        );
        let _ = shell.paint(size);

        let geometry = {
            shell.editor_area.rename_modal.layout(
                &shell.layout_context(),
                Rect::new(0.0, 0.0, size.width, size.height),
            );
            rename_geometry(shell.editor_area.rename_modal.panel_bounds())
        };
        let trilha_x = geometry.list.origin.x + geometry.list.size.width - 5.0;
        let topo = geometry.list.origin.y + 4.0;

        shell.pointer_down(Point::new(trilha_x, topo), size);
        shell.pointer_move(Point::new(trilha_x, topo + 120.0), size);
        let rolou = shell
            .editor_area
            .rename
            .as_ref()
            .map(|state| state.list.scroll_offset())
            .unwrap_or_default();
        assert!(rolou > 0.0, "arrastar a barra precisa rolar a lista");

        shell.pointer_up();
        // Solto o gesto, mover o ponteiro não arrasta mais nada.
        shell.pointer_move(Point::new(trilha_x, topo), size);
        let depois = shell
            .editor_area
            .rename
            .as_ref()
            .map(|state| state.list.scroll_offset())
            .unwrap_or_default();
        assert_eq!(depois, rolou, "sem botão pressionado a barra fica onde está");
    }

    /// Com a janela de renomear aberta, a roda é dela — não do editor atrás.
    ///
    /// Rolar o que está coberto é mexer no que não se vê: o usuário achava que
    /// estava percorrendo a lista e estava movendo o arquivo por baixo dela.
    #[test]
    fn the_wheel_belongs_to_the_rename_window_and_not_to_the_editor_behind() {
        let texto: String = (0..200).map(|linha| format!("linha {linha}\n")).collect();
        let mut shell = shell_editing(&texto);
        let size = Size::new(1280.0, 800.0);
        let _ = shell.paint(size);
        let antes = shell.editor_scroll_line();

        let local = |arquivo: &str, linha: u32| Location {
            path: PathBuf::from(arquivo),
            range: DomainTextRange {
                start: DomainTextPosition { line: linha, column: 0 },
                end: DomainTextPosition { line: linha, column: 6 },
            },
        };
        shell.show_rename(
            PathBuf::from("src/Pedido.java"),
            (0..40)
                .map(|indice| local(&format!("src/Arquivo{indice}.java"), indice))
                .collect(),
        );
        let _ = shell.paint(size);

        // A roda sobre a janela não pode mover o editor coberto.
        let centro = Point::new(size.width / 2.0, size.height / 2.0);
        for _ in 0..10 {
            shell.scroll(centro, 3.0, size);
        }
        assert_eq!(
            shell.editor_scroll_line(),
            antes,
            "o editor atrás da janela não pode rolar"
        );
    }

    /// A segunda escolha da seção recebe clique no combo e no botão.
    ///
    /// Ela era desenhada e não respondia: a janela roteava o clique só para a
    /// primeira. É a mesma falha de costura de sempre — o widget certo, o
    /// caminho até ele faltando.
    #[test]
    fn the_secondary_tool_answers_the_pointer() {
        let mut shell = test_shell();
        let mut catalog = java_catalog();
        if let Some(section) = catalog.settings_sections.first_mut() {
            section.secondary_caption = Some("Maven".to_owned());
        }
        shell.set_ui_catalog(catalog);
        let size = Size::new(1280.0, 800.0);
        shell.set_secondary_tool_options(
            vec!["Maven 3.9.6 — /opt/maven".to_owned(), "Maven 3.8.8 — /usr/share/maven".to_owned()],
            Some(0),
        );
        shell.open_settings_dialog(vec!["JDK 21".to_owned()], 0);
        let _ = shell.paint(size);

        let geometry = {
            let mut modal = shell.settings.modal.clone();
            modal.layout(
                &shell.layout_context(),
                Rect::new(0.0, 0.0, size.width, size.height),
            );
            settings_dialog_geometry(modal.panel_bounds())
        };

        // Abrir a lista e escolher a segunda opção.
        let combo = geometry.secondary_combo;
        shell.pointer_down(
            Point::new(combo.origin.x + 20.0, combo.origin.y + combo.size.height / 2.0),
            size,
        );
        shell.pointer_down(
            Point::new(
                combo.origin.x + 20.0,
                combo.origin.y + combo.size.height + combo.size.height * 1.5,
            ),
            size,
        );
        assert_eq!(
            shell.selected_secondary_tool(),
            Some(1),
            "clicar na lista precisa mudar a escolha"
        );

        // O botão põe o pedido de procurar na fila da aplicação.
        let browse = geometry.secondary_browse;
        shell.pointer_down(
            Point::new(
                browse.origin.x + browse.size.width / 2.0,
                browse.origin.y + browse.size.height / 2.0,
            ),
            size,
        );
        assert!(
            shell
                .drain_application_commands()
                .iter()
                .any(|comando| matches!(comando, ApplicationCommand::BrowseSecondaryTool)),
            "o botão precisa pedir o seletor de pasta"
        );
    }

    /// A janela mostra o nome do arquivo e todos os arquivos afetados.
    #[test]
    fn the_rename_window_lists_every_affected_file() {
        let mut shell = shell_editing("public class Pedido {\n}\n");
        let local = |arquivo: &str, linha: u32, de: u32, ate: u32| Location {
            path: PathBuf::from(arquivo),
            range: DomainTextRange {
                start: DomainTextPosition { line: linha, column: de },
                end: DomainTextPosition { line: linha, column: ate },
            },
        };
        shell.show_rename(
            PathBuf::from("src/Pedido.java"),
            vec![
                local("src/Pedido.java", 0, 13, 19),
                local("src/Servico.java", 4, 8, 14),
                local("src/Servico.java", 9, 12, 18),
            ],
        );

        assert!(shell.rename_open());
        assert_eq!(shell.rename_name(), "Pedido", "o campo começa com o nome atual");
        let lista = shell.rename_references();
        assert_eq!(lista.len(), 2, "dois arquivos afetados: {lista:?}");
        assert!(
            lista.iter().any(|item| item.contains("Servico.java") && item.ends_with("2")),
            "a lista diz quantas ocorrências cada arquivo tem: {lista:?}"
        );
        assert!(
            lista.iter().any(|item| item.contains("Pedido.java")),
            "o próprio arquivo entra: nele estão a declaração e os construtores"
        );

        // Pelos dois caminhos: a tecla no shell e o `escape` que a janela usa.
        shell.key_down("Escape");
        assert!(!shell.rename_open());
        shell.show_rename(PathBuf::from("src/Pedido.java"), Vec::new());
        assert!(shell.rename_open());
        shell.escape();
        assert!(!shell.rename_open(), "Esc precisa fechar por `escape` também");
    }

    /// Confirmar reescreve o que está aberto e manda o resto para a aplicação.
    #[test]
    fn confirming_rewrites_open_files_and_delegates_the_closed_ones() {
        let mut shell = test_shell();
        let aberto = shell
            .editor_area
            .session
            .open_memory("src/Pedido.java", "public class Pedido {\n    Pedido() {}\n}\n");
        shell.context.focus = ShellFocus::Editor;
        let local = |arquivo: &str, linha: u32, de: u32, ate: u32| Location {
            path: PathBuf::from(arquivo),
            range: DomainTextRange {
                start: DomainTextPosition { line: linha, column: de },
                end: DomainTextPosition { line: linha, column: ate },
            },
        };
        shell.show_rename(
            PathBuf::from("src/Pedido.java"),
            vec![
                local("src/Pedido.java", 0, 13, 19),
                local("src/Pedido.java", 1, 4, 10),
                local("src/Servico.java", 4, 8, 14),
            ],
        );

        for _ in 0..6 {
            shell.key_down("Backspace");
        }
        shell.text_input("Compra");
        shell.key_down("Enter");
        assert!(!shell.rename_open());

        // O arquivo aberto foi reescrito no buffer, com aba e desfazer intactos.
        let texto = shell
            .document_text(aberto)
            .unwrap_or_default();
        assert!(texto.contains("public class Compra"), "{texto}");
        assert!(texto.contains("Compra() {}"), "o construtor acompanha: {texto}");

        // O fechado vai no pedido, junto do arquivo a mover.
        let pedido = shell
            .drain_application_commands()
            .into_iter()
            .find_map(|comando| match comando {
                ApplicationCommand::RenameDocument(request) => Some(request),
                _ => None,
            });
        let Some(pedido) = pedido else {
            panic!("confirmar precisa pedir a renomeação do arquivo");
        };
        assert_eq!(pedido.from, PathBuf::from("src/Pedido.java"));
        assert_eq!(pedido.to, PathBuf::from("src/Compra.java"));
        assert_eq!(pedido.old_name, "Pedido");
        assert_eq!(pedido.new_name, "Compra");
        assert_eq!(
            pedido.occurrences.len(),
            1,
            "só o arquivo fechado: o aberto a tela já reescreveu"
        );
        assert_eq!(pedido.occurrences[0].path, PathBuf::from("src/Servico.java"));
    }

    /// O construtor usa a mesma janela, e a escolha vira o pedido à linguagem.
    ///
    /// A tela não escreve construtor nenhum: ela decide **quais campos** e
    /// entrega a lista. Marcar nada é um pedido vazio — o construtor sem
    /// parâmetros —, e o botão que gera tudo manda todos os campos.
    #[test]
    fn the_constructor_uses_the_same_window_and_asks_the_language() {
        let plano = || AccessorPlan {
            candidates: vec![
                AccessorCandidate {
                    field: "id".to_owned(),
                    // O construtor não traz texto por campo: ele é montado depois.
                    source: None,
                },
                AccessorCandidate {
                    field: "nome".to_owned(),
                    source: None,
                },
            ],
            insert_at: DomainTextPosition { line: 1, column: 0 },
        };
        let size = Size::new(1280.0, 800.0);

        // Sem marcar nada, o OK pede um construtor sem parâmetros.
        let mut shell = shell_editing("class Pedido {\n}\n");
        shell.show_accessor_plan(AccessorKind::Constructor, plano());
        assert!(shell.generate_open(), "a janela é a mesma");
        assert_eq!(
            shell.generate_fields(),
            vec!["id", "nome"],
            "o construtor lista todos os campos, e não só os que faltam"
        );
        let (_, _, ok) = shell.generate_geometry(size);
        shell.pointer_down(Point::new(ok.origin.x + 10.0, ok.origin.y + 10.0), size);
        let Some((campos, onde)) = shell.take_constructor_request() else {
            panic!("o OK precisa deixar um pedido de construtor");
        };
        assert!(campos.is_empty(), "nada marcado é o construtor sem parâmetros");
        assert_eq!(onde.line, 1);

        // Marcando um campo, só ele vai no pedido.
        let mut shell = shell_editing("class Pedido {\n}\n");
        shell.show_accessor_plan(AccessorKind::Constructor, plano());
        let (lista, _, ok) = shell.generate_geometry(size);
        shell.pointer_down(
            Point::new(lista.origin.x + 20.0, lista.origin.y + 12.0),
            size,
        );
        shell.pointer_down(Point::new(ok.origin.x + 10.0, ok.origin.y + 10.0), size);
        let Some((campos, _)) = shell.take_constructor_request() else {
            panic!("o OK precisa deixar um pedido de construtor");
        };
        assert_eq!(campos, vec!["id"], "só o campo marcado entra");

        // O botão que gera tudo manda todos, sem depender da marcação.
        let mut shell = shell_editing("class Pedido {\n}\n");
        shell.show_accessor_plan(AccessorKind::Constructor, plano());
        let (_, todos, _) = shell.generate_geometry(size);
        shell.pointer_down(
            Point::new(todos.origin.x + 10.0, todos.origin.y + 10.0),
            size,
        );
        let Some((campos, _)) = shell.take_constructor_request() else {
            panic!("o botão All precisa deixar um pedido de construtor");
        };
        assert_eq!(campos, vec!["id", "nome"]);
    }

    /// O texto do construtor vem da linguagem e é escrito onde ela mandou.
    #[test]
    fn the_constructor_text_from_the_language_is_written_at_the_given_line() {
        let mut shell = shell_editing("class Pedido {\n}\n");
        let onde = DomainTextPosition { line: 1, column: 0 };
        let fonte = "\n    public Pedido(Long id) {\n        this.id = id;\n    }\n";
        assert!(shell.insert_constructor(Some(fonte.to_owned()), onde));
        let texto = shell.active_text().unwrap_or_default();
        assert!(texto.contains("public Pedido(Long id)"), "{texto}");

        // Assinatura repetida: a linguagem devolve nada, e nada é escrito.
        let antes = shell.active_text().unwrap_or_default().to_owned();
        assert!(!shell.insert_constructor(None, onde));
        assert_eq!(shell.active_text().unwrap_or_default(), antes);
        assert!(shell.status_message().contains("já existe"));
    }

    /// Sem nada a gerar, a janela nem abre.
    #[test]
    fn nothing_to_generate_does_not_open_a_window() {
        let mut shell = shell_editing("class Matricula {}\n");
        shell.show_accessor_plan(AccessorKind::Getter, AccessorPlan {
            candidates: vec![AccessorCandidate {
                field: "nome".to_owned(),
                source: None,
            }],
            insert_at: DomainTextPosition { line: 0, column: 0 },
        });
        assert!(!shell.generate_open());
        assert_eq!(shell.status_message(), "Todos os campos já têm esse acessor");
    }

    /// A rolagem vertical é contínua, e não de linha em linha.
    ///
    /// Meio passo de roda — o que um touchpad manda o tempo todo — precisa
    /// mover meia linha. Arredondar para linha inteira é o que fazia o texto
    /// saltar a cada passo em vez de deslizar.
    #[test]
    fn the_vertical_scroll_moves_by_pixels_instead_of_whole_lines() {
        let mut shell = test_shell();
        shell.editor_area.session.open_memory(
            "long.rs",
            (0..200)
                .map(|line| format!("line {line}"))
                .collect::<Vec<_>>()
                .join("\n"),
        );
        let size = Size::new(1280.0, 800.0);
        let ponto = Point::new(ACTIVITY_WIDTH + SIDEBAR_WIDTH + 100.0, 200.0);

        shell.scroll(ponto, 0.5, size);
        let meia = shell.editor_area.pane.scroll_offset();
        assert!(
            (meia - EDITOR_LINE_HEIGHT / 2.0).abs() < 0.01,
            "meio passo move meia linha, e não uma inteira nem nenhuma: {meia}"
        );

        // Somando meios passos chega-se a uma linha inteira, sem perder resto.
        shell.scroll(ponto, 0.5, size);
        assert!(
            (shell.editor_area.pane.scroll_offset() - EDITOR_LINE_HEIGHT).abs() < 0.01,
            "as frações se somam em vez de serem descartadas"
        );
        assert_eq!(shell.editor_scroll_line(), 1);

        // Rolar para trás não passa do topo.
        shell.scroll(ponto, -50.0, size);
        assert_eq!(shell.editor_area.pane.scroll_offset(), 0.0);
    }

    /// A barra lateral do editor só existe quando alguma linha passa da área.
    ///
    /// Ela fica rente à borda de baixo, onde também está a borda do terminal.
    /// Uma barra desenhada sempre tomaria aquele clique sem ter o que rolar.
    #[test]
    fn the_editor_gets_a_horizontal_scrollbar_only_when_a_line_overflows() {
        let size = Size::new(1280.0, 800.0);
        let mut curto = shell_editing("int total = 10;");
        let _ = curto.paint(size);
        assert!(
            !curto.editor_scrolls_sideways(size),
            "linha curta não pede barra lateral"
        );

        let mut longo = shell_editing(&"x".repeat(4_000));
        let comandos = longo.paint(size);
        assert!(
            longo.editor_scrolls_sideways(size),
            "linha comprida precisa de barra lateral"
        );
        let trilha = longo.editor_horizontal_scrollbar_rect(size);
        assert!(
            comandos.iter().any(|command| matches!(
                command,
                PaintCommand::FillRect(fill) if fill.rect.origin.y >= trilha.origin.y
                    && fill.rect.size.height <= trilha.size.height + 0.01
            )),
            "a trilha precisa ser desenhada"
        );

        // Arrastar a barra rola o editor de lado.
        let ponto = Point::new(
            trilha.origin.x + trilha.size.width / 2.0,
            trilha.origin.y + trilha.size.height / 2.0,
        );
        longo.pointer_down(ponto, size);
        let apos_clique = longo.editor_area.pane.scroll_x();
        assert!(
            apos_clique > 0.0,
            "clicar na trilha leva o editor para o trecho correspondente"
        );

        // O quadro seguinte não pode desfazer o que a barra fez: revelar o
        // cursor a cada pintura anulava o clique e o arrasto.
        let _ = longo.paint(size);
        assert_eq!(
            longo.editor_area.pane.scroll_x(),
            apos_clique,
            "pintar de novo não devolve a vista ao cursor"
        );

        // Arrastar continua movendo, e para além do clique.
        longo.pointer_move(
            Point::new(ponto.x + trilha.size.width / 4.0, ponto.y),
            size,
        );
        let apos_arrasto = longo.editor_area.pane.scroll_x();
        assert!(apos_arrasto > apos_clique, "o arrasto continua rolando");
        longo.pointer_up();
        let _ = longo.paint(size);
        assert_eq!(longo.editor_area.pane.scroll_x(), apos_arrasto);

        // Mover o cursor, sim, traz a vista para ele — é o que faz digitar no
        // fim de uma linha comprida não escrever fora da tela.
        longo.editor_area.pane.set_cursor(3_900);
        let _ = longo.paint(size);
        let no_cursor = longo.editor_area.pane.scroll_x();
        assert!(
            no_cursor > apos_arrasto,
            "a vista acompanha o cursor levado para o fim da linha"
        );
        longo.editor_area.pane.set_cursor(0);
        let _ = longo.paint(size);
        assert_eq!(
            longo.editor_area.pane.scroll_x(),
            0.0,
            "e volta ao começo quando o cursor volta"
        );
    }

    /// Coluna do editor em coordenadas de tela.
    fn editor_column(shell: &IdeShell, size: Size, index: usize) -> Point {
        let geometry = shell.geometry(size);
        let editor_x = ACTIVITY_WIDTH + shell.sidebar_width(size);
        Point::new(
            editor_x + EDITOR_GUTTER + index as f32 * EDITOR_CHAR_WIDTH,
            geometry.content_top + 20.0,
        )
    }

    /// Área de transferência de teste, sem depender do sistema.
    #[derive(Default)]
    struct FakeClipboard {
        text: std::sync::Mutex<Option<String>>,
    }

    impl ClipboardService for FakeClipboard {
        fn get_text(&self) -> Result<Option<String>, ui_window_api::ClipboardError> {
            Ok(self.text.lock().ok().and_then(|text| text.clone()))
        }

        fn set_text(&self, value: &str) -> Result<(), ui_window_api::ClipboardError> {
            if let Ok(mut text) = self.text.lock() {
                *text = Some(value.to_owned());
            }
            Ok(())
        }
    }

    /// O duplo clique seleciona a palavra, e a regra vem do editor da biblioteca.
    #[test]
    fn a_double_click_selects_the_word_under_the_pointer() {
        let mut shell = shell_editing("int total = 10;");
        let size = Size::new(1280.0, 800.0);
        // Coluna 6 cai no meio de `total`.
        shell.select_word_at_point(editor_column(&shell, size, 6), size);
        assert_eq!(shell.editor_area.pane.selection_range(), Some(4..9));
        assert_eq!(
            shell
                .active_text()
                .and_then(|text| text.get(4..9))
                .map(str::to_owned),
            Some("total".to_owned())
        );
    }

    /// Copiar leva o trecho para a área de transferência; colar o traz de volta.
    #[test]
    fn copying_and_pasting_go_through_the_clipboard() {
        let clipboard = Arc::new(FakeClipboard::default());
        let mut shell = shell_editing("total");
        shell.set_clipboard(clipboard.clone());
        shell.editor_area.pane.set_cursor(5);
        shell.editor_area.pane.set_selection(Some((0, 5)));
        assert!(shell.copy_selection());
        assert_eq!(
            clipboard.get_text().unwrap_or_default(),
            Some("total".to_owned())
        );

        shell.editor_area.pane.set_selection(None);
        shell.editor_area.pane.set_cursor(5);
        assert!(shell.paste_clipboard());
        assert_eq!(shell.active_text(), Some("totaltotal"));
    }

    /// Colar sobre uma seleção troca o trecho marcado.
    #[test]
    fn pasting_over_a_selection_replaces_it() {
        let clipboard = Arc::new(FakeClipboard::default());
        assert!(clipboard.set_text("novo").is_ok());
        let mut shell = shell_editing("abcdef");
        shell.set_clipboard(clipboard);
        shell.editor_area.pane.set_cursor(4);
        shell.editor_area.pane.set_selection(Some((1, 4)));
        assert!(shell.paste_clipboard());
        assert_eq!(shell.active_text(), Some("anovoef"));
    }

    /// Sem área de transferência, copiar avisa em vez de fingir que copiou.
    #[test]
    fn copying_without_a_clipboard_reports_it() {
        let mut shell = shell_editing("total");
        shell.editor_area.pane.set_selection(Some((0, 5)));
        assert!(!shell.copy_selection());
        assert_eq!(shell.status_message(), "Área de transferência indisponível");
    }

    /// O clique direito no editor abre o menu de copiar e colar; sem seleção,
    /// copiar aparece desabilitado.
    #[test]
    fn the_editor_context_menu_offers_copy_and_paste() {
        let mut shell = shell_editing("total");
        let size = Size::new(1280.0, 800.0);
        shell.secondary_pointer_down(editor_column(&shell, size, 2), size);
        assert!(shell.context_menu_open());
        let entries = shell.explorer.context_menu.entries();
        assert_eq!(entry_labels(entries), vec!["Copiar", "Colar"]);
        let copy_enabled = |entries: &[MenuEntry]| match &entries[0] {
            MenuEntry::Item(item) => item.enabled,
            MenuEntry::Separator | MenuEntry::Submenu { .. } => false,
        };
        assert!(!copy_enabled(entries), "sem seleção não há o que copiar");

        shell.editor_area.pane.set_selection(Some((0, 5)));
        shell.secondary_pointer_down(editor_column(&shell, size, 2), size);
        assert!(copy_enabled(shell.explorer.context_menu.entries()));
    }

    /// O objeto inspecionado: um `Pedido` com um campo simples e outro objeto.
    fn inspection_value() -> DebugVariableView {
        DebugVariableView {
            name: "pedido".to_owned(),
            value: "Pedido@1a2b".to_owned(),
            type_name: Some("br.com.exemplo.Pedido".to_owned()),
            expandable: true,
        }
    }

    fn inspection_fields() -> Vec<DebugVariableView> {
        vec![
            DebugVariableView {
                name: "total".to_owned(),
                value: "42".to_owned(),
                type_name: Some("int".to_owned()),
                expandable: false,
            },
            DebugVariableView {
                name: "cliente".to_owned(),
                value: "Cliente@3c4d".to_owned(),
                type_name: Some("br.com.exemplo.Cliente".to_owned()),
                expandable: true,
            },
        ]
    }

    fn inspection_void() -> DebugVariableView {
        DebugVariableView {
            name: "retorno".to_owned(),
            value: "void".to_owned(),
            type_name: None,
            expandable: false,
        }
    }

    /// A janela abre com o objeto na lista e o detalhe do item destacado.
    #[test]
    fn the_inspection_window_lists_the_object_and_details_the_selection() {
        let mut shell = shell_editing("int total = 10;");
        let size = Size::new(1280.0, 800.0);
        shell.show_inspection("pedido", inspection_value(), inspection_fields());
        assert!(shell.inspection_open());
        assert_eq!(shell.inspected_expression(), Some("pedido"));

        let texts = painted_texts(&mut shell, size);
        // Painel esquerdo: a raiz aberta, com os campos abaixo dela.
        assert!(
            texts
                .iter()
                .any(|text| { text.contains("pedido = (br.com.exemplo.Pedido) Pedido@1a2b") }),
            "a árvore precisa mostrar o objeto: {texts:?}"
        );
        assert!(
            texts.iter().any(|text| text.contains("total = (int) 42")),
            "a árvore precisa mostrar os campos: {texts:?}"
        );
        // Painel direito: detalhe da raiz, que abre destacada.
        assert!(
            texts.iter().any(|text| text == "br.com.exemplo.Pedido"),
            "o detalhe precisa mostrar o tipo: {texts:?}"
        );
    }

    /// Clicar em um campo troca o que o painel direito detalha.
    #[test]
    fn clicking_a_field_changes_the_detail_panel() {
        let mut shell = shell_editing("int total = 10;");
        let size = Size::new(1280.0, 800.0);
        shell.show_inspection("pedido", inspection_value(), inspection_fields());
        let geometry = inspection_layout(&mut shell, size);
        // Segunda linha da árvore: o primeiro campo, com a raiz já aberta.
        shell.pointer_down(
            Point::new(
                geometry.list.origin.x + 30.0,
                geometry.list.origin.y + INSPECTION_ROW_HEIGHT + 4.0,
            ),
            size,
        );
        assert_eq!(
            shell.inspection_selected().map(|entry| entry.name.clone()),
            Some("total".to_owned())
        );
    }

    /// Abrir um campo que é objeto pede os campos dele ao alvo.
    ///
    /// Os níveis seguintes não vêm juntos: o grafo de um objeto pode ser fundo e
    /// cíclico, e percorrê-lo inteiro para mostrar o primeiro nível travaria.
    #[test]
    fn expanding_a_nested_object_asks_the_target_for_its_fields() {
        let mut shell = shell_editing("int total = 10;");
        let size = Size::new(1280.0, 800.0);
        shell.show_inspection("pedido", inspection_value(), inspection_fields());
        let geometry = inspection_layout(&mut shell, size);
        // Terceira linha: `cliente`, que é objeto.
        shell.pointer_down(
            Point::new(
                geometry.list.origin.x + 30.0,
                geometry.list.origin.y + INSPECTION_ROW_HEIGHT * 2.0 + 4.0,
            ),
            size,
        );
        assert_eq!(
            shell.take_debug_requests(),
            vec![DebugRequest::ExpandInspection("pedido.cliente".to_owned())]
        );

        // Os campos chegam e passam a aparecer sob o nó aberto.
        shell.add_inspection_fields(
            "pedido.cliente",
            vec![DebugVariableView {
                name: "nome".to_owned(),
                value: "João da Silva".to_owned(),
                type_name: Some("String".to_owned()),
                expandable: false,
            }],
        );
        assert!(
            painted_texts(&mut shell, size)
                .iter()
                .any(|text| text.contains("nome = (String) João da Silva")),
            "o campo do objeto aninhado precisa aparecer"
        );
    }

    /// Sem sessão viva não há onde executar, e a janela diz isso.
    ///
    /// A árvore continua mostrando o que foi lido enquanto a execução estava
    /// parada, então sem esse aviso o usuário clicaria em Executar achando que a
    /// sessão ainda está de pé.
    #[test]
    fn running_without_a_live_session_explains_itself() {
        let mut shell = shell_editing("int total = 10;");
        shell.show_inspection("pedido", inspection_value(), inspection_fields());
        shell.debug_panel.inspection.source = TextBuffer::new("m.setId(4L);");
        shell.run_inspection_source();
        assert!(shell.take_debug_requests().is_empty());
        assert_eq!(
            shell.debug_panel.inspection.message.as_deref(),
            Some("A sessão de depuração terminou; reconecte para executar")
        );
    }

    /// O painel direito reusa o editor, com os comportamentos de arquivo
    /// desligados: escrever ali não é editar um arquivo do projeto.
    #[test]
    fn the_inspection_editor_reuses_the_pane_without_file_behaviours() {
        let mut shell = shell_editing("int total = 10;");
        let size = Size::new(1280.0, 800.0);
        shell.show_inspection("pedido", inspection_value(), inspection_fields());
        let geometry = inspection_layout(&mut shell, size);
        let capabilities = shell.debug_panel.inspection.editor.capabilities();
        assert!(!capabilities.save, "não há arquivo para salvar");
        assert!(!capabilities.navigation, "não há definição para navegar");
        assert!(!capabilities.breakpoint_gutter, "não há linha onde parar");
        assert!(!capabilities.context_menu);

        // Clicar no editor leva o foco e a digitação para lá.
        shell.pointer_down(
            Point::new(
                geometry.source.origin.x + 60.0,
                geometry.source.origin.y + 8.0,
            ),
            size,
        );
        shell.text_input("pedido.total");
        assert_eq!(shell.inspection_source(), "pedido.total");
        // O documento aberto no editor principal não foi tocado.
        assert_eq!(shell.active_text(), Some("int total = 10;"));
    }

    /// Executar pede a avaliação do que foi digitado.
    #[test]
    fn running_the_inspection_source_asks_for_its_evaluation() {
        let mut shell = shell_editing("int total = 10;");
        shell.debug_panel.view.attached = true;
        let size = Size::new(1280.0, 800.0);
        shell.show_inspection("pedido", inspection_value(), inspection_fields());
        let geometry = inspection_layout(&mut shell, size);
        shell.pointer_down(
            Point::new(
                geometry.source.origin.x + 60.0,
                geometry.source.origin.y + 8.0,
            ),
            size,
        );
        shell.text_input("pedido.cliente.nome");
        shell.pointer_down(
            Point::new(geometry.run.origin.x + 10.0, geometry.run.origin.y + 10.0),
            size,
        );
        assert_eq!(
            shell.take_debug_requests(),
            vec![DebugRequest::Evaluate("pedido.cliente.nome".to_owned())]
        );
    }

    /// O resultado da execução não toma o lugar da árvore.
    ///
    /// Executar `pedido.pagar()` devolve `void`; trocar a árvore por isso apagaria
    /// justamente o objeto que se queria ver mudar.
    #[test]
    fn running_keeps_the_tree_and_only_refreshes_its_values() {
        let mut shell = shell_editing("int total = 10;");
        let size = Size::new(1280.0, 800.0);
        shell.debug_panel.view.attached = true;
        shell.show_inspection("pedido", inspection_value(), inspection_fields());
        // Um nível mais fundo fica aberto, para conferir que ele é relido.
        let geometry = inspection_layout(&mut shell, size);
        shell.pointer_down(
            Point::new(
                geometry.list.origin.x + 30.0,
                geometry.list.origin.y + INSPECTION_ROW_HEIGHT * 2.0 + 4.0,
            ),
            size,
        );
        let _ = shell.take_debug_requests();

        shell.debug_panel.inspection.source = TextBuffer::new("pedido.pagar()");
        shell.run_inspection_source();
        assert_eq!(
            shell.take_debug_requests(),
            vec![DebugRequest::Evaluate("pedido.pagar()".to_owned())]
        );

        // Chega o retorno da chamada: nada de árvore nova.
        shell.inspection_result("pedido.pagar()".to_owned(), inspection_void(), Vec::new());
        assert_eq!(shell.inspected_expression(), Some("pedido"));
        let texts = painted_texts(&mut shell, size);
        assert!(
            texts
                .iter()
                .any(|text| text.contains("pedido = (br.com.exemplo.Pedido) Pedido@1a2b")),
            "a árvore deveria continuar mostrando o objeto: {texts:?}"
        );
        assert_eq!(
            shell.debug_panel.inspection.message.as_deref(),
            Some("pedido.pagar() → void"),
            "o retorno aparece na linha de mensagem, não na árvore"
        );

        // A releitura da raiz foi pedida.
        assert_eq!(
            shell.take_debug_requests(),
            vec![DebugRequest::Evaluate("pedido".to_owned())]
        );

        // E ela troca os valores sem fechar o que estava aberto.
        shell.inspection_result(
            "pedido".to_owned(),
            inspection_value(),
            vec![
                DebugVariableView {
                    name: "total".to_owned(),
                    value: "0".to_owned(),
                    type_name: Some("int".to_owned()),
                    expandable: false,
                },
                DebugVariableView {
                    name: "cliente".to_owned(),
                    value: "Cliente@3c4d".to_owned(),
                    type_name: Some("br.com.exemplo.Cliente".to_owned()),
                    expandable: true,
                },
            ],
        );
        let texts = painted_texts(&mut shell, size);
        assert!(
            texts.iter().any(|text| text.contains("total = (int) 0")),
            "o valor deveria ser o de depois da execução: {texts:?}"
        );
        assert_eq!(
            shell.take_debug_requests(),
            vec![DebugRequest::ExpandInspection("pedido.cliente".to_owned())],
            "o nível aberto abaixo da raiz também precisa ser relido"
        );
    }

    /// Clicar fora dispensa a lista; clicar dentro escolhe a linha.
    ///
    /// Uma lista que sobrevive ao clique fica pairando sobre um cursor que já se
    /// moveu. E o clique dentro dela precisa ser consumido de qualquer forma, ou
    /// atravessaria a lista e moveria o cursor no editor de baixo.
    #[test]
    fn clicking_outside_the_completion_list_dismisses_it() {
        let mut shell = shell_editing("int total = 10;");
        let size = Size::new(1280.0, 800.0);
        let item = |label: &str| CompletionItem {
            label: label.to_owned(),
            detail: None,
            kind: ide_domain::CompletionKind::Method,
        };
        shell.context.focus = ShellFocus::Editor;
        shell.set_completions(vec![item("getAluno()"), item("getId()")]);
        let rect = shell
            .completion_rect(size)
            .unwrap_or_else(|| panic!("a lista aberta precisa ocupar uma área"));

        // Um ponto claramente fora: o canto oposto da janela.
        shell.pointer_down(Point::new(rect.origin.x - 40.0, rect.origin.y - 40.0), size);
        assert!(!shell.completion_open(), "a lista sai com o clique de fora");

        // Reaberta, o clique na segunda linha escolhe aquele item.
        shell.editor_area.pane.set_cursor(0);
        shell.set_completions(vec![item("getAluno()"), item("getId()")]);
        let rect = shell
            .completion_rect(size)
            .unwrap_or_else(|| panic!("a lista aberta precisa ocupar uma área"));
        shell.pointer_down(
            Point::new(
                rect.origin.x + 20.0,
                rect.origin.y + COMPLETION_POPUP_PADDING + COMPLETION_ROW_HEIGHT + 4.0,
            ),
            size,
        );
        assert!(!shell.completion_open(), "escolher também fecha a lista");
        assert_eq!(
            shell.active_text(),
            Some("getId()int total = 10;"),
            "o item clicado é o que entra no texto"
        );
    }

    fn type_hit(name: &str, kind: &str, path: &std::path::Path, line: u32) -> TypeSearchHit {
        TypeSearchHit {
            name: name.to_owned(),
            kind: kind.to_owned(),
            location: Location {
                path: path.into(),
                range: ide_domain::TextRange {
                    start: DomainTextPosition { line, column: 0 },
                    end: DomainTextPosition { line, column: 0 },
                },
            },
        }
    }

    #[test]
    fn application_commands_leave_the_shell_in_one_ordered_queue() {
        let mut shell = shell_editing("class Uso {}");
        shell.open_type_search();
        shell.request_debug(DebugRequest::Continue);

        assert_eq!(
            shell.drain_application_commands(),
            vec![
                ApplicationCommand::SearchTypes(String::new()),
                ApplicationCommand::Debug(DebugRequest::Continue),
            ]
        );
        assert!(shell.drain_application_commands().is_empty());
    }

    fn content_hit(path: &std::path::Path, line: u32, column: u32) -> ContentSearchHit {
        ContentSearchHit {
            preview: "String mensagem = \"conteúdo procurado\";".to_owned(),
            location: Location {
                path: path.into(),
                range: ide_domain::TextRange {
                    start: DomainTextPosition { line, column },
                    end: DomainTextPosition { line, column },
                },
            },
        }
    }

    #[test]
    fn type_search_shows_the_path_after_the_last_java_directory() {
        let absolute = PathBuf::from(r"C:\workspace\java\modulo\src\main\java")
            .join("br")
            .join("com")
            .join("exemplo")
            .join("Pedido.java");
        let hit = type_hit("Pedido", "classe", &absolute, 0);
        let expected = PathBuf::from("br")
            .join("com")
            .join("exemplo")
            .join("Pedido.java");

        let roots = java_source_roots();
        assert_eq!(search_display_path(&absolute, &roots), expected);
        assert!(
            !hit.label(&roots).contains("workspace"),
            "o resultado não pode mostrar o caminho absoluto: {}",
            hit.label(&roots)
        );
    }

    /// Diretório com dois tipos, para a busca ter o que abrir de verdade.
    fn type_search_workspace() -> std::path::PathBuf {
        static NEXT_WORKSPACE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let sequence = NEXT_WORKSPACE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let root =
            std::env::temp_dir().join(format!("er-ide-busca-{}-{sequence}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        assert!(std::fs::create_dir_all(&root).is_ok());
        assert!(std::fs::write(root.join("Pedido.java"), "class Pedido {}\n").is_ok());
        assert!(
            std::fs::write(
                root.join("PedidoRepository.java"),
                "interface PedidoRepository {}\n"
            )
            .is_ok()
        );
        root
    }

    /// A busca por nome pede, mostra e leva ao arquivo escolhido.
    #[test]
    fn the_type_search_asks_lists_and_opens_what_was_chosen() {
        let root = type_search_workspace();
        let mut shell = shell_editing("class Uso {}");
        let size = Size::new(1280.0, 800.0);
        shell.open_type_search();
        assert!(shell.type_search_open());
        assert_eq!(
            shell.take_type_search_request(),
            Some(String::new()),
            "a janela nasce pedindo tudo, sem esperar a primeira letra"
        );

        // Digitar refina, e cada tecla vira um pedido.
        shell.text_input("Ped");
        assert_eq!(shell.take_type_search_request(), Some("Ped".to_owned()));
        shell.key_down("Backspace");
        assert_eq!(shell.take_type_search_request(), Some("Pe".to_owned()));

        // Os resultados aparecem na janela, com nome, tipo e caminho.
        let repositorio = root.join("PedidoRepository.java");
        shell.set_type_search_results(vec![
            type_hit("Pedido", "classe", &root.join("Pedido.java"), 0),
            type_hit("PedidoRepository", "interface", &repositorio, 0),
        ]);
        let texts = painted_texts(&mut shell, size);
        assert!(
            texts
                .iter()
                .any(|text| text.contains("Pedido (classe)") && text.contains("Pedido.java")),
            "a lista precisa mostrar o que foi encontrado e onde: {texts:?}"
        );

        // As setas andam na lista e `Enter` pede à aplicação que abra o
        // escolhido. A UI não lê o arquivo.
        shell.key_down("ArrowDown");
        shell.key_down("Enter");
        assert!(!shell.type_search_open(), "escolher fecha a janela");
        assert_eq!(
            shell.drain_application_commands(),
            vec![ApplicationCommand::OpenDocument(OpenDocumentRequest::new(
                repositorio
            ))],
            "o segundo item é o que devia abrir"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    /// A busca textual usa o mesmo modal, mas só pede depois de haver texto e
    /// abre a ocorrência na linha e coluna devolvidas pelo workspace.
    #[test]
    fn content_search_reuses_the_modal_and_opens_the_occurrence() {
        let root = type_search_workspace();
        let source = root.join("Pedido.java");
        assert!(
            std::fs::write(
                &source,
                "class Pedido {\n    String mensagem = \"conteúdo procurado\";\n}\n"
            )
            .is_ok()
        );
        let mut shell = shell_editing("class Uso {}");
        let size = Size::new(1280.0, 800.0);

        shell.open_content_search();
        assert!(shell.type_search_open());
        assert_eq!(
            shell.take_content_search_request(),
            None,
            "a consulta vazia não deve varrer todos os arquivos"
        );
        shell.text_input("conteúdo");
        assert_eq!(
            shell.take_content_search_request(),
            Some("conteúdo".to_owned())
        );
        assert_eq!(
            shell.take_type_search_request(),
            None,
            "cada modo possui uma porta própria"
        );

        shell.set_content_search_results(vec![content_hit(&source, 1, 23)]);
        let texts = painted_texts(&mut shell, size);
        assert!(
            texts.iter().any(|text| {
                text.contains("Pedido.java:2") && text.contains("conteúdo procurado")
            }),
            "a lista precisa mostrar arquivo, linha e trecho: {texts:?}"
        );
        shell.key_down("Enter");

        assert!(!shell.type_search_open());
        assert_eq!(
            shell.drain_application_commands(),
            vec![ApplicationCommand::OpenDocument(
                OpenDocumentRequest::new(source).at(1, 23)
            )]
        );
        let _ = std::fs::remove_dir_all(root);
    }

    /// `Esc` dispensa a busca sem abrir nada.
    #[test]
    fn escape_closes_the_type_search() {
        let root = type_search_workspace();
        let mut shell = shell_editing("class Uso {}");
        let antes = shell.active_document_path();
        shell.open_type_search();
        let _ = shell.take_type_search_request();
        shell.set_type_search_results(vec![type_hit(
            "Pedido",
            "classe",
            &root.join("Pedido.java"),
            0,
        )]);
        shell.escape();
        assert!(!shell.type_search_open());
        assert_eq!(
            shell.active_document_path(),
            antes,
            "desistir não troca a aba aberta"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    /// A lista revela a seleção das setas e a roda nunca alcança o editor atrás.
    #[test]
    fn type_search_scroll_stays_inside_the_modal_and_reveals_keyboard_selection() {
        let root = type_search_workspace();
        let mut shell = shell_editing(
            &(0..80)
                .map(|line| format!("linha {line}\n"))
                .collect::<String>(),
        );
        let size = Size::new(1280.0, 800.0);
        shell.open_type_search();
        let _ = shell.take_type_search_request();
        shell.set_type_search_results(
            (0..30)
                .map(|index| {
                    type_hit(
                        &format!("Tipo{index:02}"),
                        "classe",
                        &root.join("Pedido.java"),
                        0,
                    )
                })
                .collect(),
        );

        for _ in 0..(TYPE_SEARCH_VISIBLE_ROWS + 3) {
            shell.key_down("ArrowDown");
        }
        assert_eq!(shell.search.selected, TYPE_SEARCH_VISIBLE_ROWS + 3);
        assert!(
            shell.search.first_visible > 0,
            "a seleção que passou do viewport precisa trazer a lista junto"
        );
        let texts = painted_texts(&mut shell, size);
        assert!(
            texts.iter().any(|text| text.contains("Tipo15")),
            "o item escolhido pelas setas precisa continuar visível: {texts:?}"
        );

        let editor_before = shell.editor_area.pane.scroll_line();
        let (_, list) = shell.type_search_geometry(size);
        shell.scroll(
            Point::new(list.origin.x + 20.0, list.origin.y + 20.0),
            3.0,
            size,
        );
        assert_eq!(
            shell.editor_area.pane.scroll_line(),
            editor_before,
            "a roda no modal não pode rolar o editor atrás"
        );
        assert!(
            shell.search.first_visible > 0,
            "a própria lista precisa receber a roda"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    /// A sequência real: `Ctrl+D` pela shell e depois digitar.
    ///
    /// O teste do painel passava e a IDE não, então o caminho que o app percorre
    /// é que precisa ser exercido — do atalho à digitação, pelas mesmas portas.
    #[test]
    fn marking_and_typing_through_the_shell_edits_every_occurrence() {
        let mut shell = shell_editing("nome = nome + nome");
        shell.context.focus = ShellFocus::Editor;
        // Cursor no fim do trecho, como fica ao selecionar arrastando.
        shell.editor_area.pane.set_cursor(4);
        shell.editor_area.pane.set_selection(Some((0, 4)));

        shell.key_down_with_modifiers(
            "d",
            Modifiers {
                control: true,
                ..Modifiers::default()
            },
        );
        assert_eq!(
            shell.editor_area.pane.occurrences(),
            vec![(0, 4), (7, 11)],
            "o atalho precisa marcar pela shell, não só pelo painel"
        );

        // Cada marca é um cursor: o que estava lá permanece, e a letra digitada
        // entra em todas as ocorrências.
        shell.text_input("s");
        assert_eq!(shell.active_text(), Some("nomes = nomes + nome"));
        shell.text_input("!");
        assert_eq!(
            shell.active_text(),
            Some("nomes! = nomes! + nome"),
            "a segunda letra também é replicada"
        );
        shell.key_down("Backspace");
        shell.key_down("Backspace");
        assert_eq!(
            shell.active_text(),
            Some("nome = nome + nome"),
            "apagar tira uma letra de cada, voltando ao começo"
        );
    }

    /// Os nomes da lista saem na cor de texto do tema.
    ///
    /// É a cor escolhida para se ler sobre a superfície, e a mesma do resto da
    /// interface — trocar o tema troca também o que a lista mostra.
    #[test]
    fn the_completion_list_paints_its_names_with_the_theme_text() {
        let mut shell = shell_editing("int total = 10;");
        let size = Size::new(1280.0, 800.0);
        shell.context.focus = ShellFocus::Editor;
        shell.set_completions(vec![CompletionItem {
            label: "getAluno()".to_owned(),
            detail: None,
            kind: ide_domain::CompletionKind::Method,
        }]);
        let colors: Vec<Color> = shell
            .paint(size)
            .into_iter()
            .filter_map(|command| match command {
                PaintCommand::DrawText(text) if text.text == "getAluno()" => Some(text.color),
                _ => None,
            })
            .collect();
        assert_eq!(
            colors,
            vec![Theme::dark().colors.text],
            "o nome sai na cor de texto do tema"
        );
    }

    /// A lista acompanha o nome sendo digitado, e não só o ponto que a abriu.
    ///
    /// Sem isto, ela mostrava o que valia no instante em que abriu e só se
    /// atualizava com `Ctrl+Space` — digitar `se` depois de `a.` deixava a lista
    /// parada.
    #[test]
    fn typing_after_the_dot_asks_for_the_list_again() {
        let mut shell = shell_editing("int total = 10;");
        let item = |label: &str| CompletionItem {
            label: label.to_owned(),
            detail: None,
            kind: ide_domain::CompletionKind::Method,
        };

        // Fechada, digitar não abre nada: abrir é do ponto ou do Ctrl+Space.
        assert!(!shell.completion_follow_up("s"));

        shell.set_completions(vec![item("setId()"), item("setNome()")]);
        assert!(shell.completion_follow_up("s"), "cada letra refaz o filtro");
        assert!(shell.completion_open(), "e a lista continua à mostra");

        // O que não faz parte de um nome encerra o nome.
        assert!(!shell.completion_follow_up("("));
        assert!(!shell.completion_open(), "a lista sai junto com o nome");

        // A resposta vazia do provider também fecha: nada casa com o prefixo.
        shell.set_completions(vec![item("setId()")]);
        shell.set_completions(Vec::new());
        assert!(!shell.completion_open());
    }

    /// O ponto no editor do depurador pergunta por um tipo, não por uma posição.
    ///
    /// O tipo de `m` só existe no quadro parado — não há fonte que o declare —,
    /// mas quem responde pelos membros é o índice do projeto. Por isso a pergunta
    /// é o nome do tipo.
    #[test]
    fn a_dot_in_the_inspection_editor_asks_for_the_members_of_a_type() {
        let mut shell = shell_editing("int total = 10;");
        shell.debug_panel.view.attached = true;
        shell.show_inspection("m", inspection_value(), inspection_fields());
        shell.debug_panel.inspection.source = TextBuffer::new("m.");
        shell.debug_panel.inspection.editor.set_cursor(2);
        assert_eq!(
            shell.inspection_member_context(),
            Some(("m.".to_owned(), 2)),
            "a shell entrega texto e cursor sem interpretar a sintaxe"
        );
        assert_eq!(
            shell.inspection_member_target("m", String::new()),
            ("br.com.exemplo.Pedido".to_owned(), String::new()),
            "o tipo vem do objeto parado, e não de declaração nenhuma"
        );

        assert_eq!(
            shell.inspection_member_target("m", "getCli".to_owned()),
            ("br.com.exemplo.Pedido".to_owned(), "getCli".to_owned())
        );
        assert_eq!(
            shell.inspection_member_target("cliente", String::new()),
            ("br.com.exemplo.Cliente".to_owned(), String::new())
        );
        assert_eq!(
            shell.inspection_member_target("Relatorio", String::new()),
            ("Relatorio".to_owned(), String::new()),
            "classe de fora do código depurado precisa ser perguntável"
        );
    }

    /// Com a lista aberta na inspeção, as setas andam nela e Enter aceita.
    #[test]
    fn the_completion_list_takes_the_keys_inside_the_inspection() {
        let mut shell = shell_editing("int total = 10;");
        shell.debug_panel.view.attached = true;
        shell.show_inspection("m", inspection_value(), inspection_fields());
        shell.debug_panel.inspection.focus = InspectionFocus::Source;
        shell.debug_panel.inspection.source = TextBuffer::new("m.");
        shell.debug_panel.inspection.editor.set_cursor(2);
        shell.set_completions(vec![
            CompletionItem {
                label: "getCliente()".to_owned(),
                detail: None,
                kind: ide_domain::CompletionKind::Method,
            },
            CompletionItem {
                label: "total".to_owned(),
                detail: None,
                kind: ide_domain::CompletionKind::Field,
            },
        ]);

        shell.key_down("ArrowDown");
        assert_eq!(shell.inspection_source(), "m.", "a seta andou na lista");
        shell.key_down("Enter");
        assert_eq!(
            shell.inspection_source(),
            "m.total",
            "aceitar escreve no editor da inspeção"
        );
        assert_eq!(
            shell.active_text(),
            Some("int total = 10;"),
            "o documento atrás da janela não é tocado"
        );
    }

    /// Seleção e área de transferência valem no editor da inspeção.
    ///
    /// O painel é o mesmo da janela principal e sempre soube selecionar; o que
    /// faltava era a janela encaminhar os gestos até ele.
    #[test]
    fn the_inspection_editor_selects_and_uses_the_clipboard() {
        let mut shell = shell_editing("int total = 10;");
        let clipboard = Arc::new(FakeClipboard::default());
        shell.set_clipboard(clipboard.clone());
        let size = Size::new(1280.0, 800.0);
        shell.show_inspection("pedido", inspection_value(), inspection_fields());
        shell.debug_panel.inspection.source = TextBuffer::new("pedido.total");
        let geometry = inspection_layout(&mut shell, size);
        let column = |index: usize| {
            Point::new(
                geometry.source.origin.x
                    + CodeEditor::gutter_width()
                    + index as f32 * CodeEditor::default_char_width(),
                geometry.source.origin.y + 4.0,
            )
        };

        // Arrastar marca o trecho.
        shell.pointer_down(column(0), size);
        shell.pointer_move(column(6), size);
        shell.pointer_up();
        assert_eq!(
            shell
                .debug_panel
                .inspection
                .editor
                .selected_text(&shell.debug_panel.inspection.source),
            Some("pedido")
        );

        // Copiar leva o que está selecionado ali, e não o documento de trás.
        assert!(shell.copy_selection());
        assert_eq!(
            clipboard.get_text().unwrap_or_default(),
            Some("pedido".to_owned())
        );

        // Colar escreve no editor da inspeção, substituindo o trecho marcado.
        assert!(clipboard.set_text("m.id").is_ok());
        assert!(shell.paste_clipboard());
        assert_eq!(shell.inspection_source(), "m.id.total");
        assert_eq!(
            shell.active_text(),
            Some("int total = 10;"),
            "o documento aberto atrás da janela não pode ser tocado"
        );

        // Duplo clique seleciona a palavra sob o ponteiro.
        shell.select_word_at_point(column(1), size);
        assert_eq!(
            shell
                .debug_panel
                .inspection
                .editor
                .selected_text(&shell.debug_panel.inspection.source),
            Some("m")
        );

        // Shift com as setas também marca, como no editor principal.
        shell.debug_panel.inspection.editor.set_cursor(0);
        for _ in 0..4 {
            shell.key_down_with_modifiers(
                "ArrowRight",
                Modifiers {
                    shift: true,
                    ..Modifiers::default()
                },
            );
        }
        assert_eq!(
            shell
                .debug_panel
                .inspection
                .editor
                .selected_text(&shell.debug_panel.inspection.source),
            Some("m.id")
        );
    }

    /// Várias instruções rodam em sequência, uma esperando a outra.
    ///
    /// Cada uma executa dentro do processo depurado e pode mudar o que a seguinte
    /// vai encontrar; mandá-las juntas, ou em paralelo, perderia essa ordem.
    #[test]
    fn several_statements_run_one_after_the_other() {
        let mut shell = shell_editing("int total = 10;");
        shell.debug_panel.view.attached = true;
        shell.show_inspection("m", inspection_value(), inspection_fields());
        shell.debug_panel.inspection.source =
            TextBuffer::new("m.setId(5L);\nm.setNome(\"Mario\");\nm.somar(1, 2)");
        shell.run_inspection_source();

        // Só a primeira vai ao alvo.
        assert_eq!(
            shell.take_debug_requests(),
            vec![DebugRequest::Evaluate("m.setId(5L)".to_owned())]
        );
        shell.inspection_result("m.setId(5L)".to_owned(), inspection_void(), Vec::new());
        assert_eq!(
            shell.take_debug_requests(),
            vec![DebugRequest::Evaluate("m.setNome(\"Mario\")".to_owned())]
        );
        shell.inspection_result(
            "m.setNome(\"Mario\")".to_owned(),
            inspection_void(),
            Vec::new(),
        );
        assert_eq!(
            shell.take_debug_requests(),
            vec![DebugRequest::Evaluate("m.somar(1, 2)".to_owned())]
        );

        // A última fecha a execução e dispara a releitura da árvore.
        shell.inspection_result(
            "m.somar(1, 2)".to_owned(),
            DebugVariableView {
                name: "m.somar(1, 2)".to_owned(),
                value: "3".to_owned(),
                type_name: Some("int".to_owned()),
                expandable: false,
            },
            Vec::new(),
        );
        assert_eq!(
            shell.debug_panel.inspection.message.as_deref(),
            Some("3 instruções executadas — m.somar(1, 2) → 3")
        );
        assert_eq!(
            shell.take_debug_requests(),
            vec![DebugRequest::Evaluate("m".to_owned())]
        );
    }

    /// Uma instrução que falha interrompe as seguintes e diz onde parou.
    #[test]
    fn a_failing_statement_stops_the_rest_and_says_where() {
        let mut shell = shell_editing("int total = 10;");
        shell.debug_panel.view.attached = true;
        shell.show_inspection("m", inspection_value(), inspection_fields());
        shell.debug_panel.inspection.source =
            TextBuffer::new("m.setId(5L);\nm.naoExiste();\nm.setId(6L);");
        shell.run_inspection_source();
        let _ = shell.take_debug_requests();
        shell.inspection_result("m.setId(5L)".to_owned(), inspection_void(), Vec::new());
        let _ = shell.take_debug_requests();

        shell.set_inspection_message("m.naoExiste(): método não encontrado");
        assert_eq!(
            shell.debug_panel.inspection.message.as_deref(),
            Some("parou na instrução 2 de 3: m.naoExiste(): método não encontrado")
        );
        // A terceira não foi pedida; a releitura, sim, porque a primeira teve
        // efeito.
        assert_eq!(
            shell.take_debug_requests(),
            vec![DebugRequest::Evaluate("m".to_owned())]
        );
    }

    /// O ponto e vírgula dentro de aspas não termina instrução.
    #[test]
    fn statements_are_split_outside_quoted_text() {
        assert_eq!(inspection_statements("a(); b()"), vec!["a()", "b()"]);
        assert_eq!(inspection_statements("a(\"x; y\")"), vec!["a(\"x; y\")"]);
        assert_eq!(
            inspection_statements("a(\"x\\\"; y\")"),
            vec!["a(\"x\\\"; y\")"],
            "aspas escapadas não fecham o texto"
        );
        assert_eq!(inspection_statements("  ;\n \n a() ;"), vec!["a()"]);
        assert!(inspection_statements("  \n ; ").is_empty());
    }

    /// Sem nada escrito, Executar avisa em vez de pedir a avaliação do vazio.
    #[test]
    fn running_an_empty_source_asks_nothing() {
        let mut shell = shell_editing("int total = 10;");
        shell.debug_panel.view.attached = true;
        shell.show_inspection("pedido", inspection_value(), inspection_fields());
        shell.run_inspection_source();
        assert!(shell.take_debug_requests().is_empty());
        assert_eq!(shell.status_message(), "Escreva a expressão a executar");
    }

    /// Esc e o botão Fechar dispensam a janela.
    #[test]
    fn the_inspection_window_closes() {
        let mut shell = shell_editing("int total = 10;");
        shell.show_inspection("pedido", inspection_value(), inspection_fields());
        shell.escape();
        assert!(!shell.inspection_open());

        let size = Size::new(1280.0, 800.0);
        shell.show_inspection("pedido", inspection_value(), inspection_fields());
        let geometry = inspection_layout(&mut shell, size);
        shell.pointer_down(
            Point::new(
                geometry.close.origin.x + 10.0,
                geometry.close.origin.y + 10.0,
            ),
            size,
        );
        assert!(!shell.inspection_open());
    }

    fn inspection_layout(shell: &mut IdeShell, size: Size) -> InspectionGeometry {
        shell.debug_panel.inspection.modal.layout(
            &LayoutContext::default(),
            Rect::new(0.0, 0.0, size.width, size.height),
        );
        inspection_geometry(shell.debug_panel.inspection.modal.panel_bounds())
    }

    fn painted_texts(shell: &mut IdeShell, size: Size) -> Vec<String> {
        shell
            .paint(size)
            .iter()
            .filter_map(|command| match command {
                PaintCommand::DrawText(text) => Some(text.text.clone()),
                _ => None,
            })
            .collect()
    }

    /// Sem depuração em curso o menu do editor não oferece Inspecionar.
    ///
    /// Fora de uma sessão não há quadro que dê valor ao nome, e o item prometeria
    /// o que não pode cumprir.
    #[test]
    fn inspect_only_appears_while_debugging() {
        let mut shell = shell_editing("int total = 10;");
        let size = Size::new(1280.0, 800.0);
        shell.editor_area.pane.set_selection(Some((4, 9)));
        shell.secondary_pointer_down(editor_column(&shell, size, 6), size);
        assert_eq!(
            entry_labels(shell.explorer.context_menu.entries()),
            vec!["Copiar", "Colar"]
        );

        shell.debug_panel.view.attached = true;
        shell.secondary_pointer_down(editor_column(&shell, size, 6), size);
        assert_eq!(
            entry_labels(shell.explorer.context_menu.entries()),
            vec!["Copiar", "Colar", "—", "Inspecionar"]
        );
    }

    /// Inspecionar pede a avaliação do trecho marcado.
    #[test]
    fn inspecting_asks_to_evaluate_the_selected_text() {
        let mut shell = shell_editing("int total = 10;");
        shell.debug_panel.view.attached = true;
        shell.editor_area.pane.set_selection(Some((4, 9)));
        shell.run_explorer_command("debug.inspect");
        assert_eq!(
            shell.take_debug_requests(),
            vec![DebugRequest::Evaluate("total".to_owned())]
        );
        assert_eq!(shell.status_message(), "Inspecionando total");
    }

    /// Sem seleção, Inspecionar aparece desabilitado e nada é pedido.
    #[test]
    fn inspecting_without_a_selection_asks_nothing() {
        let mut shell = shell_editing("int total = 10;");
        let size = Size::new(1280.0, 800.0);
        shell.debug_panel.view.attached = true;
        shell.secondary_pointer_down(editor_column(&shell, size, 6), size);
        let entries = shell.explorer.context_menu.entries();
        let enabled = match &entries[3] {
            MenuEntry::Item(item) => item.enabled,
            MenuEntry::Separator | MenuEntry::Submenu { .. } => true,
        };
        assert!(!enabled, "sem seleção não há o que inspecionar");

        shell.run_explorer_command("debug.inspect");
        assert!(shell.take_debug_requests().is_empty());
    }

    /// As setas verticais movem o cursor entre linhas, preservando a coluna.
    #[test]
    fn vertical_arrows_move_the_cursor_between_lines() {
        let mut shell = shell_editing(
            "primeira
segunda
ab",
        );
        shell.editor_area.pane.set_cursor(4);
        shell.key_down("ArrowDown");
        // Mesma coluna na linha de baixo.
        assert_eq!(
            shell.editor_area.pane.cursor(),
            "primeira
"
            .len()
                + 4
        );

        // Descer para uma linha curta para no fim dela, e não num ponto inexistente.
        shell.key_down("ArrowDown");
        assert_eq!(
            shell.editor_area.pane.cursor(),
            "primeira
segunda
ab"
            .len()
        );

        // Na última linha, descer de novo não faz nada.
        shell.key_down("ArrowDown");
        assert_eq!(
            shell.editor_area.pane.cursor(),
            "primeira
segunda
ab"
            .len()
        );

        shell.key_down("ArrowUp");
        assert_eq!(
            shell.editor_area.pane.cursor(),
            "primeira
"
            .len()
                + 2
        );
    }

    /// Shift com as setas verticais estende a seleção por linhas.
    #[test]
    fn shift_with_vertical_arrows_extends_the_selection() {
        let mut shell = shell_editing(
            "um
dois
tres",
        );
        shell.editor_area.pane.set_cursor(0);
        let shift = Modifiers {
            shift: true,
            ..Modifiers::default()
        };
        shell.key_down_with_modifiers("ArrowDown", shift);
        assert_eq!(shell.editor_area.pane.selection_range(), Some(0..3));
    }

    /// Tab com um bloco marcado desloca todas as linhas dele.
    #[test]
    fn tab_shifts_the_selected_block() {
        let mut shell = shell_editing(
            "um
dois
tres",
        );
        // Da segunda linha até o meio da terceira.
        shell.editor_area.pane.set_cursor(9);
        shell.editor_area.pane.set_selection(Some((3, 9)));
        shell.key_down("Tab");
        assert_eq!(
            shell.active_text(),
            Some(
                "um
    dois
    tres"
            )
        );
        // A seleção segue cobrindo o bloco, para indentar de novo sem remarcar.
        assert_eq!(shell.editor_area.pane.selection_range(), Some(3..20));

        let shift = Modifiers {
            shift: true,
            ..Modifiers::default()
        };
        shell.key_down_with_modifiers("Tab", shift);
        assert_eq!(
            shell.active_text(),
            Some(
                "um
dois
tres"
            )
        );
    }

    /// Arrastar no editor seleciona, e digitar substitui o trecho marcado.
    #[test]
    fn dragging_in_the_editor_selects_and_typing_replaces() {
        let mut shell = shell_editing("abcdef");
        let size = Size::new(1280.0, 800.0);
        shell.pointer_down(editor_column(&shell, size, 1), size);
        shell.pointer_move(editor_column(&shell, size, 4), size);
        shell.pointer_up();
        assert_eq!(shell.editor_area.pane.selection_range(), Some(1..4));

        shell.text_input("Z");
        assert_eq!(shell.active_text(), Some("aZef"));
        assert_eq!(shell.editor_area.pane.selection_range(), None);
    }

    /// A seleção chega ao editor da biblioteca, que é quem a desenha.
    #[test]
    fn the_selection_is_painted_by_the_library_editor() {
        let mut shell = shell_editing("abcdef");
        let size = Size::new(1280.0, 800.0);
        shell.pointer_down(editor_column(&shell, size, 1), size);
        shell.pointer_move(editor_column(&shell, size, 4), size);
        shell.pointer_up();
        let selection = shell.context.theme.colors.selection;
        assert!(
            shell.paint(size).iter().any(|command| matches!(
                command,
                PaintCommand::FillRect(fill) if fill.color == selection
            )),
            "o trecho selecionado precisa aparecer pintado"
        );
    }

    /// Shift+setas estende a seleção; sem Shift, mover desfaz.
    #[test]
    fn shift_arrows_extend_the_selection() {
        let mut shell = shell_editing("abcdef");
        let shift = Modifiers {
            shift: true,
            ..Modifiers::default()
        };
        shell.key_down_with_modifiers("ArrowRight", shift);
        shell.key_down_with_modifiers("ArrowRight", shift);
        assert_eq!(shell.editor_area.pane.selection_range(), Some(0..2));

        shell.key_down("ArrowRight");
        assert_eq!(shell.editor_area.pane.selection_range(), None);
    }

    /// Backspace com trecho marcado apaga o trecho, não um caractere.
    #[test]
    fn backspace_removes_the_selection() {
        let mut shell = shell_editing("abcdef");
        shell.editor_area.pane.set_cursor(4);
        shell.editor_area.pane.set_selection(Some((1, 4)));
        shell.key_down("Backspace");
        assert_eq!(shell.active_text(), Some("aef"));
        assert_eq!(shell.editor_area.pane.cursor(), 1);
    }

    /// Salvar grava o conteúdo da aba e limpa a marca de modificado.
    #[test]
    fn saving_writes_the_active_tab_to_disk() {
        let root = std::env::temp_dir().join(format!("er-ide-save-{}", std::process::id()));
        assert!(std::fs::create_dir_all(&root).is_ok());
        let file = root.join("Pedido.java");
        assert!(std::fs::write(&file, "class Pedido {}").is_ok());
        let Ok(mut shell) = IdeShell::open(&root) else {
            panic!("workspace de teste não abriu");
        };
        let Ok(_) = shell.open_file(&file) else {
            panic!("arquivo de teste não abriu");
        };
        shell.context.focus = ShellFocus::Editor;
        shell.editor_area.pane.set_cursor(0);
        shell.text_input("// nota\n");
        assert!(
            shell.active_document_modified(),
            "a edição deixa a aba suja"
        );

        assert!(shell.save_active_document());
        assert_eq!(
            std::fs::read_to_string(&file).unwrap_or_default(),
            "// nota\nclass Pedido {}"
        );
        assert!(
            !shell.active_document_modified(),
            "depois de gravar a aba deixa de estar suja"
        );
        assert!(shell.status_message().starts_with("Salvo "));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// O item "Salvar" entrega o conteúdo à aplicação sem escrever pela UI.
    #[test]
    fn the_file_menu_saves_the_active_tab() {
        let root = std::env::temp_dir().join(format!("er-ide-menu-save-{}", std::process::id()));
        assert!(std::fs::create_dir_all(&root).is_ok());
        let file = root.join("Pedido.java");
        assert!(std::fs::write(&file, "class Pedido {}").is_ok());
        let Ok(mut shell) = IdeShell::open(&root) else {
            panic!("workspace de teste não abriu");
        };
        let Ok(_) = shell.open_file(&file) else {
            panic!("arquivo de teste não abriu");
        };
        shell.context.focus = ShellFocus::Editor;
        shell.editor_area.pane.set_cursor(0);
        shell.text_input("// pelo menu\n");
        let size = Size::new(1280.0, 800.0);
        // Abre o menu Arquivo e escolhe a segunda entrada.
        shell.pointer_down(Point::new(100.0, TITLE_HEIGHT / 2.0), size);
        shell.pointer_down(Point::new(100.0, TITLE_HEIGHT + 42.0), size);
        let commands = shell.drain_application_commands();
        let Some(ApplicationCommand::SaveDocument(request)) = commands.first() else {
            panic!("o menu deveria emitir SaveDocument");
        };
        assert_eq!(request.path, file);
        assert_eq!(request.text, "// pelo menu\nclass Pedido {}");
        assert!(
            shell.active_document_modified(),
            "a confirmação do adapter ainda não chegou"
        );
        shell.document_saved(request.document_id, request.revision, &request.path);
        assert!(!shell.active_document_modified());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A janela nasce no centro da tela.
    ///
    /// O painel se centraliza na área que recebe no layout; sem esse layout a
    /// área era zero e ele aparecia no canto superior esquerdo.
    #[test]
    fn the_new_item_dialog_opens_centered() {
        let mut shell = shell_with_package();
        let size = Size::new(1280.0, 800.0);
        shell.explorer.context_menu_target = Some(PathBuf::from("demo/src/main/java/br/com"));
        shell.run_explorer_command("explorer.new.java.class");
        // O painel do `ModalHost` é o retângulo de superfície desenhado sobre o
        // véu, do tamanho declarado para a janela.
        let surface = shell.context.theme.colors.surface;
        let panel = shell
            .paint(size)
            .iter()
            .filter_map(|command| match command {
                PaintCommand::FillRect(fill) if fill.color == surface => Some(fill.rect),
                _ => None,
            })
            .find(|rect| rect.size == NEW_ITEM_PANEL_SIZE)
            .unwrap_or_default();
        let center_x = panel.origin.x + panel.size.width / 2.0;
        let center_y = panel.origin.y + panel.size.height / 2.0;
        assert!(
            (center_x - size.width / 2.0).abs() < 1.0,
            "centro horizontal em {center_x}, esperado {}",
            size.width / 2.0
        );
        assert!(
            (center_y - size.height / 2.0).abs() < 1.0,
            "centro vertical em {center_y}, esperado {}",
            size.height / 2.0
        );
    }

    /// Reler o disco repõe os itens da árvore, e não só a expansão.
    ///
    /// O pacote e a classe eram criados e não apareciam: a `TreeView` guarda os
    /// itens dela, e a IDE relia o `FileNode` sem repô-los.
    #[test]
    fn reloading_the_workspace_shows_what_was_created() {
        let root = std::env::temp_dir().join(format!("er-ide-reload-{}", std::process::id()));
        let package = root.join("src/main/java/br/com");
        assert!(std::fs::create_dir_all(&package).is_ok());
        let Ok(mut shell) = IdeShell::open(&root) else {
            panic!("workspace de teste não abriu");
        };
        shell.reveal_in_explorer(&package);
        let size = Size::new(1280.0, 800.0);
        let shows = |shell: &mut IdeShell, needle: &str| {
            shell.paint(size).iter().any(|command| match command {
                PaintCommand::DrawText(text) => text.text.contains(needle),
                _ => false,
            })
        };
        assert!(!shows(&mut shell, "Pedido"), "a classe ainda não existe");

        assert!(std::fs::write(package.join("Pedido.java"), "class Pedido {}").is_ok());
        assert!(shell.reload_workspace().is_ok());
        shell.reveal_in_explorer(&package);
        assert!(
            shows(&mut shell, "Pedido.java"),
            "a classe criada precisa aparecer na árvore"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Clicar no campo move o cursor, e o que se digita entra ali.
    ///
    /// O clique é entregue ao componente, que conhece a medição da fonte; a IDE
    /// não tenta adivinhar em que caractere o ponteiro caiu.
    #[test]
    fn clicking_a_field_moves_the_cursor_before_typing() {
        let mut shell = shell_with_package();
        let size = Size::new(1_000.0, 700.0);
        shell.explorer.context_menu_target = Some(PathBuf::from("demo/src/main/java/br/com"));
        shell.run_explorer_command("explorer.new.java.package");
        shell.search.new_item_modal.layout(
            &LayoutContext::default(),
            Rect::new(0.0, 0.0, size.width, size.height),
        );
        let geometry = new_item_geometry(shell.search.new_item_modal.panel_bounds());
        // Clique antes do primeiro caractere leva o cursor para o começo.
        shell.pointer_down(
            Point::new(
                geometry.package.origin.x + 1.0,
                geometry.package.origin.y + 8.0,
            ),
            size,
        );
        shell.text_input("dev.");
        shell.key_down("Enter");
        assert_eq!(
            shell.take_new_item_request().map(|request| request.package),
            Some("dev.br.com".to_owned())
        );
    }

    /// Com o nome preenchido, o tipo é pedido dentro do pacote informado.
    #[test]
    fn enter_with_a_name_asks_for_the_type_inside_the_package() {
        let mut shell = shell_with_package();
        shell.explorer.context_menu_target = Some(PathBuf::from("demo/src/main/java/br/com"));
        shell.run_explorer_command("explorer.new.java.interface");
        // Ao criar um tipo o foco já está no nome.
        shell.text_input("Repositorio");
        shell.key_down("Enter");
        assert_eq!(
            shell.take_new_item_request(),
            Some(NewItemRequest {
                template_id: NewItemTemplateId::new("java.interface"),
                package: "br.com".to_owned(),
                name: "Repositorio".to_owned(),
                source_root: PathBuf::from("demo/src/main/java"),
            })
        );
    }

    /// Tab troca o campo, então o pacote também é editável ao criar um tipo.
    #[test]
    fn tab_moves_between_the_two_fields() {
        let mut shell = shell_with_package();
        shell.explorer.context_menu_target = Some(PathBuf::from("demo/src/main/java/br/com"));
        shell.run_explorer_command("explorer.new.java.class");
        shell.key_down("Tab");
        shell.text_input(".exemplo");
        shell.key_down("Tab");
        shell.text_input("Pedido");
        shell.key_down("Enter");
        assert_eq!(
            shell.take_new_item_request(),
            Some(NewItemRequest {
                template_id: NewItemTemplateId::new("java.class"),
                package: "br.com.exemplo".to_owned(),
                name: "Pedido".to_owned(),
                source_root: PathBuf::from("demo/src/main/java"),
            })
        );
    }

    /// Classe sem nome não é pedido válido, e a janela fica aberta dizendo o quê.
    #[test]
    fn a_type_without_a_name_is_refused_without_closing() {
        let mut shell = shell_with_package();
        shell.explorer.context_menu_target = Some(PathBuf::from("demo/src/main/java/br/com"));
        shell.run_explorer_command("explorer.new.java.class");
        shell.key_down("Enter");
        assert_eq!(shell.take_new_item_request(), None);
        assert!(shell.new_item_dialog_open());
        assert_eq!(
            shell
                .search
                .new_item_dialog
                .as_ref()
                .and_then(|dialog| dialog.message.clone()),
            Some("Informe o nome.".to_owned())
        );
    }

    /// Esc fecha sem pedir nada.
    #[test]
    fn escape_closes_the_new_item_dialog_without_creating() {
        let mut shell = shell_with_package();
        shell.explorer.context_menu_target = Some(PathBuf::from("demo/src/main/java/br/com"));
        shell.run_explorer_command("explorer.new.java.class");
        shell.escape();
        assert!(!shell.new_item_dialog_open());
        assert_eq!(shell.take_new_item_request(), None);
    }

    /// Esc é cancelar: fechar sem descartar salvaria pela porta dos fundos.
    #[test]
    fn escape_in_the_settings_discards_every_change() {
        let mut shell = test_shell();
        let size = Size::new(1_000.0, 700.0);
        open_java_settings(&mut shell, vec!["JDK 8".to_owned(), "JDK 17".to_owned()], 0);
        let geometry = open_settings_geometry(&mut shell, size);
        choose_second_jdk(&mut shell, &geometry, size);
        shell.escape();
        assert_eq!(shell.take_settings_jdk_result(), None);
        assert!(!shell.settings_dialog_open());
        assert_eq!(shell.settings.toolchain_combo.selected_index(), 0);
    }

    fn open_settings_geometry(shell: &mut IdeShell, size: Size) -> SettingsDialogGeometry {
        shell.settings.modal.layout(
            &LayoutContext::default(),
            Rect::new(0.0, 0.0, size.width, size.height),
        );
        settings_dialog_geometry(shell.settings.modal.panel_bounds())
    }

    /// Abre o combo e clica na segunda linha.
    fn choose_second_jdk(shell: &mut IdeShell, geometry: &SettingsDialogGeometry, size: Size) {
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
    }
}
