//! Estado mínimo do shell e sua fronteira de comandos.

use ide_application::ApplicationCommand;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ShellFocus {
    #[default]
    None,
    Explorer,
    Editor,
    Search,
    /// A caixa de busca **do terminal**, que é outra janela.
    ///
    /// Um valor próprio, e não um alvo guardado à parte: as duas caixas podem
    /// estar abertas ao mesmo tempo, e o teclado precisa saber em qual das duas
    /// está — foi por elas dividirem um estado só que abrir uma fechava a outra.
    SearchTerminal,
    Terminal,
}

#[derive(Default)]
pub(super) struct ShellCommandQueue {
    pending: Vec<ApplicationCommand>,
}

impl ShellCommandQueue {
    pub(super) fn push(&mut self, command: ApplicationCommand) {
        self.pending.push(command);
    }

    pub(super) fn drain(&mut self) -> Vec<ApplicationCommand> {
        std::mem::take(&mut self.pending)
    }

    #[cfg(test)]
    pub(super) fn iter(&self) -> impl Iterator<Item = &ApplicationCommand> {
        self.pending.iter()
    }

    #[cfg(test)]
    pub(super) fn remove(&mut self, index: usize) -> ApplicationCommand {
        self.pending.remove(index)
    }

    #[cfg(test)]
    pub(super) fn retain(&mut self, predicate: impl FnMut(&ApplicationCommand) -> bool) {
        self.pending.retain(predicate);
    }
}
