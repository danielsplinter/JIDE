//! A casca da interface: quem existe, onde fica, e de quem é o gesto.
//!
//! O que sobrou aqui é composição — o `IdeShell` com um campo por área, a
//! geometria da janela, o funil que decide qual superfície recebe o evento, e as
//! entradas que a aplicação chama. Cada área leva o que é dela nos módulos
//! abaixo: as janelas em `rename`, `generate`, `type_search`, `new_item`,
//! `settings` e `inspection`; os painéis em `editor_area`, `explorer_area`,
//! `terminal_area` e `debug_area`; e o quadro inteiro em `painting`.
//!
//! Ver `14-ide-shell-decomposition`.

#[cfg(test)]
use crate::debugging::DebugFrameView;
use crate::debugging::{DebugPanelState, DebugView};
use crate::ide_shell::generate::GenerateSurface;
use crate::ide_shell::inspection::InspectionSurface;
use crate::ide_shell::new_item::NewItemSurface;
use crate::ide_shell::rename::RenameSurface;
use crate::ide_shell::settings::SettingsSurface;
use crate::text::{
    converted_syntax, count_outline, encloses_type, identifier_prefix, is_identifier_character,
    is_navigable, line_column, offset_for_line_column, offset_of_line, position_in_range, token_at,
};

use crate::editor::{
    CachedSyntax, EditorAction, EditorAreaState, EditorCapabilities, EditorPane, SyntaxView,
};
use crate::explorer::{
    ExplorerState, id as explorer_id, items as explorer_items, visible_row as visible_tree_row,
};
use crate::ide_shell::type_search::TypeSearchSurface;

use crate::settings::SettingsPage;
use crate::shell::{ShellCommandQueue, ShellFocus};
use crate::terminal::{
    ScrollTarget, TerminalPanelState, TerminalSelection, TerminalTab, TextPosition,
    ordered_selection, selection_columns,
};
use ide_application::{
    ApplicationCommand, DebugRequest, FileOccurrences, NavigationRequest, OpenDocumentRequest,
    RenameDocumentRequest, SaveDocumentRequest, TaskId, UiContributionCatalog,
};
#[cfg(test)]
use ide_application::{NewItemTemplateId, SettingsSection, TaskDescriptor};

use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
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
use ide_domain::{
    AccessorKind, CompletionItem, CompletionRequest, DocumentId, DocumentSnapshot, OutlineItem,
    SyntaxSnapshot, TextPosition as DomainTextPosition, TextRange as DomainTextRange,
};
use ide_terminal::{ShellKind, TerminalSession};
use ide_workspace::{EditorSession, FileNode, TextBuffer, rewrite_occurrences};
use ui_api::{EventContext, LayoutContext, PaintContext, TextMetrics, Widget};
#[cfg(test)]
use ui_components::MenuEntry;
use ui_components::{
    Button, ContextMenu, Icon, IconTint, ListView, MenuBar, MenuBarItem, MenuItem, Popup,
    Scrollbar, ScrollbarOrientation, SplitOrientation, Splitter, StatusBar, TabItem, Tabs,
    TextInput, TreeView,
};
use ui_core::{
    Color, ColorTokens, CommandId, EventResult, FontId, KeyEvent, Modifiers, Point, PointerButton,
    PointerEvent, Rect, Size, Theme, UiEvent, WidgetAction, WidgetId,
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
/// Faixa de ids das células da lista, para não colidir com outros componentes.
/// A janela é larga porque o valor de um objeto costuma ser longo.
/// Fatia da janela ocupada pela lista, à esquerda.
const INSPECTION_LIST_FRACTION: f32 = 0.42;
/// Fatia do painel direito ocupada pelo detalhe; o resto é o editor.
const INSPECTION_DETAIL_FRACTION: f32 = 0.45;
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

/// As janelas que cobrem a tela, da que fica por cima para a que fica embaixo.
///
/// A ordem é a profundidade: a primeira aberta é a que recebe o gesto, e as de
/// baixo nem são consultadas. Registrar uma janela nova é acrescentar uma linha
/// aqui e um braço em cada `match` do funil — o roteamento não precisa saber que
/// ela existe. Ver `14-ide-shell-decomposition`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SurfaceKind {
    Rename,
    Generate,
    TypeSearch,
    Inspection,
    NewItem,
    Settings,
}

