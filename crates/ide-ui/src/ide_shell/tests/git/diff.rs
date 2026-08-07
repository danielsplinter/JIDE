//! A aba `Diff`: a comparação, os realces e a margem do editor.

use ide_application::RestoreTarget;

use super::*;

/// Clicar no nome do arquivo abre a comparação, e não uma ação.
///
/// A coluna decide qual das três coisas o clique foi. Sem essa separação, quem
/// quisesse ver o que mudou acabaria preparando o arquivo por engano — e
/// preparar por engano é o começo de um commit que ninguém revisou.
#[test]
fn clicar_no_nome_do_arquivo_pede_a_comparacao() {
    let mut shell = test_shell();
    let size = Size::new(1280.0, 800.0);
    let _ = shell.paint(size);
    let raiz = shell.workspace_root().to_path_buf();
    shell.set_git_view(retrato_com_alteracoes(&raiz));
    shell.toggle_git();
    let _ = shell.paint(size);
    shell.commands.retain(|_| false);

    let context = shell.layout_context();
    let mut superficie = std::mem::take(&mut shell.git);
    let faixas = superficie.faixas_do_status(&shell.host, &context);
    shell.git = superficie;
    let faixa = faixas[1];
    shell.pointer_down(
        Point::new(faixa.origin.x + 30.0, faixa.origin.y + 20.0 + 12.0),
        size,
    );

    let pedidos: Vec<ApplicationCommand> = shell.commands.iter().cloned().collect();
    assert!(
        pedidos.iter().any(|comando| matches!(
            comando,
            ApplicationCommand::Git(GitRequest::ShowDiff { path, staged: false })
                if path.ends_with("alterado.java")
        )),
        "o nome abre a diferença: {pedidos:?}"
    );
    assert!(
        !pedidos
            .iter()
            .any(|comando| matches!(comando, ApplicationCommand::Git(GitRequest::Stage(_)))),
        "e não prepara nada"
    );
}
/// A margem do editor mostra o que mudou desde o commit.
///
/// E **não sobrepõe** o que já está marcado: um ponto de parada numa linha
/// alterada continua sendo um ponto de parada, que é o que a pessoa pôs ali. A
/// marca de versão é informação de fundo.
#[test]
fn a_margem_marca_as_linhas_que_o_git_diz_que_mudaram() {
    let root = std::env::temp_dir().join(format!("er-ide-margem-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    assert!(std::fs::create_dir_all(&root).is_ok());
    let arquivo = root.join("Pedido.java");
    assert!(std::fs::write(&arquivo, "class Pedido {\n  int total;\n}\n").is_ok());
    let Ok(mut shell) = IdeShell::open(&root) else {
        panic!("workspace de teste não abriu");
    };
    let Ok(_) = shell.open_file(&arquivo) else {
        panic!("arquivo de teste não abriu");
    };
    shell.set_git_line_marks(
        arquivo.clone(),
        vec![(1, GitLineChange::Added), (2, GitLineChange::Removed)],
    );

    let decoracoes = shell.editor_decorations(&arquivo);
    assert_eq!(
        decoracoes
            .iter()
            .filter_map(|item| item.mark.map(|mark| (item.line, mark)))
            .collect::<Vec<_>>(),
        vec![(1, GutterMark::LineAdded), (2, GutterMark::LineRemoved)],
        "onde entrou código a marca é verde; a vermelha é de quem só perdeu"
    );

    // O ponto de parada ganha da marca de versão na mesma linha.
    shell.toggle_breakpoint(&arquivo, 1);
    let decoracoes = shell.editor_decorations(&arquivo);
    assert_eq!(
        decoracoes
            .iter()
            .find(|item| item.line == 1)
            .and_then(|item| item.mark),
        Some(GutterMark::PendingBreakpoint),
        "o ponto de parada continua visível"
    );

    // Retrato limpo apaga as marcas: commitar deixa o arquivo igual ao commit, e
    // uma margem riscada estaria contando o trabalho de antes.
    shell.set_git_line_marks(arquivo.clone(), Vec::new());
    assert!(
        shell
            .editor_decorations(&arquivo)
            .iter()
            .all(|item| item.mark != Some(GutterMark::LineAdded)),
    );
    let _ = std::fs::remove_dir_all(&root);
}
/// A descrição de um commit comprido quebra, e a linha da tabela cresce.
///
/// **Quem decide quebrar é a IDE**, e só nesta coluna: data, autor e hash cabem
/// sempre, e a descrição é a única em que o texto sumia por baixo da coluna
/// vizinha. A altura vem junto — quebrar sem crescer poria a segunda linha fora
/// da célula.
#[test]
fn a_descricao_comprida_quebra_e_a_linha_cresce() {
    let mut shell = test_shell();
    let size = Size::new(1280.0, 800.0);
    let _ = shell.paint(size);
    let commit = |hash: &str, resumo: &str| CommitRow {
        hash: hash.to_owned(),
        summary: resumo.to_owned(),
        author: "Teste".to_owned(),
        date: "2026-08-07 10:00".to_owned(),
        lane: 0,
        lanes: 1,
        passing: Vec::new(),
        parents: vec![0],
    };
    shell.set_git_view(GitView {
        head: Some("main".to_owned()),
        commits: vec![
            commit("aaa1111", "curto"),
            commit(
                "bbb2222",
                "uma mensagem de commit bem comprida, dessas que explicam o porquê da                  mudança inteira numa linha só e não cabem na coluna de jeito nenhum",
            ),
        ],
        ..GitView::default()
    });
    shell.toggle_git();
    // O quadro entre abrir e clicar: sem ele a moldura da janela ainda não foi
    // arranjada, e a faixa de abas não tem área para receber o gesto.
    let _ = shell.paint(size);
    // A aba da direita é a do histórico.
    let (_, _, _, abas) = git::GitSurface::areas(&shell.host);
    shell.pointer_down(
        Point::new(
            abas.origin.x + abas.size.width * 0.75,
            abas.origin.y + abas.size.height / 2.0,
        ),
        size,
    );
    let desenhado = shell.paint(size);

    // O texto da descrição sai em mais de um pedaço, e nenhum deles é a
    // mensagem inteira.
    let pedacos: Vec<&str> = desenhado
        .iter()
        .filter_map(|comando| match comando {
            PaintCommand::DrawText(texto) if texto.text.contains("mensagem")
                || texto.text.contains("mudança") =>
            {
                Some(texto.text.as_str())
            }
            _ => None,
        })
        .collect();
    assert!(
        pedacos.len() > 1,
        "a descrição desce para a linha seguinte: {pedacos:?}"
    );

    // E a linha de baixo é mais alta que a de cima, que coube inteira.
    let alturas = shell.git_surface().alturas_do_historico();
    assert!(
        alturas.len() >= 2 && alturas[1] > alturas[0],
        "a linha que quebrou cresceu: {alturas:?}"
    );
}
/// A janela tem duas abas, e o que existe hoje mora na primeira.
///
/// `Diff` nasce vazia de propósito, como os nós da árvore nasceram: uma aba que
/// só aparece quando a capacidade chega faz a janela mudar de forma a cada
/// passo, e quem usa não sabe se ela está vindo ou se sumiu.
#[test]
fn a_janela_tem_geral_e_diff_e_o_que_existe_hoje_esta_na_geral() {
    let mut shell = test_shell();
    let size = Size::new(1280.0, 800.0);
    let _ = shell.paint(size);
    shell.set_git_view(retrato_com_branches());
    shell.toggle_git();
    let desenhado = shell.paint(size);
    let escrito = |quadro: &[PaintCommand], texto: &str| {
        quadro
            .iter()
            .any(|comando| matches!(comando, PaintCommand::DrawText(t) if t.text == texto))
    };

    // As duas abas, e a `Geral` com tudo o que a janela já tinha.
    assert!(escrito(&desenhado, "Geral") && escrito(&desenhado, "Diff"));
    for parte in ["Fetch", "Procurar branch", "Branches (2)", "Status", "History"] {
        assert!(escrito(&desenhado, parte), "falta {parte} na Geral");
    }

    // A `Diff` esconde tudo isso e mostra um painel vazio.
    let janela = shell.git_surface().abas_da_janela_para_teste(&shell.host);
    shell.pointer_down(
        Point::new(
            janela.origin.x + janela.size.width * 0.75,
            janela.origin.y + janela.size.height / 2.0,
        ),
        size,
    );
    let desenhado = shell.paint(size);
    assert!(escrito(&desenhado, "Geral") && escrito(&desenhado, "Diff"));
    for parte in ["Fetch", "Procurar branch", "Branches (2)", "Status"] {
        assert!(
            !escrito(&desenhado, parte),
            "{parte} continuou na tela com a Diff na frente"
        );
    }

    // E o botão de fechar continua lá: ele é da janela, e não da aba.
    let painel = git::GitSurface::areas(&shell.host).0;
    shell.pointer_down(
        Point::new(
            painel.origin.x + painel.size.width - 28.0,
            painel.origin.y + 28.0,
        ),
        size,
    );
    assert!(!shell.git_surface().is_open(), "o X fecha em qualquer aba");
}
/// A comparação abre **na aba `Diff`**, e não atrás da janela.
///
/// Ela era aberta no editor, e a janela dava lugar: quem pedia a diferença via a
/// janela sumir e a comparação aparecer atrás dela. Agora ela é o assunto desta
/// janela, e quem tinha uma mensagem de commit escrita não a perde para ver o
/// que acabou de pedir.
#[test]
fn a_comparacao_abre_na_aba_diff() {
    let mut shell = test_shell();
    let size = Size::new(1280.0, 800.0);
    let _ = shell.paint(size);
    let raiz = shell.workspace_root().to_path_buf();
    shell.set_git_view(retrato_com_alteracoes(&raiz));
    shell.toggle_git();
    let _ = shell.paint(size);

    // Nada pedido ainda: a aba diz por onde se pede.
    let janela = shell.git_surface().abas_da_janela_para_teste(&shell.host);
    shell.pointer_down(
        Point::new(
            janela.origin.x + janela.size.width * 0.75,
            janela.origin.y + janela.size.height / 2.0,
        ),
        size,
    );
    let desenhado = shell.paint(size);
    assert!(
        desenhado.iter().any(|comando| {
            matches!(comando, PaintCommand::DrawText(t) if t.text.contains("Escolha um arquivo"))
        }),
        "sem comparação pedida, o painel explica por onde se pede"
    );

    // Pedida, ela aparece: o título, os dois lados e a linha que mudou.
    assert!(shell.abrir_comparacao(
        &raiz.join("alterado.java"),
        GitDiff {
            current: "class Pedido {
    int total;
}
".to_owned(),
            committed: "class Pedido {
}
".to_owned(),
            marks: vec![(1, GitLineChange::Added)],
            ..GitDiff::default()
        },
    ));
    let desenhado = shell.paint(size);
    let escrito = |texto: &str| {
        desenhado
            .iter()
            .any(|comando| matches!(comando, PaintCommand::DrawText(t) if t.text == texto))
    };
    assert!(escrito("alterado.java"), "o título diz de que arquivo é");
    assert!(escrito("    int total;"), "o lado de agora");
    // O de então tem duas linhas, o de agora tem três: as duas colunas estão lá.
    let fechamentos = desenhado
        .iter()
        .filter(|comando| matches!(comando, PaintCommand::DrawText(t) if t.text == "}"))
        .count();
    assert!(fechamentos >= 2, "os dois lados aparecem: {fechamentos}");

    // E a janela continua aberta, na aba da comparação.
    assert!(shell.git_surface().is_open());
    assert!(!escrito("Procurar branch"), "a aba Geral saiu da frente");
}
/// A linha que mudou é azul dos dois lados; o trecho que mudou tem cor própria.
///
/// **São duas informações diferentes**: o azul diz onde olhar, e a cor do trecho
/// diz o que olhar. Pintar a linha inteira de verde ou de vermelho faria
/// procurar o que mudou dentro do que já foi marcado como mudado.
#[test]
fn o_diff_realca_a_linha_de_azul_e_o_trecho_com_a_cor_do_que_houve() {
    let mut shell = test_shell();
    let size = Size::new(1280.0, 800.0);
    let _ = shell.paint(size);
    let raiz = shell.workspace_root().to_path_buf();
    shell.set_git_view(retrato_com_alteracoes(&raiz));
    shell.toggle_git();
    let _ = shell.paint(size);

    // O de então tinha três linhas e perdeu a do meio; o de agora ganhou outra.
    assert!(shell.abrir_comparacao(
        &raiz.join("alterado.java"),
        GitDiff {
            current: "class Pedido {
    int novo;
}
".to_owned(),
            committed: "class Pedido {
    int velho;
}
".to_owned(),
            marks: vec![(1, GitLineChange::Added)],
            removed: vec![1],
            // `novo` contra `velho`: o que difere é o miolo da palavra.
            added_spans: vec![GitSpan {
                line: 1,
                start: 8,
                end: 12,
            }],
            removed_spans: vec![GitSpan {
                line: 1,
                start: 8,
                end: 13,
            }],
            ..GitDiff::default()
        },
    ));
    let desenhado = shell.paint(size);

    // As cores vêm do tema, e não escritas aqui: o que se afirma é que o realce
    // usa o papel certo, e não um tom específico.
    let cores = shell.context.theme.colors;
    let (verde, vermelho, azul) = (cores.success, cores.danger, cores.accent);
    let realce = |cor: ui_core::Color| {
        desenhado.iter().any(|comando| {
            matches!(comando, PaintCommand::FillRect(fill)
                if (fill.color.red - cor.red).abs() < 0.01
                    && (fill.color.green - cor.green).abs() < 0.01
                    && (fill.color.blue - cor.blue).abs() < 0.01
                    && fill.color.alpha < 1.0)
        })
    };
    assert!(realce(azul), "as duas linhas que mudaram ficam azuis");
    assert!(realce(verde), "o trecho que entrou, à direita, fica verde");
    assert!(realce(vermelho), "e o que saiu, à esquerda, fica vermelho");
}
/// A tela sabe dizer de qual arquivo ela ainda não tem a margem.
///
/// É por aqui que a marca do Git chega **em qualquer forma de trocar de aba** —
/// o clique, o `Ctrl+Tab`, a divisão, a navegação. Cada uma delas lembrar de
/// pedir seria seis lugares para esquecer; a aplicação pergunta isto a cada
/// quadro, e é uma consulta a um mapa.
#[test]
fn a_tela_diz_de_que_arquivo_falta_a_margem() {
    let root = std::env::temp_dir().join(format!("er-ide-margem-falta-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    assert!(std::fs::create_dir_all(&root).is_ok());
    let primeiro = root.join("Um.java");
    let segundo = root.join("Dois.java");
    assert!(std::fs::write(&primeiro, "class Um {}
").is_ok());
    assert!(std::fs::write(&segundo, "class Dois {}
").is_ok());
    let Ok(mut shell) = IdeShell::open(&root) else {
        panic!("workspace de teste não abriu");
    };
    let Ok(_) = shell.open_file(&primeiro) else {
        panic!("o primeiro arquivo não abriu");
    };

    // Recém-aberto, ninguém sabe o que mudou nele.
    assert_eq!(shell.git_marks_missing(), Some(primeiro.clone()));

    // **Resposta vazia é resposta.** Um arquivo igual ao commit não pode fazer a
    // IDE perguntar de novo a cada quadro.
    shell.set_git_line_marks(primeiro.clone(), Vec::new());
    assert_eq!(shell.git_marks_missing(), None);

    // Abrir o segundo faz faltar a dele, sem apagar o que se sabe do primeiro.
    let Ok(_) = shell.open_file(&segundo) else {
        panic!("o segundo arquivo não abriu");
    };
    assert_eq!(shell.git_marks_missing(), Some(segundo.clone()));
    shell.set_git_line_marks(segundo.clone(), vec![(0, GitLineChange::Added)]);
    assert_eq!(shell.git_marks_missing(), None);

    // E voltar para o primeiro não repergunta: o que se sabe dele continua lá.
    let Ok(_) = shell.open_file(&primeiro) else {
        panic!("o primeiro arquivo não reabriu");
    };
    assert_eq!(
        shell.git_marks_missing(),
        None,
        "voltar a um arquivo conhecido não pergunta de novo"
    );
    // E a marca do segundo sobreviveu à ida e à volta.
    assert_eq!(
        shell
            .editor_decorations(&segundo)
            .iter()
            .filter_map(|item| item.mark)
            .collect::<Vec<_>>(),
        vec![GutterMark::LineAdded]
    );
    let _ = std::fs::remove_dir_all(&root);
}
/// As setas da coluna de então aparecem em **todas** as linhas marcadas.
///
/// Elas não acompanham a escolha: quem lê uma comparação quer devolver duas ou
/// três linhas seguidas, e uma seta só, atrás de um clique de seleção, faria
/// dobrar os cliques de cada devolução.
#[test]
fn as_setas_da_esquerda_ficam_em_todas_as_linhas_marcadas() {
    let mut shell = test_shell();
    let size = Size::new(1280.0, 800.0);
    let _ = shell.paint(size);
    let raiz = shell.workspace_root().to_path_buf();
    shell.set_git_view(retrato_com_alteracoes(&raiz));
    shell.toggle_git();
    let _ = shell.paint(size);
    let arquivo = raiz.join("alterado.java");
    assert!(shell.abrir_comparacao(
        &arquivo,
        GitDiff {
            current: "class Pedido {
    int novo;
}
".to_owned(),
            committed: "class Pedido {
    int velho;
}
".to_owned(),
            marks: vec![(1, GitLineChange::Added)],
            removed: vec![0, 1],
            ..GitDiff::default()
        },
    ));

    // Sem escolher nada: as duas linhas marcadas já têm a sua seta.
    let desenhado = shell.paint(size);
    let setas = desenhado
        .iter()
        .filter(|comando| matches!(comando, PaintCommand::DrawText(t) if t.text == "→"))
        .count();
    assert_eq!(setas, 2, "uma seta por linha marcada, sem escolher nenhuma");

    let mut superficie = std::mem::take(&mut shell.git);
    let botoes = superficie.botoes_de_aplicar_para_teste(&shell.host);
    shell.git = superficie;
    assert_eq!(botoes.len(), 2, "as setas são as das linhas marcadas");

    // E o clique numa delas pede a devolução daquela linha, e não da escolhida.
    shell.commands.retain(|_| false);
    let botao = botoes[1].1;
    shell.pointer_down(
        Point::new(
            botao.origin.x + botao.size.width / 2.0,
            botao.origin.y + botao.size.height / 2.0,
        ),
        size,
    );
    assert!(
        shell.commands.iter().any(|comando| matches!(
            comando,
            ApplicationCommand::Git(GitRequest::RestoreLine { path, from: 1, target })
                if path == &arquivo && *target == RestoreTarget::Replace(1)
        )),
        "a linha existe dos dois lados, e a devolução é uma troca: {:?}",
        shell.commands.iter().collect::<Vec<_>>()
    );

    // E o texto que vai é o da coluna de então.
    assert_eq!(
        shell.git_diff_line(1).as_deref(),
        Some("    int velho;"),
        "o texto devolvido é o do commit"
    );
}

/// Uma linha que só existe do lado de então é **acrescentada**, e não trocada.
///
/// É o caso que apagava código. `a b c` viram `a c`: sem as fileiras, a linha 1
/// da esquerda (`b`) ficava ao lado da linha 1 da direita (`c`), e devolver o
/// `b` mandava escrevê-lo por cima do `c`. O `c` sumia, sem aviso e sem
/// desfazer.
#[test]
fn a_linha_que_so_existe_do_lado_de_entao_e_acrescentada() {
    let mut shell = test_shell();
    let size = Size::new(1280.0, 800.0);
    let _ = shell.paint(size);
    let raiz = shell.workspace_root().to_path_buf();
    shell.set_git_view(retrato_com_alteracoes(&raiz));
    shell.toggle_git();
    let _ = shell.paint(size);
    let arquivo = raiz.join("alterado.java");
    assert!(shell.abrir_comparacao(
        &arquivo,
        GitDiff {
            committed: "a
b
c
".to_owned(),
            current: "a
c
".to_owned(),
            removed: vec![1],
            // As fileiras que o domínio calculou: o `c` dos dois lados volta a
            // ficar na mesma altura, e a do meio tem o lado direito vazio.
            pairs: vec![
                GitLinePair {
                    old: Some(0),
                    new: Some(0),
                },
                GitLinePair {
                    old: Some(1),
                    new: None,
                },
                GitLinePair {
                    old: Some(2),
                    new: Some(1),
                },
            ],
            ..GitDiff::default()
        },
    ));
    let _ = shell.paint(size);

    let mut superficie = std::mem::take(&mut shell.git);
    let botoes = superficie.botoes_de_aplicar_para_teste(&shell.host);
    shell.git = superficie;
    assert_eq!(botoes.len(), 1, "só o `b` saiu");

    shell.commands.retain(|_| false);
    let botao = botoes[0].1;
    shell.pointer_down(
        Point::new(
            botao.origin.x + botao.size.width / 2.0,
            botao.origin.y + botao.size.height / 2.0,
        ),
        size,
    );
    assert!(
        shell.commands.iter().any(|comando| matches!(
            comando,
            ApplicationCommand::Git(GitRequest::RestoreLine { path, from: 1, target })
                if path == &arquivo && *target == RestoreTarget::Insert(1)
        )),
        "acrescentar na posição 1, e não trocar a linha 1 — que é o `c`: {:?}",
        shell.commands.iter().collect::<Vec<_>>()
    );
}

/// As duas colunas mostram o mesmo número de fileiras, e por isso se conferem.
///
/// Enquanto cada coluna era o seu texto numerado do zero, um arquivo de três
/// linhas ficava ao lado de um de duas e tudo abaixo da remoção escorregava uma
/// altura. Comparar exige que a mesma altura fale da mesma coisa.
#[test]
fn as_duas_colunas_tem_a_mesma_quantidade_de_fileiras() {
    let mut shell = test_shell();
    let size = Size::new(1280.0, 800.0);
    let _ = shell.paint(size);
    let raiz = shell.workspace_root().to_path_buf();
    shell.set_git_view(retrato_com_alteracoes(&raiz));
    shell.toggle_git();
    let _ = shell.paint(size);
    assert!(shell.abrir_comparacao(
        &raiz.join("alterado.java"),
        GitDiff {
            committed: "a
b
c
".to_owned(),
            current: "a
c
".to_owned(),
            removed: vec![1],
            pairs: vec![
                GitLinePair {
                    old: Some(0),
                    new: Some(0),
                },
                GitLinePair {
                    old: Some(1),
                    new: None,
                },
                GitLinePair {
                    old: Some(2),
                    new: Some(1),
                },
            ],
            ..GitDiff::default()
        },
    ));
    let desenhado = shell.paint(size);

    // O `c` aparece duas vezes, e na mesma altura: um de cada lado.
    let alturas: Vec<f32> = desenhado
        .iter()
        .filter_map(|comando| match comando {
            PaintCommand::DrawText(texto) if texto.text == "c" => Some(texto.origin.y),
            _ => None,
        })
        .collect();
    assert_eq!(alturas.len(), 2, "o `c` está nos dois lados: {alturas:?}");
    assert!(
        (alturas[0] - alturas[1]).abs() < 0.5,
        "e na mesma altura, que é o que faz duas colunas se conferirem: {alturas:?}"
    );

    // E o número de linha mostrado é o do arquivo, não o da fileira: o `c` é a
    // linha 3 de um lado e a 2 do outro.
    let numeros: Vec<String> = desenhado
        .iter()
        .filter_map(|comando| match comando {
            PaintCommand::DrawText(texto) if texto.text.trim() == "3" => {
                Some(texto.text.trim().to_owned())
            }
            _ => None,
        })
        .collect();
    assert_eq!(numeros.len(), 1, "só a coluna de então chega à linha 3");
}

/// Arrastar a barra de uma coluna arrasta a outra junto.
///
/// A roda do mouse chega às duas e elas já andavam juntas. O arrasto, não: quem
/// arrasta segura *uma* barra, e a outra coluna nem sabia que houve gesto — a
/// linha 40 de um lado ia parar ao lado de outra qualquer, e a comparação
/// deixava de comparar no meio do gesto.
#[test]
fn arrastar_a_barra_de_uma_coluna_leva_a_outra_junto() {
    let mut shell = test_shell();
    let size = Size::new(1280.0, 800.0);
    let _ = shell.paint(size);
    let raiz = shell.workspace_root().to_path_buf();
    shell.set_git_view(retrato_com_alteracoes(&raiz));
    shell.toggle_git();
    let _ = shell.paint(size);

    // Texto comprido nos dois lados: sem o que rolar, não há gesto a testar.
    let comprido = |marca: &str| {
        (0..200)
            .map(|numero| format!("{marca} linha {numero}"))
            .collect::<Vec<_>>()
            .join("
")
    };
    assert!(shell.abrir_comparacao(
        &raiz.join("alterado.java"),
        GitDiff {
            committed: comprido("entao"),
            current: comprido("agora"),
            ..GitDiff::default()
        },
    ));
    let _ = shell.paint(size);

    let mut superficie = std::mem::take(&mut shell.git);
    let colunas = superficie.colunas_do_diff_para_teste(&shell.host);
    shell.git = superficie;
    assert_eq!(shell.git_surface().rolagem_do_diff(), [0.0, 0.0]);

    // A trilha da coluna da esquerda fica encostada na borda direita dela.
    let alca = Point::new(
        colunas[0].origin.x + colunas[0].size.width - 4.0,
        colunas[0].origin.y + 20.0,
    );
    shell.pointer_down(alca, size);
    shell.pointer_move(Point::new(alca.x, alca.y + 150.0), size);

    let [esquerda, direita] = shell.git_surface().rolagem_do_diff();
    assert!(
        esquerda > 0.0,
        "arrastar a alça rola a coluna de então: {esquerda}"
    );
    assert!(
        (esquerda - direita).abs() < 0.5,
        "e a de agora vai junto, ou as duas deixam de se conferir: {esquerda} e {direita}"
    );
}

/// Devolver uma linha muda o editor principal daquele arquivo, sem roubar o foco.
///
/// O texto que a janela do Git grava é o texto que o editor mostra. Antes ele
/// ficava com o de antes até alguém reabrir o arquivo — a resposta velha
/// parecida com a certa, que é a pior de todas. E o foco tem de ficar onde
/// estava: a janela do Git não fechou, e quem clicou continua nela.
#[test]
fn refrescar_um_documento_troca_o_texto_sem_mudar_o_foco() {
    let mut shell = test_shell();
    let size = Size::new(1280.0, 800.0);
    let raiz = shell.workspace_root().to_path_buf();
    let arquivo = raiz.join("alterado.java");
    shell.show_document(&arquivo, "class Pedido {\n    int novo;\n}\n");
    let _ = shell.paint(size);
    shell.toggle_git();
    let _ = shell.paint(size);
    let foco = shell.focus();

    assert!(
        shell.refresh_document(&arquivo, "class Pedido {\n    int velho;\n}\n"),
        "o arquivo está aberto, então há o que refrescar"
    );
    assert_eq!(
        shell.active_text(),
        Some("class Pedido {\n    int velho;\n}\n"),
        "o editor mostra o que foi gravado"
    );
    assert_eq!(shell.focus(), foco, "o foco continua onde estava");

    assert!(
        !shell.refresh_document(&raiz.join("fechado.java"), "nada"),
        "arquivo que não está aberto não tem o que refrescar, e não é erro"
    );
}
