//! Análise de TypeScript: gramática, realce, estrutura e índice de nomes.
//!
//! **Este módulo não alcança processo nem modelo de projeto.** Ele responde
//! sobre texto — não executa `npm`, não lê `package.json`, não pergunta ao
//! `ide-project`. A crate inteira alcança os dois, porque o módulo `project`
//! precisa; o analisador, não.
//!
//! Numa crate por linguagem `pub(crate)` não separa isso — ele protege o lado de
//! fora do lado de dentro, e não particiona o lado de dentro. Quem garante é uma
//! guarda de texto, em `ide-core/tests/architecture.rs`. Ver a fase 8 da `12`.

pub(crate) mod index;
mod lines;
pub(crate) mod members;
pub(crate) mod parser;
pub(crate) mod references;
pub(crate) mod stdlib;
pub(crate) mod syntax;

/// O nome da linguagem, que é texto e não projeto.
pub const TYPESCRIPT_LANGUAGE_ID: &str = "typescript";
