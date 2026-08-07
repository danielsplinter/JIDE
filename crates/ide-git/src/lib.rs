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
//! A **fase 4**: tudo o que as anteriores tinham — `status`, a diferença, as
//! escritas por caminho, o histórico, o `commit`, as branches, a fusão, as tags
//! e o `stash` — mais o remoto: `fetch`, `pull`, `push`, as branches remotas e
//! a contagem de commits à frente e atrás.
//!
//! A granularidade continua sendo **por arquivo**: preparar por trecho ou por
//! linha é o que a `22` deixou anotado para depois das fases. E a resolução de
//! conflito acontece como edição de texto normal — que é como o conflito já
//! está gravado no arquivo.

mod adapters;

pub mod branches;
pub mod error;
pub mod history;
pub mod integration;
pub mod model;
pub mod remotes;
pub mod repository;
pub mod tags;
pub mod working_tree;

use std::path::{Path, PathBuf};
use std::sync::Arc;

pub use branches::BranchService;
pub use history::HistoryService;
pub use integration::IntegrationService;
pub use remotes::RemoteService;
pub use tags::TagService;
pub use error::{GitError, GitResult};
pub use model::{
    BranchName, BranchSummary, CommitId, CommitSummary, DiffLine, DiffLineKind, DiffSide, FileDiff,
    FileState, GraphRow, Head, Hunk, LineChange, LineSpan, MergeOutcome, PendingOperation,
    RemoteName,
    RepositoryStatus, StashEntry, StatusEntry, graph_rows,
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
        Arc::clone(&adapter) as Arc<dyn BranchService>,
        Arc::clone(&adapter) as Arc<dyn HistoryService>,
        Arc::clone(&adapter) as Arc<dyn IntegrationService>,
        Arc::clone(&adapter) as Arc<dyn TagService>,
        adapter as Arc<dyn RemoteService>,
    ))
}
