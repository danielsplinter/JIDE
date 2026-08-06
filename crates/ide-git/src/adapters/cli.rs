//! O adapter de linha de comando: o único lugar da IDE que fala `git`.
//!
//! # As quatro precauções, e o que cada uma previne
//!
//! - **`--porcelain=v2 -z` com `LC_ALL=C`.** O formato humano do `git status`
//!   muda entre versões e traduz para o idioma do sistema; o `-z` ainda resolve
//!   nome com espaço e com acento, que o formato normal citaria e escaparia —
//!   e nomes em português os têm;
//! - **`GIT_TERMINAL_PROMPT=0`.** Sem isto, o `git` que precisa de senha tenta
//!   perguntar num terminal que não existe e o processo **fica pendurado para
//!   sempre**: sem erro, sem saída, sem prazo. É a falha mais difícil de
//!   diagnosticar desta especificação, e ela se previne com uma variável;
//! - **`--no-optional-locks`.** O `status` normal escreve o índice para guardar
//!   o que descobriu, e para isso pega o `index.lock` — o mesmo que o terminal
//!   integrado disputa. Uma leitura da IDE não pode fazer falhar um `git add`
//!   que o usuário acabou de dar;
//! - **`sem_janela_de_console`.** No binário de produção, todo filho de
//!   subsistema console abre uma janela preta ao lado da IDE.

use std::path::{Path, PathBuf};
use std::process::Stdio;

use async_trait::async_trait;
use ide_domain::CancellationToken;
use tokio::process::Command;

use crate::branches::BranchService;
use crate::error::{GitError, GitResult};
use crate::model::{
    BranchName, BranchSummary, CommitId, FileState, Head, RepositoryStatus, StatusEntry,
};
use crate::working_tree::WorkingTreeService;

/// O Git falado por processo.
pub(crate) struct CliGit {
    root: PathBuf,
}

impl CliGit {
    pub(crate) fn new(root: PathBuf) -> Self {
        Self { root }
    }

    /// Roda o `git` na raiz deste repositório e devolve a saída crua.
    ///
    /// O cancelamento é conferido **antes e depois**, e não no meio: matar um
    /// `status` pela metade não devolve nada a ninguém, e o que o token evita
    /// aqui é a resposta que já não interessa continuar sendo processada.
    async fn run(&self, args: &[&str], cancel: &CancellationToken) -> GitResult<Vec<u8>> {
        if cancel.is_cancelled() {
            return Err(GitError::Cancelled);
        }
        let mut command = Command::new("git");
        command
            .args(args)
            .current_dir(&self.root)
            .env("LC_ALL", "C")
            .env("GIT_TERMINAL_PROMPT", "0")
            .stdin(Stdio::null())
            .kill_on_drop(true);
        ide_process::sem_janela_de_console(&mut command);
        let saida = match command.output().await {
            Ok(saida) => saida,
            Err(erro) if erro.kind() == std::io::ErrorKind::NotFound => {
                return Err(GitError::ToolMissing);
            }
            Err(erro) => {
                return Err(GitError::Backend {
                    detail: erro.to_string(),
                });
            }
        };
        if cancel.is_cancelled() {
            return Err(GitError::Cancelled);
        }
        if saida.status.success() {
            return Ok(saida.stdout);
        }
        Err(classificar(&String::from_utf8_lossy(&saida.stderr)))
    }
}

/// Traduz o que o `git` reclamou para o vocabulário do domínio.
///
/// O que não se sabe classificar vira [`GitError::Backend`], que é a válvula
/// honesta: classificação inventada seria pior que nenhuma.
fn classificar(stderr: &str) -> GitError {
    let texto = stderr.to_lowercase();
    if texto.contains("not a git repository") {
        return GitError::NotARepository;
    }
    if texto.contains("index.lock") || texto.contains("unable to create") {
        return GitError::RepositoryLocked;
    }
    if texto.contains("authentication") || texto.contains("could not read username") {
        return GitError::AuthenticationRequired {
            remote: crate::model::RemoteName("origin".to_owned()),
        };
    }
    if texto.contains("unknown revision") || texto.contains("not a valid ref") {
        return GitError::ReferenceNotFound(stderr.trim().to_owned());
    }
    GitError::Backend {
        detail: stderr.trim().to_owned(),
    }
}

#[async_trait]
impl WorkingTreeService for CliGit {
    async fn status(&self, cancel: &CancellationToken) -> GitResult<RepositoryStatus> {
        let saida = self
            .run(
                &[
                    "--no-optional-locks",
                    "status",
                    "--porcelain=v2",
                    "--branch",
                    "--untracked-files=all",
                    "-z",
                ],
                cancel,
            )
            .await?;
        Ok(ler_status(&String::from_utf8_lossy(&saida), &self.root))
    }
}

