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

/// Um nome de dependência instalada faz a pergunta passar adiante.
///
/// # Isto mudou na fase 5, e a mudança é o assunto dela
///
/// Antes o índice devolvia lista vazia aqui, e lista vazia **afirma** que o nome
/// não tem declaração nenhuma. Como o índice virou o principal, essa afirmação
/// falsa impediria o analisador externo de ser consultado — `Ctrl+clique` num
/// símbolo do Angular não abriria nada, e pareceria limite da IDE.
///
/// Dizendo que não alcança, o host procura quem alcance.
#[test]
fn a_name_from_a_dependency_passes_the_question_along() {
    let raiz = projeto("dependencia");
    escrever(
        &raiz.join("src/uso.ts"),
        "import { Component } from '@angular/core';
",
    );
    let ativo = ativado(&raiz);

    let Ok(texto) = std::fs::read_to_string(raiz.join("src/uso.ts")) else {
        panic!("o arquivo do teste precisa existir");
    };
    assert!(
        pollster::block_on(ativo.open_document(DocumentSnapshot {
            id: DocumentId(1),
            path: raiz.join("src/uso.ts"),
            version: 1,
            text: texto,
        }))
        .is_ok()
    );
    let resposta = pollster::block_on(ativo.definition(DefinitionRequest {
        document_id: DocumentId(1),
        position: TextPosition { line: 0, column: 10 },
    }));
    assert!(
        matches!(
            resposta,
            Err(ide_language_api::LanguageError::Unresolved(_))
        ),
        "o índice diz que não alcança, e não que não existe: {resposta:?}"
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

/// Um método do próprio arquivo não faz ninguém acordar.
///
/// # O defeito que este teste guarda
///
/// O índice registrava classe, interface, enum, apelido de tipo, função solta e
/// `const` de módulo — e **não** método de classe. `Ctrl+clique` em
/// `this.buscar()` caía sobre um nome declarado duas linhas acima, na mesma
/// tela, e o índice dizia que não alcançava.
///
/// A consequência não era só não navegar: dizer "não sei" **acorda o analisador
/// externo**, e a IDE subia um processo de 1,9 GB para responder o que estava
/// visível no arquivo aberto.
#[test]
fn a_method_in_the_same_file_wakes_nobody() {
    let raiz = projeto("metodo-local");
    let arquivo = raiz.join("src/pagina.ts");
    let texto = "export class Pagina {\n  buscar() {}\n  abrir() {\n    this.buscar();\n  }\n}\n";
    escrever(&arquivo, texto);
    let ativo = ativado(&raiz);

    // A posição sai do próprio texto: contar coluna à mão já errou nesta suíte,
    // e o erro se disfarça de defeito do código.
    let (linha, coluna) = posicao_de(texto, "this.b");
    let achados = definicao(ativo.as_ref(), &arquivo, linha, coluna);
    assert_eq!(
        achados.first().map(|local| local.path.clone()),
        Some(arquivo.clone()),
        "o método está no próprio arquivo, e a resposta não sai dele"
    );
    assert_eq!(
        achados.first().map(|local| local.range.start.line),
        Some(1),
        "e aponta para a declaração dele"
    );
    let _ = std::fs::remove_dir_all(&raiz);
}

/// Linha e coluna logo depois de um trecho, achadas no texto.
fn posicao_de(texto: &str, trecho: &str) -> (u32, u32) {
    for (numero, linha) in texto.lines().enumerate() {
        if let Some(byte) = linha.find(trecho) {
            return (
                numero as u32,
                linha[..byte + trecho.len()].chars().count() as u32,
            );
        }
    }
    panic!("o trecho {trecho:?} precisa estar no texto");
}
