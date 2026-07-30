#![doc = "Documentos, árvore, busca e filesystem do workspace."]

mod document;
mod filesystem;
mod search;
mod tree;

use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::SystemTime,
};

use ide_application::{SearchScope, WorkspacePort, WorkspacePortError};
use thiserror::Error;

pub use document::{BufferError, EditorSession, OpenDocument, TextBuffer, rewrite_occurrences};
pub use filesystem::NativeWorkspaceFileSystem;
pub use search::SearchMatch;
pub use tree::FileNode;

#[derive(Clone)]
pub struct WorkspaceService {
    filesystem: Arc<dyn WorkspacePort>,
}

impl Default for WorkspaceService {
    fn default() -> Self {
        Self::native()
    }
}

impl WorkspaceService {
    #[must_use]
    pub fn new(filesystem: Arc<dyn WorkspacePort>) -> Self {
        Self { filesystem }
    }

    #[must_use]
    pub fn native() -> Self {
        Self::new(Arc::new(NativeWorkspaceFileSystem))
    }

    pub fn scan(&self, root: &Path) -> Result<FileNode, WorkspaceError> {
        tree::scan(self.filesystem.as_ref(), root)
    }

    pub fn read_document(&self, path: &Path) -> Result<String, WorkspaceError> {
        self.filesystem.read_text(path).map_err(Into::into)
    }

    #[must_use]
    pub fn modified_at(&self, path: &Path) -> Option<SystemTime> {
        self.filesystem
            .metadata(path)
            .ok()
            .and_then(|entry| entry.modified)
    }

    pub fn save_document(&self, path: &Path, contents: &str) -> Result<(), WorkspaceError> {
        self.filesystem
            .write_text(path, contents)
            .map_err(Into::into)
    }

    /// Move um arquivo dentro do workspace.
    pub fn rename_path(&self, from: &Path, to: &Path) -> Result<(), WorkspaceError> {
        self.filesystem.rename_path(from, to).map_err(Into::into)
    }

    pub fn create_directory(&self, path: &Path) -> Result<(), WorkspaceError> {
        self.filesystem.create_directory(path).map_err(Into::into)
    }

    pub fn create_file(&self, path: &Path, contents: &str) -> Result<(), WorkspaceError> {
        if self.filesystem.exists(path) {
            return Err(WorkspaceError::AlreadyExists(path.to_path_buf()));
        }
        self.filesystem
            .write_text(path, contents)
            .map_err(Into::into)
    }

    #[must_use]
    pub fn search_content(
        &self,
        root: &FileNode,
        scope: &SearchScope,
        query: &str,
        limit: usize,
    ) -> Vec<SearchMatch> {
        search::search_content(self.filesystem.as_ref(), root, scope, query, limit)
    }
}

#[derive(Debug, Error)]
pub enum WorkspaceError {
    #[error(transparent)]
    Port(#[from] WorkspacePortError),
    #[error("{0} já existe")]
    AlreadyExists(PathBuf),
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        sync::atomic::{AtomicU64, Ordering},
    };

    fn workspace() -> PathBuf {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "er-ide-workspace-search-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&root);
        assert!(fs::create_dir_all(root.join("modulo/src/main/java/br/com")).is_ok());
        assert!(fs::create_dir_all(root.join("docs")).is_ok());
        assert!(
            fs::write(
                root.join("modulo/src/main/java/br/com/Pedido.java"),
                "class Pedido {\n    String mensagem = \"Conteudo procurado\";\n}\n"
            )
            .is_ok()
        );
        assert!(fs::write(root.join("docs/fora.txt"), "conteudo fora\n").is_ok());
        assert!(
            fs::write(
                root.join("modulo/src/main/java/br/com/ignorado.txt"),
                "conteudo procurado\n"
            )
            .is_ok()
        );
        root
    }

    #[test]
    fn service_searches_only_the_explicit_scope() {
        let root = workspace();
        let service = WorkspaceService::native();
        let tree = service.scan(&root);
        assert!(tree.is_ok());
        let Ok(tree) = tree else {
            return;
        };
        let scope = SearchScope::new(
            vec![root.join("modulo/src/main/java")],
            vec!["java".to_owned()],
        );
        let found = service.search_content(&tree, &scope, "CONTEUDO", 20);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].line, 2);
        let empty_scope = SearchScope::default();
        assert!(
            service
                .search_content(&tree, &empty_scope, "CONTEUDO", 20)
                .is_empty()
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn create_file_never_overwrites_existing_content() {
        let root = workspace();
        let service = WorkspaceService::native();
        let file = root.join("novo.txt");
        assert!(service.create_file(&file, "primeiro").is_ok());
        assert!(matches!(
            service.create_file(&file, "segundo"),
            Err(WorkspaceError::AlreadyExists(path)) if path == file
        ));
        assert_eq!(fs::read_to_string(&file).unwrap_or_default(), "primeiro");
        let _ = fs::remove_dir_all(root);
    }
}
