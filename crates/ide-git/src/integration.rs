//! Integração de históricos, e o estado entre commits.
//!
//! `merge`, `rebase` e `cherry-pick` estão juntos porque o que os une é mais
//! forte do que o que os separa: os três produzem conflito, os três param no
//! meio, e os três precisam de continuar, abortar e pular. A `22` decidiu isso
//! antes de existir código, e a fase 3 usa só o primeiro.

use async_trait::async_trait;
use ide_domain::CancellationToken;

use crate::error::GitResult;
use crate::model::{BranchName, MergeOutcome, PendingOperation};

/// O que a integração responde.
#[async_trait]
pub trait IntegrationService: Send + Sync {
    /// Traz para a branch atual o que está na outra.
    ///
    /// Sem token: é escrita, e cancelar pela metade deixaria o repositório num
    /// estado que ninguém pediu — no meio de uma fusão, ainda por cima.
    async fn merge(&self, branch: &BranchName) -> GitResult<MergeOutcome>;

    /// Que operação está em curso, se alguma.
    ///
    /// **É leitura de disco, e não memória nossa.** Quem rodou `git merge` no
    /// terminal integrado deixou o repositório no meio de uma operação, e uma
    /// IDE que só soubesse das fusões que ela mesma começou mostraria uma tela
    /// que não corresponde ao repositório.
    async fn pending(&self, cancel: &CancellationToken) -> GitResult<Option<PendingOperation>>;

    /// Conclui a operação em curso com o que está preparado.
    async fn continue_operation(&self) -> GitResult<()>;

    /// Desfaz a operação em curso e volta ao que era antes dela.
    async fn abort(&self) -> GitResult<()>;
}