const SURFACES: [SurfaceKind; 6] = [
    SurfaceKind::Rename,
    SurfaceKind::Generate,
    SurfaceKind::TypeSearch,
    SurfaceKind::Inspection,
    SurfaceKind::NewItem,
    SurfaceKind::Settings,
];

/// Onde a lista de completação flutua nessa pilha.
///
/// Ela não é uma janela: nasce colada ao cursor e convive com o que estiver
/// aberto. Mas tem profundidade — cobre a inspeção e o editor, e é coberta pelas
/// janelas que tomam a tela inteira. É esse número que diz até onde ela cobre.
const COMPLETION_DEPTH: usize = 3;

/// Coordenador da interface. Cada feature é dona de seus widgets e seleção.
pub struct IdeShell {
    explorer: ExplorerState,
    editor_area: EditorAreaState,
    terminal: TerminalPanelState,
    search: TypeSearchSurface,
    inspection: InspectionSurface,
    settings: SettingsSurface,
    debug_panel: DebugPanelState,
    /// As janelas de gerar e de renomear, cada uma com seu estado e seus
    /// eventos. Ver `14-ide-shell-decomposition`.
    generate: GenerateSurface,
    new_item: NewItemSurface,
    rename: RenameSurface,
    menu: MenuState,
    catalog: UiContributionCatalog,
    context: ShellContext,
    commands: ShellCommandQueue,
}
impl IdeShell {
    /// Mensagem atual da barra de estado.
    #[must_use]
    pub fn status_message(&self) -> &str {
        &self.context.status_message
    }

    pub fn set_status_message(&mut self, message: impl Into<String>) {
        self.context.status_message = message.into();
    }

    #[must_use]
    pub fn ui_catalog(&self) -> &UiContributionCatalog {
        &self.catalog
    }

    /// Retira, em ordem, todas as intenções produzidas desde a última consulta.
    pub fn drain_application_commands(&mut self) -> Vec<ApplicationCommand> {
        self.commands.drain()
    }

    pub fn tab_count(&self) -> usize {
        self.editor_area.session.tabs().count()
    }

