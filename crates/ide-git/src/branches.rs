//! As branches e as referências.

use async_trait::async_trait;
use ide_domain::CancellationToken;

use crate::error::GitResult;
use crate::model::{BranchName, BranchSummary};

/// O que o painel da esquerda do gerenciador pergunta.
#[async_trait]
pub trait BranchService: Send + Sync {
    /// As branches locais, em ordem de nome.
    async fn local(&self, cancel: &CancellationToken) -> GitResult<Vec<BranchSummary>>;

    /// Troca a branch de trabalho.
    ///
    /// **Ela recusa quando há alteração que o `checkout` sobrescreveria**, e a
    /// recusa vem como [`GitError::DirtyWorkingTree`] — que é o erro que vira o
    /// diálogo com *guardar* e *descartar*. Forçar a troca aqui perderia
    /// trabalho sem ninguém ter pedido.
    ///
    /// [`GitError::DirtyWorkingTree`]: crate::error::GitError::DirtyWorkingTree
    async fn switch(&self, branch: &BranchName) -> GitResult<()>;

    /// Cria uma branch e passa a trabalhar nela.
    ///
    /// Criar sem ir para lá é o caso raro, e ele se resolve trocando de volta;
    /// criar e ficar onde estava é o que quase ninguém quer, e daria uma branch
    /// que existe e não se usa.
    ///
    /// # De onde a branch nasce
    ///
    /// `base` é a branch de onde ela sai. `None` é "de onde se está", que é o
    /// que o `git` faz sozinho — e é o certo quando ninguém escolheu outra.
    ///
    /// **Escolher a base é diferente de trocar antes e criar depois**: o segundo
    /// caminho passa pela árvore de trabalho da base, e recusa quando há
    /// alteração que o `checkout` sobrescreveria. Nascer direto de uma
    /// referência não mexe no que está aberto.
    async fn create(&self, name: &BranchName, base: Option<&BranchName>) -> GitResult<()>;
}
