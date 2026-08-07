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
        vec![(1, ide_git::LineChange::Added)],
        "a segunda linha, trocada, contada de zero: {:?}",
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

/// O ciclo completo da fase 2: editar, preparar, commitar, e ver no histórico.
///
/// É o critério da fase escrito como teste — "um ciclo completo de trabalho
/// acontece dentro da IDE" —, e ele passa pelo `working_tree` **e** pelo
/// `history`: commitar sem ver o commit aparecer não prova nada.
#[test]
fn editar_preparar_e_commitar_aparece_no_historico() {
    if !ha_git() {
        return;
    }
    let Some(repo) = RepoDeTeste::novo("commit") else {
        panic!("não foi possível criar o repositório de teste");
    };
    repo.escrever("a.txt", "um\n");
    assert!(repo.git(&["add", "."]).is_some());
    assert!(repo.git(&["commit", "-m", "primeiro"]).is_some());

    let Ok(repositorio) = ide_git::open(repo.root()) else {
        panic!("o repositório não abriu");
    };
    let cancel = CancellationToken::new();
    let arvore = repositorio.working_tree();
    let historico = repositorio.history();

    repo.escrever("a.txt", "dois\n");
    assert!(esperar(arvore.stage(std::slice::from_ref(&repo.root().join("a.txt")))).is_ok());
    let Ok(id) = esperar(historico.commit("segundo", false)) else {
        panic!("o commit não aconteceu");
    };
    assert!(!id.0.is_empty(), "o commit devolve o hash dele");

    let Ok(pagina) = esperar(historico.log(0, 10, &cancel)) else {
        panic!("o histórico não respondeu");
    };
    assert_eq!(pagina.len(), 2, "{pagina:?}");
    assert_eq!(pagina[0].summary, "segundo", "o mais recente vem primeiro");
    assert_eq!(pagina[0].id, id);
    assert_eq!(pagina[0].author, "Teste");
    assert!(
        pagina[0].date.len() >= 16,
        "a data vem pronta: {:?}",
        pagina[0].date
    );
    assert_eq!(pagina[0].parents, vec![pagina[1].id.clone()]);

    // E a árvore fica limpa: o que foi commitado saiu da lista.
    let Ok(status) = esperar(arvore.status(&cancel)) else {
        panic!("o status não respondeu");
    };
    assert_eq!(status.changed_files(), 0, "{:?}", status.entries);
}

/// `amend` reescreve o commit anterior em vez de criar outro.
///
/// O teste afirma as duas coisas que distinguem uma coisa da outra: a mensagem
/// muda, e o número de commits **não**.
#[test]
fn amend_reescreve_em_vez_de_acrescentar() {
    if !ha_git() {
        return;
    }
    let Some(repo) = RepoDeTeste::novo("amend") else {
        panic!("não foi possível criar o repositório de teste");
    };
    repo.escrever("a.txt", "um\n");
    assert!(repo.git(&["add", "."]).is_some());
    assert!(repo.git(&["commit", "-m", "mensagem errada"]).is_some());

    let Ok(repositorio) = ide_git::open(repo.root()) else {
        panic!("o repositório não abriu");
    };
    let historico = repositorio.history();
    assert!(esperar(historico.commit("mensagem certa", true)).is_ok());

    let cancel = CancellationToken::new();
    let Ok(pagina) = esperar(historico.log(0, 10, &cancel)) else {
        panic!("o histórico não respondeu");
    };
    assert_eq!(pagina.len(), 1, "continua sendo um commit só");
    assert_eq!(pagina[0].summary, "mensagem certa");
}

