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

/// De que lado a diferença é pedida.
///
/// São duas perguntas diferentes sobre o mesmo arquivo, e confundi-las mostra a
/// diferença errada para quem já preparou parte do trabalho.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DiffSide {
    /// O que mudou na árvore de trabalho e ainda não está preparado.
    WorkingTree,
    /// O que está preparado, contra o último commit.
    Index,
}

/// O que uma linha da diferença é.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DiffLineKind {
    /// Igual dos dois lados: está ali para dar contexto.
    Context,
    Added,
    Removed,
}

/// O que a **margem do editor** mostra numa linha.
///
/// Não é o mesmo que [`DiffLineKind`], e a diferença é o motivo de os dois
/// existirem: o diff fala de linhas de um lado e do outro, e a margem fala do
/// arquivo que está na tela. Trocar uma linha por outra são duas linhas no
/// diff — uma removida e uma acrescentada — e **uma** marca na margem, porque
/// na tela é uma linha só, e ela mudou.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LineChange {
    /// Não existia antes.
    Added,
    /// Existia e foi trocada.
    Modified,
    /// Algo saiu daqui: a linha marcada é a que ficou no lugar.
    Removed,
}

/// Uma linha da diferença.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiffLine {
    pub kind: DiffLineKind,
    pub text: String,
    /// A linha no arquivo de agora, contada a partir de zero.
    ///
    /// Ausente numa linha removida, porque ela **não existe** no arquivo de
    /// agora — e é isso que impede a margem de marcar uma linha que só existia
    /// antes, deslocando todas as marcas abaixo dela.
    pub new_line: Option<usize>,
}

/// Um trecho contíguo de diferença.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Hunk {
    pub old_start: usize,
    pub new_start: usize,
    pub lines: Vec<DiffLine>,
}

/// A diferença de um arquivo.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct FileDiff {
    pub path: PathBuf,
    pub hunks: Vec<Hunk>,
}

impl FileDiff {
    /// As linhas do arquivo de agora que mudaram, e como a margem as mostra.
    ///
    /// # As três regras, e o defeito que cada uma evita
    ///
    /// - **remoção seguida de acréscimo é uma linha trocada.** É o caso mais
    ///   comum de todos — editar uma linha —, e contá-lo como duas marcas
    ///   encheria a margem de sinais onde houve uma alteração só;
    /// - **remoção sem acréscimo marca a linha que ficou no lugar.** Ela não tem
    ///   linha própria no arquivo de agora: o que sobrou é a fronteira entre
    ///   duas linhas. Sem marcar nada, apagar um bloco não deixaria sinal, e
    ///   quem olha o arquivo não saberia que algo saiu dali;
    /// - **acréscimo sem remoção é linha nova**, e é a única das três que tem
    ///   linha própria dos dois lados da conta.
    #[must_use]
    pub fn changed_lines(&self) -> Vec<(usize, LineChange)> {
        let mut marcas: Vec<(usize, LineChange)> = Vec::new();
        for hunk in &self.hunks {
            // Quantas remoções ainda esperam por um acréscimo que as substitua.
            let mut removidas = 0usize;
            let mut proxima = hunk.new_start;
            let fechar_remocao = |marcas: &mut Vec<(usize, LineChange)>,
                                      removidas: &mut usize,
                                      linha: usize| {
                if *removidas > 0 {
                    marcas.push((linha, LineChange::Removed));
                    *removidas = 0;
                }
            };
            for linha in &hunk.lines {
                match linha.kind {
                    DiffLineKind::Removed => removidas += 1,
                    DiffLineKind::Added => {
                        let numero = linha.new_line.unwrap_or(proxima);
                        proxima = numero + 1;
                        if removidas > 0 {
                            removidas -= 1;
                            marcas.push((numero, LineChange::Modified));
                        } else {
                            marcas.push((numero, LineChange::Added));
                        }
                    }
                    DiffLineKind::Context => {
                        let numero = linha.new_line.unwrap_or(proxima);
                        proxima = numero + 1;
                        fechar_remocao(&mut marcas, &mut removidas, numero);
                    }
                }
            }
            // Remoção no fim do trecho: a linha que ficou no lugar é a seguinte.
            fechar_remocao(&mut marcas, &mut removidas, proxima);
        }
        marcas.sort_unstable_by_key(|(linha, _)| *linha);
        marcas.dedup_by_key(|(linha, _)| *linha);
        marcas
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
