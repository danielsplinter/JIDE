//! Árvore lógica do workspace, sem acesso direto ao filesystem.

use std::path::{Path, PathBuf};

use ide_application::WorkspacePort;

use crate::WorkspaceError;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FileNode {
    pub path: PathBuf,
    pub is_directory: bool,
    pub children: Vec<FileNode>,
}

/// A raiz e os filhos dela — **um nível**, e não a árvore inteira.
///
/// Varrer tudo na abertura custava 2,17 s sobre 56 mil arquivos, e o Explorer
/// mostra quarenta linhas: o trabalho era jogado fora por construção. Cada pasta
/// é lida quando alguém a expande, por `children_of`.
///
/// Pasta sem filhos carregados e pasta vazia têm a mesma forma. A diferença
/// custaria um campo novo em todo lugar que monta um `FileNode`; reler uma pasta
/// vazia custa uma leitura de diretório, e é o lado barato de errar.
pub(crate) fn scan(
    filesystem: &dyn WorkspacePort,
    root: &Path,
) -> Result<FileNode, WorkspaceError> {
    let metadata = filesystem.metadata(root)?;
    let mut node = FileNode {
        path: metadata.path,
        is_directory: metadata.is_directory,
        children: Vec::new(),
    };
    if node.is_directory {
        node.children = children_of(filesystem, root)?;
    }
    Ok(node)
}

/// Os filhos diretos de uma pasta, cada um ainda sem os seus.
pub(crate) fn children_of(
    filesystem: &dyn WorkspacePort,
    directory: &Path,
) -> Result<Vec<FileNode>, WorkspaceError> {
    let mut children = Vec::new();
    for entry in filesystem.read_directory(directory)? {
        let name = entry.path.file_name().and_then(|name| name.to_str());
        if matches!(name, Some(".git" | "target")) {
            continue;
        }
        let metadata = filesystem.metadata(&entry.path)?;
        children.push(FileNode {
            path: metadata.path,
            is_directory: metadata.is_directory,
            children: Vec::new(),
        });
    }
    Ok(children)
}
