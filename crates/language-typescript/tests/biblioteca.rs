//! O ponto sobre um tipo que o TypeScript traz, e não o projeto.
//!
//! É o critério da fase 7 da `25`: `let w: String[];` e um ponto trazem
//! `forEach`, `map` e `length` **sem o analisador externo de pé** — e um projeto
//! que compila para ES2022 não recebe o que só existe em ES2024.
//!
//! # A biblioteca aqui é de mentira, e é de propósito
//!
//! Um teste que leia o `node_modules` da máquina passa ou falha conforme o que
//! está instalado, e não conforme o código. O que se afirma aqui é a **regra**,
//! e ela cabe em cinco arquivos. Contra o TypeScript de verdade há um teste
//! `#[ignore]` em `project::stdlib`, que é onde a escala se mede.

use std::path::{Path, PathBuf};

use ide_domain::{CompletionRequest, DocumentId, DocumentSnapshot, TextPosition};
use ide_language_api::{ActiveLanguage, LanguageActivationContext, LanguageProvider};
use language_typescript::TypeScriptLanguageProvider;

/// Um projeto com TypeScript "instalado", compilando para ES2022.
fn projeto(nome: &str, alvo: &str) -> PathBuf {
    let raiz = std::env::temp_dir().join(format!("er-ts-bib-{nome}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&raiz);
    let lib = raiz.join("node_modules/typescript/lib");
    assert!(std::fs::create_dir_all(&lib).is_ok());
    assert!(std::fs::create_dir_all(raiz.join("src")).is_ok());
    escrever(
        &raiz.join("tsconfig.json"),
        &format!("{{ \"compilerOptions\": {{ \"target\": \"{alvo}\" }} }}"),
    );
    // **Uma versão que não pode existir.** O cache da biblioteca é chaveado pela
    // versão do TypeScript, e não pelo projeto — dois projetos com o mesmo
    // TypeScript têm a mesma biblioteca. Um teste que se diga `5.9.3` grava por
    // cima do cache do TypeScript de verdade da máquina, e o projeto seguinte
    // abriria com quatro tipos de mentira no lugar de mil e quinhentos.
    //
    // Todos os testes daqui usam a mesma, e isso é de propósito: o alvo entra na
    // **resposta**, e não no cache, então dois projetos com alvos diferentes
    // compartilham o arquivo e recebem listas diferentes. É o desenho da fase 7,
    // e `the_project_target_decides_what_is_offered` o exercita.
    escrever(
        &raiz.join("node_modules/typescript/package.json"),
        "{\"version\": \"0.0.0-teste\"}",
    );
    // `Array` reaberta em três arquivos, que é o caso que obriga a fusão.
    escrever(
        &lib.join("lib.es5.d.ts"),
        "interface Array<T> {\n  length: number;\n  forEach(f: any): void;\n  map(f: any): any;\n}\n\
         interface String {\n  charAt(i: number): string;\n  trim(): string;\n}\n\
         interface EventTarget {\n  addEventListener(t: string): void;\n}\n\
         interface Node extends EventTarget {\n  nodeName: string;\n}\n",
    );
    escrever(
        &lib.join("lib.es2022.array.d.ts"),
        "interface Array<T> {\n  at(i: number): T;\n}\n",
    );
    escrever(
        &lib.join("lib.es2024.array.d.ts"),
        "interface Array<T> {\n  daFrente(): T;\n}\n",
    );
    escrever(
        &lib.join("lib.dom.d.ts"),
        "interface HTMLElement extends Node {\n  click(): void;\n}\n",
    );
    escrever(
        &lib.join("lib.es2022.full.d.ts"),
        "/// <reference no-default-lib=\"true\"/>\n\
         /// <reference lib=\"es5\" />\n\
         /// <reference lib=\"es2022.array\" />\n\
         /// <reference lib=\"dom\" />\n",
    );
    escrever(
        &lib.join("lib.es2024.full.d.ts"),
        "/// <reference no-default-lib=\"true\"/>\n\
         /// <reference lib=\"es5\" />\n\
         /// <reference lib=\"es2022.array\" />\n\
         /// <reference lib=\"es2024.array\" />\n\
         /// <reference lib=\"dom\" />\n",
    );
    raiz
}

fn escrever(caminho: &Path, conteudo: &str) {
    if let Some(pasta) = caminho.parent() {
        assert!(std::fs::create_dir_all(pasta).is_ok());
    }
    assert!(std::fs::write(caminho, conteudo).is_ok());
}

/// Ativa o provider e **espera a biblioteca ficar lida**.
///
/// Ela sobe na linha de execução do índice, e sem esperar por ela o teste
/// mediria a corrida entre duas threads em vez do que o código faz.
fn ativado(raiz: &Path) -> Box<dyn ActiveLanguage> {
    let ativo = match pollster::block_on(TypeScriptLanguageProvider::new().activate(
        LanguageActivationContext {
            workspace_root: raiz.to_path_buf(),
            source_roots: vec![raiz.join("src")],
            toolchains: Vec::new(),
        },
    )) {
        Ok(ativo) => ativo,
        Err(erro) => panic!("o provider nativo precisa ativar: {erro}"),
    };
    assert!(
        pollster::block_on(ativo.wait_until_indexed(std::time::Duration::from_secs(60))),
        "a preparação do projeto precisa terminar"
    );
    ativo
}

/// A posição logo depois de um trecho, achada no próprio texto.
fn depois_de(texto: &str, trecho: &str) -> TextPosition {
    for (numero, linha) in texto.lines().enumerate() {
        if let Some(byte) = linha.find(trecho) {
            return TextPosition {
                line: numero as u32,
                column: (linha[..byte + trecho.len()].chars().count()) as u32,
            };
        }
    }
    panic!("o trecho {trecho:?} precisa estar no texto");
}

fn completar(ativo: &dyn ActiveLanguage, caminho: &Path, trecho: &str) -> Vec<String> {
    let Ok(texto) = std::fs::read_to_string(caminho) else {
        panic!("o arquivo do teste precisa existir: {caminho:?}");
    };
    let position = depois_de(&texto, trecho);
    assert!(
        pollster::block_on(ativo.open_document(DocumentSnapshot {
            id: DocumentId(1),
            path: caminho.to_path_buf(),
            version: 1,
            text: texto,
        }))
        .is_ok()
    );
    match pollster::block_on(ativo.completion(CompletionRequest {
        document_id: DocumentId(1),
        position,
        prefix: String::new(),
    })) {
        Ok(itens) => itens.into_iter().map(|item| item.label).collect(),
        Err(erro) => panic!("`{trecho}` precisa completar: {erro}"),
    }
}

/// **O critério da fase.** `String[]` traz os membros de um vetor.
///
/// É o caso que abriu esta fase, dito por quem usa: digitar `w.` depois de
/// `let w: String[];` esperava cinco segundos pelo analisador, e o índice não
/// tinha o que responder porque `String` é do TypeScript, e não do projeto.
#[test]
fn an_array_of_a_library_type_completes_without_the_analyzer() {
    let raiz = projeto("vetor", "ES2022");
    let arquivo = raiz.join("src/uso.ts");
    escrever(&arquivo, "let w: String[];\nw.\n");

    let ativo = ativado(&raiz);
    let itens = completar(ativo.as_ref(), &arquivo, "w.");
    assert!(
        itens.contains(&"forEach".to_owned())
            && itens.contains(&"map".to_owned())
            && itens.contains(&"length".to_owned()),
        "os membros de um vetor: {itens:?}"
    );
    assert!(
        !itens.contains(&"charAt".to_owned()),
        "`String[]` é um `Array`, e não um `String`: {itens:?}"
    );
    let _ = std::fs::remove_dir_all(&raiz);
}

/// **O `string` minúsculo também.**
///
/// É a anotação mais comum de todas, e não é um nome declarado em lugar nenhum:
/// quem declara `charAt` e `trim` é a `interface String`.
#[test]
fn a_lowercase_primitive_completes_from_its_interface() {
    let raiz = projeto("primitivo", "ES2022");
    let arquivo = raiz.join("src/uso.ts");
    escrever(&arquivo, "const nome: string = '';\nnome.\n");

    let ativo = ativado(&raiz);
    let itens = completar(ativo.as_ref(), &arquivo, "nome.");
    assert!(
        itens.contains(&"charAt".to_owned()) && itens.contains(&"trim".to_owned()),
        "os membros de uma string: {itens:?}"
    );
    let _ = std::fs::remove_dir_all(&raiz);
}

/// **A herança do DOM vem junto.**
///
/// `HTMLElement` declara quase nada por conta própria; `addEventListener` vem de
/// `EventTarget`, dois degraus acima. Uma lista sem a cadeia pareceria certa e
/// estaria quase vazia.
#[test]
fn the_dom_inheritance_chain_is_followed() {
    let raiz = projeto("dom", "ES2022");
    let arquivo = raiz.join("src/uso.ts");
    escrever(&arquivo, "const alvo: HTMLElement = null!;\nalvo.\n");

    let ativo = ativado(&raiz);
    let itens = completar(ativo.as_ref(), &arquivo, "alvo.");
    assert!(
        itens.contains(&"click".to_owned())
            && itens.contains(&"nodeName".to_owned())
            && itens.contains(&"addEventListener".to_owned()),
        "a cadeia HTMLElement → Node → EventTarget: {itens:?}"
    );
    let _ = std::fs::remove_dir_all(&raiz);
}

/// **O alvo do projeto corta o que ele não pode usar.**
///
/// Oferecer um método de ES2024 num projeto ES2022 é sugerir código que o build
/// recusa — e o erro só apareceria na compilação, longe de onde a sugestão
/// apareceu.
#[test]
fn the_project_target_decides_what_is_offered() {
    let arquivo_relativo = "src/uso.ts";

    let antigo = projeto("alvo-2022", "ES2022");
    escrever(&antigo.join(arquivo_relativo), "let w: String[];\nw.\n");
    let itens = completar(
        ativado(&antigo).as_ref(),
        &antigo.join(arquivo_relativo),
        "w.",
    );
    assert!(
        !itens.contains(&"daFrente".to_owned()),
        "ES2024 não vale num projeto ES2022: {itens:?}"
    );
    let _ = std::fs::remove_dir_all(&antigo);

    let novo = projeto("alvo-2024", "ES2024");
    escrever(&novo.join(arquivo_relativo), "let w: String[];\nw.\n");
    let itens = completar(ativado(&novo).as_ref(), &novo.join(arquivo_relativo), "w.");
    assert!(
        itens.contains(&"daFrente".to_owned()),
        "o mesmo código, com outro alvo, alcança mais: {itens:?}"
    );
    let _ = std::fs::remove_dir_all(&novo);
}

/// **O projeto vence a biblioteca.**
///
/// Um projeto pode declarar o próprio `String`, e quando declara é dele que se
/// está falando. Responder com o da linguagem seria a IDE contradizendo o código
/// que está na tela.
#[test]
fn what_the_project_declares_outranks_the_library() {
    let raiz = projeto("proprio", "ES2022");
    let arquivo = raiz.join("src/uso.ts");
    escrever(
        &arquivo,
        "class String {\n  meuProprio(): void {}\n}\nconst s: String = null!;\ns.\n",
    );

    let ativo = ativado(&raiz);
    let itens = completar(ativo.as_ref(), &arquivo, "s.");
    assert!(
        itens.contains(&"meuProprio".to_owned()),
        "o `String` do projeto: {itens:?}"
    );
    assert!(
        !itens.contains(&"charAt".to_owned()),
        "o da linguagem não pode se sobrepor ao do projeto: {itens:?}"
    );
    let _ = std::fs::remove_dir_all(&raiz);
}
