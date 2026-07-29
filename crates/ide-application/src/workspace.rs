//! Porta de acesso ao workspace usada pelos casos de uso da aplicação.

use std::{
    path::{Path, PathBuf},
    time::SystemTime,
};

use thiserror::Error;

/// Entrada direta de um diretório, sem impor uma representação de árvore à
/// aplicação.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspaceEntry {
    pub path: PathBuf,
    pub is_directory: bool,
    pub modified: Option<SystemTime>,
}

/// Fronteira injetável entre os casos de uso e o filesystem.
///
/// A porta trabalha com operações pequenas. Montagem da árvore, busca e sessão
/// de documentos pertencem a `ide-workspace`; a implementação nativa é apenas
/// um adapter desta interface.
pub trait WorkspacePort: Send + Sync {
    fn metadata(&self, path: &Path) -> Result<WorkspaceEntry, WorkspacePortError>;
    fn read_directory(&self, path: &Path) -> Result<Vec<WorkspaceEntry>, WorkspacePortError>;
    fn read_text(&self, path: &Path) -> Result<String, WorkspacePortError>;
    fn write_text(&self, path: &Path, contents: &str) -> Result<(), WorkspacePortError>;
    fn create_directory(&self, path: &Path) -> Result<(), WorkspacePortError>;
    fn exists(&self, path: &Path) -> bool;
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
#[error("workspace I/O failed: {message}")]
pub struct WorkspacePortError {
    pub message: String,
}

impl From<std::io::Error> for WorkspacePortError {
    fn from(error: std::io::Error) -> Self {
        Self {
            message: error.to_string(),
        }
    }
}
