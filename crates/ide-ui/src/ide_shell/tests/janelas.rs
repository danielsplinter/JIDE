//! Testes das **janelas sobrepostas**: gerar, renomear, novo item,
//! configurações e a troca de abas. Ver a `14`.

use super::*;

#[test]
fn contribution_catalog_generates_templates_settings_and_task_button() {
    let mut shell = test_shell();
    shell.set_ui_catalog(fake_catalog());
    assert_eq!(
        entry_labels(&explorer_menu_entries(
            Path::new("workspace/src"),
            &shell.catalog.source_root_names,
            &shell.catalog.new_item_templates,
            false,
        )),
        vec!["Novo módulo fake"]
    );
    open_java_settings(&mut shell, vec!["Fake SDK".to_owned()], 0);
    shell.set_ui_catalog(fake_catalog());
    let texts = shell
        .paint(Size::new(1_000.0, 700.0))
        .into_iter()
        .filter_map(|command| match command {
            PaintCommand::DrawText(text) => Some(text.text),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(texts.iter().any(|text| text == "Runtime fake"));
    assert!(texts.iter().any(|text| text == "Runtime"));
    shell.escape();

    let size = Size::new(1_000.0, 700.0);
    let run = action_areas(&mut shell, size)[1];
    shell.pointer_down(Point::new(run.origin.x + 2.0, run.origin.y + 2.0), size);
    assert!(
        shell
            .drain_application_commands()
            .contains(&ApplicationCommand::ExecuteTask(TaskId(
                "fake.run".to_owned()
            )))
    );
}
#[test]
fn settings_menu_opens_compiler_and_vm_page() {
    let mut shell = test_shell();
    let size = Size::new(1_000.0, 700.0);
    shell.pointer_down(Point::new(340.0, 10.0), size);
    assert!(shell.take_open_settings_request());

    open_java_settings(&mut shell, vec!["JDK 8".to_owned(), "JDK 17".to_owned()], 0);
    assert!(shell.settings_dialog_open());
    let paint = shell.paint(size);
    let labels = paint
        .iter()
        .filter_map(|command| match command {
            PaintCommand::DrawText(text) => Some(text.text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(labels.contains(&"Configurações"));
    assert!(labels.contains(&"Compilador e VM"));
    assert!(labels.contains(&"JDK 8"));
    assert!(labels.contains(&"Procurar..."));
    assert!(labels.contains(&"PowerShell"));
    assert!(labels.contains(&"ER IDE"));
    assert!(
        paint
            .iter()
            .any(|command| matches!(command, PaintCommand::LayerBreak))
    );
}
#[test]
fn settings_jdk_combo_and_browse_button_emit_requests() {
    let mut shell = test_shell();
    let size = Size::new(1_000.0, 700.0);
    open_java_settings(&mut shell, vec!["JDK 8".to_owned(), "JDK 17".to_owned()], 0);
    let geometry = {
        // A moldura vem do arranjo, e o arranjo acontece no quadro.
        let _ = shell.paint(size);
        shell.settings.geometry(&shell.host)
    };
    shell.pointer_down(
        Point::new(
            geometry.combo.origin.x + 10.0,
            geometry.combo.origin.y + 10.0,
        ),
        size,
    );
    shell.pointer_down(
        Point::new(
            geometry.combo.origin.x + 10.0,
            geometry.combo.origin.y + geometry.combo.size.height + 28.0 + 5.0,
        ),
        size,
    );
    // A escolha fica pendente: a janela é uma transação.
    assert_eq!(shell.take_settings_jdk_result(), None);

    shell.pointer_down(
        Point::new(
            geometry.browse.origin.x + 10.0,
            geometry.browse.origin.y + 10.0,
        ),
        size,
    );
    assert!(shell.take_browse_jdk_request());
}
/// Salvar aplica o que foi escolhido e fecha.
#[test]
fn saving_the_settings_applies_the_chosen_jdk() {
    let mut shell = test_shell();
    let size = Size::new(1_000.0, 700.0);
    open_java_settings(&mut shell, vec!["JDK 8".to_owned(), "JDK 17".to_owned()], 0);
    let geometry = open_settings_geometry(&mut shell, size);
    choose_second_jdk(&mut shell, &geometry, size);
    shell.pointer_down(
        Point::new(geometry.save.origin.x + 10.0, geometry.save.origin.y + 10.0),
        size,
    );
    assert_eq!(shell.take_settings_jdk_result(), Some(1));
    assert!(!shell.settings_dialog_open());
}
/// Cancelar descarta o que foi mexido, e o combo volta ao que estava.
#[test]
fn cancelling_the_settings_discards_every_change() {
    let mut shell = test_shell();
    let size = Size::new(1_000.0, 700.0);
    open_java_settings(&mut shell, vec!["JDK 8".to_owned(), "JDK 17".to_owned()], 0);
    let geometry = open_settings_geometry(&mut shell, size);
    choose_second_jdk(&mut shell, &geometry, size);
    shell.pointer_down(
        Point::new(
            geometry.close.origin.x + 10.0,
            geometry.close.origin.y + 10.0,
        ),
        size,
    );
    assert_eq!(shell.take_settings_jdk_result(), None);
    assert!(!shell.settings_dialog_open());
    assert_eq!(shell.settings.selected_toolchain(), 0);
}
/// A mesma janela serve as três ações, mudando só o título e a legenda.
#[test]
fn the_three_menu_actions_share_one_window() {
    let mut shell = shell_with_package();
    shell.context_menu.alvo = Some(AlvoDoMenu::Explorer {
        pasta: PathBuf::from("demo/src/main/java/br/com"),
        arquivo: None,
    });
    for (command, title) in [
        ("explorer.new.java.package", "Novo pacote"),
        ("explorer.new.java.class", "Nova classe"),
        ("explorer.new.java.interface", "Nova interface"),
    ] {
        shell.run_context_command(command);
        assert_eq!(shell.new_item.title(), Some(title));
        assert_eq!(NewItemSurface::values(&shell.host).0, "br.com");
    }
}
/// A janela lista só o que falta, e `OK` gera o que foi marcado.
#[test]
fn the_generate_window_lists_what_is_missing_and_writes_what_was_checked() {
    let mut shell = shell_editing("class Matricula {\n    private Long id;\n}\n");
    let size = Size::new(1280.0, 800.0);
    shell.show_accessor_plan(AccessorKind::Getter, accessor_plan_para_teste());
    assert!(shell.generate_open());
    assert_eq!(
        shell.generate_fields(),
        vec!["id", "ativo"],
        "o campo que já tem getter não é oferecido"
    );

    // Os nomes aparecem na janela.
    let texts = painted_texts(&mut shell, size);
    assert!(texts.iter().any(|text| text == "id"), "{texts:?}");
    assert!(texts.iter().any(|text| text == "All"), "{texts:?}");
    assert!(texts.iter().any(|text| text == "OK"), "{texts:?}");

    // Marcar a segunda linha e confirmar gera só ela.
    let (lista, _, ok) = {
        // As áreas vêm do arranjo, e o arranjo acontece no quadro.
        let _ = shell.paint(size);
        GenerateSurface::areas(&shell.host)
    };
    shell.pointer_down(
        Point::new(
            lista.origin.x + 40.0,
            lista.origin.y + generate::ROW_HEIGHT + 4.0,
        ),
        size,
    );
    shell.pointer_down(Point::new(ok.origin.x + 10.0, ok.origin.y + 10.0), size);
    assert!(!shell.generate_open(), "confirmar fecha a janela");
    let texto = shell.active_text().unwrap_or_default();
    assert!(texto.contains("isAtivo"), "o marcado foi gerado: {texto}");
    assert!(!texto.contains("getId"), "o não marcado ficou de fora");
}
/// `All` gera todos, esteja marcado ou não.
#[test]
fn the_all_button_ignores_the_checkboxes() {
    let mut shell = shell_editing("class Matricula {\n    private Long id;\n}\n");
    let size = Size::new(1280.0, 800.0);
    shell.show_accessor_plan(AccessorKind::Getter, accessor_plan_para_teste());
    let (_, todos, _) = {
        // As áreas vêm do arranjo, e o arranjo acontece no quadro.
        let _ = shell.paint(size);
        GenerateSurface::areas(&shell.host)
    };
    shell.pointer_down(
        Point::new(todos.origin.x + 10.0, todos.origin.y + 10.0),
        size,
    );
    let texto = shell.active_text().unwrap_or_default();
    assert!(texto.contains("getId"), "{texto}");
    assert!(texto.contains("isAtivo"), "{texto}");
    assert!(!shell.generate_open());
}
/// A lista da janela sobrevive aos quadros: roda e clique funcionam.
///
/// Recriá-la a cada pintura jogava fora a rolagem e a deixava sem receber
/// evento nenhum — a barra não se movia e o clique não chegava.
#[test]
fn the_generate_list_keeps_its_scroll_between_frames() {
    let mut shell = shell_editing("class Muitos {}\n");
    let size = Size::new(1280.0, 800.0);
    let candidatos: Vec<AccessorCandidate> = (0..40)
        .map(|index| AccessorCandidate {
            field: format!("campo{index}"),
            source: Some(format!(
                "\n    public int getCampo{index}() {{ return 0; }}\n"
            )),
        })
        .collect();
    shell.show_accessor_plan(
        AccessorKind::Getter,
        AccessorPlan {
            candidates: candidatos,
            insert_at: DomainTextPosition { line: 1, column: 0 },
        },
    );
    let _ = shell.paint(size);

    let rolagem = |shell: &IdeShell| shell.generate.list_scroll();
    assert_eq!(rolagem(&shell), 0.0);

    // A roda rola a lista da janela.
    let (lista, ..) = {
        // As áreas vêm do arranjo, e o arranjo acontece no quadro.
        let _ = shell.paint(size);
        GenerateSurface::areas(&shell.host)
    };
    shell.scroll(
        Point::new(lista.origin.x + 40.0, lista.origin.y + 40.0),
        5.0,
        size,
    );
    let apos_roda = rolagem(&shell);
    assert!(apos_roda > 0.0, "a roda precisa mover a lista");

    // Pintar de novo não desfaz a rolagem.
    let _ = shell.paint(size);
    assert_eq!(rolagem(&shell), apos_roda, "a lista sobrevive ao quadro");

    // E o clique numa linha visível continua marcando.
    shell.pointer_down(
        Point::new(lista.origin.x + 40.0, lista.origin.y + 4.0),
        size,
    );
    let marcados = shell.generate.checked_count();
    assert_eq!(marcados, 1, "clicar numa linha marca uma");
}
/// Os três itens do menu usam a mesma janela.
///
/// Getter, setter e o par diferem no que a linguagem escreve, não na tela:
/// duplicar a janela daria duas cópias que divergiriam na primeira correção.
#[test]
fn the_three_generate_options_share_one_window() {
    for kind in [
        AccessorKind::Getter,
        AccessorKind::Setter,
        AccessorKind::Both,
    ] {
        let mut shell = shell_editing("class Matricula {\n}\n");
        let size = Size::new(1280.0, 800.0);
        let fonte = match kind {
            AccessorKind::Getter => "\n    public Long getId() { return id; }\n",
            AccessorKind::Setter => "\n    public void setId(Long id) {}\n",
            AccessorKind::Both => {
                "\n    public Long getId() { return id; }\n    public void setId(Long id) {}\n"
            }
            // O construtor não gera texto por campo, e por isso tem teste
            // próprio: aqui ele não teria o que comparar.
            AccessorKind::Constructor => continue,
        };
        shell.show_accessor_plan(
            kind,
            AccessorPlan {
                candidates: vec![AccessorCandidate {
                    field: "id".to_owned(),
                    source: Some(fonte.to_owned()),
                }],
                insert_at: DomainTextPosition { line: 1, column: 0 },
            },
        );
        assert!(shell.generate_open(), "a mesma janela abre para {kind:?}");
        assert_eq!(shell.generate_fields(), vec!["id"]);

        let (_, todos, _) = {
            let _ = shell.paint(size);
            GenerateSurface::areas(&shell.host)
        };
        shell.pointer_down(
            Point::new(todos.origin.x + 10.0, todos.origin.y + 10.0),
            size,
        );
        let texto = shell.active_text().unwrap_or_default();
        match kind {
            AccessorKind::Getter => assert!(texto.contains("getId"), "{texto}"),
            AccessorKind::Setter => assert!(texto.contains("setId"), "{texto}"),
            AccessorKind::Both => {
                assert!(texto.contains("getId"), "o par gera os dois: {texto}");
                assert!(texto.contains("setId"), "o par gera os dois: {texto}");
            }
            AccessorKind::Constructor => unreachable!("descartado acima"),
        }
    }
}
/// Clicar e arrastar a barra da janela rola a lista.
///
/// O widget já sabia arrastar; o que faltava era a janela entregar o
/// **movimento** e a **soltura** a ele — só o clique chegava, e o indicador
/// era pego e nunca andava.
#[test]
fn dragging_the_rename_scrollbar_scrolls_the_list() {
    let mut shell = shell_editing(
        "public class Pedido {
}
",
    );
    let size = Size::new(1280.0, 800.0);
    let local = |arquivo: &str, linha: u32| Location {
        path: PathBuf::from(arquivo),
        range: DomainTextRange {
            start: DomainTextPosition {
                line: linha,
                column: 0,
            },
            end: DomainTextPosition {
                line: linha,
                column: 6,
            },
        },
    };
    shell.show_rename(
        PathBuf::from("src/Pedido.java"),
        (0..60)
            .map(|indice| local(&format!("src/Arquivo{indice}.java"), indice))
            .collect(),
    );
    let _ = shell.paint(size);

    // A área da lista é da janela: o teste aponta o gesto por ela, e não por
    // dentro do estado do shell.
    let lista = { shell.rename.list_area(&shell.host) };
    let trilha_x = lista.origin.x + lista.size.width - 5.0;
    let topo = lista.origin.y + 4.0;

    shell.pointer_down(Point::new(trilha_x, topo), size);
    shell.pointer_move(Point::new(trilha_x, topo + 120.0), size);
    let rolou = shell.rename.list_scroll();
    assert!(rolou > 0.0, "arrastar a barra precisa rolar a lista");

    shell.pointer_up();
    // Solto o gesto, mover o ponteiro não arrasta mais nada.
    shell.pointer_move(Point::new(trilha_x, topo), size);
    let depois = shell.rename.list_scroll();
    assert_eq!(
        depois, rolou,
        "sem botão pressionado a barra fica onde está"
    );
}
/// Com a janela de renomear aberta, a roda é dela — não do editor atrás.
///
/// Rolar o que está coberto é mexer no que não se vê: o usuário achava que
/// estava percorrendo a lista e estava movendo o arquivo por baixo dela.
#[test]
fn the_wheel_belongs_to_the_rename_window_and_not_to_the_editor_behind() {
    let texto: String = (0..200).map(|linha| format!("linha {linha}\n")).collect();
    let mut shell = shell_editing(&texto);
    let size = Size::new(1280.0, 800.0);
    let _ = shell.paint(size);
    let antes = shell.editor_scroll_line();

    let local = |arquivo: &str, linha: u32| Location {
        path: PathBuf::from(arquivo),
        range: DomainTextRange {
            start: DomainTextPosition {
                line: linha,
                column: 0,
            },
            end: DomainTextPosition {
                line: linha,
                column: 6,
            },
        },
    };
    shell.show_rename(
        PathBuf::from("src/Pedido.java"),
        (0..40)
            .map(|indice| local(&format!("src/Arquivo{indice}.java"), indice))
            .collect(),
    );
    let _ = shell.paint(size);

    // A roda sobre a janela não pode mover o editor coberto.
    let centro = Point::new(size.width / 2.0, size.height / 2.0);
    for _ in 0..10 {
        shell.scroll(centro, 3.0, size);
    }
    assert_eq!(
        shell.editor_scroll_line(),
        antes,
        "o editor atrás da janela não pode rolar"
    );
}
/// A segunda escolha da seção recebe clique no combo e no botão.
///
/// Ela era desenhada e não respondia: a janela roteava o clique só para a
/// primeira. É a mesma falha de costura de sempre — o widget certo, o
/// caminho até ele faltando.
#[test]
fn the_secondary_tool_answers_the_pointer() {
    let mut shell = test_shell();
    let mut catalog = java_catalog();
    if let Some(section) = catalog.settings_sections.first_mut() {
        section.secondary_caption = Some("Maven".to_owned());
    }
    shell.set_ui_catalog(catalog);
    let size = Size::new(1280.0, 800.0);
    shell.set_secondary_tool_options(
        vec![
            "Maven 3.9.6 — /opt/maven".to_owned(),
            "Maven 3.8.8 — /usr/share/maven".to_owned(),
        ],
        Some(0),
    );
    shell.open_settings_dialog(vec!["JDK 21".to_owned()], 0);
    let _ = shell.paint(size);

    let geometry = shell.settings.geometry(&shell.host);

    // Abrir a lista e escolher a segunda opção.
    let combo = geometry.secondary_combo;
    shell.pointer_down(
        Point::new(
            combo.origin.x + 20.0,
            combo.origin.y + combo.size.height / 2.0,
        ),
        size,
    );
    shell.pointer_down(
        Point::new(
            combo.origin.x + 20.0,
            combo.origin.y + combo.size.height + combo.size.height * 1.5,
        ),
        size,
    );
    assert_eq!(
        shell.selected_secondary_tool(),
        Some(1),
        "clicar na lista precisa mudar a escolha"
    );

    // O botão põe o pedido de procurar na fila da aplicação.
    let browse = geometry.secondary_browse;
    shell.pointer_down(
        Point::new(
            browse.origin.x + browse.size.width / 2.0,
            browse.origin.y + browse.size.height / 2.0,
        ),
        size,
    );
    assert!(
        shell
            .drain_application_commands()
            .iter()
            .any(|comando| matches!(
                comando,
                ApplicationCommand::BrowseTool {
                    role: ToolRole::Secondary,
                    ..
                }
            )),
        "o botão precisa pedir o seletor de pasta"
    );
}
/// A janela mostra o nome do arquivo e todos os arquivos afetados.
#[test]
fn the_rename_window_lists_every_affected_file() {
    let mut shell = shell_editing("public class Pedido {\n}\n");
    let local = |arquivo: &str, linha: u32, de: u32, ate: u32| Location {
        path: PathBuf::from(arquivo),
        range: DomainTextRange {
            start: DomainTextPosition {
                line: linha,
                column: de,
            },
            end: DomainTextPosition {
                line: linha,
                column: ate,
            },
        },
    };
    shell.show_rename(
        PathBuf::from("src/Pedido.java"),
        vec![
            local("src/Pedido.java", 0, 13, 19),
            local("src/Servico.java", 4, 8, 14),
            local("src/Servico.java", 9, 12, 18),
        ],
    );

    assert!(shell.rename_open());
    assert_eq!(
        shell.rename_name(),
        "Pedido",
        "o campo começa com o nome atual"
    );
    let lista = shell.rename_references();
    assert_eq!(lista.len(), 2, "dois arquivos afetados: {lista:?}");
    assert!(
        lista
            .iter()
            .any(|item| item.contains("Servico.java") && item.ends_with("2")),
        "a lista diz quantas ocorrências cada arquivo tem: {lista:?}"
    );
    assert!(
        lista.iter().any(|item| item.contains("Pedido.java")),
        "o próprio arquivo entra: nele estão a declaração e os construtores"
    );

    // Pelos dois caminhos: a tecla no shell e o `escape` que a janela usa.
    shell.key_down("Escape");
    assert!(!shell.rename_open());
    shell.show_rename(PathBuf::from("src/Pedido.java"), Vec::new());
    assert!(shell.rename_open());
    shell.escape();
    assert!(
        !shell.rename_open(),
        "Esc precisa fechar por `escape` também"
    );
}
/// Confirmar reescreve o que está aberto e manda o resto para a aplicação.
#[test]
fn confirming_rewrites_open_files_and_delegates_the_closed_ones() {
    let mut shell = test_shell();
    let aberto = shell.editor_area.session.open_memory(
        "src/Pedido.java",
        "public class Pedido {\n    Pedido() {}\n}\n",
    );
    shell.context.focus = ShellFocus::Editor;
    let local = |arquivo: &str, linha: u32, de: u32, ate: u32| Location {
        path: PathBuf::from(arquivo),
        range: DomainTextRange {
            start: DomainTextPosition {
                line: linha,
                column: de,
            },
            end: DomainTextPosition {
                line: linha,
                column: ate,
            },
        },
    };
    shell.show_rename(
        PathBuf::from("src/Pedido.java"),
        vec![
            local("src/Pedido.java", 0, 13, 19),
            local("src/Pedido.java", 1, 4, 10),
            local("src/Servico.java", 4, 8, 14),
        ],
    );

    for _ in 0..6 {
        shell.key_down("Backspace");
    }
    shell.text_input("Compra");
    shell.key_down("Enter");
    assert!(!shell.rename_open());

    // O arquivo aberto foi reescrito no buffer, com aba e desfazer intactos.
    let texto = shell.document_text(aberto).unwrap_or_default();
    assert!(texto.contains("public class Compra"), "{texto}");
    assert!(
        texto.contains("Compra() {}"),
        "o construtor acompanha: {texto}"
    );

    // O fechado vai no pedido, junto do arquivo a mover.
    let pedido = shell
        .drain_application_commands()
        .into_iter()
        .find_map(|comando| match comando {
            ApplicationCommand::RenameDocument(request) => Some(request),
            _ => None,
        });
    let Some(pedido) = pedido else {
        panic!("confirmar precisa pedir a renomeação do arquivo");
    };
    assert_eq!(pedido.from, PathBuf::from("src/Pedido.java"));
    assert_eq!(pedido.to, PathBuf::from("src/Compra.java"));
    assert_eq!(pedido.old_name, "Pedido");
    assert_eq!(pedido.new_name, "Compra");
    assert_eq!(
        pedido.occurrences.len(),
        1,
        "só o arquivo fechado: o aberto a tela já reescreveu"
    );
    assert_eq!(
        pedido.occurrences[0].path,
        PathBuf::from("src/Servico.java")
    );
}
/// O construtor usa a mesma janela, e a escolha vira o pedido à linguagem.
///
/// A tela não escreve construtor nenhum: ela decide **quais campos** e
/// entrega a lista. Marcar nada é um pedido vazio — o construtor sem
/// parâmetros —, e o botão que gera tudo manda todos os campos.
#[test]
fn the_constructor_uses_the_same_window_and_asks_the_language() {
    let plano = || AccessorPlan {
        candidates: vec![
            AccessorCandidate {
                field: "id".to_owned(),
                // O construtor não traz texto por campo: ele é montado depois.
                source: None,
            },
            AccessorCandidate {
                field: "nome".to_owned(),
                source: None,
            },
        ],
        insert_at: DomainTextPosition { line: 1, column: 0 },
    };
    let size = Size::new(1280.0, 800.0);

    // Sem marcar nada, o OK pede um construtor sem parâmetros.
    let mut shell = shell_editing("class Pedido {\n}\n");
    shell.show_accessor_plan(AccessorKind::Constructor, plano());
    assert!(shell.generate_open(), "a janela é a mesma");
    assert_eq!(
        shell.generate_fields(),
        vec!["id", "nome"],
        "o construtor lista todos os campos, e não só os que faltam"
    );
    let (_, _, ok) = {
        // As áreas vêm do arranjo, e o arranjo acontece no quadro.
        let _ = shell.paint(size);
        GenerateSurface::areas(&shell.host)
    };
    shell.pointer_down(Point::new(ok.origin.x + 10.0, ok.origin.y + 10.0), size);
    let Some((campos, onde)) = shell.take_constructor_request() else {
        panic!("o OK precisa deixar um pedido de construtor");
    };
    assert!(
        campos.is_empty(),
        "nada marcado é o construtor sem parâmetros"
    );
    assert_eq!(onde.line, 1);

    // Marcando um campo, só ele vai no pedido.
    let mut shell = shell_editing("class Pedido {\n}\n");
    shell.show_accessor_plan(AccessorKind::Constructor, plano());
    let (lista, _, ok) = {
        // As áreas vêm do arranjo, e o arranjo acontece no quadro.
        let _ = shell.paint(size);
        GenerateSurface::areas(&shell.host)
    };
    shell.pointer_down(
        Point::new(lista.origin.x + 20.0, lista.origin.y + 12.0),
        size,
    );
    shell.pointer_down(Point::new(ok.origin.x + 10.0, ok.origin.y + 10.0), size);
    let Some((campos, _)) = shell.take_constructor_request() else {
        panic!("o OK precisa deixar um pedido de construtor");
    };
    assert_eq!(campos, vec!["id"], "só o campo marcado entra");

    // O botão que gera tudo manda todos, sem depender da marcação.
    let mut shell = shell_editing("class Pedido {\n}\n");
    shell.show_accessor_plan(AccessorKind::Constructor, plano());
    let (_, todos, _) = {
        // As áreas vêm do arranjo, e o arranjo acontece no quadro.
        let _ = shell.paint(size);
        GenerateSurface::areas(&shell.host)
    };
    shell.pointer_down(
        Point::new(todos.origin.x + 10.0, todos.origin.y + 10.0),
        size,
    );
    let Some((campos, _)) = shell.take_constructor_request() else {
        panic!("o botão All precisa deixar um pedido de construtor");
    };
    assert_eq!(campos, vec!["id", "nome"]);
}
/// O texto do construtor vem da linguagem e é escrito onde ela mandou.
#[test]
fn the_constructor_text_from_the_language_is_written_at_the_given_line() {
    let mut shell = shell_editing("class Pedido {\n}\n");
    let onde = DomainTextPosition { line: 1, column: 0 };
    let fonte = "\n    public Pedido(Long id) {\n        this.id = id;\n    }\n";
    assert!(shell.insert_constructor(Some(fonte.to_owned()), onde));
    let texto = shell.active_text().unwrap_or_default();
    assert!(texto.contains("public Pedido(Long id)"), "{texto}");

    // Assinatura repetida: a linguagem devolve nada, e nada é escrito.
    let antes = shell.active_text().unwrap_or_default().to_owned();
    assert!(!shell.insert_constructor(None, onde));
    assert_eq!(shell.active_text().unwrap_or_default(), antes);
    assert!(shell.status_message().contains("já existe"));
}
/// Sem nada a gerar, a janela nem abre.
#[test]
fn nothing_to_generate_does_not_open_a_window() {
    let mut shell = shell_editing("class Matricula {}\n");
    shell.show_accessor_plan(
        AccessorKind::Getter,
        AccessorPlan {
            candidates: vec![AccessorCandidate {
                field: "nome".to_owned(),
                source: None,
            }],
            insert_at: DomainTextPosition { line: 0, column: 0 },
        },
    );
    assert!(!shell.generate_open());
    assert_eq!(
        shell.status_message(),
        "Todos os campos já têm esse acessor"
    );
}
/// A sequência real: `Ctrl+D` pela shell e depois digitar.
///
/// O teste do painel passava e a IDE não, então o caminho que o app percorre
/// é que precisa ser exercido — do atalho à digitação, pelas mesmas portas.
#[test]
fn marking_and_typing_through_the_shell_edits_every_occurrence() {
    let mut shell = shell_editing("nome = nome + nome");
    shell.context.focus = ShellFocus::Editor;
    // Cursor no fim do trecho, como fica ao selecionar arrastando.
    shell.editor_area.pane.set_cursor(4);
    shell.editor_area.pane.set_selection(Some((0, 4)));

    shell.key_down_with_modifiers(
        "d",
        Modifiers {
            control: true,
            ..Modifiers::default()
        },
    );
    assert_eq!(
        shell.editor_area.pane.occurrences(),
        vec![(0, 4), (7, 11)],
        "o atalho precisa marcar pela shell, não só pelo painel"
    );

    // Cada marca é um cursor: o que estava lá permanece, e a letra digitada
    // entra em todas as ocorrências.
    shell.text_input("s");
    assert_eq!(shell.active_text(), Some("nomes = nomes + nome"));
    shell.text_input("!");
    assert_eq!(
        shell.active_text(),
        Some("nomes! = nomes! + nome"),
        "a segunda letra também é replicada"
    );
    shell.key_down("Backspace");
    shell.key_down("Backspace");
    assert_eq!(
        shell.active_text(),
        Some("nome = nome + nome"),
        "apagar tira uma letra de cada, voltando ao começo"
    );
}
/// Com uma janela na frente, a tecla digitada não é do editor.
///
/// **Existir documento ativo não significa que ele esteja recebendo alguma
/// coisa.** Com a busca aberta há um arquivo aberto atrás dela, e quem
/// perguntasse só pelo documento ativo receberia `Some` e agiria como se a tecla
/// fosse dele: digitar um ponto na caixa de busca abria o menu de completação do
/// editor, sobre uma janela que não era a dele.
///
/// A pergunta vale para toda janela sobreposta, e não só para a busca — é por
/// isso que a resposta é uma só.
#[test]
fn a_key_typed_over_an_open_window_does_not_belong_to_the_editor() {
    let mut shell = shell_editing("int total = 10;");
    shell.context.focus = ShellFocus::Editor;
    assert!(
        shell.text_reaches_editor(),
        "sem janela na frente e com o editor em foco, a tecla é dele"
    );
    assert!(
        shell.active_document().is_some(),
        "há documento aberto, e é isso que enganava"
    );

    shell.open_type_search();
    assert!(
        !shell.text_reaches_editor(),
        "com a busca na frente, a tecla não é do editor"
    );
    assert!(
        shell.active_document().is_some(),
        "o documento continua ativo atrás da janela"
    );

    shell.close_type_search();
    assert!(
        shell.text_reaches_editor(),
        "fechada a janela, quem decide volta a ser o foco"
    );
}
/// A janela nasce no centro da tela.
///
/// O painel se centraliza na área que recebe no layout; sem esse layout a
/// área era zero e ele aparecia no canto superior esquerdo.
#[test]
fn the_new_item_dialog_opens_centered() {
    let mut shell = shell_with_package();
    let size = Size::new(1280.0, 800.0);
    shell.context_menu.alvo = Some(AlvoDoMenu::Explorer {
        pasta: PathBuf::from("demo/src/main/java/br/com"),
        arquivo: None,
    });
    shell.run_context_command("explorer.new.java.class");
    // O painel do `ModalHost` é o retângulo de superfície desenhado sobre o
    // véu, do tamanho declarado para a janela.
    let surface = shell.context.theme.colors.surface;
    let panel = shell
        .paint(size)
        .iter()
        .filter_map(|command| match command {
            PaintCommand::FillRect(fill) if fill.color == surface => Some(fill.rect),
            _ => None,
        })
        .find(|rect| rect.size == new_item::PANEL_SIZE)
        .unwrap_or_default();
    let center_x = panel.origin.x + panel.size.width / 2.0;
    let center_y = panel.origin.y + panel.size.height / 2.0;
    assert!(
        (center_x - size.width / 2.0).abs() < 1.0,
        "centro horizontal em {center_x}, esperado {}",
        size.width / 2.0
    );
    assert!(
        (center_y - size.height / 2.0).abs() < 1.0,
        "centro vertical em {center_y}, esperado {}",
        size.height / 2.0
    );
}
/// Esc fecha sem pedir nada.
#[test]
fn escape_closes_the_new_item_dialog_without_creating() {
    let mut shell = shell_with_package();
    shell.context_menu.alvo = Some(AlvoDoMenu::Explorer {
        pasta: PathBuf::from("demo/src/main/java/br/com"),
        arquivo: None,
    });
    shell.run_context_command("explorer.new.java.class");
    shell.escape();
    assert!(!shell.new_item_dialog_open());
    assert_eq!(shell.take_new_item_request(), None);
}
/// Esc é cancelar: fechar sem descartar salvaria pela porta dos fundos.
#[test]
fn escape_in_the_settings_discards_every_change() {
    let mut shell = test_shell();
    let size = Size::new(1_000.0, 700.0);
    open_java_settings(&mut shell, vec!["JDK 8".to_owned(), "JDK 17".to_owned()], 0);
    let geometry = open_settings_geometry(&mut shell, size);
    choose_second_jdk(&mut shell, &geometry, size);
    shell.escape();
    assert_eq!(shell.take_settings_jdk_result(), None);
    assert!(!shell.settings_dialog_open());
    assert_eq!(shell.settings.selected_toolchain(), 0);
}
/// `Ctrl+Tab` percorre as abas, e soltar o `Ctrl` ativa a escolhida.
///
/// O gesto tem três partes e nenhuma vale sozinha: segurar abre a janela na
/// **próxima** aba, cada `Tab` desce um item, e a soltura conclui. Enquanto o
/// `Ctrl` está segurado o editor não muda — o que anda é o destaque.
#[test]
fn holding_control_and_tabbing_walks_the_open_tabs() {
    let root = PathBuf::from("workspace");
    let mut shell = IdeShell::from_tree(FileNode {
        path: root.clone(),
        is_directory: true,
        children: Vec::new(),
    });
    for nome in ["Primeiro.java", "Segundo.java", "Terceiro.java"] {
        shell
            .editor_area
            .session
            .open_memory(root.join(nome), "class A {}");
    }
    let terceiro = root.join("Terceiro.java");
    // Documento em memória não é persistente, e `active_document_path` filtra
    // por isso: quem responde aqui é a sessão.
    let ativo = |shell: &IdeShell| -> Option<PathBuf> {
        shell
            .editor_area
            .session
            .active()
            .map(|documento| documento.path.clone())
    };
    assert_eq!(
        ativo(&shell),
        Some(terceiro.clone()),
        "a última aberta é a ativa"
    );

    let ctrl = Modifiers {
        control: true,
        ..Modifiers::default()
    };
    // Primeiro `Tab`: a janela abre já na próxima aba — que, da última, é a
    // primeira. Nada mudou no editor ainda.
    shell.key_down_with_modifiers("Tab", ctrl);
    assert!(shell.tab_switcher.is_open(), "a janela precisa abrir");
    assert_eq!(shell.tab_switcher.scroll_state().0, 0);
    assert_eq!(
        ativo(&shell),
        Some(terceiro.clone()),
        "segurar o Ctrl nao pode trocar de aba ainda"
    );

    // Segundo `Tab`: desce mais um.
    shell.key_down_with_modifiers("Tab", ctrl);
    assert_eq!(shell.tab_switcher.scroll_state().0, 1);
    assert_eq!(ativo(&shell), Some(terceiro));

    // Soltar o `Ctrl` conclui.
    assert!(shell.release_control(), "a soltura precisa concluir");
    assert!(!shell.tab_switcher.is_open(), "e fechar a janela");
    assert_eq!(
        ativo(&shell),
        Some(root.join("Segundo.java")),
        "a aba destacada precisa ficar ativa"
    );
    // Soltar de novo, sem janela aberta, nao faz nada.
    assert!(!shell.release_control());
}
/// A lista dá a volta, e uma aba só não abre janela nenhuma.
#[test]
fn the_tab_list_wraps_and_a_single_tab_opens_nothing() {
    let root = PathBuf::from("workspace");
    let mut shell = IdeShell::from_tree(FileNode {
        path: root.clone(),
        is_directory: true,
        children: Vec::new(),
    });
    let ctrl = Modifiers {
        control: true,
        ..Modifiers::default()
    };

    // Uma aba só: não há para onde ir, e uma janela que não muda nada é ruído.
    shell
        .editor_area
        .session
        .open_memory(root.join("Unico.java"), "class A {}");
    shell.key_down_with_modifiers("Tab", ctrl);
    assert!(!shell.tab_switcher.is_open());
    assert!(!shell.release_control());

    // Com duas, o ciclo dá a volta e volta ao começo.
    shell
        .editor_area
        .session
        .open_memory(root.join("Outro.java"), "class B {}");
    shell.key_down_with_modifiers("Tab", ctrl);
    assert_eq!(shell.tab_switcher.scroll_state().0, 0);
    shell.key_down_with_modifiers("Tab", ctrl);
    assert_eq!(shell.tab_switcher.scroll_state().0, 1);
    shell.key_down_with_modifiers("Tab", ctrl);
    assert_eq!(
        shell.tab_switcher.scroll_state().0,
        0,
        "do fim da lista, o proximo e o comeco"
    );
}
