#![doc = "Casos de uso, comandos e eventos que coordenam a IDE."]

pub mod commands;
pub mod events;
pub mod workspace;

pub use commands::{
    ApplicationCommand, Command, CommandError, CommandRegistry, DebugRequest, NavigationRequest,
    NewItemKind, NewItemRequest, OpenDocumentRequest, SaveDocumentRequest,
};
pub use events::{EventBus, IdeEvent, PublishError};
pub use workspace::{WorkspaceEntry, WorkspacePort, WorkspacePortError};