/// O histórico vem por páginas, e a página seguinte continua de onde a outra
/// parou.
///
/// Um repositório de verdade tem dezenas de milhares de commits, e carregar
/// todos para mostrar quarenta linhas é o oposto do que a `19` e a `20` fizeram
/// no índice.
#[test]
fn o_historico_vem_por_paginas() {
    if !ha_git() {
        return;
    }
    let Some(repo) = RepoDeTeste::novo("paginas") else {
        panic!("não foi possível criar o repositório de teste");
    };
    for numero in 1..=5 {
        repo.escrever("a.txt", &format!("versão {numero}\n"));
        assert!(repo.git(&["add", "."]).is_some());
        assert!(repo.git(&["commit", "-m", &format!("commit {numero}")]).is_some());
    }

    let Ok(repositorio) = ide_git::open(repo.root()) else {
        panic!("o repositório não abriu");
    };
    let historico = repositorio.history();
    let cancel = CancellationToken::new();
    let Ok(primeira) = esperar(historico.log(0, 2, &cancel)) else {
        panic!("a primeira página não respondeu");
    };
    let Ok(segunda) = esperar(historico.log(2, 2, &cancel)) else {
        panic!("a segunda página não respondeu");
    };
    assert_eq!(primeira.len(), 2);
    assert_eq!(segunda.len(), 2);
    assert_eq!(primeira[0].summary, "commit 5");
    assert_eq!(segunda[0].summary, "commit 3", "a segunda continua de onde a primeira parou");
    assert!(
        primeira.iter().all(|commit| !segunda.contains(commit)),
        "nenhuma linha aparece nas duas páginas"
    );
}

/// Criar e trocar de branch é o gesto que a fase 3 abre.
///
/// Criar **leva junto**: criar e ficar onde estava daria uma branch que existe e
/// não se usa.
#[test]
fn criar_uma_branch_leva_o_trabalho_para_ela() {
    if !ha_git() {
        return;
    }
    let Some(repo) = RepoDeTeste::novo("branch-nova") else {
        panic!("não foi possível criar o repositório de teste");
    };
    repo.escrever("a.txt", "um\n");
    assert!(repo.git(&["add", "."]).is_some());
    assert!(repo.git(&["commit", "-m", "primeiro"]).is_some());

    let Ok(repositorio) = ide_git::open(repo.root()) else {
        panic!("o repositório não abriu");
    };
    let branches = repositorio.branches();
    let cancel = CancellationToken::new();
    let nova = ide_git::BranchName("feature/busca".to_owned());
    assert!(esperar(branches.create(&nova)).is_ok());

    let Ok(status) = esperar(repositorio.working_tree().status(&cancel)) else {
        panic!("o status não respondeu");
    };
    assert_eq!(
        status.head.as_ref().map(Head::label),
        Some("feature/busca".to_owned()),
        "criar leva junto"
    );

    // E voltar é trocar.
    assert!(esperar(branches.switch(&ide_git::BranchName("main".to_owned()))).is_ok());
    let Ok(status) = esperar(repositorio.working_tree().status(&cancel)) else {
        panic!("o status não respondeu");
    };
    assert_eq!(status.head.as_ref().map(Head::label), Some("main".to_owned()));
}

/// Uma fusão que dá conflito não é erro: é trabalho a fazer, e a IDE diz quais
/// arquivos.
///
/// **E dá para sair.** O critério da fase é este: a IDE não fica presa num
/// estado do qual não se sai. Abortar volta ao que era, e o `pending` volta a
/// dizer que não há operação nenhuma.
#[test]
fn um_merge_com_conflito_lista_os_arquivos_e_da_para_abortar() {
    if !ha_git() {
        return;
    }
    let Some(repo) = RepoDeTeste::novo("merge") else {
        panic!("não foi possível criar o repositório de teste");
    };
    repo.escrever("a.txt", "comum\n");
    assert!(repo.git(&["add", "."]).is_some());
    assert!(repo.git(&["commit", "-m", "primeiro"]).is_some());

    // Duas branches mexendo na mesma linha.
    assert!(repo.git(&["switch", "--create", "outra"]).is_some());
    repo.escrever("a.txt", "da outra\n");
    assert!(repo.git(&["commit", "-am", "na outra"]).is_some());
    assert!(repo.git(&["switch", "main"]).is_some());
    repo.escrever("a.txt", "da main\n");
    assert!(repo.git(&["commit", "-am", "na main"]).is_some());

    let Ok(repositorio) = ide_git::open(repo.root()) else {
        panic!("o repositório não abriu");
    };
    let integracao = repositorio.integration();
    let cancel = CancellationToken::new();
    let Ok(resultado) = esperar(integracao.merge(&ide_git::BranchName("outra".to_owned()))) else {
        panic!("a fusão nem respondeu");
    };
    let ide_git::MergeOutcome::Conflicted { paths } = resultado else {
        panic!("a fusão devia ter conflito: {resultado:?}");
    };
    assert_eq!(paths.len(), 1, "{paths:?}");
    assert!(paths[0].ends_with("a.txt"));

    // O estado é lido do disco, e não da nossa memória.
    assert_eq!(
        esperar(integracao.pending(&cancel)).ok().flatten(),
        Some(ide_git::PendingOperation::Merge)
    );
    // E o arquivo aparece no `status` como conflito, que é o que a tela mostra.
    let Ok(status) = esperar(repositorio.working_tree().status(&cancel)) else {
        panic!("o status não respondeu");
    };
    assert_eq!(status.count(ide_git::FileState::Conflicted), 1);

    assert!(esperar(integracao.abort()).is_ok());
    assert_eq!(
        esperar(integracao.pending(&cancel)).ok().flatten(),
        None,
        "abortar tira o repositório do meio da operação"
    );
    assert_eq!(
        std::fs::read_to_string(repo.root().join("a.txt")).unwrap_or_default(),
        "da main\n",
        "e o arquivo volta ao que era"
    );
}

