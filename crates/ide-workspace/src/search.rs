//! Busca textual sobre a árvore já carregada do workspace.

use std::path::PathBuf;

use ide_application::WorkspacePort;

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
    source_roots: &[PathBuf],
    query: &str,
    limit: usize,
) -> Vec<SearchMatch> {
    if query.is_empty() || limit == 0 {
        return Vec::new();
    }
    let mut matches = Vec::new();
    search_java_node(
        filesystem,
        root,
        source_roots,
        query,
        limit,
        false,
        &mut matches,
    );
    matches
}

fn search_java_node(
    filesystem: &dyn WorkspacePort,
    node: &FileNode,
    source_roots: &[PathBuf],
    query: &str,
    limit: usize,
    inside_java: bool,
    output: &mut Vec<SearchMatch>,
) {
    if output.len() >= limit {
        return;
    }
    if node.is_directory {
        let inside_java = inside_java
            || node
                .path
                .file_name()
                .is_some_and(|name| name.to_string_lossy().eq_ignore_ascii_case("java"));
        for child in &node.children {
            search_java_node(
                filesystem,
                child,
                source_roots,
                query,
                limit,
                inside_java,
                output,
            );
            if output.len() >= limit {
                break;
            }
        }
        return;
    }
    let in_source_root = source_roots.iter().any(|root| node.path.starts_with(root));
    if (!source_roots.is_empty() && !in_source_root) || (source_roots.is_empty() && !inside_java) {
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
