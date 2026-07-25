#![doc = "Registro de comandos e atalhos da IDE."]

use std::{collections::HashMap, sync::Arc};
use thiserror::Error;

pub type CommandHandler = Arc<dyn Fn() -> Result<(), CommandError> + Send + Sync>;

#[derive(Clone)]
pub struct Command {
    pub id: String,
    pub title: String,
    pub shortcut: Option<String>,
    handler: CommandHandler,
}

#[derive(Default)]
pub struct CommandRegistry {
    commands: HashMap<String, Command>,
    shortcuts: HashMap<String, String>,
}

impl CommandRegistry {
    pub fn register(&mut self, command: Command) -> Result<(), CommandError> {
        if self.commands.contains_key(&command.id) { return Err(CommandError::Duplicate(command.id)); }
        if let Some(shortcut) = &command.shortcut {
            let normalized = normalize_shortcut(shortcut);
            if self.shortcuts.contains_key(&normalized) {
                return Err(CommandError::ShortcutConflict(shortcut.clone()));
            }
            self.shortcuts.insert(normalized, command.id.clone());
        }
        self.commands.insert(command.id.clone(), command);
        Ok(())
    }

    pub fn execute(&self, id: &str) -> Result<(), CommandError> {
        let command = self.commands.get(id).ok_or_else(|| CommandError::Unknown(id.to_owned()))?;
        (command.handler)()
    }

    pub fn execute_shortcut(&self, shortcut: &str) -> Result<bool, CommandError> {
        let Some(id) = self.shortcuts.get(&normalize_shortcut(shortcut)) else { return Ok(false) };
        self.execute(id)?;
        Ok(true)
    }
}

impl Command {
    pub fn new(
        id: impl Into<String>,
        title: impl Into<String>,
        shortcut: Option<impl Into<String>>,
        handler: impl Fn() -> Result<(), CommandError> + Send + Sync + 'static,
    ) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            shortcut: shortcut.map(Into::into),
            handler: Arc::new(handler),
        }
    }
}

fn normalize_shortcut(value: &str) -> String {
    value.split('+').map(str::trim).map(str::to_ascii_lowercase).collect::<Vec<_>>().join("+")
}

#[derive(Debug, Error)]
pub enum CommandError {
    #[error("duplicate command: {0}")]
    Duplicate(String),
    #[error("unknown command: {0}")]
    Unknown(String),
    #[error("shortcut already registered: {0}")]
    ShortcutConflict(String),
    #[error("command failed: {0}")]
    Failed(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};

    #[test]
    fn shortcut_executes_registered_command() {
        let called = Arc::new(AtomicBool::new(false));
        let marker = called.clone();
        let mut commands = CommandRegistry::default();
        assert!(commands.register(Command::new("file.save", "Save", Some("Ctrl+S"), move || {
            marker.store(true, Ordering::Relaxed);
            Ok(())
        })).is_ok());
        assert!(matches!(commands.execute_shortcut("ctrl+s"), Ok(true)));
        assert!(called.load(Ordering::Relaxed));
    }
}
