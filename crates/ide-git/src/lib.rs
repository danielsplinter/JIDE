//! Git: uma crate, com as capacidades em módulos. Ver a `22`.
//!
//! A IDE **sabe que existe Git** — branch, commit, alteração são conceitos dela.
//! A IDE **não sabe como o Git é falado**: nunca vê processo, argumento, saída
//! nem `stderr`. Chama trait, recebe tipo de domínio, trata erro tipado.
//!
//! O que separa uma coisa da outra aqui não é fronteira de crate; é privacidade
//! de módulo. `adapters` é privado, e o compilador garante o que antes seria
//! disciplina.
//!
//! # O que existe hoje
//!
//! A **fase 0**: descobrir, abrir, `status` e a lista de branches locais. Nada
//! de escrita. É pouco de propósito — é a fase que prova que a fronteira se
//! sustenta, com o menor código possível atrás dela.

mod adapters;

pub mod branches;
pub mod error;
pub mod model;
pub mod repository;
pub mod working_tree;

use std::path::{Path, PathBuf};
use std::sync::Arc;

pub use branches::BranchService;
pub use error::{GitError, GitResult};
pub use model::{
    BranchName, BranchSummary, CommitId, FileState, Head, RemoteName, RepositoryStatus,
    StatusEntry,
};
pub use repository::Repository;
pub use working_tree::WorkingTreeService;

/// Se este caminho está dentro de um repositório, e onde ele começa.
///
/// Sobe a cadeia de pastas procurando `.git`. **Arquivo conta tanto quanto
/// pasta**: numa `worktree` e num submódulo o `.git` é um arquivo que aponta
/// para outro lugar, e exigir pasta responderia "não é repositório" a quem está
/// dentro de um.
///
/// É síncrona e não roda processo nenhum: perguntar ao `git` onde ele começa
/// custaria um processo por projeto aberto para uma resposta que está no disco.
#[must_use]
pub fn discover(path: &Path) -> Option<PathBuf> {
    let mut atual = Some(path);
    while let Some(pasta) = atual {
        if pasta.join(".git").exists() {
            return Some(pasta.to_path_buf());
        }
        atual = pasta.parent();
    }
    None
}

/// Abre o repositório que contém este caminho.
///
/// É o **único** ponto de construção, e o único lugar onde o adapter é nomeado:
/// trocá-lo não muda esta assinatura nem nada acima dela.
///
/// Não roda `git` — quem não tem a ferramenta instalada descobre na primeira
/// pergunta, com [`GitError::ToolMissing`], e não ao abrir o projeto. Abrir a
/// IDE não pode depender de um processo externo responder.
///
/// # Erros
///
/// [`GitError::NotARepository`] quando não há `.git` em nenhuma pasta acima.
pub fn open(path: &Path) -> GitResult<Repository> {
    let root = discover(path).ok_or(GitError::NotARepository)?;
    let adapter = Arc::new(adapters::cli::CliGit::new(root.clone()));
    Ok(Repository::new(
        root,
        Arc::clone(&adapter) as Arc<dyn WorkingTreeService>,
        adapter as Arc<dyn BranchService>,
    ))
}
