//! A aba `Status`: os três painéis, as ações de linha e o estado intermediário.

use super::*;

/// A aba `status` empilha três painéis, e cada um mostra os arquivos dele.
///
/// A divisão dos três não é da tela: é a que o `--porcelain=v2` devolve, e a que
/// decide o que cada ação faz na linha. Um painel só, com tudo junto, obrigaria
/// quem olha a descobrir o estado de cada arquivo pelo ícone.
#[test]
fn a_aba_status_empilha_os_tres_paineis() {
    let mut shell = test_shell();
    let size = Size::new(1280.0, 800.0);
    let _ = shell.paint(size);
    let raiz = shell.workspace_root().to_path_buf();
    shell.set_git_view(retrato_com_alteracoes(&raiz));
    shell.toggle_git();
    let desenhado = shell.paint(size);
    let escrito = |texto: &str| {
        desenhado
            .iter()
            .any(|comando| matches!(comando, PaintCommand::DrawText(t) if t.text == texto))
    };

    for titulo in ["Preparados (1)", "Alterados (1)", "Não rastreados (1)"] {
        assert!(escrito(titulo), "falta o painel {titulo}");
    }
    for arquivo in ["preparado.java", "alterado.java", "solto.java"] {
        assert!(escrito(arquivo), "falta o arquivo {arquivo}");
    }
    // As ações de cada painel, que não são as mesmas.
    assert!(escrito("Despreparar") && escrito("Preparar") && escrito("Descartar"));

    // Os três ficam **um embaixo do outro**, e nessa ordem.
    let context = shell.layout_context();
    let mut superficie = std::mem::take(&mut shell.git);
    let faixas = superficie.faixas_do_status(&shell.host, &context);
    shell.git = superficie;
    assert!(
        faixas[0].origin.y < faixas[1].origin.y && faixas[1].origin.y < faixas[2].origin.y,
        "empilhados de cima para baixo: {faixas:?}"
    );
}
/// A ação de uma linha vira pedido, e o pedido vem com o retrato atrás.
///
/// **Os dois juntos, sempre.** Preparar um arquivo e deixar a lista como estava
/// faria quem preparou ver a linha continuar em "alterados" — e desfazer o que
/// acabou de fazer. É o critério da fase 1 escrito como teste.
#[test]
fn preparar_uma_linha_pede_a_escrita_e_o_retrato_de_novo() {
    let mut shell = test_shell();
    let size = Size::new(1280.0, 800.0);
    let _ = shell.paint(size);
    let raiz = shell.workspace_root().to_path_buf();
    shell.set_git_view(retrato_com_alteracoes(&raiz));
    shell.toggle_git();
    let _ = shell.paint(size);
    shell.commands.retain(|_| false);

    // O botão da direita do painel do meio: preparar o arquivo alterado.
    let context = shell.layout_context();
    let mut superficie = std::mem::take(&mut shell.git);
    let faixas = superficie.faixas_do_status(&shell.host, &context);
    shell.git = superficie;
    let faixa = faixas[1];
    let direita = faixa.origin.x + faixa.size.width;
    let primeira_linha = faixa.origin.y + 20.0 + 12.0;
    // "Preparar" é o primeiro dos dois botões: ele fica antes de "Descartar".
    shell.pointer_down(Point::new(direita - 92.0 - 46.0, primeira_linha), size);

    let pedidos: Vec<ApplicationCommand> = shell.commands.iter().cloned().collect();
    assert!(
        pedidos
            .iter()
            .any(|comando| matches!(comando, ApplicationCommand::Git(GitRequest::Stage(caminho)) if caminho.ends_with("alterado.java"))),
        "o clique pede a preparação: {pedidos:?}"
    );
    assert!(
        pedidos
            .iter()
            .any(|comando| matches!(comando, ApplicationCommand::Git(GitRequest::Refresh))),
        "e pede o retrato de novo, senão a lista fica velha: {pedidos:?}"
    );
}
/// Com uma operação no meio do caminho, a saída está sempre na tela.
///
/// É o critério da fase: a IDE não fica presa num estado do qual não se sai.
/// E **continuar** só aparece habilitado quando não há mais conflito — o `git`
/// recusaria, e a recusa chegaria como falha da ferramenta.
#[test]
fn o_estado_intermediario_mostra_por_onde_sair() {
    let mut shell = test_shell();
    let size = Size::new(1280.0, 800.0);
    let _ = shell.paint(size);
    let raiz = shell.workspace_root().to_path_buf();
    shell.set_git_view(GitView {
        head: Some("main".to_owned()),
        changed: 1,
        pending: Some("Merge".to_owned()),
        entries: vec![GitEntry {
            path: raiz.join("Pedido.java"),
            label: "Pedido.java".to_owned(),
            state: GitFileState::Conflicted,
        }],
        ..GitView::default()
    });
    shell.toggle_git();
    let desenhado = shell.paint(size);
    assert!(
        desenhado.iter().any(|comando| {
            matches!(comando, PaintCommand::DrawText(t) if t.text.contains("Merge em curso"))
        }),
        "a faixa diz qual operação está no meio do caminho"
    );
    assert!(
        desenhado
            .iter()
            .any(|comando| matches!(comando, PaintCommand::DrawText(t) if t.text == "Abortar")),
        "e por onde sair"
    );

    // Abortar é o botão da direita.
    let superficie = std::mem::take(&mut shell.git);
    let faixa = superficie.faixa_do_conflito_para_teste(&shell.host);
    shell.git = superficie;
    let Some(faixa) = faixa else {
        panic!("a faixa do conflito precisa existir");
    };
    shell.commands.retain(|_| false);
    shell.pointer_down(
        Point::new(
            faixa.origin.x + faixa.size.width - 46.0,
            faixa.origin.y + 12.0,
        ),
        size,
    );
    assert!(
        shell
            .commands
            .iter()
            .any(|comando| matches!(comando, ApplicationCommand::Git(GitRequest::AbortOperation))),
        "o clique aborta"
    );

    // Sem operação em curso, a faixa não existe: ela não é decoração fixa.
    shell.set_git_view(GitView {
        head: Some("main".to_owned()),
        ..GitView::default()
    });
    let _ = shell.paint(size);
    let superficie = std::mem::take(&mut shell.git);
    let faixa = superficie.faixa_do_conflito_para_teste(&shell.host);
    shell.git = superficie;
    assert!(faixa.is_none());
}
