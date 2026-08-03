//! O analisador externo: o `tsserver` do projeto.
//!
//! Este provider responde com **tipo** — completação com os membros certos,
//! diagnóstico de verdade, definição que atravessa módulo. É o que o provider
//! nativo não faz e não vai fazer.
//!
//! E ele **não substitui** o nativo. Sem Node, sem o pacote `typescript` no
//! projeto, ou com o processo morto, quem responde é o de baixo. Ver a ADR-025.

mod language;
mod locate;
mod plugin;
mod protocol;

pub use language::{TYPESCRIPT_SERVICE_PROVIDER_ID, TypeScriptServiceProvider};
pub use locate::tsserver_in;
pub use plugin::{AnalyzerPlugin, AnalyzerPluginSource, CompanionRule};
