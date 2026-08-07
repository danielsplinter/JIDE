//! Testes da **depuração** e da **inspeção**.

use super::*;

/// As camadas nascem com o shell, na ordem em que se sobrepõem.
///
/// Enquanto o arranjo vem do consumidor, a ordem de sobreposição é a das
/// chamadas de `place`; quando vier do motor, será a da árvore. Este teste
/// prende as duas à mesma ordem — sem ele, a primeira janela a adotar o motor
/// herdaria a ordem em que as janelas foram abertas, que é acidental.
#[test]
fn the_layers_are_declared_in_the_order_they_stack() {
    let shell = test_shell();
    let filhos = shell.host.children(SHELL_ROOT_ID);
    let esperada: Vec<_> = OVERLAY
        .into_iter()
        .map(|layer| match layer {
            Layer::Surface(kind) => surface_layer_id(kind),
            Layer::Completion => COMPLETION_POPUP_ID,
        })
        .collect();
    let camadas: Vec<_> = filhos
        .iter()
        .copied()
        .filter(|id| esperada.contains(id))
        .collect();
    assert_eq!(
        camadas, esperada,
        "as camadas fora da ordem em que se cobrem"
    );

    // E a moldura vem antes de todas elas, porque é o que elas cobrem.
    let posicao = |procurado: WidgetId| filhos.iter().position(|id| *id == procurado);
    assert!(
        posicao(EDITOR_TABS_ID) < posicao(esperada[0]),
        "a moldura tem que ser declarada antes da primeira camada"
    );
}
/// A calha mostra a diferença entre pedido e confirmado, e a linha parada
/// é destacada inteira.
#[test]
fn the_gutter_shows_pending_and_confirmed_breakpoints_and_the_stopped_line() {
    let mut shell = test_shell();
    shell
        .editor_area
        .session
        .open_memory("A.java", "um\ndois\ntres\nquatro");
    let path = PathBuf::from("A.java");
    let size = Size::new(1280.0, 800.0);
    shell.toggle_breakpoint(&path, 1);
    // Só as marcas da calha interessam. A faixa é a da calha mesmo — entre a
    // barra lateral e o texto —, e não "tudo à esquerda dela": a barra de
    // atividades também desenha círculos, no ícone de busca.
    let inicio = ACTIVITY_WIDTH + SIDEBAR_WIDTH;
    let gutter = inicio + EDITOR_GUTTER;
    let na_calha = move |x: f32| x >= inicio && x < gutter;
    let circles = |shell: &mut IdeShell| {
        shell
            .paint(size)
            .iter()
            .fold((0, 0), |(filled, outlined), command| match command {
                PaintCommand::FillCircle(circle) if na_calha(circle.center.x) => {
                    (filled + 1, outlined)
                }
                PaintCommand::StrokeCircle(circle) if na_calha(circle.center.x) => {
                    (filled, outlined + 1)
                }
                _ => (filled, outlined),
            })
    };
    assert_eq!(
        circles(&mut shell),
        (0, 1),
        "sem sessão, o ponto é pendente"
    );

    shell.set_verified_breakpoints(&path, &[1]);
    assert_eq!(circles(&mut shell), (1, 0), "confirmado vira disco");

    shell.set_debug_view(DebugView {
        attached: true,
        stopped_at: Some((path, 2)),
        ..DebugView::default()
    });
    let highlight = Theme::default().colors.highlight;
    assert!(
        shell.paint(size).iter().any(|command| matches!(
            command,
            PaintCommand::FillRect(fill) if fill.color == highlight
        )),
        "a linha em execução é destacada"
    );
}
#[test]
fn clicking_the_gutter_toggles_a_breakpoint_and_marks_the_file() {
    let (mut shell, path) = shell_with_java_file();
    let size = Size::new(1280.0, 800.0);
    let geometry = shell.geometry();
    let editor_x = ACTIVITY_WIDTH + shell.sidebar_width(size);
    // Terceira linha visível, dentro da calha.
    let point = Point::new(
        editor_x + 20.0,
        geometry.content_top + 15.0 + 2.0 * EDITOR_LINE_HEIGHT + 2.0,
    );

    shell.pointer_down(point, size);
    assert_eq!(shell.breakpoints_for(&path), vec![2]);
    assert_eq!(
        shell.take_breakpoints_dirty().as_deref(),
        Some(path.as_path())
    );
    assert_eq!(shell.breakpoint_count(), 1);
    assert!(
        shell
            .paint(size)
            .iter()
            .any(|command| matches!(command, PaintCommand::StrokeCircle(_))),
        "sem confirmação do alvo, o marcador aparece apenas como contorno"
    );

    shell.set_verified_breakpoints(&path, &[2]);
    assert!(shell.breakpoint_is_verified(&path, 2));
    assert!(
        shell
            .paint(size)
            .iter()
            .any(|command| matches!(command, PaintCommand::FillCircle(_))),
        "confirmado pelo alvo, o marcador fica cheio"
    );

    shell.pointer_down(point, size);
    assert!(shell.breakpoints_for(&path).is_empty());
    assert_eq!(shell.breakpoint_count(), 0);
}
#[test]
fn debug_panel_shows_stack_and_variables_and_selects_a_frame() {
    let (mut shell, path) = shell_with_java_file();
    let size = Size::new(1280.0, 800.0);
    shell.set_debug_view(DebugView {
        attached: true,
        status: "Parado em Main.run".to_owned(),
        stopped_at: Some((path.clone(), 2)),
        frames: vec![
            DebugFrameView {
                name: "Main.run".to_owned(),
                location: Some((path.clone(), 2)),
            },
            DebugFrameView {
                name: "Main.main".to_owned(),
                location: Some((path, 3)),
            },
        ],
        selected_frame: 0,
        variables: vec![DebugVariableView {
            name: "total".to_owned(),
            value: "1".to_owned(),
            type_name: None,
            expandable: false,
        }],
    });

    let texts: Vec<String> = shell
        .paint(size)
        .iter()
        .filter_map(|command| match command {
            PaintCommand::DrawText(text) => Some(text.text.clone()),
            _ => None,
        })
        .collect();
    assert!(texts.iter().any(|text| text == "Parado em Main.run"));
    assert!(texts.iter().any(|text| text.starts_with("Main.run:3")));
    assert!(texts.iter().any(|text| text == "total = 1"));
    assert!(texts.iter().any(|text| text == "Pilha de chamadas"));

    let panel = {
        let _ = shell.paint(size);
        shell.debug_panel_geometry()
    };
    shell.pointer_down(
        Point::new(
            panel.panel.origin.x + 40.0,
            panel.frames.origin.y + DEBUG_ROW_HEIGHT + 2.0,
        ),
        size,
    );
    assert_eq!(
        shell.take_debug_requests(),
        vec![DebugRequest::SelectFrame(1)]
    );
    assert_eq!(shell.debug_view().selected_frame, 1);

    // Clicar abaixo do último quadro não é escolha de quadro nenhum.
    shell.pointer_down(
        Point::new(
            panel.panel.origin.x + 40.0,
            panel.frames.origin.y + panel.frames.size.height + 6.0,
        ),
        size,
    );
    assert_eq!(shell.take_debug_requests(), vec![]);
    assert_eq!(shell.debug_view().selected_frame, 1);
}
#[test]
fn debug_panel_buttons_and_menu_emit_session_requests() {
    let (mut shell, _) = shell_with_java_file();
    let size = Size::new(1280.0, 800.0);
    // Com o quadro parado: dar um passo só faz sentido aí, e é aí que os
    // botões da faixa aceitam o clique.
    shell.set_debug_view(DebugView {
        attached: true,
        status: "Parado".to_owned(),
        stopped_at: Some((PathBuf::from("Pedido.java"), 3)),
        ..DebugView::default()
    });

    // O painel entra no arranjo com a sessão: sem um quadro, ele ainda não tem
    // lugar, e é do lugar que sai a área dos botões.
    let _ = shell.paint(size);
    let panel = shell.debug_panel_geometry();
    let button = panel.buttons[1];
    shell.pointer_down(
        Point::new(button.origin.x + 4.0, button.origin.y + 4.0),
        size,
    );
    assert_eq!(shell.take_debug_requests(), vec![DebugRequest::StepOver]);

    // Menu `Depurar` → `Continuar`.
    shell.pointer_down(Point::new(280.0, 10.0), size);
    shell.pointer_down(Point::new(280.0, TITLE_HEIGHT + 38.0), size);
    assert_eq!(shell.take_debug_requests(), vec![DebugRequest::Continue]);
}
#[test]
fn the_debug_menu_opens_the_settings_window_on_the_debug_page() {
    let mut shell = test_shell();
    let size = Size::new(1_000.0, 700.0);
    assert_eq!(shell.settings_page(), SettingsPage::Contribution(0));

    // Menu `Depurar` → `Conectar...`.
    shell.pointer_down(Point::new(280.0, 10.0), size);
    shell.pointer_down(Point::new(280.0, TITLE_HEIGHT + 10.0), size);
    assert!(shell.take_open_settings_request());
    assert_eq!(shell.settings_page(), SettingsPage::Debug);

    open_java_settings(&mut shell, vec!["JDK 17".to_owned()], 0);
    let texts: Vec<String> = shell
        .paint(size)
        .iter()
        .filter_map(|command| match command {
            PaintCommand::DrawText(text) => Some(text.text.clone()),
            _ => None,
        })
        .collect();
    assert!(texts.iter().any(|text| text.contains("Host e porta")));
    assert!(!texts.iter().any(|text| text == "JDK"));

    // O atalho do compilador troca a página de volta.
    shell.set_settings_page(SettingsPage::Contribution(0));
    let texts: Vec<String> = shell
        .paint(size)
        .iter()
        .filter_map(|command| match command {
            PaintCommand::DrawText(text) => Some(text.text.clone()),
            _ => None,
        })
        .collect();
    assert!(texts.iter().any(|text| text == "JDK"));
}
#[test]
fn debug_settings_page_validates_the_target_before_connecting() {
    let mut shell = test_shell();
    let size = Size::new(1_000.0, 700.0);
    open_java_settings(&mut shell, vec!["JDK 8".to_owned()], 0);
    let geometry = {
        // A moldura vem do arranjo, e o arranjo acontece no quadro.
        let _ = shell.paint(size);
        shell.settings.geometry(&shell.host)
    };

    // Segunda linha da navegação é a página de Depuração.
    shell.pointer_down(
        Point::new(
            geometry.compiler_option.origin.x + 20.0,
            geometry.compiler_option.origin.y + settings::PAGE_ROW_HEIGHT + 10.0,
        ),
        size,
    );
    let texts: Vec<String> = shell
        .paint(size)
        .iter()
        .filter_map(|command| match command {
            PaintCommand::DrawText(text) => Some(text.text.clone()),
            _ => None,
        })
        .collect();
    assert!(texts.iter().any(|text| text == "Depuração"));
    assert!(texts.iter().any(|text| text.contains("agentlib:jdwp")));

    // As áreas são relidas: os campos de depuração só existem com a página
    // aberta, porque agora a geometria é a do que está na tela — e não uma conta
    // sobre o painel, que respondia por peças invisíveis.
    let geometry = shell.settings.geometry(&shell.host);

    shell.pointer_down(
        Point::new(
            geometry.debug_port.origin.x + 10.0,
            geometry.debug_port.origin.y + 10.0,
        ),
        size,
    );
    shell.key_down("Backspace");
    shell.key_down("Backspace");
    shell.key_down("Backspace");
    shell.key_down("Backspace");
    shell.text_input("porta");
    shell.pointer_down(
        Point::new(
            geometry.debug_attach.origin.x + 10.0,
            geometry.debug_attach.origin.y + 10.0,
        ),
        size,
    );
    assert!(
        shell.take_debug_requests().is_empty(),
        "porta inválida não conecta"
    );
    assert!(shell.settings_dialog_open());

    shell.key_down("Backspace");
    shell.key_down("Backspace");
    shell.key_down("Backspace");
    shell.key_down("Backspace");
    shell.key_down("Backspace");
    shell.text_input("5005");
    shell.key_down("Enter");
    assert_eq!(
        shell.take_debug_requests(),
        vec![DebugRequest::Attach {
            host: "127.0.0.1".to_owned(),
            port: 5005,
        }]
    );
    assert!(!shell.settings_dialog_open());
}
/// A janela abre com o objeto na lista e o detalhe do item destacado.
#[test]
fn the_inspection_window_lists_the_object_and_details_the_selection() {
    let mut shell = shell_editing("int total = 10;");
    let size = Size::new(1280.0, 800.0);
    shell.show_inspection("pedido", inspection_value(), inspection_fields());
    assert!(shell.inspection_open());
    assert_eq!(shell.inspected_expression(), Some("pedido"));

    let texts = painted_texts(&mut shell, size);
    // Painel esquerdo: a raiz aberta, com os campos abaixo dela.
    assert!(
        texts
            .iter()
            .any(|text| { text.contains("pedido = (br.com.exemplo.Pedido) Pedido@1a2b") }),
        "a árvore precisa mostrar o objeto: {texts:?}"
    );
    assert!(
        texts.iter().any(|text| text.contains("total = (int) 42")),
        "a árvore precisa mostrar os campos: {texts:?}"
    );
    // Painel direito: detalhe da raiz, que abre destacada.
    assert!(
        texts.iter().any(|text| text == "br.com.exemplo.Pedido"),
        "o detalhe precisa mostrar o tipo: {texts:?}"
    );
}
/// Sem sessão viva não há onde executar, e a janela diz isso.
///
/// A árvore continua mostrando o que foi lido enquanto a execução estava
/// parada, então sem esse aviso o usuário clicaria em Executar achando que a
/// sessão ainda está de pé.
#[test]
fn running_without_a_live_session_explains_itself() {
    let mut shell = shell_editing("int total = 10;");
    shell.show_inspection("pedido", inspection_value(), inspection_fields());
    shell.inspection.set_source("m.setId(4L);");
    shell.run_inspection_source();
    assert!(shell.take_debug_requests().is_empty());
    assert_eq!(
        shell.inspection.message(),
        Some("A sessão de depuração terminou; reconecte para executar")
    );
}
/// O painel direito reusa o editor, com os comportamentos de arquivo
/// desligados: escrever ali não é editar um arquivo do projeto.
#[test]
fn the_inspection_editor_reuses_the_pane_without_file_behaviours() {
    let mut shell = shell_editing("int total = 10;");
    let size = Size::new(1280.0, 800.0);
    shell.show_inspection("pedido", inspection_value(), inspection_fields());
    let geometry = inspection_layout(&mut shell, size);
    let capabilities = shell.inspection.editor_and_source_ref().0.capabilities();
    assert!(!capabilities.save, "não há arquivo para salvar");
    assert!(!capabilities.navigation, "não há definição para navegar");
    assert!(!capabilities.breakpoint_gutter, "não há linha onde parar");
    assert!(!capabilities.context_menu);

    // Clicar no editor leva o foco e a digitação para lá.
    shell.pointer_down(
        Point::new(
            geometry.source.origin.x + 60.0,
            geometry.source.origin.y + 8.0,
        ),
        size,
    );
    shell.text_input("pedido.total");
    assert_eq!(shell.inspection_source(), "pedido.total");
    // O documento aberto no editor principal não foi tocado.
    assert_eq!(shell.active_text(), Some("int total = 10;"));
}
/// Executar pede a avaliação do que foi digitado.
#[test]
fn running_the_inspection_source_asks_for_its_evaluation() {
    let mut shell = shell_editing("int total = 10;");
    shell.debug_panel.view.attached = true;
    let size = Size::new(1280.0, 800.0);
    shell.show_inspection("pedido", inspection_value(), inspection_fields());
    let geometry = inspection_layout(&mut shell, size);
    shell.pointer_down(
        Point::new(
            geometry.source.origin.x + 60.0,
            geometry.source.origin.y + 8.0,
        ),
        size,
    );
    shell.text_input("pedido.cliente.nome");
    shell.pointer_down(
        Point::new(geometry.run.origin.x + 10.0, geometry.run.origin.y + 10.0),
        size,
    );
    assert_eq!(
        shell.take_debug_requests(),
        vec![DebugRequest::Evaluate("pedido.cliente.nome".to_owned())]
    );
}
/// O resultado da execução não toma o lugar da árvore.
///
/// Executar `pedido.pagar()` devolve `void`; trocar a árvore por isso apagaria
/// justamente o objeto que se queria ver mudar.
#[test]
fn running_keeps_the_tree_and_only_refreshes_its_values() {
    let mut shell = shell_editing("int total = 10;");
    let size = Size::new(1280.0, 800.0);
    shell.debug_panel.view.attached = true;
    shell.show_inspection("pedido", inspection_value(), inspection_fields());
    // Um nível mais fundo fica aberto, para conferir que ele é relido.
    let geometry = inspection_layout(&mut shell, size);
    shell.pointer_down(
        Point::new(
            geometry.list.origin.x + 30.0,
            geometry.list.origin.y + inspection::ROW_HEIGHT * 2.0 + 4.0,
        ),
        size,
    );
    let _ = shell.take_debug_requests();

    shell.inspection.set_source("pedido.pagar()");
    shell.run_inspection_source();
    assert_eq!(
        shell.take_debug_requests(),
        vec![DebugRequest::Evaluate("pedido.pagar()".to_owned())]
    );

    // Chega o retorno da chamada: nada de árvore nova.
    shell.inspection_result("pedido.pagar()".to_owned(), inspection_void(), Vec::new());
    assert_eq!(shell.inspected_expression(), Some("pedido"));
    let texts = painted_texts(&mut shell, size);
    assert!(
        texts
            .iter()
            .any(|text| text.contains("pedido = (br.com.exemplo.Pedido) Pedido@1a2b")),
        "a árvore deveria continuar mostrando o objeto: {texts:?}"
    );
    assert_eq!(
        shell.inspection.message(),
        Some("pedido.pagar() → void"),
        "o retorno aparece na linha de mensagem, não na árvore"
    );

    // A releitura da raiz foi pedida.
    assert_eq!(
        shell.take_debug_requests(),
        vec![DebugRequest::Evaluate("pedido".to_owned())]
    );

    // E ela troca os valores sem fechar o que estava aberto.
    shell.inspection_result(
        "pedido".to_owned(),
        inspection_value(),
        vec![
            DebugVariableView {
                name: "total".to_owned(),
                value: "0".to_owned(),
                type_name: Some("int".to_owned()),
                expandable: false,
            },
            DebugVariableView {
                name: "cliente".to_owned(),
                value: "Cliente@3c4d".to_owned(),
                type_name: Some("br.com.exemplo.Cliente".to_owned()),
                expandable: true,
            },
        ],
    );
    let texts = painted_texts(&mut shell, size);
    assert!(
        texts.iter().any(|text| text.contains("total = (int) 0")),
        "o valor deveria ser o de depois da execução: {texts:?}"
    );
    assert_eq!(
        shell.take_debug_requests(),
        vec![DebugRequest::ExpandInspection("pedido.cliente".to_owned())],
        "o nível aberto abaixo da raiz também precisa ser relido"
    );
}
/// O ponto no editor do depurador pergunta por um tipo, não por uma posição.
///
/// O tipo de `m` só existe no quadro parado — não há fonte que o declare —,
/// mas quem responde pelos membros é o índice do projeto. Por isso a pergunta
/// é o nome do tipo.
#[test]
fn a_dot_in_the_inspection_editor_asks_for_the_members_of_a_type() {
    let mut shell = shell_editing("int total = 10;");
    shell.debug_panel.view.attached = true;
    shell.show_inspection("m", inspection_value(), inspection_fields());
    shell.inspection.set_source("m.");
    shell.inspection.editor_and_source().0.set_cursor(2);
    assert_eq!(
        shell.inspection_member_context(),
        Some(("m.".to_owned(), 2)),
        "a shell entrega texto e cursor sem interpretar a sintaxe"
    );
    assert_eq!(
        shell.inspection_member_target("m", String::new()),
        ("br.com.exemplo.Pedido".to_owned(), String::new()),
        "o tipo vem do objeto parado, e não de declaração nenhuma"
    );

    assert_eq!(
        shell.inspection_member_target("m", "getCli".to_owned()),
        ("br.com.exemplo.Pedido".to_owned(), "getCli".to_owned())
    );
    assert_eq!(
        shell.inspection_member_target("cliente", String::new()),
        ("br.com.exemplo.Cliente".to_owned(), String::new())
    );
    assert_eq!(
        shell.inspection_member_target("Relatorio", String::new()),
        ("Relatorio".to_owned(), String::new()),
        "classe de fora do código depurado precisa ser perguntável"
    );
}
/// Seleção e área de transferência valem no editor da inspeção.
///
/// O painel é o mesmo da janela principal e sempre soube selecionar; o que
/// faltava era a janela encaminhar os gestos até ele.
#[test]
fn the_inspection_editor_selects_and_uses_the_clipboard() {
    let mut shell = shell_editing("int total = 10;");
    let clipboard = Arc::new(FakeClipboard::default());
    shell.set_clipboard(clipboard.clone());
    let size = Size::new(1280.0, 800.0);
    shell.show_inspection("pedido", inspection_value(), inspection_fields());
    shell.inspection.set_source("pedido.total");
    let geometry = inspection_layout(&mut shell, size);
    let column = |index: usize| {
        Point::new(
            geometry.source.origin.x
                + CodeEditor::gutter_width()
                + index as f32 * CodeEditor::default_char_width(),
            geometry.source.origin.y + 4.0,
        )
    };

    // Arrastar marca o trecho.
    shell.pointer_down(column(0), size);
    shell.pointer_move(column(6), size);
    shell.pointer_up();
    assert_eq!(inspection_selection(&shell), Some("pedido"));

    // Copiar leva o que está selecionado ali, e não o documento de trás.
    assert!(shell.copy_selection());
    assert_eq!(
        clipboard.get_text().unwrap_or_default(),
        Some("pedido".to_owned())
    );

    // Colar escreve no editor da inspeção, substituindo o trecho marcado.
    assert!(clipboard.set_text("m.id").is_ok());
    assert!(shell.paste_clipboard());
    assert_eq!(shell.inspection_source(), "m.id.total");
    assert_eq!(
        shell.active_text(),
        Some("int total = 10;"),
        "o documento aberto atrás da janela não pode ser tocado"
    );

    // Duplo clique seleciona a palavra sob o ponteiro.
    shell.select_word_at_point(column(1), size);
    assert_eq!(inspection_selection(&shell), Some("m"));

    // Shift com as setas também marca, como no editor principal.
    shell.inspection.editor_and_source().0.set_cursor(0);
    for _ in 0..4 {
        shell.key_down_with_modifiers(
            "ArrowRight",
            Modifiers {
                shift: true,
                ..Modifiers::default()
            },
        );
    }
    assert_eq!(inspection_selection(&shell), Some("m.id"));
}
/// Várias instruções rodam em sequência, uma esperando a outra.
///
/// Cada uma executa dentro do processo depurado e pode mudar o que a seguinte
/// vai encontrar; mandá-las juntas, ou em paralelo, perderia essa ordem.
#[test]
fn several_statements_run_one_after_the_other() {
    let mut shell = shell_editing("int total = 10;");
    shell.debug_panel.view.attached = true;
    shell.show_inspection("m", inspection_value(), inspection_fields());
    shell
        .inspection
        .set_source("m.setId(5L);\nm.setNome(\"Mario\");\nm.somar(1, 2)");
    shell.run_inspection_source();

    // Só a primeira vai ao alvo.
    assert_eq!(
        shell.take_debug_requests(),
        vec![DebugRequest::Evaluate("m.setId(5L)".to_owned())]
    );
    shell.inspection_result("m.setId(5L)".to_owned(), inspection_void(), Vec::new());
    assert_eq!(
        shell.take_debug_requests(),
        vec![DebugRequest::Evaluate("m.setNome(\"Mario\")".to_owned())]
    );
    shell.inspection_result(
        "m.setNome(\"Mario\")".to_owned(),
        inspection_void(),
        Vec::new(),
    );
    assert_eq!(
        shell.take_debug_requests(),
        vec![DebugRequest::Evaluate("m.somar(1, 2)".to_owned())]
    );

    // A última fecha a execução e dispara a releitura da árvore.
    shell.inspection_result(
        "m.somar(1, 2)".to_owned(),
        DebugVariableView {
            name: "m.somar(1, 2)".to_owned(),
            value: "3".to_owned(),
            type_name: Some("int".to_owned()),
            expandable: false,
        },
        Vec::new(),
    );
    assert_eq!(
        shell.inspection.message(),
        Some("3 instruções executadas — m.somar(1, 2) → 3")
    );
    assert_eq!(
        shell.take_debug_requests(),
        vec![DebugRequest::Evaluate("m".to_owned())]
    );
}
/// Uma instrução que falha interrompe as seguintes e diz onde parou.
#[test]
fn a_failing_statement_stops_the_rest_and_says_where() {
    let mut shell = shell_editing("int total = 10;");
    shell.debug_panel.view.attached = true;
    shell.show_inspection("m", inspection_value(), inspection_fields());
    shell
        .inspection
        .set_source("m.setId(5L);\nm.naoExiste();\nm.setId(6L);");
    shell.run_inspection_source();
    let _ = shell.take_debug_requests();
    shell.inspection_result("m.setId(5L)".to_owned(), inspection_void(), Vec::new());
    let _ = shell.take_debug_requests();

    shell.set_inspection_message("m.naoExiste(): método não encontrado");
    assert_eq!(
        shell.inspection.message(),
        Some("parou na instrução 2 de 3: m.naoExiste(): método não encontrado")
    );
    // A terceira não foi pedida; a releitura, sim, porque a primeira teve
    // efeito.
    assert_eq!(
        shell.take_debug_requests(),
        vec![DebugRequest::Evaluate("m".to_owned())]
    );
}
/// O ponto e vírgula dentro de aspas não termina instrução.
#[test]
fn statements_are_split_outside_quoted_text() {
    assert_eq!(inspection::statements("a(); b()"), vec!["a()", "b()"]);
    assert_eq!(inspection::statements("a(\"x; y\")"), vec!["a(\"x; y\")"]);
    assert_eq!(
        inspection::statements("a(\"x\\\"; y\")"),
        vec!["a(\"x\\\"; y\")"],
        "aspas escapadas não fecham o texto"
    );
    assert_eq!(inspection::statements("  ;\n \n a() ;"), vec!["a()"]);
    assert!(inspection::statements("  \n ; ").is_empty());
}
/// Sem nada escrito, Executar avisa em vez de pedir a avaliação do vazio.
#[test]
fn running_an_empty_source_asks_nothing() {
    let mut shell = shell_editing("int total = 10;");
    shell.debug_panel.view.attached = true;
    shell.show_inspection("pedido", inspection_value(), inspection_fields());
    shell.run_inspection_source();
    assert!(shell.take_debug_requests().is_empty());
    assert_eq!(shell.status_message(), "Escreva a expressão a executar");
}
/// Esc e o botão Fechar dispensam a janela.
#[test]
fn the_inspection_window_closes() {
    let mut shell = shell_editing("int total = 10;");
    shell.show_inspection("pedido", inspection_value(), inspection_fields());
    shell.escape();
    assert!(!shell.inspection_open());

    let size = Size::new(1280.0, 800.0);
    shell.show_inspection("pedido", inspection_value(), inspection_fields());
    let geometry = inspection_layout(&mut shell, size);
    shell.pointer_down(
        Point::new(
            geometry.close.origin.x + 10.0,
            geometry.close.origin.y + 10.0,
        ),
        size,
    );
    assert!(!shell.inspection_open());
}
/// Sem depuração em curso o menu do editor não oferece Inspecionar.
///
/// Fora de uma sessão não há quadro que dê valor ao nome, e o item prometeria
/// o que não pode cumprir.
#[test]
fn inspect_only_appears_while_debugging() {
    let mut shell = shell_editing("int total = 10;");
    let size = Size::new(1280.0, 800.0);
    shell.editor_area.pane.set_selection(Some((4, 9)));
    shell.secondary_pointer_down(editor_column(&shell, size, 6), size);
    assert_eq!(
        entry_labels(shell.context_menu.menu.entries()),
        vec!["Copiar", "Colar"]
    );

    shell.debug_panel.view.attached = true;
    shell.secondary_pointer_down(editor_column(&shell, size, 6), size);
    assert_eq!(
        entry_labels(shell.context_menu.menu.entries()),
        vec!["Copiar", "Colar", "—", "Inspecionar"]
    );
}
/// Inspecionar pede a avaliação do trecho marcado.
#[test]
fn inspecting_asks_to_evaluate_the_selected_text() {
    let mut shell = shell_editing("int total = 10;");
    shell.debug_panel.view.attached = true;
    shell.editor_area.pane.set_selection(Some((4, 9)));
    shell.run_context_command("debug.inspect");
    assert_eq!(
        shell.take_debug_requests(),
        vec![DebugRequest::Evaluate("total".to_owned())]
    );
    assert_eq!(shell.status_message(), "Inspecionando total");
}
/// Sem seleção, Inspecionar aparece desabilitado e nada é pedido.
#[test]
fn inspecting_without_a_selection_asks_nothing() {
    let mut shell = shell_editing("int total = 10;");
    let size = Size::new(1280.0, 800.0);
    shell.debug_panel.view.attached = true;
    shell.secondary_pointer_down(editor_column(&shell, size, 6), size);
    let entries = shell.context_menu.menu.entries();
    let enabled = match &entries[3] {
        MenuEntry::Item(item) => item.enabled,
        MenuEntry::Separator | MenuEntry::Submenu { .. } => true,
    };
    assert!(!enabled, "sem seleção não há o que inspecionar");

    shell.run_context_command("debug.inspect");
    assert!(shell.take_debug_requests().is_empty());
}
