//! Estado transacional da janela de configurações.

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum SettingsPage {
    #[default]
    Compiler,
    Debug,
}

pub(super) struct SettingsDialog {
    pub(super) message: Option<String>,
    pub(super) pending_jdk: Option<usize>,
    pub(super) original_jdk: Option<usize>,
    pub(super) original_debug_host: String,
    pub(super) original_debug_port: String,
}
