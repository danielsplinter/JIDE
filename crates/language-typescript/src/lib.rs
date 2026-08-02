#![doc = "Tudo o que é exclusivo de TypeScript: por enquanto, a análise nativa."]
#![doc = ""]
#![doc = "Uma crate por linguagem, com as capacidades em módulos — fase 8 da `12`."]
#![doc = "Toolchain, build e o analisador externo entram aqui como módulos nas"]
#![doc = "fases seguintes da `23`, e não como crates novas."]

mod analyzer;
mod project;
mod service;
mod toolchain;

pub use analyzer::{TYPESCRIPT_LANGUAGE_ID, TYPESCRIPT_PROVIDER_ID, TypeScriptLanguageProvider};
pub use project::{NPM_BUILD_SYSTEM_ID, NpmAdapter, TsConfig, TsConfigError, npm::scripts, tsconfig};
pub use service::{TYPESCRIPT_SERVICE_PROVIDER_ID, TypeScriptServiceProvider, tsserver_in};
pub use toolchain::{NODE_TOOLCHAIN_ID, NodeToolchainProvider, node_executable};