    /// Texto de um documento aberto, para quem vai gravá-lo.
    #[must_use]
    pub fn document_text(&self, document_id: DocumentId) -> Option<String> {
        self.editor_area
            .session
            .document(document_id)
            .map(|document| document.buffer.text().to_owned())
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

    /// A janela aberta que está por cima, se houver alguma.
    ///
    /// É por aqui que cada entrada de evento pergunta a quem o gesto pertence,
    /// em vez de repetir a cadeia de `if` e um dia repeti-la torta.
    fn open_surface(&self) -> Option<SurfaceKind> {
        SURFACES
            .into_iter()
            .find(|kind| self.surface_is_open(*kind))
    }

    /// Profundidade da janela, para comparar com a da lista de completação.
    fn surface_depth(kind: SurfaceKind) -> usize {
        SURFACES
            .iter()
            .position(|candidate| *candidate == kind)
            .unwrap_or(usize::MAX)
    }

    /// Se a lista de completação está por cima do que quer que esteja aberto.
    fn completion_is_above(&self) -> bool {
        self.open_surface().map_or(usize::MAX, Self::surface_depth) >= COMPLETION_DEPTH
    }

    fn surface_is_open(&self, kind: SurfaceKind) -> bool {
        match kind {
            SurfaceKind::Rename => self.rename.is_open(),
            SurfaceKind::Generate => self.generate.is_open(),
            SurfaceKind::TypeSearch => self.search.is_open(),
            SurfaceKind::Inspection => self.inspection.is_open(),
            SurfaceKind::NewItem => self.new_item.is_open(),
            SurfaceKind::Settings => self.settings.is_open(),
        }
    }

    /// Esc na janela: cada uma decide o que desistir significa para ela.
    fn surface_escape(&mut self, kind: SurfaceKind) {
        match kind {
            SurfaceKind::Rename => self.cancel_rename(),
            SurfaceKind::Generate => self.close_generate(),
            SurfaceKind::TypeSearch => self.close_type_search(),
            SurfaceKind::Inspection => self.close_inspection(),
            SurfaceKind::NewItem => self.close_new_item_dialog(),
            // Esc nas configurações é cancelar: fechar sem descartar o que foi
            // mexido salvaria pela porta dos fundos.
            SurfaceKind::Settings => self.settings.cancel(),
        }
    }

    fn surface_pointer_down(&mut self, kind: SurfaceKind, point: Point, size: Size) {
        match kind {
            SurfaceKind::Rename => self.rename_pointer_down(point, size),
            SurfaceKind::Generate => self.generate_pointer_down(point, size),
            SurfaceKind::TypeSearch => self.type_search_pointer_down(point, size),
            SurfaceKind::Inspection => self.inspection_pointer_down(point, size),
            SurfaceKind::NewItem => self.new_item_pointer_down(point, size),
            SurfaceKind::Settings => self.settings_dialog_pointer_down(point, size),
        }
    }

    /// Movimento do ponteiro. `None` é "não é meu, siga adiante".
    fn surface_pointer_move(
        &mut self,
        kind: SurfaceKind,
        point: Point,
        size: Size,
    ) -> Option<bool> {
        match kind {
            // Arrastar a barra da lista precisa do movimento: só com o clique
            // chegando, o indicador é pego e nunca anda.
            SurfaceKind::Rename => {
                self.rename_pointer_event(&UiEvent::PointerMove(primary_pointer(point)), size);
                Some(true)
            }
            // A janela cobre o que está atrás: mover ali não arrasta nada.
            SurfaceKind::Settings => Some(false),
            _ => None,
        }
    }

    /// Soltura do ponteiro, que é o que encerra um arrasto começado na janela.
    fn surface_pointer_up(&mut self, kind: SurfaceKind) {
        if kind == SurfaceKind::Rename {
            let size = self.context.last_size;
            self.rename_pointer_event(&UiEvent::PointerUp(primary_pointer(Point::ZERO)), size);
        }
    }

    /// A roda dentro da janela. `false` deixa passar para o que está atrás.
    fn surface_scroll(
        &mut self,
        kind: SurfaceKind,
        point: Point,
        delta_lines: f32,
        size: Size,
    ) -> bool {
        match kind {
            // A janela cobre tudo: a roda ali é da lista dela, e nunca do editor
            // atrás — rolar o que está coberto é mexer no que não se vê.
            SurfaceKind::Rename => {
                self.rename_pointer_event(
                    &UiEvent::Scroll(ui_core::ScrollEvent {
                        position: point,
                        delta_x: 0.0,
                        delta_y: delta_lines * generate::ROW_HEIGHT,
                    }),
                    size,
                );
                true
            }
            SurfaceKind::Generate => {
                let context = self.layout_context();
                self.generate.scroll(&context, point, delta_lines, size);
                true
            }
            SurfaceKind::TypeSearch => {
                self.type_search_scroll(point, delta_lines, size);
                true
            }
            SurfaceKind::Settings => true,
            SurfaceKind::Inspection | SurfaceKind::NewItem => false,
        }
    }

    /// Tecla na janela. `false` deixa a tecla seguir para quem estiver atrás.
    fn surface_key(&mut self, kind: SurfaceKind, key: &str, modifiers: Modifiers) -> bool {
        match kind {
            SurfaceKind::Rename => self.rename_key(key, modifiers),
            SurfaceKind::Generate => false,
            SurfaceKind::TypeSearch => self.type_search_key(key),
            SurfaceKind::Inspection => self.inspection_key(key, modifiers),
            SurfaceKind::NewItem => self.new_item_key(key),
            SurfaceKind::Settings => {
                let outcome = self.settings.key(key, modifiers);
                self.apply_settings_outcome(outcome);
                true
            }
        }
    }

    /// Se o painel de edição da frente é o de uma janela, e não o do arquivo.
    ///
    /// O que está atrás dela está coberto: arrastar a barra de rolagem ou marcar
    /// no terminal seria o gesto indo parar no que não se vê.
    fn front_editor_is_modal(&self) -> bool {
        self.inspection.is_open()
    }

    /// Texto digitado na janela. `false` deixa o texto seguir adiante.
    fn surface_text_input(&mut self, kind: SurfaceKind, text: &str) -> bool {
        match kind {
            SurfaceKind::Rename => self.rename_text_input(text),
            SurfaceKind::Generate => false,
            SurfaceKind::TypeSearch => self.type_search_text_input(text),
            SurfaceKind::Inspection => self.inspection_text_input(text),
            SurfaceKind::NewItem => self.new_item_text_input(text),
            SurfaceKind::Settings => self.settings.text_input(text),
        }
    }

    pub fn escape(&mut self) {
        // O menu de contexto é o que está por cima de tudo: é ele que Esc
        // dispensa primeiro.
        if self.context_menu_key("Escape", Modifiers::default()) {
            return;
        }
        if let Some(surface) = self.open_surface() {
            self.surface_escape(surface);
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
        // A lista de completação vem antes das janelas que ela cobre: clicar em
        // outro lugar significa desistir dela.
        if self.completion_is_above() && self.completion_pointer_down(point, size) {
            return;
        }
        if let Some(surface) = self.open_surface() {
            self.surface_pointer_down(surface, point, size);
            return;
        }
        if point.y < TITLE_HEIGHT && self.action_buttons_pointer_down(point, size) {
            return;
        }
        if self.menu_bar_pointer_down(point, size) {
            return;
        }
        if point.y < TITLE_HEIGHT {
            return;
        }
        if self.terminal_toggle_pointer_down(point, size) {
            return;
        }
        if self.scrollbar_pointer_down(point, size) {
            return;
        }
        if self.splitter_pointer_down(point, size) {
            return;
        }
        if self.editor_tabs_pointer_down(point, size) {
            return;
        }
        if self.explorer_pointer_down(point, size) {
            return;
        }
        if self.debug_panel_area_pointer_down(point, size) {
            return;
        }
        if self.editor_pointer_down(point, size, control, shift) {
            return;
        }
        self.terminal_area_pointer_down(point, size);
    }

    pub fn pointer_move(&mut self, point: Point, size: Size) -> bool {
        self.context.pointer = point;
        // Com o menu aberto, o destaque acompanha o ponteiro dentro dele.
        if self.explorer.context_menu.is_open() {
            return self.context_menu_event(&UiEvent::PointerMove(primary_pointer(point)), size);
        }
        if let Some(surface) = self.open_surface()
            && let Some(handled) = self.surface_pointer_move(surface, point, size)
        {
            return handled;
        }
        // Com a inspeção aberta, o gesto é dela: o resto da janela está atrás do
        // painel, e arrastar sobre o que não se vê seria o gesto indo parar no
        // lugar errado.
        let inspecting = self.front_editor_is_modal();
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

    pub fn pointer_up(&mut self) {
        // A soltura encerra o arrasto começado na janela: sem ela a lista
        // continuaria achando que o gesto está em curso e seguiria o ponteiro.
        if let Some(surface) = self.open_surface() {
            self.surface_pointer_up(surface);
        }
        // Encerrar o gesto é do painel, que sabe se ele virou seleção.
        self.editor_area.pane.pointer_up();
        self.inspection.editor_and_source().0.pointer_up();
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
        if let Some(surface) = self.open_surface()
            && self.surface_scroll(surface, point, delta_lines, size)
        {
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

    pub fn text_input(&mut self, text: &str) {
        if let Some(surface) = self.open_surface()
            && self.surface_text_input(surface, text)
        {
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
        // A janela aberta fica com as teclas — a menos que ela devolva o que não
        // é dela, como a inspeção faz quando o foco está na árvore.
        if let Some(surface) = self.open_surface()
            && self.surface_key(surface, key, modifiers)
        {
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

    fn paint_inspection(&self, commands: &mut Vec<PaintCommand>, size: Size) {
        let layout = self.layout_context();
        let mut paint = self.paint_context();
        let attached = self.debug_panel.view.attached;
        if self.inspection.paint(&layout, &mut paint, size, attached) {
            commands.extend(paint.into_commands());
        }
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

/// Entrega o clique ao componente de abas e devolve o que ele decidiu.
///
/// Um clique é pressionar e soltar; a interface da IDE só encaminha o
/// pressionar, então os dois eventos vão juntos. O que volta é a ação do
/// componente — a identidade da aba, e não um texto para desmontar.
fn tab_action(tabs: &mut Tabs, point: Point) -> Option<WidgetAction> {
    let mut context = EventContext::default();
    let event = UiEvent::PointerDown(primary_pointer(point));
    tabs.event(&mut context, &event);
    match tabs.event(&mut context, &UiEvent::PointerUp(primary_pointer(point))) {
        EventResult::Action(action) => Some(action),
        _ => None,
    }
}

mod build;
mod debug_area;
mod documents;
mod editor_area;
mod explorer_area;
mod generate;
mod geometry;
mod inspection;
mod menu_bar;
mod new_item;
mod painting;
mod rename;
mod settings;
mod surfaces;
mod terminal_area;
mod type_search;

#[cfg(test)]
mod tests;
