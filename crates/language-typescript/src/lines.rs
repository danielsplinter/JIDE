//! Conversão entre pontos do tree-sitter e posições do domínio.
//!
//! O tree-sitter conta colunas em **bytes**; o domínio conta em caracteres. Sem
//! esta tradução, uma linha com acento — que em português é a regra, não a
//! exceção — desloca todo realce à direita dela.

use ide_domain::{TextPosition, TextRange};
use tree_sitter::{Node, Point};

pub(crate) struct LineIndex<'a> {
    source: &'a str,
    starts: Vec<usize>,
}

impl<'a> LineIndex<'a> {
    pub(crate) fn new(source: &'a str) -> Self {
        let mut starts = vec![0];
        starts.extend(
            source
                .bytes()
                .enumerate()
                .filter(|(_, byte)| *byte == b'\n')
                .map(|(offset, _)| offset + 1),
        );
        Self { source, starts }
    }

    pub(crate) const fn source(&self) -> &'a str {
        self.source
    }

    fn position(&self, point: Point) -> TextPosition {
        let line = self.line(point.row);
        let byte_column = point.column.min(line.len());
        let column = line
            .get(..byte_column)
            .map_or(0, |prefix| prefix.chars().count());
        TextPosition {
            line: point.row as u32,
            column: column as u32,
        }
    }

    /// A linha, sem a quebra — como `str::lines` a entregaria.
    fn line(&self, row: usize) -> &'a str {
        let Some(start) = self.starts.get(row).copied() else {
            return "";
        };
        let end = self
            .starts
            .get(row + 1)
            .map_or(self.source.len(), |next| next.saturating_sub(1));
        let line = self.source.get(start..end).unwrap_or_default();
        // O `\r` do CRLF não conta como coluna, igual ao que `lines` faz.
        line.strip_suffix('\r').unwrap_or(line)
    }
}

pub(crate) fn node_range(node: Node<'_>, lines: &LineIndex<'_>) -> TextRange {
    TextRange {
        start: lines.position(node.start_position()),
        end: lines.position(node.end_position()),
    }
}
