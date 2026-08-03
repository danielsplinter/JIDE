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

use ide_domain::{
    CompletionRequest, DocumentChange, DocumentId, DocumentSnapshot, LanguageId, TextPosition,
    TextRange,
};
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

/// A instalação detectada diz qual versão é.
///
/// A IDE resolve o Node pelo `PATH` do próprio processo, e quem troca de versão
/// com um gerenciador muda o `PATH` do **shell** — a IDE aberta não vê a troca.
/// Não há como ler o shell de outra pessoa; há como mostrar qual foi encontrada,
/// e é isso que permite a quem usa perceber que não é a esperada.
#[test]
#[ignore = "exige Node no PATH"]
fn the_detected_node_says_which_version_it_is() {
    use ide_toolchain_api::{DetectionContext, ToolchainProvider};
    let runtime = runtime();
    let encontradas = match runtime.block_on(
        language_typescript::NodeToolchainProvider::new()
            .detect(DetectionContext { workspace_root: None }),
    ) {
        Ok(encontradas) => encontradas,
        Err(erro) => panic!("a detecção precisa responder: {erro}"),
    };
    let Some(node) = encontradas.first() else {
        panic!("com Node no PATH, a detecção precisa achar alguma coisa");
    };
    let Some(versao) = node.version.as_deref() else {
        panic!("a instalação precisa dizer a versão, e não só o caminho");
    };
    assert!(
        versao.starts_with('v'),
        "a versão vem como o Node a relata: {versao}"
    );
}

/// Digitar manda só o que mudou, e o analisador enxerga o que foi digitado.
///
/// Este é o teste que autoriza a mudança incremental a existir. Reabrir o
/// arquivo inteiro a cada tecla sempre funcionou; mandar intervalo só funciona
/// se linha e coluna casarem com as do analisador, e um intervalo aplicado no
/// lugar errado **não volta como erro** — volta como resposta errada daqui em
/// diante. A prova pedida é que o membro recém-digitado apareça na completação:
/// se o intervalo tivesse caído um caractere fora, a classe estaria quebrada e
/// nada apareceria.
#[test]
#[ignore = "exige Node instalado e `npm install typescript` no projeto de teste"]
fn an_incremental_change_lands_where_the_analyzer_thinks_it_did() {
    let root = temporary("mudanca-intervalo");
    assert!(std::fs::write(root.join("package.json"), r#"{"name":"t"}"#).is_ok());
    #[cfg(windows)]
    const NPM: &str = "npm.cmd";
    #[cfg(not(windows))]
    const NPM: &str = "npm";
    let instalado = std::process::Command::new(NPM)
        .args(["install", "typescript@5", "--no-audit", "--no-fund"])
        .current_dir(&root)
        .status();
    assert!(instalado.is_ok_and(|status| status.success()));

    let arquivo = root.join("pedido.ts");
    let codigo = "class Pedido { total = 0; }\nconst p = new Pedido();\np.\n";
    assert!(std::fs::write(&arquivo, codigo).is_ok());

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
                text: codigo.to_owned(),
            }))
            .is_ok()
    );
    std::thread::sleep(Duration::from_secs(2));

    // Digitar um membro novo dentro da classe: intervalo vazio, logo depois do
    // `;` da linha zero. É exatamente o que uma tecla produz.
    let ponto = TextPosition { line: 0, column: 25 };
    assert!(
        runtime
            .block_on(ativo.change_document(DocumentChange {
                document_id: DocumentId(1),
                version: 2,
                range: Some(TextRange { start: ponto, end: ponto }),
                text: " desconto = 0;".to_owned(),
            }))
            .is_ok()
    );
    std::thread::sleep(Duration::from_secs(1));

    let itens = runtime.block_on(ativo.completion(CompletionRequest {
        document_id: DocumentId(1),
        position: TextPosition { line: 2, column: 2 },
        prefix: String::new(),
    }));
    let itens = match itens {
        Ok(itens) => itens,
        Err(erro) => panic!("a completação precisa responder depois da mudança: {erro}"),
    };
    let nomes: Vec<_> = itens.iter().map(|item| item.label.as_str()).collect();
    assert!(
        nomes.contains(&"desconto"),
        "o membro digitado precisa existir para o analisador, e vieram: {nomes:?}"
    );
    // O que já estava não pode ter sido sobrescrito pelo intervalo.
    assert!(
        nomes.contains(&"total"),
        "a mudança por intervalo não pode comer o que estava lá: {nomes:?}"
    );

    assert!(runtime.block_on(ativo.shutdown()).is_ok());
    let _ = std::fs::remove_dir_all(&root);
}

