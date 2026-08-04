//! Onde os tipos do TypeScript moram, e quais deles este projeto alcança.
//!
//! É a metade de projeto da fase 7 da `25`. A outra metade — o que cada arquivo
//! declara — é texto, e está em `analyzer::stdlib`.
//!
//! # Por que vêm do projeto, e não de dentro do executável
//!
//! É a **ADR-028**: a versão dos tipos é a que o projeto instalou. Embarcar uma
//! cópia prometeria completação do TypeScript 5 a um projeto preso ao 4 — e a
//! promessa só quebraria na compilação, longe de onde a sugestão apareceu.
//!
//! # Lê-se tudo
//!
//! Os 100 arquivos entram, 3,3 MB. Ler por demanda economizaria megabytes que
//! não faltam a ninguém e cobraria disco no meio de uma digitação.

use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

use crate::analyzer::stdlib::{Alcance, Biblioteca, referencias_de};

use super::tsconfig::TsConfig;

/// A biblioteca do TypeScript instalado neste projeto, e de que versão ela é.
pub(crate) struct BibliotecaDoProjeto {
    pub(crate) tipos: Biblioteca,
    /// A versão instalada, que é a chave do cache em disco.
    pub(crate) versao: String,
    /// O que este projeto alcança, do que foi lido.
    pub(crate) alcance: Alcance,
}

/// Lê a biblioteca e já resolve o que este projeto alcança dela.
///
/// As duas coisas juntas porque quem usa precisa das duas, e separá-las
/// convidaria a responder com o alcance de um projeto sobre a biblioteca de
/// outro — que é o defeito silencioso desta fase.
pub(crate) fn preparar(raiz: &Path, config: &TsConfig) -> Option<BibliotecaDoProjeto> {
    let mut lida = do_cache_ou_do_disco(raiz)?;
    lida.alcance = alcance_de(raiz, config);
    Some(lida)
}

/// A biblioteca do cache, ou lida dos `.d.ts` e gravada para a próxima vez.
///
/// # A chave é a versão do TypeScript, e não o projeto
///
/// Dois projetos com o mesmo TypeScript instalado têm a **mesma** biblioteca —
/// é a mesma pasta, os mesmos arquivos. Gravar por projeto guardaria a mesma
/// coisa várias vezes e reanalisaria a cada projeto novo.
///
/// E é por isso que o filtro do `target` fica de fora do cache: ele é do
/// projeto, e entra na resposta. Ver a fase 7 da `25`.
fn do_cache_ou_do_disco(raiz: &Path) -> Option<BibliotecaDoProjeto> {
    let pasta = pasta_da_biblioteca(raiz)?;
    let versao = versao_instalada(&pasta).unwrap_or_default();
    if let Some(caminho) = caminho_do_cache(&versao)
        && let Ok(texto) = std::fs::read_to_string(&caminho)
    {
        let tipos = Biblioteca::reler(&texto);
        if tipos.nomes() > 0 {
            return Some(BibliotecaDoProjeto {
                tipos,
                versao,
                alcance: Alcance::default(),
            });
        }
    }
    let lida = ler(raiz)?;
    // Gravar é **melhor esforço**: um cache que não se consegue escrever custa a
    // releitura da próxima vez, e nada mais. Falhar a abertura do projeto por
    // causa dele seria trocar um custo por um impedimento.
    if let Some(caminho) = caminho_do_cache(&lida.versao) {
        if let Some(pasta) = caminho.parent() {
            let _ = std::fs::create_dir_all(pasta);
        }
        let _ = std::fs::write(&caminho, lida.tipos.escrever());
    }
    Some(lida)
}

