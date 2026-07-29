#![doc = "Registro, ciclo de vida e isolamento dos providers de linguagem."]

mod host;
mod registry;
mod routing;
mod worker;

pub use host::{LanguageHost, LanguageHostConfig, LanguageHostError, ProviderSnapshot};
pub use ide_language_api::LanguageToolchainConfig;
pub use routing::ProviderSelection;
