//! Os tipos que as capacidades compartilham.

use std::path::PathBuf;

/// O nome de uma branch, como o Git o escreve: `main`, `feature/busca`.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct BranchName(pub String);

impl BranchName {
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for BranchName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// O nome de um remoto: `origin`.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct RemoteName(pub String);

/// O identificador de um commit.
///
/// Guarda o hash **inteiro**, e a tela é quem abrevia: quem copia um hash vai
/// colar num comando, e um hash abreviado guardado seria a abreviação virando a
/// única coisa que a IDE tem.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct CommitId(pub String);

impl CommitId {
    /// Os primeiros sete caracteres, que é como o Git mesmo abrevia.
    #[must_use]
    pub fn short(&self) -> &str {
        let fim = self.0.char_indices().nth(7).map_or(self.0.len(), |(i, _)| i);
        &self.0[..fim]
    }
}

/// Para onde `HEAD` aponta.
///
/// O segundo caso não é curiosidade: um `checkout` de commit deixa o
/// repositório assim, e uma tela que só soubesse mostrar nome de branch
/// mostraria vazio sem dizer por quê.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Head {
    /// Uma branch, que é o caso de sempre.
    Branch(BranchName),
    /// Um commit direto: `detached HEAD`.
    Detached(CommitId),
    /// Repositório sem nenhum commit ainda.
    Unborn(BranchName),
}

impl Head {
    /// O que a barra de estado mostra.
    #[must_use]
    pub fn label(&self) -> String {
        match self {
            Self::Branch(branch) | Self::Unborn(branch) => branch.0.clone(),
            Self::Detached(commit) => commit.short().to_owned(),
        }
    }
}

/// Em que estado um arquivo está.
///
/// São os três painéis da aba `status` do gerenciador, e a divisão não é da
/// tela: é a que o `--porcelain=v2` devolve, e a que decide o que `stage` e
/// `discard` fazem em cada linha.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FileState {
    /// Está no índice, e entra no próximo commit.
    Staged,
    /// Mudou na árvore de trabalho, e não está preparado.
    Modified,
    /// O Git ainda não o conhece.
    Untracked,
    /// Tem conflito a resolver.
    Conflicted,
}

/// Um arquivo e o estado dele.
///
/// Um arquivo pode aparecer **duas vezes** — preparado e alterado ao mesmo
/// tempo é o que acontece quando se edita depois do `add`. São duas entradas de
/// propósito: cada painel mostra as suas, e juntá-las obrigaria a tela a decidir
/// em qual dos dois o arquivo aparece.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StatusEntry {
    pub path: PathBuf,
    pub state: FileState,
}

/// O retrato do repositório, calculado uma vez e lido por todos.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RepositoryStatus {
    pub head: Option<Head>,
    pub entries: Vec<StatusEntry>,
}

impl RepositoryStatus {
    /// Quantos arquivos estão em cada estado.
    #[must_use]
    pub fn count(&self, state: FileState) -> usize {
        self.entries
            .iter()
            .filter(|entry| entry.state == state)
            .count()
    }

    /// Quantos arquivos mudaram, contando cada um uma vez.
    ///
    /// É o número da barra de estado. Um arquivo preparado **e** alterado
    /// aparece duas vezes nas entradas, e contaria dois — o que diria que há
    /// mais trabalho do que há.
    #[must_use]
    pub fn changed_files(&self) -> usize {
        let mut vistos: Vec<&PathBuf> = self.entries.iter().map(|entry| &entry.path).collect();
        vistos.sort_unstable();
        vistos.dedup();
        vistos.len()
    }
}

/// Uma branch, como o painel da esquerda a mostra.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BranchSummary {
    pub name: BranchName,
    /// Se é para ela que `HEAD` aponta.
    pub current: bool,
    /// O upstream configurado, quando há.
    pub upstream: Option<BranchName>,
}
