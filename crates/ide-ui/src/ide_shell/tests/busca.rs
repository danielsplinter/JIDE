//! Testes das **caixas de busca** — a do arquivo e a da saída, que são duas
//! janelas independentes.

use super::*;

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

    // `Shift+Enter` é o mesmo gesto ao contrário, e também dá a volta: do
    // começo ele salta para a última ocorrência do arquivo.
    let com_shift = Modifiers {
        shift: true,
        ..Modifiers::default()
    };
    for esperada in [marcas[2].0, marcas[1].0, marcas[0].0, marcas[2].0] {
        shell.key_down_with_modifiers("Enter", com_shift);
        assert_eq!(
            shell.editor_area.pane.cursor(),
            esperada,
            "com Shift o cursor anda para trás e volta ao fim"
        );
    }
    // E o `Enter` sem `Shift` continua indo para frente, do mesmo ponto.
    shell.key_down("Enter");
    assert_eq!(shell.editor_area.pane.cursor(), marcas[0].0);

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
/// As duas buscas são janelas independentes.
///
/// Elas dividiam um estado só, e abrir uma fechava a outra: com a caixa do
/// arquivo na tela, o `Ctrl+F` do terminal não abria nada. Quem procura `erro` na
/// saída pode estar procurando `Pedido` no código, e uma caixa não pode apagar o
/// texto da outra.
#[test]
fn as_duas_buscas_convivem_sem_se_atrapalhar() {
    let root = std::env::temp_dir().join(format!("er-ide-duas-buscas-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    assert!(std::fs::create_dir_all(&root).is_ok());
    let arquivo = root.join("Pedido.java");
    assert!(std::fs::write(&arquivo, "class Pedido { int total; }").is_ok());
    let Ok(mut shell) = IdeShell::open(&root) else {
        panic!("workspace de teste não abriu");
    };
    let Ok(_) = shell.open_file(&arquivo) else {
        panic!("arquivo de teste não abriu");
    };
    let size = Size::new(1280.0, 800.0);
    let _ = shell.paint(size);

    // A do arquivo primeiro.
    shell.context.focus = ShellFocus::Editor;
    shell.toggle_search();
    shell.text_input("total");
    assert_eq!(shell.editor_area.search_query, "total");

    // E a da saída, sem fechar a outra.
    shell.context.focus = ShellFocus::Terminal;
    shell.toggle_search();
    assert!(
        shell.editor_area.search_open,
        "abrir a busca da saída não pode fechar a do arquivo"
    );
    shell.text_input("PS");
    assert_eq!(
        shell.editor_area.search_query, "total",
        "o que se digita numa caixa não entra na outra"
    );
    assert_eq!(
        shell.terminal.busca.as_ref().map(|busca| busca.texto.as_str()),
        Some("PS")
    );

    // As duas aparecem na tela ao mesmo tempo.
    let desenhado = shell.paint(size);
    let escrito = |texto: &str| {
        desenhado
            .iter()
            .any(|comando| matches!(comando, PaintCommand::DrawText(t) if t.text == texto))
    };
    assert!(escrito("total") && escrito("PS"), "as duas caixas na tela");

    // `Esc` fecha só a que tem o foco.
    shell.escape();
    assert!(shell.terminal.busca.is_none());
    assert!(
        shell.editor_area.search_open,
        "largar a busca da saída não larga a do arquivo"
    );
    let _ = std::fs::remove_dir_all(&root);
}
