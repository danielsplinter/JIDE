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
use std::ffi::OsStr;
use std::process::Stdio;

use async_trait::async_trait;
use ide_domain::CancellationToken;
use tokio::process::Command;

use crate::branches::BranchService;
use crate::error::{GitError, GitResult};
use crate::model::{
    BranchName, BranchSummary, CommitId, CommitSummary, DiffLine, DiffLineKind, DiffSide, FileDiff,
    FileState, Head, Hunk, MergeOutcome, PendingOperation, RepositoryStatus, StashEntry,
    StatusEntry,
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
        let args: Vec<&OsStr> = args.iter().map(OsStr::new).collect();
        self.run_os(&args, cancel).await
    }

    /// O mesmo, para quem tem caminho em vez de texto.
    ///
    /// Caminho no Windows não é UTF-8, e converter com perda faria um arquivo
    /// alterado virar um arquivo que o `git` não acha. É a armadilha que a `22`
    /// registrou nos riscos, e o `OsStr` é o que a evita.
    async fn run_os(&self, args: &[&OsStr], cancel: &CancellationToken) -> GitResult<Vec<u8>> {
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

impl CliGit {
    /// O caminho como o `git` o espera: relativo à raiz do repositório.
    ///
    /// Absoluto também funciona na maioria dos comandos, mas não em todos — o
    /// `show` de uma revisão não aceita —, e um caminho só evita descobrir isso
    /// comando a comando.
    fn relativo(&self, path: &Path) -> PathBuf {
        path.strip_prefix(&self.root)
            .unwrap_or(path)
            .to_path_buf()
    }

    /// Roda um comando de escrita sobre uma lista de caminhos.
    ///
    /// Sem token: cancelar uma escrita pela metade deixaria o repositório num
    /// estado que ninguém pediu. Ver o contrato em `working_tree`.
    async fn escrever(&self, args: &[&str], paths: &[PathBuf]) -> GitResult<()> {
        if paths.is_empty() {
            return Ok(());
        }
        let relativos: Vec<PathBuf> = paths.iter().map(|path| self.relativo(path)).collect();
        let mut todos: Vec<&OsStr> = args.iter().map(OsStr::new).collect();
        todos.extend(relativos.iter().map(|path| path.as_os_str()));
        let vazio = CancellationToken::new();
        self.run_os(&todos, &vazio).await.map(|_| ())
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

impl CliGit {
    /// O mesmo `status` do contrato, chamável de dentro da crate.
    ///
    /// A fusão precisa dele para listar o que ficou em conflito, e chamar o
    /// trait de dentro do próprio tipo pediria o `use` do contrato aqui — o que
    /// faria o adapter depender da forma como ele é consumido.
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
impl WorkingTreeService for CliGit {
    async fn status(&self, cancel: &CancellationToken) -> GitResult<RepositoryStatus> {
        Self::status(self, cancel).await
    }

    async fn diff(
        &self,
        path: &Path,
        side: DiffSide,
        cancel: &CancellationToken,
    ) -> GitResult<FileDiff> {
        let relativo = self.relativo(path);
        let mut args = vec!["--no-optional-locks", "diff", "--no-color", "--unified=3"];
        if side == DiffSide::Index {
            args.push("--cached");
        }
        args.push("--");
        let Some(relativo) = relativo.to_str() else {
            return Err(GitError::Backend {
                detail: "caminho que não é UTF-8".to_owned(),
            });
        };
        args.push(relativo);
        let saida = self.run(&args, cancel).await?;
        let mut diff = ler_diff(&String::from_utf8_lossy(&saida));
        diff.path = path.to_path_buf();
        Ok(diff)
    }

    async fn committed_text(&self, path: &Path, cancel: &CancellationToken) -> GitResult<String> {
        let relativo = self.relativo(path);
        let Some(relativo) = relativo.to_str() else {
            return Err(GitError::Backend {
                detail: "caminho que não é UTF-8".to_owned(),
            });
        };
        // As barras são invertidas de propósito: `git show` fala revisão e
        // caminho no vocabulário dele, e no Windows o separador do sistema não
        // é o que ele espera depois dos dois-pontos.
        let alvo = format!("HEAD:{}", relativo.replace('\\', "/"));
        let saida = self.run(&["--no-optional-locks", "show", &alvo], cancel).await?;
        Ok(String::from_utf8_lossy(&saida).into_owned())
    }

    async fn stage(&self, paths: &[PathBuf]) -> GitResult<()> {
        self.escrever(&["add", "--"], paths).await
    }

    async fn unstage(&self, paths: &[PathBuf]) -> GitResult<()> {
        // `restore --staged` e não `reset HEAD`: num repositório sem commit
        // nenhum, `HEAD` não existe e o segundo falha com uma mensagem sobre
        // revisão desconhecida, que não é o que aconteceu.
        self.escrever(&["restore", "--staged", "--"], paths).await
    }

    async fn discard(&self, paths: &[PathBuf]) -> GitResult<()> {
        self.escrever(&["restore", "--"], paths).await
    }

    async fn stash_list(&self, cancel: &CancellationToken) -> GitResult<Vec<StashEntry>> {
        let saida = self
            .run(
                &[
                    "--no-optional-locks",
                    "stash",
                    "list",
                    "--format=%gd%x1f%gs",
                ],
                cancel,
            )
            .await?;
        Ok(String::from_utf8_lossy(&saida)
            .lines()
            .filter(|linha| !linha.trim().is_empty())
            .enumerate()
            .map(|(indice, linha)| {
                let mensagem = linha.split('\u{1f}').nth(1).unwrap_or(linha);
                StashEntry {
                    // A posição na pilha é a ordem da lista: o `stash@{0}` que o
                    // `git` escreve é a mesma coisa, e lê-la de volta seria
                    // decorar um formato para redescobrir o que já se sabe.
                    index: indice,
                    message: mensagem.trim().to_owned(),
                }
            })
            .collect())
    }

    async fn stash_push(&self, message: &str) -> GitResult<()> {
        let vazio = CancellationToken::new();
        // `--include-untracked` de propósito: quem guarda o trabalho para trocar
        // de branch espera voltar e encontrar tudo, e um arquivo novo que ficou
        // para trás reaparece como surpresa na outra branch.
        let mut args = vec!["stash", "push", "--include-untracked"];
        if !message.trim().is_empty() {
            args.push("--message");
            args.push(message);
        }
        self.run(&args, &vazio).await.map(|_| ())
    }

    async fn stash_pop(&self, index: usize) -> GitResult<()> {
        let vazio = CancellationToken::new();
        let alvo = format!("stash@{{{index}}}");
        self.run(&["stash", "pop", &alvo], &vazio).await.map(|_| ())
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

    async fn switch(&self, branch: &BranchName) -> GitResult<()> {
        let vazio = CancellationToken::new();
        self.run(&["switch", branch.as_str()], &vazio)
            .await
            .map(|_| ())
    }

    async fn create(&self, name: &BranchName) -> GitResult<()> {
        let vazio = CancellationToken::new();
        self.run(&["switch", "--create", name.as_str()], &vazio)
            .await
            .map(|_| ())
    }
}

#[async_trait]
impl crate::tags::TagService for CliGit {
    async fn list(&self, cancel: &CancellationToken) -> GitResult<Vec<String>> {
        let saida = self
            .run(
                &[
                    "for-each-ref",
                    "--sort=refname",
                    "--format=%(refname:short)",
                    "refs/tags/",
                ],
                cancel,
            )
            .await?;
        Ok(String::from_utf8_lossy(&saida)
            .lines()
            .map(str::trim)
            .filter(|linha| !linha.is_empty())
            .map(ToOwned::to_owned)
            .collect())
    }
}

#[async_trait]
impl crate::integration::IntegrationService for CliGit {
    async fn merge(&self, branch: &BranchName) -> GitResult<MergeOutcome> {
        let vazio = CancellationToken::new();
        match self.run(&["merge", "--no-edit", branch.as_str()], &vazio).await {
            Ok(saida) => {
                let texto = String::from_utf8_lossy(&saida);
                Ok(if texto.contains("Already up to date") {
                    MergeOutcome::AlreadyUpToDate
                } else {
                    MergeOutcome::Merged
                })
            }
            // **Conflito não é erro, e quem decide isso é o disco.** O `git`
            // sai com código diferente de zero e escreve o "CONFLICT" na saída
            // padrão, e não na de erro — classificar pelo texto acharia falha
            // da ferramenta onde houve trabalho a fazer. O que responde é o
            // `status`: se há arquivo em conflito, foi conflito.
            Err(erro) => {
                let status = self.status(&vazio).await?;
                let paths: Vec<PathBuf> = status
                    .entries
                    .iter()
                    .filter(|entry| entry.state == FileState::Conflicted)
                    .map(|entry| entry.path.clone())
                    .collect();
                if paths.is_empty() {
                    return Err(erro);
                }
                Ok(MergeOutcome::Conflicted { paths })
            }
        }
    }

    async fn pending(&self, cancel: &CancellationToken) -> GitResult<Option<PendingOperation>> {
        // Lido do disco, e não de memória nossa: quem rodou `git merge` no
        // terminal integrado deixou o repositório assim, e uma IDE que só
        // soubesse das fusões que ela começou mostraria uma tela que não
        // corresponde ao que está lá.
        let saida = self
            .run(&["--no-optional-locks", "rev-parse", "--git-dir"], cancel)
            .await?;
        let git_dir = self
            .root
            .join(String::from_utf8_lossy(&saida).trim());
        for (marca, operacao) in [
            ("MERGE_HEAD", PendingOperation::Merge),
            ("rebase-merge", PendingOperation::Rebase),
            ("rebase-apply", PendingOperation::Rebase),
            ("CHERRY_PICK_HEAD", PendingOperation::CherryPick),
        ] {
            if git_dir.join(marca).exists() {
                return Ok(Some(operacao));
            }
        }
        Ok(None)
    }

    async fn continue_operation(&self) -> GitResult<()> {
        let vazio = CancellationToken::new();
        // `commit --no-edit` e não `merge --continue`: o segundo abre o editor
        // configurado quando não há `GIT_EDITOR`, e um editor externo aberto por
        // dentro da IDE é o processo pendurado que a `22` já teme noutro lugar.
        self.run(&["commit", "--no-edit"], &vazio).await.map(|_| ())
    }

    async fn abort(&self) -> GitResult<()> {
        let vazio = CancellationToken::new();
        self.run(&["merge", "--abort"], &vazio).await.map(|_| ())
    }
}

#[async_trait]
impl crate::history::HistoryService for CliGit {
    async fn log(
        &self,
        pular: usize,
        quantos: usize,
        cancel: &CancellationToken,
    ) -> GitResult<Vec<CommitSummary>> {
        // Os separadores são os do ASCII: unidade (0x1f) entre os campos e
        // registro (0x1e) entre os commits. Mensagem de commit tem quebra de
        // linha e tabulação dentro, e qualquer separador que se possa digitar
        // apareceria no dado — foi o que já obrigou o `status` a usar `-z`.
        let formato = "--format=%H%x1f%P%x1f%an%x1f%ad%x1f%s%x1e";
        let pular = format!("--skip={pular}");
        let quantos = format!("--max-count={quantos}");
        let saida = self
            .run(
                &[
                    "--no-optional-locks",
                    "log",
                    "--date=format:%Y-%m-%d %H:%M",
                    formato,
                    &pular,
                    &quantos,
                ],
                cancel,
            )
            .await?;
        Ok(ler_log(&String::from_utf8_lossy(&saida)))
    }

    async fn commit(&self, message: &str, amend: bool) -> GitResult<CommitId> {
        let vazio = CancellationToken::new();
        let mut args = vec!["commit", "--message", message];
        if amend {
            args.push("--amend");
        }
        self.run(&args, &vazio).await?;
        // O `commit` não devolve o hash; perguntá-lo em seguida é o que dá a
        // quem chamou algo para guardar — e é barato, porque `HEAD` é leitura
        // de referência e não varredura.
        let saida = self.run(&["rev-parse", "HEAD"], &vazio).await?;
        Ok(CommitId(String::from_utf8_lossy(&saida).trim().to_owned()))
    }
}

/// Lê a saída do `log`, separada por unidade e por registro.
fn ler_log(saida: &str) -> Vec<CommitSummary> {
    saida
        .split('\u{1e}')
        .filter(|registro| !registro.trim().is_empty())
        .filter_map(|registro| {
            // O `git` põe uma quebra de linha entre um registro e o próximo;
            // ela vira o começo do registro seguinte, e sem tirá-la o hash
            // chegaria com um `\n` na frente.
            let mut campos = registro.trim_start().split('\u{1f}');
            let id = campos.next()?.trim();
            if id.is_empty() {
                return None;
            }
            let pais = campos.next().unwrap_or_default();
            let autor = campos.next().unwrap_or_default();
            let data = campos.next().unwrap_or_default();
            let resumo = campos.next().unwrap_or_default();
            Some(CommitSummary {
                id: CommitId(id.to_owned()),
                parents: pais
                    .split_whitespace()
                    .map(|pai| CommitId(pai.to_owned()))
                    .collect(),
                author: autor.to_owned(),
                date: data.to_owned(),
                summary: resumo.to_owned(),
            })
        })
        .collect()
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

/// Lê um diff unificado: os cabeçalhos `@@` e as linhas de cada trecho.
///
/// O que **não** se lê é tão importante quanto o que se lê: `diff --git`,
/// `index`, `---` e `+++` são cabeçalho de arquivo, e uma linha começada por
/// `+` dentro deles seria `+++ b/arquivo` virando linha acrescentada. Por isso
/// nada conta antes do primeiro `@@`.
fn ler_diff(saida: &str) -> FileDiff {
    let mut diff = FileDiff::default();
    let mut atual: Option<Hunk> = None;
    let mut proxima_linha = 0usize;
    for linha in saida.lines() {
        if let Some(cabecalho) = linha.strip_prefix("@@") {
            if let Some(hunk) = atual.take() {
                diff.hunks.push(hunk);
            }
            let (velho, novo) = numeros_do_cabecalho(cabecalho);
            proxima_linha = novo;
            atual = Some(Hunk {
                old_start: velho,
                new_start: novo,
                lines: Vec::new(),
            });
            continue;
        }
        let Some(hunk) = atual.as_mut() else {
            continue;
        };
        // "\ No newline at end of file" não é linha de conteúdo.
        if linha.starts_with('\\') {
            continue;
        }
        let (kind, texto) = match linha.as_bytes().first() {
            Some(b'+') => (DiffLineKind::Added, &linha[1..]),
            Some(b'-') => (DiffLineKind::Removed, &linha[1..]),
            Some(b' ') => (DiffLineKind::Context, &linha[1..]),
            // Linha vazia num diff é contexto vazio: o `git` corta o espaço à
            // direita, e descartá-la desalinharia tudo abaixo dela.
            None => (DiffLineKind::Context, ""),
            _ => continue,
        };
        let new_line = (kind != DiffLineKind::Removed).then(|| {
            let numero = proxima_linha;
            proxima_linha += 1;
            numero
        });
        hunk.lines.push(DiffLine {
            kind,
            text: texto.to_owned(),
            new_line,
        });
    }
    if let Some(hunk) = atual {
        diff.hunks.push(hunk);
    }
    diff
}

/// As duas primeiras linhas do cabeçalho `@@ -a,b +c,d @@`, contadas de zero.
fn numeros_do_cabecalho(cabecalho: &str) -> (usize, usize) {
    let mut velho = 0;
    let mut novo = 0;
    for campo in cabecalho.split_whitespace() {
        let numero = |texto: &str| {
            texto
                .split(',')
                .next()
                .and_then(|inicio| inicio.parse::<usize>().ok())
                // O `git` conta a partir de um, e o editor a partir de zero.
                .map_or(0, |valor| valor.saturating_sub(1))
        };
        if let Some(resto) = campo.strip_prefix('-') {
            velho = numero(resto);
        } else if let Some(resto) = campo.strip_prefix('+') {
            novo = numero(resto);
        }
    }
    (velho, novo)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::LineChange;

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

    /// O cabeçalho do arquivo não vira linha acrescentada.
    ///
    /// `+++ b/arquivo` começa com `+`, e um leitor que contasse tudo o tomaria
    /// como conteúdo — deslocando cada marca da margem em uma linha.
    #[test]
    fn o_cabecalho_do_arquivo_nao_entra_no_diff() {
        let saida = concat!(
            "diff --git a/Pedido.java b/Pedido.java\n",
            "index 1111111..2222222 100644\n",
            "--- a/Pedido.java\n",
            "+++ b/Pedido.java\n",
            "@@ -1,3 +1,4 @@\n",
            " class Pedido {\n",
            "+    int total;\n",
            " }\n"
        );
        let diff = ler_diff(saida);
        assert_eq!(diff.hunks.len(), 1);
        assert_eq!(diff.hunks[0].lines.len(), 3, "{:?}", diff.hunks[0].lines);
        assert_eq!(
            diff.changed_lines(),
            vec![(1, LineChange::Added)],
            "a segunda linha do arquivo, contada de zero"
        );
    }

    /// Uma remoção marca a linha que ficou no lugar dela.
    ///
    /// Ela não tem linha própria no arquivo de agora: o que sobrou é a fronteira
    /// entre duas linhas. Sem marcar a de cima, apagar um bloco não deixaria
    /// sinal nenhum na margem.
    #[test]
    fn a_remocao_marca_a_linha_que_ficou() {
        let saida = concat!(
            "@@ -1,4 +1,2 @@\n",
            " class Pedido {\n",
            "-    int total;\n",
            "-    int desconto;\n",
            " }\n"
        );
        let diff = ler_diff(saida);
        assert_eq!(
            diff.changed_lines(),
            vec![(1, LineChange::Removed)],
            "a linha que ficou no lugar do que saiu"
        );
    }

    /// Dois trechos distantes viram dois `Hunk`, e cada um começa onde deve.
    #[test]
    fn cada_trecho_comeca_na_linha_que_o_cabecalho_diz() {
        let saida = concat!(
            "@@ -1,2 +1,3 @@\n",
            " um\n",
            "+dois\n",
            "@@ -40,2 +41,3 @@\n",
            " quarenta\n",
            "+quarenta e um\n"
        );
        let diff = ler_diff(saida);
        assert_eq!(diff.hunks.len(), 2);
        assert_eq!(diff.hunks[1].new_start, 40, "o `git` conta de um, o editor de zero");
        assert_eq!(
            diff.changed_lines(),
            vec![(1, LineChange::Added), (41, LineChange::Added)]
        );
    }

    /// Trocar uma linha por outra é **uma** marca, e não duas.
    ///
    /// É o caso mais comum de todos — editar uma linha —, e contá-lo como uma
    /// remoção mais um acréscimo encheria a margem de sinais onde houve uma
    /// alteração só.
    #[test]
    fn linha_trocada_e_uma_marca_so() {
        let saida = concat!(
            "@@ -1,3 +1,3 @@\n",
            " um\n",
            "-dois\n",
            "+DOIS\n",
            " tres\n"
        );
        let diff = ler_diff(saida);
        assert_eq!(diff.changed_lines(), vec![(1, LineChange::Modified)]);
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
