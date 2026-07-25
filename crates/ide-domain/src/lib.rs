#![doc = "Tipos de domínio independentes de linguagem e infraestrutura."]

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

macro_rules! numeric_id {
    ($name:ident) => {
        #[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
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
            assert!(!manifest.contains(forbidden), "{forbidden} leaked into ide-domain");
        }
    }
}

