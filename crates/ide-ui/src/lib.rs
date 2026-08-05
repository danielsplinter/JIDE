#![doc = "Shell visual e interativo da IDE baseado no ERLibUi."]

mod debugging;
mod editor;
mod explorer;
mod ide_shell;
mod layout;
mod menus;
mod search;
mod settings;
mod shell;
mod terminal;
mod text;

pub use debugging::{DebugFrameView, DebugVariableView, DebugView};
pub use editor::{EditorAction, EditorCapabilities, EditorPane};
pub use ide_application::{
    ApplicationCommand, DebugRequest, NavigationRequest, NewItemRequest, NewItemTemplate,
    NewItemTemplateId, OpenDocumentRequest, SaveDocumentRequest, SettingsSection, TaskDescriptor,
    TaskId, UiContributionCatalog,
};
/// A identidade que o Explorer dá a um caminho, para a aplicação usar a mesma.
pub use explorer::id as explorer_id;
pub use ide_shell::IdeShell;
pub use search::{ContentSearchHit, TypeSearchHit};
pub use settings::SettingsPage;
pub use shell::ShellFocus;

/// Teto de resultados pedidos à linguagem.
///
/// Quem procura por nome refina até achar; uma lista de mil linhas não ajuda a
/// escolher e custa a montar a cada letra.
pub const TYPE_SEARCH_LIMIT: usize = 100;
