//! Os comandos da comparação: andar entre as alterações, trocar de lado,
//! devolver um trecho inteiro, e o que a tela recusa comparar.

use ui_components::Panel;
use ui_core::Modifiers;

use super::*;

/// `F7` anda entre as alterações, e `Shift+F7` volta; as duas dão a volta.
///
/// Sem isto, achar a terceira alteração de um arquivo de mil linhas é rolar
/// procurando o azul. A contagem no alto diz onde se está, que é a outra
/// metade: "3 de 12" responde quanto falta sem obrigar a chegar ao fim para
/// descobrir.
#[test]
fn andar_entre_as_alteracoes_leva_a_cada_bloco_e_da_a_volta() {
    let mut shell = test_shell();
    let size = Size::new(1280.0, 800.0);
    let _ = shell.paint(size);
    let raiz = shell.workspace_root().to_path_buf();
    shell.set_git_view(retrato_com_alteracoes(&raiz));
    shell.toggle_git();
    let _ = shell.paint(size);

    // Duzentas linhas com duas alterações: uma no começo, outra bem embaixo. E
    // a de baixo tem duas linhas seguidas, que são **uma** alteração.
    let texto = |marca: &str| {
        (0..200)
            .map(|numero| match numero {
                3 | 150 | 151 => format!("linha {numero} {marca}"),
                _ => format!("linha {numero}"),
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    assert!(shell.abrir_comparacao(
        &raiz.join("alterado.java"),
        GitDiff {
            committed: texto("entao"),
            current: texto("agora"),
            marks: vec![
                (3, GitLineChange::Added),
                (150, GitLineChange::Added),
                (151, GitLineChange::Added),
            ],
            removed: vec![3, 150, 151],
            ..GitDiff::default()
        },
    ));
    let _ = shell.paint(size);

    let contagem = |shell: &mut IdeShell| {
        shell
            .paint(size)
            .iter()
            .find_map(|comando| match comando {
                // A contagem, e não o botão do lado: "Árvore de trabalho"
                // também tem um " de " no meio.
                PaintCommand::DrawText(texto)
                    if texto.text.ends_with("alterações")
                        || texto.text.split(" de ").count() == 2
                            && texto
                                .text
                                .chars()
                                .next()
                                .is_some_and(|letra| letra.is_ascii_digit()) =>
                {
                    Some(texto.text.clone())
                }
                _ => None,
            })
    };
    assert_eq!(
        contagem(&mut shell).as_deref(),
        Some("2 alterações"),
        "duas linhas seguidas são uma alteração, e não duas"
    );

    shell.key_down("F7");
    assert_eq!(contagem(&mut shell).as_deref(), Some("1 de 2"));
    let primeira = shell.git_surface().rolagem_do_diff();

    shell.key_down("F7");
    assert_eq!(contagem(&mut shell).as_deref(), Some("2 de 2"));
    let segunda = shell.git_surface().rolagem_do_diff();
    assert!(
        segunda[0] > primeira[0],
        "a segunda alteração está mais abaixo: {primeira:?} para {segunda:?}"
    );
    assert!(
        (segunda[0] - segunda[1]).abs() < 0.5,
        "e as duas colunas foram juntas"
    );

    // Dá a volta: depois da última vem a primeira.
    shell.key_down("F7");
    assert_eq!(contagem(&mut shell).as_deref(), Some("1 de 2"));

    // E `Shift+F7` é o mesmo gesto ao contrário.
    shell.key_down_with_modifiers(
        "F7",
        Modifiers {
            shift: true,
            ..Modifiers::default()
        },
    );
    assert_eq!(contagem(&mut shell).as_deref(), Some("2 de 2"));
}

/// O botão do cabeçalho troca o lado da comparação: preparado ou árvore.
///
/// São duas diferenças distintas sobre o mesmo arquivo. Quem preparou parte do
/// trabalho e vê só uma delas conclui que o resto se perdeu.
#[test]
fn o_cabecalho_troca_entre_o_preparado_e_a_arvore_de_trabalho() {
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
            committed: "a\n".to_owned(),
            current: "b\n".to_owned(),
            staged: false,
            ..GitDiff::default()
        },
    ));
    let desenhado = shell.paint(size);
    assert!(
        desenhado.iter().any(|comando| matches!(
            comando,
            PaintCommand::DrawText(texto) if texto.text == "Árvore de trabalho"
        )),
        "o cabeçalho diz de que lado é a comparação"
    );

    let contexto = shell.layout_context();
    let comandos = shell
        .git_surface()
        .comandos_do_cabecalho_para_teste(&shell.host, &contexto);
    // Nenhum encosta no vizinho: é o que a barra garante, e o que os três
    // botões postos à mão não garantiam.
    for par in comandos.windows(2) {
        assert!(
            par[1].1.origin.x >= par[0].1.origin.x + par[0].1.size.width,
            "os comandos não se sobrepõem: {comandos:?}"
        );
    }
    let botao = comandos[0].1;
    shell.commands.retain(|_| false);
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
            ApplicationCommand::Git(GitRequest::ShowDiff { path, staged: true })
                if path == &arquivo
        )),
        "o clique pede a outra diferença: {:?}",
        shell.commands.iter().collect::<Vec<_>>()
    );
}

