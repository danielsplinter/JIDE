//! Testes do shell da IDE.
//!
//! Separados do código por serem 40% do arquivo: enquanto dividiam o mesmo
//! espaço, qualquer movimentação aparecia misturada ao que não se moveu.

use super::*;
use ui_core::Color;
// Tipos que o shell deixou de importar quando as funções puras saíram; os
// testes continuam falando deles.
use crate::debugging::DebugVariableView;
use crate::ide_shell::inspection::InspectionGeometry;
use crate::ide_shell::settings::SettingsDialogGeometry;
use crate::search::{ContentSearchHit, TypeSearchHit};
use ide_application::{NewItemRequest, NewItemTemplate};
use ide_domain::{AccessorCandidate, AccessorPlan, Location, SyntaxHighlightKind, ToolRole};
use ui_editor::TokenKind;

fn java_source_roots() -> Vec<String> {
    vec!["java".to_owned()]
}

fn java_catalog() -> UiContributionCatalog {
    UiContributionCatalog {
        language_names: vec!["Java".to_owned()],
        source_root_names: java_source_roots(),
        new_item_templates: vec![
            NewItemTemplate {
                id: NewItemTemplateId::new("java.package"),
                title: "Novo pacote".to_owned(),
                name_caption: "Classe (opcional)".to_owned(),
                file_extension: None,
                allows_empty_name: true,
            },
            NewItemTemplate {
                id: NewItemTemplateId::new("java.class"),
                title: "Nova classe".to_owned(),
                name_caption: "Nome da classe".to_owned(),
                file_extension: Some("java".to_owned()),
                allows_empty_name: false,
            },
            NewItemTemplate {
                id: NewItemTemplateId::new("java.interface"),
                title: "Nova interface".to_owned(),
                name_caption: "Nome da interface".to_owned(),
                file_extension: Some("java".to_owned()),
                allows_empty_name: false,
            },
        ],
        settings_sections: vec![SettingsSection {
            id: "java.compiler-vm".to_owned(),
            title: "Compilador e VM".to_owned(),
            field_caption: "JDK".to_owned(),
            browse_button_title: "Procurar...".to_owned(),
            secondary_caption: None,
        }],
        tasks: vec![TaskDescriptor {
            id: TaskId("java.run".to_owned()),
            title: "Executar".to_owned(),
            requires_active_document: true,
            show_in_toolbar: true,
        }],
    }
}

fn fake_catalog() -> UiContributionCatalog {
    UiContributionCatalog {
        language_names: vec!["Fake".to_owned()],
        source_root_names: vec!["src".to_owned()],
        new_item_templates: vec![NewItemTemplate {
            id: NewItemTemplateId::new("fake.module"),
            title: "Novo módulo fake".to_owned(),
            name_caption: "Nome do módulo".to_owned(),
            file_extension: Some("fake".to_owned()),
            allows_empty_name: false,
        }],
        settings_sections: vec![SettingsSection {
            id: "fake.runtime".to_owned(),
            title: "Runtime fake".to_owned(),
            field_caption: "Runtime".to_owned(),
            browse_button_title: "Localizar...".to_owned(),
            secondary_caption: None,
        }],
        tasks: vec![TaskDescriptor {
            id: TaskId("fake.run".to_owned()),
            title: "Executar fake".to_owned(),
            requires_active_document: false,
            show_in_toolbar: true,
        }],
    }
}

fn open_java_settings(shell: &mut IdeShell, items: Vec<String>, selected: usize) {
    shell.set_ui_catalog(java_catalog());
    shell.open_settings_dialog(items, selected);
}

fn test_shell() -> IdeShell {
    let root = PathBuf::from("workspace");
    let directory = root.join("src");
    IdeShell::from_tree(FileNode {
        path: root,
        is_directory: true,
        children: vec![FileNode {
            path: directory,
            is_directory: true,
            children: Vec::new(),
        }],
    })
}

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

/// O Explorer desenha pela árvore da biblioteca, e a rolagem horizontal
/// desloca as linhas em vez de cortá-las.
#[test]
fn the_explorer_paints_through_the_tree_and_slides_horizontally() {
    let root = PathBuf::from("workspace");
    let mut shell = IdeShell::from_tree(FileNode {
        path: root.clone(),
        is_directory: true,
        children: vec![FileNode {
            path: root.join("um_arquivo_de_nome_bem_longo_para_exceder_o_painel.rs"),
            is_directory: false,
            children: Vec::new(),
        }],
    });
    let size = Size::new(1280.0, 800.0);
    let origin_of = |shell: &mut IdeShell| {
        shell
            .paint(size)
            .iter()
            .find_map(|command| match command {
                PaintCommand::DrawText(text) if text.text.contains("um_arquivo") => {
                    Some(text.origin.x)
                }
                _ => None,
            })
            .unwrap_or_default()
    };
    let before = origin_of(&mut shell);
    assert!(before > 0.0, "o Explorer precisa desenhar o arquivo");

    shell.explorer.scroll_x = 20.0;
    assert!(
        (before - origin_of(&mut shell) - 20.0).abs() < 0.1,
        "a linha desliza com a rolagem horizontal"
    );
}

fn dir(path: &str, children: Vec<FileNode>) -> FileNode {
    FileNode {
        path: PathBuf::from(path),
        is_directory: true,
        children,
    }
}

fn file(path: &str) -> FileNode {
    FileNode {
        path: PathBuf::from(path),
        is_directory: false,
        children: Vec::new(),
    }
}

fn labels(items: &[NoDoExplorer]) -> Vec<&str> {
    items.iter().map(|item| item.label.as_str()).collect()
}

/// Projeto Maven com a cadeia de pacote que a captura mostra.
fn maven_project() -> FileNode {
    dir(
        "demo",
        vec![dir(
            "demo/src",
            vec![dir(
                "demo/src/main",
                vec![dir(
                    "demo/src/main/java",
                    vec![dir(
                        "demo/src/main/java/br",
                        vec![dir(
                            "demo/src/main/java/br/com",
                            vec![dir(
                                "demo/src/main/java/br/com/exemplo",
                                vec![dir(
                                    "demo/src/main/java/br/com/exemplo/endpoints",
                                    vec![
                                        dir(
                                            "demo/src/main/java/br/com/exemplo/endpoints/controller",
                                            Vec::new(),
                                        ),
                                        file(
                                            "demo/src/main/java/br/com/exemplo/endpoints/App.java",
                                        ),
                                    ],
                                )],
                            )],
                        )],
                    )],
                )],
            )],
        )],
    )
}

/// `br`, `com` e `exemplo` só existem porque o diretório espelha o pacote:
/// viram uma linha só, e `src`, `main` e `java` continuam separados porque
/// não são pacotes.
#[test]
fn explorer_joins_single_child_java_packages_into_one_row() {
    let items = explorer_nomes(&maven_project(), &java_source_roots(), &HashMap::new());
    let src = &items[0];
    assert_eq!(labels(&items), vec!["src"]);
    let main = &src.children[0];
    assert_eq!(labels(&src.children), vec!["main"]);
    let java = &main.children[0];
    assert_eq!(labels(&main.children), vec!["java"]);
    assert_eq!(labels(&java.children), vec!["br.com.exemplo.endpoints"]);
    assert_eq!(
        labels(&java.children[0].children),
        vec!["controller", "App.java"]
    );
}

/// Cada nó recebe o crachá da espécie dele, e nenhum recebe o do vizinho.
///
/// Pacote sai do caminho; classe, interface e enumeração saem do índice, pela
/// mesma identidade que a árvore usa. Sem resposta do índice não há crachá — é
/// a verdade, e um chutado seria pior.
#[test]
fn cada_no_recebe_o_cracha_da_especie() {
    use ide_domain::SymbolKind;

    let pacote = Path::new("demo/src/main/java/br/com/exemplo/endpoints");
    let arquivo = |nome: &str| pacote.join("controller").join(nome);
    let kinds = HashMap::from([
        (explorer_id(&arquivo("Pedido.java")), SymbolKind::Class),
        (explorer_id(&arquivo("Repositorio.java")), SymbolKind::Interface),
        (explorer_id(&arquivo("Situacao.java")), SymbolKind::Enum),
        (explorer_id(&arquivo("Dto.java")), SymbolKind::Record),
    ]);

    let tree = dir(
        "demo/src/main/java",
        vec![dir(
            "demo/src/main/java/br",
            vec![dir(
                "demo/src/main/java/br/com",
                vec![dir(
                    "demo/src/main/java/br/com/exemplo",
                    vec![dir(
                        pacote.to_string_lossy().as_ref(),
                        vec![
                            file(arquivo("Pedido.java").to_string_lossy().as_ref()),
                            file(arquivo("Repositorio.java").to_string_lossy().as_ref()),
                            file(arquivo("Situacao.java").to_string_lossy().as_ref()),
                            file(arquivo("Dto.java").to_string_lossy().as_ref()),
                            file(arquivo("leiame.md").to_string_lossy().as_ref()),
                        ],
                    )],
                )],
            )],
        )],
    );

    let nos = explorer_nomes(&tree, &java_source_roots(), &kinds);
    // A cadeia inteira comprime numa linha só, e ela é o pacote.
    let [raiz] = nos.as_slice() else {
        panic!("a cadeia de pacotes precisa virar uma linha só: {:?}", labels(&nos));
    };
    assert_eq!(raiz.especie, Especie::Pacote, "pasta sob a raiz de fontes é pacote");

    let por_nome = |nome: &str| {
        raiz.children
            .iter()
            .find(|no| no.label == nome)
            .map(|no| no.especie)
    };
    assert_eq!(por_nome("Pedido.java"), Some(Especie::Classe));
    assert_eq!(por_nome("Repositorio.java"), Some(Especie::Interface));
    assert_eq!(por_nome("Situacao.java"), Some(Especie::Enumeracao));
    assert_eq!(
        por_nome("Dto.java"),
        Some(Especie::Classe),
        "um `record` é uma classe declarada de outro jeito, e ganha o mesmo C"
    );
    assert_eq!(
        por_nome("leiame.md"),
        Some(Especie::Nenhuma),
        "arquivo que não declara tipo não ganha crachá"
    );
}

