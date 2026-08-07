//! Testes do **texto**: cursor, seleção, indentação, pares, rolagem e abas.

use super::*;

/// Tab escreve espaços até a próxima parada de tabulação. A partir da
/// coluna 2 são dois espaços, não quatro — o texto alinha com a grade que o
/// editor desenha.
#[test]
fn tab_indents_the_editor_to_the_next_stop() {
    let mut shell = test_shell();
    shell.editor_area.session.open_memory("Example.java", "ab");
    shell.context.focus = ShellFocus::Editor;
    shell.editor_area.pane.set_cursor(2);
    shell.key_down("Tab");
    assert_eq!(shell.active_text(), Some("ab  "));
    assert_eq!(shell.editor_area.pane.cursor(), 4);
}
/// Shift+Tab recolhe a margem da linha inteira, com o cursor no meio do
/// código, e o cursor acompanha o deslocamento.
#[test]
fn shift_tab_unindents_the_current_line() {
    let mut shell = test_shell();
    shell
        .editor_area
        .session
        .open_memory("Example.java", "class A {\n    int valor;\n}");
    shell.context.focus = ShellFocus::Editor;
    shell.editor_area.pane.set_cursor(14);
    shell.key_down_with_modifiers(
        "Tab",
        Modifiers {
            shift: true,
            ..Modifiers::default()
        },
    );
    assert_eq!(shell.active_text(), Some("class A {\nint valor;\n}"));
    assert_eq!(shell.editor_area.pane.cursor(), 10);
}
#[test]
fn toggling_from_the_keyboard_uses_the_cursor_line() {
    let (mut shell, path) = shell_with_java_file();
    shell.editor_area.pane.set_cursor(20); // segunda linha
    shell.toggle_breakpoint_at_cursor();
    assert_eq!(shell.breakpoints_for(&path), vec![1]);
}
#[test]
fn java_syntax_snapshot_drives_highlighting_and_outline() {
    let mut shell = test_shell();
    let document_id = shell
        .editor_area
        .session
        .open_memory("Example.java", "public class Example {}");
    shell.set_syntax_snapshot(ide_domain::SyntaxSnapshot {
        document_id,
        version: 0,
        outline: vec![ide_domain::OutlineItem {
            name: "Example".to_owned(),
            kind: ide_domain::OutlineKind::Class,
            range: ide_domain::TextRange::default(),
            name_range: ide_domain::TextRange::default(),
            children: Vec::new(),
        }],
        highlights: vec![ide_domain::SyntaxHighlight {
            range: ide_domain::TextRange {
                start: ide_domain::TextPosition { line: 0, column: 0 },
                end: ide_domain::TextPosition { line: 0, column: 6 },
            },
            kind: SyntaxHighlightKind::Keyword,
        }],
        imports: Vec::new(),
        diagnostics: Vec::new(),
    });

    assert_eq!(shell.active_outline()[0].name, "Example");
    let cached = shell
        .editor_area
        .syntax_spans
        .get(&document_id)
        .unwrap_or_else(|| panic!("o realce convertido precisa ser cacheado"));
    assert_eq!(cached.spans, vec![(0, 6, TokenKind::Keyword)]);
    let cached_pointer = cached.spans.as_ptr();
    let colors = Theme::default().colors;
    assert!(shell.paint(Size::new(1280.0, 800.0)).iter().any(|command| {
        matches!(
            command,
            PaintCommand::DrawText(text)
                if text.text == "public" && text.color == colors.syntax_keyword
        )
    }));
    let _ = shell.paint(Size::new(1280.0, 800.0));
    assert_eq!(
        shell.editor_area.syntax_spans[&document_id].spans.as_ptr(),
        cached_pointer,
        "quadros seguintes devem reutilizar o realce convertido"
    );
}
/// **Escrever um nome abre a lista sozinho, na segunda letra.**
///
/// Até aqui só o ponto abria a completação, e escrever um nome exigia
/// `Ctrl+Espaço` — que ninguém lembra de apertar. Foi o que fez `private http:
/// Http` não oferecer nada.
///
/// Abre **uma vez**, e não a cada tecla: a partir daí quem mantém a lista viva é
/// `completion_follow_up`. Se abrisse sempre que houvesse duas letras ou mais,
/// `Escape` deixaria de funcionar — a lista voltaria na tecla seguinte.
#[test]
fn typing_a_name_opens_the_list_on_the_second_letter() {
    let mut shell = test_shell();
    shell.editor_area.session.open_memory("Pagina.ts", "");
    shell.context.focus = ShellFocus::Editor;

    shell.text_input("H");
    assert!(
        !shell.completion_opens_now(),
        "uma letra abriria a lista com o alfabeto inteiro dentro"
    );
    shell.text_input("t");
    assert!(shell.completion_opens_now(), "duas letras já é um nome");
    shell.text_input("t");
    assert!(
        !shell.completion_opens_now(),
        "a terceira não reabre: quem mantém a lista viva é o filtro"
    );
}
/// A mão acende no que dá para navegar, não só em tipo.
///
/// Enquanto só `Type` acendia, o clique navegava em método, campo e variável
/// sem que nada na tela dissesse que era possível.
#[test]
fn the_navigation_cursor_agrees_with_what_the_click_resolves() {
    let mut shell = test_shell();
    //                                    0         1         2         3
    //                                    0123456789012345678901234567890
    let document_id = shell
        .editor_area
        .session
        .open_memory("A.java", "void metodo() { int x = y; }");
    let realce = |coluna_inicial: u32, coluna_final: u32, kind| ide_domain::SyntaxHighlight {
        range: ide_domain::TextRange {
            start: ide_domain::TextPosition {
                line: 0,
                column: coluna_inicial,
            },
            end: ide_domain::TextPosition {
                line: 0,
                column: coluna_final,
            },
        },
        kind,
    };
    shell.set_syntax_snapshot(ide_domain::SyntaxSnapshot {
        document_id,
        version: 0,
        outline: Vec::new(),
        highlights: vec![
            realce(0, 4, SyntaxHighlightKind::Keyword),
            realce(5, 11, SyntaxHighlightKind::Function),
            realce(20, 21, SyntaxHighlightKind::Variable),
            realce(24, 25, SyntaxHighlightKind::Field),
        ],
        imports: Vec::new(),
        diagnostics: Vec::new(),
    });

    let size = Size::new(1280.0, 800.0);
    let editor_x = ACTIVITY_WIDTH + shell.sidebar_width(size);
    let sobre = |coluna: f32| {
        Point::new(
            editor_x + EDITOR_GUTTER + coluna * EDITOR_CHAR_WIDTH,
            shell.geometry().content_top + 15.0,
        )
    };
    assert!(shell.navigation_hover(sobre(7.0), size, true), "método");
    assert!(shell.navigation_hover(sobre(20.0), size, true), "variável");
    assert!(shell.navigation_hover(sobre(24.0), size, true), "campo");
    assert!(
        !shell.navigation_hover(sobre(1.0), size, true),
        "palavra-chave não leva a lugar nenhum"
    );
}
#[test]
fn control_hover_over_java_type_uses_navigation_cursor_state() {
    let mut shell = test_shell();
    let document_id = shell
        .editor_area
        .session
        .open_memory("Example.java", "class Example {}");
    shell.set_syntax_snapshot(ide_domain::SyntaxSnapshot {
        document_id,
        version: 0,
        outline: Vec::new(),
        highlights: vec![ide_domain::SyntaxHighlight {
            range: ide_domain::TextRange {
                start: ide_domain::TextPosition { line: 0, column: 6 },
                end: ide_domain::TextPosition {
                    line: 0,
                    column: 13,
                },
            },
            kind: SyntaxHighlightKind::Type,
        }],
        imports: Vec::new(),
        diagnostics: Vec::new(),
    });
    let size = Size::new(1280.0, 800.0);
    let editor_x = ACTIVITY_WIDTH + shell.sidebar_width(size);
    let point = Point::new(
        editor_x + EDITOR_GUTTER + 8.0 * EDITOR_CHAR_WIDTH,
        shell.geometry().content_top + 15.0,
    );
    assert!(!shell.navigation_hover(point, size, false));
    assert!(shell.navigation_hover(point, size, true));
}
#[test]
fn tab_click_changes_active_document_and_typing_edits_it() {
    let mut shell = test_shell();
    let first = shell.editor_area.session.open_memory("first.rs", "one");
    let second = shell.editor_area.session.open_memory("second.rs", "two");
    assert_eq!(shell.active_document(), Some(second));
    let editor_x = ACTIVITY_WIDTH + SIDEBAR_WIDTH;
    shell.pointer_down(
        Point::new(editor_x + 10.0, TITLE_HEIGHT + 10.0),
        Size::new(1280.0, 800.0),
    );
    assert_eq!(shell.active_document(), Some(first));
    shell.pointer_down(
        Point::new(editor_x + EDITOR_GUTTER, TITLE_HEIGHT + TAB_HEIGHT + 15.0),
        Size::new(1280.0, 800.0),
    );
    shell.text_input("X");
    assert_eq!(shell.active_text(), Some("Xone"));
}
#[test]
fn tab_close_button_removes_only_the_clicked_document() {
    let mut shell = test_shell();
    let first = shell.editor_area.session.open_memory("first.rs", "one");
    shell.editor_area.session.open_memory("second.rs", "two");
    let size = Size::new(1280.0, 800.0);
    let editor_x = ACTIVITY_WIDTH + SIDEBAR_WIDTH;
    shell.pointer_down(
        Point::new(
            editor_x + TAB_WIDTH * 2.0 - 15.0,
            TITLE_HEIGHT + TAB_HEIGHT / 2.0,
        ),
        size,
    );
    assert_eq!(shell.tab_count(), 1);
    assert_eq!(shell.active_document(), Some(first));
}
/// Documento alterado e não gravado é sinalizado na aba.
#[test]
fn an_unsaved_document_is_marked_on_its_tab() {
    let mut shell = test_shell();
    shell.editor_area.session.open_memory("first.rs", "one");
    let size = Size::new(1280.0, 800.0);
    let marks = |shell: &mut IdeShell| {
        shell
            .paint(size)
            .iter()
            .filter_map(|command| match command {
                PaintCommand::DrawText(text) => Some(text.text.clone()),
                _ => None,
            })
            .any(|text| text == "●")
    };
    assert!(!marks(&mut shell), "documento intacto não é marcado");

    shell.context.focus = ShellFocus::Editor;
    shell.edit_active("x");
    assert!(marks(&mut shell), "documento alterado é marcado");
}
#[test]
fn long_tab_titles_are_clipped_and_ellipsized_before_close_button() {
    let mut shell = test_shell();
    shell
        .editor_area
        .session
        .open_memory("ExplosionEffectManager.ts", "content");
    let rendered = shell.paint(Size::new(1280.0, 800.0));
    let texts = rendered
        .iter()
        .filter_map(|command| match command {
            PaintCommand::DrawText(command) => Some(command.text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>();
    // O nome inteiro não cabe: aparece encurtado, com a marca do corte, e o
    // desenho fica contido na faixa das abas.
    assert!(!texts.contains(&"ExplosionEffectManager.ts"));
    assert!(
        texts
            .iter()
            .any(|text| text.starts_with("Explosion") && text.ends_with('…')),
        "esperava um título encurtado, veio {texts:?}"
    );
    // A faixa das abas vem do arranjo, como tudo mais na moldura.
    let tabs = shell
        .host
        .bounds(EDITOR_TABS_ID)
        .unwrap_or_else(|| panic!("as abas precisam ter área"));
    assert!(
        rendered
            .iter()
            .any(|command| { matches!(command, PaintCommand::PushClip(rect) if *rect == tabs) })
    );
}
/// `Delete` apaga para a frente, e chega ao editor.
///
/// Ela não produz texto: o caminho geral do teclado, que só encaminha o que tem
/// texto, a descartava antes de chegar à janela — e por isso não funcionava nem
/// no editor nem no terminal.
#[test]
fn the_delete_key_removes_the_character_ahead() {
    let mut shell = test_shell();
    shell.editor_area.session.open_memory("Demo.java", "abcdef");
    shell.context.focus = ShellFocus::Editor;
    shell.editor_area.pane.set_cursor(2);

    shell.key_down("Delete");
    assert_eq!(shell.active_text(), Some("abdef"));

    // Com seleção, `Delete` tira a seleção inteira, como o `Backspace`.
    shell.editor_area.pane.set_cursor(0);
    for _ in 0..2 {
        shell.key_down_with_modifiers(
            "ArrowRight",
            Modifiers {
                shift: true,
                ..Modifiers::default()
            },
        );
    }
    shell.key_down("Delete");
    assert_eq!(shell.active_text(), Some("def"));
}
/// Clicar no fim da trilha do editor leva ao fim do documento, e arrastar
/// de volta traz o conteúdo junto.
#[test]
fn the_editor_scrollbar_maps_click_and_drag_to_content_offsets() {
    let mut shell = test_shell();
    let text = (0..200)
        .map(|line| format!("linha {line}"))
        .collect::<Vec<_>>()
        .join(
            "
",
        );
    shell.editor_area.session.open_memory("longo.rs", &text);
    let size = Size::new(1280.0, 800.0);
    let track = shell.editor_scrollbar_rect(size);
    let visible = shell.editor_visible_lines();

    shell.pointer_down(
        Point::new(track.origin.x + 5.0, track.origin.y + track.size.height),
        size,
    );
    assert_eq!(shell.editor_scroll_line(), 200 - visible);

    shell.pointer_move(Point::new(track.origin.x + 5.0, track.origin.y), size);
    assert_eq!(shell.editor_scroll_line(), 0);

    shell.pointer_up();
    shell.pointer_move(
        Point::new(track.origin.x + 5.0, track.origin.y + track.size.height),
        size,
    );
    assert_eq!(shell.editor_scroll_line(), 0, "soltar encerra o arraste");
}
#[test]
fn control_click_emits_language_neutral_navigation_request() {
    let mut shell = test_shell();
    let document_id = shell
        .editor_area
        .session
        .open_memory("main.rs", "fn target() {}\n");
    let size = Size::new(1280.0, 800.0);
    let editor_x = ACTIVITY_WIDTH + SIDEBAR_WIDTH;
    let target_x = editor_x + EDITOR_GUTTER + 5.0 * EDITOR_CHAR_WIDTH;
    shell.pointer_down_with_modifiers(
        Point::new(target_x, TITLE_HEIGHT + TAB_HEIGHT + 15.0),
        size,
        true,
        false,
    );
    assert_eq!(
        shell.take_navigation_request(),
        Some(NavigationRequest {
            document_id,
            byte_offset: 5,
            token: "target".to_owned(),
        })
    );
}
/// Ir para a definição rola o editor até o destino.
///
/// Antes o cursor era movido e mais nada: um método declarado abaixo da área
/// visível continuava fora da tela, e a navegação parecia não ter acontecido.
#[test]
fn going_to_a_definition_scrolls_the_target_line_into_view() {
    let mut shell = test_shell();
    let texto = (0..200)
        .map(|linha| format!("linha {linha}"))
        .collect::<Vec<_>>()
        .join(
            "
",
        );
    shell.editor_area.session.open_memory("Longo.java", &texto);
    let size = Size::new(1280.0, 800.0);
    let visiveis = shell.editor_visible_lines();
    assert!(120 > visiveis, "o destino precisa estar fora da tela");
    assert_eq!(shell.editor_scroll_line(), 0);

    assert!(shell.open_location(Path::new("Longo.java"), 120, 0).is_ok());
    // A revelação acontece na pintura, que é onde a altura do editor existe.
    let _ = shell.paint(size);

    let topo = shell.editor_scroll_line();
    assert!(
        topo <= 120 && 120 < topo + visiveis,
        "a linha 120 precisa ficar visível; topo={topo}, visíveis={visiveis}"
    );
    // E rolou o mínimo necessário, não saltou para o fim do arquivo.
    assert!(topo > 0);
}
/// A linha de destino fica destacada até o cursor sair de lá.
#[test]
fn the_navigated_line_is_highlighted_until_the_cursor_moves() {
    let mut shell = test_shell();
    let texto = (0..60)
        .map(|linha| format!("linha {linha}"))
        .collect::<Vec<_>>()
        .join(
            "
",
        );
    shell.editor_area.session.open_memory("Longo.java", &texto);
    let size = Size::new(1280.0, 800.0);
    let destaque = Theme::default().colors.highlight;
    let destacadas = |shell: &mut IdeShell| {
        shell
            .paint(size)
            .iter()
            .filter(
                |command| matches!(command, PaintCommand::FillRect(fill) if fill.color == destaque),
            )
            .count()
    };
    assert_eq!(destacadas(&mut shell), 0, "sem navegação, nada destacado");

    assert!(shell.open_location(Path::new("Longo.java"), 30, 0).is_ok());
    assert_eq!(destacadas(&mut shell), 1, "o destino fica destacado");

    // Clicar em outro lugar tira o destaque, sem ninguém precisar apagá-lo.
    let editor_x = ACTIVITY_WIDTH + SIDEBAR_WIDTH;
    shell.pointer_down(
        Point::new(
            editor_x + EDITOR_GUTTER + 2.0 * EDITOR_CHAR_WIDTH,
            TITLE_HEIGHT + TAB_HEIGHT + 3.0 * EDITOR_LINE_HEIGHT + 5.0,
        ),
        size,
    );
    assert_eq!(
        destacadas(&mut shell),
        0,
        "mover o cursor encerra o destaque"
    );
}
#[test]
fn open_location_opens_file_and_positions_cursor() {
    let mut shell = test_shell();
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
    assert!(shell.open_location(&path, 1, 3).is_ok());
    let position = line_column(
        shell.active_text().unwrap_or_default(),
        shell.editor_area.pane.cursor(),
    );
    assert_eq!(position, (1, 3));
    assert_eq!(shell.focus(), ShellFocus::Editor);
}
/// O `Shift` das setas chega ao editor pela shell.
///
/// O defeito estava no despacho: as setas eram enviadas com modificadores
/// vazios, e o editor — que sempre soube estender — nunca via o `Shift`.
#[test]
fn shift_arrows_reach_the_editor_through_the_shell() {
    let mut shell = shell_editing("primeiro\nsegundo");
    shell.editor_area.pane.set_cursor(0);
    let com_shift = Modifiers {
        shift: true,
        ..Modifiers::default()
    };
    shell.key_down_with_modifiers("ArrowRight", com_shift);
    shell.key_down_with_modifiers("ArrowRight", com_shift);
    assert_eq!(
        shell.editor_area.pane.selection_range(),
        Some(0..2),
        "as setas com Shift precisam marcar"
    );
    shell.key_down_with_modifiers("ArrowDown", com_shift);
    assert!(
        shell
            .editor_area
            .pane
            .selection_range()
            .is_some_and(|range| range.end > 2),
        "a seleção cresce pela mesma âncora"
    );
    shell.key_down("ArrowRight");
    assert_eq!(shell.editor_area.pane.selection_range(), None);
}
/// O `Ctrl` das setas laterais também precisa atravessar o shell.
///
/// É a mesma costura do `Shift`: o painel sabe saltar de palavra em palavra,
/// mas de nada adianta se o modificador se perder no caminho.
#[test]
fn control_arrows_reach_the_editor_through_the_shell() {
    let mut shell = shell_editing("int total = valor;");
    shell.editor_area.pane.set_cursor(0);
    let com_control = Modifiers {
        control: true,
        ..Modifiers::default()
    };
    shell.key_down_with_modifiers("ArrowRight", com_control);
    assert_eq!(shell.editor_area.pane.cursor(), 3, "parou no fim de `int`");
    shell.key_down_with_modifiers("ArrowRight", com_control);
    assert_eq!(shell.editor_area.pane.cursor(), 9, "e no fim de `total`");
    shell.key_down_with_modifiers("ArrowLeft", com_control);
    assert_eq!(shell.editor_area.pane.cursor(), 4, "voltou ao começo dele");
}
/// Gerar muda a revisão do documento, que é o que pede realce novo.
///
/// Sem isso o código gerado aparecia sem cor até a primeira tecla: o realce
/// é invalidado pela revisão, e ninguém pedia um novo depois do clique.
#[test]
fn generating_changes_the_revision_so_the_highlight_is_asked_again() {
    let mut shell = shell_editing("class Matricula {\n}\n");
    let size = Size::new(1280.0, 800.0);
    let antes = shell.active_revision();
    shell.show_accessor_plan(
        AccessorKind::Getter,
        AccessorPlan {
            candidates: vec![AccessorCandidate {
                field: "id".to_owned(),
                source: Some("\n    public Long getId() { return id; }\n".to_owned()),
            }],
            insert_at: DomainTextPosition { line: 1, column: 0 },
        },
    );
    let (_, todos, _) = {
        // As áreas vêm do arranjo, e o arranjo acontece no quadro.
        let _ = shell.paint(size);
        GenerateSurface::areas(&shell.host)
    };
    shell.pointer_down(
        Point::new(todos.origin.x + 10.0, todos.origin.y + 10.0),
        size,
    );
    assert!(
        shell.active_revision() > antes,
        "escrever no documento precisa avançar a revisão"
    );
}
/// A rolagem vertical é contínua, e não de linha em linha.
///
/// Meio passo de roda — o que um touchpad manda o tempo todo — precisa
/// mover meia linha. Arredondar para linha inteira é o que fazia o texto
/// saltar a cada passo em vez de deslizar.
#[test]
fn the_vertical_scroll_moves_by_pixels_instead_of_whole_lines() {
    let mut shell = test_shell();
    shell.editor_area.session.open_memory(
        "long.rs",
        (0..200)
            .map(|line| format!("line {line}"))
            .collect::<Vec<_>>()
            .join("\n"),
    );
    let size = Size::new(1280.0, 800.0);
    let ponto = Point::new(ACTIVITY_WIDTH + SIDEBAR_WIDTH + 100.0, 200.0);

    shell.scroll(ponto, 0.5, size);
    let meia = shell.editor_area.pane.scroll_offset();
    assert!(
        (meia - EDITOR_LINE_HEIGHT / 2.0).abs() < 0.01,
        "meio passo move meia linha, e não uma inteira nem nenhuma: {meia}"
    );

    // Somando meios passos chega-se a uma linha inteira, sem perder resto.
    shell.scroll(ponto, 0.5, size);
    assert!(
        (shell.editor_area.pane.scroll_offset() - EDITOR_LINE_HEIGHT).abs() < 0.01,
        "as frações se somam em vez de serem descartadas"
    );
    assert_eq!(shell.editor_scroll_line(), 1);

    // Rolar para trás não passa do topo.
    shell.scroll(ponto, -50.0, size);
    assert_eq!(shell.editor_area.pane.scroll_offset(), 0.0);
}
/// A barra lateral do editor só existe quando alguma linha passa da área.
///
/// Ela fica rente à borda de baixo, onde também está a borda do terminal.
/// Uma barra desenhada sempre tomaria aquele clique sem ter o que rolar.
#[test]
fn the_editor_gets_a_horizontal_scrollbar_only_when_a_line_overflows() {
    let size = Size::new(1280.0, 800.0);
    let mut curto = shell_editing("int total = 10;");
    let _ = curto.paint(size);
    assert!(
        !curto.editor_scrolls_sideways(size),
        "linha curta não pede barra lateral"
    );

    let mut longo = shell_editing(&"x".repeat(4_000));
    let comandos = longo.paint(size);
    assert!(
        longo.editor_scrolls_sideways(size),
        "linha comprida precisa de barra lateral"
    );
    let trilha = longo.editor_horizontal_scrollbar_rect(size);
    assert!(
        comandos.iter().any(|command| matches!(
            command,
            PaintCommand::FillRect(fill) if fill.rect.origin.y >= trilha.origin.y
                && fill.rect.size.height <= trilha.size.height + 0.01
        )),
        "a trilha precisa ser desenhada"
    );

    // Arrastar a barra rola o editor de lado.
    let ponto = Point::new(
        trilha.origin.x + trilha.size.width / 2.0,
        trilha.origin.y + trilha.size.height / 2.0,
    );
    longo.pointer_down(ponto, size);
    let apos_clique = longo.editor_area.pane.scroll_x();
    assert!(
        apos_clique > 0.0,
        "clicar na trilha leva o editor para o trecho correspondente"
    );

    // O quadro seguinte não pode desfazer o que a barra fez: revelar o
    // cursor a cada pintura anulava o clique e o arrasto.
    let _ = longo.paint(size);
    assert_eq!(
        longo.editor_area.pane.scroll_x(),
        apos_clique,
        "pintar de novo não devolve a vista ao cursor"
    );

    // Arrastar continua movendo, e para além do clique.
    longo.pointer_move(Point::new(ponto.x + trilha.size.width / 4.0, ponto.y), size);
    let apos_arrasto = longo.editor_area.pane.scroll_x();
    assert!(apos_arrasto > apos_clique, "o arrasto continua rolando");
    longo.pointer_up();
    let _ = longo.paint(size);
    assert_eq!(longo.editor_area.pane.scroll_x(), apos_arrasto);

    // Mover o cursor, sim, traz a vista para ele — é o que faz digitar no
    // fim de uma linha comprida não escrever fora da tela.
    longo.editor_area.pane.set_cursor(3_900);
    let _ = longo.paint(size);
    let no_cursor = longo.editor_area.pane.scroll_x();
    assert!(
        no_cursor > apos_arrasto,
        "a vista acompanha o cursor levado para o fim da linha"
    );
    longo.editor_area.pane.set_cursor(0);
    let _ = longo.paint(size);
    assert_eq!(
        longo.editor_area.pane.scroll_x(),
        0.0,
        "e volta ao começo quando o cursor volta"
    );
}
/// O duplo clique seleciona a palavra, e a regra vem do editor da biblioteca.
#[test]
fn a_double_click_selects_the_word_under_the_pointer() {
    let mut shell = shell_editing("int total = 10;");
    let size = Size::new(1280.0, 800.0);
    // Coluna 6 cai no meio de `total`.
    shell.select_word_at_point(editor_column(&shell, size, 6), size);
    assert_eq!(shell.editor_area.pane.selection_range(), Some(4..9));
    assert_eq!(
        shell
            .active_text()
            .and_then(|text| text.get(4..9))
            .map(str::to_owned),
        Some("total".to_owned())
    );
}
/// Copiar leva o trecho para a área de transferência; colar o traz de volta.
#[test]
fn copying_and_pasting_go_through_the_clipboard() {
    let clipboard = Arc::new(FakeClipboard::default());
    let mut shell = shell_editing("total");
    shell.set_clipboard(clipboard.clone());
    shell.editor_area.pane.set_cursor(5);
    shell.editor_area.pane.set_selection(Some((0, 5)));
    assert!(shell.copy_selection());
    assert_eq!(
        clipboard.get_text().unwrap_or_default(),
        Some("total".to_owned())
    );

    shell.editor_area.pane.set_selection(None);
    shell.editor_area.pane.set_cursor(5);
    assert!(shell.paste_clipboard());
    assert_eq!(shell.active_text(), Some("totaltotal"));
}
/// Colar sobre uma seleção troca o trecho marcado.
#[test]
fn pasting_over_a_selection_replaces_it() {
    let clipboard = Arc::new(FakeClipboard::default());
    assert!(clipboard.set_text("novo").is_ok());
    let mut shell = shell_editing("abcdef");
    shell.set_clipboard(clipboard);
    shell.editor_area.pane.set_cursor(4);
    shell.editor_area.pane.set_selection(Some((1, 4)));
    assert!(shell.paste_clipboard());
    assert_eq!(shell.active_text(), Some("anovoef"));
}
/// Sem área de transferência, copiar avisa em vez de fingir que copiou.
#[test]
fn copying_without_a_clipboard_reports_it() {
    let mut shell = shell_editing("total");
    shell.editor_area.pane.set_selection(Some((0, 5)));
    assert!(!shell.copy_selection());
    assert_eq!(shell.status_message(), "Área de transferência indisponível");
}
/// O clique direito no editor abre o menu de copiar e colar; sem seleção,
/// copiar aparece desabilitado.
#[test]
fn the_editor_context_menu_offers_copy_and_paste() {
    let mut shell = shell_editing("total");
    let size = Size::new(1280.0, 800.0);
    shell.secondary_pointer_down(editor_column(&shell, size, 2), size);
    assert!(shell.context_menu_open());
    let entries = shell.context_menu.menu.entries();
    assert_eq!(entry_labels(entries), vec!["Copiar", "Colar"]);
    let copy_enabled = |entries: &[MenuEntry]| match &entries[0] {
        MenuEntry::Item(item) => item.enabled,
        MenuEntry::Separator | MenuEntry::Submenu { .. } => false,
    };
    assert!(!copy_enabled(entries), "sem seleção não há o que copiar");

    shell.editor_area.pane.set_selection(Some((0, 5)));
    shell.secondary_pointer_down(editor_column(&shell, size, 2), size);
    assert!(copy_enabled(shell.context_menu.menu.entries()));
}
/// A lista acompanha o nome sendo digitado, e não só o ponto que a abriu.
///
/// Sem isto, ela mostrava o que valia no instante em que abriu e só se
/// atualizava com `Ctrl+Space` — digitar `se` depois de `a.` deixava a lista
/// parada.
#[test]
fn typing_after_the_dot_asks_for_the_list_again() {
    let mut shell = shell_editing("int total = 10;");
    let item = |label: &str| CompletionItem {
        label: label.to_owned(),
        detail: None,
        kind: ide_domain::CompletionKind::Method,
    };

    // Fechada, digitar não abre nada: abrir é do ponto ou do Ctrl+Space.
    assert!(!shell.completion_follow_up("s"));

    shell.set_completions(vec![item("setId()"), item("setNome()")]);
    assert!(shell.completion_follow_up("s"), "cada letra refaz o filtro");
    assert!(shell.completion_open(), "e a lista continua à mostra");

    // O que não faz parte de um nome encerra o nome.
    assert!(!shell.completion_follow_up("("));
    assert!(!shell.completion_open(), "a lista sai junto com o nome");

    // A resposta vazia do provider também fecha: nada casa com o prefixo.
    shell.set_completions(vec![item("setId()")]);
    shell.set_completions(Vec::new());
    assert!(!shell.completion_open());
}
/// As setas verticais movem o cursor entre linhas, preservando a coluna.
#[test]
fn vertical_arrows_move_the_cursor_between_lines() {
    let mut shell = shell_editing(
        "primeira
segunda
ab",
    );
    shell.editor_area.pane.set_cursor(4);
    shell.key_down("ArrowDown");
    // Mesma coluna na linha de baixo.
    assert_eq!(
        shell.editor_area.pane.cursor(),
        "primeira
"
        .len()
            + 4
    );

    // Descer para uma linha curta para no fim dela, e não num ponto inexistente.
    shell.key_down("ArrowDown");
    assert_eq!(
        shell.editor_area.pane.cursor(),
        "primeira
segunda
ab"
        .len()
    );

    // Na última linha, descer de novo não faz nada.
    shell.key_down("ArrowDown");
    assert_eq!(
        shell.editor_area.pane.cursor(),
        "primeira
segunda
ab"
        .len()
    );

    shell.key_down("ArrowUp");
    assert_eq!(
        shell.editor_area.pane.cursor(),
        "primeira
"
        .len()
            + 2
    );
}
/// Shift com as setas verticais estende a seleção por linhas.
#[test]
fn shift_with_vertical_arrows_extends_the_selection() {
    let mut shell = shell_editing(
        "um
dois
tres",
    );
    shell.editor_area.pane.set_cursor(0);
    let shift = Modifiers {
        shift: true,
        ..Modifiers::default()
    };
    shell.key_down_with_modifiers("ArrowDown", shift);
    assert_eq!(shell.editor_area.pane.selection_range(), Some(0..3));
}
/// Arrastar no editor seleciona, e digitar substitui o trecho marcado.
#[test]
fn dragging_in_the_editor_selects_and_typing_replaces() {
    let mut shell = shell_editing("abcdef");
    let size = Size::new(1280.0, 800.0);
    shell.pointer_down(editor_column(&shell, size, 1), size);
    shell.pointer_move(editor_column(&shell, size, 4), size);
    shell.pointer_up();
    assert_eq!(shell.editor_area.pane.selection_range(), Some(1..4));

    shell.text_input("Z");
    assert_eq!(shell.active_text(), Some("aZef"));
    assert_eq!(shell.editor_area.pane.selection_range(), None);
}
/// A seleção chega ao editor da biblioteca, que é quem a desenha.
#[test]
fn the_selection_is_painted_by_the_library_editor() {
    let mut shell = shell_editing("abcdef");
    let size = Size::new(1280.0, 800.0);
    shell.pointer_down(editor_column(&shell, size, 1), size);
    shell.pointer_move(editor_column(&shell, size, 4), size);
    shell.pointer_up();
    let selection = shell.context.theme.colors.selection;
    assert!(
        shell.paint(size).iter().any(|command| matches!(
            command,
            PaintCommand::FillRect(fill) if fill.color == selection
        )),
        "o trecho selecionado precisa aparecer pintado"
    );
}
/// Shift+setas estende a seleção; sem Shift, mover desfaz.
#[test]
fn shift_arrows_extend_the_selection() {
    let mut shell = shell_editing("abcdef");
    let shift = Modifiers {
        shift: true,
        ..Modifiers::default()
    };
    shell.key_down_with_modifiers("ArrowRight", shift);
    shell.key_down_with_modifiers("ArrowRight", shift);
    assert_eq!(shell.editor_area.pane.selection_range(), Some(0..2));

    shell.key_down("ArrowRight");
    assert_eq!(shell.editor_area.pane.selection_range(), None);
}
/// Backspace com trecho marcado apaga o trecho, não um caractere.
#[test]
fn backspace_removes_the_selection() {
    let mut shell = shell_editing("abcdef");
    shell.editor_area.pane.set_cursor(4);
    shell.editor_area.pane.set_selection(Some((1, 4)));
    shell.key_down("Backspace");
    assert_eq!(shell.active_text(), Some("aef"));
    assert_eq!(shell.editor_area.pane.cursor(), 1);
}
/// Abrir um par escreve o fechamento e deixa o cursor entre os dois.
#[test]
fn abrir_um_par_escreve_o_fechamento() {
    let mut shell = shell_editing("");
    shell.text_input("(");
    assert_eq!(shell.active_text(), Some("()"));
    assert_eq!(
        shell.editor_area.pane.cursor(),
        1,
        "o cursor fica entre os dois"
    );
}
/// Apagar o abridor leva o fechamento junto, enquanto os dois estão na linha.
#[test]
fn apagar_o_abridor_leva_o_fechamento_da_mesma_linha() {
    let mut shell = shell_editing("f(a, b)");
    shell.editor_area.pane.set_cursor(2);
    shell.key_down("Backspace");
    assert_eq!(shell.active_text(), Some("fa, b"));
    assert_eq!(shell.editor_area.pane.cursor(), 1);
}
/// Depois que o `Enter` separou os dois, o fechamento fica onde está.
///
/// Ele deixou de ser o eco de uma tecla e virou o fim de um bloco: levá-lo
/// junto tiraria o fechamento de um corpo que já tem conteúdo.
#[test]
fn o_fechamento_de_outra_linha_nao_vai_junto() {
    let mut shell = shell_editing("if (x) {\n    faz();\n}");
    shell.editor_area.pane.set_cursor(8);
    shell.key_down("Backspace");
    assert_eq!(shell.active_text(), Some("if (x) \n    faz();\n}"));
}
/// O apóstrofo no meio de uma palavra não abre string nenhuma.
#[test]
fn o_apostrofo_de_uma_palavra_nao_vira_par() {
    let mut shell = shell_editing("// don");
    shell.editor_area.pane.set_cursor(6);
    shell.text_input("'");
    assert_eq!(shell.active_text(), Some("// don'"), "e não `// don''`");
}
/// `Enter` depois de uma aspa não abre bloco: partiria a string no meio.
#[test]
fn a_aspa_nao_abre_bloco() {
    let mut shell = shell_editing("  const a = ");
    shell.editor_area.pane.set_cursor(12);
    shell.text_input("'");
    assert_eq!(shell.active_text(), Some("  const a = ''"));

    shell.key_down("Enter");
    assert_eq!(
        shell.active_text(),
        Some("  const a = '\n  '"),
        "a linha nova herda a indentação, e não ganha um nível"
    );
}
/// Abrir a chave e apertar `Enter` abre o bloco em três linhas.
///
/// A linha em branco fica um nível mais fundo que a linha da abertura, o
/// fechamento volta a alinhar com ela, e o cursor fica no fim da linha em
/// branco — que é onde se vai escrever.
#[test]
fn o_enter_depois_da_chave_abre_o_bloco() {
    let mut shell = shell_editing("class A {\n  metodo() \n}");
    shell.editor_area.pane.set_cursor(21);
    shell.text_input("{");
    assert_eq!(shell.active_text(), Some("class A {\n  metodo() {}\n}"));

    shell.key_down("Enter");
    assert_eq!(
        shell.active_text(),
        Some("class A {\n  metodo() {\n    \n  }\n}")
    );
    assert_eq!(
        shell.editor_area.pane.cursor(),
        27,
        "o cursor fica no fim da linha em branco"
    );
}
/// O passo da indentação é o do arquivo, e não o da IDE.
#[test]
fn o_passo_da_indentacao_e_o_do_arquivo() {
    let mut shell = shell_editing("class A {\n    metodo() {}\n}");
    shell.editor_area.pane.set_cursor(24);
    shell.key_down("Enter");
    assert_eq!(
        shell.active_text(),
        Some("class A {\n    metodo() {\n        \n    }\n}"),
        "num arquivo de quatro espaços o degrau é de quatro"
    );

    let mut tabulado = shell_editing("class A {\n\tmetodo() {}\n}");
    tabulado.editor_area.pane.set_cursor(21);
    tabulado.key_down("Enter");
    assert_eq!(
        tabulado.active_text(),
        Some("class A {\n\tmetodo() {\n\t\t\n\t}\n}"),
        "num arquivo tabulado o degrau é uma tabulação"
    );
}
/// Sem o fechamento encostado, o `Enter` só abre a linha indentada.
#[test]
fn sem_o_fechamento_encostado_o_enter_so_indenta() {
    let mut shell = shell_editing("class A {\n  metodo() {\n}");
    shell.editor_area.pane.set_cursor(22);
    shell.key_down("Enter");
    assert_eq!(
        shell.active_text(),
        Some("class A {\n  metodo() {\n    \n}")
    );
}
/// Fora de um abridor, `Enter` continua herdando a indentação da linha.
#[test]
fn fora_do_par_o_enter_continua_como_era() {
    let mut shell = shell_editing("class A {\n  faz();\n}");
    shell.editor_area.pane.set_cursor(18);
    shell.key_down("Enter");
    assert_eq!(shell.active_text(), Some("class A {\n  faz();\n  \n}"));
}
/// Salvar grava o conteúdo da aba e limpa a marca de modificado.
#[test]
fn saving_writes_the_active_tab_to_disk() {
    let root = std::env::temp_dir().join(format!("er-ide-save-{}", std::process::id()));
    assert!(std::fs::create_dir_all(&root).is_ok());
    let file = root.join("Pedido.java");
    assert!(std::fs::write(&file, "class Pedido {}").is_ok());
    let Ok(mut shell) = IdeShell::open(&root) else {
        panic!("workspace de teste não abriu");
    };
    let Ok(_) = shell.open_file(&file) else {
        panic!("arquivo de teste não abriu");
    };
    shell.context.focus = ShellFocus::Editor;
    shell.editor_area.pane.set_cursor(0);
    shell.text_input("// nota\n");
    assert!(
        shell.active_document_modified(),
        "a edição deixa a aba suja"
    );

    assert!(shell.save_active_document());
    assert_eq!(
        std::fs::read_to_string(&file).unwrap_or_default(),
        "// nota\nclass Pedido {}"
    );
    assert!(
        !shell.active_document_modified(),
        "depois de gravar a aba deixa de estar suja"
    );
    assert!(shell.status_message().starts_with("Salvo "));
    let _ = std::fs::remove_dir_all(&root);
}
/// Clicar no campo move o cursor, e o que se digita entra ali.
///
/// O clique é entregue ao componente, que conhece a medição da fonte; a IDE
/// não tenta adivinhar em que caractere o ponteiro caiu.
#[test]
fn clicking_a_field_moves_the_cursor_before_typing() {
    let mut shell = shell_with_package();
    let size = Size::new(1_000.0, 700.0);
    shell.context_menu.alvo = Some(AlvoDoMenu::Explorer {
        pasta: PathBuf::from("demo/src/main/java/br/com"),
        arquivo: None,
    });
    shell.run_context_command("explorer.new.java.package");
    // A área vem do arranjo, e o arranjo acontece no quadro: sem desenhar um,
    // a janela recém-aberta ainda não tem onde cada campo fica.
    let _ = shell.paint(size);
    let package = NewItemSurface::field_area(&shell.host, true);
    // Clique antes do primeiro caractere leva o cursor para o começo.
    shell.pointer_down(
        Point::new(package.origin.x + 1.0, package.origin.y + 8.0),
        size,
    );
    shell.text_input("dev.");
    shell.key_down("Enter");
    assert_eq!(
        shell.take_new_item_request().map(|request| request.package),
        Some("dev.br.com".to_owned())
    );
}
/// `Tab` sem `Ctrl` continua sendo do editor.
///
/// Sem esta separação, indentar viraria troca de aba — e é o mesmo caractere
/// chegando do sistema nos dois casos.
#[test]
fn tab_without_control_still_belongs_to_the_editor() {
    let root = PathBuf::from("workspace");
    let mut shell = IdeShell::from_tree(FileNode {
        path: root.clone(),
        is_directory: true,
        children: Vec::new(),
    });
    for nome in ["Primeiro.java", "Segundo.java"] {
        shell
            .editor_area
            .session
            .open_memory(root.join(nome), "class A {}");
    }
    let antes = shell
        .editor_area
        .session
        .active()
        .map(|documento| documento.path.clone());
    shell.key_down_with_modifiers("Tab", Modifiers::default());
    assert!(
        !shell.tab_switcher.is_open(),
        "Tab sozinho nao abre a troca de abas"
    );
    assert_eq!(
        shell
            .editor_area
            .session
            .active()
            .map(|documento| documento.path.clone()),
        antes
    );
}
