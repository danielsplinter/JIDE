//! As tags.

use async_trait::async_trait;
use ide_domain::CancellationToken;

use crate::error::GitResult;

/// O que as tags respondem.
///
/// Só leitura na fase 3: criar e apagar tag é gesto de quem publica versão, e
/// nenhuma das duas coisas aparece no caminho de trabalho que esta
/// especificação cobre.
#[async_trait]
pub trait TagService: Send + Sync {
    /// As tags do repositório, em ordem de nome.
    async fn list(&self, cancel: &CancellationToken) -> GitResult<Vec<String>>;
}
