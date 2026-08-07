//! Os testes rodam contra repositório de verdade, criado por `git init`.
//!
//! É o que a `22` exige, e o motivo está lá: repositório falso testaria o nosso
//! próprio simulacro, e é justamente a tradução da saída real do `git` que
//! precisa ser verificada — ela muda entre versões, e a nossa leitura dela é a
//! parte que pode errar calada.
//!
//! **Nenhum teste daqui nomeia o adapter.** Eles são escritos contra os traits,
//! e é isso que os torna a prova da regra: no dia em que um segundo adapter
//! existir, esta bateria roda contra ele sem uma linha mudada.

use std::path::{Path, PathBuf};
use std::process::Command;

use ide_domain::CancellationToken;
use ide_git::{FileState, Head};

/// Espera a resposta do Git numa linha de execução com runtime próprio.
///
/// O adapter fala com um processo, e processo precisa do reator do tokio: sem
/// ele a chamada não bloqueia — ela entra em pânico. Quem usa a crate monta o
/// runtime, como a aplicação já faz com as ferramentas de build.
fn esperar<T>(futuro: impl std::future::Future<Output = T>) -> T {
    let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    else {
        panic!("o runtime de teste não subiu");
    };
    runtime.block_on(futuro)
}

/// Um repositório temporário, apagado no fim.
struct RepoDeTeste {
    root: PathBuf,
}

