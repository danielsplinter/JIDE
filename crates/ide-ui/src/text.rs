//! Posições, limites e conversões de texto usados pela interface.
//!
//! São funções puras: recebem o texto e devolvem um número ou um recorte. Viviam
//! no `ide_shell`, e cinco delas **também** no `editor` — cópias que diziam a
//! mesma coisa por caminhos diferentes. Aqui elas dizem uma vez só, e podem ser
//! testadas sem construir uma tela.

use ide_domain::{
    OutlineItem, OutlineKind, SyntaxHighlightKind, SyntaxSnapshot,
    TextPosition as DomainTextPosition,
};
use ui_editor::TokenKind;

/// Início do caractere anterior ao deslocamento.
///
/// Nunca estoura: deslocamento fora do texto é grampeado ao fim. As duas cópias
/// que existiam divergiam só nisso — uma fatiava direto e quebrava, a outra
/// devolvia zero.
pub(crate) fn previous_boundary(text: &str, cursor: usize) -> usize {
    let cursor = cursor.min(text.len());
    text.get(..cursor)
        .and_then(|prefix| prefix.char_indices().next_back().map(|(index, _)| index))
        .unwrap_or(0)
}

/// Início do caractere seguinte ao deslocamento, ou o fim do texto.
pub(crate) fn next_boundary(text: &str, cursor: usize) -> usize {
    let cursor = cursor.min(text.len());
    text.get(cursor..)
        .and_then(|suffix| suffix.chars().next())
        .map_or(text.len(), |value| cursor + value.len_utf8())
}

