//! Realce e estrutura de folha de estilo, e o limite do que a gramática julga.

use ide_domain::{DocumentId, DocumentSnapshot, OutlineKind, SyntaxHighlightKind};
use ide_language_api::{ActiveLanguage, LanguageActivationContext, LanguageProvider};
use language_style::StyleLanguageProvider;

fn ativo() -> Box<dyn ActiveLanguage> {
    let context = LanguageActivationContext {
        workspace_root: std::path::PathBuf::from("/w"),
        source_roots: Vec::new(),
        toolchains: Vec::new(),
    };
    match pollster::block_on(StyleLanguageProvider::new().activate(context)) {
        Ok(ativo) => ativo,
        Err(erro) => panic!("ativação falhou: {erro}"),
    }
}

fn abrir(ativo: &dyn ActiveLanguage, nome: &str, texto: &str) -> DocumentId {
    let id = DocumentId(1);
    let documento = DocumentSnapshot {
        id,
        path: std::path::PathBuf::from("/w").join(nome),
        version: 1,
        text: texto.to_owned(),
    };
    assert!(pollster::block_on(ativo.open_document(documento)).is_ok());
    id
}

/// O provider atende `.css` e `.scss`.
///
/// Só `.css` não atenderia arquivo nenhum num projeto Angular: o `angular.json`
/// declara `"style": "scss"`.
#[test]
fn the_provider_answers_for_css_and_scss() {
    let extensoes = StyleLanguageProvider::new().metadata().extensions;
    assert_eq!(extensoes, vec!["css".to_owned(), "scss".to_owned()]);
}

#[test]
fn a_stylesheet_is_highlighted_and_navigable() {
    let ativo = ativo();
    let id = abrir(
        ativo.as_ref(),
        "tema.css",
        "/* tema */
.cartao {
  color: #333;
  font-family: \"Inter\";
}
",
    );
    let Ok(snapshot) = pollster::block_on(ativo.syntax(id)) else {
        panic!("o realce precisa responder");
    };
    for esperado in [
        SyntaxHighlightKind::Comment,
        SyntaxHighlightKind::String,
        SyntaxHighlightKind::Number,
        SyntaxHighlightKind::Field,
    ] {
        assert!(
            snapshot.highlights.iter().any(|span| span.kind == esperado),
            "faltou {esperado:?}"
        );
    }
    let nomes: Vec<_> = snapshot.outline.iter().map(|item| item.name.as_str()).collect();
    assert_eq!(nomes, vec![".cartao"], "a estrutura é a lista de regras");
    assert_eq!(snapshot.outline[0].kind, OutlineKind::Class);
}

/// Um `.css` inválido é apontado.
#[test]
fn broken_css_is_reported() {
    let ativo = ativo();
    let id = abrir(ativo.as_ref(), "quebrado.css", ".cartao { color: ;;; 
");
    let Ok(diagnosticos) = pollster::block_on(ativo.diagnostics(id)) else {
        panic!("os diagnósticos precisam responder");
    };
    assert!(!diagnosticos.is_empty(), "um .css quebrado é apontado");
}

/// **Um `.scss` válido não é acusado.**
///
/// É o caso que decidiu o desenho. A gramática é a de CSS: `$cor`, o `&:hover` e
/// o `@mixin` viram erro nela, e um arquivo correto apareceria cheio de marcas
/// vermelhas. Realçar quase tudo é útil; acusar o que não se entende é mentira.
#[test]
fn valid_scss_is_never_accused() {
    let ativo = ativo();
    let scss = "$cor: #333;
.cartao {
  color: $cor;
  &:hover { color: red; }
}
@mixin centro { display: flex; }
";
    let id = abrir(ativo.as_ref(), "tema.scss", scss);
    let Ok(diagnosticos) = pollster::block_on(ativo.diagnostics(id)) else {
        panic!("os diagnósticos precisam responder");
    };
    assert!(
        diagnosticos.is_empty(),
        "um SCSS correto não pode ser acusado por uma gramática de CSS: {diagnosticos:?}"
    );

    // E o realce continua vindo: é o que faz o arquivo deixar de ser texto cru.
    let Ok(snapshot) = pollster::block_on(ativo.syntax(id)) else {
        panic!("o realce precisa responder");
    };
    assert!(
        snapshot.highlights.len() > 5,
        "o SCSS precisa ser realçado, ainda que com buracos: {}",
        snapshot.highlights.len()
    );
}
