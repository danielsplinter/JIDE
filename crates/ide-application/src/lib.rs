#![doc = "Casos de uso, comandos e eventos que coordenam a IDE."]

pub mod commands;
pub mod contributions;
pub mod events;
pub mod workspace;

pub use commands::{
    ApplicationCommand, Command, CommandError, CommandRegistry, DebugRequest, NavigationRequest,
    NewItemRequest, NewItemTemplateId, OpenDocumentRequest, SaveDocumentRequest,
};
pub use contributions::{
    ContributionError, ContributionRegistry, LanguageContribution, LanguageDescriptor,
    NewItemTemplate, SettingsSection, TaskDescriptor, TaskId, TaskRegistry, ToolchainRegistry,
    ToolchainSelection,
};
pub use events::{EventBus, IdeEvent, PublishError};
pub use workspace::{WorkspaceEntry, WorkspacePort, WorkspacePortError};
