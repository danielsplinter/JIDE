//! Quais arquivos uma folha de estilo alcança, e sob que nome.
//!
//! É o nível 1b da fase 5 da `23`. O nível 1 completa o que o próprio arquivo
//! declara; medido em projeto real, isso quase nunca é onde as variáveis estão —
//! elas moram num arquivo de tema, e chegam por `@use` ou `@import`.
//!
//! # O custo está aqui, e não na extração
//!
//! Tirar `$cor: #333` de uma linha é trivial. Descobrir que `'../styles-config'`
//! quer dizer `../_styles-config.scss`, que `'./styles/index'` quer dizer
//! `./styles/_index.scss` e que `'@spartacus/styles/scss/core'` quer dizer
//! `node_modules/@spartacus/styles/scss/_core.scss` é o trabalho. As três formas
//! saíram de um projeto real, e não de documentação.

use std::{
    collections::HashMap,
    path::{Component, Path, PathBuf},
};

/// Quantos arquivos, no máximo, uma completação percorre.
///
/// Uma cadeia de `@forward` pode alcançar a folha de estilo inteira de um
/// framework. O teto existe para a lista aparecer enquanto se digita, e não
/// depois; ele corta abrangência, e não corretude — o que falta simplesmente
/// não é oferecido.
const TETO: usize = 64;

/// Quantos arquivos, no máximo, a subida percorre.
///
/// Mais generoso que o [`TETO`] da descida porque um parcial pode ser agregado
/// por vários arquivos, e cada um traz o seu escopo.
const TETO_ACIMA: usize = 128;

/// Quem importa quem, no projeto inteiro.
///
/// # Por que este grafo existe, e por que ele aponta para trás
///
/// Seguir `@import` enxerga **para baixo**: o que este arquivo trouxe. Mas o
/// modelo de escopo do `@import` é global — um parcial usa variáveis que **quem
/// o importou** trouxe, e ele próprio não importa nada.
///
/// Medido no projeto de referência: dos 134 `.scss` que usam `$`, 82 não
/// importam coisa alguma. Eles são agregados por um arquivo acima, e é lá que o
/// escopo deles nasce. Sem a seta invertida, a completação neles é vazia — e
/// vazia por olhar para o lado errado.
#[derive(Debug, Default)]
pub(crate) struct Grafo {
    /// Para cada arquivo, quem o importa.
    quem_importa: HashMap<PathBuf, Vec<PathBuf>>,
}

impl Grafo {
    /// Varre o projeto e liga as arestas, uma vez.
    ///
    /// **É a varredura inteira, e ela é feita uma vez por ativação.** Uma
    /// importação acrescentada em outro arquivo enquanto a IDE está aberta só é
    /// vista na próxima; é edição rara, e o preço de reconstruir a cada
    /// completação seria pago sempre.
    pub(crate) fn construir(raiz: &Path) -> Self {
        let mut quem_importa: HashMap<PathBuf, Vec<PathBuf>> = HashMap::new();
        for arquivo in folhas(raiz) {
            let Ok(texto) = std::fs::read_to_string(&arquivo) else {
                continue;
            };
            for (especificador, _) in importacoes(&texto, true) {
                if let Some(alvo) = resolver(&especificador, &arquivo, raiz) {
                    quem_importa.entry(alvo).or_default().push(arquivo.clone());
                }
            }
        }
        Self { quem_importa }
    }

    /// Os arquivos de cujo escopo este participa.
    ///
    /// Sobe pelas arestas invertidas — quem me importa, e quem importa esse — e
    /// devolve todos, porque cada nível pode ser onde as variáveis estão.
    pub(crate) fn ancestrais(&self, arquivo: &Path) -> Vec<PathBuf> {
        let mut achados = Vec::new();
        let mut vistos = vec![normalizar(arquivo)];
        let mut fila = vec![normalizar(arquivo)];
        while let Some(atual) = fila.pop() {
            if achados.len() >= TETO_ACIMA {
                break;
            }
            let Some(acima) = self.quem_importa.get(&atual) else {
                continue;
            };
            for pai in acima {
                if vistos.contains(pai) {
                    continue;
                }
                vistos.push(pai.clone());
                achados.push(pai.clone());
                fila.push(pai.clone());
            }
        }
        achados
    }
}

/// Os `.scss` do projeto, sem descer no que não é fonte.
fn folhas(raiz: &Path) -> Vec<PathBuf> {
    let mut achados = Vec::new();
    let mut pilha = vec![raiz.to_path_buf()];
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
                // `node_modules` fica de fora da **varredura**, e não da
                // resolução: uma biblioteca instalada é destino de importação, e
                // não origem de escopo do projeto. Varrê-la custaria dezenas de
                // milhares de arquivos para nada.
                if nome != "node_modules" && nome != "dist" && !nome.starts_with('.') {
                    pilha.push(caminho);
                }
            } else if nome.ends_with(".scss") {
                achados.push(normalizar(&caminho));
            }
        }
    }
    achados
}

