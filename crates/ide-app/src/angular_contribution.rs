//! Composição embutida de Angular.
//!
//! Ela não registra provider nenhum, e isso é o ponto: Angular não é uma
//! linguagem ao lado de TypeScript, é uma extensão dele. O que esta composição
//! faz é **entregar um plugin** ao analisador de TypeScript, que passa a
//! responder também pelos templates — no mesmo processo, no mesmo grafo.
//!
//! Medido, e é o motivo do desenho: o plugin custa +385 MB no analisador que já
//! sobe, contra +2,1 GB de um segundo processo. Ver a ADR-029.

use std::path::PathBuf;

use language_angular::AngularAnalyzerPlugin;

/// O diretório que a IDE carrega com os plugins de analisador.
///
/// Fica ao lado do executável porque é isso que ele é: parte da instalação, e
/// não do projeto de quem usa. Ausente, o resultado é degradar — projeto que
/// traz o próprio pacote continua servido, e os outros abrem o template como
/// HTML puro.
const PASTA: &str = "analyzers";

/// A contribuição de Angular ao analisador de TypeScript.
#[must_use]
pub fn analyzer_plugin() -> AngularAnalyzerPlugin {
    match reserva() {
        Some(caminho) => AngularAnalyzerPlugin::with_fallback(caminho),
        None => AngularAnalyzerPlugin::new(),
    }
}

/// Onde a instalação guarda os plugins, se guardar.
fn reserva() -> Option<PathBuf> {
    let caminho = std::env::current_exe()
        .ok()?
        .parent()?
        .join(PASTA);
    caminho.is_dir().then_some(caminho)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Sem a pasta ao lado do executável — que é o caso ao rodar os testes —, a
    /// contribuição existe e não promete reserva. Ela não pode falhar por isso.
    #[test]
    fn sem_a_pasta_a_contribuicao_continua_existindo() {
        let _ = analyzer_plugin();
    }

    /// O nome da pasta é parte do contrato com quem empacota a IDE, e trocá-lo
    /// em silêncio deixaria a reserva instalada e nunca encontrada.
    #[test]
    fn a_pasta_dos_plugins_tem_nome_fixo() {
        assert_eq!(PASTA, "analyzers");
    }
}
