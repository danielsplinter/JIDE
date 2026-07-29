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
    if !node.is_directory {
        return Ok(node);
    }
    for entry in filesystem.read_directory(root)? {
        let name = entry.path.file_name().and_then(|name| name.to_str());
        if matches!(name, Some(".git" | "target")) {
            continue;
        }
        node.children.push(scan(filesystem, &entry.path)?);
    }
    Ok(node)
}