/// Um bloco de várias linhas ganha uma seta que devolve o trecho inteiro.
///
/// Devolver uma alteração de sete linhas eram sete cliques — e sete gravações,
/// com os números andando entre uma e outra. Num bloco de uma linha só a seta
/// não aparece: ela faria o que a da linha já faz.
#[test]
fn um_bloco_de_varias_linhas_ganha_a_seta_do_trecho() {
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
            committed: "a\nb\nc\nd\n".to_owned(),
            current: "a\nB\nC\nd\n".to_owned(),
            marks: vec![(1, GitLineChange::Added), (2, GitLineChange::Added)],
            removed: vec![1, 2],
            ..GitDiff::default()
        },
    ));
    let _ = shell.paint(size);

    let mut superficie = std::mem::take(&mut shell.git);
    let trechos = superficie.botoes_de_trecho_para_teste(&shell.host);
    shell.git = superficie;
    assert_eq!(trechos.len(), 1, "as duas linhas seguidas são um trecho só");

    shell.commands.retain(|_| false);
    let botao = trechos[0].2;
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
            ApplicationCommand::Git(GitRequest::RestoreRange { path, from, to })
                if path == &arquivo && *from == (1, 3) && *to == (1, 3)
        )),
        "a seta do trecho devolve as duas de uma vez: {:?}",
        shell.commands.iter().collect::<Vec<_>>()
    );
}

/// Escolher uma linha num lado escolhe a mesma fileira no outro.
///
/// A fileira já emparelha as duas versões. Escolher à esquerda e a direita
/// continuar noutra linha é a comparação dizendo duas coisas ao mesmo tempo.
#[test]
fn escolher_de_um_lado_escolhe_a_mesma_fileira_do_outro() {
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
            committed: "a\nb\nc\n".to_owned(),
            current: "a\nB\nc\n".to_owned(),
            ..GitDiff::default()
        },
    ));
    let _ = shell.paint(size);

    let mut superficie = std::mem::take(&mut shell.git);
    let colunas = superficie.colunas_do_diff_para_teste(&shell.host);
    shell.git = superficie;
    // A terceira fileira da coluna da esquerda.
    shell.pointer_down(
        Point::new(colunas[0].origin.x + 30.0, colunas[0].origin.y + 2.5 * 24.0),
        size,
    );
    let escolhidas = shell.git_surface().escolhidas_do_diff();
    assert!(escolhidas[0].is_some(), "há uma escolhida: {escolhidas:?}");
    assert_eq!(
        escolhidas[0], escolhidas[1],
        "e os dois lados apontam para a mesma fileira: {escolhidas:?}"
    );
}

