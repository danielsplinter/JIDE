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
