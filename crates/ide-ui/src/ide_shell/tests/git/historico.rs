//! A aba `History` e o commit.

use super::*;

/// Abrir a aba `history` pede a primeira página, e a tabela mostra as colunas.
///
/// O histórico é caro e só é pedido quando alguém vai olhá-lo: quem abre o
/// gerenciador para preparar um arquivo não paga por ele.
#[test]
fn a_aba_history_pede_o_historico_e_mostra_a_tabela() {
    let mut shell = test_shell();
    let size = Size::new(1280.0, 800.0);
    let _ = shell.paint(size);
    shell.set_git_view(GitView {
        head: Some("main".to_owned()),
        ..GitView::default()
    });
    shell.toggle_git();
    let _ = shell.paint(size);
    shell.commands.retain(|_| false);

    // Clicar na aba da direita.
    let (_, _, _, abas) = git::GitSurface::areas(&shell.host);
    shell.pointer_down(
        Point::new(
            abas.origin.x + abas.size.width * 0.75,
            abas.origin.y + abas.size.height / 2.0,
        ),
        size,
    );
    assert!(
        shell
            .commands
            .iter()
            .any(|comando| matches!(
                comando,
                ApplicationCommand::Git(GitRequest::LoadHistory { ja_carregados: 0 })
            )),
        "abrir a aba pede a primeira página"
    );

    // Com a resposta, a tabela aparece com cabeçalho e conteúdo.
    shell.set_git_view(GitView {
        head: Some("main".to_owned()),
        commits: vec![
            CommitRow {
                hash: "0123456789abcdef".to_owned(),
                summary: "o commit de cima".to_owned(),
                author: "Teste".to_owned(),
                date: "2026-08-06 19:14".to_owned(),
                lane: 0,
                lanes: 1,
                passing: Vec::new(),
                parents: vec![0],
            },
            CommitRow {
                hash: "fedcba9876543210".to_owned(),
                summary: "o de baixo".to_owned(),
                author: "Outra".to_owned(),
                date: "2026-08-05 10:00".to_owned(),
                lane: 0,
                lanes: 1,
                passing: Vec::new(),
                parents: Vec::new(),
            },
        ],
        ..GitView::default()
    });
    let desenhado = shell.paint(size);
    let escrito = |texto: &str| {
        desenhado
            .iter()
            .any(|comando| matches!(comando, PaintCommand::DrawText(t) if t.text == texto))
    };
    for coluna in ["Nó", "Description", "Date", "Author", "Hash"] {
        assert!(escrito(coluna), "falta a coluna {coluna}");
    }
    assert!(escrito("o commit de cima") && escrito("Teste") && escrito("2026-08-06 19:14"));
    assert!(escrito("0123456"), "o hash aparece abreviado");
    assert!(
        !escrito("0123456789abcdef"),
        "e o inteiro não vai para a tela"
    );
    // O ponto do commit é desenhado pela célula da biblioteca.
    assert!(
        desenhado
            .iter()
            .any(|comando| matches!(comando, PaintCommand::FillCircle(_))),
        "o grafo tem ponto"
    );
}
/// Commitar manda a mensagem, limpa a caixa e recarrega as duas listas.
///
/// **As três coisas juntas.** A mensagem já foi usada, e deixá-la na tela
/// convida a commitar duas vezes o mesmo texto; a lista de alterações ficou
/// vazia e o histórico ganhou uma linha, e nenhuma das duas se descobre sozinha.
#[test]
fn commitar_manda_a_mensagem_e_limpa_a_caixa() {
    let mut shell = test_shell();
    let size = Size::new(1280.0, 800.0);
    let _ = shell.paint(size);
    let raiz = shell.workspace_root().to_path_buf();
    shell.set_git_view(retrato_com_alteracoes(&raiz));
    shell.toggle_git();
    let _ = shell.paint(size);

    // Clicar na caixa da mensagem e escrever.
    let context = shell.layout_context();
    let superficie = std::mem::take(&mut shell.git);
    let faixa = superficie.faixa_do_commit_para_teste(&shell.host);
    shell.git = superficie;
    shell.pointer_down(
        Point::new(faixa.origin.x + 40.0, faixa.origin.y + 10.0),
        size,
    );
    shell.text_input("conserta a margem");
    assert_eq!(shell.git_surface().mensagem(), "conserta a margem");
    let _ = context;
    let _ = shell.paint(size);
    shell.commands.retain(|_| false);

    // E clicar em Commit.
    let superficie = std::mem::take(&mut shell.git);
    let faixa = superficie.faixa_do_commit_para_teste(&shell.host);
    shell.git = superficie;
    let botao = Point::new(
        faixa.origin.x + faixa.size.width - 46.0,
        faixa.origin.y + faixa.size.height - 16.0,
    );
    shell.pointer_down(botao, size);

    let pedidos: Vec<ApplicationCommand> = shell.commands.iter().cloned().collect();
    assert!(
        pedidos.iter().any(|comando| matches!(
            comando,
            ApplicationCommand::Git(GitRequest::Commit { message, amend: false })
                if message == "conserta a margem"
        )),
        "o botão manda a mensagem: {pedidos:?}"
    );
    assert!(
        pedidos
            .iter()
            .any(|comando| matches!(comando, ApplicationCommand::Git(GitRequest::Refresh))),
        "e o retrato de novo"
    );
    assert!(
        pedidos.iter().any(|comando| matches!(
            comando,
            ApplicationCommand::Git(GitRequest::LoadHistory { ja_carregados: 0 })
        )),
        "e o histórico do começo, porque o `amend` reescreve a linha de cima"
    );
    assert!(
        shell.git_surface().mensagem().is_empty(),
        "a caixa esvazia no mesmo gesto"
    );
}
/// Sem mensagem não se commita, e o botão diz isso antes de alguém tentar.
#[test]
fn sem_mensagem_o_commit_nao_acontece() {
    let mut shell = test_shell();
    let size = Size::new(1280.0, 800.0);
    let _ = shell.paint(size);
    let raiz = shell.workspace_root().to_path_buf();
    shell.set_git_view(retrato_com_alteracoes(&raiz));
    shell.toggle_git();
    let _ = shell.paint(size);
    shell.commands.retain(|_| false);

    let superficie = std::mem::take(&mut shell.git);
    let faixa = superficie.faixa_do_commit_para_teste(&shell.host);
    shell.git = superficie;
    shell.pointer_down(
        Point::new(
            faixa.origin.x + faixa.size.width - 46.0,
            faixa.origin.y + faixa.size.height - 16.0,
        ),
        size,
    );
    assert!(
        !shell
            .commands
            .iter()
            .any(|comando| matches!(comando, ApplicationCommand::Git(GitRequest::Commit { .. }))),
        "o clique não vira commit"
    );
}
