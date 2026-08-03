//! O índice contra um projeto grande de verdade.
//!
//! É o critério da fase 1 da `25`, e ele tem três partes que só um projeto real
//! exerce: **achar** o que existe, achar **rápido**, e não crescer a memória do
//! processo. Um projeto de teste criado na hora satisfaz as três por acidente.
//!
//! Rodam com o caminho em `IDE_PROJETO_GRANDE`:
//!
//! ```text
//! set IDE_PROJETO_GRANDE=C:\caminho\do\projeto
//! cargo test --release -p language-typescript --test index -- --ignored --nocapture
//! ```

use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

use ide_domain::LanguageId;
use ide_language_api::{LanguageActivationContext, LanguageCapabilities, LanguageProvider};
use language_typescript::TypeScriptLanguageProvider;

fn projeto() -> PathBuf {
    let Some(caminho) = std::env::var_os("IDE_PROJETO_GRANDE").map(PathBuf::from) else {
        panic!("aponte IDE_PROJETO_GRANDE para um projeto grande");
    };
    assert!(caminho.is_dir(), "o projeto precisa existir: {caminho:?}");
    caminho
}

fn memoria_mb() -> u64 {
    ide_core::MemoryMeter::read(&[]).own_mb
}

/// Espera a varredura terminar, e diz quanto ela levou.
///
/// Ativar não espera por ela — esse é o ponto —, então o teste espera.
fn esperar_indexacao(ativo: &dyn ide_language_api::ActiveLanguage) -> Duration {
    let Some(sinal) = ativo.readiness() else {
        panic!("o provider precisa dizer quando termina de preparar o projeto");
    };
    let inicio = Instant::now();
    while !sinal.is_ready() {
        std::thread::sleep(Duration::from_millis(50));
        assert!(
            inicio.elapsed() < Duration::from_secs(600),
            "a varredura precisa terminar"
        );
    }
    inicio.elapsed()
}

/// A busca acha o que existe, rápido, sem inchar o processo.
///
/// # As três partes do critério, e por que nenhuma sobra
///
/// **Achar**: um projeto de teste tem quatro tipos, e qualquer coisa os acha.
/// Aqui são milhares, e o nome procurado está no meio deles.
///
/// **Rápido**: hoje esta mesma pergunta espera o analisador externo montar o
/// projeto — 30,4 s medidos. O índice tem de responder em outra ordem de
/// grandeza, ou não vale o que custa.
///
/// **Sem inchar**: é o que separa "índice em disco" de "índice residente". Sem
/// esta medida, a fase pode ser dada por cumprida do jeito errado — respondendo
/// depressa porque carregou tudo, que é exatamente o que se está tentando
/// evitar. Ver "O índice mora no disco" na `25`.
#[test]
#[ignore = "exige IDE_PROJETO_GRANDE; a construção do índice leva alguns segundos"]
fn the_index_answers_the_real_project_fast_and_without_growing() {
    let root = projeto();
    let provider = TypeScriptLanguageProvider::new();
    assert!(
        provider
            .capabilities()
            .contains(LanguageCapabilities::WORKSPACE_SYMBOLS),
        "o provider nativo precisa declarar que responde busca por nome"
    );

    let antes = memoria_mb();
    let inicio = Instant::now();
    let ativo = match pollster::block_on(provider.activate(LanguageActivationContext {
        workspace_root: root.clone(),
        source_roots: Vec::new(),
        toolchains: Vec::new(),
    })) {
        Ok(ativo) => ativo,
        Err(erro) => panic!("o provider nativo precisa ativar: {erro}"),
    };
    let ativacao = inicio.elapsed();
    println!("ativar: {ativacao:?}");
    assert!(
        ativacao < Duration::from_millis(100),
        "ativar não pode esperar a varredura: levou {ativacao:?}"
    );

    // Enquanto indexa, a resposta é "ainda não" — e não uma lista vazia.
    if let Err(erro) = pollster::block_on(ativo.workspace_types("qualquer", 10)) {
        println!("durante a varredura: {erro}");
    }

    println!("varredura: {:?}", esperar_indexacao(ativo.as_ref()));

    // A consulta que o projeto real motivou.
    let inicio = Instant::now();
    let achados = match pollster::block_on(ativo.workspace_types("federated-login-context", 100)) {
        Ok(achados) => achados,
        Err(erro) => panic!("a busca precisa responder: {erro}"),
    };
    let consulta = inicio.elapsed();
    let depois = memoria_mb();
    println!(
        "consulta: {consulta:?}, {} resultados, memória {antes} MB -> {depois} MB",
        achados.len()
    );
    for achado in achados.iter().take(5) {
        println!("   {} em {:?}", achado.name, achado.location.path);
    }

    assert!(
        achados.iter().any(|s| s.name == "FederatedLoginContext"),
        "o tipo procurado precisa aparecer: {:?}",
        achados.iter().map(|s| &s.name).collect::<Vec<_>>()
    );
    assert!(
        consulta < Duration::from_millis(50),
        "a busca precisa responder em outra ordem de grandeza que os 30 s do \
         analisador externo, e levou {consulta:?}"
    );
    assert!(
        depois.saturating_sub(antes) < 10,
        "o índice mora no disco: o processo cresceu de {antes} MB para {depois} MB"
    );
}

/// A busca funciona **sem `node_modules`**, que é o que o analisador não faz.
///
/// Foi a primeira falha encontrada ao abrir um projeto real: um monorepo Angular
/// sem dependências instaladas, e a busca por tipo não achava nada num projeto
/// cheio de tipos. O índice lê o código, e o código está lá.
#[test]
#[ignore = "exige IDE_PROJETO_GRANDE"]
fn the_search_works_without_installed_dependencies() {
    let root = projeto();
    // O índice não desce em `node_modules` por construção, então esta busca
    // responde igual com ou sem dependências instaladas — é o que se cobra.
    let provider = TypeScriptLanguageProvider::new();
    let ativo = match pollster::block_on(provider.activate(LanguageActivationContext {
        workspace_root: root,
        source_roots: Vec::new(),
        toolchains: Vec::new(),
    })) {
        Ok(ativo) => ativo,
        Err(erro) => panic!("o provider nativo precisa ativar: {erro}"),
    };
    assert_eq!(ativo.language_id(), &LanguageId("typescript".to_owned()));
    esperar_indexacao(ativo.as_ref());

    let achados = match pollster::block_on(ativo.workspace_types("component", 100)) {
        Ok(achados) => achados,
        Err(erro) => panic!("a busca precisa responder: {erro}"),
    };
    assert!(
        !achados.is_empty(),
        "um projeto Angular tem tipos com `component` no nome"
    );
    assert!(
        achados
            .iter()
            .all(|s| !s.location.path.to_string_lossy().contains("node_modules")),
        "o índice responde pelos tipos do projeto, e não pelos das dependências"
    );
}
