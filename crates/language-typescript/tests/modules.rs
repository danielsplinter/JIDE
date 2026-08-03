//! O resolvedor de módulos contra o analisador de verdade.
//!
//! É o critério da fase 2 da `25`, e ele é uma comparação: para uma amostra de
//! `import` do projeto apontado, **o arquivo que nós resolvemos tem de ser o
//! mesmo que o analisador resolve**. Divergência é defeito nosso, pelo mesmo
//! desenho da ADR-027 — a origem é o `tsconfig.json`, e nós dois o lemos.
//!
//! A amostra sai do projeto, e não desta lista: um teste que dissesse um
//! `import` à mão só valeria para o projeto de onde ele veio.
//!
//! ```text
//! set IDE_PROJETO_GRANDE=C:\caminho\de\um\projeto
//! cargo test --release -p language-typescript --test modules -- --ignored --nocapture
//! ```

use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use ide_domain::{DefinitionRequest, DocumentId, DocumentSnapshot, LanguageId, TextPosition};
use ide_language_api::{LanguageActivationContext, LanguageProvider};
use ide_process::NativeProcessSupervisor;
use language_typescript::{ModuleResolver, TypeScriptServiceProvider, tsconfig, tsserver_in};

fn projeto() -> PathBuf {
    let Some(caminho) = std::env::var_os("IDE_PROJETO_GRANDE").map(PathBuf::from) else {
        panic!("aponte IDE_PROJETO_GRANDE para um projeto TypeScript");
    };
    assert!(caminho.is_dir(), "o projeto precisa existir: {caminho:?}");
    caminho
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

/// Um `import` achado no projeto: arquivo, especificador e onde ele está.
struct Importacao {
    arquivo: PathBuf,
    especificador: String,
    /// Linha e coluna **dentro** das aspas, em caracteres.
    posicao: TextPosition,
}

/// Colhe `import ... from '...'` pelo projeto, sem usar o resolvedor.
///
/// # A amostra é enviesada de propósito, e o viés não é o nosso
///
/// Só entram importações que o **projeto** declara como internas: as relativas,
/// que começam com ponto, e as que casam um apelido do `paths`. Colher as
/// primeiras sessenta de qualquer tipo enche a amostra de `@angular/core` e
/// deixa catorze para conferir — e o resolvedor não alcança dependência
/// instalada de propósito, então elas não dizem nada sobre ele.
///
/// O critério do que é interno vem do `tsconfig.json`, e não de nós resolvermos:
/// filtrar pelo que já sabemos resolver provaria só que sabemos o que sabemos.
fn importacoes(root: &Path, apelidos: &[String], quantas: usize) -> Vec<Importacao> {
    let mut achadas = Vec::new();
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
            for (numero, linha) in texto.lines().enumerate() {
                if !linha.starts_with("import ") {
                    continue;
                }
                let Some((antes, resto)) = linha.split_once("from '") else {
                    continue;
                };
                let Some((especificador, _)) = resto.split_once('\'') else {
                    continue;
                };
                let interna = especificador.starts_with('.')
                    || apelidos.iter().any(|apelido| {
                        let raiz = apelido.trim_end_matches('*');
                        !raiz.is_empty() && especificador.starts_with(raiz)
                    });
                if !interna {
                    continue;
                }
                achadas.push(Importacao {
                    arquivo: caminho.clone(),
                    especificador: especificador.to_owned(),
                    posicao: TextPosition {
                        line: numero as u32,
                        // Uma coluna adentro das aspas, que é onde o analisador
                        // reconhece o especificador.
                        column: (antes.chars().count() + "from '".len() + 1) as u32,
                    },
                });
                if achadas.len() >= quantas {
                    return achadas;
                }
            }
        }
    }
    achadas
}

/// O `tsconfig.json` que manda no projeto.
fn configuracao(root: &Path) -> language_typescript::TsConfig {
    let caminho = root.join("tsconfig.json");
    assert!(
        caminho.is_file(),
        "o projeto precisa de um tsconfig.json na raiz: {caminho:?}"
    );
    match tsconfig::load(&caminho) {
        Ok(config) => config,
        Err(erro) => panic!("o tsconfig.json precisa carregar: {erro:?}"),
    }
}

