#![doc = "Casos de uso, comandos e eventos que coordenam a IDE."]

pub mod commands;
pub mod contributions;
pub mod events;
pub mod workspace;

pub use commands::{
    ApplicationCommand, Command, CommandError, CommandRegistry, DebugRequest, FileOccurrences,
    NavigationRequest,
    NewItemRequest, NewItemTemplateId, OpenDocumentRequest, RenameDocumentRequest,
    SaveDocumentRequest, SearchScope,
};
pub use contributions::{
    ContributionError, ContributionRegistry, LanguageContribution, LanguageDescriptor,
    NewItemTemplate, SettingsSection, TaskController, TaskControllerError, TaskDescriptor,
    TaskExecutionContext, TaskExecutionError, TaskExecutionResult, TaskExecutor, TaskId,
    TaskRegistry, ToolchainRegistry, ToolchainSelection, UiContributionCatalog,
};
pub use events::{EventBus, IdeEvent, PublishError};
pub use workspace::{WorkspaceEntry, WorkspacePort, WorkspacePortError};
