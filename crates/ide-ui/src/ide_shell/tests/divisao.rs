//! Testes da **área dividida**. Ver a `28`.

use super::*;

/// O divisor é desenhado no lugar certo desde o primeiro quadro, antes de
/// qualquer evento de ponteiro, e se destaca quando o ponteiro se aproxima.
#[test]
fn the_sidebar_divider_is_painted_in_place_and_highlights_under_the_pointer() {
    let mut shell = test_shell();
    let size = Size::new(1280.0, 800.0);
    let divider_color = |shell: &mut IdeShell| {
        let x = ACTIVITY_WIDTH + shell.sidebar_width(size);
        shell.paint(size).iter().find_map(|command| match command {
            PaintCommand::FillRect(fill)
                if (fill.rect.origin.x - x).abs() < 0.01
                    && fill.rect.size.width == Splitter::THICKNESS =>
            {
                Some(fill.color)
            }
            _ => None,
        })
    };
    assert_eq!(divider_color(&mut shell), Some(shell.theme().colors.border));

    shell.pointer_move(Point::new(ACTIVITY_WIDTH + SIDEBAR_WIDTH, 300.0), size);
    assert_eq!(divider_color(&mut shell), Some(shell.theme().colors.accent));
}
/// Dividir à direita põe o mesmo documento em dois editores independentes.
///
/// Independentes **na vista**, e não no texto: o cursor e a rolagem são de cada
/// lado, e o conteúdo é um só. Duas cópias do texto fariam gravar escolher em
/// silêncio qual das versões sobrevive.
#[test]
fn dividir_a_direita_abre_dois_editores_sobre_o_mesmo_texto() {
    let root = std::env::temp_dir().join(format!("er-ide-split-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    assert!(std::fs::create_dir_all(&root).is_ok());
    let arquivo = root.join("Pedido.java");
    assert!(std::fs::write(&arquivo, "class Pedido {}").is_ok());
    let Ok(mut shell) = IdeShell::open(&root) else {
        panic!("workspace de teste não abriu");
    };
    let Ok(_) = shell.open_file(&arquivo) else {
        panic!("arquivo de teste não abriu");
    };
    let Some(documento) = shell.editor_area.session.active_id() else {
        panic!("o arquivo aberto precisa ser o ativo");
    };
    let size = Size::new(1280.0, 800.0);
    assert!(!shell.is_split());

    // Clique secundário sobre a aba, e a opção do menu.
    let aba = Point::new(
        shell.editor_view_rect(size).origin.x + 10.0,
        TITLE_HEIGHT + TAB_HEIGHT / 2.0,
    );
    shell.secondary_pointer_down(aba, size);
    assert!(shell.context_menu_open(), "o menu da aba deveria abrir");
    shell.run_context_command("editor.split.right");
    // Escolher pelo menu o fecha; aqui o comando foi chamado direto, e o menu
    // aberto engoliria os gestos seguintes.
    shell.key_down("Escape");

    assert!(shell.is_split(), "a área deveria estar dividida");
    assert_eq!(
        shell.editor_area.session.active_id(),
        Some(documento),
        "o documento dividido continua sendo o ativo"
    );
    // As duas faixas de abas continuam na tela, cada uma na coluna dela. A da
    // esquerda sumiu uma vez por ter recebido uma largura sem crescimento, e a
    // tela ficou com o editor dividido e nenhuma aba do lado esquerdo.
    let (Some(esquerda), Some(direita)) = (shell.left_tabs_rect(size), shell.right_tabs_rect(size))
    else {
        panic!("as duas faixas precisam ter área");
    };
    let desenhado = shell.paint(size);
    let nome_em = |faixa: Rect| {
        desenhado.iter().any(|comando| {
            matches!(comando, PaintCommand::DrawText(texto)
                if texto.text.contains("Pedido.java")
                    && texto.origin.x >= faixa.origin.x
                    && texto.origin.x < faixa.origin.x + faixa.size.width)
        })
    };
    assert!(nome_em(esquerda), "a faixa da esquerda precisa mostrar o arquivo");
    assert!(nome_em(direita), "a faixa da direita precisa mostrar o arquivo");

    // O texto é um só: escrever de um lado vale para o outro.
    shell.context.focus = ShellFocus::Editor;
    shell.editor_area.pane.set_cursor(0);
    shell.text_input("// dois lados\n");
    let Some(divisao) = shell.editor_area.divisao.as_ref() else {
        panic!("a divisão precisa existir");
    };
    assert_eq!(divisao.ativa, documento);
    assert!(
        shell
            .editor_area
            .session
            .document(divisao.ativa)
            .is_some_and(|documento| documento.buffer.text().starts_with("// dois lados")),
        "os dois lados olham o mesmo texto"
    );

    // Fechar a aba da direita desfaz a divisão.
    let Some(abas) = shell.right_tabs_rect(size) else {
        panic!("a faixa de abas da direita precisa ter área");
    };
    let fechar = Point::new(abas.origin.x + TAB_WIDTH - 14.0, abas.origin.y + TAB_HEIGHT / 2.0);
    // O ponteiro passa antes de clicar, como acontece com um mouse: numa aba com
    // alterações por gravar, é a passagem que revela o botão de fechar.
    shell.pointer_move(fechar, size);
    shell.pointer_down(fechar, size);
    assert!(
        !shell.is_split(),
        "sem aba nenhuma não há painel; abas={:?}",
        shell
            .editor_area
            .divisao
            .as_ref()
            .map(|divisao| divisao.abas.clone())
    );
    let _ = std::fs::remove_dir_all(&root);
}
/// Soltar o botão encerra o arrasto da divisa.
///
/// Sem o soltar chegando ao painel, ele continuava em arrasto para sempre: o
/// ponteiro ficava preso na divisa, e mexer o mouse — sem nenhum botão apertado —
/// continuava movendo os dois editores.
#[test]
fn soltar_o_botao_solta_a_divisa_dos_dois_editores() {
    let root = std::env::temp_dir().join(format!("er-ide-divisa-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    assert!(std::fs::create_dir_all(&root).is_ok());
    let arquivo = root.join("Pedido.java");
    assert!(std::fs::write(&arquivo, "class Pedido {}").is_ok());
    let Ok(mut shell) = IdeShell::open(&root) else {
        panic!("workspace de teste não abriu");
    };
    let Ok(_) = shell.open_file(&arquivo) else {
        panic!("arquivo de teste não abriu");
    };
    let Some(documento) = shell.editor_area.session.active_id() else {
        panic!("o arquivo aberto precisa ser o ativo");
    };
    let size = Size::new(1280.0, 800.0);
    shell.dividir_a_direita(documento);

    let Some(painel) = shell.split_panel_for(size) else {
        panic!("a divisão precisa existir");
    };
    let divisa = painel.divider();
    let meio = Point::new(
        divisa.origin.x + divisa.size.width / 2.0,
        divisa.origin.y + 40.0,
    );
    shell.pointer_down(meio, size);
    shell.pointer_move(Point::new(meio.x + 120.0, meio.y), size);

    let arrastado = shell
        .split_panel_for(size)
        .map_or(0.0, |painel| painel.ratio());
    shell.pointer_up();

    // Solto, o movimento seguinte não move mais nada.
    shell.pointer_move(Point::new(meio.x + 400.0, meio.y), size);
    assert_eq!(
        shell
            .split_panel_for(size)
            .map_or(0.0, |painel| painel.ratio()),
        arrastado,
        "sem o botão apertado a divisa não pode mais seguir o ponteiro"
    );
    let _ = std::fs::remove_dir_all(&root);
}
/// O arquivo novo abre no painel em que se clicou por último.
///
/// **Clique, e não passagem do ponteiro.** O caminho do mouse até o Explorer
/// atravessa o painel da esquerda, e essa travessia não significa que se deixou
/// de trabalhar na direita — quem diz onde se estava trabalhando é o último
/// clique.
#[test]
fn o_arquivo_novo_abre_no_painel_com_foco() {
    let root = std::env::temp_dir().join(format!("er-ide-abre-foco-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    assert!(std::fs::create_dir_all(&root).is_ok());
    let primeiro = root.join("Pedido.java");
    let segundo = root.join("Cliente.java");
    assert!(std::fs::write(&primeiro, "class Pedido {}").is_ok());
    assert!(std::fs::write(&segundo, "class Cliente {}").is_ok());
    let Ok(mut shell) = IdeShell::open(&root) else {
        panic!("workspace de teste não abriu");
    };
    let Ok(_) = shell.open_file(&primeiro) else {
        panic!("arquivo de teste não abriu");
    };
    let Some(documento) = shell.editor_area.session.active_id() else {
        panic!("o arquivo aberto precisa ser o ativo");
    };
    let size = Size::new(1280.0, 800.0);
    shell.dividir_a_direita(documento);
    assert!(shell.split_focado(), "a divisão nasce com a direita em foco");

    // Um clique no painel da esquerda: é ele que passa a receber os arquivos.
    let Some(esquerda) = shell.left_column(size) else {
        panic!("a coluna da esquerda precisa ter área");
    };
    let dentro_da_esquerda = Point::new(esquerda.origin.x + 30.0, esquerda.origin.y + 150.0);
    shell.pointer_move(dentro_da_esquerda, size);
    shell.pointer_down(dentro_da_esquerda, size);
    // E o ponteiro passa pela direita a caminho do Explorer: a travessia não
    // pode mudar para onde o arquivo vai.
    let Some(direita) = shell.right_column(size) else {
        panic!("a coluna da direita precisa ter área");
    };
    shell.pointer_move(
        Point::new(direita.origin.x + 30.0, direita.origin.y + 150.0),
        size,
    );
    shell.pointer_move(Point::new(ACTIVITY_WIDTH + 40.0, 300.0), size);

    let Ok(_) = shell.open_file(&segundo) else {
        panic!("o segundo arquivo não abriu");
    };
    let Some(novo) = shell.editor_area.session.active_id() else {
        panic!("o arquivo aberto precisa ser o ativo");
    };
    let Some(divisao) = shell.editor_area.divisao.as_ref() else {
        panic!("a divisão precisa continuar existindo");
    };
    assert!(
        !divisao.abas.contains(&novo),
        "o arquivo novo não pode ir para o painel que não tem o foco"
    );
    assert_eq!(
        shell.left_active_document(),
        Some(novo),
        "ele abre no painel apontado, e é a aba acesa dele"
    );
    let _ = std::fs::remove_dir_all(&root);
}
/// Dividida a área, a barra horizontal continua aparecendo.
///
/// A trilha dela era a da área inteira. Com dois painéis, uma trilha do tamanho
/// dos dois é larga demais para o texto de um só: a barra concluía que não havia
/// o que rolar e sumia dos **dois** lados, mesmo com linhas passando da borda.
#[test]
fn a_barra_horizontal_sobrevive_a_divisao() {
    let root = std::env::temp_dir().join(format!("er-ide-barra-split-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    assert!(std::fs::create_dir_all(&root).is_ok());
    let arquivo = root.join("Longo.java");
    let linha = format!("class Longo {{ // {} }}", "x".repeat(400));
    assert!(std::fs::write(&arquivo, linha).is_ok());
    let Ok(mut shell) = IdeShell::open(&root) else {
        panic!("workspace de teste não abriu");
    };
    let Ok(_) = shell.open_file(&arquivo) else {
        panic!("arquivo de teste não abriu");
    };
    let Some(documento) = shell.editor_area.session.active_id() else {
        panic!("o arquivo aberto precisa ser o ativo");
    };
    let size = Size::new(1280.0, 800.0);
    let _ = shell.paint(size);
    assert!(
        shell.editor_scrolls_sideways(size),
        "a linha longa precisa passar da borda antes da divisão"
    );

    shell.dividir_a_direita(documento);
    let _ = shell.paint(size);

    assert!(
        shell.editor_scrolls_sideways(size),
        "com metade da largura, ela passa ainda mais"
    );
    let trilha = shell.editor_horizontal_scrollbar_rect(size);
    let coluna = match shell.right_editor_rect(size) {
        Some(coluna) => coluna,
        None => panic!("a coluna da direita precisa ter área"),
    };
    assert!(
        trilha.size.width <= coluna.size.width,
        "a trilha é a do painel da frente, e não a dos dois: {trilha:?}"
    );
    assert!(
        trilha.origin.x >= coluna.origin.x - 0.01,
        "e ela começa onde o painel da frente começa"
    );
    let _ = std::fs::remove_dir_all(&root);
}
/// Clicar no editor da direita move o cursor **dele**.
///
/// O clique era entregue ao painel guardado na divisão — que, depois da troca de
/// foco, é o do outro lado. O cursor ia parar no editor da esquerda, e clicar no
/// da direita não movia nada.
#[test]
fn o_clique_no_editor_da_direita_move_o_cursor_dele() {
    let root = std::env::temp_dir().join(format!("er-ide-cursor-split-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    assert!(std::fs::create_dir_all(&root).is_ok());
    let arquivo = root.join("Pedido.java");
    assert!(std::fs::write(&arquivo, "class Pedido {\n    int total;\n    int itens;\n}\n").is_ok());
    let Ok(mut shell) = IdeShell::open(&root) else {
        panic!("workspace de teste não abriu");
    };
    let Ok(_) = shell.open_file(&arquivo) else {
        panic!("arquivo de teste não abriu");
    };
    let Some(documento) = shell.editor_area.session.active_id() else {
        panic!("o arquivo aberto precisa ser o ativo");
    };
    let size = Size::new(1280.0, 800.0);
    shell.dividir_a_direita(documento);
    let _ = shell.paint(size);

    let Some(direita) = shell.right_editor_rect(size) else {
        panic!("a coluna da direita precisa ter área");
    };
    // Terceira linha do texto, bem dentro da coluna da direita.
    let alvo = Point::new(
        direita.origin.x + 80.0,
        direita.origin.y + EDITOR_LINE_HEIGHT * 2.5,
    );
    shell.pointer_move(alvo, size);
    shell.pointer_down(alvo, size);

    let Some(divisao) = shell.editor_area.divisao.as_ref() else {
        panic!("a divisão precisa existir");
    };
    assert!(
        shell.editor_area.pane.cursor() > 0,
        "o cursor do painel apontado precisa ter ido para o ponto clicado"
    );
    assert_eq!(
        divisao.pane.cursor(),
        0,
        "o cursor do outro painel não pode andar"
    );
    let _ = std::fs::remove_dir_all(&root);
}
/// A lista de completação nasce dentro do painel que tem o foco.
///
/// Ela era ancorada na borda esquerda da área do editor. Dividida a tela, o
/// cursor está numa das colunas e a lista aparecia sobre a outra — longe do que
/// se estava digitando, e por cima do texto de quem não pediu nada.
#[test]
fn a_completacao_nasce_no_painel_com_foco() {
    let root = std::env::temp_dir().join(format!("er-ide-comp-split-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    assert!(std::fs::create_dir_all(&root).is_ok());
    let arquivo = root.join("Pedido.java");
    assert!(std::fs::write(&arquivo, "class Pedido {\n    int total;\n}\n").is_ok());
    let Ok(mut shell) = IdeShell::open(&root) else {
        panic!("workspace de teste não abriu");
    };
    let Ok(_) = shell.open_file(&arquivo) else {
        panic!("arquivo de teste não abriu");
    };
    let Some(documento) = shell.editor_area.session.active_id() else {
        panic!("o arquivo aberto precisa ser o ativo");
    };
    let size = Size::new(1280.0, 800.0);
    shell.dividir_a_direita(documento);
    let _ = shell.paint(size);
    shell.context.focus = ShellFocus::Editor;
    shell.set_completions(vec![CompletionItem {
        label: "total".to_owned(),
        detail: None,
        kind: ide_domain::CompletionKind::Field,
    }]);

    let Some(direita) = shell.right_editor_rect(size) else {
        panic!("a coluna da direita precisa ter área");
    };
    let Some(ancora) = shell.completion_anchor(size) else {
        panic!("com itens e foco no editor, a lista tem âncora");
    };
    assert!(
        ancora.x >= direita.origin.x,
        "a lista nasce dentro do painel com foco: {ancora:?} contra {direita:?}"
    );
    assert!(
        ancora.x + COMPLETION_POPUP_WIDTH <= direita.origin.x + direita.size.width + 0.01,
        "e não transborda para o painel vizinho"
    );
    let _ = std::fs::remove_dir_all(&root);
}
/// Sobre a divisa, o ponteiro anuncia que ela se arrasta.
///
/// A janela troca o desenho do ponteiro perguntando isto. Sem a pergunta, a
/// divisa é uma linha como outra qualquer na tela, e quem usa só descobre que
/// ela se move por acidente.
#[test]
fn a_divisa_dos_editores_pede_a_seta_de_redimensionar() {
    let root = std::env::temp_dir().join(format!("er-ide-seta-split-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    assert!(std::fs::create_dir_all(&root).is_ok());
    let arquivo = root.join("Pedido.java");
    assert!(std::fs::write(&arquivo, "class Pedido {}").is_ok());
    let Ok(mut shell) = IdeShell::open(&root) else {
        panic!("workspace de teste não abriu");
    };
    let Ok(_) = shell.open_file(&arquivo) else {
        panic!("arquivo de teste não abriu");
    };
    let Some(documento) = shell.editor_area.session.active_id() else {
        panic!("o arquivo aberto precisa ser o ativo");
    };
    let size = Size::new(1280.0, 800.0);
    // Sem divisão não há divisa, e a pergunta não pode inventar uma.
    assert!(!shell.split_divider_hover(Point::new(640.0, 400.0), size));

    shell.dividir_a_direita(documento);
    let Some(painel) = shell.split_panel_for(size) else {
        panic!("a divisão precisa existir");
    };
    let divisa = painel.divider();
    let sobre = Point::new(
        divisa.origin.x + divisa.size.width / 2.0,
        divisa.origin.y + 100.0,
    );
    assert!(shell.split_divider_hover(sobre, size));
    assert!(
        !shell.split_divider_hover(Point::new(sobre.x + 60.0, sobre.y), size),
        "longe dela, o ponteiro volta a ser o de sempre"
    );

    // Em arrasto a resposta continua sendo sim, mesmo com o ponteiro longe: a
    // divisa segue sendo o alvo do gesto.
    shell.pointer_down(sobre, size);
    shell.pointer_move(Point::new(sobre.x + 200.0, sobre.y), size);
    assert!(shell.split_divider_hover(Point::new(sobre.x + 200.0, sobre.y), size));
    let _ = std::fs::remove_dir_all(&root);
}