pub(crate) fn byte_at_column(text: &str, column: usize) -> usize {
    text.char_indices()
        .nth(column)
        .map_or(text.len(), |(index, _)| index)
}
pub(crate) fn line_column(text: &str, cursor: usize) -> (usize, usize) {
    let prefix = &text[..cursor.min(text.len())];
    let line = prefix.bytes().filter(|byte| *byte == b'\n').count();
    let column = prefix
        .rsplit('\n')
        .next()
        .unwrap_or_default()
        .chars()
        .count();
    (line, column)
}
pub(crate) fn offset_for_line_column(text: &str, target_line: usize, target_column: usize) -> usize {
    let mut offset = 0;
    for (line, value) in text.split('\n').enumerate() {
        if line == target_line {
            return offset + byte_at_column(value, target_column);
        }
        offset += value.len() + 1;
    }
    text.len()
}
/// Deslocamento em bytes onde uma linha começa.
///
/// Além da última, devolve o fim do texto: inserir depois do fim é acrescentar,
/// e é o que se quer quando o tipo fecha na última linha.
pub(crate) fn offset_of_line(text: &str, line: usize) -> usize {
    let mut offset = 0;
    for (index, value) in text.split('\n').enumerate() {
        if index == line {
            return offset;
        }
        offset += value.len() + 1;
    }
    text.len()
}
pub(crate) fn token_at(text: &str, offset: usize) -> Option<String> {
    if text.is_empty() {
        return None;
    }
    let offset = offset.min(text.len());
    let mut start = offset;
    while start > 0 {
        let previous = previous_boundary(text, start);
        let character = text[previous..start].chars().next()?;
        if !is_identifier_character(character) {
            break;
        }
        start = previous;
    }
    let mut end = offset;
    while end < text.len() {
        let next = next_boundary(text, end);
        let character = text[end..next].chars().next()?;
        if !is_identifier_character(character) {
            break;
        }
        end = next;
    }
    (start < end).then(|| text[start..end].to_owned())
}
pub(crate) fn identifier_prefix(text: &str, offset: usize) -> String {
    let offset = offset.min(text.len());
    let mut start = offset;
    while start > 0 {
        let previous = previous_boundary(text, start);
        let Some(character) = text[previous..start].chars().next() else {
            break;
        };
        if !is_identifier_character(character) {
            break;
        }
        start = previous;
    }
    text[start..offset].to_owned()
}
pub(crate) fn is_identifier_character(character: char) -> bool {
    character == '_' || character.is_alphanumeric()
}
pub(crate) fn position_in_range(line: usize, column: usize, range: ide_domain::TextRange) -> bool {
    let start = (range.start.line as usize, range.start.column as usize);
    let end = (range.end.line as usize, range.end.column as usize);
    (line, column) >= start && (line, column) < end
}
/// O item do outline é um tipo e contém a posição — ou algum filho dele é.
///
/// A busca desce a árvore porque classes aninhadas existem, e estar dentro de
/// uma interna continua sendo estar dentro de um tipo.
pub(crate) fn encloses_type(item: &OutlineItem, position: DomainTextPosition) -> bool {
    let dentro = position_in_range(position.line as usize, position.column as usize, item.range);
    let tipo = matches!(
        item.kind,
        OutlineKind::Class | OutlineKind::Interface | OutlineKind::Enum | OutlineKind::Annotation
    );
    (dentro && tipo)
        || item
            .children
            .iter()
            .any(|child| encloses_type(child, position))
}
pub(crate) fn count_outline(items: &[OutlineItem]) -> usize {
    items
        .iter()
        .map(|item| 1 + count_outline(&item.children))
        .sum()
}
/// Converte todos os realces em uma única passagem pelo texto.
///
/// O snapshot fala em linha/coluna e o editor em caracteres absolutos. Manter
/// início e tamanho de cada linha transforma cada extremo de token em consulta
/// O(1); percorrer desde a primeira linha para cada token tornava a pintura
/// quadrática em classes grandes.
pub(crate) fn converted_syntax(text: &str, snapshot: &SyntaxSnapshot) -> Vec<(usize, usize, TokenKind)> {
    let mut starts = Vec::new();
    let mut lengths = Vec::new();
    let mut offset = 0;
    for line in text.split('\n') {
        starts.push(offset);
        let length = line.chars().count();
        lengths.push(length);
        offset += length + 1;
    }
    let position = |position: DomainTextPosition| {
        let line = (position.line as usize).min(starts.len().saturating_sub(1));
        starts.get(line).copied().unwrap_or_default()
            + (position.column as usize).min(lengths.get(line).copied().unwrap_or_default())
    };
    snapshot
        .highlights
        .iter()
        .map(|highlight| {
            (
                position(highlight.range.start),
                position(highlight.range.end),
                token_kind_for(highlight.kind),
            )
        })
        .collect()
}
/// Papel do realce da IDE no vocabulário do editor da biblioteca.
pub(crate) const fn token_kind_for(kind: SyntaxHighlightKind) -> TokenKind {
    match kind {
        SyntaxHighlightKind::Keyword | SyntaxHighlightKind::Operator => TokenKind::Keyword,
        SyntaxHighlightKind::Type => TokenKind::Type,
        SyntaxHighlightKind::Function => TokenKind::Function,
        SyntaxHighlightKind::String => TokenKind::String,
        SyntaxHighlightKind::Number => TokenKind::Number,
        SyntaxHighlightKind::Comment => TokenKind::Comment,
        // Anotação e nomes comuns não têm token próprio no editor: seguem o
        // texto, que é o que o tema define para código sem classificação.
        SyntaxHighlightKind::Annotation
        | SyntaxHighlightKind::Field
        | SyntaxHighlightKind::Variable => TokenKind::Plain,
    }
}
/// Se um token realçado pode levar a uma definição.
///
/// O cursor precisa concordar com o clique. Enquanto só `Type` acendia a mão, o
/// clique navegava em método, campo e variável sem que nada na tela dissesse que
/// era possível — e o usuário concluía, com razão, que ali não funcionava.
///
/// Palavra-chave, literal, comentário e operador ficam de fora: nenhum deles
/// declara nada, e uma mão sobre cada palavra do arquivo não informa coisa
/// alguma.
pub(crate) const fn is_navigable(kind: SyntaxHighlightKind) -> bool {
    matches!(
        kind,
        SyntaxHighlightKind::Type
            | SyntaxHighlightKind::Function
            | SyntaxHighlightKind::Field
            | SyntaxHighlightKind::Variable
            | SyntaxHighlightKind::Annotation
    )
}
