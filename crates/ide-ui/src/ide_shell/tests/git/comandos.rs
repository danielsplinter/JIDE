//! Os comandos da comparação: andar entre as alterações, trocar de lado,
//! devolver um trecho inteiro, e o que a tela recusa comparar.

use ide_application::RestoreTarget;
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

    let botao = shell
        .git_surface()
        .comandos_do_cabecalho_para_teste(&shell.host)[0];
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
            ApplicationCommand::Git(GitRequest::RestoreBlock { path, from, target })
                if path == &arquivo && *from == (1, 2) && *target == RestoreTarget::Replace(1)
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