/// Intervalo que não cabe no texto reabre o arquivo, e o analisador acompanha.
///
/// É a válvula: espelho e editor discordando é a situação em que mandar intervalo
/// envenenaria o buffer do analisador em silêncio. Reabrir custa caro e
/// ressincroniza os dois, e este teste cobra que o caminho caro **funcione**, e
/// não só que ele exista.
#[test]
#[ignore = "exige Node instalado e `npm install typescript` no projeto de teste"]
fn a_range_that_does_not_fit_falls_back_to_reopening() {
    let root = temporary("intervalo-invalido");
    assert!(std::fs::write(root.join("package.json"), r#"{"name":"t"}"#).is_ok());
    #[cfg(windows)]
    const NPM: &str = "npm.cmd";
    #[cfg(not(windows))]
    const NPM: &str = "npm";
    let instalado = std::process::Command::new(NPM)
        .args(["install", "typescript@5", "--no-audit", "--no-fund"])
        .current_dir(&root)
        .status();
    assert!(instalado.is_ok_and(|status| status.success()));

    let arquivo = root.join("pedido.ts");
    let codigo = "class Pedido { total = 0; }\nconst p = new Pedido();\np.\n";
    assert!(std::fs::write(&arquivo, codigo).is_ok());

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
                text: codigo.to_owned(),
            }))
            .is_ok()
    );
    std::thread::sleep(Duration::from_secs(2));

    // Uma linha que não existe. O texto guardado é remendado do jeito que der, e
    // o arquivo inteiro vai para o analisador.
    let fora = TextPosition { line: 40, column: 0 };
    assert!(
        runtime
            .block_on(ativo.change_document(DocumentChange {
                document_id: DocumentId(1),
                version: 2,
                range: Some(TextRange { start: fora, end: fora }),
                text: "\nclass Recibo {}\n".to_owned(),
            }))
            .is_ok()
    );
    std::thread::sleep(Duration::from_secs(1));

    let achados = match runtime.block_on(ativo.workspace_types("Recibo", 20)) {
        Ok(achados) => achados,
        Err(erro) => panic!("a busca precisa responder depois da reabertura: {erro}"),
    };
    let nomes: Vec<_> = achados.iter().map(|s| s.name.as_str()).collect();
    assert!(
        nomes.contains(&"Recibo"),
        "reabrir precisa entregar o texto novo ao analisador, e veio: {nomes:?}"
    );

    assert!(runtime.block_on(ativo.shutdown()).is_ok());
    let _ = std::fs::remove_dir_all(&root);
}

/// SONDAGEM: a coluna do analisador conta caractere ou unidade UTF-16?
///
/// Nós contamos **caractere**. O TypeScript trabalha em UTF-16 por dentro, e um
/// emoji vale um caractere e duas unidades — se o protocolo seguir o interno,
/// tudo depois dele na linha tem coluna diferente, e a mudança por intervalo
/// escreve um caractere fora do lugar sem erro nenhum a apontar.
///
/// **A montação é o que torna a sondagem decisiva.** A primeira tentativa
/// inseria código, e um deslocamento produzia sintaxe quebrada — da qual o
/// analisador se recupera, e o teste passava do mesmo jeito. Aqui se troca a
/// primeira letra do membro: acertando, o membro passa a se chamar `Xesconto`;
/// errando por um, a troca cai no espaço anterior e ele continua `desconto`.
/// Os dois resultados são programas válidos, e só um deles é o certo.
#[test]
#[ignore = "exige Node instalado e `npm install typescript` no projeto de teste"]
fn an_astral_character_before_the_edit_does_not_shift_it() {
    let root = temporary("emoji");
    assert!(std::fs::write(root.join("package.json"), r#"{"name":"t"}"#).is_ok());
    #[cfg(windows)]
    const NPM: &str = "npm.cmd";
    #[cfg(not(windows))]
    const NPM: &str = "npm";
    let instalado = std::process::Command::new(NPM)
        .args(["install", "typescript@5", "--no-audit", "--no-fund"])
        .current_dir(&root)
        .status();
    assert!(instalado.is_ok_and(|status| status.success()));

    let arquivo = root.join("pedido.ts");
    let codigo = "const e = \"\u{1F642}\"; class Pedido { desconto = 0; }\nconst p = new Pedido();\np.\n";
    assert!(std::fs::write(&arquivo, codigo).is_ok());

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
                text: codigo.to_owned(),
            }))
            .is_ok()
    );
    std::thread::sleep(Duration::from_secs(2));

    let Some(linha) = codigo.lines().next() else {
        panic!("o código tem uma primeira linha");
    };
    let Some(byte) = linha.find("desconto") else {
        panic!("o membro está na primeira linha");
    };
    let coluna = linha[..byte].chars().count() as u32;
    assert!(
        runtime
            .block_on(ativo.change_document(DocumentChange {
                document_id: DocumentId(1),
                version: 2,
                range: Some(TextRange {
                    start: TextPosition { line: 0, column: coluna },
                    end: TextPosition { line: 0, column: coluna + 1 },
                }),
                text: "X".to_owned(),
            }))
            .is_ok()
    );
    std::thread::sleep(Duration::from_secs(1));

    let itens = runtime.block_on(ativo.completion(CompletionRequest {
        document_id: DocumentId(1),
        position: TextPosition { line: 2, column: 2 },
        prefix: String::new(),
    }));
    let itens = match itens {
        Ok(itens) => itens,
        Err(erro) => panic!("a completação precisa responder: {erro}"),
    };
    let nomes: Vec<_> = itens.iter().map(|item| item.label.as_str()).collect();
    assert!(
        nomes.contains(&"Xesconto"),
        "a troca precisa cair na primeira letra do membro; vieram: {nomes:?}"
    );
    assert!(
        !nomes.contains(&"desconto"),
        "o membro antigo não pode sobreviver — a troca caiu um caractere fora, e a         coluna do analisador não é a nossa: {nomes:?}"
    );

    assert!(runtime.block_on(ativo.shutdown()).is_ok());
    let _ = std::fs::remove_dir_all(&root);
}

