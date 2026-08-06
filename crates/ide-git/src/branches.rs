//! As branches e as referências.

use async_trait::async_trait;
use ide_domain::CancellationToken;

use crate::error::GitResult;
use crate::model::BranchSummary;

/// O que o painel da esquerda do gerenciador pergunta.
///
/// Trocar e criar branch são fase 3; listar é fase 0, porque é leitura pura e é
/// o que dá conteúdo ao primeiro nó da árvore.
#[async_trait]
pub trait BranchService: Send + Sync {
    /// As branches locais, em ordem de nome.
    async fn local(&self, cancel: &CancellationToken) -> GitResult<Vec<BranchSummary>>;
}