#[async_trait]
impl BranchService for CliGit {
    async fn local(&self, cancel: &CancellationToken) -> GitResult<Vec<BranchSummary>> {
        // O `%09` é tabulação, e nome de referência não pode conter uma: o Git
        // recusa espaço e todo caractere de controle em `refs/`. É separador que
        // não aparece no dado.
        let saida = self
            .run(
                &[
                    "for-each-ref",
                    "--sort=refname",
                    "--format=%(refname:short)%09%(upstream:short)%09%(HEAD)",
                    "refs/heads/",
                ],
                cancel,
            )
            .await?;
        Ok(ler_branches(&String::from_utf8_lossy(&saida)))
    }
}

/// Lê a saída de `for-each-ref`: nome, upstream e o `*` da atual.
fn ler_branches(saida: &str) -> Vec<BranchSummary> {
    saida
        .lines()
        .filter(|linha| !linha.trim().is_empty())
        .map(|linha| {
            let mut campos = linha.split('\t');
            let nome = campos.next().unwrap_or_default().trim();
            let upstream = campos.next().unwrap_or_default().trim();
            let marca = campos.next().unwrap_or_default().trim();
            BranchSummary {
                name: BranchName(nome.to_owned()),
                current: marca == "*",
                upstream: (!upstream.is_empty()).then(|| BranchName(upstream.to_owned())),
            }
        })
        .collect()
}

/// Lê a saída de `status --porcelain=v2 -z`.
///
/// # O caso que o `-z` cria
///
/// Numa renomeação, o caminho de origem vem como **um campo separado** logo
/// depois do registro, e não colado nele por tabulação como no formato de
/// linhas. Quem varresse os registros sem consumi-lo trataria o caminho antigo
/// como um registro solto — e um registro que não começa com `1`, `2`, `u` ou
/// `?` seria descartado em silêncio, fazendo a renomeação sumir da tela.
fn ler_status(saida: &str, root: &Path) -> RepositoryStatus {
    let mut status = RepositoryStatus::default();
    let mut oid: Option<String> = None;
    let mut nome_do_head: Option<String> = None;
    let mut campos = saida.split('\0').filter(|campo| !campo.is_empty());
    while let Some(campo) = campos.next() {
        match campo.as_bytes().first() {
            Some(b'#') => {
                if let Some(resto) = campo.strip_prefix("# branch.oid ") {
                    oid = Some(resto.trim().to_owned());
                } else if let Some(resto) = campo.strip_prefix("# branch.head ") {
                    nome_do_head = Some(resto.trim().to_owned());
                }
            }
            Some(b'1') | Some(b'2') => {
                if campo.starts_with("2 ") {
                    // O caminho de origem da renomeação, que não é registro.
                    let _ = campos.next();
                }
                if let Some((caminho, xy)) = caminho_e_xy(campo) {
                    for estado in estados_de(xy) {
                        status.entries.push(StatusEntry {
                            path: root.join(&caminho),
                            state: estado,
                        });
                    }
                }
            }
            Some(b'u') => {
                if let Some((caminho, _)) = caminho_e_xy(campo) {
                    status.entries.push(StatusEntry {
                        path: root.join(&caminho),
                        state: FileState::Conflicted,
                    });
                }
            }
            Some(b'?') => {
                if let Some(caminho) = campo.strip_prefix("? ") {
                    status.entries.push(StatusEntry {
                        path: root.join(caminho),
                        state: FileState::Untracked,
                    });
                }
            }
            // `!` é ignorado, e ignorado não pedimos. Qualquer outra coisa é
            // formato que não conhecemos, e inventar leitura seria pior.
            _ => {}
        }
    }
    status.head = montar_head(nome_do_head, oid);
    status
}

/// O `XY` e o caminho de um registro `1`, `2` ou `u`.
///
/// O caminho é o **último** campo separado por espaço, e vem depois de um número
/// fixo de campos — mas nomes de arquivo têm espaço, e por isso ele é lido pelo
/// resto da linha a partir do campo certo, e não por `split` até o fim.
fn caminho_e_xy(campo: &str) -> Option<(String, &str)> {
    let (tipo, resto) = campo.split_once(' ')?;
    let (xy, resto) = resto.split_once(' ')?;
    // Depois do `XY` vêm campos fixos; o caminho é tudo o que sobra depois
    // deles. `1` tem 6, `2` tem 7 — o sétimo é o escore da renomeação —, e `u`
    // tem 8.
    let fixos = match tipo {
        "1" => 6,
        "2" => 7,
        _ => 8,
    };
    let mut restante = resto;
    for _ in 0..fixos {
        restante = restante.split_once(' ')?.1;
    }
    Some((restante.to_owned(), xy))
}

