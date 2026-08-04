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

/// **O critério do nível 2.**
///
/// No começo de uma declaração, o que cabe é o nome de uma propriedade, e a
/// lista vem do `mdn-data`, embarcada.
#[test]
fn at_the_start_of_a_declaration_the_properties_are_offered() {
    let ativo = ativo();
    let id = abrir(ativo.as_ref(), "tema.scss", TEMA);
    let nomes = completar(ativo.as_ref(), id, depois_de(TEMA, "  col"), "col");
    assert!(nomes.contains(&"color".to_owned()), "{nomes:?}");
    assert!(nomes.contains(&"column-gap".to_owned()), "{nomes:?}");
    assert!(
        nomes.iter().all(|nome| nome.starts_with("col")),
        "o que já foi digitado estreita: {nomes:?}"
    );
}

/// Um `.css` comum recebe a mesma lista: propriedade é de CSS, e não de SCSS.
#[test]
fn plain_css_gets_the_properties_too() {
    let ativo = ativo();
    let css = ".cartao {\n  col\n}\n";
    let id = abrir(ativo.as_ref(), "tema.css", css);
    let nomes = completar(ativo.as_ref(), id, depois_de(css, "  col"), "col");
    assert!(nomes.contains(&"color".to_owned()), "{nomes:?}");
}

/// **Onde cabe um valor, não cabe um nome de propriedade.**
///
/// Depois de `color:` o que falta é o valor, e valor é o nível 3. Oferecer a
/// lista de propriedades ali seria oferecer o que não compila.
#[test]
fn in_value_position_no_property_is_offered() {
    let ativo = ativo();
    let css = ".cartao {\n  color: re\n}\n";
    let id = abrir(ativo.as_ref(), "tema.css", css);
    let nomes = completar(ativo.as_ref(), id, depois_de(css, "color: re"), "re");
    assert!(nomes.is_empty(), "valor é o nível 3: {nomes:?}");
}

/// **No topo do arquivo não cabe propriedade.**
///
/// Fora de um bloco só cabem seletor e regra `@`. A conta é de chaves abertas
/// menos fechadas, e o aninhamento do SCSS entra nela de graça.
#[test]
fn outside_a_block_no_property_is_offered() {
    let ativo = ativo();
    let css = "col\n.cartao {\n  color: #333;\n}\n";
    let id = abrir(ativo.as_ref(), "tema.css", css);
    let nomes = completar(ativo.as_ref(), id, depois_de(css, "col"), "col");
    assert!(nomes.is_empty(), "no topo não cabe propriedade: {nomes:?}");
}

/// A lista embarcada é a que a procedência declara.
///
/// Sem isto, atualizar o `mdn-data` com outro critério de corte passaria
/// despercebido até alguém notar uma propriedade faltando.
#[test]
fn a_lista_tem_o_tamanho_que_a_procedencia_declara() {
    let ativo = ativo();
    let css = ".cartao {\n  \n}\n";
    let id = abrir(ativo.as_ref(), "tema.css", css);
    let nomes = completar(ativo.as_ref(), id, depois_de(css, "  "), "");
    assert_eq!(nomes.len(), 523, "a procedência declara 523 propriedades");
    assert!(
        !nomes.iter().any(|nome| nome.starts_with('-')),
        "prefixo de fornecedor fica de fora"
    );
    assert!(
        !nomes.contains(&"grid-gap".to_owned()),
        "obsoleta fica de fora"
    );
    assert!(
        nomes.contains(&"anchor-name".to_owned()),
        "experimental entra: é o que já se escreve hoje"
    );
}

/// Um projeto de mentira, com o tema num arquivo e o componente noutro.
///
/// É o arranjo que a medição encontrou nos dois projetos de referência:
/// **as variáveis não moram no arquivo que as usa**.
struct Projeto(std::path::PathBuf);

impl Projeto {
    fn novo(nome: &str) -> Self {
        let raiz = std::env::temp_dir().join(format!("er-ide-tema-{nome}"));
        let _ = std::fs::remove_dir_all(&raiz);
        assert!(std::fs::create_dir_all(&raiz).is_ok());
        Self(raiz)
    }

    fn arquivo(&self, relativo: &str, conteudo: &str) -> std::path::PathBuf {
        let destino = self
            .0
            .join(relativo.replace('/', std::path::MAIN_SEPARATOR_STR));
        if let Some(pai) = destino.parent() {
            assert!(std::fs::create_dir_all(pai).is_ok());
        }
        assert!(std::fs::write(&destino, conteudo).is_ok());
        destino
    }

    fn ativo(&self) -> Box<dyn ActiveLanguage> {
        let context = LanguageActivationContext {
            workspace_root: self.0.clone(),
            source_roots: Vec::new(),
            toolchains: Vec::new(),
        };
        match pollster::block_on(StyleLanguageProvider::new().activate(context)) {
            Ok(ativo) => ativo,
            Err(erro) => panic!("ativação falhou: {erro}"),
        }
    }
}