/// Um arquivo alcançado, e o espaço de nomes que qualifica o que ele declara.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Alcancado {
    pub(crate) caminho: PathBuf,
    /// `None` quando os nomes entram sem qualificação.
    ///
    /// É o caso de `@import` — que despeja tudo no arquivo que importa — e de
    /// `@use ... as *`. Com `@use 'variaveis' as v`, o espaço é `v`, e escrever
    /// `$cor` sem o `v.` não acha nada.
    pub(crate) espaco: Option<String>,
}

/// Os arquivos que este alcança.
///
/// # Por que `@forward` e `@import` são seguidos, e `@use` não
///
/// É a semântica do Sass, e não uma economia nossa: um módulo **não reexporta**
/// o que ele próprio `@use`. Quem reexporta é o `@forward`, e é por isso que o
/// arranjo `_index.scss` cheio de `@forward` existe. Seguir `@use` em
/// profundidade ofereceria nomes que o arquivo de fato não enxerga — a resposta
/// errada com cara de certa.
pub(crate) fn alcancados(de: &Path, texto: &str, raiz: &Path) -> Vec<Alcancado> {
    let mut achados = Vec::new();
    let mut vistos = vec![de.to_path_buf()];
    let mut fila = Vec::new();

    // O primeiro nível: aqui `@use` conta, e é o único lugar onde ele conta.
    for (especificador, espaco) in importacoes(texto, true) {
        if let Some(caminho) = resolver(&especificador, de, raiz)
            && !vistos.contains(&caminho)
        {
            vistos.push(caminho.clone());
            achados.push(Alcancado {
                caminho: caminho.clone(),
                espaco: espaco.clone(),
            });
            fila.push((caminho, espaco));
        }
    }

    // Os seguintes: só o que é reexportado, herdando o espaço de quem trouxe.
    while let Some((caminho, espaco)) = fila.pop() {
        if achados.len() >= TETO {
            break;
        }
        let Ok(conteudo) = std::fs::read_to_string(&caminho) else {
            continue;
        };
        for (especificador, _) in importacoes(&conteudo, false) {
            if let Some(alcancado) = resolver(&especificador, &caminho, raiz)
                && !vistos.contains(&alcancado)
            {
                vistos.push(alcancado.clone());
                achados.push(Alcancado {
                    caminho: alcancado.clone(),
                    espaco: espaco.clone(),
                });
                fila.push((alcancado, espaco.clone()));
            }
        }
    }
    achados
}

/// As importações declaradas num texto, com o espaço de nomes de cada uma.
///
/// `com_use` distingue o primeiro nível dos seguintes: ver [`alcancados`].
fn importacoes(texto: &str, com_use: bool) -> Vec<(String, Option<String>)> {
    let mut encontradas = Vec::new();
    for linha in texto.lines() {
        let linha = linha.trim();
        let (resto, e_use) = match (
            linha.strip_prefix("@use "),
            linha.strip_prefix("@forward "),
            linha.strip_prefix("@import "),
        ) {
            (Some(resto), _, _) if com_use => (resto, true),
            (_, Some(resto), _) | (_, _, Some(resto)) => (resto, false),
            _ => continue,
        };
        let resto = resto.trim_end_matches(';').trim();
        // `@import 'a', 'b';` traz duas.
        for parte in resto.split(',') {
            let parte = parte.trim();
            let (citado, cauda) = match parte.split_once(char::is_whitespace) {
                Some((citado, cauda)) => (citado, cauda.trim()),
                None => (parte, ""),
            };
            let Some(especificador) = desaspar(citado) else {
                continue;
            };
            if especificador.starts_with("sass:") || especificador.ends_with(".css") {
                continue;
            }
            let espaco = if e_use {
                match cauda.strip_prefix("as ").map(str::trim) {
                    // `as *` despeja no arquivo que usa, sem qualificar.
                    Some("*") => None,
                    Some(apelido) => Some(apelido.to_owned()),
                    // Sem apelido, o espaço é o nome do arquivo — sem o
                    // sublinhado de parcial e sem a extensão.
                    None => Some(nome_do_modulo(&especificador)),
                }
            } else {
                None
            };
            encontradas.push((especificador, espaco));
        }
    }
    encontradas
}