/// Sem resposta do índice, nenhum arquivo ganha crachá.
#[test]
fn sem_indice_nao_ha_cracha_chutado() {
    let nos = explorer_nomes(&maven_project(), &java_source_roots(), &HashMap::new());
    fn nenhum_tipo(nos: &[NoDoExplorer]) {
        for no in nos {
            assert_ne!(no.especie, Especie::Classe, "{}", no.label);
            assert_ne!(no.especie, Especie::Interface, "{}", no.label);
            assert_ne!(no.especie, Especie::Enumeracao, "{}", no.label);
            nenhum_tipo(&no.children);
        }
    }
    nenhum_tipo(&nos);
}

/// O nó comprimido responde pelo diretório final da cadeia — é assim que o
/// clique continua resolvendo para um caminho que existe.
#[test]
fn a_joined_package_keeps_the_identity_of_the_deepest_directory() {
    let items = explorer_nomes(&maven_project(), &java_source_roots(), &HashMap::new());
    let package = &items[0].children[0].children[0].children[0];
    assert_eq!(
        package.id,
        explorer_id(Path::new("demo/src/main/java/br/com/exemplo/endpoints"))
    );
}

/// Um arquivo ao lado do subdiretório interrompe a cadeia: `br` passa a ter
/// conteúdo próprio e merece a linha dele.
#[test]
fn a_file_beside_the_subdirectory_stops_the_chain() {
    let tree = dir(
        "demo/src/main/java",
        vec![dir(
            "demo/src/main/java/br",
            vec![
                dir("demo/src/main/java/br/com", Vec::new()),
                file("demo/src/main/java/br/leiame.md"),
            ],
        )],
    );
    assert_eq!(
        labels(&explorer_nomes(&tree, &java_source_roots(), &HashMap::new())),
        vec!["br"]
    );
}

/// Fora de uma raiz de fontes não há pacote, e juntar nomes com ponto diria
/// algo que não é verdade sobre aquelas pastas.
#[test]
fn directories_outside_a_source_root_are_left_alone() {
    let tree = dir(
        "demo",
        vec![dir(
            "demo/docs",
            vec![dir("demo/docs/adr", vec![file("demo/docs/adr/0001.md")])],
        )],
    );
    let items = explorer_nomes(&tree, &java_source_roots(), &HashMap::new());
    assert_eq!(labels(&items), vec!["docs"]);
    assert_eq!(labels(&items[0].children), vec!["adr"]);
}

#[test]
fn explorer_click_toggles_directory() {
    let mut shell = test_shell();
    let directory = PathBuf::from("workspace").join("src");
    assert!(!shell.is_expanded(&directory));
    shell.pointer_down(
        Point::new(80.0, EXPLORER_TOP + 2.0),
        Size::new(1280.0, 800.0),
    );
    assert!(shell.is_expanded(&directory));
}

fn shell_with_java_file() -> (IdeShell, PathBuf) {
    let mut shell = test_shell();
    let path = PathBuf::from("Main.java");
    shell.editor_area.session.open_memory(
        "Main.java",
        "class Main {\n  void run() {\n    int total = 1;\n  }\n}",
    );
    (shell, path)
}

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

fn entry_labels(entries: &[MenuEntry]) -> Vec<String> {
    entries
        .iter()
        .map(|entry| match entry {
            MenuEntry::Item(item) => item.label.clone(),
            MenuEntry::Submenu { label, .. } => label.clone(),
            MenuEntry::Separator => "—".to_owned(),
        })
        .collect()
}

