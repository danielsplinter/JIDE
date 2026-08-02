//! Mapeamento entre arquivos do workspace e classes do alvo.
//!
//! Tudo aqui é função pura: recebe caminhos, assinaturas e tabelas de linha, e
//! decide qual classe e qual índice de código correspondem a uma linha do
//! editor. Nenhuma decisão depende de estar conectado.

use std::path::{Path, PathBuf};

use ide_debug_api::relative_to_source_root;

/// Nome totalmente qualificado da classe de topo de um arquivo fonte.
///
/// `com/example/Main.java` produz `com.example.Main`.
#[must_use]
pub(crate) fn fully_qualified_name(relative: &Path) -> Option<String> {
    let stem = relative.file_stem()?.to_str()?;
    let mut segments: Vec<&str> = relative
        .parent()
        .map(|parent| {
            parent
                .components()
                .filter_map(|component| component.as_os_str().to_str())
                .collect()
        })
        .unwrap_or_default();
    segments.push(stem);
    let name = segments.join(".");
    (!name.is_empty()).then_some(name)
}

/// Padrão aceito por `ClassMatch`, cobrindo também classes internas e anônimas.
#[must_use]
pub(crate) fn class_match_pattern(fully_qualified: &str) -> String {
    format!("{fully_qualified}*")
}

/// Prefixo da assinatura JNI da classe: `com.example.Main` vira `Lcom/example/Main`.
#[must_use]
pub(crate) fn signature_prefix(fully_qualified: &str) -> String {
    format!("L{}", fully_qualified.replace('.', "/"))
}

/// Aceita a própria classe e suas internas, nunca uma classe de nome parecido.
#[must_use]
pub(crate) fn signature_matches(signature: &str, prefix: &str) -> bool {
    let Some(rest) = signature.strip_prefix(prefix) else {
        return false;
    };
    rest == ";" || rest.starts_with('$')
}

/// Arquivo do workspace correspondente a uma assinatura de classe.
#[must_use]
pub(crate) fn source_path(signature: &str, source_roots: &[PathBuf]) -> Option<PathBuf> {
    let inner = signature
        .strip_prefix('L')
        .and_then(|value| value.strip_suffix(';'))?;
    let top_level = inner.split('$').next().unwrap_or(inner);
    let relative = format!("{top_level}.java");
    source_roots
        .iter()
        .map(|root| root.join(&relative))
        .find(|candidate| candidate.is_file())
        .or_else(|| source_roots.first().map(|root| root.join(&relative)))
}

/// Entrada da tabela de linhas: índice de código e número da linha, 1-based.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct LineEntry {
    pub(crate) index: u64,
    pub(crate) line: i32,
}

/// Escolhe onde instalar um breakpoint pedido para `line` (1-based).
///
/// Linhas em branco, comentários e declarações sem código não aparecem na
/// tabela; nesses casos o breakpoint desce para a próxima linha executável, e
/// quem chamou informa ao usuário a linha efetiva.
#[must_use]
pub(crate) fn best_entry(entries: &[LineEntry], line: i32) -> Option<LineEntry> {
    entries
        .iter()
        .filter(|entry| entry.line == line)
        .min_by_key(|entry| entry.index)
        .copied()
        .or_else(|| {
            entries
                .iter()
                .filter(|entry| entry.line > line)
                .min_by_key(|entry| (entry.line, entry.index))
                .copied()
        })
}

/// Caminho do arquivo relativo à raiz de código que o contém.
#[must_use]
pub(crate) fn relative_source(path: &Path, source_roots: &[PathBuf]) -> Option<PathBuf> {
    relative_to_source_root(path, source_roots).map(Path::to_path_buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derives_class_name_pattern_and_signature_from_a_source_path() {
        let name = fully_qualified_name(Path::new("com/example/Main.java"));
        assert_eq!(name.as_deref(), Some("com.example.Main"));
        assert_eq!(class_match_pattern("com.example.Main"), "com.example.Main*");
        assert_eq!(signature_prefix("com.example.Main"), "Lcom/example/Main");
        assert_eq!(
            fully_qualified_name(Path::new("Main.java")).as_deref(),
            Some("Main")
        );
    }

    #[test]
    fn signature_matching_accepts_inner_classes_and_rejects_similar_names() {
        let prefix = signature_prefix("com.example.Main");
        assert!(signature_matches("Lcom/example/Main;", &prefix));
        assert!(signature_matches("Lcom/example/Main$1;", &prefix));
        assert!(signature_matches("Lcom/example/Main$Inner;", &prefix));
        assert!(!signature_matches("Lcom/example/MainHelper;", &prefix));
        assert!(!signature_matches("Lcom/other/Main;", &prefix));
    }

    #[test]
    fn source_path_uses_the_top_level_class_of_inner_classes() {
        let roots = vec![PathBuf::from("/w/src/main/java")];
        assert_eq!(
            source_path("Lcom/example/Main$1;", &roots),
            Some(PathBuf::from("/w/src/main/java/com/example/Main.java"))
        );
        assert!(source_path("not-a-signature", &roots).is_none());
    }

    #[test]
    fn breakpoint_moves_to_the_next_executable_line_when_needed() {
        let entries = [
            LineEntry { index: 0, line: 10 },
            LineEntry { index: 8, line: 10 },
            LineEntry {
                index: 12,
                line: 14,
            },
        ];
        assert_eq!(
            best_entry(&entries, 10),
            Some(LineEntry { index: 0, line: 10 }),
            "linha exata usa o menor índice de código"
        );
        assert_eq!(
            best_entry(&entries, 12),
            Some(LineEntry {
                index: 12,
                line: 14
            }),
            "linha sem código desce para a próxima executável"
        );
        assert_eq!(best_entry(&entries, 20), None);
        assert_eq!(best_entry(&[], 1), None);
    }
}