/// Binário não vira duas colunas de lixo: vira uma frase.
///
/// Desenhar os bytes de um `.png` como texto enche a tela e ainda oferece setas
/// para devolvê-lo linha a linha.
#[test]
fn arquivo_binario_ganha_um_aviso_no_lugar_das_colunas() {
    let mut shell = test_shell();
    let size = Size::new(1280.0, 800.0);
    let _ = shell.paint(size);
    let raiz = shell.workspace_root().to_path_buf();
    shell.set_git_view(retrato_com_alteracoes(&raiz));
    shell.toggle_git();
    let _ = shell.paint(size);
    assert!(shell.abrir_comparacao(
        &raiz.join("marca.png"),
        GitDiff {
            committed: "\u{0}PNG\u{1}\u{2}".to_owned(),
            current: "\u{0}PNG\u{1}\u{3}".to_owned(),
            ..GitDiff::default()
        },
    ));
    let desenhado = shell.paint(size);
    assert!(
        desenhado.iter().any(|comando| matches!(
            comando,
            PaintCommand::DrawText(texto) if texto.text.contains("binário")
        )),
        "o aviso aparece"
    );

    let mut superficie = std::mem::take(&mut shell.git);
    let setas = superficie.botoes_de_aplicar_para_teste(&shell.host);
    shell.git = superficie;
    assert!(setas.is_empty(), "e não há seta nenhuma a oferecer");
}

/// A seta também aparece onde só **entrou** código, e desfaz a adição.
///
/// Faltava exatamente isto: a seta só existia onde algo tinha saído. Uma linha
/// acrescentada não tem par do lado de então, e a comparação a mostrava sem
/// oferecer nada — quando desfazer o que se acabou de escrever é o gesto mais
/// comum de todos, e é o que o `»` do IntelliJ faz na mesma seta.
#[test]
fn a_seta_desfaz_uma_linha_acrescentada() {
    let mut shell = test_shell();
    let size = Size::new(1280.0, 800.0);
    let _ = shell.paint(size);
    let raiz = shell.workspace_root().to_path_buf();
    shell.set_git_view(retrato_com_alteracoes(&raiz));
    shell.toggle_git();
    let _ = shell.paint(size);
    let arquivo = raiz.join("alterado.java");

    // Uma linha nova entre a primeira e a segunda: só existe do lado de agora.
    assert!(shell.abrir_comparacao(
        &arquivo,
        GitDiff {
            committed: "a\nb\n".to_owned(),
            current: "a\nnova\nb\n".to_owned(),
            marks: vec![(1, GitLineChange::Added)],
            pairs: vec![
                GitLinePair {
                    old: Some(0),
                    new: Some(0),
                },
                GitLinePair {
                    old: None,
                    new: Some(1),
                },
                GitLinePair {
                    old: Some(1),
                    new: Some(2),
                },
            ],
            ..GitDiff::default()
        },
    ));
    let _ = shell.paint(size);

    let mut superficie = std::mem::take(&mut shell.git);
    let setas = superficie.botoes_de_aplicar_para_teste(&shell.host);
    shell.git = superficie;
    assert_eq!(
        setas.len(),
        1,
        "a linha acrescentada tem seta, e nada mais tem"
    );

    shell.commands.retain(|_| false);
    let botao = setas[0].1;
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
            ApplicationCommand::Git(GitRequest::RestoreRange { path, from, to })
                if path == &arquivo && *from == (1, 1) && *to == (1, 2)
        )),
        "nada entra e a linha 1 sai: é apagar a linha nova: {:?}",
        shell.commands.iter().collect::<Vec<_>>()
    );
}

