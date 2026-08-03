//! `Ctrl+clique` abre a declaração certa, e não a primeira com aquele nome.
//!
//! É o critério da fase 3 da `25`. O teste que importa é o do **nome repetido em
//! dois módulos**: em Java, pacote e classpath tornam um nome globalmente
//! resolvível; em TypeScript quem decide é o `import`, e abrir o primeiro que o
//! índice achasse seria abrir o errado com a mesma cara de certo.
//!
//! Estes não dependem de projeto de fora: eles montam o caso, porque o caso é o
//! ponto. A conferência contra o analisador num projeto real está em
//! `modules.rs`, que é o que garante que a resolução concorda com o TypeScript.

use std::path::{Path, PathBuf};

use ide_domain::{DefinitionRequest, DocumentId, DocumentSnapshot, TextPosition};
use ide_language_api::{ActiveLanguage, LanguageActivationContext, LanguageProvider};
use language_typescript::TypeScriptLanguageProvider;

fn projeto(nome: &str) -> PathBuf {
    let raiz = std::env::temp_dir().join(format!("er-ts-def-{nome}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&raiz);
    assert!(std::fs::create_dir_all(&raiz).is_ok());
    raiz
}

fn escrever(caminho: &Path, conteudo: &str) {
    if let Some(pasta) = caminho.parent() {
        assert!(std::fs::create_dir_all(pasta).is_ok());
    }
    assert!(std::fs::write(caminho, conteudo).is_ok());
}

fn ativado(raiz: &Path) -> Box<dyn ActiveLanguage> {
    let provider = TypeScriptLanguageProvider::new();
    match pollster::block_on(provider.activate(LanguageActivationContext {
        workspace_root: raiz.to_path_buf(),
        source_roots: Vec::new(),
        toolchains: Vec::new(),
    })) {
        Ok(ativo) => ativo,
        Err(erro) => panic!("o provider nativo precisa ativar: {erro}"),
    }
}

/// Abre o arquivo e pergunta pela definição na posição dada.
fn definicao(
    ativo: &dyn ActiveLanguage,
    caminho: &Path,
    linha: u32,
    coluna: u32,
) -> Vec<ide_domain::Location> {
    let Ok(texto) = std::fs::read_to_string(caminho) else {
        panic!("o arquivo do teste precisa existir: {caminho:?}");
    };
    assert!(
        pollster::block_on(ativo.open_document(DocumentSnapshot {
            id: DocumentId(1),
            path: caminho.to_path_buf(),
            version: 1,
            text: texto,
        }))
        .is_ok()
    );
    match pollster::block_on(ativo.definition(DefinitionRequest {
        document_id: DocumentId(1),
        position: TextPosition {
            line: linha,
            column: coluna,
        },
    })) {
        Ok(achados) => achados,
        Err(erro) => panic!("a definição precisa responder: {erro}"),
    }
}

/// **O teste que importa.** Dois módulos declaram o mesmo nome.
///
/// Um índice que respondesse "quem se chama assim" acertaria por sorte metade
/// das vezes. Quem decide é o `import` do arquivo que pergunta.
#[test]
fn the_same_name_in_two_modules_resolves_by_the_import() {
    let raiz = projeto("nome-repetido");
    escrever(
        &raiz.join("src/a/login.service.ts"),
        "export class LoginService {}\n",
    );
    escrever(
        &raiz.join("src/b/login.service.ts"),
        "export class LoginService {}\n",
    );
    escrever(
        &raiz.join("src/usa-a.ts"),
        "import { LoginService } from './a/login.service';\n",
    );
    escrever(
        &raiz.join("src/usa-b.ts"),
        "import { LoginService } from './b/login.service';\n",
    );
    let ativo = ativado(&raiz);

    let de_a = definicao(ativo.as_ref(), &raiz.join("src/usa-a.ts"), 0, 10);
    assert_eq!(
        de_a.first().map(|local| local.path.clone()),
        Some(raiz.join("src/a/login.service.ts")),
        "quem importa de `a` tem de abrir o de `a`"
    );

    let de_b = definicao(ativo.as_ref(), &raiz.join("src/usa-b.ts"), 0, 10);
    assert_eq!(
        de_b.first().map(|local| local.path.clone()),
        Some(raiz.join("src/b/login.service.ts")),
        "e quem importa de `b`, o de `b` — é o `import` que decide"
    );
    let _ = std::fs::remove_dir_all(&raiz);
}

/// O nome declarado no próprio arquivo não sai dele.
#[test]
fn a_name_declared_here_does_not_leave_the_file() {
    let raiz = projeto("mesmo-arquivo");
    let arquivo = raiz.join("src/pedido.ts");
    escrever(&arquivo, "export class Pedido {}\nconst p: Pedido = null!;\n");
    let ativo = ativado(&raiz);

    let achados = definicao(ativo.as_ref(), &arquivo, 1, 9);
    assert_eq!(
        achados.first().map(|local| local.path.clone()),
        Some(arquivo.clone())
    );
    assert_eq!(
        achados.first().map(|local| local.range.start.line),
        Some(0),
        "a declaração está na primeira linha"
    );
    let _ = std::fs::remove_dir_all(&raiz);
}

/// O barril é atravessado até quem declara de verdade.
///
/// Importar de `./modelo` abre o arquivo que **declara**, e não o `index.ts` que
/// só reexporta — parar no barril seria abrir um arquivo sem o que se procura.
#[test]
fn the_barrel_is_not_the_answer() {
    let raiz = projeto("barril");
    escrever(
        &raiz.join("src/modelo/pedido.ts"),
        "export class Pedido {}\n",
    );
    escrever(&raiz.join("src/modelo/index.ts"), "export * from './pedido';\n");
    escrever(
        &raiz.join("src/uso.ts"),
        "import { Pedido } from './modelo';\n",
    );
    let ativo = ativado(&raiz);

    let achados = definicao(ativo.as_ref(), &raiz.join("src/uso.ts"), 0, 10);
    assert_eq!(
        achados.first().map(|local| local.path.clone()),
        Some(raiz.join("src/modelo/pedido.ts")),
        "quem declara é o alvo, e não o barril que reexporta"
    );
    let _ = std::fs::remove_dir_all(&raiz);
}

/// Um apelido do `paths` chega ao mesmo lugar que o caminho relativo chegaria.
#[test]
fn an_alias_reaches_the_declaration() {
    let raiz = projeto("apelido");
    escrever(
        &raiz.join("libs/core/public_api.ts"),
        "export * from './pedido';\n",
    );
    escrever(&raiz.join("libs/core/pedido.ts"), "export class Pedido {}\n");
    escrever(
        &raiz.join("tsconfig.json"),
        r#"{ "compilerOptions": { "baseUrl": ".", "paths": { "@loja/core": ["libs/core/public_api"] } } }"#,
    );
    escrever(
        &raiz.join("src/uso.ts"),
        "import { Pedido } from '@loja/core';\n",
    );
    let ativo = ativado(&raiz);

    let achados = definicao(ativo.as_ref(), &raiz.join("src/uso.ts"), 0, 10);
    assert_eq!(
        achados.first().map(|local| local.path.clone()),
        Some(raiz.join("libs/core/pedido.ts")),
        "o apelido leva ao barril, e o barril à declaração"
    );
    let _ = std::fs::remove_dir_all(&raiz);
}

/// Um nome de dependência instalada não é resolvido, e isso não é erro.
///
/// Lista vazia é o que faz o host encaminhar a pergunta a quem alcança mais.
/// Um erro faria o provider parecer quebrado, e devolver a declaração errada
/// seria pior do que as duas.
#[test]
fn a_name_from_a_dependency_is_left_to_someone_else() {
    let raiz = projeto("dependencia");
    escrever(
        &raiz.join("src/uso.ts"),
        "import { Component } from '@angular/core';\n",
    );
    let ativo = ativado(&raiz);

    let achados = definicao(ativo.as_ref(), &raiz.join("src/uso.ts"), 0, 10);
    assert!(
        achados.is_empty(),
        "o índice responde pelo projeto, e diz que não alcança: {achados:?}"
    );
    let _ = std::fs::remove_dir_all(&raiz);
}

/// `import { A as B }` abre a declaração de `A`, e não procura por `B`.
///
/// O nome sob o cursor é `B`; o arquivo de destino não conhece esse nome. Sem
/// distinguir os dois, a navegação não acharia nada e pareceria um limite do
/// índice, quando é só troca de nome.
#[test]
fn a_renamed_import_still_opens_the_declaration() {
    let raiz = projeto("renomeado");
    escrever(&raiz.join("src/modelo.ts"), "export class Pedido {}\n");
    escrever(
        &raiz.join("src/uso.ts"),
        "import { Pedido as PedidoAntigo } from './modelo';\n",
    );
    let ativo = ativado(&raiz);

    let achados = definicao(ativo.as_ref(), &raiz.join("src/uso.ts"), 0, 20);
    assert_eq!(
        achados.first().map(|local| local.path.clone()),
        Some(raiz.join("src/modelo.ts"))
    );
    let _ = std::fs::remove_dir_all(&raiz);
}
