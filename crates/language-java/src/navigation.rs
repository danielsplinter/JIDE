//! Utilitários de navegação por posição e token.

use ide_domain::{TextPosition, TextRange};

pub(super) fn within(range: &TextRange, position: TextPosition) -> bool {
    let point = (position.line, position.column);
    point >= (range.start.line, range.start.column) && point <= (range.end.line, range.end.column)
}

pub(super) fn token_at_position(text: &str, position: TextPosition) -> String {
    let offset = match super::offset_for_position(text, position) {
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
