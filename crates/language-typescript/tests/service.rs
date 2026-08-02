//! O analisador externo, contra o `tsserver` de verdade.
//!
//! Estes testes exigem **Node instalado** e o pacote `typescript` no projeto de
//! teste, e por isso são `#[ignore]` — como os de `real_jvm` do adapter de
//! depuração, que exigem uma JVM. Rodam com:
//!
//! ```text
//! cargo test -p language-typescript --test service -- --ignored
//! ```
//!
//! O que **não** é ignorado é a degradação: que faltar Node ou faltar o pacote
//! do projeto produza `Unavailable`, e não travamento, roda em qualquer máquina.
//! É a parte que a ADR-025 promete, e a que não pode depender de ambiente.

use std::{path::PathBuf, sync::Arc, time::Duration};

use ide_domain::{CompletionRequest, DocumentId, DocumentSnapshot, LanguageId, TextPosition};
use ide_language_api::{
    LanguageActivationContext, LanguageError, LanguageProvider, LanguageToolchainConfig,
};
use ide_process::NativeProcessSupervisor;
use language_typescript::{TypeScriptServiceProvider, tsserver_in};

fn temporary(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("er-tsserver-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    assert!(std::fs::create_dir_all(&root).is_ok());
    root
}

fn context(root: &std::path::Path) -> LanguageActivationContext {
    LanguageActivationContext {
        workspace_root: root.to_path_buf(),
        source_roots: Vec::new(),
        toolchains: Vec::new(),
    }
}

fn provider() -> TypeScriptServiceProvider {
    TypeScriptServiceProvider::new(Arc::new(NativeProcessSupervisor::default()))
}

/// O adapter usa prazo, e prazo pede relógio.
///
/// O worker do host já sobe o runtime com o tempo habilitado, então em produção
/// isto está atendido. Aqui é preciso dizê-lo à mão: `pollster` não tem runtime
/// nenhum, e o `timeout` do pedido entraria em pânico.
fn runtime() -> tokio::runtime::Runtime {
    match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(erro) => panic!("runtime de teste: {erro}"),
    }
}

/// Sem o pacote do projeto, o analisador externo não sobe — e diz por quê.
///
/// É a metade da ADR-025 que não depende de ambiente: o provider recusa com
/// `Unavailable`, que é o que faz o host cair para o nativo. Um erro genérico
/// deixaria o `.ts` sem ninguém.
#[test]
fn without_the_project_package_the_service_declines_and_says_why() {
    let root = temporary("sem-pacote");
    let resultado = pollster::block_on(provider().activate(context(&root)));
    match resultado {
        Err(LanguageError::Unavailable(detalhe)) => {
            assert!(
                detalhe.contains("npm install"),
                "a mensagem precisa dizer o que fazer, e não só que falhou: {detalhe}"
            );
        }
        Err(outro) => panic!("faltar o pacote precisa recusar com Unavailable: {outro:?}"),
        Ok(_) => panic!("sem o pacote do projeto, o analisador externo não pode subir"),
    }
    let _ = std::fs::remove_dir_all(&root);
}

/// Um projeto sem `node_modules` não tem analisador para achar.
#[test]
fn the_locator_finds_nothing_in_an_empty_project() {
    let root = temporary("vazio");
    assert!(tsserver_in(&root).is_none());
    let _ = std::fs::remove_dir_all(&root);
}

/// O analisador é procurado subindo a árvore, como o Node resolve módulo.
///
/// É o que faz funcionar em monorepo, onde o `node_modules` fica na raiz e os
/// pacotes ficam abaixo.
#[test]
fn the_locator_climbs_to_the_root_of_a_monorepo() {
    let root = temporary("monorepo");
    let biblioteca = root.join("packages").join("loja");
    assert!(std::fs::create_dir_all(&biblioteca).is_ok());
    let lib = root
        .join("node_modules")
        .join("typescript")
        .join("lib");
    assert!(std::fs::create_dir_all(&lib).is_ok());
    assert!(std::fs::write(lib.join("tsserver.js"), "// analisador").is_ok());

    assert_eq!(tsserver_in(&biblioteca), Some(lib.join("tsserver.js")));
    let _ = std::fs::remove_dir_all(&root);
}

/// Node apontado para uma pasta que não tem Node não trava a ativação.
#[test]
fn a_wrong_node_home_falls_back_to_the_path() {
    let root = temporary("node-errado");
    let mut contexto = context(&root);
    contexto.toolchains.push(LanguageToolchainConfig {
        language_id: LanguageId("typescript".to_owned()),
        installation_root: root.join("nao-existe"),
        properties: std::collections::BTreeMap::new(),
    });
    // Sem o pacote do projeto a recusa vem de qualquer jeito; o que se cobra
    // aqui é que ela **chegue**, em vez de a ativação ficar pendurada.
    let resultado = pollster::block_on(provider().activate(contexto));
    assert!(matches!(resultado, Err(LanguageError::Unavailable(_))));
    let _ = std::fs::remove_dir_all(&root);
}

