//! Testes da **árvore do projeto**: o que ela mostra, o que ela abre e o que
//! ela marca.

use super::*;

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
    assert_eq!(raiz.especie, Especie::Pasta, "toda pasta leva a marca");

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
/// A extensão dá o crachá sem perguntar a ninguém, e o mais específico ganha.
#[test]
fn a_extensao_da_o_cracha_e_o_mais_especifico_ganha() {
    let projeto = "app/src";
    let arquivo = |nome: &str| file(&format!("{projeto}/{nome}"));
    let tree = dir(
        "app",
        vec![dir(
            projeto,
            vec![
                arquivo("pedido.ts"),
                arquivo("pedido.spec.ts"),
                arquivo("pedido.component.html"),
                arquivo("estilo.scss"),
                arquivo("estilo.css"),
                arquivo("leiame.md"),
            ],
        )],
    );

    // Sem índice nenhum: a extensão não espera o analisador subir.
    let nos = explorer_nomes(&tree, &[], &HashMap::new());
    let Some(src) = nos.first().map(|raiz| &raiz.children) else {
        panic!("a árvore precisa ter a pasta de fontes");
    };
    let especie = |nome: &str| {
        src.iter()
            .find(|no| no.label == nome)
            .map(|no| no.especie)
    };
    assert_eq!(especie("pedido.ts"), Some(Especie::TypeScript));
    assert_eq!(
        especie("pedido.spec.ts"),
        Some(Especie::Teste),
        "`.spec.ts` diz mais do que `.ts`, e por isso e decidido antes"
    );
    assert_eq!(especie("pedido.component.html"), Some(Especie::Marcacao));
    assert_eq!(especie("estilo.scss"), Some(Especie::FolhaSass));
    assert_eq!(especie("estilo.css"), Some(Especie::FolhaEstilo));
    assert_eq!(
        especie("leiame.md"),
        Some(Especie::Nenhuma),
        "extensão que ninguém reivindica não ganha crachá"
    );
}
/// Toda pasta ganha a marca, em qualquer tipo de projeto.
///
/// Antes ela dependia de a linguagem declarar raízes de fontes — e num projeto
/// sem elas, como um Angular, pasta nenhuma era marcada. A árvore fica
/// ilegível quando pasta e arquivo se parecem, e isso não depende do projeto.
#[test]
fn toda_pasta_ganha_a_marca_em_qualquer_projeto() {
    let tree = dir(
        "app",
        vec![
            dir("app/src", vec![file("app/src/main.ts")]),
            dir("app/node_modules", Vec::new()),
        ],
    );
    // Lista de raízes **vazia**: é o que um projeto sem convenção de fontes dá.
    let nos = explorer_nomes(&tree, &[], &HashMap::new());
    for no in &nos {
        assert_eq!(
            no.especie,
            Especie::Pasta,
            "a pasta {} precisa da marca mesmo sem raiz de fontes declarada",
            no.label
        );
    }
    assert!(!nos.is_empty(), "o projeto de teste precisa ter pastas");
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
/// Expandir uma pasta não arrasta a árvore para o arquivo da aba.
///
/// Quando os filhos de uma pasta chegam, o Explorer reconciliava com o
/// documento ativo: rolava até ele e o selecionava. Para quem acabou de clicar
/// num pacote, isso é a lista pulando para outro lugar e a seleção mudando
/// sozinha, sem nada a ver com o que se abriu.
///
/// A reconciliação existe para outro caso — a árvore é rasa, e o arquivo da aba
/// pode estar sob uma pasta que ninguém abriu ainda —, e é só nele que ela vale.
#[test]
fn expandir_uma_pasta_nao_pula_para_o_arquivo_da_aba() {
    // O arquivo da aba precisa cair **fora da parte visível**, senão a
    // reconciliação não teria para onde rolar e o teste passaria sem provar
    // nada — foi assim na primeira versão dele.
    let mut filhos: Vec<FileNode> = (0..60)
        .map(|indice| dir(&format!("demo/p{indice:02}"), Vec::new()))
        .collect();
    filhos.push(dir(
        "demo/aberto",
        vec![file("demo/aberto/Aberto.java")],
    ));
    filhos.push(dir("demo/outro", Vec::new()));
    let tree = dir("demo", filhos);
    let mut shell = IdeShell::from_tree(tree);
    // A pasta do arquivo aberto está expandida: ele existe na árvore, e não há
    // o que reconciliar.
    shell
        .explorer
        .expanded
        .insert(PathBuf::from("demo/aberto"));
    shell.sync_explorer_tree();
    shell.show_document(Path::new("demo/aberto/Aberto.java"), "class Aberto {}");
    assert!(
        shell.explorer.scroll_line > 0,
        "o teste precisa de um arquivo fundo o bastante para a rolagem existir"
    );

    // De volta ao topo, como quem rolou para olhar outra coisa.
    shell.explorer.scroll_line = 0;
    shell.explorer.tree.set_selected(None);

    // Agora chegam os filhos de uma pasta **outra**, como no clique de expandir.
    shell.insert_path_children(vec![(
        PathBuf::from("demo/outro"),
        vec![file("demo/outro/Outro.java")],
    )]);

    assert_eq!(
        shell.explorer.scroll_line, 0,
        "a árvore não pode rolar para o arquivo da aba quando alguém expande \
         outra pasta"
    );
    assert_eq!(
        shell.explorer.tree.selected(),
        None,
        "nem trocar a seleção sozinha"
    );
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
        shell.context_menu.pasta().cloned(),
        Some(PathBuf::from("demo/src"))
    );
    assert_eq!(
        entry_labels(shell.context_menu.menu.entries()),
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
        shell.context_menu.pasta().cloned(),
        Some(PathBuf::from("demo/src/main/java"))
    );
    // A criação é na pasta do arquivo; renomear é do arquivo clicado, e por
    // isso as duas coisas convivem no mesmo menu.
    assert_eq!(
        entry_labels(shell.context_menu.menu.entries()),
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
        shell.context_menu.arquivo().cloned(),
        Some(PathBuf::from("demo/src/main/java/App.java")),
        "renomear precisa do arquivo, e não da pasta"
    );
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
    // Muitos arquivos antes: o ativo precisa cair **fora** da parte visível
    // para haver o que revelar. Com poucos, ele já estaria à vista, e exigir
    // rolagem seria exigir que a árvore se mexesse à toa.
    for index in 0..60 {
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
/// O menu abre a janela com o pacote do alvo já preenchido.
///
/// Quem clicou com o botão direito sobre um pacote não deveria ter que
/// digitar de novo onde está.
#[test]
fn the_new_item_dialog_opens_with_the_clicked_package() {
    let mut shell = shell_with_package();
    shell.context_menu.alvo = Some(AlvoDoMenu::Explorer {
        pasta: PathBuf::from("demo/src/main/java/br/com"),
        arquivo: None,
    });
    shell.run_context_command("explorer.new.java.class");
    assert!(shell.new_item_dialog_open());
    assert_eq!(NewItemSurface::values(&shell.host), ("br.com", ""));
}
/// Enter com só o pacote pede o pacote; o nome fica vazio.
#[test]
fn enter_with_only_the_package_asks_for_the_package() {
    let mut shell = shell_with_package();
    shell.context_menu.alvo = Some(AlvoDoMenu::Explorer {
        pasta: PathBuf::from("demo/src/main/java/br/com"),
        arquivo: None,
    });
    shell.run_context_command("explorer.new.java.package");
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
/// Com o nome preenchido, o tipo é pedido dentro do pacote informado.
#[test]
fn enter_with_a_name_asks_for_the_type_inside_the_package() {
    let mut shell = shell_with_package();
    shell.context_menu.alvo = Some(AlvoDoMenu::Explorer {
        pasta: PathBuf::from("demo/src/main/java/br/com"),
        arquivo: None,
    });
    shell.run_context_command("explorer.new.java.interface");
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
