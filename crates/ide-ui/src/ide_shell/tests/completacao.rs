//! Testes da **lista de completação**.

use super::*;

#[test]
fn completion_popup_can_apply_selected_item() {
    let mut shell = test_shell();
    shell.editor_area.session.open_memory("Example.java", "Exa");
    shell.context.focus = ShellFocus::Editor;
    shell.editor_area.pane.set_cursor(3);
    shell.set_completions(vec![CompletionItem {
        label: "Example".to_owned(),
        detail: Some("class".to_owned()),
        kind: ide_domain::CompletionKind::Class,
    }]);
    assert!(shell.paint(Size::new(1280.0, 800.0)).iter().any(|command| {
        matches!(command, PaintCommand::DrawText(text) if text.text == "Example")
    }));
    shell.key_down("Enter");
    assert_eq!(shell.active_text(), Some("Example"));
}
/// Clicar fora dispensa a lista; clicar dentro escolhe a linha.
///
/// Uma lista que sobrevive ao clique fica pairando sobre um cursor que já se
/// moveu. E o clique dentro dela precisa ser consumido de qualquer forma, ou
/// atravessaria a lista e moveria o cursor no editor de baixo.
#[test]
fn clicking_outside_the_completion_list_dismisses_it() {
    let mut shell = shell_editing("int total = 10;");
    let size = Size::new(1280.0, 800.0);
    let item = |label: &str| CompletionItem {
        label: label.to_owned(),
        detail: None,
        kind: ide_domain::CompletionKind::Method,
    };
    shell.context.focus = ShellFocus::Editor;
    shell.set_completions(vec![item("getAluno()"), item("getId()")]);
    let rect = shell
        .completion_rect(size)
        .unwrap_or_else(|| panic!("a lista aberta precisa ocupar uma área"));

    // Um ponto claramente fora: o canto oposto da janela.
    shell.pointer_down(Point::new(rect.origin.x - 40.0, rect.origin.y - 40.0), size);
    assert!(!shell.completion_open(), "a lista sai com o clique de fora");

    // Reaberta, o clique na segunda linha escolhe aquele item.
    shell.editor_area.pane.set_cursor(0);
    shell.set_completions(vec![item("getAluno()"), item("getId()")]);
    let rect = shell
        .completion_rect(size)
        .unwrap_or_else(|| panic!("a lista aberta precisa ocupar uma área"));
    shell.pointer_down(
        Point::new(
            rect.origin.x + 20.0,
            rect.origin.y + COMPLETION_POPUP_PADDING + COMPLETION_ROW_HEIGHT + 4.0,
        ),
        size,
    );
    assert!(!shell.completion_open(), "escolher também fecha a lista");
    assert_eq!(
        shell.active_text(),
        Some("getId()int total = 10;"),
        "o item clicado é o que entra no texto"
    );
}
/// Os nomes da lista saem na cor de texto do tema.
///
/// É a cor escolhida para se ler sobre a superfície, e a mesma do resto da
/// interface — trocar o tema troca também o que a lista mostra.
#[test]
fn the_completion_list_paints_its_names_with_the_theme_text() {
    let mut shell = shell_editing("int total = 10;");
    let size = Size::new(1280.0, 800.0);
    shell.context.focus = ShellFocus::Editor;
    shell.set_completions(vec![CompletionItem {
        label: "getAluno()".to_owned(),
        detail: None,
        kind: ide_domain::CompletionKind::Method,
    }]);
    let colors: Vec<Color> = shell
        .paint(size)
        .into_iter()
        .filter_map(|command| match command {
            PaintCommand::DrawText(text) if text.text == "getAluno()" => Some(text.color),
            _ => None,
        })
        .collect();
    assert_eq!(
        colors,
        vec![Theme::dark().colors.text],
        "o nome sai na cor de texto do tema"
    );
}
/// Com a lista aberta na inspeção, as setas andam nela e Enter aceita.
#[test]
fn the_completion_list_takes_the_keys_inside_the_inspection() {
    let mut shell = shell_editing("int total = 10;");
    shell.debug_panel.view.attached = true;
    shell.show_inspection("m", inspection_value(), inspection_fields());
    shell.inspection.focus_source();
    shell.inspection.set_source("m.");
    shell.inspection.editor_and_source().0.set_cursor(2);
    shell.set_completions(vec![
        CompletionItem {
            label: "getCliente()".to_owned(),
            detail: None,
            kind: ide_domain::CompletionKind::Method,
        },
        CompletionItem {
            label: "total".to_owned(),
            detail: None,
            kind: ide_domain::CompletionKind::Field,
        },
    ]);

    shell.key_down("ArrowDown");
    assert_eq!(shell.inspection_source(), "m.", "a seta andou na lista");
    shell.key_down("Enter");
    assert_eq!(
        shell.inspection_source(),
        "m.total",
        "aceitar escreve no editor da inspeção"
    );
    assert_eq!(
        shell.active_text(),
        Some("int total = 10;"),
        "o documento atrás da janela não é tocado"
    );
}
