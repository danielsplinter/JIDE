//! Quanto do ponto o índice alcança, medido contra projetos de verdade.
//!
//! É a medição que a fase 4 da `25` fez à mão e que a fase 7 precisa refazer.
//! **Desta vez o instrumento fica no repositório**: o número antigo — 14% a 17%
//! — veio de um arranjo que não foi guardado, e por isso a comparação de então
//! dependia de eu lembrar como tinha contado.
//!
//! # Como se conta
//!
//! Uma amostra **espalhada** dos `.ts` do projeto: os arquivos são ordenados e
//! se pega um a cada `N`, de modo a somar 400. Espalhar não é capricho — a
//! primeira versão desta medição pegou os 400 primeiros de uma varredura em
//! profundidade, caiu inteira em testes de Cypress, onde não há classe nenhuma,
//! e mediu **1%**: um número que falava da ordem das pastas.
//!
//! Em cada arquivo, cada `.` que segue um identificador é uma pergunta ao
//! provider nativo. A resposta é uma de três:
//!
//! - **os membros** — o índice alcançou;
//! - **lista vazia** — alcançou, e o tipo não tem membros. É afirmação, e é
//!   certa;
//! - **`Unresolved`** — não soube, e diz que não soube. Desce ao analisador.
//!
//! ```text
//! ER_IDE_PROJETO_TS=C:/caminho/do/projeto cargo test -p language-typescript --test cobertura -- --ignored --nocapture
//! ```

use std::path::{Path, PathBuf};

use ide_domain::{CompletionRequest, DocumentId, DocumentSnapshot, TextPosition};
use ide_language_api::{LanguageActivationContext, LanguageError, LanguageProvider};
use language_typescript::{TypeScriptLanguageProvider, tsconfig};

/// Quantos arquivos entram na amostra.
///
/// O mesmo número da medição da fase 4, para os totais serem comparáveis: se
/// esta contagem batesse com 7 828 pontos no monorepo, é porque o instrumento
/// conta o que o de então contava.
const ARQUIVOS_NA_AMOSTRA: usize = 400;

fn projeto() -> Option<PathBuf> {
    std::env::var("ER_IDE_PROJETO_TS").ok().map(PathBuf::from)
}

/// Os `.ts` do projeto, ordenados. `.d.ts` fica de fora: ele é declaração, e
/// ninguém digita um ponto dentro dele.
fn fontes(raizes: &[PathBuf]) -> Vec<PathBuf> {
    let mut pilha = raizes.to_vec();
    let mut achados = Vec::new();
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
            } else if nome.ends_with(".ts") && !nome.ends_with(".d.ts") {
                achados.push(caminho);
            }
        }
    }
    achados.sort();
    achados
}

/// Um de cada `N`, para a amostra cobrir o projeto inteiro.
fn espalhada(todos: &[PathBuf]) -> Vec<PathBuf> {
    if todos.len() <= ARQUIVOS_NA_AMOSTRA {
        return todos.to_vec();
    }
    let passo = todos.len() / ARQUIVOS_NA_AMOSTRA;
    todos.iter().step_by(passo.max(1)).cloned().collect()
}

/// As posições logo depois de cada `.` que segue um identificador.
///
/// **Só depois de um nome.** `1.5` é número, `./caminho` é texto, e `...args` é
/// espalhamento — nenhum dos três é uma pergunta que alguém faria.
fn pontos_de(texto: &str) -> Vec<(TextPosition, bool)> {
    let mut achados = Vec::new();
    for (numero, linha) in texto.lines().enumerate() {
        let caracteres: Vec<char> = linha.chars().collect();
        for (indice, caractere) in caracteres.iter().enumerate() {
            if *caractere != '.' || indice == 0 {
                continue;
            }
            let anterior = caracteres[indice - 1];
            if !anterior.is_alphanumeric() && anterior != '_' && anterior != '$' {
                continue;
            }
            if anterior.is_numeric() {
                continue;
            }
            // Andar para trás pelo nome: se antes dele houver outro ponto, este
            // é o segundo elo de uma cadeia.
            let mut inicio = indice - 1;
            while inicio > 0 {
                let anterior = caracteres[inicio - 1];
                if anterior.is_alphanumeric() || anterior == '_' || anterior == '$' {
                    inicio -= 1;
                } else {
                    break;
                }
            }
            let em_cadeia = inicio > 0 && caracteres[inicio - 1] == '.';
            achados.push((
                TextPosition {
                    line: numero as u32,
                    column: (indice + 1) as u32,
                },
                em_cadeia,
            ));
        }
    }
    achados
}

struct Contagem {
    pontos: usize,
    membros: usize,
    vazias: usize,
    nao_sei: usize,
    falhas: usize,
    /// Dos que não souberam, quantos eram o segundo ponto de uma cadeia.
    ///
    /// **É a pergunta que a medição precisa responder**, e não só "quanto se
    /// alcança": saber *onde* os pontos morrem decide o que fazer depois. Em
    /// `this.svc.buscar`, o `this.svc` é alcançável e o resto não, porque o tipo
    /// de um membro exige resolver o membro — que é um passo além do que a fase
    /// 4 entregou.
    cadeias_sem_resposta: usize,
}

