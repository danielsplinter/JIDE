//! O índice contra um projeto de verdade — qualquer um.
//!
//! É o critério da fase 1 da `25`, e ele tem três partes que só um projeto real
//! exerce: **achar** o que existe, achar **rápido**, e não crescer a memória do
//! processo. Um projeto de teste criado na hora satisfaz as três por acidente.
//!
//! # Nada aqui é de um projeto em particular
//!
//! A consulta e o que se espera dela **saem do próprio projeto**: uma varredura
//! independente do índice acha nomes de tipo declarados, e o teste cobra que o
//! índice ache os mesmos. Um teste que dissesse um nome à mão só valeria para o
//! projeto de onde esse nome veio — e a solução tem de servir a qualquer um.
//!
//! Rodam com o caminho em `IDE_PROJETO_GRANDE`:
//!
//! ```text
//! set IDE_PROJETO_GRANDE=C:\caminho\de\um\projeto
//! cargo test --release -p ide-app --test typescript_index -- --ignored --nocapture
//! ```

use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use ide_language_api::{
    ActiveLanguage, LanguageActivationContext, LanguageCapabilities, LanguageProvider,
};
use language_typescript::TypeScriptLanguageProvider;

fn projeto() -> PathBuf {
    let Some(caminho) = std::env::var_os("IDE_PROJETO_GRANDE").map(PathBuf::from) else {
        panic!("aponte IDE_PROJETO_GRANDE para um projeto TypeScript");
    };
    assert!(caminho.is_dir(), "o projeto precisa existir: {caminho:?}");
    caminho
}

fn memoria_mb() -> u64 {
    ide_core::MemoryMeter::read(&[]).own_mb
}

/// Ativa o provider nativo e espera a varredura terminar.
///
/// Ativar **não** espera por ela — esse é o ponto —, então o teste espera.
fn ativado(root: &Path) -> (Box<dyn ActiveLanguage>, Duration, Duration) {
    let provider = TypeScriptLanguageProvider::new();
    let inicio = Instant::now();
    let ativo = match pollster::block_on(provider.activate(LanguageActivationContext {
        workspace_root: root.to_path_buf(),
        source_roots: Vec::new(),
        toolchains: Vec::new(),
    })) {
        Ok(ativo) => ativo,
        Err(erro) => panic!("o provider nativo precisa ativar: {erro}"),
    };
    let ativacao = inicio.elapsed();

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
    (ativo, ativacao, inicio.elapsed())
}

/// Nomes de tipo declarados no projeto, achados **sem** o índice.
///
/// É a fonte independente contra a qual o índice é conferido — o mesmo desenho
/// da ADR-027, em que a divergência entre a nossa leitura e a de fora é defeito
/// nosso.
///
/// # A primeira versão desta função estava errada, e o índice estava certo
///
/// Ela aceitava `export class X` em qualquer indentação, e num `.spec.ts` de
/// regra de ESLint achou `LoadProductsFail` **dentro de um literal de texto** —
/// código citado como string, não declarado. O índice não o indexou, e não
/// deveria: a gramática sabe distinguir declaração de texto, e o varredor
/// ingênuo não sabia.
///
/// Por isso só conta o que está na **coluna zero**. Uma declaração de módulo não
/// é indentada; o que aparece indentado está dentro de um `namespace`, de um
/// bloco ou de uma string. Deixar de fora uma declaração de verdade é inofensivo
/// aqui — o conjunto esperado só precisa ser **verdadeiro**, não completo.
fn tipos_declarados(root: &Path, quantos: usize) -> Vec<String> {
    let mut achados: Vec<String> = Vec::new();
    let mut pilha = vec![root.to_path_buf()];
    while let Some(pasta) = pilha.pop() {
        let Ok(entradas) = std::fs::read_dir(&pasta) else {
            continue;
        };
        for entrada in entradas.flatten() {
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
            if !nome.ends_with(".ts") || nome.ends_with(".d.ts") {
                continue;
            }
            let Ok(texto) = std::fs::read_to_string(&caminho) else {
                continue;
            };
            for linha in texto.lines() {
                let Some(resto) = linha.strip_prefix("export class ") else {
                    continue;
                };
                let tipo: String = resto
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_')
                    .collect();
                // Nome curto casa com tudo e não prova nada.
                if tipo.len() >= 6 && !achados.contains(&tipo) {
                    achados.push(tipo);
                }
                if achados.len() >= quantos {
                    return achados;
                }
            }
        }
    }
    achados
}

