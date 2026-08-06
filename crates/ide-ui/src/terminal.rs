//! Estado de abas, seleção e rolagem do terminal.

use ide_terminal::{ShellKind, TerminalMatch, TerminalSession};
use ui_components::{Console, TerminalView};
use ui_components::{Scrollbar, Splitter};

#[derive(Clone, Copy)]
pub(super) struct TextPosition {
    pub(super) line: usize,
    pub(super) column: usize,
}

#[derive(Clone, Copy)]
pub(super) struct TerminalSelection {
    pub(super) anchor: TextPosition,
    pub(super) focus: TextPosition,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ScrollTarget {
    Editor,
    /// Rolagem lateral do editor: uma linha de código não quebra, e o que passa
    /// da área visível só é alcançável rolando.
    EditorHorizontal,
    Terminal,
    ExplorerHorizontal,
    ExplorerVertical,
}

pub(super) struct TerminalTab {
    pub(super) session: TerminalSession,
    pub(super) scroll_line: usize,
    pub(super) follow_output: bool,
}

/// Estado do painel de terminais e de sua interação.
/// A busca na saída de um terminal.
///
/// Vive no painel, e não junto da busca do editor, porque o que ela guarda é do
/// terminal: as ocorrências vêm em **linha absoluta** do histórico, e é por elas
/// que a rolagem anda.
pub(super) struct BuscaNoTerminal {
    /// O que se está procurando aqui.
    ///
    /// **Próprio, e não compartilhado com o editor.** São duas janelas: quem
    /// procura `erro` na saída pode estar procurando `Pedido` no arquivo, e uma
    /// caixa não pode apagar o texto da outra.
    pub(super) texto: String,
    pub(super) achados: Vec<TerminalMatch>,
    /// Qual das ocorrências está em foco. Sem nenhuma, não há para onde rolar.
    pub(super) atual: Option<usize>,
}

pub(super) struct TerminalPanelState {
    /// A busca em curso na saída, quando há uma.
    pub(super) busca: Option<BuscaNoTerminal>,
    /// A saída, viva entre quadros.
    ///
    /// Não é reconstruída a cada pintura porque duas coisas dependem da medição
    /// que ela guarda e precisam concordar: onde o realce da seleção é desenhado
    /// e em que coluna um clique caiu.
    pub(super) console: Console,
    /// A grade do emulador, desenhada pela biblioteca.
    pub(super) grid: TerminalView,
    pub(super) tabs: Vec<TerminalTab>,
    pub(super) active: usize,
    pub(super) height: f32,
    pub(super) last_height: f32,
    pub(super) minimized: bool,
    pub(super) splitter: Splitter,
    pub(super) scrollbar: Scrollbar,
    pub(super) selection: Option<TerminalSelection>,
    pub(super) selecting: bool,
    pub(super) running_terminal: Option<usize>,
    /// Colunas que os terminais já receberam.
    ///
    /// Guardadas para o tamanho só ser reenviado quando muda de verdade: cada
    /// reenvio faz o programa do outro lado redesenhar, e arrastar o divisor na
    /// vertical não deveria produzir saída nenhuma.
    pub(super) pty_cols: u16,
    /// Linhas já enviadas, pelo mesmo motivo das colunas.
    pub(super) pty_rows: u16,
}

impl TerminalPanelState {
    #[must_use]
    pub(super) fn active_session(&self) -> &TerminalSession {
        &self.tabs[self.active].session
    }

    #[must_use]
    pub(super) fn active(&self) -> &TerminalSession {
        self.active_session()
    }

    pub(super) fn active_session_mut(&mut self) -> &mut TerminalSession {
        &mut self.tabs[self.active].session
    }

    #[must_use]
    pub(super) fn selected_shell(&self) -> ShellKind {
        self.active_session().selected_profile().kind
    }

    pub(super) fn active_lines(&self) -> impl Iterator<Item = &str> {
        self.active_session().lines().map(|line| line.text.as_str())
    }

    pub(super) fn run(&mut self, command: &str) -> Result<(), String> {
        let active = self.active;
        let Some(tab) = self.tabs.get_mut(active) else {
            return Err("Nenhum terminal disponível".to_owned());
        };
        tab.session
            .run(command)
            .map_err(|error| error.to_string())?;
        tab.follow_output = true;
        self.minimized = false;
        self.running_terminal = Some(active);
        Ok(())
    }
}

pub(super) fn ordered_selection(selection: TerminalSelection) -> (TextPosition, TextPosition) {
    if (selection.anchor.line, selection.anchor.column)
        <= (selection.focus.line, selection.focus.column)
    {
        (selection.anchor, selection.focus)
    } else {
        (selection.focus, selection.anchor)
    }
}

/// Uma coordenada que a saída aponta: caminho, linha e coluna.
///
/// Linha e coluna vêm **como estão no texto**, contadas de um — é assim que todo
/// compilador as escreve. Quem abre converte para a contagem interna.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct LinkDaSaida {
    pub(super) caminho: String,
    pub(super) linha: usize,
    pub(super) coluna: usize,
}