impl Drop for Projeto {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn abrir_em(ativo: &dyn ActiveLanguage, caminho: &std::path::Path, texto: &str) -> DocumentId {
    let id = DocumentId(7);
    assert!(
        pollster::block_on(ativo.open_document(DocumentSnapshot {
            id,
            path: caminho.to_path_buf(),
            version: 1,
            text: texto.to_owned(),
        }))
        .is_ok()
    );
    id
}

/// **O critério do nível 1b.**
///
/// O tema declara `$cor-primaria`; o componente o traz por `@import` e não
/// declara nada. Digitar `$` no componente oferece a variável do tema.
#[test]
fn an_imported_variable_is_offered() {
    let projeto = Projeto::novo("import");
    projeto.arquivo("src/_tema.scss", "$cor-primaria: #333;\n$espaco: 8px;\n");
    let componente = "@import '../tema';\n\n.cartao {\n  color: $\n}\n";
    let caminho = projeto.arquivo("src/app/cartao.component.scss", componente);
    let ativo = projeto.ativo();
    let id = abrir_em(ativo.as_ref(), &caminho, componente);

    let nomes = completar(ativo.as_ref(), id, depois_de(componente, "color: $"), "");
    assert_eq!(
        nomes,
        vec!["cor-primaria".to_owned(), "espaco".to_owned()],
        "o que o tema declara precisa chegar a quem o importa"
    );
}

/// **O espaço de nomes é respeitado, e não é enfeite.**
///
/// Com `@use '../tema' as t`, escrever `$cor` não acha nada — o nome só existe
/// como `t.$cor`. Oferecer sem o prefixo daria uma lista que não compila.
#[test]
fn a_namespace_is_required_when_the_use_declares_one() {
    let projeto = Projeto::novo("espaco");
    projeto.arquivo("src/_tema.scss", "$cor-primaria: #333;\n");
    let componente = "@use '../tema' as t;\n\n.cartao {\n  color: t.$\n  border: $\n}\n";
    let caminho = projeto.arquivo("src/app/cartao.component.scss", componente);
    let ativo = projeto.ativo();
    let id = abrir_em(ativo.as_ref(), &caminho, componente);

    let com = completar(ativo.as_ref(), id, depois_de(componente, "color: t.$"), "");
    assert_eq!(com, vec!["cor-primaria".to_owned()], "com o prefixo, aparece");

    let sem = completar(ativo.as_ref(), id, depois_de(componente, "border: $"), "");
    assert!(sem.is_empty(), "sem o prefixo, não existe: {sem:?}");
}

/// Um especificador nu resolve em `node_modules`, que é como uma biblioteca de
/// design chega ao projeto.
#[test]
fn a_bare_specifier_reaches_the_installed_library() {
    let projeto = Projeto::novo("nu");
    projeto.arquivo(
        "node_modules/uma-lib/styles/_variables.scss",
        "$j-fis-gutter: 8px;\n",
    );
    projeto.arquivo(
        "node_modules/uma-lib/styles/_index.scss",
        "@forward './variables';\n",
    );
    let global = "@use 'uma-lib/styles' as *;\n\nbody {\n  margin: $\n}\n";
    let caminho = projeto.arquivo("src/styles.scss", global);
    let ativo = projeto.ativo();
    let id = abrir_em(ativo.as_ref(), &caminho, global);

    let nomes = completar(ativo.as_ref(), id, depois_de(global, "margin: $"), "");
    assert_eq!(nomes, vec!["j-fis-gutter".to_owned()]);
}

/// **O critério do nível 1c.**
///
/// O parcial **não importa nada** e usa `$cor-primaria`. Quem a trouxe foi o
/// arquivo que o agrega, e é lá que o escopo dele nasce. Sem a seta invertida,
/// a completação aqui é vazia — e vazia por olhar para o lado errado.
///
/// É o arranjo de 82 dos 134 arquivos que usam `$` no projeto de referência.
#[test]
fn a_partial_sees_what_its_importer_brought() {
    let projeto = Projeto::novo("de-cima");
    projeto.arquivo("src/_tema.scss", "$cor-primaria: #333;\n");
    let parcial = "  .cartao {\n  color: $\n}\n";
    let caminho = projeto.arquivo("src/componentes/_cartao.scss", parcial);
    // O agregador: traz o tema e o parcial, nessa ordem, como um `_index` faz.
    projeto.arquivo(
        "src/agregado.scss",
        "@import './tema';\n@import './componentes/cartao';\n",
    );

    let ativo = projeto.ativo();
    let id = abrir_em(ativo.as_ref(), &caminho, parcial);
    let nomes = completar(ativo.as_ref(), id, depois_de(parcial, "color: $"), "");
    assert_eq!(
        nomes,
        vec!["cor-primaria".to_owned()],
        "o parcial precisa enxergar o escopo de quem o agrega"
    );
}

/// A subida não vaza espaço de nomes.
///
/// Se o agregador trouxe o tema com `@use ... as t`, o parcial **não** enxerga
/// `$cor-primaria` sem prefixo: aquele nome só existe dentro do agregador, e
/// como `t.$cor-primaria`.
#[test]
fn a_namespaced_use_above_does_not_leak_down() {
    let projeto = Projeto::novo("nao-vaza");
    projeto.arquivo("src/_tema.scss", "$cor-primaria: #333;\n");
    let parcial = ".cartao {\n  color: $\n}\n";
    let caminho = projeto.arquivo("src/componentes/_cartao.scss", parcial);
    projeto.arquivo(
        "src/agregado.scss",
        "@use './tema' as t;\n@import './componentes/cartao';\n",
    );

    let ativo = projeto.ativo();
    let id = abrir_em(ativo.as_ref(), &caminho, parcial);
    let nomes = completar(ativo.as_ref(), id, depois_de(parcial, "color: $"), "");
    assert!(
        nomes.is_empty(),
        "um `@use` com apelido não vaza para quem foi importado: {nomes:?}"
    );
}
