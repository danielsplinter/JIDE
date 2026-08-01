//! Identidade, compactação e projeção visual da árvore do Explorer.

use std::{
    collections::{HashSet, hash_map::DefaultHasher},
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
};

use ide_workspace::FileNode;
use ui_components::{ContextMenu, Scrollbar, Splitter, TreeItem, TreeView};

/// Estado completo do painel de arquivos.
///
/// A shell encaminha eventos e fornece geometrias; árvore, expansão, seleção,
/// rolagem e redimensionamento permanecem juntos nesta feature.
pub(super) struct ExplorerState {
    pub(super) workspace_name: String,
    pub(super) workspace: FileNode,
    pub(super) tree: TreeView,
    pub(super) context_menu: ContextMenu,
    pub(super) context_menu_target: Option<PathBuf>,
    /// Arquivo sob o clique, quando não foi uma pasta.
    ///
    /// `context_menu_target` guarda a **pasta**, porque é nela que a criação
    /// acontece; renomear fala do arquivo, e são caminhos diferentes.
    pub(super) context_menu_file: Option<PathBuf>,
    pub(super) expanded: HashSet<PathBuf>,
    /// Pastas cujos filhos já foram pedidos à aplicação.
    ///
    /// Sem isto, uma pasta **vazia no disco** pede leitura para sempre: ela tem
    /// a mesma forma de uma pasta não lida, e cada resposta faz a reconciliação
    /// da seleção pedir todas as outras de novo. Com quarenta pastas expandidas
    /// vira milhar de leituras por quadro, e a janela não chega a desenhar.
    pub(super) requested: HashSet<PathBuf>,
    pub(super) scroll_x: f32,
    pub(super) scroll_line: usize,
    pub(super) sidebar_width: f32,
    pub(super) splitter: Splitter,
    pub(super) vertical_scrollbar: Scrollbar,
    pub(super) horizontal_scrollbar: Scrollbar,
}

impl ExplorerState {
    #[must_use]
    pub(super) fn workspace_root(&self) -> &Path {
        &self.workspace.path
    }

    #[must_use]
    pub(super) const fn workspace_tree(&self) -> &FileNode {
        &self.workspace
    }

    /// Repõe as linhas da árvore a partir do que está em memória.
    ///
    /// Carregar uma pasta muda o `FileNode` e não a `TreeView`: sem repor os
    /// itens, a árvore continua desenhando a varredura anterior — o arquivo está
    /// lá dentro e não aparece.
    pub(super) fn rebuild_items(&mut self, source_root_names: &[String]) {
        self.tree.set_roots(items(&self.workspace, source_root_names));
    }

    pub(super) fn replace_workspace(&mut self, workspace: FileNode, source_root_names: &[String]) {
        self.workspace_name = workspace
            .path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("workspace")
            .to_owned();
        self.workspace = workspace;
        self.tree
            .set_roots(items(&self.workspace, source_root_names));
        self.expanded
            .retain(|path| path.starts_with(&self.workspace.path));
        self.expanded.insert(self.workspace.path.clone());
        // Outra árvore, outra leitura: recarregar o projeto é justamente o
        // pedido de ler tudo de novo.
        self.requested.clear();
        self.context_menu.close();
        self.context_menu_target = None;
        self.context_menu_file = None;
        self.scroll_x = 0.0;
        self.scroll_line = 0;
    }

    #[must_use]
    pub(super) fn is_expanded(&self, path: &Path) -> bool {
        self.expanded.contains(path)
    }
}

pub(super) fn id(path: &Path) -> u64 {
    let mut hasher = DefaultHasher::new();
    path.hash(&mut hasher);
    hasher.finish()
}

fn label(node: &FileNode) -> &str {
    node.path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("?")
}

pub(super) fn is_source_root(path: &Path, source_root_names: &[String]) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    source_root_names
        .iter()
        .any(|candidate| candidate.eq_ignore_ascii_case(name))
}

pub(super) fn is_package(path: &Path, source_root_names: &[String]) -> bool {
    path.ancestors()
        .skip(1)
        .any(|ancestor| is_source_root(ancestor, source_root_names))
}

fn compact_package_chain<'a>(
    node: &'a FileNode,
    source_root_names: &[String],
) -> (&'a FileNode, String) {
    let mut label = label(node).to_owned();
    let mut current = node;
    while is_package(&current.path, source_root_names) {
        let [only_child] = current.children.as_slice() else {
            break;
        };
        if !only_child.is_directory {
            break;
        }
        label.push('.');
        label.push_str(self::label(only_child));
        current = only_child;
    }
    (current, label)
}

pub(super) fn items(node: &FileNode, source_root_names: &[String]) -> Vec<TreeItem> {
    node.children
        .iter()
        .map(|child| {
            let (node, label) = compact_package_chain(child, source_root_names);
            TreeItem::new(id(&node.path), label, items(node, source_root_names))
        })
        .collect()
}

pub(super) fn visible_row(
    items: &[TreeItem],
    expanded: &HashSet<u64>,
    target: u64,
) -> Option<usize> {
    fn visit(
        items: &[TreeItem],
        expanded: &HashSet<u64>,
        target: u64,
        row: &mut usize,
    ) -> Option<usize> {
        for item in items {
            if item.id == target {
                return Some(*row);
            }
            *row += 1;
            if expanded.contains(&item.id)
                && let Some(found) = visit(&item.children, expanded, target, row)
            {
                return Some(found);
            }
        }
        None
    }
    let mut row = 0;
    visit(items, expanded, target, &mut row)
}
