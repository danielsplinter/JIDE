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

/// Onde o cursor está, em linha e coluna, logo depois da marca.
fn depois_de(texto: &str, marca: &str) -> ide_domain::TextPosition {
    for (numero, linha) in texto.lines().enumerate() {
        if let Some(coluna) = linha.find(marca) {
            return ide_domain::TextPosition {
                line: numero as u32,
                column: (coluna + marca.chars().count()) as u32,
            };
        }
    }
    panic!("a marca {marca:?} precisa existir no texto");
}

fn completar(
    ativo: &dyn ActiveLanguage,
    id: DocumentId,
    posicao: ide_domain::TextPosition,
    prefixo: &str,
) -> Vec<String> {
    let pedido = ide_domain::CompletionRequest {
        document_id: id,
        position: posicao,
        prefix: prefixo.to_owned(),
    };
    match pollster::block_on(ativo.completion(pedido)) {
        Ok(itens) => itens.into_iter().map(|item| item.label).collect(),
        Err(erro) => panic!("a completação precisa responder: {erro}"),
    }
}

const TEMA: &str = "$cor-primaria: #333;
$cor-secundaria: #666;
$espaco: 8px;

%base {
  margin: 0;
}

.cartao {
  color: $cor-primaria;
}
";

/// **O critério do nível 1 da fase 5 da `23`.**
///
/// Num `.scss` que declara `$cor-primaria`, digitar `$` dentro de uma regra
/// oferece as variáveis daquele arquivo.
#[test]
fn typing_a_dollar_offers_the_variables_this_file_declares() {
    let ativo = ativo();
    let id = abrir(ativo.as_ref(), "tema.scss", TEMA);
    let nomes = completar(ativo.as_ref(), id, depois_de(TEMA, "color: $"), "");
    assert_eq!(
        nomes,
        vec![
            "cor-primaria".to_owned(),
            "cor-secundaria".to_owned(),
            "espaco".to_owned()
        ]
    );
}

/// O rótulo **não** traz o sigilo, e isso não é descuido.
///
/// A interface substitui o trecho de identificador antes do cursor, e `$` não é
/// identificador para ela. Com o sigilo no rótulo, aceitar depois de digitar
/// `$` escreveria `$$cor`. Ele vai no `detail`, que é o que a lista mostra.
#[test]
fn the_label_carries_no_sigil_and_the_detail_does() {
    let ativo = ativo();
    let id = abrir(ativo.as_ref(), "tema.scss", TEMA);
    let pedido = ide_domain::CompletionRequest {
        document_id: id,
        position: depois_de(TEMA, "color: $"),
        prefix: String::new(),
    };
    let Ok(itens) = pollster::block_on(ativo.completion(pedido)) else {
        panic!("a completação precisa responder");
    };
    let Some(primeiro) = itens.first() else {
        panic!("a lista não pode estar vazia");
    };
    assert_eq!(primeiro.label, "cor-primaria");
    assert_eq!(primeiro.detail.as_deref(), Some("$cor-primaria"));
}

/// O que já foi digitado estreita a lista.
///
/// A posição e o prefixo andam juntos: com `es` digitado, o cursor está dois
/// caracteres à frente do `$`. A primeira versão deste teste passava a posição
/// do `$` com o prefixo `es`, combinação que a interface nunca produz — e
/// cobrava do provider uma resposta para um estado impossível.
#[test]
fn what_was_typed_narrows_the_list() {
    let ativo = ativo();
    let digitando = "$cor-primaria: #333;\n$espaco: 8px;\n\n.cartao {\n  margin: $es\n}\n";
    let id = abrir(ativo.as_ref(), "tema.scss", digitando);
    let nomes = completar(
        ativo.as_ref(),
        id,
        depois_de(digitando, "margin: $es"),
        "es",
    );
    assert_eq!(nomes, vec!["espaco".to_owned()]);
}

/// **Uso não é declaração.**
///
/// `color: $cor-primaria` menciona a variável, e não a declara. Contá-lo
/// encheria a lista de repetição, e num arquivo grande a lista seria o
/// histórico de uso em vez do que existe.
#[test]
fn a_use_is_not_a_declaration() {
    let ativo = ativo();
    let usa_sem_declarar = ".cartao {\n  color: $vinda-de-fora;\n}\n";
    let id = abrir(ativo.as_ref(), "uso.scss", usa_sem_declarar);
    let nomes = completar(
        ativo.as_ref(),
        id,
        depois_de(usa_sem_declarar, "color: $"),
        "",
    );
    assert!(nomes.is_empty(), "só declarações entram: {nomes:?}");
}

#[test]
fn a_percent_offers_the_placeholders() {
    let ativo = ativo();
    let id = abrir(ativo.as_ref(), "tema.scss", TEMA);
    let nomes = completar(ativo.as_ref(), id, depois_de(TEMA, "%"), "");
    assert_eq!(nomes, vec!["base".to_owned()]);
}

/// **Fora de um nome que o arquivo inventa, a resposta é vazia — e não um erro.**
///
/// Nome de propriedade é o nível 2, e ainda não existe. Uma lista vazia diz
/// isso; um erro faria a IDE reclamar de uma posição comum.
#[test]
fn outside_a_sigil_the_answer_is_empty_and_not_a_failure() {
    let ativo = ativo();
    let id = abrir(ativo.as_ref(), "tema.scss", TEMA);
    let nomes = completar(ativo.as_ref(), id, depois_de(TEMA, "  col"), "col");
    assert!(nomes.is_empty(), "nome de propriedade é o nível 2: {nomes:?}");
}

/// Um `.css` sem variável nenhuma não oferece nada, e não falha.
#[test]
fn plain_css_offers_nothing_and_does_not_fail() {
    let ativo = ativo();
    let css = ".cartao {\n  color: #333;\n}\n";
    let id = abrir(ativo.as_ref(), "tema.css", css);
    let nomes = completar(ativo.as_ref(), id, depois_de(css, "  col"), "col");
    assert!(nomes.is_empty());
}
