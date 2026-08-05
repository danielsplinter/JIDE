//! **Onde o índice responde, ele responde o mesmo que o analisador.**
//!
//! # Por que este teste existe
//!
//! O índice está na frente do analisador para a completação: ele responde em
//! milissegundos, sem processo nenhum, e o que não alcança ele diz que não
//! alcança. Essa ordem só se sustenta sobre uma afirmação: **quando ele
//! responde, ele não mente.**
//!
//! Nada garantia isso. O índice é uma aproximação — lê texto e não instancia
//! genérico —, e uma aproximação que ofereça de menos é uma lista curta, que
//! ninguém nota; uma que ofereça de mais é **código que não compila sugerido
//! como se compilasse**, e é a família de defeito que esta especificação mais
//! persegue, porque a lista errada tem a mesma cara da certa.
//!
//! Este teste pergunta a **mesma coisa** aos dois, ponto a ponto, num projeto de
//! verdade, e compara conjunto com conjunto.
//!
//! # O que ele já encontrou
//!
//! Na primeira execução: 96 pontos, **51 iguais e 45 com item a mais** — sempre
//! `ɵfac` e `ɵprov`, que o Angular declara `static` em toda classe gerada, e
//! `useNonNullable`, que é `private` numa classe alheia. Duas regras da
//! linguagem que faltavam.
//!
//! Depois da correção: **96 de 96 iguais.**
//!
//! # Como rodar
//!
//! ```text
//! ER_IDE_PROJETO_TS=C:/caminho/do/projeto cargo test --release -p language-typescript --test concorda -- --ignored --nocapture
//! ```
//!
//! É `#[ignore]` porque exige um projeto com `node_modules` instalado e sobe o
//! `tsserver` — e `--release` porque em compilação de depuração ele leva
//! minutos sem dizer nada de diferente.

use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Duration;

use ide_domain::{CompletionRequest, DocumentId, DocumentSnapshot, TextPosition};
use ide_language_api::{LanguageActivationContext, LanguageProvider};
use language_typescript::{TypeScriptLanguageProvider, TypeScriptServiceProvider, tsconfig};

const ARQUIVOS: usize = 60;

fn fontes(raizes: &[PathBuf]) -> Vec<PathBuf> {
    let mut pilha = raizes.to_vec();
    let mut achados = Vec::new();
    while let Some(pasta) = pilha.pop() {
        let Ok(entradas) = std::fs::read_dir(&pasta) else {
            continue;
        };
        for entrada in entradas.flatten() {
            let caminho = entrada.path();
            let Some(nome) = caminho.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            if caminho.is_dir() {
                if nome != "node_modules" && nome != "dist" && !nome.starts_with('.') {
                    pilha.push(caminho);
                }
            } else if nome.ends_with(".ts") && !nome.ends_with(".d.ts") {
                achados.push(caminho);
            }
        }
    }
    achados.sort();
    achados
}

fn pontos_de(texto: &str) -> Vec<TextPosition> {
    let mut achados = Vec::new();
    for (numero, linha) in texto.lines().enumerate() {
        let caracteres: Vec<char> = linha.chars().collect();
        let mut aspas = false;
        for (indice, caractere) in caracteres.iter().enumerate() {
            if matches!(caractere, '\'' | '"' | '`') {
                aspas = !aspas;
            }
            if *caractere != '.' || indice == 0 || aspas {
                continue;
            }
            let anterior = caracteres[indice - 1];
            if (!anterior.is_alphanumeric() && anterior != '_' && anterior != '$')
                || anterior.is_numeric()
            {
                continue;
            }
            achados.push(TextPosition {
                line: numero as u32,
                column: (indice + 1) as u32,
            });
        }
    }
    achados
}

