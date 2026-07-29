#![doc = "Árvore de arquivos e busca textual do workspace."]

use std::{
    fs,
    path::{Path, PathBuf},
};
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
        let mut node = Self {
            path: root.to_path_buf(),
            is_directory: metadata.is_dir(),
            children: Vec::new(),
        };
        if node.is_directory {
            let mut entries = fs::read_dir(root)?.collect::<Result<Vec<_>, _>>()?;
            entries.sort_by_key(|entry| entry.file_name());
            for entry in entries {
                let name = entry.file_name();
                if matches!(name.to_str(), Some(".git" | "target")) {
                    continue;
                }
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

/// Procura texto somente em arquivos descendentes de uma pasta chamada `java`.
///
/// A árvore pode representar um projeto multimódulo, portanto não existe uma
/// única raiz fixa como `src/main/java`: qualquer diretório `java` inicia uma
/// raiz de fontes. Arquivos fora dessas raízes nunca entram no resultado.
#[must_use]
pub fn search_java_content(root: &FileNode, query: &str, limit: usize) -> Vec<SearchMatch> {
    if query.is_empty() || limit == 0 {
        return Vec::new();
    }
    let mut matches = Vec::new();
    search_java_node(root, query, limit, false, &mut matches);
    matches
}

fn search_java_node(
    node: &FileNode,
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
            search_java_node(child, query, limit, inside_java, output);
            if output.len() >= limit {
                break;
            }
        }
        return;
    }
    if !inside_java {
        return;
    }
    let Ok(content) = fs::read_to_string(&node.path) else {
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

fn search_node(node: &FileNode, query: &str, limit: usize, output: &mut Vec<SearchMatch>) {
    if output.len() >= limit || query.is_empty() {
        return;
    }
    if node.is_directory {
        for child in &node.children {
            search_node(child, query, limit, output);
        }
        return;
    }
    let Ok(content) = fs::read_to_string(&node.path) else {
        return;
    };
    for (line_index, line) in content.lines().enumerate() {
        if let Some(column) = line.find(query) {
            output.push(SearchMatch {
                path: node.path.clone(),
                line: line_index + 1,
                column: column + 1,
                preview: line.trim().to_owned(),
            });
            if output.len() >= limit {
                return;
            }
        }
    }
}

#[derive(Debug, Error)]
pub enum WorkspaceError {
    #[error("workspace I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("{0} já existe")]
    AlreadyExists(PathBuf),
}

/// Cria o diretório e os intermediários que faltarem.
///
/// Um pacote Java é uma cadeia de diretórios, e criar `br.com.exemplo` de uma
/// vez é o que o usuário pediu ao digitar o nome inteiro.
pub fn create_directory(path: &Path) -> Result<(), WorkspaceError> {
    fs::create_dir_all(path)?;
    Ok(())
}

/// Cria o arquivo com o conteúdo, recusando sobrescrever o que já existe.
///
/// Sobrescrever apagaria trabalho por causa de um nome repetido digitado sem
/// atenção; o erro é a resposta útil.
pub fn create_file(path: &Path, contents: &str) -> Result<(), WorkspaceError> {
    if path.exists() {
        return Err(WorkspaceError::AlreadyExists(path.to_path_buf()));
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, contents)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn workspace() -> PathBuf {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "er-ide-workspace-search-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&root);
        assert!(fs::create_dir_all(root.join("modulo/src/main/java/br/com")).is_ok());
        assert!(fs::create_dir_all(root.join("modulo/src/test/java/br/com")).is_ok());
        assert!(fs::create_dir_all(root.join("docs")).is_ok());
        assert!(
            fs::write(
                root.join("modulo/src/main/java/br/com/Pedido.java"),
                "class Pedido {\n    String mensagem = \"Conteudo procurado\";\n}\n"
            )
            .is_ok()
        );
        assert!(
            fs::write(
                root.join("modulo/src/test/java/br/com/PedidoTest.java"),
                "class PedidoTest { // conteudo de teste\n}\n"
            )
            .is_ok()
        );
        assert!(fs::write(root.join("docs/fora.txt"), "conteudo fora da pasta java\n").is_ok());
        root
    }

    #[test]
    fn empty_query_does_not_scan() {
        let root = FileNode {
            path: PathBuf::from("."),
            is_directory: true,
            children: Vec::new(),
        };
        assert!(search(&root, "", 20).is_empty());
    }

    #[test]
    fn java_content_search_ignores_files_outside_java_roots() {
        let root = workspace();
        let tree = FileNode::scan(&root);
        assert!(tree.is_ok(), "a árvore de teste precisa ser válida");
        let Ok(tree) = tree else {
            return;
        };

        let found = search_java_content(&tree, "CONTEUDO", 20);

        assert_eq!(found.len(), 2);
        assert!(
            found
                .iter()
                .all(|hit| hit.path.components().any(|part| part.as_os_str() == "java"))
        );
        assert_eq!(found[0].line, 2);
        assert_eq!(found[0].column, 24);
        assert!(found[0].preview.contains("Conteudo procurado"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn java_content_search_obeys_the_limit_and_empty_query() {
        let root = workspace();
        let tree = FileNode::scan(&root);
        assert!(tree.is_ok(), "a árvore de teste precisa ser válida");
        let Ok(tree) = tree else {
            return;
        };

        assert!(search_java_content(&tree, "", 100).is_empty());
        assert_eq!(search_java_content(&tree, "conteudo", 1).len(), 1);
        assert!(search_java_content(&tree, "conteudo", 0).is_empty());
        let _ = fs::remove_dir_all(root);
    }
}
