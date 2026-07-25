#![doc = "Shell visual e interativo da IDE baseado no ERLibUi."]

use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

use ide_domain::DocumentId;
use ide_terminal::{ShellKind, TerminalSession};
use ide_text::EditorSession;
use ide_workspace::{FileNode, WorkspaceError};
use ui_core::{Color, FontId, Point, Rect, Size};
use ui_render_api::{DrawTextCommand, FillRectCommand, PaintCommand, StrokeRectCommand};

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
const FILE_MENU_RECT: Rect = Rect::new(82.0, 0.0, 72.0, TITLE_HEIGHT);
const PROJECT_MENU_RECT: Rect = Rect::new(82.0, TITLE_HEIGHT, 180.0, 32.0);

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
    file_menu_open: bool,
    open_project_requested: bool,
    pending_navigation: Option<NavigationRequest>,
    status_message: String,
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
            file_menu_open: false,
            open_project_requested: false,
            pending_navigation: None,
            status_message: "Ready".to_owned(),
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
        geometry(
            size,
            if self.terminal_minimized {
                TERMINAL_COLLAPSED_HEIGHT
            } else {
                self.terminal_height
            },
            self.sidebar_width(size),
        )
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
        if self.focus == ShellFocus::Search {
            self.search_query.clear();
            self.focus = ShellFocus::Editor;
        }
    }

    pub fn pointer_down(&mut self, point: Point, size: Size) {
        self.pointer_down_with_modifiers(point, size, false);
    }

    pub fn pointer_down_with_modifiers(&mut self, point: Point, size: Size, control: bool) {
        if FILE_MENU_RECT.contains(point) {
            self.file_menu_open = !self.file_menu_open;
            return;
        }
        if self.file_menu_open && PROJECT_MENU_RECT.contains(point) {
            self.file_menu_open = false;
            self.open_project_requested = true;
            self.status_message = "Select a project folder".to_owned();
            return;
        }
        self.file_menu_open = false;
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
        if point.x >= editor_x
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
        match self.focus {
            ShellFocus::Editor => self.edit_active(text),
            ShellFocus::Search => self.search_query.push_str(text),
            ShellFocus::Terminal => self.active_terminal_mut().input_mut().push_str(text),
            _ => {}
        }
    }

    pub fn key_down(&mut self, key: &str) {
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
        if let Some(document) = self.editor.active_mut() {
            let cursor = self.cursor_offset.min(document.buffer.text().len());
            if document.buffer.replace(cursor..cursor, text).is_ok() {
                self.cursor_offset = cursor + text.len();
                self.status_message = "Modified".to_owned();
            }
        }
    }

    fn backspace(&mut self) {
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
        let colors = Colors::default();
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
            fill(
                Rect::new(0.0, geo.content_bottom, size.width, 24.0),
                colors.accent,
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
                colors.muted,
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
                colors.muted,
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
        if let Some(text) = self.active_text() {
            let visible = (geo.editor_height / EDITOR_LINE_HEIGHT).ceil() as usize;
            for (index, line) in text
                .lines()
                .skip(self.editor_scroll_line)
                .take(visible)
                .enumerate()
            {
                let y = geo.content_top + 15.0 + index as f32 * EDITOR_LINE_HEIGHT;
                commands.push(label(
                    &(index + self.editor_scroll_line + 1).to_string(),
                    Point::new(editor_x + 12.0, y),
                    colors.muted,
                    13.0,
                ));
                commands.push(label(
                    line,
                    Point::new(editor_x + EDITOR_GUTTER, y),
                    syntax_color(line, colors.text, colors.accent, colors.muted),
                    15.0,
                ));
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
                colors.muted,
                16.0,
            ));
        }
        commands.push(PaintCommand::PopClip);
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
                        colors.muted
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
                "{}  •  UTF-8  •  Ln {}, Col {}",
                self.status_message,
                position.0 + 1,
                position.1 + 1
            ),
            Point::new(12.0, geo.content_bottom + 5.0),
            Color::rgba(1.0, 1.0, 1.0, 1.0),
            12.0,
        ));
        commands.push(label(
            "Arquivo",
            Point::new(FILE_MENU_RECT.origin.x + 10.0, 9.0),
            colors.text,
            14.0,
        ));
        if self.file_menu_open {
            commands.push(fill(PROJECT_MENU_RECT, colors.elevated));
            commands.push(stroke(PROJECT_MENU_RECT, colors.border));
            commands.push(label(
                "Projeto...",
                Point::new(
                    PROJECT_MENU_RECT.origin.x + 12.0,
                    PROJECT_MENU_RECT.origin.y + 8.0,
                ),
                colors.text,
                14.0,
            ));
        }
        commands
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

fn is_identifier_character(character: char) -> bool {
    character == '_' || character.is_alphanumeric()
}

#[derive(Clone, Copy)]
struct Colors {
    background: Color,
    surface: Color,
    elevated: Color,
    border: Color,
    text: Color,
    muted: Color,
    accent: Color,
}
impl Default for Colors {
    fn default() -> Self {
        Self {
            background: Color::rgba(0.055, 0.067, 0.09, 1.0),
            surface: Color::rgba(0.075, 0.09, 0.12, 1.0),
            elevated: Color::rgba(0.10, 0.12, 0.16, 1.0),
            border: Color::rgba(0.18, 0.21, 0.28, 1.0),
            text: Color::rgba(0.86, 0.89, 0.95, 1.0),
            muted: Color::rgba(0.55, 0.60, 0.70, 1.0),
            accent: Color::rgba(0.30, 0.55, 0.96, 1.0),
        }
    }
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
    colors: Colors,
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
        colors.muted,
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
    colors: Colors,
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
        colors.muted,
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
}
