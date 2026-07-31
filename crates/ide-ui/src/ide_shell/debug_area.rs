//! O painel de depuração: pontos de parada, quadros e os botões de ação.

use super::*;

impl IdeShell {
    /// Alvo de depuração apresentado na janela e usado pelo botão de depurar.
    pub fn set_debug_target(&mut self, host: &str, port: u16) {
        self.settings.set_debug_target(host, port);
    }

    #[must_use]
    pub fn debug_target(&self) -> Option<(String, u16)> {
        self.settings.debug_target()
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

    pub(super) fn debug_panel_pointer_down(&mut self, point: Point) {
        let geometry = self.debug_panel_geometry();
        // O gesto vai ao botão de verdade: é ele que guarda a pressão em curso.
        let parado = self.debug_panel.view.is_stopped();
        let areas: Vec<_> = geometry.buttons.to_vec();
        let context = self.layout_context();
        for (index, rect) in areas.iter().enumerate() {
            let Some(button) = self.debug_panel.step_buttons.get_mut(index) else {
                continue;
            };
            button.set_disabled(!parado);
            button.layout(&context, *rect);
            if !matches!(click_widget(button, point), EventResult::Ignored)
                && let Some((_, request)) = DEBUG_BUTTONS.get(index)
            {
                self.commands
                    .push(ApplicationCommand::Debug(request.clone()));
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

    /// Tipo em execução de um nome visível no quadro parado.
    ///
    /// A árvore de inspeção vem primeiro porque é o objeto que o usuário mandou
    /// inspecionar; as variáveis do quadro cobrem o resto do que está no escopo.
    pub(super) fn debug_type_of(&self, name: &str) -> Option<String> {
        if let Some(type_name) = self.inspection.type_of(name) {
            return Some(type_name);
        }
        self.debug_panel
            .view
            .variables
            .iter()
            .find(|variable| variable.name == name)
            .and_then(|variable| variable.type_name.clone())
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
    pub(super) fn action_buttons_pointer_down(&mut self, point: Point) -> bool {
        let rects = self.action_button_areas();
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
    pub(super) fn request_run_and_attach(&mut self) {
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
                self.settings.set_page(SettingsPage::Debug);
                self.commands.push(ApplicationCommand::OpenSettings);
                self.context.status_message =
                    "Informe um host e uma porta de depuração válidos".to_owned();
            }
        }
    }

    /// Clique na faixa do painel de depuração, quando há sessão conectada.
    pub(super) fn debug_panel_area_pointer_down(&mut self, point: Point, size: Size) -> bool {
        let geometry = self.geometry();
        let editor_x = ACTIVITY_WIDTH + self.sidebar_width(size);
        if !self.debug_panel.view.attached
            || point.x < editor_x + geometry.editor_width
            || point.y < geometry.content_top
            || point.y >= geometry.editor_bottom
        {
            return false;
        }
        self.debug_panel_pointer_down(point);
        true
    }
}
