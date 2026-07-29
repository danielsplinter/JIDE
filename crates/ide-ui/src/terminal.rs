//! Estado de abas, seleção e rolagem do terminal.

use ide_terminal::TerminalSession;

#[derive(Clone, Copy)]
pub(super) struct TextPosition {
    pub(super) line: usize,
    pub(super) column: usize,
}

#[derive(Clone, Copy)]
pub(super) struct TerminalSelection {
    pub(super) anchor: TextPosition,
    pub(super) focus: TextPosition,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ScrollTarget {
    Editor,
    Terminal,
    ExplorerHorizontal,
    ExplorerVertical,
}

pub(super) struct TerminalTab {
    pub(super) session: TerminalSession,
    pub(super) scroll_line: usize,
    pub(super) follow_output: bool,
}

pub(super) fn ordered_selection(selection: TerminalSelection) -> (TextPosition, TextPosition) {
    if (selection.anchor.line, selection.anchor.column)
        <= (selection.focus.line, selection.focus.column)
    {
        (selection.anchor, selection.focus)
    } else {
        (selection.focus, selection.anchor)
    }
}

pub(super) fn selection_columns(
    selection: Option<TerminalSelection>,
    line: usize,
    text: &str,
) -> Option<(usize, usize)> {
    let selection = selection?;
    let (start, end) = ordered_selection(selection);
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
