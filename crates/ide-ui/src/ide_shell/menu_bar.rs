//! A barra de menus do topo: o que cada comando dela significa.
//!
//! A barra é um componente da biblioteca; o que está aqui é a tradução do
//! comando que ela devolve para a intenção que a aplicação executa.

use super::*;

impl IdeShell {
    /// Clique na barra de menus. Devolve `true` quando ela o consumiu.
    pub(super) fn menu_bar_pointer_down(&mut self, point: Point, size: Size) -> bool {
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
                return true;
            }
            EventResult::Action(WidgetAction::Command(command))
                if command.0 == "file.duplicate" =>
            {
                self.commands.push(ApplicationCommand::DuplicateWorkspace);
                self.context.status_message = "Abrindo outra janela deste projeto".to_owned();
                return true;
            }
            EventResult::Action(WidgetAction::Command(command))
                if command.0.starts_with(crate::menus::RECENTE) =>
            {
                // A posição volta a ser caminho aqui, contra a mesma lista que
                // montou o menu. Uma posição sem caminho não vira comando: é o
                // que sobra de um menu montado antes da lista encolher.
                if let Some(path) = command
                    .0
                    .strip_prefix(crate::menus::RECENTE)
                    .and_then(|posicao| posicao.parse::<usize>().ok())
                    .and_then(|posicao| self.menu.recents.get(posicao))
                {
                    self.context.status_message = format!("Abrindo {}", path.display());
                    self.commands
                        .push(ApplicationCommand::OpenRecentProject(path.clone()));
                }
                return true;
            }
            EventResult::Action(WidgetAction::Command(command)) if command.0 == "file.save" => {
                self.request_save_active_document();
                return true;
            }
            EventResult::Action(WidgetAction::Command(command)) if command.0 == "settings.open" => {
                self.commands.push(ApplicationCommand::OpenSettings);
                return true;
            }
            EventResult::Action(WidgetAction::Command(command)) if command.0 == "project.build" => {
                self.commands.push(ApplicationCommand::BuildProject);
                return true;
            }
            EventResult::Action(WidgetAction::Command(command))
                if command.0 == "project.reimport" =>
            {
                self.commands.push(ApplicationCommand::ReimportProject);
                return true;
            }
            EventResult::Action(WidgetAction::Command(command)) if command.0 == "project.run" => {
                self.commands.push(ApplicationCommand::RunProject);
                self.context.status_message = "Executando a aplicação".to_owned();
                return true;
            }
            EventResult::Action(WidgetAction::Command(command)) if command.0 == "project.stop" => {
                self.commands.push(ApplicationCommand::StopProject);
                return true;
            }
            EventResult::Action(WidgetAction::Command(command))
                if command.0.starts_with("task.execute.") =>
            {
                if let Some(id) = command.0.strip_prefix("task.execute.") {
                    self.commands
                        .push(ApplicationCommand::ExecuteTask(TaskId(id.to_owned())));
                }
                return true;
            }
            EventResult::Action(WidgetAction::Command(command)) if command.0 == "debug.connect" => {
                self.settings.set_page(SettingsPage::Debug);
                self.commands.push(ApplicationCommand::OpenSettings);
                return true;
            }
            EventResult::Action(WidgetAction::Command(command))
                if command.0.starts_with("debug.") =>
            {
                if let Some(request) = debug_request_for(&command.0) {
                    self.commands.push(ApplicationCommand::Debug(request));
                }
                return true;
            }
            EventResult::Handled | EventResult::Action(_) => return true,
            EventResult::Ignored => {}
        }
        false
    }
}
