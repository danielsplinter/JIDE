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

    for titulo in ["Staged (1)", "Alterados (1)", "Não rastreados (1)"] {
        assert!(escrito(titulo), "falta o painel {titulo}");
    }
    for arquivo in ["preparado.java", "alterado.java", "solto.java"] {
        assert!(escrito(arquivo), "falta o arquivo {arquivo}");
    }
    // As ações de cada painel, que não são as mesmas.
    assert!(escrito("Unstage") && escrito("Stage") && escrito("Discard"));

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
    // "Stage" é o primeiro dos dois botões: ele fica antes de "Discard".
    shell.pointer_down(Point::new(direita - 92.0 - 46.0, primeira_linha), size);

    let pedidos: Vec<ApplicationCommand> = shell.commands.iter().cloned().collect();
    assert!(
        pedidos
            .iter()
            .any(|comando| matches!(comando, ApplicationCommand::Git(GitRequest::Stage(caminho)) if caminho.ends_with("alterado.java"))),
        "o clique pede a preparação: {pedidos:?}"
    );
    // **E não pede o retrato junto.** Os dois iam para threads diferentes, e o
    // retrato costumava voltar antes de a escrita terminar: a lista mostrava o
    // estado de antes, e quem preparava um arquivo via ele ficar onde estava.
    // Quem pede o retrato de uma escrita é a aplicação, quando ela recolhe a
    // resposta — aí a ordem é certa por construção.
    assert!(
        !pedidos
            .iter()
            .any(|comando| matches!(comando, ApplicationCommand::Git(GitRequest::Refresh))),
        "o retrato vem depois da escrita, e não ao lado dela: {pedidos:?}"
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

/// O botão `Stage` continua clicável quando o painel ganha barra de rolagem.
///
/// A área da lista encolhe pela trilha da barra, e os botões vão junto — mas a
/// conta que decide de quem é o clique media da borda da **área**, e não do
/// conteúdo. Dez pontos de diferença: o clique na direita do botão caía fora
/// dele.
#[test]
fn o_botao_de_stage_responde_com_a_lista_rolando() {
    let mut shell = test_shell();
    let size = Size::new(1280.0, 800.0);
    let _ = shell.paint(size);
    let raiz = shell.workspace_root().to_path_buf();
    // Arquivos alterados de sobra: é a barra que muda a conta.
    shell.set_git_view(GitView {
        head: Some("main".to_owned()),
        changed: 40,
        modified: 40,
        entries: (0..40)
            .map(|numero| GitEntry {
                path: raiz.join(format!("Arquivo{numero}.java")),
                label: format!("Arquivo{numero}.java"),
                state: GitFileState::Modified,
            })
            .collect(),
        ..GitView::default()
    });
    shell.toggle_git();
    let _ = shell.paint(size);

    // Onde o `Stage` está **desenhado**, que é onde o dedo vai.
    let desenhado = shell.paint(size);
    let onde = desenhado
        .iter()
        .find_map(|comando| match comando {
            PaintCommand::DrawText(texto) if texto.text == "Stage" => Some(texto.origin),
            _ => None,
        });
    if onde.is_none() {
        for comando in &desenhado {
            if let PaintCommand::DrawText(texto) = comando {
                println!("{:?} em {:?}", texto.text, texto.origin);
            }
        }
        panic!("o botão precisa estar na tela");
    }
    let Some(onde) = onde else { return };
    // A moldura do botão, que é o alvo de verdade: o texto fica no meio dela.
    let botao = desenhado
        .iter()
        .filter_map(|comando| match comando {
            PaintCommand::StrokeRect(traco)
                if (traco.rect.origin.y - onde.y).abs() < 12.0
                    && traco.rect.origin.x <= onde.x
                    && traco.rect.origin.x + traco.rect.size.width >= onde.x =>
            {
                Some(traco.rect)
            }
            _ => None,
        })
        .next_back();
    let Some(botao) = botao else {
        panic!("o botão precisa ter moldura");
    };

    // Três pontos do mesmo botão: a borda esquerda, o meio e a direita. Os três
    // são o botão para quem olha, e os três têm de ser o botão para quem clica.
    for (nome, x) in [
        ("borda esquerda", botao.origin.x + 2.0),
        ("meio", botao.origin.x + botao.size.width / 2.0),
        ("borda direita", botao.origin.x + botao.size.width - 2.0),
    ] {
        shell.commands.retain(|_| false);
        shell.pointer_down(Point::new(x, botao.origin.y + botao.size.height / 2.0), size);
        assert!(
            shell.commands.iter().any(|comando| matches!(
                comando,
                ApplicationCommand::Git(GitRequest::Stage(_))
            )),
            "clique na {nome} do botão em x={x}: {:?}",
            shell.commands.iter().collect::<Vec<_>>()
        );
    }
}

/// A comparação abre pelo **nome**, e não por qualquer ponto da linha.
///
/// O nome ocupa o começo da linha; o resto até os botões é espaço vazio.
/// Clicar no vazio abria a comparação de um arquivo que quem clicou talvez nem
/// quisesse ver — e ela toma a aba inteira.
#[test]
fn a_comparacao_abre_pelo_nome_e_nao_pelo_vazio_da_linha() {
    let mut shell = test_shell();
    let size = Size::new(1280.0, 800.0);
    let _ = shell.paint(size);
    let raiz = shell.workspace_root().to_path_buf();
    shell.set_git_view(retrato_com_alteracoes(&raiz));
    shell.toggle_git();
    let desenhado = shell.paint(size);

    // Onde o nome está escrito, que é o alvo.
    let onde = desenhado
        .iter()
        .find_map(|comando| match comando {
            PaintCommand::DrawText(texto) if texto.text == "alterado.java" => Some(texto.origin),
            _ => None,
        });
    let Some(onde) = onde else {
        panic!("o nome precisa estar na tela");
    };

    shell.commands.retain(|_| false);
    shell.pointer_down(Point::new(onde.x + 10.0, onde.y + 6.0), size);
    assert!(
        shell.commands.iter().any(|comando| matches!(
            comando,
            ApplicationCommand::Git(GitRequest::ShowDiff { path, .. })
                if path.ends_with("alterado.java")
        )),
        "clicar no nome abre a comparação: {:?}",
        shell.commands.iter().collect::<Vec<_>>()
    );

    // **A segunda vez também.** Relatado: depois de abrir uma comparação, o
    // clique no vazio voltava a abri-la.
    // E o vazio à direita do nome, antes dos botões, não abre nada.
    let mut superficie = std::mem::take(&mut shell.git);
    let contexto = shell.layout_context();
    let faixas = superficie.faixas_do_status(&shell.host, &contexto);
    shell.git = superficie;
    let vazio = faixas[1].origin.x + faixas[1].size.width / 2.0;
    shell.commands.retain(|_| false);
    shell.pointer_down(Point::new(vazio, onde.y + 6.0), size);
    assert!(
        !shell.commands.iter().any(|comando| matches!(
            comando,
            ApplicationCommand::Git(GitRequest::ShowDiff { .. })
        )),
        "o vazio da linha não abre nada: {:?}",
        shell.commands.iter().collect::<Vec<_>>()
    );
}

/// Com uma comparação já aberta, o vazio da linha continua não abrindo nada.
///
/// Relatado: depois do primeiro clique no nome, clicar fora do nome voltava a
/// mostrar a comparação. A volta inteira importa — a resposta da aplicação põe a
/// janela na aba `Diff`, e é com esse estado que o segundo clique acontece.
#[test]
fn com_a_comparacao_aberta_o_vazio_da_linha_continua_sem_abrir() {
    let mut shell = test_shell();
    let size = Size::new(1280.0, 800.0);
    let _ = shell.paint(size);
    let raiz = shell.workspace_root().to_path_buf();
    shell.set_git_view(retrato_com_alteracoes(&raiz));
    shell.toggle_git();
    let desenhado = shell.paint(size);

    let onde = desenhado
        .iter()
        .find_map(|comando| match comando {
            PaintCommand::DrawText(texto) if texto.text == "alterado.java" => Some(texto.origin),
            _ => None,
        });
    let Some(onde) = onde else {
        panic!("o nome precisa estar na tela");
    };

    // O primeiro clique, e a resposta da aplicação: a janela vai para a `Diff`.
    shell.pointer_down(Point::new(onde.x + 10.0, onde.y + 6.0), size);
    assert!(shell.abrir_comparacao(
        &raiz.join("alterado.java"),
        GitDiff {
            committed: "a
".to_owned(),
            current: "b
".to_owned(),
            ..GitDiff::default()
        },
    ));
    let _ = shell.paint(size);

    // De volta à `Geral`, pela aba.
    let abas = shell.git_surface().abas_da_janela_para_teste(&shell.host);
    shell.pointer_down(
        Point::new(abas.origin.x + abas.size.width / 4.0, abas.origin.y + 8.0),
        size,
    );
    let _ = shell.paint(size);

    // E o clique no vazio da linha: não pede comparação nenhuma.
    let mut superficie = std::mem::take(&mut shell.git);
    let contexto = shell.layout_context();
    let faixas = superficie.faixas_do_status(&shell.host, &contexto);
    shell.git = superficie;
    let vazio = faixas[1].origin.x + faixas[1].size.width / 2.0;
    shell.commands.retain(|_| false);
    shell.pointer_down(Point::new(vazio, onde.y + 6.0), size);
    assert!(
        !shell.commands.iter().any(|comando| matches!(
            comando,
            ApplicationCommand::Git(GitRequest::ShowDiff { .. })
        )),
        "o vazio não abre nada, nem na segunda vez: {:?}",
        shell.commands.iter().collect::<Vec<_>>()
    );
}
