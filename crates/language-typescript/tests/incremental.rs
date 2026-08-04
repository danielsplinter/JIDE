//! A reanálise incremental **não pode** mudar a resposta.
//!
//! O tree-sitter reaproveita a árvore anterior quando recebe o `InputEdit` que
//! descreve a edição. Se esse `InputEdit` estiver errado por um byte, ele
//! reaproveita nós em posições que mudaram — e o resultado é uma árvore certa
//! para um texto que não é este, **sem erro nenhum**. Realce e navegação
//! passariam a apontar para o lugar errado, calados.
//!
//! Por isso o critério destes testes não é "funciona": é **igual ao do zero**.

use ide_domain::{DocumentChange, DocumentId, DocumentSnapshot, TextPosition, TextRange};
use ide_language_api::{ActiveLanguage, LanguageActivationContext, LanguageProvider};
use language_typescript::TypeScriptLanguageProvider;

fn ativado() -> Box<dyn ActiveLanguage> {
    let raiz = std::env::temp_dir().join(format!("er-ts-inc-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&raiz);
    match pollster::block_on(TypeScriptLanguageProvider::new().activate(
        LanguageActivationContext {
            workspace_root: raiz,
            source_roots: Vec::new(),
            toolchains: Vec::new(),
        },
    )) {
        Ok(ativo) => ativo,
        Err(erro) => panic!("o provider precisa ativar: {erro}"),
    }
}

fn abrir(ativo: &dyn ActiveLanguage, id: DocumentId, texto: &str) {
    assert!(
        pollster::block_on(ativo.open_document(DocumentSnapshot {
            id,
            path: std::path::PathBuf::from("arquivo.ts"),
            version: 1,
            text: texto.to_owned(),
        }))
        .is_ok()
    );
}

fn realce(ativo: &dyn ActiveLanguage, id: DocumentId) -> ide_domain::SyntaxSnapshot {
    match pollster::block_on(ativo.syntax(id)) {
        Ok(snapshot) => snapshot,
        Err(erro) => panic!("o realce precisa responder: {erro}"),
    }
}

/// Aplica a edição por intervalo e compara com abrir o resultado do zero.
fn incremental_bate_com_do_zero(antes: &str, range: TextRange, inserido: &str, depois: &str) {
    let ativo = ativado();
    abrir(ativo.as_ref(), DocumentId(1), antes);
    assert!(
        pollster::block_on(ativo.change_document(DocumentChange {
            document_id: DocumentId(1),
            version: 2,
            range: Some(range),
            text: inserido.to_owned(),
        }))
        .is_ok()
    );
    let incremental = realce(ativo.as_ref(), DocumentId(1));

    let do_zero = ativado();
    abrir(do_zero.as_ref(), DocumentId(2), depois);
    let inteiro = realce(do_zero.as_ref(), DocumentId(2));

    assert_eq!(
        incremental.highlights, inteiro.highlights,
        "o realce incremental precisa ser o mesmo do zero"
    );
    assert_eq!(
        incremental.outline, inteiro.outline,
        "a estrutura incremental precisa ser a mesma do zero"
    );
    assert_eq!(
        incremental.diagnostics, inteiro.diagnostics,
        "os diagnósticos incrementais precisam ser os mesmos do zero"
    );
}

const BASE: &str = "export class Pedido {\n  private total = 0;\n\n  somar(valor: number): void {\n    this.total += valor;\n  }\n}\n";

#[test]
fn inserting_a_character_matches_a_full_parse() {
    let depois = BASE.replacen("somar", "somarX", 1);
    incremental_bate_com_do_zero(
        BASE,
        TextRange {
            start: TextPosition { line: 3, column: 7 },
            end: TextPosition { line: 3, column: 7 },
        },
        "X",
        &depois,
    );
}

#[test]
fn deleting_a_range_matches_a_full_parse() {
    let depois = BASE.replacen("  private total = 0;\n", "", 1);
    incremental_bate_com_do_zero(
        BASE,
        TextRange {
            start: TextPosition { line: 1, column: 0 },
            end: TextPosition { line: 2, column: 0 },
        },
        "",
        &depois,
    );
}

#[test]
fn inserting_a_whole_line_matches_a_full_parse() {
    let depois = BASE.replacen(
        "  somar(",
        "  dobrar(): void {}\n\n  somar(",
        1,
    );
    incremental_bate_com_do_zero(
        BASE,
        TextRange {
            start: TextPosition { line: 3, column: 2 },
            end: TextPosition { line: 3, column: 2 },
        },
        "dobrar(): void {}\n\n  ",
        &depois,
    );
}

/// **O caso que a conversão de unidade quebraria.**
///
/// O `InputEdit` conta colunas em bytes e o domínio conta em caracteres. Numa
/// linha com acento — que em português é a regra — os dois divergem, e a árvore
/// reaproveitada ficaria deslocada.
#[test]
fn editing_after_an_accent_matches_a_full_parse() {
    let antes = "const título = 'ação';\nconst outro = título;\n";
    let depois = "const título = 'ações';\nconst outro = título;\n";
    incremental_bate_com_do_zero(
        antes,
        TextRange {
            start: TextPosition {
                line: 0,
                column: 20,
            },
            end: TextPosition {
                line: 0,
                column: 20,
            },
        },
        "s",
        depois,
    );
}

/// Sem intervalo é substituição do documento inteiro: não há o que reaproveitar.
#[test]
fn replacing_the_whole_document_matches_a_full_parse() {
    let ativo = ativado();
    abrir(ativo.as_ref(), DocumentId(1), BASE);
    let novo = "export interface Cliente {\n  nome: string;\n}\n";
    assert!(
        pollster::block_on(ativo.change_document(DocumentChange {
            document_id: DocumentId(1),
            version: 2,
            range: None,
            text: novo.to_owned(),
        }))
        .is_ok()
    );
    let incremental = realce(ativo.as_ref(), DocumentId(1));

    let do_zero = ativado();
    abrir(do_zero.as_ref(), DocumentId(2), novo);
    assert_eq!(incremental.highlights, realce(do_zero.as_ref(), DocumentId(2)).highlights);
}
