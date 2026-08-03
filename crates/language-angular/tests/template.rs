//! O critério da fase 1 da `24`, contra um projeto Angular de verdade.
//!
//! **Dentro de `{{ }}` num template, a completação oferece os membros da classe
//! do componente.** E o mesmo binário atende qualquer projeto Angular, sem
//! recompilar — que é a regra de não envelhecer, cobrada como teste.
//!
//! ```text
//! ER_IDE_PROJETO_ANGULAR=C:/caminho/do/projeto cargo test -p language-angular --test template -- --ignored --nocapture
//! ```
//!
//! Opcionalmente, `ER_IDE_RESERVA_ANGULAR` aponta o diretório com o
//! `@angular/language-service` que a IDE carregaria — é o caso dos projetos que
//! não trazem o pacote, que é a maioria.
//!
//! É leitura apenas: nada aqui escreve no projeto.

use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use ide_domain::{CompletionRequest, DocumentId, DocumentSnapshot, TextPosition};
use ide_language_api::{LanguageActivationContext, LanguageProvider};
use ide_process::NativeProcessSupervisor;
use language_angular::AngularAnalyzerPlugin;
use language_typescript::TypeScriptServiceProvider;

fn projeto() -> Option<PathBuf> {
    std::env::var_os("ER_IDE_PROJETO_ANGULAR").map(PathBuf::from)
}

fn reserva() -> Option<PathBuf> {
    std::env::var_os("ER_IDE_RESERVA_ANGULAR").map(PathBuf::from)
}

fn runtime() -> tokio::runtime::Runtime {
    match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(erro) => panic!("runtime de teste: {erro}"),
    }
}

/// Um template com interpolação, e o componente que o acompanha.
///
/// Procurado no projeto, e não fixado: o teste precisa valer para qualquer
/// projeto Angular, e um caminho escrito à mão só valeria para um.
fn algum_template(raiz: &Path) -> Option<(PathBuf, String, TextPosition)> {
    let mut pilha = vec![raiz.to_path_buf()];
    while let Some(pasta) = pilha.pop() {
        for entrada in std::fs::read_dir(&pasta).into_iter().flatten().flatten() {
            let caminho = entrada.path();
            let Some(nome) = caminho.file_name().and_then(|nome| nome.to_str()) else {
                continue;
            };
            if caminho.is_dir() {
                if nome != "node_modules" && nome != "dist" && !nome.starts_with('.') {
                    pilha.push(caminho);
                }
                continue;
            }
            // Qualquer `.html` com um `.ts` de mesmo nome. Não só
            // `.component.html`: um projeto real usa `.page.html`, e a regra de
            // companhia é sobre o irmão, e não sobre o sufixo.
            if !nome.ends_with(".html") || !caminho.with_extension("ts").is_file() {
                continue;
            }
            let Ok(texto) = std::fs::read_to_string(&caminho) else {
                continue;
            };
            let Ok(componente) = std::fs::read_to_string(caminho.with_extension("ts")) else {
                continue;
            };
            if let Some(posicao) = depois_do_ponto(&texto, &componente) {
                return Some((caminho, texto, posicao));
            }
        }
    }
    None
}

/// A posição logo depois do ponto de um `{{ algo. }}` em que `algo` é **campo
/// do componente**.
///
/// # O critério da amostra, e por que ele não é escolher a dedo
///
/// Um template real tem posições que o serviço **não deve** resolver: um
/// `{{ model. }}` vindo de `let-model` num `ng-template`, cujo contexto é de uma
/// diretiva. Cobrar resposta ali seria cobrar o que não existe — e a primeira
/// execução caiu justamente num desses.
///
/// O critério não é "declarado no `.ts`", e essa foi a segunda tentativa
/// errada: ela rejeitava `@if (jogador(); as jogadorAtual)`, que **é**
/// resolvível e que o serviço resolve. O critério é o do enunciado — o nome tem
/// de estar **ligado a algo que a IDE pode ver**: um membro da classe do
/// componente, ou um apelido dado no próprio template.
///
/// O `let-` de diretiva não é nenhum dos dois, e continua de fora.
fn depois_do_ponto(texto: &str, componente: &str) -> Option<TextPosition> {
    for (numero, linha) in texto.lines().enumerate() {
        let Some(abre) = linha.find("{{") else {
            continue;
        };
        let resto = &linha[abre + 2..];
        let Some(ponto) = resto.find('.') else {
            continue;
        };
        let nome = resto[..ponto].trim();
        // Só um identificador simples: `{{ a.b.c }}` e chamadas trariam outra
        // pergunta, e a primeira é a que se quer.
        if nome.is_empty() || !nome.chars().all(|c| c.is_alphanumeric() || c == '_') {
            continue;
        }
        if !declara_o_campo(componente, nome) && !apelidado_no_template(texto, nome) {
            continue;
        }
        let coluna = linha[..abre + 2 + ponto + 1].chars().count();
        return Some(TextPosition {
            line: u32::try_from(numero).unwrap_or(u32::MAX),
            column: u32::try_from(coluna).unwrap_or(u32::MAX),
        });
    }
    None
}

