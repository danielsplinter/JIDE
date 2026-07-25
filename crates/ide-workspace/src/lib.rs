#![doc = "Árvore de arquivos e busca textual do workspace."]

use std::{fs, path::{Path, PathBuf}};
use thiserror::Error;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FileNode {
    pub path: PathBuf,
    pub is_directory: bool,
    pub children: Vec<FileNode>,
}

impl FileNode {
    pub fn scan(root: &Path) -> Result<Self, WorkspaceError> {
        let metadata = fs::metadata(root)?;
        let mut node = Self { path: root.to_path_buf(), is_directory: metadata.is_dir(), children: Vec::new() };
        if node.is_directory {
            let mut entries = fs::read_dir(root)?.collect::<Result<Vec<_>, _>>()?;
            entries.sort_by_key(|entry| entry.file_name());
            for entry in entries {
                let name = entry.file_name();
                if matches!(name.to_str(), Some(".git" | "target")) { continue; }
                node.children.push(Self::scan(&entry.path())?);
            }
        }
        Ok(node)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SearchMatch {
    pub path: PathBuf,
    pub line: usize,
    pub column: usize,
    pub preview: String,
}

pub fn search(root: &FileNode, query: &str, limit: usize) -> Vec<SearchMatch> {
    let mut matches = Vec::new();
    search_node(root, query, limit, &mut matches);
    matches
}

fn search_node(node: &FileNode, query: &str, limit: usize, output: &mut Vec<SearchMatch>) {
    if output.len() >= limit || query.is_empty() { return; }
    if node.is_directory {
        for child in &node.children { search_node(child, query, limit, output); }
        return;
    }
    let Ok(content) = fs::read_to_string(&node.path) else { return };
    for (line_index, line) in content.lines().enumerate() {
        if let Some(column) = line.find(query) {
            output.push(SearchMatch {
                path: node.path.clone(),
                line: line_index + 1,
                column: column + 1,
                preview: line.trim().to_owned(),
            });
            if output.len() >= limit { return; }
        }
    }
}

#[derive(Debug, Error)]
pub enum WorkspaceError {
    #[error("workspace I/O failed: {0}")]
    Io(#[from] std::io::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_query_does_not_scan() {
        let root = FileNode { path: PathBuf::from("."), is_directory: true, children: Vec::new() };
        assert!(search(&root, "", 20).is_empty());
    }
}

