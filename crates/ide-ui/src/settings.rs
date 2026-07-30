//! Estado transacional da janela de configurações.

use ui_components::{Button, ComboBox, ListView, ModalHost, TextInput};
use ui_core::WidgetId;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SettingsPage {
    Contribution(usize),
    Debug,
}

impl Default for SettingsPage {
    fn default() -> Self {
        Self::Contribution(0)
    }
}

pub(super) struct SettingsDialog {
    pub(super) message: Option<String>,
    pub(super) pending_toolchain: Option<usize>,
    pub(super) original_toolchain: Option<usize>,
    /// Segunda ferramenta escolhida na janela e ainda não aplicada.
    pub(super) pending_secondary: Option<usize>,
    pub(super) original_secondary: Option<usize>,
    pub(super) original_debug_host: String,
    pub(super) original_debug_port: String,
}

/// Estado e widgets da janela de configurações.
pub(super) struct SettingsState {
    pub(super) modal: ModalHost,
    pub(super) toolchain_combo: ComboBox,
    pub(super) toolchain_browse_button: Button,
    /// Segunda escolha da seção, ao lado da primeira e com o mesmo gesto.
    pub(super) secondary_combo: ComboBox,
    pub(super) secondary_browse_button: Button,
    pub(super) close_button: Button,
    pub(super) save_button: Button,
    pub(super) pages: ListView,
    pub(super) dialog: Option<SettingsDialog>,
    pub(super) page: SettingsPage,
    pub(super) focus: Option<WidgetId>,
    pub(super) debug_host: TextInput,
    pub(super) debug_port: TextInput,
    pub(super) debug_attach_button: Button,
}

impl SettingsState {
    #[must_use]
    pub(super) const fn is_open(&self) -> bool {
        self.modal.is_open()
    }

    pub(super) fn set_page(&mut self, page: SettingsPage) {
        self.page = page;
        self.focus = None;
    }
}
