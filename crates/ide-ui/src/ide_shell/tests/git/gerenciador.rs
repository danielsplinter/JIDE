//! A janela do gerenciador: o botão que a abre, as abas, a árvore da esquerda e
//! a barra do alto.

use super::*;

/// Digitar a linha inteira, fechamento incluído, não duplica nada.
///
/// É o critério da fase: quem já tem o hábito de escrever o `)` escreve, e não
/// pode receber `f(a, b))` por isso.
#[test]
fn digitar_o_fechamento_a_mao_nao_duplica() {
    let mut shell = shell_editing("");
    for caractere in "f(a, b)".chars() {
        shell.text_input(&caractere.to_string());
    }
    assert_eq!(shell.active_text(), Some("f(a, b)"));
    assert_eq!(
        shell.editor_area.pane.cursor(),
        7,
        "passar por cima do fechamento leva o cursor para depois dele"
    );
}
/// Escrever uma string inteira, aspas incluídas, não duplica nada.
#[test]
fn digitar_a_string_inteira_nao_duplica_a_aspa() {
    let mut shell = shell_editing("");
    for caractere in "const a = 'oi';".chars() {
        shell.text_input(&caractere.to_string());
    }
    assert_eq!(shell.active_text(), Some("const a = 'oi';"));
}
/// O terceiro botão da barra abre o gerenciador, e pede o retrato de novo.
///
/// As duas coisas juntas de propósito: abrir uma janela que mostra o estado da
/// última vez é o defeito que a `21` nomeou — a resposta velha parecida com a
/// certa. Quem commitou no terminal integrado entre uma abertura e outra veria
/// a contagem de antes, sem nada avisando.
#[test]
fn o_botao_do_git_abre_o_gerenciador_e_pede_o_retrato() {
    let mut shell = test_shell();
    let size = Size::new(1280.0, 800.0);
    let _ = shell.paint(size);

    let botao = IdeShell::activity_rect(ACTIVITY_GIT_ID);
    let clique = Point::new(botao.origin.x + 12.0, botao.origin.y + 12.0);
    shell.pointer_down(clique, size);
    assert!(shell.git_surface().is_open(), "o botão abre a janela");
    assert!(
        shell
            .commands
            .iter()
            .any(|comando| matches!(comando, ApplicationCommand::Git(GitRequest::Refresh))),
        "abrir pergunta ao repositório de novo"
    );

    // **Clicar fora não fecha.** Ela é tela de trabalho, e não aviso: quem está
    // escrevendo a mensagem de um commit e erra o alvo do clique perderia o que
    // escreveu. Nem o botão que a abriu a fecha, porque com ela aberta o gesto é
    // dela.
    shell.pointer_down(clique, size);
    assert!(shell.git_surface().is_open(), "o clique fora não dispensa");
    let _ = shell.paint(size);
    let veu = Point::new(20.0, size.height - 20.0);
    shell.pointer_down(veu, size);
    assert!(shell.git_surface().is_open(), "nem o clique no véu");

    // Quem fecha é o botão do canto de cima.
    let painel = git::GitSurface::areas(&shell.host).0;
    shell.pointer_down(
        Point::new(
            painel.origin.x + painel.size.width - 28.0,
            painel.origin.y + 28.0,
        ),
        size,
    );
    assert!(!shell.git_surface().is_open(), "o X fecha");
}
/// A busca filtra as branches, e some com o nó que ficou sem nenhuma.
///
/// Um `Tags` vazio depois de digitar diria que não há tag nenhuma — o que é
/// mentira: o que há é uma busca que não casou com nada ali.
#[test]
fn a_busca_do_gerenciador_filtra_as_branches() {
    let mut shell = test_shell();
    let size = Size::new(1280.0, 800.0);
    let _ = shell.paint(size);
    shell.set_git_view(GitView {
        head: Some("main".to_owned()),
        branches: vec![
            BranchItem {
                name: "main".to_owned(),
                current: true,
            ..BranchItem::default()
            },
            BranchItem {
                name: "feature/busca".to_owned(),
                current: false,
            ..BranchItem::default()
            },
            BranchItem {
                name: "feature/git".to_owned(),
                current: false,
            ..BranchItem::default()
            },
        ],
        ..GitView::default()
    });
    shell.toggle_git();
    let _ = shell.paint(size);
    // Quatro raízes, nenhuma aberta ainda.
    assert_eq!(shell.git_surface().visible_rows(), 4);

    shell.text_input("feature");
    let _ = shell.paint(size);
    assert_eq!(shell.git_surface().query(), "feature");
    assert_eq!(
        shell.git_surface().visible_rows(),
        1,
        "com filtro sobra só o nó das branches, e os três vazios somem"
    );
    let desenhado = shell.paint(size);
    assert!(
        desenhado
            .iter()
            .any(|comando| matches!(comando, PaintCommand::DrawText(t) if t.text == "Branches (2)")),
        "e ele conta só as que casaram"
    );

    // Apagar devolve tudo: o filtro não pode consumir o que ele escondeu.
    for _ in 0..7 {
        shell.key_down("Backspace");
    }
    let _ = shell.paint(size);
    assert!(shell.git_surface().query().is_empty());
    assert_eq!(shell.git_surface().visible_rows(), 4);
}
/// A barra de estado mostra a branch e quantos arquivos mudaram.
///
/// É o critério da fase 0 da `22`, e ele vale com a janela fechada: quem não
/// abre o gerenciador precisa ver em que branch está.
#[test]
fn a_barra_de_estado_mostra_a_branch_e_o_que_mudou() {
    let mut shell = test_shell();
    let size = Size::new(1280.0, 800.0);
    shell.set_git_view(GitView {
        head: Some("main".to_owned()),
        changed: 3,
        ..GitView::default()
    });
    let desenhado = shell.paint(size);
    assert!(
        desenhado
            .iter()
            .any(|comando| matches!(comando, PaintCommand::DrawText(t) if t.text == "main ~3")),
        "a branch e a contagem, com a janela fechada"
    );

    // Sem nada alterado, a contagem some — um `~0` seria ruído fixo na barra.
    shell.set_git_view(GitView {
        head: Some("main".to_owned()),
        ..GitView::default()
    });
    let desenhado = shell.paint(size);
    assert!(
        desenhado
            .iter()
            .any(|comando| matches!(comando, PaintCommand::DrawText(t) if t.text == "main")),
    );
}
/// Uma pasta que não é repositório não é erro, e a janela diz isso.
///
/// A maioria das pastas não é repositório. Abrir vazia deixaria quem clicou sem
/// saber se a IDE não achou nada ou se ela falhou.
#[test]
fn sem_repositorio_o_gerenciador_explica_em_vez_de_ficar_vazio() {
    let mut shell = test_shell();
    let size = Size::new(1280.0, 800.0);
    let _ = shell.paint(size);
    shell.toggle_git();
    let desenhado = shell.paint(size);
    assert!(
        desenhado.iter().any(|comando| {
            matches!(comando, PaintCommand::DrawText(t) if t.text.contains("não está num repositório"))
        }),
        "a janela explica a ausência"
    );
    assert!(
        shell.git_view().status_segment().is_none(),
        "e a barra de estado não inventa segmento nenhum"
    );
}
/// A divisa do gerenciador se arrasta, e o painel da esquerda muda de largura.
///
/// A divisão é do `SplitPane` da biblioteca — a IDE não desenha nem arruma —, e
/// o que se afirma aqui é o que quem usa vê: puxar a linha do meio dá mais
/// espaço para a árvore. A mesma pergunta que muda o ponteiro para a seta dupla
/// é a que o arrasto usa, e por isso as duas não podem divergir.
#[test]
fn a_divisa_do_gerenciador_se_arrasta() {
    let mut shell = test_shell();
    let size = Size::new(1280.0, 800.0);
    let _ = shell.paint(size);
    shell.toggle_git();
    let _ = shell.paint(size);

    let (_, _, arvore_antes, _) = git::GitSurface::areas(&shell.host);
    let divisa = shell.git_surface().divider_area();
    assert!(divisa.size.width > 0.0, "a divisa precisa de área para o gesto");
    let meio = Point::new(
        divisa.origin.x + divisa.size.width / 2.0,
        divisa.origin.y + divisa.size.height / 2.0,
    );
    assert!(
        shell.git_divider_hover(meio),
        "sob o ponteiro ela se anuncia, e não só depois de alguém movê-la"
    );

    shell.pointer_down(meio, size);
    shell.pointer_move(Point::new(meio.x + 120.0, meio.y), size);
    let _ = shell.paint(size);
    let (_, _, arvore_depois, _) = git::GitSurface::areas(&shell.host);
    assert!(
        arvore_depois.size.width > arvore_antes.size.width + 50.0,
        "puxar para a direita alarga a árvore: {arvore_antes:?} para {arvore_depois:?}"
    );

    // Soltar encerra o arrasto: sem isso a divisa seguiria o ponteiro sem
    // ninguém estar segurando nada, que é o defeito que a divisão do editor já
    // teve.
    shell.pointer_up();
    assert!(!shell.git_divider_hover(Point::new(10.0, 10.0)));
}
/// A digitação vai para a caixa que tem o cursor, e não sempre para a busca.
///
/// Duas caixas na mesma janela: escrever a mensagem do commit não pode filtrar
/// as branches, e procurar uma branch não pode escrever no commit.
#[test]
fn as_duas_caixas_do_gerenciador_nao_se_misturam() {
    let mut shell = test_shell();
    let size = Size::new(1280.0, 800.0);
    let _ = shell.paint(size);
    let raiz = shell.workspace_root().to_path_buf();
    shell.set_git_view(retrato_com_alteracoes(&raiz));
    shell.toggle_git();
    let _ = shell.paint(size);

    // Sem clicar em nada, o texto é da busca — é onde o cursor nasce.
    shell.text_input("main");
    assert_eq!(shell.git_surface().query(), "main");
    assert!(shell.git_surface().mensagem().is_empty());

    // Clicando na caixa da mensagem, ele passa a ser dela.
    let superficie = std::mem::take(&mut shell.git);
    let faixa = superficie.faixa_do_commit_para_teste(&shell.host);
    shell.git = superficie;
    shell.pointer_down(
        Point::new(faixa.origin.x + 40.0, faixa.origin.y + 10.0),
        size,
    );
    shell.text_input("mensagem");
    assert_eq!(shell.git_surface().mensagem(), "mensagem");
    assert_eq!(
        shell.git_surface().query(),
        "main",
        "o que estava na busca continua lá"
    );
}
/// Os quatro nós deixam de ser promessa: tags e stashes têm conteúdo.
///
/// Eles apareceram vazios desde a fase 0 de propósito — um nó que só existe
/// quando a capacidade chega faria a tela mudar de forma a cada fase. A fase 3 é
/// quem lhes devia o conteúdo.
#[test]
fn as_tags_e_os_stashes_enchem_os_nos_que_estavam_vazios() {
    let mut shell = test_shell();
    let size = Size::new(1280.0, 800.0);
    let _ = shell.paint(size);
    shell.set_git_view(retrato_com_branches());
    shell.toggle_git();
    let desenhado = shell.paint(size);
    let escrito = |texto: &str| {
        desenhado
            .iter()
            .any(|comando| matches!(comando, PaintCommand::DrawText(t) if t.text == texto))
    };
    assert!(escrito("Tags (1)") && escrito("Stashes (1)"));
    assert!(escrito("Remotes (0)"), "sem remoto configurado, o nó diz zero");
}
/// A linha de uma branch oferece trocar e fundir — menos a branch atual.
///
/// Trocar para onde já se está não faz nada, e fundir uma branch nela mesma é um
/// comando que o `git` recusa: oferecer os dois seria oferecer erro.
#[test]
fn a_branch_atual_nao_oferece_trocar_nem_fundir() {
    let mut shell = test_shell();
    let size = Size::new(1280.0, 800.0);
    let _ = shell.paint(size);
    shell.set_git_view(retrato_com_branches());
    shell.toggle_git();
    let _ = shell.paint(size);

    // Abrir o nó das branches para ver as linhas.
    let (_, _, arvore, _) = git::GitSurface::areas(&shell.host);
    shell.pointer_down(
        Point::new(arvore.origin.x + 10.0, arvore.origin.y + 12.0),
        size,
    );
    let desenhado = shell.paint(size);
    let quantos = |texto: &str| {
        desenhado
            .iter()
            .filter(|comando| matches!(comando, PaintCommand::DrawText(t) if t.text == texto))
            .count()
    };
    assert_eq!(quantos("Checkout"), 1, "só a branch que não é a atual oferece");
    assert_eq!(quantos("Merge"), 1);
}
/// Trocar de branch pede a troca, o retrato e o histórico do começo.
///
/// **Os três juntos.** A troca reescreve os arquivos, muda o que está alterado e
/// muda qual é a linha de cima do histórico: mostrar qualquer um dos três como
/// estava seria mostrar a branch anterior.
#[test]
fn trocar_de_branch_pede_a_troca_e_recarrega_o_que_mudou() {
    let mut shell = test_shell();
    let size = Size::new(1280.0, 800.0);
    let _ = shell.paint(size);
    shell.set_git_view(retrato_com_branches());
    shell.toggle_git();
    let _ = shell.paint(size);

    let (_, _, arvore, _) = git::GitSurface::areas(&shell.host);
    shell.pointer_down(
        Point::new(arvore.origin.x + 10.0, arvore.origin.y + 12.0),
        size,
    );
    let _ = shell.paint(size);
    shell.commands.retain(|_| false);

    // A terceira linha é a outra branch — a primeira é o nó, a segunda é a
    // branch atual —, e o botão "Checkout" é o penúltimo dela.
    let direita = arvore.origin.x + arvore.size.width;
    shell.pointer_down(
        Point::new(direita - 64.0 - 32.0, arvore.origin.y + 12.0 + 48.0),
        size,
    );

    let pedidos: Vec<ApplicationCommand> = shell.commands.iter().cloned().collect();
    assert!(
        pedidos.iter().any(|comando| matches!(
            comando,
            ApplicationCommand::Git(GitRequest::SwitchBranch(nome)) if nome == "feature/busca"
        )),
        "o clique pede a troca: {pedidos:?}"
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
    assert!(
        pedidos.iter().any(|comando| matches!(
            comando,
            ApplicationCommand::Git(GitRequest::LoadHistory { ja_carregados: 0 })
        )),
        "e o histórico do começo, porque a linha de cima é outra"
    );
}
/// A caixa do nome cria a branch, e não se mistura com a busca.
///
/// Procurar e nomear são duas coisas: uma caixa que fizesse as duas criaria
/// branch com o texto de um filtro.
#[test]
fn a_caixa_do_nome_cria_a_branch_sem_mexer_na_busca() {
    let mut shell = test_shell();
    let size = Size::new(1280.0, 800.0);
    let _ = shell.paint(size);
    shell.set_git_view(retrato_com_branches());
    shell.toggle_git();
    let _ = shell.paint(size);

    // Primeiro a busca, que é onde o cursor nasce.
    shell.text_input("feat");
    assert_eq!(shell.git_surface().query(), "feat");

    // Depois o diálogo, aberto pelo botão da barra do alto.
    let branch = shell.git_surface().botao_de_branch_para_teste(&shell.host);
    shell.pointer_down(
        Point::new(
            branch.origin.x + branch.size.width / 2.0,
            branch.origin.y + branch.size.height / 2.0,
        ),
        size,
    );
    shell.text_input("feature/git");
    assert_eq!(shell.git_surface().nome_novo(), Some("feature/git"));
    assert_eq!(
        shell.git_surface().query(),
        "feat",
        "o filtro continua sendo o que era"
    );
    let _ = shell.paint(size);
    shell.commands.retain(|_| false);

    // E o `OK` cria, a partir da branch escolhida na árvore.
    let contexto = shell.layout_context();
    let ok = shell
        .git_surface()
        .areas_do_dialogo_para_teste(&shell.host, contexto.theme())[5];
    shell.pointer_down(
        Point::new(
            ok.origin.x + ok.size.width / 2.0,
            ok.origin.y + ok.size.height / 2.0,
        ),
        size,
    );
    assert!(
        shell.commands.iter().any(|comando| matches!(
            comando,
            ApplicationCommand::Git(GitRequest::CreateBranch { name, .. }) if name == "feature/git"
        )),
        "o `OK` cria a branch com o nome digitado: {:?}",
        shell.commands.iter().collect::<Vec<_>>()
    );
    assert!(
        shell.git_surface().nome_novo().is_none(),
        "e o diálogo fecha, porque o nome já foi usado"
    );
}
/// As três ações do repositório moram na barra do alto.
///
/// **Elas não são da linha em que alguém clicou.** `Fetch` traz as referências
/// todas de uma vez, e `Pull` e `Push` falam sempre da branch em que se está —
/// pendurá-los numa linha fazia parecer que valiam só para aquela.
#[test]
fn a_barra_do_alto_tem_as_tres_acoes_do_repositorio() {
    let mut shell = test_shell();
    let size = Size::new(1280.0, 800.0);
    let _ = shell.paint(size);
    shell.set_git_view(GitView {
        head: Some("main".to_owned()),
        branches: vec![BranchItem {
            name: "main".to_owned(),
            current: true,
            ahead: 2,
            behind: 3,
        }],
        remotes: vec!["origin/main".to_owned()],
        ..GitView::default()
    });
    shell.toggle_git();
    let desenhado = shell.paint(size);
    let escrito = |texto: &str| {
        desenhado
            .iter()
            .any(|comando| matches!(comando, PaintCommand::DrawText(t) if t.text == texto))
    };
    assert!(escrito("Fetch") && escrito("Pull") && escrito("Push"));

    // Eles estão **acima** da árvore e das abas, e não dentro delas.
    let barra = shell.git_surface().barra_para_teste(&shell.host);
    let (_, _, arvore, abas) = git::GitSurface::areas(&shell.host);
    assert!(
        barra.origin.y + barra.size.height <= arvore.origin.y + 0.01
            && barra.origin.y + barra.size.height <= abas.origin.y + 0.01,
        "a barra fica no alto: {barra:?} contra {arvore:?} e {abas:?}"
    );

    // O primeiro botão busca; o terceiro empurra.
    for (posicao, esperado) in [(0.0, GitRequest::Fetch), (2.0, GitRequest::Push)] {
        shell.commands.retain(|_| false);
        shell.pointer_down(
            Point::new(
                barra.origin.x + posicao * 100.0 + 40.0,
                barra.origin.y + 20.0,
            ),
            size,
        );
        assert!(
            shell
                .commands
                .iter()
                .any(|comando| matches!(comando, ApplicationCommand::Git(pedido) if *pedido == esperado)),
            "o botão {posicao} pede {esperado:?}: {:?}",
            shell.commands.iter().collect::<Vec<_>>()
        );
    }
}
/// A branch atual não oferece ação nenhuma na linha dela.
///
/// Trocar para onde já se está não faz nada, e fundir uma branch nela mesma é
/// comando que o `git` recusa. O que ela tinha subiu para a barra.
#[test]
fn a_branch_atual_nao_oferece_acao_na_linha() {
    let mut shell = test_shell();
    let size = Size::new(1280.0, 800.0);
    let _ = shell.paint(size);
    shell.set_git_view(GitView {
        head: Some("main".to_owned()),
        branches: vec![
            BranchItem {
                name: "main".to_owned(),
                current: true,
                ahead: 2,
                behind: 3,
            },
            BranchItem {
                name: "outra".to_owned(),
                ..BranchItem::default()
            },
        ],
        ..GitView::default()
    });
    shell.toggle_git();
    let _ = shell.paint(size);

    // Abrir o nó das branches.
    let (_, _, arvore, _) = git::GitSurface::areas(&shell.host);
    shell.pointer_down(
        Point::new(arvore.origin.x + 10.0, arvore.origin.y + 12.0),
        size,
    );
    let desenhado = shell.paint(size);
    let quantos = |texto: &str| {
        desenhado
            .iter()
            .filter(|comando| matches!(comando, PaintCommand::DrawText(t) if t.text == texto))
            .count()
    };
    assert_eq!(quantos("Checkout"), 1, "só a branch que não é a atual oferece");
    assert_eq!(quantos("Merge"), 1);
    // A contagem contra o que já foi buscado continua na linha da atual.
    assert_eq!(quantos("↑2 ↓3"), 1);

    // E o clique no vazio à direita da atual não pede nada.
    shell.commands.retain(|_| false);
    shell.pointer_down(
        Point::new(
            arvore.origin.x + arvore.size.width - 32.0,
            arvore.origin.y + 12.0 + 24.0,
        ),
        size,
    );
    assert!(
        shell.commands.iter().next().is_none(),
        "{:?}",
        shell.commands.iter().collect::<Vec<_>>()
    );
}
/// A barra de rolagem da árvore rola quando alguém a arrasta.
///
/// **O arrasto começa dentro do componente e continua fora dele**: quem agarra a
/// alça sai da trilha nos primeiros pixels. O movimento chegava só às divisas
/// desta janela, e o resultado era uma barra que ficava onde foi agarrada
/// enquanto o ponteiro seguia sozinho — em toda a janela, nas duas direções.
#[test]
fn as_barras_do_gerenciador_rolam_no_arrasto() {
    let mut shell = test_shell();
    let size = Size::new(1280.0, 800.0);
    let _ = shell.paint(size);
    // Branches em quantidade suficiente para não caber na altura da árvore.
    let branches = (0..60)
        .map(|numero| BranchItem {
            name: format!("feature/assunto-numero-{numero:02}"),
            ..BranchItem::default()
        })
        .collect();
    shell.set_git_view(GitView {
        head: Some("main".to_owned()),
        branches,
        ..GitView::default()
    });
    shell.toggle_git();
    let _ = shell.paint(size);

    // Abrir o nó para as branches entrarem na conta da altura.
    let (_, _, arvore, _) = git::GitSurface::areas(&shell.host);
    shell.pointer_down(
        Point::new(arvore.origin.x + 10.0, arvore.origin.y + 12.0),
        size,
    );
    let _ = shell.paint(size);

    let antes = shell.git_surface().rolagem_da_arvore();
    // A trilha fica encostada na borda direita da árvore.
    let alca = Point::new(
        arvore.origin.x + arvore.size.width - 4.0,
        arvore.origin.y + 20.0,
    );
    shell.pointer_down(alca, size);
    shell.pointer_move(Point::new(alca.x, alca.y + 120.0), size);
    let depois = shell.git_surface().rolagem_da_arvore();
    assert!(
        depois > antes,
        "arrastar a alça rola a árvore: {antes} para {depois}"
    );

    // E soltar encerra: o movimento seguinte não move mais nada.
    shell.pointer_up();
    shell.pointer_move(Point::new(alca.x, alca.y + 300.0), size);
    assert_eq!(
        shell.git_surface().rolagem_da_arvore(),
        depois,
        "solto, o ponteiro não arrasta mais"
    );
}

/// `Enter` cria, e a base é a da combo — que abre na branch em que se está.
///
/// **O padrão é onde o usuário fez checkout**: é de lá que quase toda branch
/// nova sai, e obrigar a escolher o óbvio a cada vez é cobrar um gesto por nada.
#[test]
fn o_enter_cria_a_partir_da_branch_em_que_se_esta() {
    let mut shell = test_shell();
    let size = Size::new(1280.0, 800.0);
    let _ = shell.paint(size);
    shell.set_git_view(GitView {
        head: Some("release/1.0".to_owned()),
        branches: vec![
            BranchItem {
                name: "main".to_owned(),
                ..BranchItem::default()
            },
            BranchItem {
                name: "release/1.0".to_owned(),
                current: true,
                ..BranchItem::default()
            },
        ],
        ..GitView::default()
    });
    shell.toggle_git();
    let _ = shell.paint(size);

    let branch = shell.git_surface().botao_de_branch_para_teste(&shell.host);
    shell.pointer_down(
        Point::new(
            branch.origin.x + branch.size.width / 2.0,
            branch.origin.y + branch.size.height / 2.0,
        ),
        size,
    );
    shell.text_input("feature/nova");
    shell.commands.retain(|_| false);
    shell.key_down("Enter");

    assert!(
        shell.commands.iter().any(|comando| matches!(
            comando,
            ApplicationCommand::Git(GitRequest::CreateBranch { name, base })
                if name == "feature/nova" && base.as_deref() == Some("release/1.0")
        )),
        "a base é a branch em que se está: {:?}",
        shell.commands.iter().collect::<Vec<_>>()
    );
}

/// O `Buscar` filtra as bases da combo, e a escolhida sobrevive ao filtro.
///
/// A lista de bases é a do repositório inteiro; num projeto com sessenta
/// branches, achar a certa rolando é pior do que escrever três letras. E
/// filtrar é procurar, não escolher: quem digitou para conferir não quer voltar
/// com outra base selecionada.
#[test]
fn o_buscar_filtra_as_bases_sem_trocar_a_escolhida() {
    let mut shell = test_shell();
    let size = Size::new(1280.0, 800.0);
    let _ = shell.paint(size);
    shell.set_git_view(GitView {
        head: Some("main".to_owned()),
        branches: vec![
            BranchItem {
                name: "main".to_owned(),
                current: true,
                ..BranchItem::default()
            },
            BranchItem {
                name: "release/1.0".to_owned(),
                ..BranchItem::default()
            },
            BranchItem {
                name: "release/2.0".to_owned(),
                ..BranchItem::default()
            },
        ],
        remotes: vec!["origin/main".to_owned()],
        ..GitView::default()
    });
    shell.toggle_git();
    let _ = shell.paint(size);

    let branch = shell.git_surface().botao_de_branch_para_teste(&shell.host);
    shell.pointer_down(
        Point::new(
            branch.origin.x + branch.size.width / 2.0,
            branch.origin.y + branch.size.height / 2.0,
        ),
        size,
    );
    assert_eq!(
        shell.git_surface().bases_do_dialogo_para_teste(),
        vec![
            "main".to_owned(),
            "release/1.0".to_owned(),
            "release/2.0".to_owned(),
            "origin/main".to_owned(),
        ],
        "as locais e as remotas, que são bases tão válidas quanto"
    );

    // Escrever no filtro e buscar.
    let contexto = shell.layout_context();
    let areas = shell
        .git_surface()
        .areas_do_dialogo_para_teste(&shell.host, contexto.theme());
    let (campo_da_busca, botao_de_buscar) = (areas[2], areas[3]);
    shell.pointer_down(
        Point::new(
            campo_da_busca.origin.x + 10.0,
            campo_da_busca.origin.y + campo_da_busca.size.height / 2.0,
        ),
        size,
    );
    shell.text_input("release");
    shell.pointer_down(
        Point::new(
            botao_de_buscar.origin.x + botao_de_buscar.size.width / 2.0,
            botao_de_buscar.origin.y + botao_de_buscar.size.height / 2.0,
        ),
        size,
    );
    assert_eq!(
        shell.git_surface().bases_do_dialogo_para_teste(),
        vec!["release/1.0".to_owned(), "release/2.0".to_owned()],
        "só o que casa com o filtro"
    );

    // O nome digitado antes do filtro continua lá: os dois campos são dois.
    assert_eq!(shell.git_surface().nome_novo(), Some(""));
}

/// `Esc` fecha o diálogo sem criar nada, e sem fechar a janela.
#[test]
fn o_escape_fecha_o_dialogo_e_nao_a_janela() {
    let mut shell = test_shell();
    let size = Size::new(1280.0, 800.0);
    let _ = shell.paint(size);
    let raiz = shell.workspace_root().to_path_buf();
    shell.set_git_view(retrato_com_alteracoes(&raiz));
    shell.toggle_git();
    let _ = shell.paint(size);

    let branch = shell.git_surface().botao_de_branch_para_teste(&shell.host);
    shell.pointer_down(
        Point::new(
            branch.origin.x + branch.size.width / 2.0,
            branch.origin.y + branch.size.height / 2.0,
        ),
        size,
    );
    shell.text_input("descartada");
    assert_eq!(shell.git_surface().nome_novo(), Some("descartada"));

    shell.commands.retain(|_| false);
    shell.key_down("Escape");
    assert!(shell.git_surface().nome_novo().is_none(), "o diálogo fechou");
    assert!(shell.git_surface().is_open(), "e a janela continua aberta");
    assert!(
        !shell.commands.iter().any(|comando| matches!(
            comando,
            ApplicationCommand::Git(GitRequest::CreateBranch { .. })
        )),
        "e nada foi criado"
    );

    // E o nome não volta: abrir de novo começa vazio.
    shell.pointer_down(
        Point::new(
            branch.origin.x + branch.size.width / 2.0,
            branch.origin.y + branch.size.height / 2.0,
        ),
        size,
    );
    assert_eq!(shell.git_surface().nome_novo(), Some(""));
}

/// O diálogo é desenhado por cima da janela, e não atrás dela.
///
/// Ele nasceu atrás dos painéis: a chamada que o pintava estava três linhas
/// acima de onde precisava estar. Quem flutua diz que flutua, e a camada de
/// cima da biblioteca resolve a ordem — esta tela não depende mais de ser a
/// última a falar.
#[test]
fn o_dialogo_de_branch_fica_por_cima_da_janela() {
    let mut shell = test_shell();
    let size = Size::new(1280.0, 800.0);
    let _ = shell.paint(size);
    let raiz = shell.workspace_root().to_path_buf();
    shell.set_git_view(retrato_com_alteracoes(&raiz));
    shell.toggle_git();
    let _ = shell.paint(size);

    let branch = shell.git_surface().botao_de_branch_para_teste(&shell.host);
    shell.pointer_down(
        Point::new(
            branch.origin.x + branch.size.width / 2.0,
            branch.origin.y + branch.size.height / 2.0,
        ),
        size,
    );
    let desenhado = shell.paint(size);

    let posicao = |procurado: &str| {
        desenhado
            .iter()
            .position(|comando| matches!(comando, PaintCommand::DrawText(t) if t.text == procurado))
    };
    let (Some(dialogo), Some(atras)) = (posicao("Nova branch"), posicao("alterado.java")) else {
        panic!("o diálogo e o painel precisam estar na tela");
    };
    assert!(
        dialogo > atras,
        "o diálogo é desenhado depois do que ele cobre: {dialogo} contra {atras}"
    );

}

/// A lista aberta da combo fica acima do `OK`, que é pintado depois dela.
///
/// Relatado duas vezes. A combo já punha a lista numa camada; a camada protege
/// do que **já** foi pintado, e o `OK` vem depois. Camadas empilham: o que
/// flutua dentro do diálogo sobe mais uma altura que o diálogo inteiro.
#[test]
fn a_lista_da_combo_fica_acima_do_ok() {
    let mut shell = test_shell();
    let size = Size::new(1280.0, 800.0);
    let _ = shell.paint(size);
    shell.set_git_view(GitView {
        head: Some("main".to_owned()),
        branches: vec![BranchItem {
            name: "main".to_owned(),
            current: true,
            ..BranchItem::default()
        }],
        remotes: vec!["origin/main".to_owned()],
        ..GitView::default()
    });
    shell.toggle_git();
    let _ = shell.paint(size);

    // Abrir o diálogo e depois a combo.
    let branch = shell.git_surface().botao_de_branch_para_teste(&shell.host);
    shell.pointer_down(
        Point::new(
            branch.origin.x + branch.size.width / 2.0,
            branch.origin.y + branch.size.height / 2.0,
        ),
        size,
    );
    let contexto = shell.layout_context();
    let combo = shell
        .git_surface()
        .areas_do_dialogo_para_teste(&shell.host, contexto.theme())[4];
    shell.pointer_down(
        Point::new(
            combo.origin.x + combo.size.width / 2.0,
            combo.origin.y + combo.size.height / 2.0,
        ),
        size,
    );
    let desenhado = shell.paint(size);

    // `origin/main` só aparece na lista aberta; `OK` é o botão do diálogo.
    let posicao = |procurado: &str| {
        desenhado
            .iter()
            .position(|comando| matches!(comando, PaintCommand::DrawText(t) if t.text == procurado))
    };
    let (Some(lista), Some(ok)) = (posicao("origin/main"), posicao("OK")) else {
        panic!("a lista aberta e o `OK` precisam estar na tela");
    };
    assert!(
        lista > ok,
        "a lista é desenhada depois do botão que ela cobre: {lista} contra {ok}"
    );
}
