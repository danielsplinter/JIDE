//! Estado transacional da janela de configurações.

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
    pub(super) original_debug_host: String,
    pub(super) original_debug_port: String,
}