/// Resolver o conflito e continuar fecha a fusão.
///
/// A resolução acontece como edição de texto normal — que é como o conflito já
/// está gravado no arquivo —, e o que a IDE faz é preparar e continuar.
#[test]
fn resolver_e_continuar_fecha_a_fusao() {
    if !ha_git() {
        return;
    }
    let Some(repo) = RepoDeTeste::novo("continuar") else {
        panic!("não foi possível criar o repositório de teste");
    };
    repo.escrever("a.txt", "comum\n");
    assert!(repo.git(&["add", "."]).is_some());
    assert!(repo.git(&["commit", "-m", "primeiro"]).is_some());
    assert!(repo.git(&["switch", "--create", "outra"]).is_some());
    repo.escrever("a.txt", "da outra\n");
    assert!(repo.git(&["commit", "-am", "na outra"]).is_some());
    assert!(repo.git(&["switch", "main"]).is_some());
    repo.escrever("a.txt", "da main\n");
    assert!(repo.git(&["commit", "-am", "na main"]).is_some());

    let Ok(repositorio) = ide_git::open(repo.root()) else {
        panic!("o repositório não abriu");
    };
    let integracao = repositorio.integration();
    let arvore = repositorio.working_tree();
    let cancel = CancellationToken::new();
    let _ = esperar(integracao.merge(&ide_git::BranchName("outra".to_owned())));

    // Quem resolve é quem edita: aqui, o teste faz o papel do editor.
    repo.escrever("a.txt", "resolvido\n");
    assert!(esperar(arvore.stage(std::slice::from_ref(&repo.root().join("a.txt")))).is_ok());
    assert!(esperar(integracao.continue_operation()).is_ok());

    assert_eq!(esperar(integracao.pending(&cancel)).ok().flatten(), None);
    let Ok(status) = esperar(arvore.status(&cancel)) else {
        panic!("o status não respondeu");
    };
    assert_eq!(status.changed_files(), 0, "{:?}", status.entries);
    let Ok(pagina) = esperar(repositorio.history().log(0, 5, &cancel)) else {
        panic!("o histórico não respondeu");
    };
    assert_eq!(
        pagina[0].parents.len(),
        2,
        "o commit de fusão tem dois pais: {:?}",
        pagina[0]
    );
}