/// O link sob uma coluna de uma linha da saída, se houver um ali.
///
/// # Como o padrão é lido
///
/// `Arquivo.java:42:7` é lido **da direita para a esquerda**: os dois últimos
/// campos separados por `:` são números, e o que sobra é o caminho. Ler da
/// esquerda quebraria em `C:\\projetos\\Arquivo.java:42` — a letra da unidade
/// tem dois-pontos, e ela é a primeira coisa que aparece num caminho do Windows.
///
/// A coluna é opcional: `Arquivo.java:42` aponta para o começo da linha.
///
/// # O que não vira link
///
/// Um trecho sem `.` nem barra não é caminho — `versão:1:2` não abriria nada. É
/// heurística, e ela erra para o lado de não oferecer: um link que não abre nada
/// é pior do que texto que não é link.
pub(super) fn link_da_saida(texto: &str, coluna: usize) -> Option<LinkDaSaida> {
    let mut inicio = 0usize;
    for palavra in texto.split_whitespace() {
        // Onde esta palavra começa, em caracteres — `split_whitespace` não diz.
        let deslocamento = texto[inicio..].find(palavra)? + inicio;
        let comeco = texto[..deslocamento].chars().count();
        let fim = comeco + palavra.chars().count();
        inicio = deslocamento + palavra.len();
        if coluna < comeco || coluna >= fim {
            continue;
        }
        return link_do_trecho(palavra);
    }
    None
}

/// A leitura de um trecho isolado, já sem o resto da linha.
fn link_do_trecho(palavra: &str) -> Option<LinkDaSaida> {
    // O rastro de pilha do Java escreve `(Arquivo.java:42)`, e a saída de muita
    // ferramenta termina em vírgula ou ponto.
    let limpo = palavra.trim_matches(|c: char| "()[]{}<>,;'\"".contains(c));
    let partes: Vec<&str> = limpo.rsplitn(3, ':').collect();
    let (caminho, linha, coluna) = match partes.as_slice() {
        [coluna, linha, caminho] => (
            (*caminho).to_owned(),
            linha.parse().ok()?,
            coluna.parse().ok()?,
        ),
        [linha, caminho] => ((*caminho).to_owned(), linha.parse().ok()?, 1usize),
        _ => return None,
    };
    if linha == 0 || coluna == 0 {
        return None;
    }
    // `at br.Pedido.total(Pedido.java:42)` — o rastro de pilha cola o método no
    // arquivo, e tirar só o parêntese do fim deixaria o método dentro do
    // caminho. O arquivo é o que vem **depois** do último parêntese aberto.
    let caminho = caminho
        .rsplit_once('(')
        .map_or(caminho.clone(), |(_, depois)| depois.to_owned());
    let parece_caminho = caminho.contains('.') || caminho.contains('/') || caminho.contains('\\');
    (parece_caminho && !caminho.is_empty()).then_some(LinkDaSaida {
        caminho,
        linha,
        coluna,
    })
}

pub(super) fn selection_columns(
    selection: Option<TerminalSelection>,
    line: usize,
    text: &str,
) -> Option<(usize, usize)> {
    let selection = selection?;
    let (start, end) = ordered_selection(selection);
    if line < start.line || line > end.line {
        return None;
    }
    let length = text.chars().count();
    let from = if line == start.line {
        start.column.min(length)
    } else {
        0
    };
    let to = if line == end.line {
        end.column.min(length)
    } else {
        length
    };
    (to > from).then_some((from, to))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// O que a saída aponta, e o que ela não aponta.
    #[test]
    fn a_saida_aponta_arquivo_linha_e_coluna() {
        let texto = "erro em src/Pedido.java:42:7 — falta ponto e vírgula";
        assert_eq!(
            link_da_saida(texto, 12),
            Some(LinkDaSaida {
                caminho: "src/Pedido.java".to_owned(),
                linha: 42,
                coluna: 7,
            })
        );
        assert!(link_da_saida(texto, 0).is_none());
        assert_eq!(
            link_da_saida("Pedido.java:42", 3).map(|link| (link.linha, link.coluna)),
            Some((42, 1))
        );
        assert_eq!(
            link_da_saida("at br.Pedido.total(Pedido.java:42)", 20).map(|link| link.caminho),
            Some("Pedido.java".to_owned())
        );
        let janela = concat!("C:", "\\", "projetos", "\\", "Pedido.java:42:7");
        assert_eq!(
            link_da_saida(janela, 5).map(|link| (link.linha, link.coluna)),
            Some((42, 7))
        );
        assert!(link_da_saida("versao:1:2", 3).is_none());
        assert!(link_da_saida("Pedido.java:0:1", 3).is_none());
    }
}