/// SONDAGEM: a coluna que o analisador **devolve** também conta UTF-16.
///
/// É o caminho de volta do mesmo desencontro. Um tipo declarado depois de um
/// emoji na mesma linha tem coluna diferente nas duas contagens, e sem conversão
/// a IDE apontaria um caractere adiante — o realce de um diagnóstico caindo ao
/// lado do que ele acusa.
#[test]
#[ignore = "exige Node instalado e `npm install typescript` no projeto de teste"]
fn the_column_the_analyzer_returns_is_translated_back() {
    let root = temporary("emoji-volta");
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
    #[cfg(windows)]
    const NPM: &str = "npm.cmd";
    #[cfg(not(windows))]
    const NPM: &str = "npm";
    let instalado = std::process::Command::new(NPM)
        .args(["install", "typescript@5", "--no-audit", "--no-fund"])
        .current_dir(&root)
        .status();
    assert!(instalado.is_ok_and(|status| status.success()));

    let arquivo = fonte.join("pedido.ts");
    let codigo = "const e = \"\u{1F642}\"; export class PedidoDeCompra {}\n";
    assert!(std::fs::write(&arquivo, codigo).is_ok());

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
                text: codigo.to_owned(),
            }))
            .is_ok()
    );
    std::thread::sleep(Duration::from_secs(2));

    let achados = match runtime.block_on(ativo.workspace_types("PedidoDeCompra", 20)) {
        Ok(achados) => achados,
        Err(erro) => panic!("a busca precisa responder: {erro}"),
    };
    let Some(achado) = achados.iter().find(|s| s.name == "PedidoDeCompra") else {
        panic!("o tipo declarado precisa aparecer: {achados:?}");
    };

    // A coluna esperada, contada em caracteres como o resto da IDE conta.
    //
    // O intervalo que o `navto` devolve cobre a **declaração inteira**, e não o
    // nome: começa no `export`. Esperar o nome foi o primeiro palpite, e o
    // analisador desmentiu — o número errado era o do teste.
    let Some(linha) = codigo.lines().next() else {
        panic!("o código tem uma primeira linha");
    };
    let Some(byte) = linha.find("export") else {
        panic!("a declaração está na primeira linha");
    };
    // Quinze caracteres até ali, e dezesseis unidades UTF-16: o emoji conta duas
    // vezes. É essa diferença que o teste cobra.
    let esperada = linha[..byte].chars().count() as u32;
    assert_eq!(
        achado.location.range.start.column, esperada,
        "com um emoji na linha, a coluna devolvida precisa vir traduzida para caractere"
    );

    assert!(runtime.block_on(ativo.shutdown()).is_ok());
    let _ = std::fs::remove_dir_all(&root);
}
