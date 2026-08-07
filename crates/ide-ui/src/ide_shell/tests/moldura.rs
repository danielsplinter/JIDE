//! Testes da **moldura**: as faixas, as barras, os menus, o tema e o
//! roteamento do ponteiro.

use super::*;

/// A moldura declarada dá os mesmos números que a moldura calculada.
///
/// É o que autoriza a próxima etapa a trocar a fonte da geometria: enquanto as
/// duas não concordarem, trocar seria mover a tela inteira sem querer.
#[test]
fn the_declared_frame_agrees_with_the_computed_one() {
    let mut shell = test_shell();
    let size = Size::new(1_280.0, 800.0);
    let _ = shell.paint(size);
    let calculada = shell.geometry();
    let area = |id| {
        shell
            .host
            .bounds(id)
            .unwrap_or_else(|| panic!("a faixa {id:?} precisa estar no arranjo"))
    };

    let tabs = area(FRAME_TABS_ID);
    assert_eq!(
        tabs.origin.y + tabs.size.height,
        calculada.content_top,
        "o conteúdo começa onde as abas terminam"
    );
    assert_eq!(
        area(FRAME_STATUS_ID).origin.y,
        calculada.content_bottom,
        "o conteúdo termina onde a barra de estado começa"
    );
    let editor = area(FRAME_EDITOR_ID);
    assert_eq!(editor.size.height, calculada.editor_height);
    assert_eq!(
        editor.origin.y + editor.size.height,
        calculada.editor_bottom
    );
    assert_eq!(
        area(FRAME_TERMINAL_ID).size.height,
        calculada.terminal_height
    );
    assert_eq!(area(FRAME_CENTER_ID).size.width, calculada.editor_width);
    assert_eq!(area(FRAME_ACTIVITY_ID).size.width, ACTIVITY_WIDTH);
    assert_eq!(area(FRAME_SIDEBAR_ID).size.width, shell.sidebar_width(size));
}
/// Fora dela não há pacote nem classe: resta a pasta.
#[test]
fn outside_the_source_root_the_menu_offers_a_folder() {
    for target in ["demo", "demo/docs", "demo/src/main/resources"] {
        assert_eq!(
            entry_labels(&explorer_menu_entries(
                Path::new(target),
                &java_source_roots(),
                &java_catalog().new_item_templates,
                false,
            )),
            vec!["Nova pasta"],
            "alvo {target}"
        );
    }
}
/// A linha juntada nasce **aberta**, e não pedindo mais um clique.
///
/// A árvore junta `br`, `com` e `exemplo` numa linha só, e a identidade dessa
/// linha é a do **último** diretório. Quem clicou abriu o **primeiro** — era o
/// único que existia na árvore naquele instante. Sem casar os dois, os filhos
/// chegam, o nome vira `br.com.exemplo`, e a linha continua fechada.
#[test]
fn a_cadeia_de_pacotes_termina_aberta_apos_um_clique() {
    let tree = dir(
        "demo",
        vec![dir("demo/src/main/java", vec![dir("demo/src/main/java/br", Vec::new())])],
    );
    let mut shell = IdeShell::from_tree(tree);
    shell.set_ui_catalog(java_catalog());

    // O clique abriu o primeiro elo: é o único que existia na árvore.
    shell
        .explorer
        .expanded
        .insert(PathBuf::from("demo/src/main/java/br"));

    // A leitura desce a cadeia de uma vez, como o workspace passou a fazer.
    shell.insert_path_children(vec![
        (
            PathBuf::from("demo/src/main/java/br"),
            vec![dir("demo/src/main/java/br/com", Vec::new())],
        ),
        (
            PathBuf::from("demo/src/main/java/br/com"),
            vec![dir("demo/src/main/java/br/com/exemplo", Vec::new())],
        ),
        (
            PathBuf::from("demo/src/main/java/br/com/exemplo"),
            vec![file("demo/src/main/java/br/com/exemplo/App.java")],
        ),
    ]);

    assert!(
        shell
            .explorer
            .expanded
            .contains(&PathBuf::from("demo/src/main/java/br/com/exemplo")),
        "o fim da cadeia precisa ficar aberto: é ele que a linha juntada \
         representa, e é onde está o que se quis ver"
    );
}
/// Clicar num arquivo já à vista não mexe na rolagem.
///
/// Abrir um documento reconcilia a árvore com ele — e a reconciliação rolava
/// sempre, mesmo quando a linha já estava na tela. O efeito era o arquivo
/// clicado saltar para o alto da lista e tudo o mais deslizar junto, sem que
/// ninguém tivesse pedido.
#[test]
fn clicar_num_arquivo_a_vista_nao_rola_a_arvore() {
    let arquivos: Vec<FileNode> = (1..=8)
        .map(|indice| file(&format!("demo/pasta/A{indice}.java")))
        .collect();
    let tree = dir("demo", vec![dir("demo/pasta", arquivos)]);
    let mut shell = IdeShell::from_tree(tree);
    shell.explorer.expanded.insert(PathBuf::from("demo/pasta"));
    shell.sync_explorer_tree();

    // A vista está no topo, e o painel é alto o bastante para caberem todos.
    shell.explorer.scroll_line = 0;
    shell.show_document(Path::new("demo/pasta/A5.java"), "class A5 {}");

    assert_eq!(
        shell.explorer.scroll_line, 0,
        "o arquivo já estava à vista: rolar move a árvore debaixo de quem clicou"
    );
    assert_eq!(
        shell.explorer.tree.selected(),
        Some(explorer_id(Path::new("demo/pasta/A5.java"))),
        "e ele continua sendo o escolhido"
    );
}
/// Um arquivo fora da vista **é** revelado: revelar continua sendo o trabalho.
#[test]
fn um_arquivo_fora_da_vista_continua_sendo_revelado() {
    let arquivos: Vec<FileNode> = (1..=200)
        .map(|indice| file(&format!("demo/pasta/A{indice}.java")))
        .collect();
    let tree = dir("demo", vec![dir("demo/pasta", arquivos)]);
    let mut shell = IdeShell::from_tree(tree);
    shell.explorer.expanded.insert(PathBuf::from("demo/pasta"));
    shell.sync_explorer_tree();
    shell.explorer.scroll_line = 0;

    shell.show_document(Path::new("demo/pasta/A180.java"), "class A180 {}");

    assert!(
        shell.explorer.scroll_line > 0,
        "um arquivo muito abaixo da vista precisa ser trazido para ela"
    );
}
/// Esc dispensa o menu antes de qualquer outra coisa que Esc faria.
#[test]
fn escape_dismisses_the_context_menu_first() {
    let mut shell = IdeShell::from_tree(maven_project());
    let size = Size::new(1280.0, 800.0);
    shell.context.focus = ShellFocus::Search;
    shell.editor_area.search_query = "consulta".to_owned();
    shell.secondary_pointer_down(Point::new(80.0, EXPLORER_TOP + 2.0), size);
    shell.escape();
    assert!(!shell.context_menu_open());
    assert_eq!(shell.editor_area.search_query, "consulta");
}
/// A barra de estado informa em segmentos: a mensagem à esquerda, e o que
/// se procura sempre no mesmo lugar ancorado à direita.
#[test]
fn the_status_bar_reports_message_and_position_in_separate_segments() {
    let mut shell = test_shell();
    shell.set_status_message("Compilação concluída");
    let size = Size::new(1_000.0, 700.0);
    let texts: Vec<String> = shell
        .paint(size)
        .iter()
        .filter_map(|command| match command {
            PaintCommand::DrawText(text) => Some(text.text.clone()),
            _ => None,
        })
        .collect();
    assert!(texts.iter().any(|text| text == "Compilação concluída"));
    assert!(texts.iter().any(|text| text == "UTF-8"));
    assert!(texts.iter().any(|text| text == "Ln 1, Col 1"));
}
#[test]
fn the_action_buttons_are_library_widgets_with_accessible_names() {
    let mut shell = test_shell();
    let areas = action_areas(&mut shell, Size::new(1_280.0, 800.0));
    let mut context = PaintContext::with_theme(*shell.theme());
    let mut accessibility = ui_api::AccessibilityContext::default();
    for (button, rect) in shell.action_buttons().into_iter().zip(areas) {
        let mut button = button.clone();
        button.layout(&LayoutContext::default(), rect);
        button.paint(&mut context);
        button.accessibility(&mut accessibility);
    }
    let names: Vec<&str> = accessibility
        .nodes()
        .iter()
        .map(|node| node.label.as_str())
        .collect();
    assert_eq!(
        names,
        vec![
            "Parar aplicação",
            "Executar aplicação",
            "Executar com depuração"
        ],
        "um ícone não é legível: quem o expõe é a biblioteca"
    );
}
#[test]
fn the_play_button_requests_a_plain_run() {
    let mut shell = test_shell();
    let size = Size::new(1_280.0, 800.0);
    let [_, run, debug] = action_areas(&mut shell, size);
    assert!(
        run.origin.x + run.size.width <= debug.origin.x,
        "o play fica à esquerda do inseto, sem sobrepor"
    );
    let colors = Theme::default().colors;
    assert!(
        shell
            .paint(size)
            .iter()
            .filter(|command| matches!(command, PaintCommand::FillRect(rect)
                    if rect.color == colors.success && run.contains(rect.rect.origin)))
            .count()
            >= 5,
        "o triângulo de play é desenhado com a cor de ação da paleta"
    );

    shell.pointer_down(Point::new(run.origin.x + 6.0, run.origin.y + 6.0), size);
    assert!(shell.take_run_request());
    assert!(!shell.take_run_request(), "o pedido é consumido uma vez");
    assert!(
        shell.take_debug_requests().is_empty(),
        "executar sem depuração não abre sessão"
    );
}
#[test]
fn the_stop_button_sits_left_of_play_and_only_acts_after_a_run() {
    let mut shell = test_shell();
    let size = Size::new(1_280.0, 800.0);
    let [stop, run, _] = action_areas(&mut shell, size);
    assert!(
        stop.origin.x + stop.size.width <= run.origin.x,
        "a ordem é parar, executar, depurar"
    );

    let colors = Theme::default().colors;
    let icon_color = |shell: &mut IdeShell| {
        shell.paint(size).iter().find_map(|command| match command {
            PaintCommand::FillRect(rect)
                if stop.contains(rect.rect.origin) && rect.rect.size.width < 20.0 =>
            {
                Some(rect.color)
            }
            _ => None,
        })
    };
    assert_eq!(
        icon_color(&mut shell),
        Some(colors.muted_text),
        "sem aplicação iniciada, o ícone fica apagado"
    );
    assert!(!shell.application_running());

    shell.pointer_down(Point::new(stop.origin.x + 6.0, stop.origin.y + 6.0), size);
    assert!(shell.take_stop_request());
    assert!(
        shell.stop_application().is_err(),
        "sem aplicação iniciada não há o que interromper"
    );
}
#[test]
fn the_project_menu_also_runs_the_application() {
    let mut shell = test_shell();
    let size = Size::new(1_000.0, 700.0);
    shell.pointer_down(Point::new(200.0, 10.0), size);
    shell.pointer_down(Point::new(200.0, TITLE_HEIGHT + 66.0), size);
    assert!(shell.take_run_request());
    assert!(!shell.take_build_project_request());
    assert!(!shell.take_reimport_project_request());
}
#[test]
fn the_bug_button_runs_and_attaches_with_the_configured_target() {
    let mut shell = test_shell();
    let size = Size::new(1_280.0, 800.0);
    shell.set_debug_target("10.0.0.20", 8787);

    let button = action_areas(&mut shell, size)[2];
    assert!(
        button.origin.x + button.size.width < size.width && button.origin.x > size.width - 60.0,
        "o botão fica no canto direito da barra de menus"
    );
    assert!(
        shell
            .paint(size)
            .iter()
            .filter(|command| matches!(command, PaintCommand::FillCircle(circle)
                    if button.contains(circle.center)))
            .count()
            >= 2,
        "o ícone desenha corpo e cabeça do inseto dentro do botão"
    );

    shell.pointer_down(
        Point::new(button.origin.x + 6.0, button.origin.y + 6.0),
        size,
    );
    assert_eq!(
        shell.take_debug_requests(),
        vec![DebugRequest::RunAndAttach {
            host: "10.0.0.20".to_owned(),
            port: 8787,
        }]
    );
}
#[test]
fn the_bug_button_asks_for_a_target_when_it_is_invalid() {
    let mut shell = test_shell();
    let size = Size::new(1_280.0, 800.0);
    shell.set_debug_target("", 0);

    let button = action_areas(&mut shell, size)[2];
    shell.pointer_down(
        Point::new(button.origin.x + 6.0, button.origin.y + 6.0),
        size,
    );

    assert!(shell.take_debug_requests().is_empty());
    assert!(
        shell.take_open_settings_request(),
        "sem alvo válido, o botão abre a página de depuração"
    );
    assert_eq!(shell.settings_page(), SettingsPage::Debug);
}
/// O que não é nome não abre lista nenhuma.
#[test]
fn punctuation_does_not_open_the_list() {
    let mut shell = test_shell();
    shell.editor_area.session.open_memory("Pagina.ts", "");
    shell.context.focus = ShellFocus::Editor;
    shell.text_input("ab");
    assert!(shell.completion_opens_now());
    shell.text_input("(");
    assert!(
        !shell.completion_opens_now(),
        "um parêntese encerra o nome: {:?}",
        shell.active_text()
    );
}
#[test]
fn file_project_menu_requests_a_folder_picker() {
    let mut shell = test_shell();
    let size = Size::new(1280.0, 800.0);
    shell.pointer_down(Point::new(100.0, 15.0), size);
    let menu_is_visible = shell.paint(size).into_iter().any(|command| {
        matches!(
            command,
            PaintCommand::DrawText(command) if command.text == "Projeto..."
        )
    });
    assert!(menu_is_visible);
    shell.pointer_down(Point::new(110.0, TITLE_HEIGHT + 15.0), size);
    assert!(shell.take_open_project_request());
    assert!(!shell.take_open_project_request());
}
/// Recarregar o mesmo caminho não gera trabalho novo.
///
/// Este teste existe por causa de um travamento: `insert` respondia "mudou" por
/// ter **achado** o nó, e não por o conteúdo ser outro. A reconciliação da
/// seleção então revelava o caminho, que pedia a leitura de novo, que reconciliava
/// de novo — a fila nunca esvaziava, o laço da janela nunca chegava a desenhar, e
/// a IDE abria em branco. A suíte inteira passava.
#[test]
fn loading_the_same_path_twice_asks_for_nothing_more() {
    let root = PathBuf::from("workspace");
    let mut shell = IdeShell::from_tree(FileNode {
        path: root.clone(),
        is_directory: true,
        children: vec![FileNode {
            path: root.join("src"),
            is_directory: true,
            children: Vec::new(),
        }],
    });
    let filhos = vec![FileNode {
        path: root.join("src/Arquivo.java"),
        is_directory: false,
        children: Vec::new(),
    }];

    // Um documento ativo que a árvore não tem: é ele que faz a reconciliação
    // revelar o caminho, e revelar é o que pede leitura.
    shell
        .editor_area
        .session
        .open_memory(root.join("src/Fantasma.java"), "class Fantasma {}");

    shell.insert_path_children(vec![(root.join("src"), filhos.clone())]);
    let pendentes = |shell: &IdeShell| {
        shell
            .commands
            .iter()
            .filter(|command| matches!(command, ApplicationCommand::LoadDirectory(_)))
            .count()
    };
    let depois_da_primeira = pendentes(&shell);

    // A mesma resposta de novo: nada mudou, então nada mais é pedido.
    shell.insert_path_children(vec![(root.join("src"), filhos)]);
    assert_eq!(
        pendentes(&shell),
        depois_da_primeira,
        "recarregar o mesmo conteúdo não pode pedir mais leitura"
    );
}
#[test]
fn project_menu_requests_build_and_reimport() {
    let mut shell = test_shell();
    let size = Size::new(1_000.0, 700.0);

    shell.pointer_down(Point::new(200.0, 10.0), size);
    shell.pointer_down(Point::new(200.0, TITLE_HEIGHT + 10.0), size);
    assert!(shell.take_build_project_request());
    assert!(!shell.take_reimport_project_request());

    shell.pointer_down(Point::new(200.0, 10.0), size);
    shell.pointer_down(Point::new(200.0, TITLE_HEIGHT + 38.0), size);
    assert!(shell.take_reimport_project_request());
    assert!(!shell.take_build_project_request());
}
/// A interface não define cor própria: todas vêm do tema da ERLibUi.
///
/// Sem esta trava, uma cor solta volta a aparecer na primeira pressa — foi
/// o que aconteceu com a barra de status e com o marcador de breakpoint.
#[test]
fn the_interface_does_not_hardcode_colors() {
    // O arquivo inteiro é produção desde que os testes saíram dele: não há mais
    // o que recortar, e a varredura deixa de depender de achar um marcador.
    let source = include_str!("../../ide_shell.rs");
    for (number, line) in source.lines().enumerate() {
        assert!(
            !line.contains("Color::rgba"),
            "linha {} usa cor fixa; use um token de `Theme`: {}",
            number + 1,
            line.trim()
        );
    }
}
#[test]
fn the_theme_comes_from_the_library_and_reaches_its_components() {
    let mut shell = test_shell();
    let size = Size::new(1_000.0, 700.0);
    assert_eq!(shell.theme(), &Theme::dark(), "o tema padrão é o da lib");

    let dark: Vec<Color> = shell
        .paint(size)
        .iter()
        .filter_map(|command| match command {
            PaintCommand::DrawText(text) => Some(text.color),
            _ => None,
        })
        .collect();

    shell.set_theme(Theme::high_contrast());
    let contrast: Vec<Color> = shell
        .paint(size)
        .iter()
        .filter_map(|command| match command {
            PaintCommand::DrawText(text) => Some(text.color),
            _ => None,
        })
        .collect();

    assert_ne!(dark, contrast, "trocar o tema muda o que é pintado");
    assert!(
        contrast.contains(&Theme::high_contrast().colors.text),
        "o texto usa o token do tema ativo"
    );
    // A barra de menus é um componente da lib: ela recebe o tema pelo
    // contexto de pintura, sem a IDE redesenhá-la.
    assert!(
        shell.paint(size).iter().any(|command| matches!(
            command,
            PaintCommand::DrawText(text)
                if text.text == "Arquivo"
                    && text.color == Theme::high_contrast().colors.text
        )),
        "os componentes da biblioteca seguem o tema da aplicação"
    );
}
#[test]
fn status_bar_uses_palette_colors_with_readable_contrast() {
    let mut shell = test_shell();
    shell.set_status_message("Pronto");
    let size = Size::new(1_000.0, 700.0);
    let colors = Theme::default().colors;
    // O quadro primeiro: a geometria é a do arranjo, e o arranjo é deste tamanho.
    let commands = shell.paint(size);
    let geometry = shell.geometry();

    let background = commands.iter().find_map(|command| match command {
        PaintCommand::FillRect(rect)
            if rect.rect.origin.y == geometry.content_bottom && rect.rect.size.height > 1.0 =>
        {
            Some(rect.color)
        }
        _ => None,
    });
    assert_eq!(
        background,
        Some(colors.surface),
        "a barra usa a superfície da paleta, não a cor de destaque"
    );

    let text_color = commands.iter().find_map(|command| match command {
        PaintCommand::DrawText(text) if text.text.starts_with("Pronto") => Some(text.color),
        _ => None,
    });
    assert_eq!(
        text_color,
        Some(colors.text),
        "o texto usa a cor de texto da paleta, não branco puro"
    );
    assert!(
        contrast_ratio(colors.text, colors.surface) >= 7.0,
        "texto e fundo da barra precisam de contraste confortável"
    );
}
#[test]
fn status_bar_shows_the_imported_project_summary() {
    let mut shell = test_shell();
    shell.set_project_summary(Some("Maven • demo • 2 módulo(s)".to_owned()));
    assert_eq!(shell.project_summary(), Some("Maven • demo • 2 módulo(s)"));
    assert!(
        shell
            .paint(Size::new(1_000.0, 700.0))
            .iter()
            .any(|command| matches!(
                command,
                PaintCommand::DrawText(text) if text.text.contains("Maven • demo • 2 módulo(s)")
            ))
    );
}
/// Clicar em um campo troca o que o painel direito detalha.
#[test]
fn clicking_a_field_changes_the_detail_panel() {
    let mut shell = shell_editing("int total = 10;");
    let size = Size::new(1280.0, 800.0);
    shell.show_inspection("pedido", inspection_value(), inspection_fields());
    let geometry = inspection_layout(&mut shell, size);
    // Segunda linha da árvore: o primeiro campo, com a raiz já aberta.
    shell.pointer_down(
        Point::new(
            geometry.list.origin.x + 30.0,
            geometry.list.origin.y + inspection::ROW_HEIGHT + 4.0,
        ),
        size,
    );
    assert_eq!(
        shell
            .inspection
            .selected_variable()
            .map(|entry| entry.name.clone()),
        Some("total".to_owned())
    );
}
/// Abrir um campo que é objeto pede os campos dele ao alvo.
///
/// Os níveis seguintes não vêm juntos: o grafo de um objeto pode ser fundo e
/// cíclico, e percorrê-lo inteiro para mostrar o primeiro nível travaria.
#[test]
fn expanding_a_nested_object_asks_the_target_for_its_fields() {
    let mut shell = shell_editing("int total = 10;");
    let size = Size::new(1280.0, 800.0);
    shell.show_inspection("pedido", inspection_value(), inspection_fields());
    let geometry = inspection_layout(&mut shell, size);
    // Terceira linha: `cliente`, que é objeto.
    shell.pointer_down(
        Point::new(
            geometry.list.origin.x + 30.0,
            geometry.list.origin.y + inspection::ROW_HEIGHT * 2.0 + 4.0,
        ),
        size,
    );
    assert_eq!(
        shell.take_debug_requests(),
        vec![DebugRequest::ExpandInspection("pedido.cliente".to_owned())]
    );

    // Os campos chegam e passam a aparecer sob o nó aberto.
    shell.add_inspection_fields(
        "pedido.cliente",
        vec![DebugVariableView {
            name: "nome".to_owned(),
            value: "João da Silva".to_owned(),
            type_name: Some("String".to_owned()),
            expandable: false,
        }],
    );
    assert!(
        painted_texts(&mut shell, size)
            .iter()
            .any(|text| text.contains("nome = (String) João da Silva")),
        "o campo do objeto aninhado precisa aparecer"
    );
}
#[test]
fn application_commands_leave_the_shell_in_one_ordered_queue() {
    let mut shell = shell_editing("class Uso {}");
    shell.open_type_search();
    shell.request_debug(DebugRequest::Continue);

    assert_eq!(
        shell.drain_application_commands(),
        vec![
            ApplicationCommand::SearchTypes(String::new()),
            ApplicationCommand::Debug(DebugRequest::Continue),
        ]
    );
    assert!(shell.drain_application_commands().is_empty());
}
/// Enquanto o projeto e preparado, gira no meio da tela.
///
/// **Enquanto isso dura, o que a IDE responde e incompleto**: a busca nao acha, a
/// completacao nao sabe os tipos. Sem sinal nenhum, quem usa atribui isso a IDE
/// em vez de a espera -- foi o que aconteceu, e o giro existe para evitar isso.
/// Ele nao bloqueia nada: da para editar e navegar enquanto roda.
#[test]
fn while_the_project_is_prepared_it_spins_in_the_middle_of_the_screen() {
    let size = Size::new(1280.0, 800.0);
    let mut shell = shell_with_package();

    let parado = paint_circles(&mut shell, size).len();
    shell.set_project_loading(Some(0.0));
    let girando = paint_circles(&mut shell, size);
    assert!(
        girando.len() >= parado + 8,
        "o anel inteiro precisa aparecer: {} contra {parado}",
        girando.len()
    );

    // Preparado o projeto, ele some.
    shell.set_project_loading(None);
    assert_eq!(
        paint_circles(&mut shell, size).len(),
        parado,
        "terminada a preparacao, o giro precisa sumir"
    );
}
/// O giro do carregamento fica no centro, e nao num canto.
///
/// Ele e o ultimo a ser desenhado -- por cima de tudo, porque tudo o que esta
/// embaixo esta incompleto enquanto ele roda --, e por isso os oito ultimos
/// circulos do quadro sao os pontos dele. Tomar todos os circulos da tela
/// misturaria icones de outros componentes e deslocaria a media.
#[test]
fn the_loading_spinner_sits_in_the_centre() {
    let size = Size::new(1280.0, 800.0);
    let mut shell = shell_with_package();
    shell.set_project_loading(Some(0.0));
    let mut centros: Vec<_> = shell
        .paint(size)
        .iter()
        .filter_map(|command| match command {
            PaintCommand::FillCircle(circle) => Some(circle.center),
            _ => None,
        })
        .collect();
    let anel = centros.split_off(centros.len().saturating_sub(8));
    assert_eq!(anel.len(), 8, "o anel tem oito pontos");
    let media_x = anel.iter().map(|p| p.x).sum::<f32>() / 8.0;
    let media_y = anel.iter().map(|p| p.y).sum::<f32>() / 8.0;
    assert!(
        (media_x - size.width / 2.0).abs() < 2.0 && (media_y - size.height / 2.0).abs() < 2.0,
        "o anel precisa estar centrado, e esta em ({media_x}, {media_y})"
    );
}
/// O giro não deixa a lista da busca anterior aparecer embaixo dele.
///
/// Sem isto, a resposta velha ficaria na tela como se fosse a desta pergunta.
#[test]
fn the_spinner_replaces_the_previous_results() {
    let size = Size::new(1280.0, 800.0);
    let root = std::env::temp_dir().join(format!("er-giro-{}", std::process::id()));
    let mut shell = shell_with_package();
    shell.open_type_search();
    shell.set_type_search_results(vec![type_hit(
        "PedidoAntigo",
        "classe",
        &root.join("PedidoAntigo.java"),
        0,
    )]);
    assert!(
        painted_texts(&mut shell, size)
            .iter()
            .any(|text| text.contains("PedidoAntigo")),
        "a lista da busca anterior está na tela antes da nova pergunta"
    );

    shell.set_search_progress(Some(0.25));
    assert!(
        !painted_texts(&mut shell, size)
            .iter()
            .any(|text| text.contains("PedidoAntigo")),
        "a resposta velha não pode ficar embaixo do giro"
    );
}
/// Tab com um bloco marcado desloca todas as linhas dele.
#[test]
fn tab_shifts_the_selected_block() {
    let mut shell = shell_editing(
        "um
dois
tres",
    );
    // Da segunda linha até o meio da terceira.
    shell.editor_area.pane.set_cursor(9);
    shell.editor_area.pane.set_selection(Some((3, 9)));
    shell.key_down("Tab");
    assert_eq!(
        shell.active_text(),
        Some(
            "um
    dois
    tres"
        )
    );
    // A seleção segue cobrindo o bloco, para indentar de novo sem remarcar.
    assert_eq!(shell.editor_area.pane.selection_range(), Some(3..20));

    let shift = Modifiers {
        shift: true,
        ..Modifiers::default()
    };
    shell.key_down_with_modifiers("Tab", shift);
    assert_eq!(
        shell.active_text(),
        Some(
            "um
dois
tres"
        )
    );
}
/// Com trecho marcado, o abridor envolve em vez de apagar.
#[test]
fn o_abridor_envolve_o_trecho_marcado() {
    let mut shell = shell_editing("total");
    shell.editor_area.pane.set_cursor(5);
    shell.editor_area.pane.set_selection(Some((0, 5)));
    shell.text_input("(");
    assert_eq!(shell.active_text(), Some("(total)"));
    assert_eq!(
        shell.editor_area.pane.selection_range(),
        Some(1..6),
        "o trecho continua marcado, para envolver de novo"
    );
}
/// "Duplicar workspace" pede outra janela, e não abre nada por conta própria.
///
/// O shell não sabe abrir processo, e não deve saber: ele diz o que o clique
/// significa, e quem executa é a aplicação. Aqui se confere só isso — que o
/// item existe, que é o terceiro do menu Arquivo, e que ele emite o comando.
#[test]
fn duplicar_workspace_pede_outra_janela() {
    let mut shell = test_shell();
    let size = Size::new(1280.0, 800.0);

    shell.pointer_down(Point::new(100.0, TITLE_HEIGHT / 2.0), size);
    shell.pointer_down(Point::new(100.0, TITLE_HEIGHT + 63.0), size);

    let commands = shell.drain_application_commands();
    assert!(
        commands
            .iter()
            .any(|comando| matches!(comando, ApplicationCommand::DuplicateWorkspace)),
        "o menu deveria emitir DuplicateWorkspace, e veio {commands:?}"
    );
}
/// O movimento do ponteiro chega à barra de menus, e pede o quadro.
///
/// A barra não está no anfitrião: ela é arranjada na hora, e sem alguém lhe
/// entregar o movimento ela nunca saberia que o ponteiro passou por cima. O
/// retorno é o que faz a janela redesenhar — sem ele o realce só apareceria
/// quando outra coisa qualquer pedisse um quadro novo.
#[test]
fn o_movimento_do_ponteiro_chega_a_barra_de_menus() {
    let mut shell = test_shell();
    let size = Size::new(1280.0, 800.0);

    assert!(
        shell.pointer_move(Point::new(100.0, TITLE_HEIGHT / 2.0), size),
        "sobre um item da barra, o quadro precisa ser redesenhado"
    );
    // Parado no mesmo item, nada mudou e não há o que redesenhar.
    assert!(!shell.pointer_move(Point::new(101.0, TITLE_HEIGHT / 2.0), size));
    // Saindo da barra, o realce apagado ainda pede um quadro.
    assert!(shell.pointer_move(Point::new(600.0, 400.0), size));

    // Com o menu aberto, apontar "Recentes" abre a lista ao lado sem clique —
    // e nela estão as linguagens, uma porta cada.
    shell.set_recent_projects(vec![RecentProject {
        path: std::path::PathBuf::from("/tmp/loja"),
        language: Some("TypeScript".to_owned()),
    }]);
    shell.pointer_down(Point::new(100.0, TITLE_HEIGHT / 2.0), size);
    shell.pointer_move(Point::new(100.0, TITLE_HEIGHT + 42.0), size);
    let desenhado = shell.paint(size);
    assert!(
        desenhado.iter().any(|comando| matches!(
            comando,
            PaintCommand::DrawText(texto) if texto.text == "TypeScript"
        )),
        "a lista de recentes deveria estar aberta só por apontar"
    );
}
/// Escolher um projeto em "Arquivo → Recentes → linguagem" pede a abertura.
///
/// A posição clicada volta a ser caminho aqui dentro; a aplicação recebe o
/// caminho pronto, e não um índice que ela teria de reinterpretar. O caminho
/// atravessa os três níveis do menu: a barra, os recentes e a linguagem.
#[test]
fn um_recente_escolhido_no_menu_pede_o_projeto() {
    let mut shell = test_shell();
    let size = Size::new(1280.0, 800.0);
    let primeiro = std::path::PathBuf::from("/tmp/loja");
    let segundo = std::path::PathBuf::from("/tmp/portal");
    shell.set_recent_projects(vec![
        RecentProject {
            path: primeiro,
            language: Some("TypeScript".to_owned()),
        },
        RecentProject {
            path: segundo.clone(),
            language: Some("TypeScript".to_owned()),
        },
    ]);

    // Arquivo → Recentes → TypeScript → o segundo projeto do grupo.
    shell.pointer_down(Point::new(100.0, TITLE_HEIGHT / 2.0), size);
    shell.pointer_down(Point::new(100.0, TITLE_HEIGHT + 42.0), size);
    shell.pointer_down(Point::new(420.0, TITLE_HEIGHT + 42.0), size);
    shell.pointer_down(Point::new(700.0, TITLE_HEIGHT + 63.0), size);

    let commands = shell.drain_application_commands();
    assert!(
        commands.iter().any(|comando| matches!(
            comando,
            ApplicationCommand::OpenRecentProject(caminho) if *caminho == segundo
        )),
        "o menu deveria pedir o projeto recente, e veio {commands:?}"
    );
}
/// O item "Salvar" entrega o conteúdo à aplicação sem escrever pela UI.
#[test]
fn the_file_menu_saves_the_active_tab() {
    let root = std::env::temp_dir().join(format!("er-ide-menu-save-{}", std::process::id()));
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
    shell.text_input("// pelo menu\n");
    let size = Size::new(1280.0, 800.0);
    // Abre o menu Arquivo e escolhe "Salvar", que é a **quarta** entrada:
    // "Projeto...", "Recentes" e "Duplicar workspace" vêm antes. A escolha é por
    // posição, então acrescentar um item acima dela muda esta linha — e é a
    // falha deste teste que avisa.
    shell.pointer_down(Point::new(100.0, TITLE_HEIGHT / 2.0), size);
    shell.pointer_down(Point::new(100.0, TITLE_HEIGHT + 84.0), size);
    let commands = shell.drain_application_commands();
    let Some(ApplicationCommand::SaveDocument(request)) = commands.first() else {
        panic!("o menu deveria emitir SaveDocument");
    };
    assert_eq!(request.path, file);
    assert_eq!(request.text, "// pelo menu\nclass Pedido {}");
    assert!(
        shell.active_document_modified(),
        "a confirmação do adapter ainda não chegou"
    );
    shell.document_saved(request.document_id, request.revision, &request.path);
    assert!(!shell.active_document_modified());
    let _ = std::fs::remove_dir_all(&root);
}
/// Tab troca o campo, então o pacote também é editável ao criar um tipo.
#[test]
fn tab_moves_between_the_two_fields() {
    let mut shell = shell_with_package();
    shell.context_menu.alvo = Some(AlvoDoMenu::Explorer {
        pasta: PathBuf::from("demo/src/main/java/br/com"),
        arquivo: None,
    });
    shell.run_context_command("explorer.new.java.class");
    shell.key_down("Tab");
    shell.text_input(".exemplo");
    shell.key_down("Tab");
    shell.text_input("Pedido");
    shell.key_down("Enter");
    assert_eq!(
        shell.take_new_item_request(),
        Some(NewItemRequest {
            template_id: NewItemTemplateId::new("java.class"),
            package: "br.com.exemplo".to_owned(),
            name: "Pedido".to_owned(),
            source_root: PathBuf::from("demo/src/main/java"),
        })
    );
}
/// Classe sem nome não é pedido válido, e a janela fica aberta dizendo o quê.
#[test]
fn a_type_without_a_name_is_refused_without_closing() {
    let mut shell = shell_with_package();
    shell.context_menu.alvo = Some(AlvoDoMenu::Explorer {
        pasta: PathBuf::from("demo/src/main/java/br/com"),
        arquivo: None,
    });
    shell.run_context_command("explorer.new.java.class");
    shell.key_down("Enter");
    assert_eq!(shell.take_new_item_request(), None);
    assert!(shell.new_item_dialog_open());
    assert_eq!(shell.new_item.message(), Some("Informe o nome.".to_owned()));
}
/// O caminho do `Ctrl+clique` tem volta, e a volta tem ida.
///
/// Descer três camadas e voltar pelas mesmas três é o gesto inteiro. O que se
/// guarda é **posição**, e não só arquivo: voltar devolve o cursor à linha de
/// onde ele saiu, senão cair no topo do arquivo faria a volta perder o lugar.
#[test]
fn control_click_leaves_a_trail_that_can_be_walked_back() {
    let root = PathBuf::from("workspace");
    let mut shell = IdeShell::from_tree(FileNode {
        path: root.clone(),
        is_directory: true,
        children: Vec::new(),
    });
    let ctrl_alt = Modifiers {
        control: true,
        alt: true,
        ..Modifiers::default()
    };

    // Sem histórico, as duas teclas não fazem nada — e nem por isso quebram.
    shell.key_down_with_modifiers("ArrowLeft", ctrl_alt);
    shell.key_down_with_modifiers("ArrowRight", ctrl_alt);
    assert_eq!(shell.history_depth(), (0, 0));

    // Descendo: cada Ctrl+clique registra de onde saiu.
    let camadas = ["Primeiro.java", "Segundo.java", "Terceiro.java"];
    for nome in camadas {
        shell
            .editor_area
            .session
            .open(&root.join(nome), "class A {\n    int a;\n}\n");
    }
    for _ in 0..2 {
        shell.handle_editor_action(EditorAction::Navigate(0));
    }
    assert_eq!(
        shell.history_depth(),
        (1, 0),
        "dois saltos do mesmo lugar sao um passo so"
    );

    // Um salto de outro lugar acrescenta um passo.
    shell.editor_area.pane.set_cursor(6);
    shell.handle_editor_action(EditorAction::Navigate(6));
    assert_eq!(shell.history_depth(), (2, 0));

    // Voltar: pede o arquivo na linha e coluna de onde se saiu.
    shell.drain_application_commands();
    assert!(shell.navigate_back());
    let pedido = shell
        .drain_application_commands()
        .into_iter()
        .find_map(|command| match command {
            ApplicationCommand::OpenDocument(request) => Some(request),
            _ => None,
        });
    let Some(pedido) = pedido else {
        panic!("voltar precisa pedir a abertura do passo anterior");
    };
    assert_eq!(pedido.path, root.join("Terceiro.java"));
    assert_eq!(
        (pedido.line, pedido.column),
        (0, 6),
        "a volta devolve a coluna de onde se saiu, e nao o topo do arquivo"
    );
    assert_eq!(shell.history_depth(), (1, 1), "o que voltou fica adiante");

    // Avançar desfaz a volta.
    assert!(shell.navigate_forward());
    assert_eq!(shell.history_depth(), (2, 0));

    // E um salto novo descarta o que havia adiante.
    assert!(shell.navigate_back());
    assert_eq!(shell.history_depth(), (1, 1));
    shell.handle_editor_action(EditorAction::Navigate(0));
    assert_eq!(
        shell.history_depth().1,
        0,
        "saltar de novo apaga o caminho para a frente"
    );
}
/// O foco segue o ponteiro, e tudo passa a acontecer no painel apontado.
///
/// Olhar um lado e digitar no outro é o que confunde quem divide a tela. Aqui se
/// confere o caminho inteiro: passar o ponteiro dá o foco, digitar escreve no
/// painel apontado, e abrir um arquivo abre a aba **nele**.
#[test]
fn o_ponteiro_leva_o_foco_para_o_painel_da_direita() {
    let root = std::env::temp_dir().join(format!("er-ide-foco-split-{}", std::process::id()));
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

    // O ponteiro volta para a esquerda: o foco vai com ele.
    let Some(esquerda) = shell.left_column(size) else {
        panic!("a coluna da esquerda precisa ter área");
    };
    shell.pointer_move(
        Point::new(esquerda.origin.x + 40.0, esquerda.origin.y + 200.0),
        size,
    );
    assert!(!shell.split_focado(), "apontar a esquerda foca a esquerda");

    // E de volta para a direita, sem clique nenhum.
    let Some(direita) = shell.right_column(size) else {
        panic!("a coluna da direita precisa ter área");
    };
    let dentro_da_direita = Point::new(direita.origin.x + 40.0, direita.origin.y + 200.0);
    shell.pointer_move(dentro_da_direita, size);
    assert!(shell.split_focado(), "apontar a direita foca a direita");

    // A digitação vai para o painel apontado: é o cursor dele que anda.
    let Some((_, texto)) = shell.focused_editor_ref() else {
        panic!("o painel apontado precisa ter documento");
    };
    let antes = texto.text().to_owned();
    shell.text_input("//");
    let Some(divisao) = shell.editor_area.divisao.as_ref() else {
        panic!("a divisão precisa existir");
    };
    // Com a direita em foco, o painel da frente **é** o dela: os dois trocam de
    // lugar quando o foco troca. Ver `focar_a_direita`.
    assert_eq!(
        shell.editor_area.pane.cursor(),
        2,
        "o cursor que andou é o do lado apontado"
    );
    assert_eq!(
        divisao.pane.cursor(),
        0,
        "o painel do outro lado não recebeu a digitação"
    );
    assert!(shell.active_text().is_some_and(|texto| texto != antes));

    // Abrir um arquivo com a direita em foco põe a aba nova nela — e o clique
    // que escolhe o arquivo é **no Explorer**, que fica do outro lado da tela.
    // Esse clique chega ao mesmo caminho que trata os cliques dos dois painéis,
    // e tratá-lo como clique na esquerda roubava o lado que ia receber.
    shell.pointer_down(Point::new(ACTIVITY_WIDTH + 40.0, 300.0), size);
    let Ok(_) = shell.open_file(&segundo) else {
        panic!("o segundo arquivo não abriu");
    };
    let Some(novo) = shell.editor_area.session.active_id() else {
        panic!("o arquivo aberto precisa ser o ativo");
    };
    let Some(divisao) = shell.editor_area.divisao.as_ref() else {
        panic!("a divisão precisa continuar existindo");
    };
    assert_eq!(divisao.ativa, novo, "a aba nova é do painel apontado");
    assert!(divisao.abas.contains(&novo));

    // E a faixa da esquerda continua acesa no arquivo **dela**. Com o mesmo
    // arquivo aberto dos dois lados, ela acendia a aba do documento ativo da
    // sessão — que é o do lado com foco —, e parecia que o editor da esquerda
    // tinha recebido o clique que foi na direita.
    assert_eq!(
        shell.left_active_document(),
        Some(documento),
        "a faixa da esquerda acende a aba do lado dela"
    );
    let _ = std::fs::remove_dir_all(&root);
}
/// Os três divisores anunciam que se movem antes de alguém os mover.
///
/// Só respondiam durante o arrasto, e um divisor que se anuncia depois de já ter
/// sido movido não anuncia nada: para descobrir que a linha se arrasta era
/// preciso arrastá-la por acidente.
#[test]
fn os_divisores_pedem_a_seta_antes_do_arrasto() {
    let root = std::env::temp_dir().join(format!("er-ide-setas-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    assert!(std::fs::create_dir_all(&root).is_ok());
    let Ok(shell) = IdeShell::open(&root) else {
        panic!("workspace de teste não abriu");
    };
    let size = Size::new(1280.0, 800.0);

    // A lateral: a linha entre o Explorer e o editor.
    let lateral = shell.sidebar_splitter_for(size);
    let sobre_a_lateral = Point::new(lateral.line().origin.x, 300.0);
    assert!(!shell.sidebar_resizing(), "ninguém está arrastando nada");
    assert!(shell.sidebar_divider_hover(sobre_a_lateral, size));
    assert!(!shell.sidebar_divider_hover(Point::new(sobre_a_lateral.x + 80.0, 300.0), size));

    // O terminal: a linha entre o editor e o painel de baixo.
    let terminal = shell.terminal_splitter_for(size);
    let sobre_o_terminal = Point::new(700.0, terminal.line().origin.y);
    assert!(!shell.terminal_resizing());
    assert!(shell.terminal_divider_hover(sobre_o_terminal, size));
    assert!(!shell.terminal_divider_hover(Point::new(700.0, sobre_o_terminal.y - 80.0), size));
    let _ = std::fs::remove_dir_all(&root);
}
/// Os dois itens da barra de atividades são botões, e cada um faz o seu.
///
/// Eram ícones pintados à mão: não acendiam sob o ponteiro, não recebiam clique
/// e não chegavam à árvore de acessibilidade. Pareciam ações, e não eram
/// nenhuma.
#[test]
fn a_barra_de_atividades_tem_tres_botoes() {
    let root = std::env::temp_dir().join(format!("er-ide-atividades-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    assert!(std::fs::create_dir_all(&root).is_ok());
    let Ok(mut shell) = IdeShell::open(&root) else {
        panic!("workspace de teste não abriu");
    };
    let size = Size::new(1280.0, 800.0);
    let _ = shell.paint(size);
    let largura_com_painel = shell.sidebar_width(size);
    assert!(largura_com_painel > 0.0);

    // A lupa abre a busca.
    let lupa = IdeShell::activity_rect(ACTIVITY_SEARCH_ID);
    shell.pointer_down(
        Point::new(lupa.origin.x + 12.0, lupa.origin.y + 12.0),
        size,
    );
    assert!(shell.type_search_open(), "a lupa abre a busca");
    shell.escape();

    // O outro recolhe o painel, e o que sobra vai para os outros.
    let painel = IdeShell::activity_rect(ACTIVITY_SIDEBAR_ID);
    let clique = Point::new(painel.origin.x + 12.0, painel.origin.y + 12.0);
    shell.pointer_down(clique, size);
    let _ = shell.paint(size);
    assert!(shell.sidebar_collapsed(), "o painel recolhe");
    assert_eq!(shell.sidebar_width(size), 0.0);
    assert!(
        shell.editor_view_rect(size).size.width > 0.0,
        "o editor fica com a largura que era do painel"
    );

    // Nada do painel sobra na tela: nem trilha de rolagem, nem a linha do
    // divisor — que descia até o rodapé e cruzava o terminal.
    let desenhado = shell.paint(size);
    let editor = shell.editor_view_rect(size);
    // Tudo o que ainda é legítimo depois de recolher fica na borda direita do
    // editor: as barras de rolagem dele. Antes dessa borda não pode sobrar
    // nenhuma linha vertical.
    let borda_direita = editor.origin.x + editor.size.width - 20.0;
    let restos = desenhado
        .iter()
        .filter(|comando| match comando {
            // Uma faixa **estreita e alta** encostada na barra de atividades:
            // é o formato de uma trilha de rolagem e o de uma linha de divisor.
            // O editor também começa ali agora, mas ele é largo — é a largura
            // que separa um do outro.
            PaintCommand::FillRect(fill) => {
                fill.rect.origin.x >= ACTIVITY_WIDTH - 12.0
                    && fill.rect.origin.x < borda_direita
                    && fill.rect.size.width > 0.0
                    && fill.rect.size.width <= 12.0
                    && fill.rect.size.height > TAB_HEIGHT
            }
            _ => false,
        })
        .count();
    assert_eq!(
        restos, 0,
        "com o painel recolhido nada dele pode continuar sendo desenhado"
    );

    // E o mesmo botão o traz de volta, com a largura que ele tinha.
    shell.pointer_down(clique, size);
    let _ = shell.paint(size);
    assert!(!shell.sidebar_collapsed());
    assert_eq!(shell.sidebar_width(size), largura_com_painel);
    let _ = std::fs::remove_dir_all(&root);
}
/// Um ponto resolve para **um** alvo, e a precedência é afirmável.
///
/// Antes o roteamento era uma fila de treze `if tratador(ponto) { return }`, e
/// cada tratador decidia se o ponto era dele **depois** de já ter agido. Aqui a
/// pergunta é separada da ação: dá para afirmar quem pega cada ponto sem simular
/// gesto nenhum e sem efeito colateral.
#[test]
fn cada_ponto_da_janela_tem_um_alvo_so() {
    let root = std::env::temp_dir().join(format!("er-ide-alvos-{}", std::process::id()));
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
    let size = Size::new(1280.0, 800.0);
    let _ = shell.paint(size);

    let geo = shell.geometry();
    let editor_x = ACTIVITY_WIDTH + shell.sidebar_width(size);
    assert_eq!(shell.alvo_do_ponto(Point::new(200.0, 10.0), size), Alvo::Topo);
    assert_eq!(
        shell.alvo_do_ponto(Point::new(20.0, 300.0), size),
        Alvo::Atividades
    );
    assert_eq!(
        shell.alvo_do_ponto(Point::new(editor_x + 100.0, TITLE_HEIGHT + 10.0), size),
        Alvo::Abas
    );
    assert_eq!(
        shell.alvo_do_ponto(Point::new(ACTIVITY_WIDTH + 40.0, EXPLORER_TOP + 40.0), size),
        Alvo::Explorer
    );
    assert_eq!(
        shell.alvo_do_ponto(Point::new(editor_x + 200.0, geo.content_top + 60.0), size),
        Alvo::Editor
    );
    assert_eq!(
        shell.alvo_do_ponto(Point::new(editor_x + 200.0, geo.editor_bottom + 40.0), size),
        Alvo::Terminal
    );

    // Dividido, a coluna da direita responde por si — e o clique no Explorer
    // continua sendo do Explorer, que é onde o defeito antigo morava.
    let Some(documento) = shell.editor_area.session.active_id() else {
        panic!("o arquivo aberto precisa ser o ativo");
    };
    shell.dividir_a_direita(documento);
    let _ = shell.paint(size);
    let Some(direita) = shell.right_editor_rect(size) else {
        panic!("a coluna da direita precisa ter área");
    };
    assert_eq!(
        shell.alvo_do_ponto(Point::new(direita.origin.x + 40.0, direita.origin.y + 40.0), size),
        Alvo::EditorDaDireita
    );
    assert_eq!(
        shell.alvo_do_ponto(Point::new(ACTIVITY_WIDTH + 40.0, EXPLORER_TOP + 40.0), size),
        Alvo::Explorer,
        "o clique no Explorer nunca é da área dividida"
    );
    let _ = std::fs::remove_dir_all(&root);
}
/// A janela mostra os quatro nós, e só o de branches tem conteúdo.
///
/// Os outros três aparecem vazios de propósito. Um nó que só existe depois que a
/// capacidade chega faria a tela mudar de forma a cada fase.
#[test]
fn o_painel_da_esquerda_tem_os_quatro_nos() {
    let mut shell = test_shell();
    let size = Size::new(1280.0, 800.0);
    let _ = shell.paint(size);
    // Nada alterado: este teste é sobre os quatro nós, e o lado direito com
    // três painéis tem teste próprio.
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
        ],
        ..GitView::default()
    });
    shell.toggle_git();
    let desenhado = shell.paint(size);
    let escrito = |texto: &str| {
        desenhado
            .iter()
            .any(|comando| matches!(comando, PaintCommand::DrawText(t) if t.text == texto))
    };

    assert!(escrito("Branches (2)"), "o nó das branches conta o que tem");
    for nome in ["Tags (0)", "Remotes (0)", "Stashes (0)"] {
        assert!(escrito(nome), "o nó {nome} aparece mesmo vazio");
    }
    // E o lado direito diz que não há o que mostrar, em vez de ficar vazio.
    assert!(escrito("Nada mudou desde o último commit"));
}
