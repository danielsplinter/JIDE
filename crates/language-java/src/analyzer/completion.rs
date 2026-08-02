//! Ordenação, filtragem e identidade de itens de completação.

use std::collections::HashSet;

use ide_domain::CompletionItem;

pub(super) fn finish_member_list(
    mut items: Vec<CompletionItem>,
    prefix: &str,
) -> Vec<CompletionItem> {
    let mut seen = HashSet::new();
    items.retain(|item| item.label.starts_with(prefix) && seen.insert(item.label.clone()));
    items.sort_by(|left, right| left.label.cmp(&right.label));
    items.truncate(100);
    items
}

pub(super) fn member_name(label: &str) -> &str {
    label.strip_suffix("()").unwrap_or(label)
}
