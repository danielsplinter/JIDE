//! Tradução entre intenções da UI e ações coordenadas pela aplicação.

use std::path::PathBuf;

use ide_application::{
    ApplicationCommand, DebugRequest, EventBus, IdeEvent, NavigationRequest, NewItemRequest,
    OpenDocumentRequest, RenameDocumentRequest, SaveDocumentRequest, TaskId,
};
use ide_domain::ToolRole;
use ide_ui::IdeShell;

pub(super) enum UiAction {
    OpenDocument(OpenDocumentRequest),
    Git(ide_application::GitRequest),
    SaveDocument(SaveDocumentRequest),
    ReloadWorkspace,
    LoadDirectory(std::path::PathBuf),
    OpenProject,
    DuplicateWorkspace,
    OpenRecentProject(PathBuf),
    OpenSettings,
    OpenToolchainSettings,
    BrowseTool {
        section: String,
        role: ToolRole,
    },
    SelectTool {
        section: String,
        role: ToolRole,
        index: usize,
    },
    BuildProject,
    ReimportProject,
    RunProject,
    ExecuteTask(TaskId),
    StopProject,
    Navigate(NavigationRequest),
    CreateItem(NewItemRequest),
    BreakpointsChanged(PathBuf),
    Debug(DebugRequest),
    RenameDocument(RenameDocumentRequest),
    SearchTypes(String),
    FindReferences(ide_application::NavigationRequest),
    SearchContent(String),
}

impl From<ApplicationCommand> for UiAction {
    fn from(command: ApplicationCommand) -> Self {
        match command {
            ApplicationCommand::OpenDocument(value) => Self::OpenDocument(value),
            ApplicationCommand::RenameDocument(value) => Self::RenameDocument(value),
            ApplicationCommand::SaveDocument(value) => Self::SaveDocument(value),
            ApplicationCommand::ReloadWorkspace => Self::ReloadWorkspace,
            ApplicationCommand::LoadDirectory(path) => Self::LoadDirectory(path),
            ApplicationCommand::OpenProject => Self::OpenProject,
            ApplicationCommand::DuplicateWorkspace => Self::DuplicateWorkspace,
            ApplicationCommand::OpenRecentProject(path) => Self::OpenRecentProject(path),
            ApplicationCommand::Git(request) => Self::Git(request),
            ApplicationCommand::OpenSettings => Self::OpenSettings,
            ApplicationCommand::OpenToolchainSettings => Self::OpenToolchainSettings,
            ApplicationCommand::BrowseTool { section, role } => {
                Self::BrowseTool { section, role }
            }
            ApplicationCommand::SelectTool {
                section,
                role,
                index,
            } => Self::SelectTool {
                section,
                role,
                index,
            },
            ApplicationCommand::BuildProject => Self::BuildProject,
            ApplicationCommand::ReimportProject => Self::ReimportProject,
            ApplicationCommand::RunProject => Self::RunProject,
            ApplicationCommand::ExecuteTask(value) => Self::ExecuteTask(value),
            ApplicationCommand::StopProject => Self::StopProject,
            ApplicationCommand::Navigate(value) => Self::Navigate(value),
            ApplicationCommand::CreateItem(value) => Self::CreateItem(value),
            ApplicationCommand::BreakpointsChanged(value) => Self::BreakpointsChanged(value),
            ApplicationCommand::Debug(value) => Self::Debug(value),
            ApplicationCommand::SearchTypes(value) => Self::SearchTypes(value),
            ApplicationCommand::FindReferences(request) => Self::FindReferences(request),
            ApplicationCommand::SearchContent(value) => Self::SearchContent(value),
        }
    }
}

#[derive(Default)]
pub(super) struct UiBridge {
    pub(super) shell: Option<IdeShell>,
    /// A medida do texto, guardada porque o shell não é eterno.
    ///
    /// Trocar de projeto constrói um shell novo, e ele precisa da mesma medida.
    /// Guardar aqui — em vez de reconstruir — evita varrer as fontes do sistema
    /// a cada troca, e evita o defeito que veio antes disso: um shell novo sem
    /// medida nenhuma, com o cursor caindo longe do clique.
    pub(super) text_metrics: Option<std::sync::Arc<ui_text_cosmic::CosmicTextEngine>>,
    /// A área de transferência, pelo mesmo motivo.
    ///
    /// Sem ela, copiar e colar paravam de funcionar depois da primeira troca de
    /// projeto, e só voltavam fechando a IDE.
    pub(super) clipboard: Option<std::sync::Arc<ui_clipboard_arboard::SystemClipboard>>,
    events: EventBus,
    navigation_requests: Vec<NavigationRequest>,
}

impl UiBridge {
    pub(super) fn replace_event_bus(&mut self, capacity: usize) {
        self.events = EventBus::bounded(capacity.max(1));
    }

    pub(super) fn actions(&mut self, mut direct: Vec<ApplicationCommand>) -> Vec<UiAction> {
        if let Some(shell) = self.shell.as_mut() {
            direct.extend(shell.drain_application_commands());
        }
        direct.into_iter().map(UiAction::from).collect()
    }

    pub(super) fn publish(&self, event: IdeEvent) -> Result<(), String> {
        self.events
            .publish(event)
            .map_err(|error| format!("{error:?}"))
    }

    pub(super) fn drain_events(&self) -> Result<Vec<IdeEvent>, String> {
        self.events.drain().map_err(|error| format!("{error:?}"))
    }

    pub(super) fn remember_navigation(&mut self, request: NavigationRequest) {
        self.navigation_requests.push(request);
    }

    #[cfg(test)]
    pub(super) fn navigation_requests(&self) -> &[NavigationRequest] {
        &self.navigation_requests
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ide_domain::DocumentId;

    #[test]
    fn command_translation_does_not_need_a_native_window() {
        let mut bridge = UiBridge::default();
        let actions = bridge.actions(vec![
            ApplicationCommand::ReloadWorkspace,
            ApplicationCommand::SearchTypes("Item".to_owned()),
        ]);
        assert!(matches!(actions[0], UiAction::ReloadWorkspace));
        assert!(matches!(&actions[1], UiAction::SearchTypes(query) if query == "Item"));

        bridge.remember_navigation(NavigationRequest {
            document_id: DocumentId(7),
            byte_offset: 12,
            token: "Item".to_owned(),
        });
        assert_eq!(bridge.navigation_requests()[0].document_id, DocumentId(7));
    }
}