/// O `stash` guarda o trabalho e o devolve, e as tags aparecem.
///
/// Os dois juntos porque os dois enchem um nó da árvore que estava vazio desde a
/// fase 0 — e é isso que a fase 3 lhes devia.
#[test]
fn o_stash_guarda_e_devolve_e_as_tags_aparecem() {
    if !ha_git() {
        return;
    }
    let Some(repo) = RepoDeTeste::novo("stash") else {
        panic!("não foi possível criar o repositório de teste");
    };
    repo.escrever("a.txt", "um\n");
    assert!(repo.git(&["add", "."]).is_some());
    assert!(repo.git(&["commit", "-m", "primeiro"]).is_some());
    assert!(repo.git(&["tag", "v1.0"]).is_some());

    let Ok(repositorio) = ide_git::open(repo.root()) else {
        panic!("o repositório não abriu");
    };
    let arvore = repositorio.working_tree();
    let cancel = CancellationToken::new();

    assert_eq!(
        esperar(repositorio.tags().list(&cancel)).unwrap_or_default(),
        vec!["v1.0".to_owned()]
    );

    repo.escrever("a.txt", "mexido\n");
    assert!(esperar(arvore.stash_push("no meio do caminho")).is_ok());
    assert_eq!(
        std::fs::read_to_string(repo.root().join("a.txt")).unwrap_or_default(),
        "um\n",
        "guardar deixa a árvore limpa"
    );
    let guardados = esperar(arvore.stash_list(&cancel)).unwrap_or_default();
    assert_eq!(guardados.len(), 1);
    assert!(
        guardados[0].message.contains("no meio do caminho"),
        "{:?}",
        guardados[0]
    );

    assert!(esperar(arvore.stash_pop(0)).is_ok());
    assert_eq!(
        std::fs::read_to_string(repo.root().join("a.txt")).unwrap_or_default(),
        "mexido\n",
        "devolver traz o trabalho de volta"
    );
    assert!(esperar(arvore.stash_list(&cancel)).unwrap_or_default().is_empty());
}

/// O ciclo com o remoto: empurrar, buscar, e a contagem de commits.
///
/// O "remoto" é um repositório **bare** noutra pasta desta máquina — que é o que
/// o Git chama de remoto sem precisar de rede. Testar com rede seria testar a
/// rede: o que precisa de verificação aqui é a nossa leitura do que o `git`
/// responde.
#[test]
fn empurrar_e_buscar_movem_a_contagem_de_commits() {
    if !ha_git() {
        return;
    }
    let Some(repo) = RepoDeTeste::novo("remoto") else {
        panic!("não foi possível criar o repositório de teste");
    };
    repo.escrever("a.txt", "um\n");
    assert!(repo.git(&["add", "."]).is_some());
    assert!(repo.git(&["commit", "-m", "primeiro"]).is_some());

    // O remoto, ao lado, e a ligação entre os dois.
    let remoto = repo.root().with_extension("git");
    let _ = std::fs::remove_dir_all(&remoto);
    let Some(caminho) = remoto.to_str() else {
        panic!("caminho do remoto ilegível");
    };
    assert!(repo.git(&["init", "--bare", caminho]).is_some());
    assert!(repo.git(&["remote", "add", "origin", caminho]).is_some());

    let Ok(repositorio) = ide_git::open(repo.root()) else {
        panic!("o repositório não abriu");
    };
    let remotos = repositorio.remotes();
    let branches = repositorio.branches();
    let cancel = CancellationToken::new();

    assert_eq!(
        esperar(remotos.list(&cancel)).unwrap_or_default(),
        vec![ide_git::RemoteName("origin".to_owned())]
    );

    // Empurrar leva a branch para lá — e a primeira vez precisa dizer para onde.
    assert!(repo.git(&["push", "--set-upstream", "origin", "main"]).is_some());
    assert!(
        esperar(remotos.remote_branches(&cancel))
            .unwrap_or_default()
            .contains(&ide_git::BranchName("origin/main".to_owned())),
        "a branch remota aparece depois do push"
    );

    // Um commit a mais aqui, e a contagem passa a dizer que estamos à frente.
    repo.escrever("a.txt", "dois\n");
    assert!(repo.git(&["commit", "-am", "segundo"]).is_some());
    let locais = esperar(branches.local(&cancel)).unwrap_or_default();
    let Some(main) = locais.iter().find(|branch| branch.name.0 == "main") else {
        panic!("a branch main precisa estar na lista: {locais:?}");
    };
    assert_eq!(
        (main.ahead, main.behind),
        (1, 0),
        "um commit à frente do que já foi buscado: {main:?}"
    );
    assert_eq!(
        main.upstream,
        Some(ide_git::BranchName("origin/main".to_owned()))
    );

    // E depois de empurrar, ela volta a zero.
    assert!(esperar(remotos.push(false)).is_ok());
    assert!(esperar(remotos.fetch()).is_ok());
    let locais = esperar(branches.local(&cancel)).unwrap_or_default();
    let Some(main) = locais.iter().find(|branch| branch.name.0 == "main") else {
        panic!("a branch main precisa estar na lista");
    };
    assert_eq!((main.ahead, main.behind), (0, 0), "{main:?}");

    let _ = std::fs::remove_dir_all(&remoto);
}
