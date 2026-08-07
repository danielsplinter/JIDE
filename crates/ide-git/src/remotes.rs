//! Sincronização com o remoto.
//!
//! # A precaução que decide este módulo
//!
//! `GIT_TERMINAL_PROMPT=0`, que já vale para todo comando desta crate. Sem ela,
//! o `git` que precisa de senha tenta perguntar num terminal que não existe e o
//! processo **fica pendurado para sempre** — sem erro, sem saída, sem prazo. Com
//! ela, ele falha rápido, e a falha vira [`GitError::AuthenticationRequired`],
//! que é uma frase que se pode mostrar.
//!
//! [`GitError::AuthenticationRequired`]: crate::error::GitError::AuthenticationRequired

use async_trait::async_trait;
use ide_domain::CancellationToken;

use crate::error::GitResult;
use crate::model::{BranchName, RemoteName};

/// O que a sincronização responde.
#[async_trait]
pub trait RemoteService: Send + Sync {
    /// Os remotos configurados.
    async fn list(&self, cancel: &CancellationToken) -> GitResult<Vec<RemoteName>>;

    /// As branches que existem nos remotos, como `origin/main`.
    async fn remote_branches(&self, cancel: &CancellationToken) -> GitResult<Vec<BranchName>>;

    /// Traz as referências do remoto, sem tocar em arquivo nenhum.
    ///
    /// É o comando que **muda o repositório sem mudar o disco de trabalho**, e é
    /// por isso que a fase 4 precisou do observador antes: sem observar `.git`,
    /// a IDE não teria como saber que a contagem de commits mudou.
    async fn fetch(&self) -> GitResult<()>;

    /// Traz e integra o que veio.
    async fn pull(&self) -> GitResult<()>;

    /// Manda o que está aqui.
    ///
    /// `force` reescreve o que está lá, e é destrutivo do lado de fora: quem
    /// pergunta antes é a camada que tem usuário na frente.
    async fn push(&self, force: bool) -> GitResult<()>;
}
