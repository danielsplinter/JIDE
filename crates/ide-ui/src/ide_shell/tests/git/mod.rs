//! Testes do gerenciador de Git, partidos por assunto. Ver a `22`.
//!
//! Eles estavam num arquivo só, e ele passou das 1 400 linhas que a guarda
//! permite — a mesma que substituiu o teto de `tests.rs` quando ele foi partido.
//! A regra é a de lá, e ela funcionou como devia: **parte-se o assunto, e não o
//! teto**.
//!
//! Os ajudantes ficam aqui, no lugar que os quatro alcançam: os dois retratos
//! servem a mais de um assunto, e copiá-los daria a divergência na primeira
//! correção.

use super::*;

/// A janela, as abas e a árvore.
mod gerenciador;
/// A aba `Status`.
mod alteracoes;
/// A aba `History` e o commit.
mod historico;
/// A aba `Diff` e a margem.
mod diff;
