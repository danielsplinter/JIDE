//! Resolução semântica de receptores e tipos.

use ide_domain::{SemanticSymbol, SymbolKind};

pub(super) fn receiver_type(receiver: &str, symbols: &[SemanticSymbol]) -> String {
    symbols
        .iter()
        .filter(|symbol| {
            matches!(
                symbol.kind,
                SymbolKind::LocalVariable | SymbolKind::Parameter | SymbolKind::Field
            )
        })
        .find(|symbol| symbol.name == receiver)
        .and_then(|symbol| symbol.type_descriptor.as_ref())
        .map_or_else(|| receiver.to_owned(), |descriptor| descriptor.name.clone())
}