impl Contagem {
    /// A cobertura: o que o índice alcançou, sobre tudo o que foi perguntado.
    ///
    /// A lista vazia **conta como alcançada**: ela é uma afirmação sobre um tipo
    /// conhecido, e não uma desistência.
    fn cobertura(&self) -> f64 {
        if self.pontos == 0 {
            return 0.0;
        }
        ((self.membros + self.vazias) as f64) * 100.0 / (self.pontos as f64)
    }
}

fn medir(raiz: &Path) -> Contagem {
    let Ok(config) = tsconfig::load(&raiz.join("tsconfig.json")) else {
        panic!("o tsconfig do projeto precisa ser lido");
    };
    let raizes = config.source_roots();
    let todos = fontes(&raizes);
    let amostra = espalhada(&todos);
    println!(
        "  {} arquivos no projeto, {} na amostra",
        todos.len(),
        amostra.len()
    );

    let ativo = match pollster::block_on(TypeScriptLanguageProvider::new().activate(
        LanguageActivationContext {
            workspace_root: raiz.to_path_buf(),
            source_roots: raizes.clone(),
            toolchains: Vec::new(),
        },
    )) {
        Ok(ativo) => ativo,
        Err(erro) => panic!("o provider nativo precisa ativar: {erro}"),
    };
    // Sem esperar, a amostra correria contra a construção do índice e da
    // biblioteca — e mediria a máquina, não o código.
    assert!(
        pollster::block_on(ativo.wait_until_indexed(std::time::Duration::from_secs(600))),
        "a preparação do projeto precisa terminar"
    );

    let mut contagem = Contagem {
        pontos: 0,
        membros: 0,
        vazias: 0,
        nao_sei: 0,
        falhas: 0,
        cadeias_sem_resposta: 0,
    };
    for (numero, arquivo) in amostra.iter().enumerate() {
        let Ok(texto) = std::fs::read_to_string(arquivo) else {
            continue;
        };
        let document_id = DocumentId(numero as u64 + 1);
        if pollster::block_on(ativo.open_document(DocumentSnapshot {
            id: document_id,
            path: arquivo.clone(),
            version: 1,
            text: texto.clone(),
        }))
        .is_err()
        {
            continue;
        }
        for (position, em_cadeia) in pontos_de(&texto) {
            contagem.pontos += 1;
            match pollster::block_on(ativo.completion(CompletionRequest {
                document_id,
                position,
                prefix: String::new(),
            })) {
                Ok(itens) if itens.is_empty() => contagem.vazias += 1,
                Ok(_) => contagem.membros += 1,
                Err(LanguageError::Unresolved(_)) => {
                    contagem.nao_sei += 1;
                    if em_cadeia {
                        contagem.cadeias_sem_resposta += 1;
                    }
                }
                // Qualquer outro erro é defeito, e não "não sei": ele precisa
                // aparecer separado, senão some dentro da estatística.
                Err(_) => contagem.falhas += 1,
            }
        }
        let _ = pollster::block_on(ativo.close_document(document_id));
    }
    contagem
}

/// **Quanto do ponto o índice alcança**, com os tipos da linguagem dentro dele.
///
/// A fase 4 mediu 17% no monorepo de biblioteca e 14% na aplicação, **sem** os
/// tipos do TypeScript. A fase 7 os trouxe, e o que este teste responde é de
/// quanto foi o ganho.
#[test]
#[ignore = "exige ER_IDE_PROJETO_TS; percorre milhares de pontos e leva minutos"]
fn how_much_of_the_dot_the_index_reaches() {
    let Some(raiz) = projeto() else {
        panic!("defina ER_IDE_PROJETO_TS");
    };
    println!("projeto: {}", raiz.display());
    let inicio = std::time::Instant::now();
    let contagem = medir(&raiz);
    println!(
        "  {} pontos | {} com membros | {} vazias | {} \"não sei\" | {} falhas",
        contagem.pontos, contagem.membros, contagem.vazias, contagem.nao_sei, contagem.falhas
    );
    println!(
        "  cobertura: {:.1}% (em {:?})",
        contagem.cobertura(),
        inicio.elapsed()
    );
    println!(
        "  dos {} \"não sei\", {} ({:.0}%) são o segundo elo de uma cadeia",
        contagem.nao_sei,
        contagem.cadeias_sem_resposta,
        (contagem.cadeias_sem_resposta as f64) * 100.0 / (contagem.nao_sei.max(1) as f64)
    );

    assert!(contagem.pontos > 0, "a amostra precisa ter pontos");
    // **Zero falhas.** Um erro que não seja `Unresolved` quer dizer que o
    // provider quebrou, e um provider que quebra some dentro de uma estatística
    // de cobertura como se fosse "não sei".
    assert_eq!(
        contagem.falhas, 0,
        "nenhum ponto pode falhar; só responder ou dizer que não sabe"
    );
}
