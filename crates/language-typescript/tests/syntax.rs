//! O que o provider nativo responde sobre um `.ts`.
//!
//! Ele não sabe tipo nenhum, e é de propósito: o que se cobra aqui é realce,
//! estrutura e erro de sintaxe — o chão da fase 1 da `23`.

use ide_domain::{
    DocumentChange, DocumentId, DocumentSnapshot, LanguageId, OutlineKind, SyntaxHighlightKind,
    TextPosition, TextRange,
};
use ide_language_api::{ActiveLanguage, LanguageActivationContext, LanguageProvider};
use language_typescript::{TYPESCRIPT_LANGUAGE_ID, TypeScriptLanguageProvider};

const PEDIDO: &str = r#"// um comentário
import { Item } from "./item";

export class Pedido {
  private total: number = 0;

  adicionar(item: Item): void {
    this.total += item.preço;
  }
}

export interface Resumo {
  total: number;
}
"#;

fn ativo() -> Box<dyn ActiveLanguage> {
    let provider = TypeScriptLanguageProvider::new();
    let context = LanguageActivationContext {
        workspace_root: std::path::PathBuf::from("/w"),
        source_roots: Vec::new(),
        toolchains: Vec::new(),
    };
    match pollster::block_on(provider.activate(context)) {
        Ok(ativo) => ativo,
        Err(error) => panic!("ativação falhou: {error}"),
    }
}

fn abrir(ativo: &dyn ActiveLanguage, texto: &str) -> DocumentId {
    let id = DocumentId(1);
    let documento = DocumentSnapshot {
        id,
        path: std::path::PathBuf::from("/w/pedido.ts"),
        version: 1,
        text: texto.to_owned(),
    };
    assert!(pollster::block_on(ativo.open_document(documento)).is_ok());
    id
}

#[test]
fn the_provider_declares_typescript_and_answers_for_ts_files() {
    let provider = TypeScriptLanguageProvider::new();
    let metadata = provider.metadata();
    assert_eq!(
        metadata.language_id,
        LanguageId(TYPESCRIPT_LANGUAGE_ID.to_owned())
    );
    assert_eq!(metadata.extensions, vec!["ts".to_owned()]);
    // O ponto **passou** a valer, na fase 4 da `25`. Até ali não havia o que
    // oferecer depois dele, e prometer completação que adivinha seria pior do
    // que não prometer nada; agora há o que oferecer quando o receptor tem tipo
    // declarado, e quando não tem, a resposta é dizer que não se sabe.
    assert_eq!(metadata.trigger_characters, vec!['.']);
}

#[test]
fn the_structure_of_the_file_is_navigable() {
    let ativo = ativo();
    let id = abrir(ativo.as_ref(), PEDIDO);
    let Ok(snapshot) = pollster::block_on(ativo.syntax(id, None)) else {
        panic!("o realce precisa responder");
    };

    let nomes: Vec<_> = snapshot
        .outline
        .iter()
        .map(|item| (item.name.as_str(), item.kind))
        .collect();
    assert_eq!(
        nomes,
        vec![
            ("Pedido", OutlineKind::Class),
            ("Resumo", OutlineKind::Interface)
        ],
        "o `export` embrulha a declaração e não pode virar um item sem nome"
    );

    let membros: Vec<_> = snapshot.outline[0]
        .children
        .iter()
        .map(|item| item.name.as_str())
        .collect();
    assert_eq!(membros, vec!["total", "adicionar"]);
}

#[test]
fn the_highlight_covers_comment_string_keyword_and_type() {
    let ativo = ativo();
    let id = abrir(ativo.as_ref(), PEDIDO);
    let Ok(snapshot) = pollster::block_on(ativo.syntax(id, None)) else {
        panic!("o realce precisa responder");
    };
    for esperado in [
        SyntaxHighlightKind::Comment,
        SyntaxHighlightKind::String,
        SyntaxHighlightKind::Keyword,
        SyntaxHighlightKind::Type,
        SyntaxHighlightKind::Number,
    ] {
        assert!(
            snapshot.highlights.iter().any(|span| span.kind == esperado),
            "faltou {esperado:?} no realce"
        );
    }
}

/// A coluna do domínio conta caracteres; o tree-sitter conta bytes.
///
/// Sem a conversão, `preço` desloca todo realce à direita dele na mesma linha —
/// e em português o acento é a regra, não a exceção.
#[test]
fn an_accent_does_not_shift_the_highlight_to_the_right() {
    let ativo = ativo();
    let id = abrir(ativo.as_ref(), "const preço = 1;\nconst depois = 2;\n");
    let Ok(snapshot) = pollster::block_on(ativo.syntax(id, None)) else {
        panic!("o realce precisa responder");
    };
    let numero = snapshot
        .highlights
        .iter()
        .find(|span| span.kind == SyntaxHighlightKind::Number && span.range.start.line == 0);
    let Some(numero) = numero else {
        panic!("o número da primeira linha precisa ter realce");
    };
    // `const preço = ` são 14 caracteres; em bytes seriam 15, por causa do `ç`.
    assert_eq!(numero.range.start.column, 14);
}

#[test]
fn a_syntax_error_is_reported_and_the_rest_still_answers() {
    let ativo = ativo();
    let id = abrir(ativo.as_ref(), "class Quebrado {\n  metodo(: void {}\n");
    let Ok(diagnosticos) = pollster::block_on(ativo.diagnostics(id)) else {
        panic!("os diagnósticos precisam responder");
    };
    assert!(
        !diagnosticos.is_empty(),
        "um arquivo que não fecha precisa ser apontado"
    );
    assert!(
        pollster::block_on(ativo.syntax(id, None)).is_ok(),
        "erro de sintaxe não pode calar o realce do resto"
    );
}

#[test]
fn typing_updates_the_answer() {
    let ativo = ativo();
    let id = abrir(ativo.as_ref(), "const a = 1;\n");
    let mudanca = DocumentChange {
        document_id: id,
        version: 2,
        range: Some(TextRange {
            start: TextPosition { line: 0, column: 6 },
            end: TextPosition { line: 0, column: 7 },
        }),
        text: "renomeado".to_owned(),
    };
    assert!(pollster::block_on(ativo.change_document(mudanca)).is_ok());
    let Ok(snapshot) = pollster::block_on(ativo.syntax(id, None)) else {
        panic!("o realce precisa responder depois da mudança");
    };
    assert_eq!(snapshot.version, 2);
    assert!(
        snapshot
            .highlights
            .iter()
            .any(|span| span.range.start.column == 6 && span.range.end.column == 15),
        "o identificador novo precisa aparecer com o tamanho novo"
    );
}

#[test]
fn closing_the_document_forgets_it() {
    let ativo = ativo();
    let id = abrir(ativo.as_ref(), PEDIDO);
    assert!(pollster::block_on(ativo.close_document(id)).is_ok());
    assert!(
        pollster::block_on(ativo.syntax(id, None)).is_err(),
        "documento fechado não pode continuar respondendo"
    );
}
