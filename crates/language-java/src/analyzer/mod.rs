//! Análise de Java: gramática, parsing incremental, índice e semântica.
//!
//! **Este módulo não alcança processo nem modelo de projeto.** Ele recebe as
//! raízes de fonte pelo `LanguageActivationContext` e responde sobre texto — não
//! executa `javac`, não lê `pom.xml`, não pergunta ao `ide-project`.
//!
//! Até a fase 8 da `12` isso era garantido pelo compilador, porque o analisador
//! era uma crate com dependências estreitas. Numa crate por linguagem a garantia
//! passa a ser uma guarda de texto, em `ide-core/tests/architecture.rs`. É mais
//! fraca, e a troca está registrada na especificação.

mod completion;
mod documents;
mod index;
mod language;
mod navigation;
mod observador;
mod parser;
mod semantics;
mod symbols;

pub(crate) mod classfile;

pub use language::{JAVA_LANGUAGE_ID, JAVA_PROVIDER_ID, JavaLanguageProvider};
