//! Tradução entre documentos da UI, eventos e contratos de linguagem.

use ide_domain::{DocumentChange, DocumentSnapshot, TextPosition, TextRange};

pub(super) struct ToolEvent {
    pub(super) status: String,
    pub(super) stdout: String,
    pub(super) stderr: String,
}

pub(super) fn document_change(
    previous: &DocumentSnapshot,
    current: &DocumentSnapshot,
) -> DocumentChange {
    let prefix = common_prefix_boundary(&previous.text, &current.text);
    let suffix = common_suffix_boundary(&previous.text, &current.text, prefix);
    let old_end = previous.text.len().saturating_sub(suffix);
    let new_end = current.text.len().saturating_sub(suffix);
    DocumentChange {
        document_id: current.id,
        version: current.version,
        range: Some(TextRange {
            start: position_at_offset(&previous.text, prefix),
            end: position_at_offset(&previous.text, old_end),
        }),
        text: current.text[prefix..new_end].to_owned(),
    }
}

fn common_prefix_boundary(left: &str, right: &str) -> usize {
    left.char_indices()
        .zip(right.char_indices())
        .take_while(|((_, left), (_, right))| left == right)
        .map(|((index, character), _)| index + character.len_utf8())
        .last()
        .unwrap_or(0)
}

fn common_suffix_boundary(left: &str, right: &str, prefix: usize) -> usize {
    left[prefix..]
        .chars()
        .rev()
        .zip(right[prefix..].chars().rev())
        .take_while(|(left, right)| left == right)
        .map(|(character, _)| character.len_utf8())
        .sum()
}

pub(super) fn position_at_offset(text: &str, offset: usize) -> TextPosition {
    let prefix = &text[..offset.min(text.len())];
    let line = prefix.bytes().filter(|byte| *byte == b'\n').count();
    let column = prefix
        .rsplit_once('\n')
        .map_or(prefix, |(_, line)| line)
        .chars()
        .count();
    TextPosition {
        line: line as u32,
        column: column as u32,
    }
}
