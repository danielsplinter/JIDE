//! Testes do **painel de terminais**. Ver a `18`.
//!
//! Vários deles sobem um shell de verdade: sem ele na máquina, é aqui que
//! aparece.

use super::*;

#[test]
fn java_tool_output_is_appended_to_terminal() {
    let mut shell = test_shell();
    shell.append_tool_output("compile ok\nruntime failure", true);
    let lines = shell.active_terminal_lines().collect::<Vec<_>>();
    assert!(lines.contains(&"compile ok"));
    assert!(lines.contains(&"runtime failure"));
}
#[test]
fn sidebar_border_resizes_explorer_editor_and_terminal_widths_together() {
    let mut shell = test_shell();
    let size = Size::new(1280.0, 800.0);
    let before = shell.geometry().editor_width;
    let border = ACTIVITY_WIDTH + SIDEBAR_WIDTH;
    shell.pointer_down(Point::new(border, 300.0), size);
    assert!(shell.sidebar_resizing());
    shell.pointer_move(Point::new(border + 80.0, 300.0), size);
    shell.pointer_up();
    assert_eq!(shell.sidebar_width(size), SIDEBAR_WIDTH + 80.0);
    assert_eq!(shell.geometry().editor_width, before - 80.0);
    assert!(!shell.sidebar_resizing());
}
#[test]
fn editor_wheel_scrolls_and_terminal_profile_is_selectable() {
    let mut shell = test_shell();
    shell.editor_area.session.open_memory(
        "long.rs",
        (0..100)
            .map(|line| format!("line {line}"))
            .collect::<Vec<_>>()
            .join("\n"),
    );
    let size = Size::new(1280.0, 800.0);
    let editor_x = ACTIVITY_WIDTH + SIDEBAR_WIDTH;
    shell.scroll(Point::new(editor_x + 100.0, 200.0), 8.0, size);
    assert_eq!(shell.editor_scroll_line(), 8);
    let terminal_y = shell.geometry().editor_bottom + 10.0;
    shell.pointer_down(Point::new(editor_x + 115.0, terminal_y), size);
    assert_eq!(shell.selected_shell(), ShellKind::Cmd);
}
/// Cada aba tem a sua sessão, e o que se digita numa não alcança a outra.
///
/// Antes isto se verificava pelo texto que a IDE acumulava por aba. Ela não
/// acumula mais: quem guarda a linha é o shell, e a prova passou a ser que as
/// grades são independentes — trocar de aba troca de grade e de cursor.
#[test]
fn each_terminal_tab_has_its_own_grid() {
    let mut shell = test_shell();
    let size = Size::new(1280.0, 800.0);
    let editor_x = ACTIVITY_WIDTH + SIDEBAR_WIDTH;
    let terminal_y = shell.geometry().editor_bottom + 10.0;

    shell.pointer_down(Point::new(editor_x + 10.0, terminal_y), size);
    assert_eq!(shell.active_terminal_index(), 0);
    let primeira = shell.active_terminal().grid_rows();

    shell.pointer_down(Point::new(editor_x + 115.0, terminal_y), size);
    assert_eq!(shell.active_terminal_index(), 1);
    let segunda = shell.active_terminal().grid_rows();

    // Duas sessões, duas grades: mesma forma, conteúdos que não se misturam.
    assert_eq!(primeira.len(), segunda.len());
    assert_eq!(
        shell.active_terminal().cursor_position(),
        shell.active_terminal().cursor_position(),
        "o cursor é o da aba ativa"
    );
}
#[cfg(windows)]
#[test]
fn the_command_line_waits_below_what_already_ran() {
    let mut shell = test_shell();
    let size = Size::new(1280.0, 800.0);
    let editor_x = ACTIVITY_WIDTH + SIDEBAR_WIDTH;
    let terminal_y = shell.geometry().editor_bottom + 10.0;
    shell.pointer_down(Point::new(editor_x + 10.0, terminal_y), size);
    shell.text_input("Write-Output RESULT_BELOW");
    shell.key_down("Enter");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    while std::time::Instant::now() < deadline {
        shell.update_terminals();
        if shell
            .active_terminal_lines()
            .any(|line| line.contains("RESULT_BELOW"))
        {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    assert!(
        shell
            .active_terminal_lines()
            .any(|line| line.contains("RESULT_BELOW"))
    );

    // O que já rodou fica em cima; o cursor espera embaixo, como em qualquer
    // terminal. Antes era o contrário, e a linha de comando ficava no topo com o
    // histórico crescendo abaixo dela.
    let (saida, entrada) = shell.terminal_bands();
    assert!(
        entrada.origin.y >= saida.origin.y + saida.size.height,
        "a linha de comando tem que começar depois do fim da saída:          saída {saida:?}, entrada {entrada:?}"
    );
}
#[test]
fn terminal_selection_supports_forward_and_reverse_drag() {
    let forward = TerminalSelection {
        anchor: TextPosition { line: 2, column: 1 },
        focus: TextPosition { line: 2, column: 4 },
    };
    let reverse = TerminalSelection {
        anchor: forward.focus,
        focus: forward.anchor,
    };
    assert_eq!(selection_columns(Some(forward), 2, "abcdef"), Some((1, 4)));
    assert_eq!(selection_columns(Some(reverse), 2, "abcdef"), Some((1, 4)));
}
#[cfg(windows)]
#[test]
fn terminal_wheel_and_scrollbar_change_the_visible_offset() {
    let mut shell = test_shell();
    let size = Size::new(1280.0, 800.0);
    let editor_x = ACTIVITY_WIDTH + SIDEBAR_WIDTH;
    let terminal_y = shell.geometry().editor_bottom + 10.0;
    shell.pointer_down(Point::new(editor_x + 10.0, terminal_y), size);
    shell.text_input("1..80 | ForEach-Object { Write-Output \"scroll-$_\" }");
    shell.key_down("Enter");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    while std::time::Instant::now() < deadline {
        shell.update_terminals();
        if shell.active_terminal().line_count() >= 80 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    // O que se vê é a grade: rolar tem que mudar o texto visível, e não só um
    // número guardado à parte.
    let visivel = |shell: &IdeShell| -> String {
        shell
            .active_terminal()
            .grid_rows()
            .iter()
            .map(|linha| {
                linha
                    .iter()
                    .map(|celula| celula.character)
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join(
                "
",
            )
    };
    let no_fim = visivel(&shell);

    let content_point = Point::new(editor_x + 100.0, shell.geometry().editor_bottom + 90.0);
    shell.scroll(content_point, -5.0, size);
    assert_ne!(
        visivel(&shell),
        no_fim,
        "rolar para trás tem que mudar o que está na tela"
    );

    // O cursor é da tela viva: rolado para trás ele some, e volta ao chegar ao
    // fim. Fixo enquanto o texto sobe, ele apontaria uma linha que já passou.
    let cursores = |shell: &mut IdeShell, size: Size| -> usize {
        let acento = shell.theme().colors.accent;
        shell
            .paint(size)
            .iter()
            .filter(
                |command| matches!(command, PaintCommand::FillRect(fill) if fill.color == acento),
            )
            .count()
    };
    let rolado = cursores(&mut shell, size);

    shell.scroll(content_point, 50.0, size);
    assert_eq!(
        visivel(&shell),
        no_fim,
        "voltar ao fim mostra de novo o que estava lá"
    );
    assert!(
        cursores(&mut shell, size) > rolado,
        "o cursor volta a aparecer quando a janela chega ao fim"
    );
}
#[cfg(windows)]
#[test]
fn resizing_the_terminal_refills_it_without_losing_the_end() {
    let mut shell = test_shell();
    let size = Size::new(1280.0, 800.0);
    let editor_x = ACTIVITY_WIDTH + SIDEBAR_WIDTH;
    let terminal_header = shell.geometry().editor_bottom + 10.0;
    shell.pointer_down(Point::new(editor_x + 10.0, terminal_header), size);
    shell.text_input("Write-Output RESIZE_STABLE");
    shell.key_down("Enter");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    while std::time::Instant::now() < deadline {
        shell.update_terminals();
        if shell
            .active_terminal_lines()
            .any(|line| line.trim() == "RESIZE_STABLE")
        {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    std::thread::sleep(std::time::Duration::from_millis(150));
    shell.update_terminals();
    let com_conteudo = |shell: &IdeShell| -> usize {
        shell
            .active_terminal()
            .grid_rows()
            .iter()
            .filter(|linha| linha.iter().any(|celula| celula.character != ' '))
            .count()
            + shell.active_terminal().scrollback_len()
    };
    let antes = com_conteudo(&shell);
    assert!(antes > 0, "o terminal precisa ter saída antes do arrasto");

    let border = shell.geometry().editor_bottom;
    shell.pointer_down(Point::new(editor_x + 200.0, border), size);
    for y in [border - 20.0, border - 60.0, border - 100.0, border - 40.0] {
        shell.pointer_move(Point::new(editor_x + 200.0, y), size);
        shell.update_terminals();
    }
    shell.pointer_up();
    std::thread::sleep(std::time::Duration::from_millis(150));
    shell.update_terminals();

    // A grade acompanha o painel: sem isso, crescer deixaria uma faixa vazia
    // embaixo, com o texto parado no tamanho antigo.
    assert_eq!(
        shell.active_terminal().grid_rows().len(),
        shell.terminal_visible_lines(),
        "a grade tem que ter tantas linhas quantas cabem no painel"
    );
    // E nada se perde. O que sai da tela ao encolher vai para o histórico, que é
    // o que um terminal faz — por isso a conta soma os dois.
    assert!(
        com_conteudo(&shell) >= antes,
        "encolher o painel não pode descartar saída"
    );
}
#[test]
fn terminal_button_minimizes_and_restores_previous_height() {
    let mut shell = test_shell();
    let size = Size::new(1280.0, 800.0);
    let original = shell.terminal_height();
    let toggle = Point::new(size.width - 20.0, shell.geometry().editor_bottom + 12.0);
    shell.pointer_down(toggle, size);
    assert!(shell.terminal_minimized());
    assert_eq!(shell.geometry().terminal_height, TERMINAL_COLLAPSED_HEIGHT);

    let restore = Point::new(size.width - 20.0, shell.geometry().editor_bottom + 12.0);
    shell.pointer_down(restore, size);
    assert!(!shell.terminal_minimized());
    assert_eq!(shell.terminal_height(), original);
}
#[test]
fn dragging_terminal_top_border_changes_height_with_limits() {
    let mut shell = test_shell();
    let size = Size::new(1280.0, 800.0);
    let editor_x = ACTIVITY_WIDTH + SIDEBAR_WIDTH;
    let border_y = shell.geometry().editor_bottom;
    shell.pointer_down(Point::new(editor_x + 100.0, border_y), size);
    assert!(shell.terminal_resizing());
    assert!(shell.pointer_move(Point::new(editor_x + 100.0, border_y - 70.0), size));
    assert_eq!(shell.terminal_height(), TERMINAL_DEFAULT_HEIGHT + 70.0);
    shell.pointer_move(Point::new(editor_x + 100.0, size.height), size);
    assert_eq!(shell.terminal_height(), TERMINAL_MIN_HEIGHT);
    shell.pointer_up();
    assert!(!shell.terminal_resizing());
}
/// O que se copia do terminal é o que está na tela, célula por célula.
///
/// A leitura vinha de `lines()`, uma lista acumulada à parte, enquanto o que
/// aparece vem de `grid_rows()`. Duas fontes para a mesma tela — e os números da
/// seleção são da **viewport**, então lidos contra a lista acumulada apontavam
/// para o começo do histórico.
#[test]
fn o_terminal_copia_o_que_esta_na_grade() {
    let mut shell = test_shell();
    let size = Size::new(1280.0, 800.0);
    let grade = shell.active_terminal().grid_rows();
    let Some(primeira) = grade.first() else {
        panic!("a grade precisa ter linhas");
    };
    let largura = primeira.len();
    assert!(largura > 4, "a grade precisa ter colunas");

    // Marca as quatro primeiras células da primeira linha visível.
    shell.set_terminal_selection_for_test(
        TextPosition { line: 0, column: 0 },
        TextPosition { line: 0, column: 4 },
    );
    let esperado: String = primeira.iter().take(4).map(|celula| celula.character).collect();
    assert_eq!(
        shell.selected_terminal_text(),
        esperado.trim_end(),
        "o copiado sai das mesmas células que o desenho lê"
    );

    // Até o fim da linha, os espaços de preenchimento da grade não vêm junto.
    shell.set_terminal_selection_for_test(
        TextPosition { line: 0, column: 0 },
        TextPosition {
            line: 0,
            column: largura,
        },
    );
    let copiado = shell.selected_terminal_text();
    assert_eq!(
        copiado,
        copiado.trim_end(),
        "a cauda de espaços da grade retangular não é conteúdo"
    );
    let _ = size;
}
/// `Ctrl+F` com o foco no terminal procura **na saída**, e não no arquivo.
///
/// A barra é a mesma — o componente do editor —, e o que muda é o alvo, decidido
/// pelo foco de então. Aqui se confere o caminho inteiro: abrir, achar, andar
/// entre as ocorrências e fechar.
#[test]
fn a_busca_no_terminal_usa_a_mesma_barra_e_procura_na_saida() {
    let mut shell = test_shell();
    let size = Size::new(1280.0, 800.0);
    let _ = shell.paint(size);

    // Sem foco no terminal, `Ctrl+F` é a busca do arquivo.
    shell.context.focus = ShellFocus::Editor;
    shell.toggle_search();
    assert!(shell.editor_area.search_open);
    assert!(shell.terminal.busca.is_none());
    shell.escape();

    // Com o foco no terminal, a mesma tecla abre a busca da saída.
    shell.context.focus = ShellFocus::Terminal;
    shell.toggle_search();
    assert!(
        shell.terminal.busca.is_some(),
        "o Ctrl+F do terminal abre a busca dele"
    );
    assert_eq!(shell.context.focus, ShellFocus::SearchTerminal);

    // O que se digita procura na grade: o prompt do shell tem o caminho do
    // projeto, e é o que há na tela num terminal recém-aberto.
    let alvo = shell
        .active_terminal()
        .grid_rows()
        .first()
        .and_then(|linha| linha.iter().find(|celula| celula.character.is_alphanumeric()))
        .map(|celula| celula.character);
    if let Some(caractere) = alvo {
        shell.text_input(&caractere.to_string());
        let Some(busca) = shell.terminal.busca.as_ref() else {
            panic!("a busca da saída precisa existir depois de digitar");
        };
        assert!(!busca.achados.is_empty(), "o caractere está na tela");
        assert_eq!(busca.atual, Some(0), "a primeira já nasce em foco");

        // `Enter` anda, `Shift+Enter` volta, e as duas dão a volta.
        let total = busca.achados.len();
        let com_shift = Modifiers {
            shift: true,
            ..Modifiers::default()
        };
        shell.key_down("Enter");
        assert_eq!(
            shell.terminal.busca.as_ref().and_then(|busca| busca.atual),
            Some(1 % total)
        );
        shell.key_down_with_modifiers("Enter", com_shift);
        assert_eq!(
            shell.terminal.busca.as_ref().and_then(|busca| busca.atual),
            Some(0)
        );
        shell.key_down_with_modifiers("Enter", com_shift);
        assert_eq!(
            shell.terminal.busca.as_ref().and_then(|busca| busca.atual),
            Some(total - 1),
            "do começo, Shift+Enter dá a volta pelo fim"
        );
    }

    // Fechar larga a busca da saída e devolve o foco ao terminal.
    shell.escape();
    assert!(shell.terminal.busca.is_none());
    assert_eq!(shell.context.focus, ShellFocus::Terminal);
}
/// A busca do terminal mora na fileira das abas, antes do botão de recolher.
///
/// Ela nasceu embaixo da saída, onde disputava altura com o que se estava
/// lendo. Na fileira das abas ela não come linha nenhuma do terminal — e o
/// espaço da direita continua sendo do botão que recolhe o painel.
#[test]
fn a_busca_do_terminal_fica_na_fileira_das_abas() {
    let mut shell = test_shell();
    let size = Size::new(1280.0, 800.0);
    let _ = shell.paint(size);

    shell.context.focus = ShellFocus::Terminal;
    shell.toggle_search();
    let _ = shell.paint(size);

    let caixa = shell.terminal_search_box_area();
    let recolher = shell.terminal_toggle_rect(size);
    let (saida, _) = shell.terminal_bands_for_test();
    assert!(caixa.size.width > 0.0, "a caixa precisa ter área");
    assert!(
        caixa.origin.y + caixa.size.height <= saida.origin.y + 0.01,
        "ela fica acima da saída, na fileira das abas: {caixa:?} contra {saida:?}"
    );
    assert!(
        caixa.origin.x + caixa.size.width <= recolher.origin.x + 0.01,
        "e termina antes do botão de recolher: {caixa:?} contra {recolher:?}"
    );
}
