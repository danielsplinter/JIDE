//! Utilitários de navegação por posição e token.

use ide_domain::{TextPosition, TextRange};
use ide_language_api::MemberAccess;

pub(super) fn member_access(text: &str, offset: usize) -> Option<MemberAccess> {
    let head = &text[..offset.min(text.len())];
    let prefix_start = head
        .char_indices()
        .rev()
        .find(|(_, value)| !is_identifier_char(*value))
        .map_or(0, |(index, value)| index + value.len_utf8());
    let prefix = head[prefix_start..].to_owned();
    let before = head[..prefix_start].trim_end();
    let receiver_end = before.strip_suffix('.')?.trim_end();
    let receiver_start = receiver_end
        .char_indices()
        .rev()
        .find(|(_, value)| !is_identifier_char(*value))
        .map_or(0, |(index, value)| index + value.len_utf8());
    let receiver = receiver_end[receiver_start..].to_owned();
    (!receiver.is_empty() && !receiver.starts_with(|value: char| value.is_ascii_digit()))
        .then_some(MemberAccess { receiver, prefix })
}

fn is_identifier_char(value: char) -> bool {
    value.is_alphanumeric() || value == '_' || value == '$'
}

pub(super) fn within(range: &TextRange, position: TextPosition) -> bool {
    let point = (position.line, position.column);
    point >= (range.start.line, range.start.column) && point <= (range.end.line, range.end.column)
}

pub(super) fn token_at_position(text: &str, position: TextPosition) -> String {
    let offset = match crate::analyzer::language::offset_for_position(text, position) {
        Ok(offset) => offset,
        Err(_) => return String::new(),
    };
    let mut start = offset;
    while start > 0 {
        let previous = text[..start]
            .char_indices()
            .last()
            .map_or(0, |(index, _)| index);
        let character = text[previous..start].chars().next();
        if !character.is_some_and(|character| character == '_' || character.is_alphanumeric()) {
            break;
        }
        start = previous;
    }
    let mut end = offset;
    while end < text.len() {
        let Some(character) = text[end..].chars().next() else {
            break;
        };
        if character != '_' && !character.is_alphanumeric() {
            break;
        }
        end += character.len_utf8();
    }
    text[start..end].to_owned()
}
