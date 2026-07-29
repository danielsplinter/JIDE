//! Identidade, compactação e projeção visual da árvore do Explorer.

use std::{
    collections::{HashSet, hash_map::DefaultHasher},
    hash::{Hash, Hasher},
    path::Path,
};

use ide_workspace::FileNode;
use ui_components::TreeItem;

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

pub(super) fn is_java_source_root(path: &Path) -> bool {
    if path.file_name().and_then(|name| name.to_str()) != Some("java") {
        return false;
    }
    let mut ancestors = path.ancestors().skip(1);
    let parent = ancestors.next().and_then(Path::file_name);
    let grandparent = ancestors.next().and_then(Path::file_name);
    parent.is_some_and(|name| name == "src") || grandparent.is_some_and(|name| name == "src")
}

pub(super) fn is_java_package(path: &Path) -> bool {
    path.ancestors().skip(1).any(is_java_source_root)
}

fn compact_package_chain(node: &FileNode) -> (&FileNode, String) {
    let mut label = label(node).to_owned();
    let mut current = node;
    while is_java_package(&current.path) {
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

pub(super) fn items(node: &FileNode) -> Vec<TreeItem> {
    node.children
        .iter()
        .map(|child| {
            let (node, label) = compact_package_chain(child);
            TreeItem::new(id(&node.path), label, items(node))
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