/// Este nome é declarado como campo da classe do componente?
///
/// Reconhece as três formas que aparecem em componente real: a propriedade
/// escrita no corpo, e os dois lugares do construtor onde o parâmetro vira
/// campo. É leitura de texto, e não análise — basta para escolher a amostra.
fn declara_o_campo(componente: &str, nome: &str) -> bool {
    componente.lines().map(str::trim).any(|linha| {
        let Some(resto) = linha.strip_prefix(nome) else {
            // `private cartService: CartService` e `readonly x = ...`
            return ["private ", "protected ", "public ", "readonly "]
                .iter()
                .filter_map(|prefixo| linha.strip_prefix(prefixo))
                .any(|resto| {
                    resto
                        .strip_prefix(nome)
                        .is_some_and(|fim| fim.starts_with(':') || fim.starts_with(' ') || fim.starts_with('='))
                });
        };
        resto.starts_with(':') || resto.starts_with(" =") || resto.starts_with('=')
    })
}

/// Este nome é apelido dado no próprio template?
///
/// `@if (jogador(); as jogadorAtual)` e `*ngIf="x as y"` ligam um nome a uma
/// expressão que está ali mesmo, e o serviço resolve os dois. Um `let-model`,
/// não: o valor vem da diretiva, e não do template.
fn apelidado_no_template(texto: &str, nome: &str) -> bool {
    let apelido = format!("as {nome}");
    texto.contains(&apelido) && !texto.contains(&format!("let-{nome}"))
}

#[test]
#[ignore = "exige ER_IDE_PROJETO_ANGULAR com node_modules instalado; leva ~1 min"]
fn inside_an_interpolation_the_members_of_the_component_are_offered() {
    let Some(raiz) = projeto() else {
        panic!("aponte ER_IDE_PROJETO_ANGULAR para um projeto Angular");
    };
    assert!(
        language_angular::e_angular(&raiz),
        "o projeto precisa ter @angular/core instalado: {raiz:?}"
    );

    let plugin = match reserva() {
        Some(caminho) => AngularAnalyzerPlugin::with_fallback(caminho),
        None => AngularAnalyzerPlugin::new(),
    };
    let provider = TypeScriptServiceProvider::new(Arc::new(NativeProcessSupervisor::default()))
        .with_plugin_source(Arc::new(plugin));

    // O `.html` precisa estar entre o que este provider atende — é a regra de
    // companhia que o põe lá, e sem ela o host nem encaminharia o documento.
    assert!(
        provider
            .metadata()
            .extensions
            .iter()
            .any(|extensao| extensao == "html"),
        "o provider precisa atender templates: {:?}",
        provider.metadata().extensions
    );

    let Some((caminho, texto, posicao)) = algum_template(&raiz) else {
        panic!("o projeto precisa ter um `.component.html` com uma interpolação");
    };
    println!("template: {caminho:?}  posição: {posicao:?}");

    let runtime = runtime();
    let contexto = LanguageActivationContext {
        workspace_root: raiz.clone(),
        source_roots: Vec::new(),
        toolchains: Vec::new(),
    };
    let ativo = match runtime.block_on(provider.activate(contexto)) {
        Ok(ativo) => ativo,
        Err(erro) => panic!("o analisador precisa subir: {erro}"),
    };

    let inicio = std::time::Instant::now();
    assert!(
        runtime
            .block_on(ativo.open_document(DocumentSnapshot {
                id: DocumentId(1),
                path: caminho,
                version: 1,
                text: texto,
            }))
            .is_ok(),
        "o template precisa abrir no analisador"
    );
    println!("o template abriu em {:?}", inicio.elapsed());

    let itens = match runtime.block_on(ativo.completion(CompletionRequest {
        document_id: DocumentId(1),
        position: posicao,
        prefix: String::new(),
    })) {
        Ok(itens) => itens,
        Err(erro) => panic!("a completação no template não pode falhar: {erro}"),
    };
    println!(
        "{} itens: {:?}",
        itens.len(),
        itens
            .iter()
            .take(12)
            .map(|item| item.label.as_str())
            .collect::<Vec<_>>()
    );

    assert!(
        !itens.is_empty(),
        "dentro de {{{{ }}}} a completação precisa oferecer os membros do tipo"
    );

    assert!(runtime.block_on(ativo.shutdown()).is_ok());
}
