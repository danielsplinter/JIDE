//! Varredura limitada das entradas que alimentam o índice do workspace.

use std::{
    fs,
    path::{Path, PathBuf},
};

pub(super) fn collect_workspace_paths(root: &Path, output: &mut Vec<PathBuf>, limit: usize) {
    if output.len() >= limit {
        return;
    }
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        if output.len() >= limit {
            break;
        }
        let path = entry.path();
        if path.is_dir() {
            let ignored = path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| matches!(name, ".git" | "target" | "node_modules" | ".gradle"));
            if !ignored {
                collect_workspace_paths(&path, output, limit);
            }
        } else {
            output.push(path);
        }
    }
}
