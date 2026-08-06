//! Registro de comandos e atalhos da IDE.

use std::{collections::HashMap, path::PathBuf, sync::Arc};

use ide_domain::{DocumentId, TextRange, ToolRole};
use thiserror::Error;

use crate::TaskId;

/// Intenções que atravessam a fronteira entre apresentação e aplicação.
///
/// A interface apenas descreve o pedido. Quem conhece providers, adapters,
/// filesystem e ciclo de vida é a aplicação.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ApplicationCommand {
    OpenDocument(OpenDocumentRequest),
    SaveDocument(SaveDocumentRequest),
    ReloadWorkspace,
    /// Lê os filhos de uma pasta que acabou de ser expandida.
    ///
    /// A árvore é rasa: só o que foi aberto está em memória. Ver a `19`.
    LoadDirectory(std::path::PathBuf),
    OpenProject,
    /// Abre **outra janela** sobre o mesmo projeto.
    ///
    /// Duas janelas do mesmo workspace servem a quem lê um arquivo enquanto
    /// escreve outro, ou compara dois pontos distantes do mesmo código. É outra
    /// instância, e não outra aba: cada uma tem o próprio editor, o próprio
    /// terminal e o próprio analisador.
    DuplicateWorkspace,
    /// Abre um projeto escolhido na lista de recentes.
    ///
    /// Carrega o **caminho**, e não a posição na lista: entre montar o menu e
    /// alguém escolher, a lista pode ter sido reordenada por outra janela, e
    /// uma posição passaria a apontar para outro projeto.
    OpenRecentProject(std::path::PathBuf),
    /// Pede o retrato do repositório de novo.
    ///
    /// A tela não fala com o Git: ela pede, e quem responde é a aplicação, fora
    /// da linha de execução da interface. Ver a `22`.
    RefreshGit,
    OpenSettings,
    OpenToolchainSettings,
    /// Abre o seletor de pasta para apontar uma instalação.
    ///
    /// Carrega **de qual seção** o clique veio. Antes não carregava, e
    /// funcionava porque só existia uma: com duas, o comando não teria como
    /// dizer qual foi clicada, e a aplicação ligava o botão genérico direto na
    /// ferramenta de Java. Ver a fase 0 da `23` e a ADR-026.
    BrowseTool { section: String, role: ToolRole },
    /// Escolhe uma das instalações detectadas, pelo índice na lista.
    SelectTool {
        section: String,
        role: ToolRole,
        index: usize,
    },
    BuildProject,
    ReimportProject,
    RunProject,
    ExecuteTask(TaskId),
    StopProject,
    Navigate(NavigationRequest),
    CreateItem(NewItemRequest),
    BreakpointsChanged(PathBuf),
    Debug(DebugRequest),
    /// Renomeia o arquivo de um documento aberto, seguindo o tipo.
    RenameDocument(RenameDocumentRequest),
    SearchTypes(String),
    /// Quem **usa** o nome na posição informada.
    ///
    /// Leva o mesmo pedido da navegação porque precisa do mesmo: qual
    /// documento, que posição, e qual nome está ali.
    FindReferences(NavigationRequest),
    SearchContent(String),
}

/// Arquivo a renomear, com tudo o que precisa ser reescrito junto.
///
/// A tela decidiu o nome novo e a linguagem disse onde o antigo aparece; o que
/// falta é escrever, e escrever em arquivo é da aplicação. As ocorrências vêm
/// agrupadas por caminho porque é assim que serão aplicadas: um arquivo por vez,
/// e nenhum pela metade.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RenameDocumentRequest {
    pub from: PathBuf,
    pub to: PathBuf,
    /// Nome antigo e novo do símbolo, quando o arquivo declara um.
    pub old_name: String,
    pub new_name: String,
    /// Onde o nome antigo aparece, por arquivo.
    pub occurrences: Vec<FileOccurrences>,
}

/// Ocorrências do nome antigo dentro de um arquivo.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FileOccurrences {
    pub path: PathBuf,
    pub ranges: Vec<TextRange>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OpenDocumentRequest {
    pub path: PathBuf,
    pub line: usize,
    pub column: usize,
}

impl OpenDocumentRequest {
    #[must_use]
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            line: 0,
            column: 0,
        }
    }

    #[must_use]
    pub fn at(mut self, line: usize, column: usize) -> Self {
        self.line = line;
        self.column = column;
        self
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SaveDocumentRequest {
    pub document_id: DocumentId,
    pub path: PathBuf,
    pub text: String,
    pub revision: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NavigationRequest {
    pub document_id: DocumentId,
    pub byte_offset: usize,
    pub token: String,
}

/// Pedido da interface para a sessão de depuração.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DebugRequest {
    Attach { host: String, port: u16 },
    RunAndAttach { host: String, port: u16 },
    Continue,
    Pause,
    StepOver,
    StepInto,
    StepOut,
    Detach,
    SelectFrame(usize),
    Evaluate(String),
    ExpandInspection(String),
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct NewItemTemplateId(pub String);

impl NewItemTemplateId {
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NewItemRequest {
    pub template_id: NewItemTemplateId,
    pub package: String,
    pub name: String,
    pub source_root: PathBuf,
}

/// Delimita explicitamente os arquivos que uma busca textual pode visitar.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SearchScope {
    pub roots: Vec<PathBuf>,
    pub extensions: Vec<String>,
}

impl SearchScope {
    #[must_use]
    pub fn new(roots: Vec<PathBuf>, extensions: Vec<String>) -> Self {
        Self { roots, extensions }
    }

    #[must_use]
    pub fn contains(&self, path: &std::path::Path) -> bool {
        let in_root = self.roots.iter().any(|root| path.starts_with(root));
        let extension_matches = self.extensions.is_empty()
            || path
                .extension()
                .and_then(|value| value.to_str())
                .is_some_and(|extension| {
                    self.extensions
                        .iter()
                        .any(|candidate| candidate.eq_ignore_ascii_case(extension))
                });
        in_root && extension_matches
    }
}

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
        if self.commands.contains_key(&command.id) {
            return Err(CommandError::Duplicate(command.id));
        }
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
        let command = self
            .commands
            .get(id)
            .ok_or_else(|| CommandError::Unknown(id.to_owned()))?;
        (command.handler)()
    }

    pub fn execute_shortcut(&self, shortcut: &str) -> Result<bool, CommandError> {
        let Some(id) = self.shortcuts.get(&normalize_shortcut(shortcut)) else {
            return Ok(false);
        };
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
    value
        .split('+')
        .map(str::trim)
        .map(str::to_ascii_lowercase)
        .collect::<Vec<_>>()
        .join("+")
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
        assert!(
            commands
                .register(Command::new(
                    "file.save",
                    "Save",
                    Some("Ctrl+S"),
                    move || {
                        marker.store(true, Ordering::Relaxed);
                        Ok(())
                    }
                ))
                .is_ok()
        );
        assert!(matches!(commands.execute_shortcut("ctrl+s"), Ok(true)));
        assert!(called.load(Ordering::Relaxed));
    }
}
