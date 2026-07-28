#![doc = "Tipos de domínio independentes de linguagem e infraestrutura."]

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

macro_rules! numeric_id {
    ($name:ident) => {
        #[derive(
            Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize,
        )]
        pub struct $name(pub u64);
    };
}

numeric_id!(DocumentId);
numeric_id!(WorkspaceId);
numeric_id!(ProjectId);
numeric_id!(SymbolId);
numeric_id!(ProcessId);
numeric_id!(RequestId);

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct LanguageId(pub String);

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct ProviderId(pub String);

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct TextPosition {
    pub line: u32,
    pub column: u32,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct TextRange {
    pub start: TextPosition,
    pub end: TextPosition,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DocumentSnapshot {
    pub id: DocumentId,
    pub path: PathBuf,
    pub version: u64,
    pub text: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DocumentChange {
    pub document_id: DocumentId,
    pub version: u64,
    pub range: Option<TextRange>,
    pub text: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Location {
    pub path: PathBuf,
    pub range: TextRange,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DiagnosticSeverity {
    Error,
    Warning,
    Information,
    Hint,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Diagnostic {
    pub range: TextRange,
    pub severity: DiagnosticSeverity,
    pub message: String,
    pub source: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SyntaxHighlightKind {
    Keyword,
    Type,
    Function,
    Field,
    Variable,
    String,
    Number,
    Comment,
    Annotation,
    Operator,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SyntaxHighlight {
    pub range: TextRange,
    pub kind: SyntaxHighlightKind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OutlineKind {
    Class,
    Interface,
    Enum,
    Annotation,
    Constructor,
    Method,
    Field,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OutlineItem {
    pub name: String,
    pub kind: OutlineKind,
    pub range: TextRange,
    pub children: Vec<OutlineItem>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImportItem {
    pub path: String,
    pub is_static: bool,
    pub wildcard: bool,
    pub range: TextRange,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SyntaxNode {
    pub kind: String,
    pub range: TextRange,
    pub has_error: bool,
    pub children: Vec<SyntaxNode>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SyntaxSnapshot {
    pub document_id: DocumentId,
    pub version: u64,
    pub tree: SyntaxNode,
    pub outline: Vec<OutlineItem>,
    pub highlights: Vec<SyntaxHighlight>,
    pub imports: Vec<ImportItem>,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SymbolKind {
    Package,
    Class,
    /// Registro — uma classe, mas declarada por `record`.
    Record,
    Interface,
    Enum,
    /// Constante de enumeração, como um item declarado dentro de um `enum`.
    EnumConstant,
    Annotation,
    Constructor,
    Method,
    Field,
    Parameter,
    LocalVariable,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TypeDescriptor {
    pub name: String,
    pub array_dimensions: u8,
    pub generic_arguments: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SemanticSymbol {
    pub name: String,
    pub kind: SymbolKind,
    pub location: Location,
    pub type_descriptor: Option<TypeDescriptor>,
    pub scope_depth: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SemanticScope {
    pub range: TextRange,
    pub depth: u32,
    pub symbols: Vec<usize>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SemanticSnapshot {
    pub document_id: DocumentId,
    pub version: u64,
    pub symbols: Vec<SemanticSymbol>,
    pub scopes: Vec<SemanticScope>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompletionKind {
    Keyword,
    Class,
    Interface,
    Enum,
    Constructor,
    Method,
    Field,
    Variable,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompletionItem {
    pub label: String,
    pub detail: Option<String>,
    pub kind: CompletionKind,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompletionRequest {
    pub document_id: DocumentId,
    pub position: TextPosition,
    pub prefix: String,
}

/// Acesso a membro sendo digitado: `pedido.` ou `pedido.get`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MemberAccess {
    /// Expressão à esquerda do ponto.
    pub receiver: String,
    /// O que já foi digitado depois do ponto.
    pub prefix: String,
}

/// Lê para trás a partir do cursor procurando um acesso por ponto.
///
/// Só o identificador imediatamente antes do ponto conta. Encadeamento
/// (`a.b().c.`) exigiria saber o tipo de retorno de cada elo.
///
/// Mora aqui, e não na linguagem, porque quem pergunta não é sempre um arquivo:
/// o editor de expressões do depurador faz a mesma leitura sobre um texto que
/// não é documento nenhum, e as duas precisam concordar sobre onde o receptor
/// termina e o filtro começa.
#[must_use]
pub fn member_access(text: &str, offset: usize) -> Option<MemberAccess> {
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
    // Ponto sem identificador à esquerda é literal decimal ou erro de digitação,
    // não acesso a membro.
    (!receiver.is_empty() && !receiver.starts_with(|value: char| value.is_ascii_digit()))
        .then_some(MemberAccess { receiver, prefix })
}

#[must_use]
pub fn is_identifier_char(value: char) -> bool {
    value.is_alphanumeric() || value == '_' || value == '$'
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DefinitionRequest {
    pub document_id: DocumentId,
    pub position: TextPosition,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReferencesRequest {
    pub document_id: DocumentId,
    pub position: TextPosition,
    pub include_declaration: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn range_preserves_zero_based_positions() {
        let range = TextRange {
            start: TextPosition { line: 2, column: 4 },
            end: TextPosition { line: 2, column: 9 },
        };
        assert_eq!(range.start.line, 2);
        assert_eq!(range.end.column, 9);
    }

    #[test]
    fn domain_manifest_has_no_infrastructure_dependency() {
        let manifest = include_str!("../Cargo.toml");
        for forbidden in ["wgpu", "winit", "tree-sitter", "tokio", "tracing"] {
            assert!(
                !manifest.contains(forbidden),
                "{forbidden} leaked into ide-domain"
            );
        }
    }
}
