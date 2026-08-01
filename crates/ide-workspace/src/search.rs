//! Busca textual sobre a árvore já carregada do workspace.

use std::path::PathBuf;

use ide_application::{SearchScope, WorkspacePort};

use crate::FileNode;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SearchMatch {
    pub path: PathBuf,
    pub line: usize,
    pub column: usize,
    pub preview: String,
}

pub(crate) fn search_content(
    filesystem: &dyn WorkspacePort,
    root: &FileNode,
    scope: &SearchScope,
    query: &str,
    limit: usize,
) -> Vec<SearchMatch> {
    if query.is_empty() || limit == 0 {
        return Vec::new();
    }
    let mut matches = Vec::new();
    search_node(filesystem, root, scope, query, limit, &mut matches);
    matches
}

fn search_node(
    filesystem: &dyn WorkspacePort,
    node: &FileNode,
    scope: &SearchScope,
    query: &str,
    limit: usize,
    output: &mut Vec<SearchMatch>,
) {
    if output.len() >= limit {
        return;
    }
    if node.is_directory {
        // A árvore em memória é rasa desde a `19`: ela tem só o que foi
        // expandido. A busca lê o diretório na hora, para responder pelo projeto
        // inteiro e não pelo que o usuário abriu no Explorer.
        let filhos = crate::tree::children_of(filesystem, &node.path).unwrap_or_default();
        for child in &filhos {
            search_node(filesystem, child, scope, query, limit, output);
            if output.len() >= limit {
                break;
            }
        }
        return;
    }
    if !scope.contains(&node.path) {
        return;
    }
    let Ok(content) = filesystem.read_text(&node.path) else {
        return;
    };
    let query_lower = query.to_lowercase();
    for (line_index, line) in content.lines().enumerate() {
        let line_lower = line.to_lowercase();
        let Some(byte_column) = line_lower.find(&query_lower) else {
            continue;
        };
        output.push(SearchMatch {
            path: node.path.clone(),
            line: line_index + 1,
            column: line_lower[..byte_column].chars().count() + 1,
            preview: compact_preview(line),
        });
        if output.len() >= limit {
            return;
        }
    }
}

fn compact_preview(line: &str) -> String {
    const MAX_CHARS: usize = 160;
    let trimmed = line.trim();
    let mut preview = trimmed.chars().take(MAX_CHARS).collect::<String>();
    if trimmed.chars().count() > MAX_CHARS {
        preview.push('…');
    }
    preview
}
