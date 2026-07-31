//! O painel de terminais: abas, rolagem, seleção e o que é enviado.

use super::*;

impl IdeShell {
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

    pub fn append_tool_output(&mut self, text: &str, is_error: bool) {
        let active = self.terminal.active;
        self.terminal.tabs[active]
            .session
            .append_external_output(text, is_error);
        self.terminal.tabs[active].follow_output = true;
        self.terminal.tabs[active].scroll_line = self.terminal.tabs[active].session.line_count();
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

    pub fn active_terminal_lines(&self) -> impl Iterator<Item = &str> {
        self.terminal.active_lines()
    }

    pub fn active_terminal_input(&self) -> &str {
        self.active_terminal().input()
    }

    pub(super) fn active_terminal(&self) -> &TerminalSession {
        self.terminal.active()
    }

    pub(super) fn active_terminal_mut(&mut self) -> &mut TerminalSession {
        self.terminal.active_session_mut()
    }

    pub fn update_terminals(&mut self) -> bool {
        let geo = self.geometry();
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

    /// Repõe no anfitrião a apresentação das abas do terminal e a área delas.
    pub(super) fn sync_terminal_tabs(&mut self) {
        let tabs = self.terminal_tabs();
        self.host.replace(Box::new(tabs));
    }

    pub(super) fn terminal_splitter_for(&self, size: Size) -> Splitter {
        let geometry = self.geometry();
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

    pub(super) fn terminal_visible_lines(&self) -> usize {
        ((self.geometry().terminal_height - 62.0) / EDITOR_LINE_HEIGHT)
            .floor()
            .max(1.0) as usize
    }

    pub(super) fn terminal_scrollbar_rect(&self, size: Size) -> Rect {
        let geo = self.geometry();
        let editor_x = ACTIVITY_WIDTH + self.sidebar_width(size);
        Rect::new(
            editor_x + geo.editor_width - 10.0,
            geo.editor_bottom + 60.0,
            10.0,
            (geo.terminal_height - 60.0).max(0.0),
        )
    }

    /// Linha e coluna do texto sob um ponto do terminal.
    ///
    /// O tamanho da janela não entra mais na conta: quem sabe onde a saída está e
    /// quanto mede um caractere é o console, desde a última pintura.
    pub(super) fn terminal_position_at(&self, point: Point, _size: Size) -> TextPosition {
        let active = &self.terminal.tabs[self.terminal.active];
        // Quem responde a coluna é o console, pela mesma medição com que
        // desenhou: estimar a largura do caractere aqui punha o clique numa
        // coluna e o realce noutra.
        let (linha, column) = self.terminal.console.position_at(point);
        let line = linha.min(active.session.line_count().saturating_sub(1));
        let line_length = active
            .session
            .lines()
            .nth(line)
            .map_or(0, |value| value.text.chars().count());
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

    /// O botão que recolhe e restaura o painel de terminais.
    pub(super) fn terminal_toggle_pointer_down(&mut self, point: Point, size: Size) -> bool {
        let geometry = self.geometry();
        let toggle = Rect::new(size.width - 30.0, geometry.editor_bottom + 4.0, 22.0, 22.0);
        if !toggle.contains(point) {
            return false;
        }
        if self.terminal.minimized {
            self.terminal.minimized = false;
            self.terminal.height = self.terminal.last_height;
        } else {
            self.terminal.last_height = self.terminal.height;
            self.terminal.minimized = true;
        }
        true
    }

    /// Clique no painel de terminais: a aba escolhida, ou o começo de uma marca.
    pub(super) fn terminal_area_pointer_down(&mut self, point: Point, size: Size) {
        let geometry = self.geometry();
        let editor_x = ACTIVITY_WIDTH + self.sidebar_width(size);
        if point.x < editor_x || point.y < geometry.editor_bottom {
            return;
        }
        self.context.focus = ShellFocus::Terminal;
        if point.y < geometry.editor_bottom + TERMINAL_TAB_HEIGHT {
            self.place_overlay(size);
            if let Some(WidgetAction::TabSelected { tab, .. }) = tab_action(&mut self.host, point) {
                self.terminal.active = tab as usize;
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