/// Em que painéis um registro aparece, lendo o `XY`.
///
/// `X` é o índice e `Y` é a árvore de trabalho, e `.` quer dizer "não mudou".
/// Um arquivo alterado depois do `add` tem os dois, e aparece nos dois painéis:
/// é o que de fato está acontecendo com ele.
fn estados_de(xy: &str) -> Vec<FileState> {
    let mut estados = Vec::new();
    let mut letras = xy.chars();
    let indice = letras.next().unwrap_or('.');
    let arvore = letras.next().unwrap_or('.');
    if indice != '.' {
        estados.push(FileState::Staged);
    }
    if arvore != '.' {
        estados.push(FileState::Modified);
    }
    estados
}

/// Para onde `HEAD` aponta, a partir das duas linhas de cabeçalho.
fn montar_head(nome: Option<String>, oid: Option<String>) -> Option<Head> {
    let nome = nome?;
    let oid = oid.unwrap_or_default();
    if nome == "(detached)" {
        return Some(Head::Detached(CommitId(oid)));
    }
    if oid == "(initial)" {
        return Some(Head::Unborn(BranchName(nome)));
    }
    Some(Head::Branch(BranchName(nome)))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A leitura do formato, sem processo nenhum.
    ///
    /// Os testes com repositório de verdade estão em `tests/`, como a `22`
    /// exige. Estes cobrem o que é caro de montar com `git`: renomeação,
    /// arquivo com espaço no nome, e `HEAD` solto.
    #[test]
    fn o_caminho_de_origem_da_renomeacao_nao_vira_registro() {
        let saida = concat!(
            "# branch.oid abc123\0",
            "# branch.head main\0",
            "2 R. N... 100644 100644 100644 aaa bbb R100 novo.txt\0",
            "velho.txt\0",
            "? outro.txt\0"
        );
        let status = ler_status(saida, Path::new("/projeto"));
        assert_eq!(status.entries.len(), 2, "{:?}", status.entries);
        assert_eq!(status.entries[0].path, Path::new("/projeto").join("novo.txt"));
        assert_eq!(status.entries[0].state, FileState::Staged);
        assert_eq!(status.entries[1].state, FileState::Untracked);
    }

    #[test]
    fn nome_com_espaco_chega_inteiro() {
        let saida = "1 .M N... 100644 100644 100644 aaa bbb Contas a pagar.txt\0";
        let status = ler_status(saida, Path::new("/p"));
        assert_eq!(
            status.entries.first().map(|entry| entry.path.clone()),
            Some(Path::new("/p").join("Contas a pagar.txt"))
        );
    }

    #[test]
    fn preparado_e_alterado_e_o_mesmo_arquivo_em_dois_paineis() {
        let saida = "1 MM N... 100644 100644 100644 aaa bbb a.txt\0";
        let status = ler_status(saida, Path::new("/p"));
        assert_eq!(status.count(FileState::Staged), 1);
        assert_eq!(status.count(FileState::Modified), 1);
        assert_eq!(status.changed_files(), 1, "a barra conta arquivos, não linhas");
    }

    #[test]
    fn head_solto_mostra_o_commit_abreviado() {
        let saida = "# branch.oid 0123456789abcdef\0# branch.head (detached)\0";
        let status = ler_status(saida, Path::new("/p"));
        assert_eq!(
            status.head.as_ref().map(Head::label),
            Some("0123456".to_owned())
        );
    }

    #[test]
    fn repositorio_sem_commit_ainda_tem_branch() {
        let saida = "# branch.oid (initial)\0# branch.head main\0";
        let status = ler_status(saida, Path::new("/p"));
        assert_eq!(
            status.head,
            Some(Head::Unborn(BranchName("main".to_owned())))
        );
    }

    #[test]
    fn a_branch_atual_vem_marcada() {
        let branches = ler_branches("main\torigin/main\t*\nfeature/busca\t\t\n");
        assert_eq!(branches.len(), 2);
        assert!(branches[0].current);
        assert_eq!(
            branches[0].upstream,
            Some(BranchName("origin/main".to_owned()))
        );
        assert!(!branches[1].current);
        assert_eq!(branches[1].upstream, None);
    }
}