/// **O índice nunca oferece o que o analisador não ofereceria.**
///
/// # Por que a afirmação não é "tudo igual"
///
/// Oferecer **de menos** é aproximação, e o índice é uma por construção: ele lê
/// texto e não instancia genérico. Uma lista curta é uma lista curta.
///
/// Oferecer **de mais** é outra coisa: é dizer que existe o que não existe, e
/// quem aceitar a sugestão escreve código que não compila. É isso, e só isso,
/// que este teste proíbe.
#[test]
#[ignore = "exige ER_IDE_PROJETO_TS com node_modules instalado; leva minutos"]
fn the_index_never_offers_what_the_analyzer_would_not() {
    let Ok(raiz) = std::env::var("ER_IDE_PROJETO_TS") else {
        panic!("aponte ER_IDE_PROJETO_TS");
    };
    let raiz = PathBuf::from(raiz);
    let Ok(config) = tsconfig::load(&raiz.join("tsconfig.json")) else {
        panic!("o tsconfig precisa abrir");
    };
    let raizes = config.source_roots();
    let todos = fontes(&raizes);
    let passo = (todos.len() / ARQUIVOS).max(1);
    let amostra: Vec<PathBuf> = todos.iter().step_by(passo).cloned().collect();
    println!("[concorda] {} arquivos na amostra", amostra.len());

    let contexto = || LanguageActivationContext {
        workspace_root: raiz.clone(),
        source_roots: raizes.clone(),
        toolchains: Vec::new(),
    };
    let indice = match pollster::block_on(TypeScriptLanguageProvider::new().activate(contexto())) {
        Ok(ativo) => ativo,
        Err(erro) => panic!("o nativo precisa ativar: {erro}"),
    };
    assert!(pollster::block_on(
        indice.wait_until_indexed(Duration::from_secs(600))
    ));

    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(erro) => panic!("runtime: {erro}"),
    };
    let processos = std::sync::Arc::new(ide_process::NativeProcessSupervisor::default());
    let servico =
        match runtime.block_on(TypeScriptServiceProvider::new(processos).activate(contexto())) {
            Ok(ativo) => ativo,
            Err(erro) => panic!("o analisador precisa subir: {erro}"),
        };

    let (mut iguais, mut a_menos, mut a_mais, mut discordam, mut so_nosso) = (0, 0, 0, 0, 0);

    for (numero, arquivo) in amostra.iter().enumerate() {
        let Ok(texto) = std::fs::read_to_string(arquivo) else {
            continue;
        };
        let id = DocumentId(numero as u64 + 1);
        let instantaneo = DocumentSnapshot {
            id,
            path: arquivo.clone(),
            version: 1,
            text: texto.clone(),
        };
        if pollster::block_on(indice.open_document(instantaneo.clone())).is_err() {
            continue;
        }
        if runtime.block_on(servico.open_document(instantaneo)).is_err() {
            continue;
        }
        for position in pontos_de(&texto) {
            let pedido = CompletionRequest {
                document_id: id,
                position,
                prefix: String::new(),
            };
            let Ok(nosso) = pollster::block_on(indice.completion(pedido.clone())) else {
                continue;
            };
            if nosso.is_empty() {
                continue;
            }
            let Ok(deles) = runtime.block_on(servico.completion(pedido)) else {
                so_nosso += 1;
                continue;
            };
            let nossos: HashSet<&str> = nosso.iter().map(|i| i.label.as_str()).collect();
            let deles: HashSet<&str> = deles.iter().map(|i| i.label.as_str()).collect();
            let linha = texto.lines().nth(position.line as usize).unwrap_or("");
            if deles.is_empty() {
                so_nosso += 1;
            } else if nossos == deles {
                iguais += 1;
            } else if nossos.is_subset(&deles) {
                a_menos += 1;
                if a_menos <= 6 {
                    let faltando: Vec<&&str> = deles.difference(&nossos).take(6).collect();
                    println!(
                        "[concorda] faltou em {:?}: {faltando:?}",
                        linha.trim().chars().take(48).collect::<String>()
                    );
                }
            } else if deles.is_subset(&nossos) {
                a_mais += 1;
                if a_mais <= 6 {
                    let sobrando: Vec<&&str> = nossos.difference(&deles).take(6).collect();
                    println!(
                        "[concorda] sobrou em {:?}: {sobrando:?}",
                        linha.trim().chars().take(48).collect::<String>()
                    );
                }
            } else {
                discordam += 1;
                if discordam <= 6 {
                    let sobrando: Vec<&&str> = nossos.difference(&deles).take(4).collect();
                    let faltando: Vec<&&str> = deles.difference(&nossos).take(4).collect();
                    println!(
                        "[concorda] discorda em {:?}: sobrou {sobrando:?}, faltou {faltando:?}",
                        linha.trim().chars().take(40).collect::<String>()
                    );
                }
            }
        }
        let _ = pollster::block_on(indice.close_document(id));
        let _ = runtime.block_on(servico.close_document(id));
    }

    let total = iguais + a_menos + a_mais + discordam + so_nosso;
    println!(
        "[concorda] {total} pontos que o índice respondeu: {iguais} iguais | \
         {a_menos} ofereceu menos | {a_mais} ofereceu mais | {discordam} discordam | \
         {so_nosso} só o nosso respondeu"
    );
    let _ = runtime.block_on(servico.shutdown());

    assert!(
        total > 0,
        "a amostra precisa ter pontos que o índice responda; \
         zero quer dizer que o projeto apontado não exercita nada"
    );
    assert_eq!(
        (a_mais, discordam),
        (0, 0),
        "o índice ofereceu o que o analisador não ofereceria — é sugestão de \
         código que não compila. As linhas com `sobrou` acima dizem o quê"
    );
}
