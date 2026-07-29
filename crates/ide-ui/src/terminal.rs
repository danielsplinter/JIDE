//! Estado de abas, seleção e rolagem do terminal.

use ide_terminal::{ShellKind, TerminalSession};
use ui_components::{Scrollbar, Splitter};

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

/// Estado do painel de terminais e de sua interação.
pub(super) struct TerminalPanelState {
    pub(super) tabs: Vec<TerminalTab>,
    pub(super) active: usize,
    pub(super) height: f32,
    pub(super) last_height: f32,
    pub(super) minimized: bool,
    pub(super) splitter: Splitter,
    pub(super) scrollbar: Scrollbar,
    pub(super) selection: Option<TerminalSelection>,
    pub(super) selecting: bool,
    pub(super) running_terminal: Option<usize>,
}

impl TerminalPanelState {
    #[must_use]
    pub(super) fn active_session(&self) -> &TerminalSession {
        &self.tabs[self.active].session
    }

    #[must_use]
    pub(super) fn active(&self) -> &TerminalSession {
        self.active_session()
    }

    pub(super) fn active_session_mut(&mut self) -> &mut TerminalSession {
        &mut self.tabs[self.active].session
    }

    #[must_use]
    pub(super) fn selected_shell(&self) -> ShellKind {
        self.active_session().selected_profile().kind
    }

    pub(super) fn active_lines(&self) -> impl Iterator<Item = &str> {
        self.active_session().lines().map(|line| line.text.as_str())
    }

    pub(super) fn run(&mut self, command: &str) -> Result<(), String> {
        let active = self.active;
        let Some(tab) = self.tabs.get_mut(active) else {
            return Err("Nenhum terminal disponível".to_owned());
        };
        tab.session
            .run(command)
            .map_err(|error| error.to_string())?;
        tab.follow_output = true;
        self.minimized = false;
        self.running_terminal = Some(active);
        Ok(())
    }
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
