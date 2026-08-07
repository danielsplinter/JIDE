//! Testes do **gerenciador de Git**. Ver a `22`.

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

    // O mesmo botão fecha: sem isso, quem clicou de novo fica sem saída a não
    // ser o Esc.
    shell.pointer_down(clique, size);
    assert!(!shell.git_surface().is_open());
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
/// Clicar no nome do arquivo abre a comparação, e não uma ação.
///
/// A coluna decide qual das três coisas o clique foi. Sem essa separação, quem
/// quisesse ver o que mudou acabaria preparando o arquivo por engano — e
/// preparar por engano é o começo de um commit que ninguém revisou.
#[test]
fn clicar_no_nome_do_arquivo_pede_a_comparacao() {
    let mut shell = test_shell();
    let size = Size::new(1280.0, 800.0);
    let _ = shell.paint(size);
    let raiz = shell.workspace_root().to_path_buf();
    shell.set_git_view(retrato_com_alteracoes(&raiz));
    shell.toggle_git();
    let _ = shell.paint(size);
    shell.commands.retain(|_| false);

    let context = shell.layout_context();
    let mut superficie = std::mem::take(&mut shell.git);
    let faixas = superficie.faixas_do_status(&shell.host, &context);
    shell.git = superficie;
    let faixa = faixas[1];
    shell.pointer_down(
        Point::new(faixa.origin.x + 30.0, faixa.origin.y + 20.0 + 12.0),
        size,
    );

    let pedidos: Vec<ApplicationCommand> = shell.commands.iter().cloned().collect();
    assert!(
        pedidos.iter().any(|comando| matches!(
            comando,
            ApplicationCommand::Git(GitRequest::ShowDiff { path, staged: false })
                if path.ends_with("alterado.java")
        )),
        "o nome abre a diferença: {pedidos:?}"
    );
    assert!(
        !pedidos
            .iter()
            .any(|comando| matches!(comando, ApplicationCommand::Git(GitRequest::Stage(_)))),
        "e não prepara nada"
    );
}
/// A margem do editor mostra o que mudou desde o commit.
///
/// E **não sobrepõe** o que já está marcado: um ponto de parada numa linha
/// alterada continua sendo um ponto de parada, que é o que a pessoa pôs ali. A
/// marca de versão é informação de fundo.
#[test]
fn a_margem_marca_as_linhas_que_o_git_diz_que_mudaram() {
    let root = std::env::temp_dir().join(format!("er-ide-margem-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    assert!(std::fs::create_dir_all(&root).is_ok());
    let arquivo = root.join("Pedido.java");
    assert!(std::fs::write(&arquivo, "class Pedido {\n  int total;\n}\n").is_ok());
    let Ok(mut shell) = IdeShell::open(&root) else {
        panic!("workspace de teste não abriu");
    };
    let Ok(_) = shell.open_file(&arquivo) else {
        panic!("arquivo de teste não abriu");
    };
    shell.set_git_line_marks(
        arquivo.clone(),
        vec![(1, GitLineChange::Modified), (2, GitLineChange::Added)],
    );

    let decoracoes = shell.editor_decorations(&arquivo);
    assert_eq!(
        decoracoes
            .iter()
            .filter_map(|item| item.mark.map(|mark| (item.line, mark)))
            .collect::<Vec<_>>(),
        vec![
            (1, GutterMark::LineModified),
            (2, GutterMark::LineAdded)
        ]
    );

    // O ponto de parada ganha da marca de versão na mesma linha.
    shell.toggle_breakpoint(&arquivo, 1);
    let decoracoes = shell.editor_decorations(&arquivo);
    assert_eq!(
        decoracoes
            .iter()
            .find(|item| item.line == 1)
            .and_then(|item| item.mark),
        Some(GutterMark::PendingBreakpoint),
        "o ponto de parada continua visível"
    );

    // Retrato limpo apaga as marcas: commitar deixa o arquivo igual ao commit, e
    // uma margem riscada estaria contando o trabalho de antes.
    shell.set_git_line_marks(arquivo.clone(), Vec::new());
    assert!(
        shell
            .editor_decorations(&arquivo)
            .iter()
            .all(|item| item.mark != Some(GutterMark::LineAdded)),
    );
    let _ = std::fs::remove_dir_all(&root);
}
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
    assert_eq!(quantos("Trocar"), 1, "só a branch que não é a atual oferece");
    assert_eq!(quantos("Fundir"), 1);
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
    // branch atual —, e o botão "Trocar" é o penúltimo dela.
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
    assert!(
        pedidos
            .iter()
            .any(|comando| matches!(comando, ApplicationCommand::Git(GitRequest::Refresh))),
        "e o retrato"
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

    // Depois a caixa do nome, embaixo da árvore.
    let (_, _, arvore, _) = git::GitSurface::areas(&shell.host);
    let faixa_do_nome = Point::new(
        arvore.origin.x + 20.0,
        arvore.origin.y + arvore.size.height + 12.0,
    );
    shell.pointer_down(faixa_do_nome, size);
    shell.text_input("feature/git");
    assert_eq!(shell.git_surface().nome_novo(), "feature/git");
    assert_eq!(
        shell.git_surface().query(),
        "feat",
        "o filtro continua sendo o que era"
    );
    let _ = shell.paint(size);
    shell.commands.retain(|_| false);

    // E o botão cria.
    let (_, _, arvore, _) = git::GitSurface::areas(&shell.host);
    shell.pointer_down(
        Point::new(
            arvore.origin.x + arvore.size.width - 40.0,
            arvore.origin.y + arvore.size.height + 12.0,
        ),
        size,
    );
    assert!(
        shell.commands.iter().any(|comando| matches!(
            comando,
            ApplicationCommand::Git(GitRequest::CreateBranch(nome)) if nome == "feature/git"
        )),
        "o botão cria a branch com o nome digitado"
    );
    assert!(
        shell.git_surface().nome_novo().is_empty(),
        "e a caixa esvazia, porque o nome já foi usado"
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
    assert_eq!(quantos("Trocar"), 1, "só a branch que não é a atual oferece");
    assert_eq!(quantos("Fundir"), 1);
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

/// A descrição de um commit comprido quebra, e a linha da tabela cresce.
///
/// **Quem decide quebrar é a IDE**, e só nesta coluna: data, autor e hash cabem
/// sempre, e a descrição é a única em que o texto sumia por baixo da coluna
/// vizinha. A altura vem junto — quebrar sem crescer poria a segunda linha fora
/// da célula.
#[test]
fn a_descricao_comprida_quebra_e_a_linha_cresce() {
    let mut shell = test_shell();
    let size = Size::new(1280.0, 800.0);
    let _ = shell.paint(size);
    let commit = |hash: &str, resumo: &str| CommitRow {
        hash: hash.to_owned(),
        summary: resumo.to_owned(),
        author: "Teste".to_owned(),
        date: "2026-08-07 10:00".to_owned(),
        lane: 0,
        lanes: 1,
        passing: Vec::new(),
        parents: vec![0],
    };
    shell.set_git_view(GitView {
        head: Some("main".to_owned()),
        commits: vec![
            commit("aaa1111", "curto"),
            commit(
                "bbb2222",
                "uma mensagem de commit bem comprida, dessas que explicam o porquê da                  mudança inteira numa linha só e não cabem na coluna de jeito nenhum",
            ),
        ],
        ..GitView::default()
    });
    shell.toggle_git();
    // O quadro entre abrir e clicar: sem ele a moldura da janela ainda não foi
    // arranjada, e a faixa de abas não tem área para receber o gesto.
    let _ = shell.paint(size);
    // A aba da direita é a do histórico.
    let (_, _, _, abas) = git::GitSurface::areas(&shell.host);
    shell.pointer_down(
        Point::new(
            abas.origin.x + abas.size.width * 0.75,
            abas.origin.y + abas.size.height / 2.0,
        ),
        size,
    );
    let desenhado = shell.paint(size);

    // O texto da descrição sai em mais de um pedaço, e nenhum deles é a
    // mensagem inteira.
    let pedacos: Vec<&str> = desenhado
        .iter()
        .filter_map(|comando| match comando {
            PaintCommand::DrawText(texto) if texto.text.contains("mensagem")
                || texto.text.contains("mudança") =>
            {
                Some(texto.text.as_str())
            }
            _ => None,
        })
        .collect();
    assert!(
        pedacos.len() > 1,
        "a descrição desce para a linha seguinte: {pedacos:?}"
    );

    // E a linha de baixo é mais alta que a de cima, que coube inteira.
    let alturas = shell.git_surface().alturas_do_historico();
    assert!(
        alturas.len() >= 2 && alturas[1] > alturas[0],
        "a linha que quebrou cresceu: {alturas:?}"
    );
}