impl RepoDeTeste {
    /// Cria a pasta e roda `git init`, com identidade própria.
    ///
    /// A identidade é dada aqui de propósito: uma máquina sem `user.email`
    /// configurado faria o `commit` falhar, e o teste acusaria a nossa leitura
    /// por um defeito que é da máquina.
    fn novo(nome: &str) -> Option<Self> {
        let root = std::env::temp_dir().join(format!("er-git-{nome}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).ok()?;
        let repo = Self { root };
        repo.git(&["init", "--initial-branch=main"])?;
        repo.git(&["config", "user.email", "teste@exemplo"])?;
        repo.git(&["config", "user.name", "Teste"])?;
        // Sem isto, o `checkout` de uma máquina com `core.autocrlf=true` devolve
        // o arquivo com CRLF, e o teste acusaria a nossa leitura por uma
        // conversão que é configuração de quem está rodando. O fim de linha é um
        // risco real desta especificação, e o lugar de tratá-lo é o produto —
        // não o teste que mede outra coisa.
        repo.git(&["config", "core.autocrlf", "false"])?;
        Some(repo)
    }

    fn git(&self, args: &[&str]) -> Option<String> {
        let mut comando = Command::new("git");
        comando.args(args).current_dir(&self.root).env("LC_ALL", "C");
        ide_process::sem_janela_de_console_sincrono(&mut comando);
        let saida = comando.output().ok()?;
        saida
            .status
            .success()
            .then(|| String::from_utf8_lossy(&saida.stdout).into_owned())
    }

    fn escrever(&self, nome: &str, conteudo: &str) {
        let _ = std::fs::write(self.root.join(nome), conteudo);
    }

    fn root(&self) -> &Path {
        &self.root
    }
}

impl Drop for RepoDeTeste {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// Se não há `git` na máquina, não há o que testar aqui.
///
/// Devolver "passou" é honesto: a IDE degrada sem a ferramenta, e é isso que o
/// teste `sem_git_a_ide_nao_quebra` afirma. O que não se pode é falhar e mandar
/// alguém procurar defeito onde não há.
fn ha_git() -> bool {
    let mut comando = Command::new("git");
    comando.arg("--version");
    ide_process::sem_janela_de_console_sincrono(&mut comando);
    comando.output().is_ok_and(|saida| saida.status.success())
}

#[test]
fn um_projeto_versionado_diz_a_branch_e_o_que_mudou() {
    if !ha_git() {
        return;
    }
    let Some(repo) = RepoDeTeste::novo("status") else {
        panic!("não foi possível criar o repositório de teste");
    };
    repo.escrever("a.txt", "um\n");
    assert!(repo.git(&["add", "a.txt"]).is_some());
    assert!(repo.git(&["commit", "-m", "primeiro"]).is_some());

    // Um arquivo preparado, um alterado e um que o Git ainda não conhece.
    repo.escrever("a.txt", "dois\n");
    repo.escrever("b.txt", "novo\n");
    assert!(repo.git(&["add", "b.txt"]).is_some());
    repo.escrever("c.txt", "solto\n");

    let Ok(repositorio) = ide_git::open(repo.root()) else {
        panic!("o repositório não abriu");
    };
    let cancel = CancellationToken::new();
    let Ok(status) = esperar(repositorio.working_tree().status(&cancel)) else {
        panic!("o status não respondeu");
    };

    assert_eq!(
        status.head.as_ref().map(Head::label),
        Some("main".to_owned())
    );
    assert_eq!(status.count(FileState::Staged), 1, "{:?}", status.entries);
    assert_eq!(status.count(FileState::Modified), 1, "{:?}", status.entries);
    assert_eq!(status.count(FileState::Untracked), 1, "{:?}", status.entries);
    assert_eq!(status.changed_files(), 3);

    // Os caminhos são absolutos: quem os recebe abre arquivo com eles.
    assert!(
        status
            .entries
            .iter()
            .all(|entry| entry.path.starts_with(repo.root())),
        "{:?}",
        status.entries
    );
}

#[test]
fn as_branches_locais_vem_com_a_atual_marcada() {
    if !ha_git() {
        return;
    }
    let Some(repo) = RepoDeTeste::novo("branches") else {
        panic!("não foi possível criar o repositório de teste");
    };
    repo.escrever("a.txt", "um\n");
    assert!(repo.git(&["add", "a.txt"]).is_some());
    assert!(repo.git(&["commit", "-m", "primeiro"]).is_some());
    assert!(repo.git(&["branch", "feature/busca"]).is_some());

    let Ok(repositorio) = ide_git::open(repo.root()) else {
        panic!("o repositório não abriu");
    };
    let cancel = CancellationToken::new();
    let Ok(branches) = esperar(repositorio.branches().local(&cancel)) else {
        panic!("as branches não responderam");
    };

    let nomes: Vec<String> = branches
        .iter()
        .map(|branch| branch.name.0.clone())
        .collect();
    assert_eq!(nomes, vec!["feature/busca".to_owned(), "main".to_owned()]);
    assert_eq!(
        branches
            .iter()
            .filter(|branch| branch.current)
            .map(|branch| branch.name.0.clone())
            .collect::<Vec<_>>(),
        vec!["main".to_owned()],
        "só a atual leva marca"
    );
}

#[test]
fn uma_pasta_dentro_do_repositorio_encontra_a_raiz() {
    if !ha_git() {
        return;
    }
    let Some(repo) = RepoDeTeste::novo("descoberta") else {
        panic!("não foi possível criar o repositório de teste");
    };
    let dentro = repo.root().join("src").join("main");
    assert!(std::fs::create_dir_all(&dentro).is_ok());

    // A raiz vem da pasta de dentro, e é a de cima. É o caso normal: quem abre
    // a IDE num subprojeto está dentro do repositório, e não na raiz dele.
    let achada = ide_git::discover(&dentro);
    assert_eq!(
        achada.as_deref().and_then(Path::file_name),
        repo.root().file_name(),
        "{achada:?}"
    );
}

#[test]
fn uma_pasta_sem_git_nao_e_repositorio_e_nao_falha() {
    let pasta = std::env::temp_dir().join(format!("er-git-nada-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&pasta);
    assert!(std::fs::create_dir_all(&pasta).is_ok());

    assert_eq!(ide_git::discover(&pasta), None);
    assert!(
        matches!(
            ide_git::open(&pasta),
            Err(ide_git::GitError::NotARepository)
        ),
        "abrir o que não é repositório responde, e não explode"
    );
    let _ = std::fs::remove_dir_all(&pasta);
}

/// O ciclo da fase 1: ver o que mudou, preparar um, descartar outro.
///
/// É o critério da fase escrito como teste, e ele é um só de propósito: preparar
/// e descartar em testes separados não afirmaria o que importa — que **a lista
/// não fica velha depois de cada ação**. Quem prepara e vê o arquivo continuar
/// em "alterados" desfaz o que acabou de fazer.
#[test]
fn preparar_e_descartar_mudam_o_que_o_status_responde() {
    if !ha_git() {
        return;
    }
    let Some(repo) = RepoDeTeste::novo("escritas") else {
        panic!("não foi possível criar o repositório de teste");
    };
    repo.escrever("a.txt", "um
dois
tres
");
    repo.escrever("b.txt", "b
");
    assert!(repo.git(&["add", "."]).is_some());
    assert!(repo.git(&["commit", "-m", "primeiro"]).is_some());

    repo.escrever("a.txt", "um
DOIS
tres
");
    repo.escrever("b.txt", "b alterado
");

    let Ok(repositorio) = ide_git::open(repo.root()) else {
        panic!("o repositório não abriu");
    };
    let arvore = repositorio.working_tree();
    let cancel = CancellationToken::new();
    let caminho_a = repo.root().join("a.txt");
    let caminho_b = repo.root().join("b.txt");

    // A diferença de `a.txt` diz qual linha mudou.
    let Ok(diff) = esperar(arvore.diff(&caminho_a, ide_git::DiffSide::WorkingTree, &cancel)) else {
        panic!("a diferença não respondeu");
    };
    assert_eq!(
        diff.changed_lines(),
        vec![(1, ide_git::LineChange::Modified)],
        "a segunda linha trocada, contada de zero: {:?}",
        diff.hunks
    );

    // Preparar `a.txt` o tira de "alterados" e o põe em "preparados".
    assert!(esperar(arvore.stage(std::slice::from_ref(&caminho_a))).is_ok());
    let Ok(status) = esperar(arvore.status(&cancel)) else {
        panic!("o status não respondeu");
    };
    assert_eq!(status.count(ide_git::FileState::Staged), 1);
    assert!(
        status
            .entries
            .iter()
            .any(|entry| entry.path == caminho_a && entry.state == ide_git::FileState::Staged),
        "{:?}",
        status.entries
    );

    // Despreparar o devolve para "alterados", sem tocar no arquivo.
    assert!(esperar(arvore.unstage(std::slice::from_ref(&caminho_a))).is_ok());
    let Ok(status) = esperar(arvore.status(&cancel)) else {
        panic!("o status não respondeu");
    };
    assert_eq!(status.count(ide_git::FileState::Staged), 0);
    assert_eq!(
        std::fs::read_to_string(&caminho_a).unwrap_or_default(),
        "um
DOIS
tres
",
        "despreparar não mexe no que está escrito"
    );

    // Descartar `b.txt` devolve o arquivo ao que estava commitado.
    assert!(esperar(arvore.discard(std::slice::from_ref(&caminho_b))).is_ok());
    assert_eq!(
        std::fs::read_to_string(&caminho_b).unwrap_or_default(),
        "b
",
        "descartar traz de volta o conteúdo do commit"
    );
    let Ok(status) = esperar(arvore.status(&cancel)) else {
        panic!("o status não respondeu");
    };
    assert_eq!(status.changed_files(), 1, "sobrou só o a.txt: {:?}", status.entries);
}

/// O conteúdo commitado vem como texto, para o lado esquerdo da comparação.
///
/// Como texto e não como caminho: o arquivo de então não existe no disco, e
/// materializá-lo num temporário daria a quem abrisse uma cópia editável do
/// passado — que salva por cima de nada, e some sem avisar.
#[test]
fn o_conteudo_commitado_vem_como_texto() {
    if !ha_git() {
        return;
    }
    let Some(repo) = RepoDeTeste::novo("commitado") else {
        panic!("não foi possível criar o repositório de teste");
    };
    repo.escrever("a.txt", "antes
");
    assert!(repo.git(&["add", "."]).is_some());
    assert!(repo.git(&["commit", "-m", "primeiro"]).is_some());
    repo.escrever("a.txt", "depois
");

    let Ok(repositorio) = ide_git::open(repo.root()) else {
        panic!("o repositório não abriu");
    };
    let cancel = CancellationToken::new();
    let Ok(texto) = esperar(
        repositorio
            .working_tree()
            .committed_text(&repo.root().join("a.txt"), &cancel),
    ) else {
        panic!("o conteúdo commitado não respondeu");
    };
    assert_eq!(texto, "antes
");
}

/// Descartar não alcança o que o Git não rastreia.
///
/// Seria apagar o arquivo do disco, e não há de onde trazê-lo de volta. O `git`
/// responde sem erro e sem fazer nada, e é isso que se afirma aqui: a IDE não
/// pode achar que descartou.
#[test]
fn descartar_nao_apaga_arquivo_nao_rastreado() {
    if !ha_git() {
        return;
    }
    let Some(repo) = RepoDeTeste::novo("solto") else {
        panic!("não foi possível criar o repositório de teste");
    };
    repo.escrever("a.txt", "um
");
    assert!(repo.git(&["add", "."]).is_some());
    assert!(repo.git(&["commit", "-m", "primeiro"]).is_some());
    repo.escrever("solto.txt", "nunca foi commitado
");

    let Ok(repositorio) = ide_git::open(repo.root()) else {
        panic!("o repositório não abriu");
    };
    let solto = repo.root().join("solto.txt");
    let _ = esperar(repositorio.working_tree().discard(std::slice::from_ref(&solto)));
    assert!(solto.exists(), "o arquivo continua no disco");
}