/// A completação responde com os membros do tipo certo.
///
/// É o critério da fase 3c: o que o provider nativo não faz e não vai fazer.
#[test]
#[ignore = "exige Node instalado e `npm install typescript` no projeto de teste"]
fn completion_answers_with_the_members_of_the_right_type() {
    let root = temporary("completar");
    assert!(std::fs::write(root.join("package.json"), r#"{"name":"t"}"#).is_ok());
    // No Windows o npm é um `.cmd`, e não um executável: `Command::new("npm")`
    // não o encontra. É o mesmo detalhe que o executor de tarefas já trata.
    #[cfg(windows)]
    const NPM: &str = "npm.cmd";
    #[cfg(not(windows))]
    const NPM: &str = "npm";
    let instalado = std::process::Command::new(NPM)
        .args(["install", "typescript@5", "--no-audit", "--no-fund"])
        .current_dir(&root)
        .status();
    assert!(
        instalado.is_ok_and(|status| status.success()),
        "o teste instala o TypeScript no projeto temporário"
    );

    let arquivo = root.join("pedido.ts");
    let codigo = "class Pedido { total = 0; somar(v: number) {} }\nconst p = new Pedido();\np.\n";
    assert!(std::fs::write(&arquivo, codigo).is_ok());

    let runtime = runtime();
    let ativo = match runtime.block_on(provider().activate(context(&root))) {
        Ok(ativo) => ativo,
        Err(erro) => panic!("com Node e pacote, o analisador precisa subir: {erro}"),
    };
    let documento = DocumentSnapshot {
        id: DocumentId(1),
        path: arquivo,
        version: 1,
        text: codigo.to_owned(),
    };
    assert!(runtime.block_on(ativo.open_document(documento)).is_ok());
    // O analisador precisa de um instante para montar o projeto.
    std::thread::sleep(Duration::from_secs(2));

    let itens = runtime.block_on(ativo.completion(CompletionRequest {
        document_id: DocumentId(1),
        // Depois do ponto da terceira linha.
        position: TextPosition { line: 2, column: 2 },
        prefix: String::new(),
    }));
    let itens = match itens {
        Ok(itens) => itens,
        Err(erro) => panic!("a completação precisa responder: {erro}"),
    };
    let nomes: Vec<_> = itens.iter().map(|item| item.label.as_str()).collect();
    assert!(
        nomes.contains(&"total") && nomes.contains(&"somar"),
        "os membros do tipo precisam aparecer, e vieram: {nomes:?}"
    );

    assert!(runtime.block_on(ativo.shutdown()).is_ok());
    let _ = std::fs::remove_dir_all(&root);
}

/// A busca por tipo encontra o que o projeto declara.
///
/// É o `Ctrl+L` da IDE. O provider nativo não sabe responder — sem índice, ele
/// só conhece o arquivo aberto —, e por isso a pergunta cai no analisador.
#[test]
#[ignore = "exige Node instalado e `npm install typescript` no projeto de teste"]
fn the_type_search_finds_what_the_project_declares() {
    let root = temporary("busca-tipo");
    assert!(std::fs::write(root.join("package.json"), r#"{"name":"t"}"#).is_ok());
    assert!(
        std::fs::write(
            root.join("tsconfig.json"),
            r#"{ "include": ["src/**/*.ts"] }"#,
        )
        .is_ok()
    );
    let fonte = root.join("src");
    assert!(std::fs::create_dir_all(&fonte).is_ok());
    let arquivo = fonte.join("pedido.ts");
    assert!(
        std::fs::write(
            &arquivo,
            "export class PedidoDeCompra {}\nexport interface ResumoDoPedido {}\n",
        )
        .is_ok()
    );

    #[cfg(windows)]
    const NPM: &str = "npm.cmd";
    #[cfg(not(windows))]
    const NPM: &str = "npm";
    let instalado = std::process::Command::new(NPM)
        .args(["install", "typescript@5", "--no-audit", "--no-fund"])
        .current_dir(&root)
        .status();
    assert!(instalado.is_ok_and(|status| status.success()));

    let runtime = runtime();
    let ativo = match runtime.block_on(provider().activate(context(&root))) {
        Ok(ativo) => ativo,
        Err(erro) => panic!("o analisador precisa subir: {erro}"),
    };
    assert!(
        runtime
            .block_on(ativo.open_document(DocumentSnapshot {
                id: DocumentId(1),
                path: arquivo,
                version: 1,
                text: "export class PedidoDeCompra {}\nexport interface ResumoDoPedido {}\n"
                    .to_owned(),
            }))
            .is_ok()
    );
    std::thread::sleep(Duration::from_secs(2));

    let achados = match runtime.block_on(ativo.workspace_types("Pedido", 50)) {
        Ok(achados) => achados,
        Err(erro) => panic!("a busca por tipo precisa responder: {erro}"),
    };
    let nomes: Vec<_> = achados.iter().map(|s| s.name.as_str()).collect();
    assert!(
        nomes.contains(&"PedidoDeCompra") && nomes.contains(&"ResumoDoPedido"),
        "os tipos do projeto precisam aparecer, e vieram: {nomes:?}"
    );
    // Só tipo entra: função e variável soltas encheriam a lista com o que a
    // pergunta não é.
    assert!(
        achados
            .iter()
            .all(|s| matches!(s.kind, ide_domain::SymbolKind::Class | ide_domain::SymbolKind::Interface | ide_domain::SymbolKind::Enum)),
        "a busca por tipo só devolve tipo: {achados:?}"
    );

    assert!(runtime.block_on(ativo.shutdown()).is_ok());
    let _ = std::fs::remove_dir_all(&root);
}
