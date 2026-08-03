//! Composição embutida de Angular.
//!
//! Ela não registra provider nenhum, e isso é o ponto: Angular não é uma
//! linguagem ao lado de TypeScript, é uma extensão dele. O que esta composição
//! faz é **entregar um plugin** ao analisador de TypeScript, que passa a
//! responder também pelos templates — no mesmo processo, no mesmo grafo.
//!
//! Medido, e é o motivo do desenho: o plugin custa +385 MB no analisador que já
//! sobe, contra +2,1 GB de um segundo processo. Ver a ADR-029.

use std::path::{Path, PathBuf};

use language_angular::AngularAnalyzerPlugin;

/// O `@angular/language-service`, dentro do executável.
///
/// # Por que dentro, e não ao lado
///
/// A IDE é um executável, e "vem junto" precisa continuar querendo dizer *um
/// arquivo para copiar*. Uma pasta ao lado do binário depende de o
/// empacotamento acertar, e some quando alguém copia só o `.exe` — que é
/// exatamente o que se faz.
///
/// São 4,1 MB, e a procedência está em `vendor/angular-language-service`.
const EMBARCADO: &[(&str, &[u8])] = &[
    (
        "package.json",
        include_bytes!("../vendor/angular-language-service/package.json"),
    ),
    (
        "index.js",
        include_bytes!("../vendor/angular-language-service/index.js"),
    ),
    (
        "factory_bundle.js",
        include_bytes!("../vendor/angular-language-service/factory_bundle.js"),
    ),
    (
        "bundles/language-service.js",
        include_bytes!("../vendor/angular-language-service/bundles/language-service.js"),
    ),
];

/// A contribuição de Angular ao analisador de TypeScript.
///
/// A reserva é extraída na primeira vez que a IDE sobe com esta versão. Falhar
/// a extração não é erro: o resultado é a IDE servir só os projetos que trazem
/// o próprio pacote, e os outros abrirem o template como HTML puro — que é
/// degradar, e não recusar, como manda a ADR-025.
#[must_use]
pub fn analyzer_plugin() -> AngularAnalyzerPlugin {
    match extrair() {
        Some(caminho) => AngularAnalyzerPlugin::with_fallback(caminho),
        None => AngularAnalyzerPlugin::new(),
    }
}

/// Põe o pacote embarcado no disco, e devolve o diretório de sondagem.
///
/// # Por que o nome do diretório carrega o conteúdo
///
/// O carimbo é o tamanho de cada arquivo embarcado. Binário novo com pacote
/// novo escreve num diretório novo, e o antigo deixa de ser procurado — sem
/// isso, um cache velho sobreviveria à atualização e responderia por uma versão
/// que não está mais no executável, que é a resposta velha com cara de nova que
/// a `21` nomeia.
///
/// Extrair só quando falta é o que faz o custo ser pago uma vez: nas aberturas
/// seguintes isto é um `is_file` por arquivo.
fn extrair() -> Option<PathBuf> {
    let raiz = cache()?.join("analyzers").join(carimbo());
    let destino = raiz.join("node_modules").join("@angular").join("language-service");
    for (relativo, conteudo) in EMBARCADO {
        let caminho = destino.join(relativo);
        if caminho.metadata().is_ok_and(|dados| {
            usize::try_from(dados.len()).is_ok_and(|tamanho| tamanho == conteudo.len())
        }) {
            continue;
        }
        std::fs::create_dir_all(caminho.parent()?).ok()?;
        std::fs::write(&caminho, conteudo).ok()?;
    }
    Some(raiz)
}

/// O carimbo do que está embarcado.
///
/// Tamanho e não resumo criptográfico: o que se precisa é distinguir versões,
/// e duas versões do mesmo bundle não têm o mesmo número de bytes. Um resumo
/// custaria ler 4 MB a cada abertura para responder a mesma coisa.
fn carimbo() -> String {
    EMBARCADO
        .iter()
        .map(|(_, conteudo)| conteudo.len().to_string())
        .collect::<Vec<_>>()
        .join("-")
}

/// Onde a máquina guarda o que pode ser reconstruído.
///
/// O mesmo lugar que o índice de TypeScript usa, e pelo mesmo motivo: não é
/// dado de quem usa, é cópia do que já está no executável.
fn cache() -> Option<PathBuf> {
    let base = if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA").map(PathBuf::from)
    } else {
        std::env::var_os("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|home| Path::new(&home).join(".cache")))
    }?;
    Some(base.join("er-ide"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A extração precisa acontecer, e o que sai precisa ser o que o
    /// `tsserver` procura.
    #[test]
    fn o_pacote_embarcado_chega_ao_disco() {
        let Some(raiz) = extrair() else {
            panic!("a reserva precisa ser extraída");
        };
        let pacote = raiz.join("node_modules").join("@angular").join("language-service");
        for (relativo, conteudo) in EMBARCADO {
            let caminho = pacote.join(relativo);
            assert!(caminho.is_file(), "faltou {relativo} em {caminho:?}");
            let Ok(gravado) = std::fs::metadata(&caminho) else {
                panic!("não foi possível ler {relativo}");
            };
            assert_eq!(
                usize::try_from(gravado.len()).unwrap_or(usize::MAX),
                conteudo.len(),
                "{relativo} saiu truncado"
            );
        }
    }

    /// Extrair duas vezes não reescreve nada, e devolve o mesmo lugar.
    #[test]
    fn extrair_de_novo_e_barato_e_estavel() {
        let primeira = extrair();
        let segunda = extrair();
        assert_eq!(primeira, segunda);
    }

    /// O carimbo tem de mudar quando o conteúdo muda — é o que faz a
    /// atualização se resolver sozinha.
    #[test]
    fn o_carimbo_vem_do_conteudo() {
        let atual = carimbo();
        assert!(atual.contains('-'), "o carimbo cobre todos os arquivos: {atual}");
        for (_, conteudo) in EMBARCADO {
            assert!(
                atual.contains(&conteudo.len().to_string()),
                "o carimbo precisa cobrir cada arquivo: {atual}"
            );
        }
    }

    /// A versão que a procedência declara é a que está embarcada.
    ///
    /// Sem isto, atualizar o pacote e esquecer o documento deixaria a IDE
    /// dizendo uma versão e carregando outra — e é o tipo de erro que só
    /// aparece quando alguém for depurar um defeito de template.
    #[test]
    fn a_versao_embarcada_esta_declarada() {
        let Some((_, package_json)) = EMBARCADO.iter().find(|(nome, _)| *nome == "package.json")
        else {
            panic!("o pacote precisa trazer o package.json");
        };
        let manifesto = String::from_utf8_lossy(package_json);
        let Some(inicio) = manifesto.find("\"version\"") else {
            panic!("o package.json precisa declarar a versão");
        };
        let Some(versao) = manifesto[inicio..]
            .split('"')
            .nth(3)
            .map(str::to_owned)
        else {
            panic!("não foi possível ler a versão do package.json");
        };
        let procedencia = include_str!("../vendor/angular-language-service/PROVENIENCIA.md");
        assert!(
            procedencia.contains(&versao),
            "a procedência não menciona a versão embarcada, {versao}"
        );
    }
}
