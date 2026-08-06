//! Quem responde por cada ponto da janela.
//!
//! # Por que isto existe
//!
//! O roteamento do ponteiro era uma fila de treze `if tratador(ponto) { return }`,
//! e **cada tratador respondia duas perguntas de uma vez**: "o ponto é meu?" e
//! "então faça". Quem devolvia `false` estava dizendo "não era meu" — depois de
//! já ter executado o que quis.
//!
//! Foi assim que o clique no Explorer roubou o painel que ia receber o arquivo:
//! o tratador da área dividida recebia **todo** clique da janela e tratava "não
//! caiu na direita" como "caiu na esquerda". O erro não foi de distração; a forma
//! convidava a ele, porque decidir a região e agir estavam no mesmo lugar.
//!
//! Aqui a decisão é separada da ação: um ponto resolve para **um** alvo, e só o
//! tratador daquele alvo é chamado. Um tratador nunca vê um clique que não é
//! dele, e a classe inteira daquele defeito deixa de ser possível.
//!
//! # O que não está aqui
//!
//! O que cobre a janela inteira e depende de **estado**, e não de região: o menu
//! de contexto aberto, a lista de completação, as janelas sobrepostas. Elas vêm
//! antes de qualquer pergunta sobre região, e é correto que venham — quando
//! estão abertas, o ponto é delas onde quer que esteja.

use super::*;

/// A área da janela que responde por um ponto.
///
/// A ordem das perguntas em [`IdeShell::alvo_do_ponto`] é a precedência entre
/// elas, e ela é **dado**, não sequência de código: quem sobrepõe quem está
/// escrito num lugar só, e pode ser afirmado por teste sem simular gesto nenhum.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Alvo {
    /// A barra do topo: menus e os botões de ação.
    Topo,
    /// A faixa estreita da esquerda, com os dois botões.
    Atividades,
    /// O botão que recolhe e mostra o terminal.
    RecolherTerminal,
    /// Uma das trilhas de rolagem.
    Barra(ScrollTarget),
    /// Um dos divisores arrastáveis da moldura.
    Divisor,
    /// A divisa entre os dois editores.
    DivisaDosEditores,
    /// A faixa de abas — de qualquer um dos lados.
    Abas,
    /// O editor da direita, quando a área está dividida.
    EditorDaDireita,
    /// A árvore do projeto.
    Explorer,
    /// O painel de depuração, à direita.
    Depuracao,
    /// O texto do editor da frente.
    Editor,
    /// O painel de terminais.
    Terminal,
    /// Nenhuma área reclama o ponto.
    Nenhum,
}

impl IdeShell {
    /// Qual área responde por este ponto.
    ///
    /// As perguntas descem da que cobre as outras para a que fica por baixo. Os
    /// retângulos vêm das mesmas funções que o desenho usa: perguntar aqui por
    /// uma conta própria seria repetir a geometria, e duas contas divergem.
    pub(super) fn alvo_do_ponto(&self, point: Point, size: Size) -> Alvo {
        if point.y < TITLE_HEIGHT {
            return Alvo::Topo;
        }
        if point.x < ACTIVITY_WIDTH {
            return Alvo::Atividades;
        }
        if self.terminal_toggle_rect(size).contains(point) {
            return Alvo::RecolherTerminal;
        }
        if let Some(alvo) = self.barra_em(point, size) {
            return Alvo::Barra(alvo);
        }
        if self.divisor_em(point, size) {
            return Alvo::Divisor;
        }
        if self
            .split_panel_for(size)
            .is_some_and(|painel| painel.divider().contains(point))
        {
            return Alvo::DivisaDosEditores;
        }
        let geometry = self.geometry();
        let editor_x = ACTIVITY_WIDTH + self.sidebar_width(size);
        if point.y < TITLE_HEIGHT + TAB_HEIGHT {
            // A faixa de abas dos dois lados: qual delas é, quem trata resolve
            // pela coluna, e é ele quem tem a lista de cada uma.
            return if point.x >= editor_x {
                Alvo::Abas
            } else {
                Alvo::Explorer
            };
        }
        if self
            .right_editor_rect(size)
            .is_some_and(|direita| direita.contains(point))
        {
            return Alvo::EditorDaDireita;
        }
        if point.x < editor_x {
            return if point.y >= EXPLORER_TOP {
                Alvo::Explorer
            } else {
                Alvo::Nenhum
            };
        }
        if self.debug_panel.view.attached
            && point.x >= editor_x + geometry.editor_width
            && point.y >= geometry.content_top
            && point.y < geometry.editor_bottom
        {
            return Alvo::Depuracao;
        }
        if point.y < geometry.editor_bottom {
            return Alvo::Editor;
        }
        Alvo::Terminal
    }

    /// Em qual trilha de rolagem o ponto caiu, se caiu em alguma.
    ///
    /// A lista e as condições são as mesmas de antes: a trilha do terminal não
    /// existe com ele recolhido, e a lateral do editor só existe quando há linha
    /// passando da área — sem isso ela tomaria o clique da borda do terminal,
    /// que fica na mesma altura, sem sequer estar desenhada.
    fn barra_em(&self, point: Point, size: Size) -> Option<ScrollTarget> {
        [
            ScrollTarget::Terminal,
            ScrollTarget::Editor,
            ScrollTarget::EditorHorizontal,
            ScrollTarget::ExplorerHorizontal,
            ScrollTarget::ExplorerVertical,
        ]
        .into_iter()
        .find(|target| {
            if *target == ScrollTarget::Terminal && self.terminal.minimized {
                return false;
            }
            if *target == ScrollTarget::EditorHorizontal && !self.editor_scrolls_sideways(size) {
                return false;
            }
            if matches!(
                target,
                ScrollTarget::ExplorerHorizontal | ScrollTarget::ExplorerVertical
            ) && self.sidebar_collapsed()
            {
                return false;
            }
            self.scrollbar_range(*target, size).0.contains(point)
        })
    }

    /// Se o ponto caiu na área de arrasto de um dos divisores da moldura.
    fn divisor_em(&self, point: Point, size: Size) -> bool {
        let lateral = !self.sidebar_collapsed()
            && self.sidebar_splitter_for(size).hit_area().contains(point);
        let terminal =
            !self.terminal.minimized && self.terminal_splitter_for(size).hit_area().contains(point);
        lateral || terminal
    }
}
