//! Os erros são do domínio. Ver a `22`.

use std::path::PathBuf;

use crate::model::RemoteName;

pub type GitResult<T> = Result<T, GitError>;

/// O que pode dar errado, dito no vocabulário de quem usa a IDE.
///
/// A regra que esta lista existe para cumprir está na `22`: se o `stderr` do
/// `git` chegasse à interface como `String`, a IDE já conheceria a
/// implementação — e conheceria de um jeito que compila, passa nos testes e só
/// aparece na tela de quem usa.
///
/// A fase 0 só produz três destes. Os outros nascem agora mesmo assim, porque
/// acrescentar variante depois é mudar todo `match` que já existe, e porque a
/// lista inteira é o que dá sentido à decisão de não usar uma `String`.
#[derive(Debug, thiserror::Error)]
pub enum GitError {
    #[error("não é um repositório Git")]
    NotARepository,

    #[error("o repositório está em uso por outro processo")]
    RepositoryLocked,

    #[error("há conflitos a resolver")]
    Conflicted { paths: Vec<PathBuf> },

    #[error("há alterações não commitadas")]
    DirtyWorkingTree { paths: Vec<PathBuf> },

    #[error("o remoto pediu autenticação")]
    AuthenticationRequired { remote: RemoteName },

    #[error("referência não encontrada: {0}")]
    ReferenceNotFound(String),

    #[error("a operação foi cancelada")]
    Cancelled,

    /// O `git` não está no `PATH`.
    ///
    /// Degrada como o JDK: a IDE abre e trabalha, sem o gerenciador. Não é
    /// `Backend` porque não é falha da ferramenta — é ausência dela, e a
    /// diferença decide o que a tela diz.
    #[error("o Git não foi encontrado no PATH")]
    ToolMissing,

    /// O Git falhou de um jeito que não temos como classificar.
    ///
    /// O texto original fica aqui, para o registro e para o relatório de
    /// defeito. **Não** é o que a interface mostra como explicação.
    #[error("falha na ferramenta Git")]
    Backend { detail: String },
}
