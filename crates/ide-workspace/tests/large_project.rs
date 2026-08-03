//! A busca de conteúdo contra um projeto grande de verdade.
//!
//! Estes testes não trazem projeto embutido: rodam contra o caminho em
//! `IDE_PROJETO_GRANDE`, e são `#[ignore]` porque dependem dele. Rodam com:
//!
//! ```text
//! set IDE_PROJETO_GRANDE=C:\caminho\do\projeto
//! cargo test --release -p ide-workspace --test large_project -- --ignored --nocapture
//! ```
//!
//! Existem porque um projeto Angular com nove mil arquivos travou a IDE. Travar
//! não é coisa que se conclua lendo código — é coisa que se mede, e o número
//! medido é que dirigiu a correção.

use std::time::{Duration, Instant};

use ide_application::SearchScope;
use ide_domain::CancellationToken;
use ide_workspace::WorkspaceService;

fn projeto() -> Option<std::path::PathBuf> {
    let caminho = std::env::var_os("IDE_PROJETO_GRANDE")?;
    let caminho = std::path::PathBuf::from(caminho);
    caminho.is_dir().then_some(caminho)
}

/// Quanto custa uma busca que não acha nada, que é o pior caso.
///
/// **O limite de resultados não protege quem não acha nada.** A busca para quando
/// enche a lista; uma consulta sem ocorrência nenhuma varre o projeto inteiro até
/// o fim.
///
/// Este teste **não** reprova por lentidão: depois da correção a busca roda fora
/// da thread da interface, e demorar lá não trava nada. Ele imprime o número, que
/// é o que justifica a correção — e o número muda uma ordem de grandeza conforme
/// o cache do sistema: medido num projeto de 8 958 arquivos, 1,4 s quente e
/// **106 s frio**. Frio é o estado na primeira busca depois de abrir o projeto,
/// que é exatamente quando ela é pedida.
#[test]
#[ignore = "exige IDE_PROJETO_GRANDE apontando para um projeto grande"]
fn a_search_that_finds_nothing_walks_the_whole_project() {
    let Some(root) = projeto() else {
        panic!("aponte IDE_PROJETO_GRANDE para um projeto grande");
    };
    let service = WorkspaceService::native();
    let Ok(tree) = service.scan(&root) else {
        panic!("a varredura da raiz precisa funcionar");
    };
    let cancel = CancellationToken::new();

    let escopo = SearchScope::new(vec![root.clone()], vec!["ts".to_owned()]);
    let inicio = Instant::now();
    let achados = service.search_content(&tree, &escopo, "zzqqxx-nao-existe", 50, &cancel);
    println!(
        "busca sem ocorrência: {:?} ({} achados)",
        inicio.elapsed(),
        achados.len()
    );
    assert!(achados.is_empty());
}

/// Cancelar interrompe a varredura, e interrompe **rápido**.
///
/// É o que sustenta a correção. A busca saiu para uma thread própria, e sem
/// cancelamento cada tecla numa caixa de busca deixaria mais uma varredura de um
/// minuto rodando no fundo, todas lendo o mesmo disco e disputando entre si.
#[test]
#[ignore = "exige IDE_PROJETO_GRANDE apontando para um projeto grande"]
fn cancelling_stops_the_walk_promptly() {
    let Some(root) = projeto() else {
        panic!("aponte IDE_PROJETO_GRANDE para um projeto grande");
    };
    let service = WorkspaceService::native();
    let Ok(tree) = service.scan(&root) else {
        panic!("a varredura da raiz precisa funcionar");
    };
    let escopo = SearchScope::new(vec![root.clone()], vec!["ts".to_owned()]);

    // Cancelada antes de começar: o caso extremo, e o que prova que ela nem chega
    // a ler arquivo.
    let cancel = CancellationToken::new();
    cancel.cancel();
    let inicio = Instant::now();
    let achados = service.search_content(&tree, &escopo, "component", 50, &cancel);
    let imediato = inicio.elapsed();
    println!("busca já cancelada: {imediato:?} ({} achados)", achados.len());
    assert!(
        achados.is_empty() && imediato < Duration::from_millis(50),
        "cancelada antes de começar, a busca não pode varrer nada: {imediato:?}"
    );

    // E cancelada no meio, por outra thread.
    let cancel = CancellationToken::new();
    let cancelador = cancel.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(200));
        cancelador.cancel();
    });
    let inicio = Instant::now();
    let _ = service.search_content(&tree, &escopo, "zzqqxx-nao-existe", 50, &cancel);
    let interrompida = inicio.elapsed();
    println!("busca cancelada no meio: {interrompida:?}");
    // Um arquivo grande sendo lido no instante do cancelamento é a folga; o que
    // não pode é a varredura seguir até o fim, que sem isto levava mais de um
    // minuto com o cache frio.
    assert!(
        interrompida < Duration::from_secs(5),
        "a desistência precisa chegar perto de onde a varredura está: {interrompida:?}"
    );
}
