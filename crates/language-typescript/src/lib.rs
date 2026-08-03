#![doc = "Tudo o que é exclusivo de TypeScript: por enquanto, a análise nativa."]
#![doc = ""]
#![doc = "Uma crate por linguagem, com as capacidades em módulos — fase 8 da `12`."]
#![doc = "Toolchain, build e o analisador externo entram aqui como módulos nas"]
#![doc = "fases seguintes da `23`, e não como crates novas."]

mod analyzer;
mod modules;
mod native;
mod project;
mod service;
mod toolchain;

pub use analyzer::TYPESCRIPT_LANGUAGE_ID;
pub use native::{TYPESCRIPT_PROVIDER_ID, TypeScriptLanguageProvider};
pub use project::{NPM_BUILD_SYSTEM_ID, NpmAdapter, TsConfig, TsConfigError, npm::scripts, tsconfig};
pub use service::{
    TYPESCRIPT_SERVICE_PROVIDER_ID, TypeScriptServiceProvider, tsserver_in,
};

/// A entrada por onde outra crate acrescenta um plugin ao analisador.
///
/// Genérica de propósito: esta crate carrega o que lhe entregam e não sabe de
/// que assunto é. Ver a `24`.
pub use service::{AnalyzerPlugin, AnalyzerPluginSource, CompanionRule};
pub use toolchain::{NODE_TOOLCHAIN_ID, NodeToolchainProvider, node_executable};

/// O resolvedor de módulos e o que ele precisa, para quem confere de fora.
///
/// Exposto porque o critério da fase 2 da `25` é uma **comparação com o
/// analisador**, e ela mora num teste de integração.
pub use modules::{ModuleResolver, Reexportacao, declarante};
