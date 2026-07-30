//! Coordenação dos estados de feature da interface.

#[cfg(test)]
use crate::debugging::DebugFrameView;
use crate::debugging::{
    DebugPanelState, DebugVariableView, DebugView, InspectionFocus, InspectionNode, InspectionRun,
    InspectionState, InspectionView,
};
use crate::text::{
    converted_syntax, count_outline, encloses_type, identifier_prefix, is_identifier_character,
    is_navigable, line_column, offset_for_line_column, offset_of_line, position_in_range, token_at,
};
use crate::ide_shell::generate::{GenerateOutcome, GenerateSurface};
use crate::ide_shell::rename::{RenameOutcome, RenameSurface};
use crate::ide_shell::geometry::{
    InspectionGeometry, SettingsDialogGeometry, inspection_geometry, new_item_geometry,
    settings_dialog_geometry, settings_pages_rect,
};
use crate::editor::{
    CachedSyntax, EditorAction, EditorAreaState, EditorCapabilities, EditorPane, SyntaxView,
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
    ApplicationCommand, DebugRequest, FileOccurrences, NavigationRequest, NewItemRequest,
    NewItemTemplate, OpenDocumentRequest, RenameDocumentRequest, SaveDocumentRequest, TaskId,
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
    AccessorKind, AccessorPlan, CompletionItem, CompletionRequest, DocumentId,
    DocumentSnapshot, Location, OutlineItem, SyntaxSnapshot,
    TextPosition as DomainTextPosition, TextRange as DomainTextRange,
};
use ide_terminal::{ShellKind, TerminalSession};
use ide_workspace::{EditorSession, FileNode, TextBuffer, rewrite_occurrences};
use ui_api::{EventContext, LayoutContext, PaintContext, TextMetrics, Widget};
#[cfg(test)]
use ui_components::MenuEntry;
use ui_components::{
    Button, ComboBox, ComboBoxItem,
    ContextMenu, Icon, IconTint, Label, ListSelection, ListView, MenuBar, MenuBarItem, MenuItem,
    ModalHost, Popup, Scrollbar, ScrollbarOrientation, SplitOrientation, Splitter, StatusBar,
    TabItem, Tabs, TextInput, TreeItem, TreeView,
};
use ui_core::{
    Color, ColorTokens, CommandId, EventResult, FontId, KeyEvent, Modifiers, Point, PointerButton,
    PointerEvent, Rect, Size, TextInputEvent, Theme, UiEvent, WidgetAction, WidgetId,
};
use ui_editor::{CodeEditor, GutterMark, LineDecoration};
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
/// Faixa de ids das células da lista, para não colidir com outros componentes.
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
    /// As janelas de gerar e de renomear, cada uma com seu estado e seus
    /// eventos. Ver `14-ide-shell-decomposition`.
    generate: GenerateSurface,
    rename: RenameSurface,
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
            generate: GenerateSurface::default(),
            rename: RenameSurface::default(),
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
        self.generate.request(kind);
    }

    /// Pedido que a tela quer ver respondido, se houver um esperando.
    pub fn take_accessor_request(&mut self) -> Option<AccessorKind> {
        self.generate.take_request()
    }

    /// Campos escolhidos para o construtor, se houver um pedido esperando.
    pub fn take_constructor_request(&mut self) -> Option<(Vec<String>, DomainTextPosition)> {
        self.generate.take_constructor_request()
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
    pub fn show_accessor_plan(&mut self, kind: AccessorKind, plan: AccessorPlan) {
        if let Some(recusa) = self.generate.show(kind, plan) {
            self.set_status_message(recusa);
        }
    }

    #[must_use]
    pub fn generate_open(&self) -> bool {
        self.generate.is_open()
    }

    /// Campos oferecidos na janela, na ordem em que aparecem.
    #[must_use]
    pub fn generate_fields(&self) -> Vec<String> {
        self.generate.fields()
    }

    pub fn close_generate(&mut self) {
        self.generate.close();
    }

    /// Escreve o que foi escolhido na janela.
    ///
    /// `todos` ignora a marcação — é o botão que gera tudo de uma vez, sem
    /// obrigar a marcar campo por campo quando se quer a classe inteira.
    pub fn apply_generate(&mut self, todos: bool) {
        let outcome = self.generate.confirm(todos);
        self.apply_generate_outcome(outcome);
    }

    /// Executa o que a janela decidiu.
    ///
    /// A janela não alcança a sessão: ela entrega o trecho e a linha, e é aqui
    /// que isso vira texto no documento.
    fn apply_generate_outcome(&mut self, outcome: GenerateOutcome) {
        match outcome {
            GenerateOutcome::Idle => {}
            GenerateOutcome::Message(message) => self.set_status_message(message),
            GenerateOutcome::Insert {
                text,
                line,
                message,
            } => {
                let Some(document) = self.editor_area.session.active_mut() else {
                    return;
                };
                // A linha vem da linguagem, que sabe onde o tipo fecha.
                let inicio = offset_of_line(document.buffer.text(), line as usize);
                if document.buffer.replace(inicio..inicio, &text).is_ok() {
                    self.set_status_message(message);
                }
            }
        }
    }

    /// Pede a renomeação do arquivo escolhido na árvore.
    ///
    /// O menu só marca; quem responde é a aplicação, que pergunta à linguagem
    /// onde o nome aparece no projeto — inclusive em arquivos fechados.
    pub fn request_rename(&mut self, path: PathBuf) {
        self.rename.request(path);
    }

    /// Arquivo cuja renomeação a tela quer ver respondida, se houver um.
    pub fn take_rename_request(&mut self) -> Option<PathBuf> {
        self.rename.take_request()
    }

    /// Abre a janela de renomear com o arquivo e o que será reescrito junto.
    pub fn show_rename(&mut self, path: PathBuf, references: Vec<Location>) {
        self.rename.show(path, references);
    }

    #[must_use]
    pub fn rename_open(&self) -> bool {
        self.rename.is_open()
    }

    /// Nome que está no campo, que é o que a confirmação aplica.
    #[must_use]
    pub fn rename_name(&self) -> String {
        self.rename.name()
    }

    /// Arquivos afetados, como aparecem na lista.
    #[must_use]
    pub fn rename_references(&self) -> Vec<String> {
        self.rename.references()
    }

    pub fn cancel_rename(&mut self) {
        self.rename.cancel();
    }

    /// Confirma a renomeação pendente na janela.
    pub fn apply_rename(&mut self) {
        let outcome = self.rename.confirm();
        self.apply_rename_outcome(outcome);
    }

    /// Executa o que a janela decidiu.
    ///
    /// A janela não alcança a sessão nem a fila de comandos: ela entrega a
    /// decisão, e é aqui que ela vira texto trocado e pedido à aplicação. O que
    /// está aberto é reescrito no buffer — a aba mantém cursor, desfazer e
    /// alterações não salvas —, e o que está fechado vai para o disco.
    fn apply_rename_outcome(&mut self, outcome: RenameOutcome) {
        let decision = match outcome {
            RenameOutcome::Idle => return,
            RenameOutcome::Message(message) => {
                self.set_status_message(message);
                return;
            }
            RenameOutcome::Apply(decision) => decision,
        };
        let arquivos = decision.occurrences.len();
        let mut fechados = Vec::new();
        for (caminho, ranges) in decision.occurrences {
            let aberto = self
                .editor_area
                .session
                .tabs()
                .find(|documento| documento.path == caminho)
                .map(|documento| documento.id);
            match aberto {
                Some(id) => {
                    self.rewrite_open_document(id, &ranges, &decision.old_name, &decision.new_name);
                }
                None => fechados.push(FileOccurrences {
                    path: caminho,
                    ranges,
                }),
            }
        }
        self.commands
            .push(ApplicationCommand::RenameDocument(RenameDocumentRequest {
                from: decision.from,
                to: decision.to,
                old_name: decision.old_name,
                new_name: decision.new_name,
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
        let context = self.layout_context();
        let outcome = self.rename.pointer_down(&context, point, size);
        self.apply_rename_outcome(outcome);
    }

    /// Movimento e soltura vão à janela: são o arrasto das barras da lista.
    fn rename_pointer_event(&mut self, event: &UiEvent, size: Size) {
        let context = self.layout_context();
        self.rename.pointer_event(&context, event, size);
    }

    /// Teclas da janela de renomear: `Enter` confirma, `Esc` desiste.
    fn rename_key(&mut self, key: &str, modifiers: Modifiers) -> bool {
        if !self.rename.is_open() {
            return false;
        }
        let outcome = self.rename.key(key, modifiers);
        self.apply_rename_outcome(outcome);
        true
    }

    /// Texto digitado enquanto a janela de renomear está aberta.
    fn rename_text_input(&mut self, text: &str) -> bool {
        if !self.rename.is_open() {
            return false;
        }
        self.rename.text_input(text);
        true
    }

    fn paint_rename(&mut self, commands: &mut Vec<PaintCommand>, size: Size) {
        let layout = self.layout_context();
        let mut paint = self.paint_context();
        if self.rename.paint(&layout, &mut paint, size) {
            commands.extend(paint.into_commands());
        }
    }

    fn generate_pointer_down(&mut self, point: Point, size: Size) {
        let context = self.layout_context();
        let outcome = self.generate.pointer_down(&context, point, size);
        self.apply_generate_outcome(outcome);
    }

    fn paint_generate(&mut self, commands: &mut Vec<PaintCommand>, size: Size) {
        let layout = self.layout_context();
        let mut paint = self.paint_context();
        if self.generate.paint(&layout, &mut paint, size) {
            commands.extend(paint.into_commands());
        }
    }

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
        if self.rename.is_open() {
            self.cancel_rename();
            return;
        }
        if self.generate.is_open() {
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
        if self.rename.is_open() {
            self.rename_pointer_down(point, size);
            return;
        }
        if self.generate.is_open() {
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
            let action =
                self.editor_area
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
        if self.rename.is_open() {
            self.rename_pointer_event(&UiEvent::PointerMove(primary_pointer(point)), size);
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
        if self.rename.is_open() {
            let size = self.context.last_size;
            self.rename_pointer_event(&UiEvent::PointerUp(primary_pointer(Point::ZERO)), size);
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
        if self.rename.is_open() {
            self.rename_pointer_event(
                &UiEvent::Scroll(ui_core::ScrollEvent {
                    position: point,
                    delta_x: 0.0,
                    delta_y: delta_lines * generate::ROW_HEIGHT,
                }),
                size,
            );
            return;
        }
        // A janela de geração cobre tudo: a roda ali é dela.
        if self.generate.is_open() {
            let context = self.layout_context();
            self.generate.scroll(&context, point, delta_lines, size);
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

mod generate;
mod geometry;
mod rename;

#[cfg(test)]
mod tests;