/// Dentro da raiz de fontes o que se cria é pacote e tipo. A própria pasta
/// `java` conta como dentro: é a raiz onde o primeiro pacote nasce.
#[test]
fn inside_the_java_source_root_the_menu_offers_packages_and_types() {
    for target in [
        "demo/src/main/java",
        "demo/src/main/java/br",
        "demo/src/test/java/br/com/exemplo",
    ] {
        assert_eq!(
            entry_labels(&explorer_menu_entries(
                Path::new(target),
                &java_source_roots(),
                &java_catalog().new_item_templates,
                false,
            )),
            vec!["Novo pacote", "—", "Nova classe", "Nova interface"],
            "alvo {target}"
        );
    }
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

/// Abrir uma pasta não pode esvaziar a irmã que já estava aberta.
///
/// Ler um caminho traz todos os níveis até ele, cada um com os filhos
/// imediatos — e filho imediato vem sem netos. Trocar a lista inteira pela
/// recém-lida apagava o que já estava carregado nos irmãos: clicar em
/// `resources` esvaziava `java`, e os pacotes sumiam da tela.
#[test]
fn abrir_uma_pasta_nao_esvazia_a_irma_ja_carregada() {
    let tree = dir(
        "demo",
        vec![dir(
            "demo/src/main",
            vec![
                dir(
                    "demo/src/main/java",
                    vec![dir("demo/src/main/java/br", vec![file("demo/src/main/java/br/App.java")])],
                ),
                dir("demo/src/main/resources", Vec::new()),
            ],
        )],
    );
    let mut shell = IdeShell::from_tree(tree);

    // A leitura de `resources` traz os níveis acima dele, e `java` volta raso.
    shell.insert_path_children(vec![
        (
            PathBuf::from("demo/src/main"),
            vec![
                dir("demo/src/main/java", Vec::new()),
                dir("demo/src/main/resources", Vec::new()),
            ],
        ),
        (
            PathBuf::from("demo/src/main/resources"),
            vec![file("demo/src/main/resources/application.properties")],
        ),
    ]);

    let java = shell
        .explorer
        .workspace_tree()
        .children
        .iter()
        .find(|no| no.path == Path::new("demo/src/main"))
        .and_then(|main| {
            main.children
                .iter()
                .find(|no| no.path == Path::new("demo/src/main/java"))
        });
    let Some(java) = java else {
        panic!("`java` precisa continuar na árvore");
    };
    assert!(
        !java.children.is_empty(),
        "os pacotes já lidos de `java` não podem sumir porque a irmã foi aberta"
    );
}

/// Clicar no triângulo dobra **a pasta clicada**, e não a de antes.
///
/// A árvore resolve qual nó recebeu o clique, e o shell age pela resposta dela.
/// Quando o triângulo abria o nó sem marcá-lo como selecionado, a resposta era o
/// nó **anterior**: o Explorer expandia a pasta errada, e depois de algumas
/// navegações havia pastas que simplesmente paravam de abrir.
#[test]
fn o_triangulo_dobra_a_pasta_clicada_e_nao_a_anterior() {
    let tree = dir(
        "demo",
        vec![
            dir("demo/um", vec![file("demo/um/A.java")]),
            dir("demo/dois", vec![file("demo/dois/B.java")]),
        ],
    );
    let mut shell = IdeShell::from_tree(tree);
    let size = Size::new(1280.0, 800.0);
    // Um quadro para a árvore existir posicionada, como na janela.
    shell.paint(size);

    // O triângulo fica na coluna do recuo, à esquerda do crachá.
    let triangulo = |linha: usize| {
        Point::new(
            ACTIVITY_WIDTH + 12.0,
            EXPLORER_TOP + linha as f32 * EXPLORER_ROW_HEIGHT + 2.0,
        )
    };

    shell.pointer_down(triangulo(0), size);
    assert!(
        shell.explorer.expanded.contains(&PathBuf::from("demo/um")),
        "a primeira pasta precisa abrir"
    );

    // Com `demo/um` aberta, a linha 2 é `demo/dois`.
    shell.paint(size);
    shell.pointer_down(triangulo(2), size);
    assert!(
        shell.explorer.expanded.contains(&PathBuf::from("demo/dois")),
        "a segunda pasta precisa abrir, e não a primeira fechar"
    );
    assert!(
        shell.explorer.expanded.contains(&PathBuf::from("demo/um")),
        "clicar na segunda não pode mexer na primeira"
    );
}

/// O clique secundário abre o menu sobre a linha apontada, e a escolha
/// relata o diretório onde a ação aconteceria.
#[test]
fn the_secondary_click_on_the_explorer_opens_the_menu_for_that_row() {
    let mut shell = IdeShell::from_tree(maven_project());
    let size = Size::new(1280.0, 800.0);
    let row = Point::new(80.0, EXPLORER_TOP + 2.0);
    shell.secondary_pointer_down(row, size);
    assert!(shell.context_menu_open());
    assert_eq!(
        shell.explorer.context_menu_target,
        Some(PathBuf::from("demo/src"))
    );
    assert_eq!(
        entry_labels(shell.explorer.context_menu.entries()),
        vec!["Nova pasta"]
    );
}

/// Clicando em um arquivo, o alvo é a pasta dele: criar dentro de um
/// arquivo não quer dizer nada.
#[test]
fn a_file_hands_the_menu_over_to_its_directory() {
    let tree = dir(
        "demo",
        vec![dir(
            "demo/src/main/java",
            vec![file("demo/src/main/java/App.java")],
        )],
    );
    let mut shell = IdeShell::from_tree(tree);
    shell.set_ui_catalog(java_catalog());
    let size = Size::new(1280.0, 800.0);
    shell
        .explorer
        .expanded
        .insert(PathBuf::from("demo/src/main/java"));
    shell.sync_explorer_tree();
    shell.secondary_pointer_down(
        Point::new(80.0, EXPLORER_TOP + EXPLORER_ROW_HEIGHT + 2.0),
        size,
    );
    assert_eq!(
        shell.explorer.context_menu_target,
        Some(PathBuf::from("demo/src/main/java"))
    );
    // A criação é na pasta do arquivo; renomear é do arquivo clicado, e por
    // isso as duas coisas convivem no mesmo menu.
    assert_eq!(
        entry_labels(shell.explorer.context_menu.entries()),
        vec![
            "Novo pacote",
            "—",
            "Nova classe",
            "Nova interface",
            "—",
            "Renomear"
        ]
    );
    assert_eq!(
        shell.explorer.context_menu_file,
        Some(PathBuf::from("demo/src/main/java/App.java")),
        "renomear precisa do arquivo, e não da pasta"
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
fn toggling_from_the_keyboard_uses_the_cursor_line() {
    let (mut shell, path) = shell_with_java_file();
    shell.editor_area.pane.set_cursor(20); // segunda linha
    shell.toggle_breakpoint_at_cursor();
    assert_eq!(shell.breakpoints_for(&path), vec![1]);
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
fn java_tool_output_is_appended_to_terminal() {
    let mut shell = test_shell();
    shell.append_tool_output("compile ok\nruntime failure", true);
    let lines = shell.active_terminal_lines().collect::<Vec<_>>();
    assert!(lines.contains(&"compile ok"));
    assert!(lines.contains(&"runtime failure"));
}

#[test]
fn explorer_horizontal_scrollbar_keeps_long_names_inside_sidebar() {
    let mut shell = IdeShell::from_tree(FileNode {
        path: PathBuf::from("workspace"),
        is_directory: true,
        children: vec![FileNode {
            path: PathBuf::from("workspace")
                .join("a_very_long_project_filename_that_must_not_overflow_into_the_editor.rs"),
            is_directory: false,
            children: Vec::new(),
        }],
    });
    let size = Size::new(1280.0, 800.0);
    let track = shell.explorer_horizontal_scrollbar_rect(size);
    // A largura do conteúdo é **medida** ao posicionar as linhas, e não estimada
    // por contagem de caracteres: as linhas são componentes, e quem sabe quanto
    // um rótulo ocupa é a fonte que vai desenhá-lo. Um quadro basta, e é o que a
    // janela faz antes de qualquer gesto chegar.
    shell.paint(size);
    // O nome não cabe na largura visível, então há o que rolar.
    let (_, content, viewport, _) = shell.scrollbar_range(ScrollTarget::ExplorerHorizontal, size);
    assert!(
        content > viewport,
        "o nome longo precisa passar da largura visível: {content} contra {viewport}"
    );
    shell.pointer_down(
        Point::new(
            track.origin.x + track.size.width - 1.0,
            track.origin.y + 5.0,
        ),
        size,
    );
    assert!(shell.explorer.scroll_x > 0.0);
    let rendered = shell.paint(size);
    assert!(rendered.iter().any(|command| {
        matches!(
            command,
            PaintCommand::PushClip(rect)
                if rect.origin.x == ACTIVITY_WIDTH
                    && rect.size.width == shell.sidebar_width(size)
        )
    }));
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
fn active_file_expands_selects_and_scrolls_the_explorer() {
    static NEXT_PROJECT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let sequence = NEXT_PROJECT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!(
        "er-ide-explorer-active-{}-{sequence}",
        std::process::id()
    ));
    let package = root
        .join("src")
        .join("main")
        .join("java")
        .join("br")
        .join("com")
        .join("exemplo")
        .join("controller");
    assert!(std::fs::create_dir_all(&package).is_ok());
    for index in 0..12 {
        assert!(std::fs::write(root.join(format!("Anterior{index:02}.txt")), "x").is_ok());
    }
    let first = package.join("PrimeiroController.java");
    let second = package.join("SegundoController.java");
    assert!(std::fs::write(&first, "class PrimeiroController {}").is_ok());
    assert!(std::fs::write(&second, "class SegundoController {}").is_ok());

    let mut shell = match IdeShell::open(&root) {
        Ok(shell) => shell,
        Err(error) => panic!("projeto não abriu: {error}"),
    };
    assert!(shell.open_file(&first).is_ok());
    assert!(shell.open_file(&second).is_ok());
    assert_eq!(
        shell.explorer.tree.selected(),
        Some(explorer_id(&second)),
        "a última aba restaurada deve nascer selecionada no Explorer"
    );
    for ancestor in second.ancestors().skip(1).take_while(|path| *path != root) {
        assert!(
            shell.explorer.expanded.contains(ancestor),
            "{} deveria estar expandido",
            ancestor.display()
        );
    }
    assert!(
        shell.explorer.scroll_line > 0,
        "o arquivo ativo precisa ser revelado mesmo abaixo do primeiro viewport"
    );

    let editor_x = ACTIVITY_WIDTH + SIDEBAR_WIDTH;
    shell.pointer_down(
        Point::new(editor_x + 10.0, TITLE_HEIGHT + 10.0),
        Size::new(1280.0, 800.0),
    );
    assert_eq!(shell.active_document_path(), Some(first.clone()));
    assert_eq!(
        shell.explorer.tree.selected(),
        Some(explorer_id(&first)),
        "trocar de aba também precisa trocar a seleção da árvore"
    );
    let _ = std::fs::remove_dir_all(root);
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
fn explorer_vertical_scrollbar_and_wheel_reach_later_entries() {
    let children = (0..80)
        .map(|index| FileNode {
            path: PathBuf::from("workspace").join(format!("file_{index:03}.rs")),
            is_directory: false,
            children: Vec::new(),
        })
        .collect();
    let mut shell = IdeShell::from_tree(FileNode {
        path: PathBuf::from("workspace"),
        is_directory: true,
        children,
    });
    let size = Size::new(1280.0, 800.0);
    let track = shell.explorer_vertical_scrollbar_rect(size);
    shell.scroll(
        Point::new(ACTIVITY_WIDTH + 40.0, EXPLORER_TOP + 40.0),
        5.0,
        size,
    );
    assert_eq!(shell.explorer.scroll_line, 5);
    shell.pointer_down(
        Point::new(
            track.origin.x + 5.0,
            track.origin.y + track.size.height - 1.0,
        ),
        size,
    );
    assert!(shell.explorer.scroll_line > 5);
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

/// A fila de leituras termina, e é por isso que a janela chega a desenhar.
///
/// Este é o defeito que deixou a IDE branca e sem responder: uma pasta **vazia
/// no disco** tem a mesma forma de uma pasta não lida, então respondê-la não a
/// tirava da lista de pendentes. Cada resposta fazia a reconciliação da seleção
/// pedir todas as outras de novo — quarenta pastas expandidas viravam milhares
/// de leituras por quadro, e o laço de eventos nunca voltava.
///
/// O teste acima não pegava isso porque tem **uma** pasta e **uma** rodada; a
/// cascata precisa de várias pastas e do laço da aplicação.
#[test]
fn the_queue_of_directory_reads_settles() {
    let root = PathBuf::from("workspace");
    let vazias = ["a", "b", "c", "d", "e"];
    let mut shell = IdeShell::from_tree(FileNode {
        path: root.clone(),
        is_directory: true,
        children: vazias
            .iter()
            .map(|nome| FileNode {
                path: root.join(nome),
                is_directory: true,
                children: Vec::new(),
            })
            .collect(),
    });

    // Um documento ativo ausente da árvore: é ele que dispara a reconciliação a
    // cada carga, e era a reconciliação que realimentava a cascata.
    shell
        .editor_area
        .session
        .open_memory(root.join("a/Fantasma.java"), "class Fantasma {}");
    for nome in vazias {
        shell.reveal_in_explorer(&root.join(nome));
    }

    // O laço da aplicação: tira os pedidos, lê o "disco" — onde toda pasta está
    // vazia — e devolve. Sem o conserto isto não converge.
    let mut rodadas = 0;
    let mut lidas = 0;
    loop {
        let pedidos = shell
            .drain_application_commands()
            .into_iter()
            .filter_map(|command| match command {
                ApplicationCommand::LoadDirectory(path) => Some(path),
                _ => None,
            })
            .collect::<Vec<_>>();
        if pedidos.is_empty() {
            break;
        }
        lidas += pedidos.len();
        rodadas += 1;
        assert!(
            rodadas < 20 && lidas < 200,
            "a fila de leituras não termina: {rodadas} rodadas, {lidas} leituras"
        );
        for pasta in pedidos {
            shell.insert_path_children(vec![(pasta, Vec::new())]);
        }
    }

    assert!(
        lidas <= vazias.len(),
        "cada pasta é lida uma vez, e não {lidas} vezes"
    );
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
    let source = include_str!("../ide_shell.rs");
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

/// Razão de contraste WCAG entre duas cores opacas.
fn contrast_ratio(first: Color, second: Color) -> f32 {
    fn luminance(color: Color) -> f32 {
        fn channel(value: f32) -> f32 {
            if value <= 0.03928 {
                value / 12.92
            } else {
                ((value + 0.055) / 1.055).powf(2.4)
            }
        }
        0.2126 * channel(color.red) + 0.7152 * channel(color.green) + 0.0722 * channel(color.blue)
    }
    let first = luminance(first);
    let second = luminance(second);
    (first.max(second) + 0.05) / (first.min(second) + 0.05)
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

/// Projeto Maven com um pacote já criado, para o menu agir sobre ele.
fn shell_with_package() -> IdeShell {
    let mut shell = IdeShell::from_tree(dir(
        "demo",
        vec![dir(
            "demo/src/main/java",
            vec![dir(
                "demo/src/main/java/br",
                vec![dir("demo/src/main/java/br/com", Vec::new())],
            )],
        )],
    ));
    shell.set_ui_catalog(java_catalog());
    shell
}

/// O menu abre a janela com o pacote do alvo já preenchido.
///
/// Quem clicou com o botão direito sobre um pacote não deveria ter que
/// digitar de novo onde está.
#[test]
fn the_new_item_dialog_opens_with_the_clicked_package() {
    let mut shell = shell_with_package();
    shell.explorer.context_menu_target = Some(PathBuf::from("demo/src/main/java/br/com"));
    shell.run_explorer_command("explorer.new.java.class");
    assert!(shell.new_item_dialog_open());
    assert_eq!(NewItemSurface::values(&shell.host), ("br.com", ""));
}

/// A mesma janela serve as três ações, mudando só o título e a legenda.
#[test]
fn the_three_menu_actions_share_one_window() {
    let mut shell = shell_with_package();
    shell.explorer.context_menu_target = Some(PathBuf::from("demo/src/main/java/br/com"));
    for (command, title) in [
        ("explorer.new.java.package", "Novo pacote"),
        ("explorer.new.java.class", "Nova classe"),
        ("explorer.new.java.interface", "Nova interface"),
    ] {
        shell.run_explorer_command(command);
        assert_eq!(shell.new_item.title(), Some(title));
        assert_eq!(NewItemSurface::values(&shell.host).0, "br.com");
    }
}

/// Enter com só o pacote pede o pacote; o nome fica vazio.
#[test]
fn enter_with_only_the_package_asks_for_the_package() {
    let mut shell = shell_with_package();
    shell.explorer.context_menu_target = Some(PathBuf::from("demo/src/main/java/br/com"));
    shell.run_explorer_command("explorer.new.java.package");
    // O foco começa no pacote, com o cursor no fim do que veio preenchido.
    shell.text_input(".exemplo");
    shell.key_down("Enter");
    let request = shell.take_new_item_request();
    assert_eq!(
        request,
        Some(NewItemRequest {
            template_id: NewItemTemplateId::new("java.package"),
            package: "br.com.exemplo".to_owned(),
            name: String::new(),
            source_root: PathBuf::from("demo/src/main/java"),
        })
    );
}

/// Prepara um shell com arquivo aberto e foco no editor.
fn shell_editing(text: &str) -> IdeShell {
    let mut shell = test_shell();
    shell.editor_area.session.open_memory("Pedido.java", text);
    shell.context.focus = ShellFocus::Editor;
    shell.editor_area.pane.set_cursor(0);
    shell
}

fn accessor_plan_para_teste() -> AccessorPlan {
    let candidato = |campo: &str, fonte: Option<&str>| AccessorCandidate {
        field: campo.to_owned(),
        source: fonte.map(str::to_owned),
    };
    AccessorPlan {
        candidates: vec![
            candidato(
                "id",
                Some("\n    public Long getId() {\n        return id;\n    }\n"),
            ),
            // Já tem getter: não deve nem aparecer na janela.
            candidato("nome", None),
            candidato(
                "ativo",
                Some("\n    public boolean isAtivo() {\n        return ativo;\n    }\n"),
            ),
        ],
        insert_at: DomainTextPosition { line: 2, column: 0 },
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

/// Coluna do editor em coordenadas de tela.
fn editor_column(shell: &IdeShell, size: Size, index: usize) -> Point {
    let geometry = shell.geometry();
    let editor_x = ACTIVITY_WIDTH + shell.sidebar_width(size);
    Point::new(
        editor_x + EDITOR_GUTTER + index as f32 * EDITOR_CHAR_WIDTH,
        geometry.content_top + 20.0,
    )
}

/// Área de transferência de teste, sem depender do sistema.
#[derive(Default)]
struct FakeClipboard {
    text: std::sync::Mutex<Option<String>>,
}

impl ClipboardService for FakeClipboard {
    fn get_text(&self) -> Result<Option<String>, ui_window_api::ClipboardError> {
        Ok(self.text.lock().ok().and_then(|text| text.clone()))
    }

    fn set_text(&self, value: &str) -> Result<(), ui_window_api::ClipboardError> {
        if let Ok(mut text) = self.text.lock() {
            *text = Some(value.to_owned());
        }
        Ok(())
    }
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
    let entries = shell.explorer.context_menu.entries();
    assert_eq!(entry_labels(entries), vec!["Copiar", "Colar"]);
    let copy_enabled = |entries: &[MenuEntry]| match &entries[0] {
        MenuEntry::Item(item) => item.enabled,
        MenuEntry::Separator | MenuEntry::Submenu { .. } => false,
    };
    assert!(!copy_enabled(entries), "sem seleção não há o que copiar");

    shell.editor_area.pane.set_selection(Some((0, 5)));
    shell.secondary_pointer_down(editor_column(&shell, size, 2), size);
    assert!(copy_enabled(shell.explorer.context_menu.entries()));
}

/// O objeto inspecionado: um `Pedido` com um campo simples e outro objeto.
fn inspection_value() -> DebugVariableView {
    DebugVariableView {
        name: "pedido".to_owned(),
        value: "Pedido@1a2b".to_owned(),
        type_name: Some("br.com.exemplo.Pedido".to_owned()),
        expandable: true,
    }
}

fn inspection_fields() -> Vec<DebugVariableView> {
    vec![
        DebugVariableView {
            name: "total".to_owned(),
            value: "42".to_owned(),
            type_name: Some("int".to_owned()),
            expandable: false,
        },
        DebugVariableView {
            name: "cliente".to_owned(),
            value: "Cliente@3c4d".to_owned(),
            type_name: Some("br.com.exemplo.Cliente".to_owned()),
            expandable: true,
        },
    ]
}

fn inspection_void() -> DebugVariableView {
    DebugVariableView {
        name: "retorno".to_owned(),
        value: "void".to_owned(),
        type_name: None,
        expandable: false,
    }
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

fn type_hit(name: &str, kind: &str, path: &std::path::Path, line: u32) -> TypeSearchHit {
    TypeSearchHit {
        name: name.to_owned(),
        kind: kind.to_owned(),
        location: Location {
            path: path.into(),
            range: ide_domain::TextRange {
                start: DomainTextPosition { line, column: 0 },
                end: DomainTextPosition { line, column: 0 },
            },
        },
    }
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

fn content_hit(path: &std::path::Path, line: u32, column: u32) -> ContentSearchHit {
    ContentSearchHit {
        preview: "String mensagem = \"conteúdo procurado\";".to_owned(),
        location: Location {
            path: path.into(),
            range: ide_domain::TextRange {
                start: DomainTextPosition { line, column },
                end: DomainTextPosition { line, column },
            },
        },
    }
}

#[test]
fn type_search_shows_the_path_after_the_last_java_directory() {
    let absolute = PathBuf::from(r"C:\workspace\java\modulo\src\main\java")
        .join("br")
        .join("com")
        .join("exemplo")
        .join("Pedido.java");
    let hit = type_hit("Pedido", "classe", &absolute, 0);
    let expected = PathBuf::from("br")
        .join("com")
        .join("exemplo")
        .join("Pedido.java");

    let roots = java_source_roots();
    assert_eq!(search_display_path(&absolute, &roots), expected);
    assert!(
        !hit.label(&roots).contains("workspace"),
        "o resultado não pode mostrar o caminho absoluto: {}",
        hit.label(&roots)
    );
}

/// Diretório com dois tipos, para a busca ter o que abrir de verdade.
fn type_search_workspace() -> std::path::PathBuf {
    static NEXT_WORKSPACE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let sequence = NEXT_WORKSPACE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!("er-ide-busca-{}-{sequence}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    assert!(std::fs::create_dir_all(&root).is_ok());
    assert!(std::fs::write(root.join("Pedido.java"), "class Pedido {}\n").is_ok());
    assert!(
        std::fs::write(
            root.join("PedidoRepository.java"),
            "interface PedidoRepository {}\n"
        )
        .is_ok()
    );
    root
}

/// A busca por nome pede, mostra e leva ao arquivo escolhido.
#[test]
fn the_type_search_asks_lists_and_opens_what_was_chosen() {
    let root = type_search_workspace();
    let mut shell = shell_editing("class Uso {}");
    let size = Size::new(1280.0, 800.0);
    shell.open_type_search();
    assert!(shell.type_search_open());
    assert_eq!(
        shell.take_type_search_request(),
        Some(String::new()),
        "a janela nasce pedindo tudo, sem esperar a primeira letra"
    );

    // Digitar refina, e cada tecla vira um pedido.
    shell.text_input("Ped");
    assert_eq!(shell.take_type_search_request(), Some("Ped".to_owned()));
    shell.key_down("Backspace");
    assert_eq!(shell.take_type_search_request(), Some("Pe".to_owned()));

    // Os resultados aparecem na janela, com nome, tipo e caminho.
    let repositorio = root.join("PedidoRepository.java");
    shell.set_type_search_results(vec![
        type_hit("Pedido", "classe", &root.join("Pedido.java"), 0),
        type_hit("PedidoRepository", "interface", &repositorio, 0),
    ]);
    let texts = painted_texts(&mut shell, size);
    assert!(
        texts
            .iter()
            .any(|text| text.contains("Pedido (classe)") && text.contains("Pedido.java")),
        "a lista precisa mostrar o que foi encontrado e onde: {texts:?}"
    );

    // As setas andam na lista e `Enter` pede à aplicação que abra o
    // escolhido. A UI não lê o arquivo.
    shell.key_down("ArrowDown");
    shell.key_down("Enter");
    assert!(!shell.type_search_open(), "escolher fecha a janela");
    assert_eq!(
        shell.drain_application_commands(),
        vec![ApplicationCommand::OpenDocument(OpenDocumentRequest::new(
            repositorio
        ))],
        "o segundo item é o que devia abrir"
    );
    let _ = std::fs::remove_dir_all(root);
}

/// A busca textual usa o mesmo modal, mas só pede depois de haver texto e
/// abre a ocorrência na linha e coluna devolvidas pelo workspace.
#[test]
fn content_search_reuses_the_modal_and_opens_the_occurrence() {
    let root = type_search_workspace();
    let source = root.join("Pedido.java");
    assert!(
        std::fs::write(
            &source,
            "class Pedido {\n    String mensagem = \"conteúdo procurado\";\n}\n"
        )
        .is_ok()
    );
    let mut shell = shell_editing("class Uso {}");
    let size = Size::new(1280.0, 800.0);

    shell.open_content_search();
    assert!(shell.type_search_open());
    assert_eq!(
        shell.take_content_search_request(),
        None,
        "a consulta vazia não deve varrer todos os arquivos"
    );
    shell.text_input("conteúdo");
    assert_eq!(
        shell.take_content_search_request(),
        Some("conteúdo".to_owned())
    );
    assert_eq!(
        shell.take_type_search_request(),
        None,
        "cada modo possui uma porta própria"
    );

    shell.set_content_search_results(vec![content_hit(&source, 1, 23)]);
    let texts = painted_texts(&mut shell, size);
    assert!(
        texts
            .iter()
            .any(|text| { text.contains("Pedido.java:2") && text.contains("conteúdo procurado") }),
        "a lista precisa mostrar arquivo, linha e trecho: {texts:?}"
    );
    shell.key_down("Enter");

    assert!(!shell.type_search_open());
    assert_eq!(
        shell.drain_application_commands(),
        vec![ApplicationCommand::OpenDocument(
            OpenDocumentRequest::new(source).at(1, 23)
        )]
    );
    let _ = std::fs::remove_dir_all(root);
}

/// `Esc` dispensa a busca sem abrir nada.
#[test]
fn escape_closes_the_type_search() {
    let root = type_search_workspace();
    let mut shell = shell_editing("class Uso {}");
    let antes = shell.active_document_path();
    shell.open_type_search();
    let _ = shell.take_type_search_request();
    shell.set_type_search_results(vec![type_hit(
        "Pedido",
        "classe",
        &root.join("Pedido.java"),
        0,
    )]);
    shell.escape();
    assert!(!shell.type_search_open());
    assert_eq!(
        shell.active_document_path(),
        antes,
        "desistir não troca a aba aberta"
    );
    let _ = std::fs::remove_dir_all(root);
}

/// A lista revela a seleção das setas e a roda nunca alcança o editor atrás.
#[test]
fn type_search_scroll_stays_inside_the_modal_and_reveals_keyboard_selection() {
    let root = type_search_workspace();
    let mut shell = shell_editing(
        &(0..80)
            .map(|line| format!("linha {line}\n"))
            .collect::<String>(),
    );
    let size = Size::new(1280.0, 800.0);
    shell.open_type_search();
    let _ = shell.take_type_search_request();
    shell.set_type_search_results(
        (0..30)
            .map(|index| {
                type_hit(
                    &format!("Tipo{index:02}"),
                    "classe",
                    &root.join("Pedido.java"),
                    0,
                )
            })
            .collect(),
    );

    for _ in 0..(type_search::VISIBLE_ROWS + 3) {
        shell.key_down("ArrowDown");
    }
    assert_eq!(shell.search.scroll_state().0, type_search::VISIBLE_ROWS + 3);
    assert!(
        shell.search.scroll_state().1 > 0,
        "a seleção que passou do viewport precisa trazer a lista junto"
    );
    let texts = painted_texts(&mut shell, size);
    assert!(
        texts.iter().any(|text| text.contains("Tipo15")),
        "o item escolhido pelas setas precisa continuar visível: {texts:?}"
    );

    let editor_before = shell.editor_area.pane.scroll_line();
    let list = TypeSearchSurface::list_area(&shell.host);
    shell.scroll(
        Point::new(list.origin.x + 20.0, list.origin.y + 20.0),
        3.0,
        size,
    );
    assert_eq!(
        shell.editor_area.pane.scroll_line(),
        editor_before,
        "a roda no modal não pode rolar o editor atrás"
    );
    assert!(
        shell.search.scroll_state().1 > 0,
        "a própria lista precisa receber a roda"
    );
    let _ = std::fs::remove_dir_all(root);
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

/// As áreas dos ícones do título, depois de um quadro.
///
/// Elas vêm do arranjo, e o arranjo acontece no quadro — pedir antes daria a
/// moldura do tamanho anterior.
fn action_areas(shell: &mut IdeShell, size: Size) -> [Rect; 3] {
    let _ = shell.paint(size);
    shell.action_button_areas()
}

fn inspection_layout(shell: &mut IdeShell, size: Size) -> InspectionGeometry {
    // As áreas vêm do arranjo, e o arranjo acontece no quadro.
    let _ = shell.paint(size);
    inspection::areas(&shell.host)
}

/// O que está marcado no editor da inspeção.
fn inspection_selection(shell: &IdeShell) -> Option<&str> {
    let (editor, source) = shell.inspection.editor_and_source_ref();
    editor.selected_text(source)
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

/// Abrir a busca dispensa a lista de completação que estivesse aberta.
///
/// Sem isto ela ficaria pairando sobre a janela de busca, e ainda se refazendo a
/// cada letra digitada nela.
#[test]
fn opening_the_search_dismisses_the_completion_list() {
    let mut shell = shell_editing("int total = 10;");
    shell.context.focus = ShellFocus::Editor;
    shell.set_completions(vec![CompletionItem {
        label: "getPedido()".to_owned(),
        detail: None,
        kind: ide_domain::CompletionKind::Method,
    }]);
    assert!(
        shell.completion_open(),
        "a lista precisa estar aberta antes"
    );

    shell.open_type_search();
    assert!(
        !shell.completion_open(),
        "a lista do editor não pode sobreviver à janela que tomou o teclado"
    );
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

/// Enquanto procura, a janela mostra o giro no lugar do resultado.
///
/// **Uma lista vazia não distingue "procurando" de "não achei nada".** Antes, a
/// espera era uma linha na barra de estado — no canto oposto ao que se olha
/// quando se abre a janela de busca. Quem procurava via a lista vazia e concluía
/// que não havia resultado, num projeto grande em que a busca ainda estava
/// varrendo.
#[test]
fn while_searching_the_window_spins_where_the_result_will_be() {
    let size = Size::new(1280.0, 800.0);
    let mut shell = shell_with_package();
    shell.open_type_search();

    shell.set_search_progress(Some(0.0));
    let girando = paint_circles(&mut shell, size);
    assert!(
        !girando.is_empty(),
        "com busca em curso, a janela precisa desenhar o giro"
    );

    // O resultado apaga o giro: ele diz "ainda não", e já chegou.
    shell.set_type_search_results(Vec::new());
    let parado = paint_circles(&mut shell, size);
    // A diferença é o anel inteiro, e não um círculo qualquer que a tela já
    // desenhasse: é o que impede este teste de passar por outro motivo.
    assert!(
        girando.len() >= parado.len() + 8,
        "entregue o resultado, o anel inteiro precisa sumir: {} contra {}",
        parado.len(),
        girando.len()
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

fn paint_circles(shell: &mut IdeShell, size: Size) -> Vec<f32> {
    shell
        .paint(size)
        .iter()
        .filter_map(|command| match command {
            PaintCommand::FillCircle(circle) => Some(circle.radius),
            _ => None,
        })
        .collect()
}

fn painted_texts(shell: &mut IdeShell, size: Size) -> Vec<String> {
    shell
        .paint(size)
        .iter()
        .filter_map(|command| match command {
            PaintCommand::DrawText(text) => Some(text.text.clone()),
            _ => None,
        })
        .collect()
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
        entry_labels(shell.explorer.context_menu.entries()),
        vec!["Copiar", "Colar"]
    );

    shell.debug_panel.view.attached = true;
    shell.secondary_pointer_down(editor_column(&shell, size, 6), size);
    assert_eq!(
        entry_labels(shell.explorer.context_menu.entries()),
        vec!["Copiar", "Colar", "—", "Inspecionar"]
    );
}

/// Inspecionar pede a avaliação do trecho marcado.
#[test]
fn inspecting_asks_to_evaluate_the_selected_text() {
    let mut shell = shell_editing("int total = 10;");
    shell.debug_panel.view.attached = true;
    shell.editor_area.pane.set_selection(Some((4, 9)));
    shell.run_explorer_command("debug.inspect");
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
    let entries = shell.explorer.context_menu.entries();
    let enabled = match &entries[3] {
        MenuEntry::Item(item) => item.enabled,
        MenuEntry::Separator | MenuEntry::Submenu { .. } => true,
    };
    assert!(!enabled, "sem seleção não há o que inspecionar");

    shell.run_explorer_command("debug.inspect");
    assert!(shell.take_debug_requests().is_empty());
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

/// Escrever uma string inteira, aspas incluídas, não duplica nada.
#[test]
fn digitar_a_string_inteira_nao_duplica_a_aspa() {
    let mut shell = shell_editing("");
    for caractere in "const a = 'oi';".chars() {
        shell.text_input(&caractere.to_string());
    }
    assert_eq!(shell.active_text(), Some("const a = 'oi';"));
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
    // Abre o menu Arquivo e escolhe a segunda entrada.
    shell.pointer_down(Point::new(100.0, TITLE_HEIGHT / 2.0), size);
    shell.pointer_down(Point::new(100.0, TITLE_HEIGHT + 42.0), size);
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

/// A janela nasce no centro da tela.
///
/// O painel se centraliza na área que recebe no layout; sem esse layout a
/// área era zero e ele aparecia no canto superior esquerdo.
#[test]
fn the_new_item_dialog_opens_centered() {
    let mut shell = shell_with_package();
    let size = Size::new(1280.0, 800.0);
    shell.explorer.context_menu_target = Some(PathBuf::from("demo/src/main/java/br/com"));
    shell.run_explorer_command("explorer.new.java.class");
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

/// Reler o disco repõe os itens da árvore, e não só a expansão.
///
/// O pacote e a classe eram criados e não apareciam: a `TreeView` guarda os
/// itens dela, e a IDE relia o `FileNode` sem repô-los.
#[test]
fn reloading_the_workspace_shows_what_was_created() {
    let root = std::env::temp_dir().join(format!("er-ide-reload-{}", std::process::id()));
    let package = root.join("src/main/java/br/com");
    assert!(std::fs::create_dir_all(&package).is_ok());
    let Ok(mut shell) = IdeShell::open(&root) else {
        panic!("workspace de teste não abriu");
    };
    shell.reveal_in_explorer(&package);
    shell.fulfill_directory_loads();
    let size = Size::new(1280.0, 800.0);
    let shows = |shell: &mut IdeShell, needle: &str| {
        shell.paint(size).iter().any(|command| match command {
            PaintCommand::DrawText(text) => text.text.contains(needle),
            _ => false,
        })
    };
    assert!(!shows(&mut shell, "Pedido"), "a classe ainda não existe");

    assert!(std::fs::write(package.join("Pedido.java"), "class Pedido {}").is_ok());
    assert!(shell.reload_workspace().is_ok());
    shell.reveal_in_explorer(&package);
    shell.fulfill_directory_loads();
    assert!(
        shows(&mut shell, "Pedido.java"),
        "a classe criada precisa aparecer na árvore"
    );
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
    shell.explorer.context_menu_target = Some(PathBuf::from("demo/src/main/java/br/com"));
    shell.run_explorer_command("explorer.new.java.package");
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

/// Com o nome preenchido, o tipo é pedido dentro do pacote informado.
#[test]
fn enter_with_a_name_asks_for_the_type_inside_the_package() {
    let mut shell = shell_with_package();
    shell.explorer.context_menu_target = Some(PathBuf::from("demo/src/main/java/br/com"));
    shell.run_explorer_command("explorer.new.java.interface");
    // Ao criar um tipo o foco já está no nome.
    shell.text_input("Repositorio");
    shell.key_down("Enter");
    assert_eq!(
        shell.take_new_item_request(),
        Some(NewItemRequest {
            template_id: NewItemTemplateId::new("java.interface"),
            package: "br.com".to_owned(),
            name: "Repositorio".to_owned(),
            source_root: PathBuf::from("demo/src/main/java"),
        })
    );
}

/// Tab troca o campo, então o pacote também é editável ao criar um tipo.
#[test]
fn tab_moves_between_the_two_fields() {
    let mut shell = shell_with_package();
    shell.explorer.context_menu_target = Some(PathBuf::from("demo/src/main/java/br/com"));
    shell.run_explorer_command("explorer.new.java.class");
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
    shell.explorer.context_menu_target = Some(PathBuf::from("demo/src/main/java/br/com"));
    shell.run_explorer_command("explorer.new.java.class");
    shell.key_down("Enter");
    assert_eq!(shell.take_new_item_request(), None);
    assert!(shell.new_item_dialog_open());
    assert_eq!(shell.new_item.message(), Some("Informe o nome.".to_owned()));
}

/// Esc fecha sem pedir nada.
#[test]
fn escape_closes_the_new_item_dialog_without_creating() {
    let mut shell = shell_with_package();
    shell.explorer.context_menu_target = Some(PathBuf::from("demo/src/main/java/br/com"));
    shell.run_explorer_command("explorer.new.java.class");
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

fn open_settings_geometry(shell: &mut IdeShell, size: Size) -> SettingsDialogGeometry {
    // A moldura vem do arranjo, e o arranjo acontece no quadro.
    let _ = shell.paint(size);
    shell.settings.geometry(&shell.host)
}

/// Abre o combo e clica na segunda linha.
fn choose_second_jdk(shell: &mut IdeShell, geometry: &SettingsDialogGeometry, size: Size) {
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
}

/// Acessos que só os testes usam para entrar pela porta do shell.
impl IdeShell {
    #[cfg(test)]
    fn open(root: &Path) -> Result<Self, ide_workspace::WorkspaceError> {
        ide_workspace::WorkspaceService::native()
            .scan(root)
            .map(Self::from_tree)
    }

    /// Atende os pedidos de leitura de pasta, como a aplicação faria.
    ///
    /// A varredura é rasa desde a `19`: o shell pede as pastas que precisa, e
    /// quem lê o disco é a aplicação. Nos testes não há laço de aplicação, e sem
    /// isto a árvore ficaria só com o primeiro nível.
    #[cfg(test)]
    fn fulfill_directory_loads(&mut self) {
        let service = ide_workspace::WorkspaceService::native();
        let raiz = self.workspace_root().to_path_buf();
        while let Some(ApplicationCommand::LoadDirectory(path)) = self
            .take_test_command(|command| matches!(command, ApplicationCommand::LoadDirectory(_)))
        {
            self.insert_path_children(service.scan_path(&raiz, &path));
        }
    }

    #[cfg(test)]
    fn open_file(&mut self, path: &Path) -> Result<DocumentId, String> {
        // Abrir um arquivo revela o caminho dele, e revelar pede leitura das
        // pastas: a aplicação atenderia no laço seguinte, e aqui atendemos na
        // hora, antes e depois.
        self.fulfill_directory_loads();
        let resultado = self.open_file_inner(path);
        self.fulfill_directory_loads();
        resultado
    }

    #[cfg(test)]
    fn open_file_inner(&mut self, path: &Path) -> Result<DocumentId, String> {
        if self
            .editor_area
            .session
            .tabs()
            .any(|document| document.path == path)
        {
            return Ok(self.show_document(path, String::new()));
        }
        let text = ide_workspace::WorkspaceService::native()
            .read_document(path)
            .map_err(|error| error.to_string())?;
        Ok(self.show_document(path, text))
    }

    #[cfg(test)]
    fn open_location(
        &mut self,
        path: &Path,
        line: usize,
        column: usize,
    ) -> Result<DocumentId, String> {
        if self
            .editor_area
            .session
            .tabs()
            .any(|document| document.path == path)
        {
            return Ok(self.show_location(path, String::new(), line, column));
        }
        let text = ide_workspace::WorkspaceService::native()
            .read_document(path)
            .map_err(|error| error.to_string())?;
        Ok(self.show_location(path, text, line, column))
    }

    #[cfg(test)]
    fn save_active_document(&mut self) -> bool {
        let Some(document) = self.editor_area.session.active() else {
            return false;
        };
        let id = document.id;
        let path = document.path.clone();
        let text = document.buffer.text().to_owned();
        let revision = document.buffer.revision();
        if ide_workspace::WorkspaceService::native()
            .save_document(&path, &text)
            .is_err()
        {
            return false;
        }
        self.document_saved(id, revision, &path);
        true
    }

    #[cfg(test)]
    fn reload_workspace(&mut self) -> Result<(), ide_workspace::WorkspaceError> {
        let tree = ide_workspace::WorkspaceService::native().scan(&self.explorer.workspace.path)?;
        self.replace_workspace_tree(tree);
        // Trocar a árvore pede de volta as pastas abertas; na aplicação quem
        // atende é o laço, e aqui somos nós.
        self.fulfill_directory_loads();
        Ok(())
    }

    #[cfg(test)]
    fn take_test_command(
        &mut self,
        predicate: impl Fn(&ApplicationCommand) -> bool,
    ) -> Option<ApplicationCommand> {
        let index = self.commands.iter().position(predicate)?;
        Some(self.commands.remove(index))
    }

    #[cfg(test)]
    fn take_settings_jdk_result(&mut self) -> Option<usize> {
        match self.take_test_command(|command| {
            matches!(
                command,
                ApplicationCommand::SelectTool {
                    role: ToolRole::Primary,
                    ..
                }
            )
        }) {
            Some(ApplicationCommand::SelectTool { index, .. }) => Some(index),
            _ => None,
        }
    }

    #[cfg(test)]
    fn take_browse_jdk_request(&mut self) -> bool {
        self.take_test_command(|command| {
            matches!(
                command,
                ApplicationCommand::BrowseTool {
                    role: ToolRole::Primary,
                    ..
                }
            )
        })
        .is_some()
    }

    #[cfg(test)]
    fn take_navigation_request(&mut self) -> Option<NavigationRequest> {
        match self.take_test_command(|command| matches!(command, ApplicationCommand::Navigate(_))) {
            Some(ApplicationCommand::Navigate(request)) => Some(request),
            _ => None,
        }
    }

    #[cfg(test)]
    fn take_open_project_request(&mut self) -> bool {
        self.take_test_command(|command| matches!(command, ApplicationCommand::OpenProject))
            .is_some()
    }

    #[cfg(test)]
    fn take_breakpoints_dirty(&mut self) -> Option<PathBuf> {
        match self.take_test_command(|command| {
            matches!(command, ApplicationCommand::BreakpointsChanged(_))
        }) {
            Some(ApplicationCommand::BreakpointsChanged(path)) => Some(path),
            _ => None,
        }
    }

    #[cfg(test)]
    fn take_debug_requests(&mut self) -> Vec<DebugRequest> {
        let mut requests = Vec::new();
        self.commands.retain(|command| {
            if let ApplicationCommand::Debug(request) = command {
                requests.push(request.clone());
                false
            } else {
                true
            }
        });
        requests
    }

    #[cfg(test)]
    fn take_build_project_request(&mut self) -> bool {
        self.take_test_command(|command| matches!(command, ApplicationCommand::BuildProject))
            .is_some()
    }

    #[cfg(test)]
    fn take_reimport_project_request(&mut self) -> bool {
        self.take_test_command(|command| matches!(command, ApplicationCommand::ReimportProject))
            .is_some()
    }

    #[cfg(test)]
    fn take_run_request(&mut self) -> bool {
        self.take_test_command(|command| matches!(command, ApplicationCommand::RunProject))
            .is_some()
    }

    #[cfg(test)]
    fn take_stop_request(&mut self) -> bool {
        self.take_test_command(|command| matches!(command, ApplicationCommand::StopProject))
            .is_some()
    }

    #[cfg(test)]
    fn take_open_settings_request(&mut self) -> bool {
        self.take_test_command(|command| matches!(command, ApplicationCommand::OpenSettings))
            .is_some()
    }

    #[cfg(test)]
    fn take_new_item_request(&mut self) -> Option<ide_application::NewItemRequest> {
        match self.take_test_command(|command| matches!(command, ApplicationCommand::CreateItem(_)))
        {
            Some(ApplicationCommand::CreateItem(request)) => Some(request),
            _ => None,
        }
    }

    #[cfg(test)]
    fn take_type_search_request(&mut self) -> Option<String> {
        match self
            .take_test_command(|command| matches!(command, ApplicationCommand::SearchTypes(_)))
        {
            Some(ApplicationCommand::SearchTypes(query)) => Some(query),
            _ => None,
        }
    }

    #[cfg(test)]
    fn take_content_search_request(&mut self) -> Option<String> {
        match self
            .take_test_command(|command| matches!(command, ApplicationCommand::SearchContent(_)))
        {
            Some(ApplicationCommand::SearchContent(query)) => Some(query),
            _ => None,
        }
    }
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

/// A busca marca todas as ocorrências e o `Enter` anda por elas.
///
/// Marcar é o que diz onde elas estão; andar é o que leva até lá. Uma coisa sem
/// a outra deixa o gesto pela metade — ver tudo sem chegar, ou chegar sem ver.
#[test]
fn the_search_bar_marks_every_hit_and_enter_walks_them() {
    let root = PathBuf::from("workspace");
    let mut shell = IdeShell::from_tree(FileNode {
        path: root.clone(),
        is_directory: true,
        children: Vec::new(),
    });
    // "nome" aparece três vezes, em linhas diferentes.
    shell.editor_area.session.open_memory(
        root.join("Pedido.java"),
        "class Pedido {\n    String nome;\n    String nome() { return nome; }\n}\n",
    );

    // Sem a barra aberta, nada é marcado.
    assert!(shell.search_hits().is_empty());

    shell.toggle_search();
    for letra in ["n", "o", "m", "e"] {
        shell.text_input(letra);
    }
    let marcas = shell.search_hits().to_vec();
    assert_eq!(marcas.len(), 3, "as tres ocorrencias precisam ser marcadas");
    let texto = "class Pedido {\n    String nome;\n    String nome() { return nome; }\n}\n";
    for (inicio, fim) in &marcas {
        assert_eq!(&texto[*inicio..*fim], "nome", "cada marca e a palavra");
    }

    // O `Enter` leva o cursor à primeira ocorrência depois dele, e dá a volta.
    shell.editor_area.pane.set_cursor(0);
    for esperada in [marcas[0].0, marcas[1].0, marcas[2].0, marcas[0].0] {
        assert!(shell.go_to_next_search_hit(), "precisa haver para onde ir");
        assert_eq!(
            shell.editor_area.pane.cursor(),
            esperada,
            "o cursor anda de ocorrencia em ocorrencia e volta ao comeco"
        );
    }

    // Apagar uma letra muda o que se procura, e as marcas acompanham.
    shell.key_down("Backspace");
    assert!(
        shell.search_hits().len() >= 3,
        "procurar por menos acha pelo menos o mesmo"
    );

    // Fechar a barra apaga o que se procurava e as marcas junto.
    shell.toggle_search();
    assert!(
        shell.search_hits().is_empty(),
        "destaque nao pode sobreviver a janela que o criou"
    );
}

/// Procurar o que não existe não marca nada e não move o cursor.
#[test]
fn searching_for_what_is_not_there_marks_nothing() {
    let root = PathBuf::from("workspace");
    let mut shell = IdeShell::from_tree(FileNode {
        path: root.clone(),
        is_directory: true,
        children: Vec::new(),
    });
    shell
        .editor_area
        .session
        .open_memory(root.join("Pedido.java"), "class Pedido {}\n");
    shell.toggle_search();
    for letra in ["z", "z", "z"] {
        shell.text_input(letra);
    }
    assert!(shell.search_hits().is_empty());
    shell.editor_area.pane.set_cursor(3);
    assert!(!shell.go_to_next_search_hit(), "sem ocorrencia, nao anda");
    assert_eq!(
        shell.editor_area.pane.cursor(),
        3,
        "e o cursor fica onde estava"
    );
}

/// Quanto custa uma tecla na barra de busca, num arquivo grande.
///
/// Mede antes de consertar: a tecla no editor já teve dois episódios de lentidão
/// (ADR-016 e ADR-017), e a resposta das duas vezes veio de medir, não de supor.
#[test]
#[ignore = "medicao; rodar com --ignored --nocapture"]
fn typing_in_the_search_bar_costs_this_much() {
    let root = PathBuf::from("workspace");
    let mut shell = IdeShell::from_tree(FileNode {
        path: root.clone(),
        is_directory: true,
        children: Vec::new(),
    });
    // Um arquivo grande de verdade: 3.000 linhas, como as classes que doem.
    let mut texto = String::new();
    for numero in 0..3_000 {
        texto.push_str(&format!(
            "    private String nome{numero} = \"valor{numero}\";\n"
        ));
    }
    eprintln!("arquivo com {} KB", texto.len() / 1024);
    shell
        .editor_area
        .session
        .open_memory(root.join("Grande.java"), texto);

    shell.toggle_search();
    let inicio = std::time::Instant::now();
    for letra in ["n", "o", "m", "e"] {
        let tecla = std::time::Instant::now();
        shell.text_input(letra);
        eprintln!("tecla {letra:?}: {:?}", tecla.elapsed());
    }
    eprintln!(
        "quatro teclas: {:?}, {} ocorrencias",
        inicio.elapsed(),
        shell.search_hits().len()
    );
}

/// A ocorrência sob o cursor é a que a IDE marca como em foco.
///
/// É o que dá a borda. Sem escolher uma, todas ficam iguais e quem procura
/// perde o próprio lugar assim que a tela rola.
#[test]
fn the_hit_under_the_cursor_is_the_focused_one() {
    let root = PathBuf::from("workspace");
    let mut shell = IdeShell::from_tree(FileNode {
        path: root.clone(),
        is_directory: true,
        children: Vec::new(),
    });
    shell
        .editor_area
        .session
        .open_memory(root.join("Pedido.java"), "nome e nome e nome");
    shell.toggle_search();
    for letra in ["n", "o", "m", "e"] {
        shell.text_input(letra);
    }
    let marcas = shell.search_hits().to_vec();
    assert_eq!(marcas.len(), 3);

    // O `Enter` leva o cursor a uma ocorrência, e é ela que fica em foco.
    shell.editor_area.pane.set_cursor(0);
    for esperada in [marcas[1], marcas[2], marcas[0]] {
        assert!(shell.go_to_next_search_hit());
        shell.sync_editor_pane(Size::new(1280.0, 800.0));
        assert_eq!(
            shell.editor_area.pane.focused_search_hit(),
            Some(esperada),
            "a ocorrencia sob o cursor e a que recebe a borda"
        );
    }

    // Fora de qualquer ocorrência, nenhuma fica em foco.
    shell.editor_area.pane.set_cursor(4);
    shell.sync_editor_pane(Size::new(1280.0, 800.0));
    assert_eq!(shell.editor_area.pane.focused_search_hit(), None);
}

/// `Esc` fecha a busca e leva o destaque junto.
///
/// Marca que sobrevive à barra fica na tela sem nada dizendo de onde veio nem
/// como tirar. É a mesma regra do fechamento por `Ctrl+F`, e ela precisava valer
/// nos dois caminhos.
#[test]
fn escape_closes_the_search_and_takes_the_marks_with_it() {
    let root = PathBuf::from("workspace");
    let mut shell = IdeShell::from_tree(FileNode {
        path: root.clone(),
        is_directory: true,
        children: Vec::new(),
    });
    shell
        .editor_area
        .session
        .open_memory(root.join("Pedido.java"), "nome e nome e nome");
    shell.toggle_search();
    for letra in ["n", "o", "m", "e"] {
        shell.text_input(letra);
    }
    assert_eq!(shell.search_hits().len(), 3);

    shell.escape();
    assert!(
        shell.search_hits().is_empty(),
        "o destaque tem de sair junto com a busca"
    );
    shell.sync_editor_pane(Size::new(1280.0, 800.0));
    assert_eq!(shell.editor_area.pane.focused_search_hit(), None);
}

/// Clicar no editor tira o foco da barra, mas não a fecha.
///
/// Aberta e focada são coisas diferentes. Amarrá-las fazia o destaque sumir a
/// cada clique no código, e reencontrar o lugar exigia digitar tudo de novo —
/// justamente quando se clica para ler o que a busca achou.
#[test]
fn clicking_the_editor_unfocuses_the_search_without_closing_it() {
    let root = PathBuf::from("workspace");
    let mut shell = IdeShell::from_tree(FileNode {
        path: root.clone(),
        is_directory: true,
        children: Vec::new(),
    });
    shell
        .editor_area
        .session
        .open_memory(root.join("Pedido.java"), "nome e nome e nome");
    shell.toggle_search();
    for letra in ["n", "o", "m", "e"] {
        shell.text_input(letra);
    }
    assert_eq!(shell.search_hits().len(), 3);

    // Um clique no corpo do editor.
    let size = Size::new(1280.0, 800.0);
    let editor_x = ACTIVITY_WIDTH + SIDEBAR_WIDTH;
    shell.pointer_down(Point::new(editor_x + 40.0, TITLE_HEIGHT + 80.0), size);

    assert!(shell.search_is_open(), "a barra continua na tela");
    assert_eq!(
        shell.search_hits().len(),
        3,
        "e o destaque continua marcando o que se procurava"
    );

    // Digitar agora é código, e não consulta.
    shell.text_input("x");
    assert_eq!(
        shell.search_hits().len(),
        3,
        "o texto digitado no editor nao entra na busca"
    );

    // `Ctrl+F` com a barra aberta e sem foco devolve o foco, e não fecha.
    shell.toggle_search();
    assert!(
        shell.search_is_open(),
        "voltar para a busca nao pode fecha-la"
    );
    shell.text_input("!");
    assert!(
        shell.search_hits().is_empty(),
        "e agora o que se digita volta a ser consulta"
    );

    // Só o segundo `Ctrl+F`, já com o foco, fecha.
    shell.toggle_search();
    assert!(!shell.search_is_open());
    assert!(shell.search_hits().is_empty());
}

/// Clicar **dentro** da barra devolve o foco, e o clique não atravessa.
///
/// A barra fica sobre o código. Sem perguntar ao anfitrião onde o ponto caiu, o
/// clique nela moveria o cursor do editor embaixo — e o campo continuaria sem
/// foco, de modo que digitar iria para o lugar errado.
#[test]
fn clicking_inside_the_search_bar_focuses_it_instead_of_the_code() {
    let root = PathBuf::from("workspace");
    let mut shell = IdeShell::from_tree(FileNode {
        path: root.clone(),
        is_directory: true,
        children: Vec::new(),
    });
    shell
        .editor_area
        .session
        .open_memory(root.join("Pedido.java"), "nome e nome e nome");
    let size = Size::new(1280.0, 800.0);
    shell.toggle_search();
    shell.place_overlay(size);

    // O clique no editor tira o foco, como já se sabia.
    let editor_x = ACTIVITY_WIDTH + SIDEBAR_WIDTH;
    shell.pointer_down(Point::new(editor_x + 40.0, TITLE_HEIGHT + 200.0), size);
    assert!(shell.search_is_open());
    let cursor_antes = shell.editor_area.pane.cursor();

    // Agora um clique no meio da barra: o anfitrião diz que o ponto é dela.
    let caixa = shell.search_box_area();
    assert!(caixa.size.width > 0.0, "a barra precisa ter area declarada");
    shell.pointer_down(
        Point::new(
            caixa.origin.x + caixa.size.width / 2.0,
            caixa.origin.y + caixa.size.height / 2.0,
        ),
        size,
    );
    assert_eq!(
        shell.editor_area.pane.cursor(),
        cursor_antes,
        "o clique na barra nao pode mover o cursor do codigo embaixo"
    );

    // E o foco voltou: o que se digita agora é consulta.
    for letra in ["n", "o", "m", "e"] {
        shell.text_input(letra);
    }
    assert_eq!(
        shell.search_hits().len(),
        3,
        "digitar depois do clique e busca"
    );
}