/// E um bloco inteiro de linhas novas volta de uma vez só.
///
/// É a outra metade do que faltava: o IntelliJ agrupa, e clicar uma vez desfaz o
/// bloco. Sem isso, um bloco de sete linhas coladas eram sete cliques.
#[test]
fn a_seta_do_trecho_desfaz_um_bloco_inteiro_de_linhas_novas() {
    let mut shell = test_shell();
    let size = Size::new(1280.0, 800.0);
    let _ = shell.paint(size);
    let raiz = shell.workspace_root().to_path_buf();
    shell.set_git_view(retrato_com_alteracoes(&raiz));
    shell.toggle_git();
    let _ = shell.paint(size);
    let arquivo = raiz.join("alterado.java");

    // Três linhas novas seguidas, e nada removido.
    assert!(shell.abrir_comparacao(
        &arquivo,
        GitDiff {
            committed: "a\nb\n".to_owned(),
            current: "a\numa\noutra\nmais\nb\n".to_owned(),
            marks: vec![
                (1, GitLineChange::Added),
                (2, GitLineChange::Added),
                (3, GitLineChange::Added),
            ],
            pairs: vec![
                GitLinePair {
                    old: Some(0),
                    new: Some(0),
                },
                GitLinePair {
                    old: None,
                    new: Some(1),
                },
                GitLinePair {
                    old: None,
                    new: Some(2),
                },
                GitLinePair {
                    old: None,
                    new: Some(3),
                },
                GitLinePair {
                    old: Some(1),
                    new: Some(4),
                },
            ],
            ..GitDiff::default()
        },
    ));
    let _ = shell.paint(size);

    let mut superficie = std::mem::take(&mut shell.git);
    let trechos = superficie.botoes_de_trecho_para_teste(&shell.host);
    shell.git = superficie;
    assert_eq!(trechos.len(), 1, "as três seguidas são um bloco só");

    shell.commands.retain(|_| false);
    let botao = trechos[0].2;
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
            ApplicationCommand::Git(GitRequest::RestoreRange { path, from, to })
                if path == &arquivo && *from == (1, 1) && *to == (1, 4)
        )),
        "as três saem de uma vez, e nada entra: {:?}",
        shell.commands.iter().collect::<Vec<_>>()
    );
}

/// As setas saem em preto, e não na cor do código que está por baixo.
///
/// Elas flutuam sobre o texto: na cor de texto ficavam iguais ao código de
/// baixo, e um controle que se confunde com o conteúdo é um controle que
/// ninguém vê.
#[test]
fn as_setas_saem_em_preto_e_maiores_que_o_texto() {
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
            committed: "a\nb\nc\nd\n".to_owned(),
            current: "a\nB\nC\nd\n".to_owned(),
            marks: vec![(1, GitLineChange::Added), (2, GitLineChange::Added)],
            removed: vec![1, 2],
            ..GitDiff::default()
        },
    ));
    let desenhado = shell.paint(size);

    let preto = ui_core::Theme::default().colors.ink;
    let setas: Vec<(String, f32)> = desenhado
        .iter()
        .filter_map(|comando| match comando {
            PaintCommand::DrawText(texto)
                if texto.text == "\u{2192}" || texto.text == "\u{21d2}" =>
            {
                assert_eq!(texto.color, preto, "a seta {} sai em preto", texto.text);
                Some((texto.text.clone(), texto.size))
            }
            _ => None,
        })
        .collect();
    assert!(!setas.is_empty(), "há setas a conferir");

    // E maiores que o texto das linhas, que é o que elas precisam vencer.
    let texto_da_linha = desenhado
        .iter()
        .find_map(|comando| match comando {
            PaintCommand::DrawText(texto) if texto.text.trim() == "a" => Some(texto.size),
            _ => None,
        })
        .unwrap_or_default();
    assert!(
        setas.iter().all(|(_, tamanho)| *tamanho > texto_da_linha),
        "as setas são maiores que o código: {setas:?} contra {texto_da_linha}"
    );
}

