#![doc = "Provider Java nativo: gramática, parsing incremental e análise semântica."]

mod completion;
mod documents;
mod index;
mod language;
mod navigation;
mod observador;
mod parser;
mod semantics;
mod symbols;

pub use language::{JAVA_LANGUAGE_ID, JAVA_PROVIDER_ID, JavaLanguageProvider};