/// Onde a máquina guarda o que pode ser reconstruído.
///
/// O mesmo lugar do índice de TypeScript, e pelo mesmo motivo: não é dado de
/// quem usa, é cópia do que já está no disco.
///
/// Sem versão não há cache: gravar sob um nome vazio faria dois TypeScript
/// diferentes compartilharem o mesmo arquivo, e o segundo projeto receberia os
/// tipos do primeiro — calado, que é a pior forma.
fn caminho_do_cache(versao: &str) -> Option<PathBuf> {
    // A versão vem de um `package.json`, que é arquivo de terceiro: ela entra
    // num nome de arquivo, e um `../` ali escreveria fora da pasta de cache.
    // Só o que uma versão pode ter passa.
    let aceitavel = !versao.is_empty()
        && versao
            .chars()
            .all(|caractere| caractere.is_ascii_alphanumeric() || matches!(caractere, '.' | '-'))
        && !versao.contains("..");
    if !aceitavel {
        return None;
    }
    let base = if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA").map(PathBuf::from)
    } else {
        std::env::var_os("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|home| Path::new(&home).join(".cache")))
    }?;
    Some(
        base.join("er-ide")
            .join("typescript")
            .join(format!("lib-{versao}.txt")),
    )
}

/// Lê a biblioteca do TypeScript instalado a partir da raiz do projeto.
///
/// `None` quando não há `node_modules/typescript` — um projeto sem TypeScript
/// instalado não tem tipos para oferecer, e inventá-los seria afirmar o que não
/// se sabe.
pub(crate) fn ler(raiz: &Path) -> Option<BibliotecaDoProjeto> {
    let pasta = pasta_da_biblioteca(raiz)?;
    let versao = versao_instalada(&pasta).unwrap_or_default();
    let arquivos = arquivos_de_biblioteca(&pasta);
    let mut lidos = Vec::new();
    for caminho in arquivos {
        let Some(lib) = nome_do_lib(&caminho) else {
            continue;
        };
        let Ok(texto) = std::fs::read_to_string(&caminho) else {
            continue;
        };
        lidos.push((lib, texto));
    }
    let tipos = Biblioteca::nova(
        lidos
            .iter()
            .map(|(lib, texto)| (lib.clone(), texto.as_str())),
    );
    (tipos.nomes() > 0).then_some(BibliotecaDoProjeto {
        tipos,
        versao,
        alcance: Alcance::default(),
    })
}

/// Que `lib` valem para um projeto, a partir do `tsconfig`.
///
/// O `lib` explícito manda; sem ele, o `target` escolhe o `lib.<alvo>.full`, que
/// é o que o compilador faz. Cada arquivo lista os seus com
/// `/// <reference lib="…" />`, e seguir a corrente é a regra inteira.
///
/// Sem `lib` e sem `target`, o alcance sai **vazio**, que quer dizer "sem
/// filtro". É o lado certo de errar aqui: oferecer demais numa configuração que
/// não se soube ler é melhor do que calar sobre um projeto inteiro.
pub(crate) fn alcance_de(raiz: &Path, config: &TsConfig) -> Alcance {
    let Some(pasta) = pasta_da_biblioteca(raiz) else {
        return Alcance::default();
    };
    let sementes: Vec<String> = if config.lib.is_empty() {
        match config.target.as_deref() {
            Some(alvo) => vec![format!("{}.full", alvo.to_lowercase())],
            None => return Alcance::default(),
        }
    } else {
        config.lib.iter().map(|lib| lib.to_lowercase()).collect()
    };

    let mut libs = HashSet::new();
    let mut pilha = sementes;
    while let Some(lib) = pilha.pop() {
        if !libs.insert(lib.clone()) {
            continue;
        }
        let Ok(texto) = std::fs::read_to_string(pasta.join(format!("lib.{lib}.d.ts"))) else {
            continue;
        };
        pilha.extend(referencias_de(&texto));
    }
    Alcance::de(libs)
}

/// A pasta `lib` do TypeScript instalado, procurada subindo a partir da raiz.
///
/// Subir é o que faz um projeto de monorepo achar o TypeScript que está na raiz
/// do repositório, e não só o que estiver ao lado dele.
fn pasta_da_biblioteca(raiz: &Path) -> Option<PathBuf> {
    let mut atual = Some(raiz);
    while let Some(pasta) = atual {
        let candidata = pasta.join("node_modules").join("typescript").join("lib");
        if candidata.is_dir() {
            return Some(candidata);
        }
        atual = pasta.parent();
    }
    None
}