/// Para cada `import` da amostra, resolvemos o mesmo arquivo que o analisador.
///
/// # Por que a comparação é com o analisador, e não com uma lista
///
/// A regra de resolução é do TypeScript, e o `tsconfig.json` é a origem — a
/// ADR-027 diz que quem decide é o arquivo, não a convenção. Uma lista escrita à
/// mão só provaria que o teste concorda com quem o escreveu; o analisador
/// discorda quando estamos errados.
///
/// # O que não se cobra aqui
///
/// Especificador que o analisador aponta para dentro de `node_modules` é
/// **ignorado**: o resolvedor não alcança dependência instalada de propósito, e
/// cobrar isso seria cobrar o que a `25` decidiu não fazer.
#[test]
#[ignore = "exige IDE_PROJETO_GRANDE com node_modules instalado; leva ~1 min"]
fn we_resolve_the_same_file_the_analyzer_resolves() {
    let root = projeto();
    assert!(
        tsserver_in(&root).is_some(),
        "o projeto precisa ter `node_modules` para o analisador servir de referência"
    );
    let config = configuracao(&root);
    let apelidos: Vec<String> = config.paths.iter().map(|(padrao, _)| padrao.clone()).collect();
    let resolver = ModuleResolver::new(&config);
    let amostra = importacoes(&root, &apelidos, 200);
    assert!(
        !amostra.is_empty(),
        "o projeto apontado precisa ter algum `import`"
    );

    let runtime = runtime();
    let provider = TypeScriptServiceProvider::new(Arc::new(NativeProcessSupervisor::default()));
    let ativo = match runtime.block_on(provider.activate(LanguageActivationContext {
        workspace_root: root.clone(),
        source_roots: Vec::new(),
        toolchains: Vec::new(),
    })) {
        Ok(ativo) => ativo,
        Err(erro) => panic!("o analisador precisa subir: {erro}"),
    };
    assert_eq!(ativo.language_id(), &LanguageId("typescript".to_owned()));

    let mut conferidos = 0usize;
    let mut fora_do_projeto = 0usize;
    let mut sem_resposta = 0usize;
    let mut divergentes: Vec<String> = Vec::new();

    for (numero, importacao) in amostra.iter().enumerate() {
        let Ok(texto) = std::fs::read_to_string(&importacao.arquivo) else {
            continue;
        };
        let documento = DocumentId(numero as u64 + 1);
        if runtime
            .block_on(ativo.open_document(DocumentSnapshot {
                id: documento,
                path: importacao.arquivo.clone(),
                version: 1,
                text: texto,
            }))
            .is_err()
        {
            continue;
        }
        // O analisador só responde depois de montar o projeto; a primeira
        // pergunta espera, e as outras vão junto.
        std::thread::sleep(Duration::from_millis(20));

        let deles = runtime.block_on(ativo.definition(DefinitionRequest {
            document_id: documento,
            position: importacao.posicao,
        }));
        let _ = runtime.block_on(ativo.close_document(documento));

        let Ok(deles) = deles else {
            sem_resposta += 1;
            continue;
        };
        let Some(esperado) = deles.first().map(|local| local.path.clone()) else {
            sem_resposta += 1;
            continue;
        };
        if esperado.to_string_lossy().contains("node_modules") {
            fora_do_projeto += 1;
            continue;
        }

        let nosso = resolver.resolve(&importacao.arquivo, &importacao.especificador);
        conferidos += 1;
        let iguais = nosso
            .as_ref()
            .is_some_and(|caminho| mesmo_arquivo(caminho, &esperado));
        if !iguais {
            divergentes.push(format!(
                "{} importando {:?}: nós {:?}, o analisador {:?}",
                importacao.arquivo.display(),
                importacao.especificador,
                nosso,
                esperado
            ));
        }
    }

    println!(
        "{conferidos} conferidos, {fora_do_projeto} em dependência (ignorados), \
         {sem_resposta} sem resposta do analisador, {} divergentes",
        divergentes.len()
    );
    for divergencia in divergentes.iter().take(10) {
        println!("   {divergencia}");
    }

    // O piso é baixo de propósito: um projeto de oitenta arquivos não tem
    // duzentas importações internas, e exigir um número grande faria este teste
    // valer só para monorepo. A força da conferência acompanha o tamanho do
    // projeto, e o número conferido é impresso para que ela seja visível.
    assert!(
        conferidos >= 10,
        "a amostra precisa ter o que conferir: só {conferidos} chegaram à comparação"
    );
    assert!(
        divergentes.is_empty(),
        "{} de {conferidos} importações resolvem para outro arquivo",
        divergentes.len()
    );
    assert!(runtime.block_on(ativo.shutdown()).is_ok());
}

/// Dois caminhos que apontam para o mesmo arquivo, apesar da forma.
///
/// O analisador devolve barra normal mesmo no Windows, e nós devolvemos o que o
/// sistema usa. Comparar cadeia crua acusaria divergência onde não há.
fn mesmo_arquivo(esquerda: &Path, direita: &Path) -> bool {
    let normal = |caminho: &Path| {
        caminho
            .to_string_lossy()
            .replace('\\', "/")
            .to_lowercase()
    };
    normal(esquerda) == normal(direita)
}
