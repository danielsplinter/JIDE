//! O que faz um diretório ser um projeto TypeScript, e o que ele declara.

pub mod npm;
pub(crate) mod stdlib;
pub mod tsconfig;

pub use npm::{NPM_BUILD_SYSTEM_ID, NpmAdapter};
pub use tsconfig::{TsConfig, TsConfigError};