fn desaspar(texto: &str) -> Option<String> {
    let sem = texto
        .strip_prefix('\'')
        .and_then(|resto| resto.strip_suffix('\''))
        .or_else(|| {
            texto
                .strip_prefix('"')
                .and_then(|resto| resto.strip_suffix('"'))
        })?;
    (!sem.is_empty()).then(|| sem.to_owned())
}

/// O espaço de nomes que o Sass dá a um `@use` sem apelido.
fn nome_do_modulo(especificador: &str) -> String {
    let ultimo = especificador.rsplit('/').next().unwrap_or(especificador);
    let sem_extensao = ultimo.strip_suffix(".scss").unwrap_or(ultimo);
    sem_extensao.strip_prefix('_').unwrap_or(sem_extensao).to_owned()
}

/// Onde está o arquivo que este especificador nomeia.
///
/// # As formas, e de onde elas vieram
///
/// Todas saíram de um projeto real:
///
/// | escrito | é o arquivo |
/// | --- | --- |
/// | `'../styles-config'` | `../_styles-config.scss` |
/// | `'./styles/index'` | `./styles/_index.scss` |
/// | `'@spartacus/styles/scss/core'` | `node_modules/@spartacus/styles/scss/_core.scss` |
/// | `'functions'` | `./_functions.scss`, ao lado |
///
/// O `~` de começo é herança de empacotador antigo, e quer dizer a mesma coisa
/// que o especificador nu.
fn resolver(especificador: &str, de: &Path, raiz: &Path) -> Option<PathBuf> {
    let especificador = especificador.strip_prefix('~').unwrap_or(especificador);
    let vizinhanca = de.parent()?;

    // Relativo, e o vizinho de mesma pasta, que o Sass também aceita.
    if let Some(achado) = candidatos(vizinhanca, especificador) {
        return Some(achado);
    }
    if especificador.starts_with('.') {
        return None;
    }
    // Especificador nu: `node_modules`, subindo a árvore como o Node faz.
    let mut atual = Some(vizinhanca);
    while let Some(diretorio) = atual {
        if let Some(achado) = candidatos(&diretorio.join("node_modules"), especificador) {
            return Some(achado);
        }
        atual = diretorio.parent();
    }
    candidatos(&raiz.join("node_modules"), especificador)
}

/// As quatro grafias possíveis de um mesmo módulo, na ordem que o Sass usa.
fn candidatos(base: &Path, especificador: &str) -> Option<PathBuf> {
    let bruto = base.join(especificador.replace('/', std::path::MAIN_SEPARATOR_STR));
    let pasta = bruto.parent()?;
    let nome = bruto.file_name()?.to_str()?;
    let tentativas = [
        pasta.join(format!("{nome}.scss")),
        pasta.join(format!("_{nome}.scss")),
        bruto.join("index.scss"),
        bruto.join("_index.scss"),
    ];
    tentativas
        .into_iter()
        .find(|caminho| caminho.is_file())
        .map(|caminho| normalizar(&caminho))
}

