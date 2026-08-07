//! Commits e histórico.

use async_trait::async_trait;
use ide_domain::CancellationToken;

use crate::error::GitResult;
use crate::model::{CommitId, CommitSummary};

/// O que o histórico responde.
///
/// **O `log` vem por páginas**, e não inteiro: um repositório de verdade tem
/// dezenas de milhares de commits, e carregar todos para mostrar quarenta linhas
/// é o oposto do que a `19` e a `20` fizeram no índice.
#[async_trait]
pub trait HistoryService: Send + Sync {
    /// Uma página do histórico, do mais recente para o mais antigo.
    ///
    /// `pular` é quantos commits já foram mostrados. A tabela é virtualizada, e
    /// quem rola pede a página seguinte.
    async fn log(
        &self,
        pular: usize,
        quantos: usize,
        cancel: &CancellationToken,
    ) -> GitResult<Vec<CommitSummary>>;

    /// Grava o que está preparado.
    ///
    /// Sem token, como toda escrita: cancelar um commit pela metade deixaria o
    /// repositório num estado que ninguém pediu.
    ///
    /// `amend` reescreve o commit anterior em vez de criar um novo. É a mesma
    /// chamada porque é o mesmo gesto com uma opção — separá-la em duas daria
    /// dois caminhos para a mesma coisa, e o segundo envelheceria.
    async fn commit(&self, message: &str, amend: bool) -> GitResult<CommitId>;
}