/// A versão declarada no `package.json` do TypeScript instalado.
fn versao_instalada(pasta_lib: &Path) -> Option<String> {
    let package = pasta_lib.parent()?.join("package.json");
    let texto = std::fs::read_to_string(package).ok()?;
    let valor: serde_json::Value = serde_json::from_str(&texto).ok()?;
    valor
        .get("version")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
}

/// Os `lib.*.d.ts` da pasta, sem entrar nas traduções.
fn arquivos_de_biblioteca(pasta: &Path) -> Vec<PathBuf> {
    let Ok(entradas) = std::fs::read_dir(pasta) else {
        return Vec::new();
    };
    let mut achados: Vec<PathBuf> = entradas
        .flatten()
        .map(|entrada| entrada.path())
        .filter(|caminho| {
            caminho.is_file()
                && caminho
                    .file_name()
                    .and_then(|nome| nome.to_str())
                    .is_some_and(|nome| nome.starts_with("lib.") && nome.ends_with(".d.ts"))
        })
        .collect();
    // Ordem estável: a fusão preserva a ordem em que os membros aparecem, e uma
    // lista que muda de ordem a cada abertura é impossível de conferir num teste.
    achados.sort();
    achados
}

/// `lib.es2022.array.d.ts` vira `es2022.array`.
fn nome_do_lib(caminho: &Path) -> Option<String> {
    let nome = caminho.file_name()?.to_str()?;
    Some(
        nome.strip_prefix("lib.")?
            .strip_suffix(".d.ts")?
            .to_lowercase(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Uma pasta `node_modules/typescript/lib` de mentira, com o que importa.
    ///
    /// De mentira **de propósito**: um teste que dependa do `node_modules` de um
    /// projeto real passa ou falha conforme a máquina, e não conforme o código.
    /// O que se afirma aqui é a regra, e a regra cabe em quatro arquivos.
    fn projeto(nome: &str) -> PathBuf {
        let raiz = std::env::temp_dir().join(format!("er-ts-stdlib-{nome}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&raiz);
        let lib = raiz.join("node_modules/typescript/lib");
        assert!(std::fs::create_dir_all(&lib).is_ok());
        assert!(
            std::fs::write(
                raiz.join("node_modules/typescript/package.json"),
                // **Uma versão que não pode existir.** O cache é chaveado pela
            // versão do TypeScript, e não pelo projeto: um teste que se diga
            // `5.9.3` grava por cima do cache do TypeScript de verdade desta
            // máquina, e o projeto seguinte receberia quatro tipos de mentira.
            "{\"version\": \"0.0.0-teste\"}",
            )
            .is_ok()
        );
        assert!(
            std::fs::write(
                lib.join("lib.es5.d.ts"),
                "interface Array<T> {\n  forEach(f: any): void;\n}\n",
            )
            .is_ok()
        );
        assert!(
            std::fs::write(
                lib.join("lib.es2022.array.d.ts"),
                "interface Array<T> {\n  at(i: number): T;\n}\n",
            )
            .is_ok()
        );
        assert!(
            std::fs::write(
                lib.join("lib.es2024.array.d.ts"),
                "interface Array<T> {\n  daFrente(): T;\n}\n",
            )
            .is_ok()
        );
        assert!(
            std::fs::write(
                lib.join("lib.es2022.full.d.ts"),
                "/// <reference lib=\"es5\" />\n/// <reference lib=\"es2022.array\" />\n",
            )
            .is_ok()
        );
        // Uma tradução, que não é declaração de tipo nenhuma.
        assert!(std::fs::create_dir_all(lib.join("pt-br")).is_ok());
        raiz
    }

    fn config(alvo: Option<&str>, libs: &[&str]) -> TsConfig {
        let mut config = TsConfig::default();
        config.target = alvo.map(str::to_owned);
        config.lib = libs.iter().map(|lib| (*lib).to_owned()).collect();
        config
    }

    /// A biblioteca instalada é lida, e a versão vem junto.
    #[test]
    fn the_installed_library_is_read_with_its_version() {
        let raiz = projeto("leitura");
        let Some(biblioteca) = ler(&raiz) else {
            panic!("a biblioteca precisa ser lida");
        };
        assert_eq!(biblioteca.versao, "0.0.0-teste");
        assert_eq!(biblioteca.tipos.nomes(), 1, "só `Array`");
        let _ = std::fs::remove_dir_all(&raiz);
    }

    /// **O alvo é seguido pela corrente de `reference`, e não adivinhado.**
    ///
    /// `ES2022` leva a `lib.es2022.full.d.ts`, que aponta para os que valem. Um
    /// alcance montado por palpite ofereceria o que o build recusa.
    #[test]
    fn the_target_is_followed_through_the_reference_chain() {
        let raiz = projeto("alcance");
        let Some(biblioteca) = ler(&raiz) else {
            panic!("a biblioteca precisa ser lida");
        };
        let alcance = alcance_de(&raiz, &config(Some("ES2022"), &[]));
        let Some(membros) = biblioteca.tipos.membros("Array", &alcance) else {
            panic!("`Array` precisa existir");
        };
        let nomes: Vec<_> = membros.itens.iter().map(|item| item.label.as_str()).collect();
        assert!(
            nomes.contains(&"forEach") && nomes.contains(&"at"),
            "a corrente traz es5 e es2022.array: {nomes:?}"
        );
        assert!(
            !nomes.contains(&"daFrente"),
            "ES2024 não vale num projeto ES2022: {nomes:?}"
        );
        let _ = std::fs::remove_dir_all(&raiz);
    }

    /// O `lib` explícito manda mais do que o `target`.
    #[test]
    fn an_explicit_lib_outranks_the_target() {
        let raiz = projeto("lib-explicito");
        let Some(biblioteca) = ler(&raiz) else {
            panic!("a biblioteca precisa ser lida");
        };
        // O alvo diria es5 + es2022.array; o `lib` diz só es5.
        let alcance = alcance_de(&raiz, &config(Some("ES2022"), &["ES5"]));
        let Some(membros) = biblioteca.tipos.membros("Array", &alcance) else {
            panic!("`Array` precisa existir");
        };
        let nomes: Vec<_> = membros.itens.iter().map(|item| item.label.as_str()).collect();
        assert!(nomes.contains(&"forEach"), "o es5 vale: {nomes:?}");
        assert!(!nomes.contains(&"at"), "o es2022 não foi pedido: {nomes:?}");
        let _ = std::fs::remove_dir_all(&raiz);
    }

    /// **Sem `target` e sem `lib`, não se filtra nada.**
    ///
    /// Um `tsconfig` que não se soube ler não pode calar sobre o projeto
    /// inteiro: oferecer demais é o lado certo de errar aqui.
    #[test]
    fn a_config_that_says_nothing_filters_nothing() {
        let raiz = projeto("sem-alvo");
        let Some(biblioteca) = ler(&raiz) else {
            panic!("a biblioteca precisa ser lida");
        };
        let alcance = alcance_de(&raiz, &config(None, &[]));
        let Some(membros) = biblioteca.tipos.membros("Array", &alcance) else {
            panic!("`Array` precisa existir");
        };
        let nomes: Vec<_> = membros.itens.iter().map(|item| item.label.as_str()).collect();
        assert!(nomes.contains(&"daFrente"), "sem filtro, tudo vale");
        let _ = std::fs::remove_dir_all(&raiz);
    }

    /// **Contra o TypeScript de verdade**, quando houver um apontado.
    ///
    /// Ignorado por padrão porque depende de um `node_modules` instalado, e um
    /// teste que passa conforme a máquina não é teste. Mas os quatro arquivos
    /// da biblioteca falsa provam a regra e não provam a **escala**: os `.d.ts`
    /// reais têm sobrecarga, genérico, `declare global` e comentário de
    /// documentação em toda linha, e é aqui que se descobre o que a gramática
    /// faz com isso.
    ///
    /// ```text
    /// ER_IDE_PROJETO_TS=C:\...\j-fis-cloud cargo test -p language-typescript -- --ignored
    /// ```
    #[test]
    #[ignore = "depende de um node_modules instalado"]
    fn the_real_typescript_library_is_read() {
        let Ok(raiz) = std::env::var("ER_IDE_PROJETO_TS") else {
            panic!("aponte ER_IDE_PROJETO_TS para um projeto com TypeScript instalado");
        };
        let raiz = PathBuf::from(raiz);
        let inicio = std::time::Instant::now();
        let Some(biblioteca) = ler(&raiz) else {
            panic!("a biblioteca do projeto precisa ser lida");
        };
        let leitura = inicio.elapsed();
        let config = crate::project::tsconfig::load(&raiz.join("tsconfig.json"))
            .unwrap_or_else(|error| panic!("o tsconfig precisa abrir: {error}"));
        let alcance = alcance_de(&raiz, &config);
        let Some(vetor) = biblioteca.tipos.membros("Array", &alcance) else {
            panic!("`Array` precisa existir na biblioteca de verdade");
        };
        let nomes: Vec<_> = vetor.itens.iter().map(|item| item.label.as_str()).collect();
        let gravado = biblioteca.tipos.escrever();
        let inicio = std::time::Instant::now();
        let relida = crate::analyzer::stdlib::Biblioteca::reler(&gravado);
        let releitura = inicio.elapsed();
        eprintln!(
            "[stdlib] versão {} | {} nomes | leitura {leitura:?} | cache {} KB, releitura {releitura:?} | Array com {} membros",
            biblioteca.versao,
            biblioteca.tipos.nomes(),
            gravado.len() / 1024,
            nomes.len()
        );
        assert_eq!(
            relida.nomes(),
            biblioteca.tipos.nomes(),
            "o cache precisa devolver o que guardou"
        );
        assert!(
            nomes.contains(&"forEach") && nomes.contains(&"map") && nomes.contains(&"length"),
            "o `Array` de verdade precisa trazer o básico: {nomes:?}"
        );

        // Seguir a herança é papel de quem pergunta, e não da biblioteca: ela
        // devolve o que cada tipo declara mais **de quem ele herda**. O
        // `Response` mostra por quê — `json` não é dele, é do `Body`, e uma
        // lista sem a cadeia teria oito membros no lugar de quinze.
        let com_herança = |tipo: &str| -> Vec<String> {
            let mut rotulos = Vec::new();
            let mut fila = vec![tipo.to_owned()];
            let mut vistos = std::collections::HashSet::new();
            while let Some(nome) = fila.pop() {
                if vistos.len() > 32 || !vistos.insert(nome.clone()) {
                    continue;
                }
                let Some(membros) = biblioteca.tipos.membros(&nome, &alcance) else {
                    continue;
                };
                rotulos.extend(membros.itens.into_iter().map(|item| item.label));
                fila.extend(membros.herda);
            }
            rotulos
        };

        // **Não é sobre um tipo.** A fase entrega a biblioteca inteira, e é isto
        // que separa "o `String[]` funcionou" de "a linguagem entrou no índice".
        for (tipo, esperado) in [
            ("String", "charAt"),
            ("Number", "toFixed"),
            ("Date", "getTime"),
            ("Promise", "then"),
            ("Map", "get"),
            ("RegExp", "test"),
            ("HTMLElement", "click"),
            // Herdado do `Body`, dois degraus acima.
            ("Response", "json"),
            // Herdado de `HTMLElement`, `Element`, `Node` e `EventTarget`.
            ("HTMLInputElement", "addEventListener"),
        ] {
            let rotulos = com_herança(tipo);
            assert!(
                !rotulos.is_empty(),
                "`{tipo}` precisa existir na biblioteca de verdade"
            );
            assert!(
                rotulos.contains(&esperado.to_owned()),
                "`{tipo}` precisa ter `{esperado}`: {rotulos:?}"
            );
        }
        // O DOM sozinho tem 2 136 declarações; um número muito menor quer dizer
        // que a leitura parou em algum lugar sem dizer.
        assert!(
            biblioteca.tipos.nomes() > 1_000,
            "vieram só {} nomes",
            biblioteca.tipos.nomes()
        );
    }

    /// Um projeto sem TypeScript instalado não ganha biblioteca inventada.
    #[test]
    fn a_project_without_typescript_has_no_library() {
        let raiz = std::env::temp_dir().join(format!("er-ts-sem-lib-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&raiz);
        assert!(std::fs::create_dir_all(&raiz).is_ok());
        assert!(ler(&raiz).is_none());
        let _ = std::fs::remove_dir_all(&raiz);
    }
}
