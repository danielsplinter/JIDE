//! O estado dos arquivos e do índice.

use async_trait::async_trait;
use ide_domain::CancellationToken;

use crate::error::GitResult;
use crate::model::RepositoryStatus;

/// O que a árvore de trabalho responde.
///
/// A fase 0 tem só a leitura. `stage`, `unstage`, `discard` e `diff` entram na
/// fase 1, e entram **aqui** — a assinatura assíncrona com cancelamento já está
/// posta porque é ela que não se retrofita: `fn status()` e
/// `async fn status(cancel)` são arquiteturas diferentes, e descobrir isso
/// depois reescreve todas as chamadas.
#[async_trait]
pub trait WorkingTreeService: Send + Sync {
    /// O retrato do repositório: para onde `HEAD` aponta e o que mudou.
    async fn status(&self, cancel: &CancellationToken) -> GitResult<RepositoryStatus>;
}