/// Clicar numa seta não joga a comparação de volta ao topo.
///
/// Devolver uma linha refaz a comparação, e as colunas são remontadas — listas
/// novas nascem no topo. Quem clicou perdia o lugar em que estava lendo, que é
/// justamente onde acabou de mexer. Vale para toda remontagem: o realce também
/// chega depois e refaz as colunas.
#[test]
fn refazer_a_comparacao_do_mesmo_arquivo_guarda_o_lugar() {
    let mut shell = test_shell();
    let size = Size::new(1280.0, 800.0);
    let _ = shell.paint(size);
    let raiz = shell.workspace_root().to_path_buf();
    shell.set_git_view(retrato_com_alteracoes(&raiz));
    shell.toggle_git();
    let _ = shell.paint(size);

    let texto = |marca: &str| {
        (0..200)
            .map(|numero| format!("linha {numero} {marca}"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let comparacao = |marca: &str| GitDiff {
        committed: texto("entao"),
        current: texto(marca),
        marks: vec![(150, GitLineChange::Added)],
        removed: vec![150],
        ..GitDiff::default()
    };
    let arquivo = raiz.join("alterado.java");
    assert!(shell.abrir_comparacao(&arquivo, comparacao("agora")));
    let _ = shell.paint(size);

    // Descer até a alteração, e conferir que se desceu mesmo.
    shell.key_down("F7");
    let _ = shell.paint(size);
    let onde = shell.git_surface().rolagem_do_diff();
    assert!(onde[0] > 0.0, "a comparação rolou até a alteração: {onde:?}");

    // A mesma comparação de novo — é o que chega depois de devolver uma linha.
    assert!(shell.abrir_comparacao(&arquivo, comparacao("depois")));
    let _ = shell.paint(size);
    assert_eq!(
        shell.git_surface().rolagem_do_diff(),
        onde,
        "o mesmo arquivo continua onde estava"
    );

    // Arquivo diferente, porém, começa do topo: é outro assunto.
    assert!(shell.abrir_comparacao(&raiz.join("outro.java"), comparacao("outro")));
    let _ = shell.paint(size);
    assert_eq!(
        shell.git_surface().rolagem_do_diff(),
        [0.0, 0.0],
        "outro arquivo começa do começo"
    );
}

/// As setas ficam fora da trilha da barra de rolagem.
///
/// Encostadas nela, cobriam quatro dos dez pontos da trilha, e ali o clique
/// passava a ser da seta: a barra deixava de se arrastar naquele trecho.
#[test]
fn as_setas_nao_cobrem_a_trilha_da_barra() {
    let mut shell = test_shell();
    let size = Size::new(1280.0, 800.0);
    let _ = shell.paint(size);
    let raiz = shell.workspace_root().to_path_buf();
    shell.set_git_view(retrato_com_alteracoes(&raiz));
    shell.toggle_git();
    let _ = shell.paint(size);

    // Comprido o bastante para haver barra.
    let texto = |marca: &str| {
        (0..200)
            .map(|numero| format!("linha {numero} {marca}"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    assert!(shell.abrir_comparacao(
        &raiz.join("alterado.java"),
        GitDiff {
            committed: texto("entao"),
            current: texto("agora"),
            marks: vec![(0, GitLineChange::Added)],
            removed: vec![0],
            ..GitDiff::default()
        },
    ));
    let _ = shell.paint(size);

    let mut superficie = std::mem::take(&mut shell.git);
    let colunas = superficie.colunas_do_diff_para_teste(&shell.host);
    let setas = superficie.botoes_de_aplicar_para_teste(&shell.host);
    shell.git = superficie;
    assert!(!setas.is_empty(), "há seta a conferir");

    let borda = colunas[0].origin.x + colunas[0].size.width;
    for (_, area) in &setas {
        let direita = area.origin.x + area.size.width;
        assert!(
            direita <= borda - 10.0,
            "a seta acaba antes da trilha: {direita} contra {borda}"
        );
    }
}

/// Nada no cabeçalho da comparação passa por cima de nada.
///
/// Era o que acontecia: "Árvore de trabalho" saía por cima de "sem alterações",
/// porque as três larguras estavam escritas à mão e o texto não cabia nelas.
/// Agora cada um mede o que tem e ocupa o que sobrou do anterior — e o caminho
/// do arquivo, que é o mais comprido, é encurtado com reticências em vez de
/// passar por baixo dos outros.
#[test]
fn o_cabecalho_da_comparacao_nao_sobrepoe_nada() {
    let mut shell = test_shell();
    let size = Size::new(1280.0, 800.0);
    let _ = shell.paint(size);
    let raiz = shell.workspace_root().to_path_buf();
    shell.set_git_view(retrato_com_alteracoes(&raiz));
    shell.toggle_git();
    let _ = shell.paint(size);

    // Um caminho comprido de verdade, que é o caso que quebrava.
    assert!(shell.abrir_comparacao(
        &raiz.join("src/main/java/br/com/besta/server/command/PlayerCommandApplier.java"),
        GitDiff {
            committed: "a\nb\n".to_owned(),
            current: "a\nB\n".to_owned(),
            marks: vec![(1, GitLineChange::Added)],
            removed: vec![1],
            ..GitDiff::default()
        },
    ));
    let desenhado = shell.paint(size);

    // Todo texto do cabeçalho, com onde começa e quanto ocupa.
    let contexto = shell.layout_context();
    let comandos = shell
        .git_surface()
        .comandos_do_cabecalho_para_teste(&shell.host, &contexto);
    let topo_do_cabecalho = comandos
        .first()
        .map(|(_, area)| area.origin.y)
        .unwrap_or_default();
    let mut faixas: Vec<(f32, f32, String)> = desenhado
        .iter()
        .filter_map(|comando| match comando {
            PaintCommand::DrawText(texto)
                // Só a faixa do cabeçalho: as colunas ficam abaixo dela.
                if (texto.origin.y - topo_do_cabecalho).abs() < 16.0 =>
            {
                let largura = texto.text.chars().count() as f32 * texto.size * 0.5;
                Some((texto.origin.x, largura, texto.text.clone()))
            }
            _ => None,
        })
        .collect();
    faixas.sort_by(|a, b| a.0.total_cmp(&b.0));
    assert!(faixas.len() >= 3, "há o caminho, a contagem e os botões: {faixas:?}");

    for par in faixas.windows(2) {
        assert!(
            par[1].0 >= par[0].0 + par[0].1,
            "\"{}\" começa depois de \"{}\" acabar: {faixas:?}",
            par[1].2,
            par[0].2
        );
    }

    // O caminho recebe o que sobrou, e é ele que encurta se não couber — o
    // encurtamento em si depende da medida da fonte, que este teste não tem.
    assert!(
        faixas
            .first()
            .is_some_and(|(x, _, texto)| texto.contains("PlayerCommandApplier")
                && *x < faixas[1].0),
        "o caminho vem antes de tudo, e não por baixo: {faixas:?}"
    );
}

/// O conteúdo da comparação não encosta na moldura do painel.
///
/// Um painel com borda e conteúdo colado nela vira uma caixa em volta do
/// conteúdo: o caminho do arquivo começava no mesmo ponto da linha vertical, e
/// a linha passava a parecer parte do texto — a ponto de alguém perguntar o que
/// ela significava. Não significava nada; faltava respiro. Quanto respirar é o
/// painel quem diz.
#[test]
fn o_conteudo_da_comparacao_nao_encosta_na_moldura() {
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
            committed: "a\nb\n".to_owned(),
            current: "a\nB\n".to_owned(),
            ..GitDiff::default()
        },
    ));
    let desenhado = shell.paint(size);

    // A moldura é o traço mais largo da faixa da comparação.
    let moldura = desenhado
        .iter()
        .filter_map(|comando| match comando {
            PaintCommand::StrokeRect(traco) if traco.rect.size.width > 400.0 => Some(traco.rect),
            _ => None,
        })
        .next_back();
    let Some(moldura) = moldura else {
        panic!("a comparação tem moldura");
    };

    // Nada de texto começa antes da borda esquerda mais o respiro, nem acima
    // da borda de cima mais o respiro.
    let dentro = |x: f32, y: f32| {
        x >= moldura.origin.x + Panel::PADDING
            && y >= moldura.origin.y + Panel::PADDING
            && x <= moldura.origin.x + moldura.size.width - Panel::PADDING
    };
    let fora: Vec<(String, f32, f32)> = desenhado
        .iter()
        .filter_map(|comando| match comando {
            PaintCommand::DrawText(texto)
                if texto.origin.y >= moldura.origin.y
                    && texto.origin.y <= moldura.origin.y + moldura.size.height
                    && !dentro(texto.origin.x, texto.origin.y) =>
            {
                Some((texto.text.clone(), texto.origin.x, texto.origin.y))
            }
            _ => None,
        })
        .collect();
    assert!(
        fora.is_empty(),
        "nada encosta na moldura, e estes encostaram: {fora:?}"
    );
}