/// Tira os `.` e os `..` de um caminho, sem tocar no disco.
///
/// `resolver` monta `a/b/../c`, e o grafo compara caminhos por igualdade: sem
/// isto, o mesmo arquivo entraria duas vezes com nomes diferentes e a subida
/// não acharia o pai. `canonicalize` resolveria, mas vai ao disco a cada
/// comparação e devolve o prefixo estendido do Windows.
fn normalizar(caminho: &Path) -> PathBuf {
    let mut partes = PathBuf::new();
    for componente in caminho.components() {
        match componente {
            Component::CurDir => {}
            Component::ParentDir => {
                partes.pop();
            }
            outro => partes.push(outro.as_os_str()),
        }
    }
    partes
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Pasta(PathBuf);

    impl Pasta {
        fn nova(nome: &str) -> Self {
            let caminho = std::env::temp_dir().join(format!("er-ide-scss-{nome}"));
            let _ = std::fs::remove_dir_all(&caminho);
            assert!(std::fs::create_dir_all(&caminho).is_ok());
            Self(caminho)
        }

        fn arquivo(&self, relativo: &str, conteudo: &str) -> PathBuf {
            let destino = self.0.join(relativo.replace('/', std::path::MAIN_SEPARATOR_STR));
            if let Some(pai) = destino.parent() {
                assert!(std::fs::create_dir_all(pai).is_ok());
            }
            assert!(std::fs::write(&destino, conteudo).is_ok());
            destino
        }
    }

    impl Drop for Pasta {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn o_parcial_com_sublinhado_e_encontrado() {
        let pasta = Pasta::nova("parcial");
        pasta.arquivo("_styles-config.scss", "$cor: #333;");
        let de = pasta.arquivo("tema.scss", "@import '../styles-config';");
        assert_eq!(
            resolver("./styles-config", &de, &pasta.0),
            Some(pasta.0.join("_styles-config.scss"))
        );
    }

    #[test]
    fn a_pasta_com_indice_e_encontrada() {
        let pasta = Pasta::nova("indice");
        let indice = pasta.arquivo("styles/_index.scss", "$a: 1;");
        let de = pasta.arquivo("tema.scss", "");
        assert_eq!(resolver("./styles/index", &de, &pasta.0), Some(indice));
    }

    #[test]
    fn o_especificador_nu_vai_ao_node_modules() {
        let pasta = Pasta::nova("nu");
        let alvo = pasta.arquivo("node_modules/@algum/estilos/scss/_core.scss", "$a: 1;");
        let de = pasta.arquivo("src/app/tema.scss", "");
        assert_eq!(resolver("@algum/estilos/scss/core", &de, &pasta.0), Some(alvo));
    }

    /// `sass:map` é módulo embutido, e `.css` não traz declaração nossa.
    #[test]
    fn o_que_nao_e_arquivo_do_projeto_fica_de_fora() {
        let achadas = importacoes("@use 'sass:map';\n@import 'reset.css';\n", true);
        assert!(achadas.is_empty(), "{achadas:?}");
    }

    #[test]
    fn o_espaco_de_nomes_sai_do_apelido_ou_do_arquivo() {
        assert_eq!(
            importacoes("@use 'variaveis' as v;", true),
            vec![("variaveis".to_owned(), Some("v".to_owned()))]
        );
        assert_eq!(
            importacoes("@use 'tema/_cores.scss';", true),
            vec![("tema/_cores.scss".to_owned(), Some("cores".to_owned()))]
        );
        assert_eq!(
            importacoes("@use 'variaveis' as *;", true),
            vec![("variaveis".to_owned(), None)]
        );
        // `@import` nunca qualifica: ele despeja no arquivo que importa.
        assert_eq!(
            importacoes("@import 'variaveis';", true),
            vec![("variaveis".to_owned(), None)]
        );
    }

    #[test]
    fn um_import_com_virgula_traz_os_dois() {
        assert_eq!(
            importacoes("@import 'um', 'dois';", true).len(),
            2
        );
    }

    /// **`@forward` atravessa, `@use` não.**
    ///
    /// É a semântica do Sass: um módulo não reexporta o que ele próprio `@use`.
    /// Seguir em profundidade ofereceria nomes que o arquivo não enxerga.
    #[test]
    fn o_forward_atravessa_e_o_use_nao() {
        let pasta = Pasta::nova("cadeia");
        pasta.arquivo("_cores.scss", "$cor: #333;");
        pasta.arquivo("_grade.scss", "$grade: 8px;");
        pasta.arquivo(
            "estilos/_index.scss",
            "@forward '../cores';\n@use '../grade';\n",
        );
        let de = pasta.arquivo("tema.scss", "@import './estilos/index';");
        let achados = alcancados(&de, "@import './estilos/index';", &pasta.0);
        let nomes = achados
            .iter()
            .filter_map(|a| a.caminho.file_name().and_then(|n| n.to_str()))
            .collect::<Vec<_>>();
        assert!(nomes.contains(&"_index.scss"), "{nomes:?}");
        assert!(
            nomes.contains(&"_cores.scss"),
            "o `@forward` precisa atravessar: {nomes:?}"
        );
        assert!(
            !nomes.contains(&"_grade.scss"),
            "o `@use` de dentro não é reexportado: {nomes:?}"
        );
    }

    /// Um ciclo de importação não pode travar a IDE.
    #[test]
    fn um_ciclo_nao_prende() {
        let pasta = Pasta::nova("ciclo");
        pasta.arquivo("_a.scss", "@forward './b';");
        pasta.arquivo("_b.scss", "@forward './a';");
        let de = pasta.arquivo("tema.scss", "@import './a';");
        let achados = alcancados(&de, "@import './a';", &pasta.0);
        assert!(achados.len() <= 2, "{achados:?}");
    }

    /// O espaço de nomes de quem trouxe vale para o que veio junto.
    #[test]
    fn o_espaco_desce_pela_cadeia() {
        let pasta = Pasta::nova("espaco-desce");
        pasta.arquivo("_cores.scss", "$cor: #333;");
        pasta.arquivo("_index.scss", "@forward './cores';");
        let de = pasta.arquivo("tema.scss", "");
        let achados = alcancados(&de, "@use './index' as v;", &pasta.0);
        assert!(
            achados
                .iter()
                .all(|a| a.espaco.as_deref() == Some("v")),
            "{achados:?}"
        );
    }
}