/// O índice acha o que o projeto declara, rápido, sem inchar o processo.
///
/// # As três partes do critério, e por que nenhuma sobra
///
/// **Achar**: os nomes procurados vêm de uma varredura independente do índice,
/// no projeto que estiver apontado. Não há nome escrito à mão aqui.
///
/// **Rápido**: esta mesma pergunta, pelo analisador externo, espera ele montar o
/// projeto — 30,4 s medidos num monorepo de 8 958 arquivos. O índice tem de
/// responder em outra ordem de grandeza, ou não vale o que custa.
///
/// **Sem inchar**: é o que separa "índice em disco" de "índice residente". Sem
/// esta medida, a fase pode ser dada por cumprida do jeito errado — respondendo
/// depressa **porque carregou tudo**, que é o que se está tentando evitar.
#[test]
#[ignore = "exige IDE_PROJETO_GRANDE apontando para um projeto TypeScript"]
fn the_index_finds_what_the_project_declares_fast_and_without_growing() {
    let root = projeto();
    let provider = TypeScriptLanguageProvider::new();
    assert!(
        provider
            .capabilities()
            .contains(LanguageCapabilities::WORKSPACE_SYMBOLS),
        "o provider nativo precisa declarar que responde busca por nome"
    );

    let esperados = tipos_declarados(&root, 20);
    assert!(
        !esperados.is_empty(),
        "o projeto apontado precisa declarar algum tipo: {root:?}"
    );

    let antes = memoria_mb();
    let (ativo, ativacao, varredura) = ativado(&root);
    println!("ativar: {ativacao:?}, varredura: {varredura:?}");
    assert!(
        ativacao < Duration::from_millis(100),
        "ativar não pode esperar a varredura: levou {ativacao:?}"
    );

    let mut pior = Duration::ZERO;
    for esperado in &esperados {
        let inicio = Instant::now();
        let achados = match pollster::block_on(ativo.workspace_types(esperado, 100)) {
            Ok(achados) => achados,
            Err(erro) => panic!("a busca por {esperado} precisa responder: {erro}"),
        };
        pior = pior.max(inicio.elapsed());
        assert!(
            achados.iter().any(|simbolo| &simbolo.name == esperado),
            "o índice precisa achar {esperado}, que o projeto declara; veio: {:?}",
            achados.iter().map(|s| &s.name).collect::<Vec<_>>()
        );
    }
    let depois = memoria_mb();
    println!(
        "{} nomes conferidos, pior consulta {pior:?}, memória {antes} MB -> {depois} MB",
        esperados.len()
    );

    assert!(
        pior < Duration::from_millis(50),
        "a busca precisa responder em outra ordem de grandeza que o analisador \
         externo, e a pior levou {pior:?}"
    );
    assert!(
        depois.saturating_sub(antes) < 10,
        "o índice mora no disco: o processo cresceu de {antes} MB para {depois} MB"
    );
}

/// A consulta escrita como nome de arquivo acha o tipo em `CamelCase`.
///
/// O nome vem do projeto apontado e é convertido para a forma com hífen — que é
/// como se escreve o arquivo, e como quem procura digita. Foi o defeito que o
/// analisador externo teve na `23`, e o índice não pode repeti-lo.
#[test]
#[ignore = "exige IDE_PROJETO_GRANDE apontando para um projeto TypeScript"]
fn a_hyphenated_query_finds_the_type_in_any_project() {
    let root = projeto();
    let Some(esperado) = tipos_declarados(&root, 1).into_iter().next() else {
        panic!("o projeto apontado precisa declarar algum tipo");
    };

    // `FederatedLoginContext` vira `federated-login-context`, sem que o teste
    // saiba de que projeto o nome veio.
    let mut com_hifen = String::new();
    for (posicao, caractere) in esperado.char_indices() {
        if caractere.is_uppercase() && posicao > 0 {
            com_hifen.push('-');
        }
        com_hifen.extend(caractere.to_lowercase());
    }
    println!("procurando {com_hifen:?} para achar {esperado:?}");

    let (ativo, _, _) = ativado(&root);
    let achados = match pollster::block_on(ativo.workspace_types(&com_hifen, 100)) {
        Ok(achados) => achados,
        Err(erro) => panic!("a busca precisa responder: {erro}"),
    };
    assert!(
        achados.iter().any(|simbolo| simbolo.name == esperado),
        "a consulta com hífen precisa achar {esperado}; veio: {:?}",
        achados.iter().map(|s| &s.name).collect::<Vec<_>>()
    );
}

/// O índice responde pelos tipos do projeto, e não pelos das dependências.
///
/// É a diferença que o torna útil **sem `node_modules`** — a primeira falha
/// encontrada ao abrir um projeto real.
#[test]
#[ignore = "exige IDE_PROJETO_GRANDE apontando para um projeto TypeScript"]
fn the_index_answers_for_the_project_and_not_for_its_dependencies() {
    let root = projeto();
    let Some(esperado) = tipos_declarados(&root, 1).into_iter().next() else {
        panic!("o projeto apontado precisa declarar algum tipo");
    };
    let (ativo, _, _) = ativado(&root);
    let achados = match pollster::block_on(ativo.workspace_types(&esperado, 100)) {
        Ok(achados) => achados,
        Err(erro) => panic!("a busca precisa responder: {erro}"),
    };
    assert!(!achados.is_empty());
    assert!(
        achados.iter().all(|simbolo| {
            !simbolo.location.path.to_string_lossy().contains("node_modules")
        }),
        "nenhum resultado pode vir de dependência instalada"
    );
}
