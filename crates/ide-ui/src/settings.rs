//! Qual página a janela de configurações está mostrando.
//!
//! A janela em si mora em `ide_shell::settings`; aqui fica só o que atravessa a
//! fronteira, porque a aplicação pede uma página específica ao abri-la.

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
