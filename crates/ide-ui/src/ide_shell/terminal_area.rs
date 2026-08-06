//! O painel de terminais: abas, rolagem, seleção e o que é enviado.

use super::*;
use ui_core::FontId;

impl IdeShell {
    /// Executa um comando na aba de terminal ativa, como se o usuário digitasse.
    pub fn run_in_terminal(&mut self, command: &str) -> Result<(), String> {
        self.terminal.run(command)
    }

    /// Interrompe a aplicação iniciada pela IDE, como um `Ctrl+C` do usuário.
    pub fn stop_application(&mut self) -> Result<(), String> {
        let Some(index) = self.terminal.running_terminal else {
            return Err("Nenhuma aplicação iniciada pela IDE".to_owned());
        };
        let Some(tab) = self.terminal.tabs.get_mut(index) else {
            self.terminal.running_terminal = None;
            return Err("O terminal da aplicação não existe mais".to_owned());
        };
        tab.session.interrupt().map_err(|error| error.to_string())?;
        tab.follow_output = true;
        self.terminal.active = index;
        self.terminal.running_terminal = None;
        self.context.status_message = "Aplicação interrompida".to_owned();
        Ok(())
    }

    /// Indica que a IDE iniciou uma aplicação e ainda não a interrompeu.
    #[must_use]
    pub const fn application_running(&self) -> bool {
        self.terminal.running_terminal.is_some()
    }

    pub fn append_tool_output(&mut self, text: &str, is_error: bool) {
        let active = self.terminal.active;
        self.terminal.tabs[active]
            .session
            .append_external_output(text, is_error);
        self.terminal.tabs[active].follow_output = true;
        self.terminal.tabs[active].scroll_line = self.terminal.tabs[active].session.line_count();
    }

    pub fn terminal_scroll_line(&self) -> usize {
        self.terminal.tabs[self.terminal.active].scroll_line
    }

    pub const fn active_terminal_index(&self) -> usize {
        self.terminal.active
    }

    pub const fn terminal_height(&self) -> f32 {
        self.terminal.height
    }

    pub const fn terminal_minimized(&self) -> bool {
        self.terminal.minimized
    }

    pub const fn terminal_resizing(&self) -> bool {
        self.terminal.splitter.is_dragging()
    }

    /// Se o ponteiro está sobre o divisor do terminal — ou arrastando-o.
    ///
    /// Minimizado não há divisor: o painel está encostado no rodapé, e não há o
    /// que arrastar. Ver `sidebar_divider_hover`, que responde o mesmo pela
    /// lateral.
    #[must_use]
    pub fn terminal_divider_hover(&self, point: Point, size: Size) -> bool {
        if self.terminal.minimized {
            return false;
        }
        self.terminal_resizing() || self.terminal_splitter_for(size).hit_area().contains(point)
    }

    pub fn active_terminal_lines(&self) -> impl Iterator<Item = &str> {
        self.terminal.active_lines()
    }

    pub fn active_terminal_input(&self) -> &str {
        self.active_terminal().input()
    }

    pub(super) fn active_terminal(&self) -> &TerminalSession {
        self.terminal.active()
    }

    pub(super) fn active_terminal_mut(&mut self) -> &mut TerminalSession {
        self.terminal.active_session_mut()
    }

    pub fn update_terminals(&mut self) -> bool {
        let geo = self.geometry();
        let rows = self.terminal_visible_lines().max(1) as u16;
        // O terminal tem o tamanho do painel, nas duas direções. A largura
        // decide onde o programa quebra a linha; a altura decide quantas linhas
        // a grade tem — e sem reenviá-la, crescer o painel deixava uma faixa
        // vazia embaixo, porque a grade continuava com as linhas de antes.
        let cols = self.terminal_columns(geo.editor_width);
        let mudou = cols != self.terminal.pty_cols || rows != self.terminal.pty_rows;
        if mudou {
            self.terminal.pty_cols = cols;
            self.terminal.pty_rows = rows;
        }
        let mut changed = false;
        for terminal in &mut self.terminal.tabs {
            if mudou {
                let _ = terminal.session.resize(cols, rows);
                // Crescer traz histórico de volta; encolher empurra para o
                // histórico. Nos dois casos o fim é onde o prompt está.
                if terminal.follow_output {
                    terminal.session.scroll_to_bottom();
                    terminal.scroll_line = terminal.session.scrollback_len();
                }
            }
            let received = terminal.session.drain_output();
            changed |= received > 0;
            // Saída nova traz a janela de volta ao fim, que é onde o prompt
            // está — a não ser que alguém tenha rolado para trás de propósito.
            if received > 0 && terminal.follow_output {
                terminal.session.scroll_to_bottom();
                terminal.scroll_line = terminal.session.scrollback_len();
            }
        }
        changed
    }

    /// Quantas colunas cabem na largura do painel.
    ///
    /// A largura do caractere é **medida** na mesma fonte que o console desenha —
    /// estimá-la deixaria o programa quebrando numa coluna e a tela mostrando
    /// noutra. Ver a ADR-021.
    fn terminal_columns(&self, width: f32) -> u16 {
        let caractere = self
            .layout_context()
            .text_width("0", FontId::MONOSPACE, TERMINAL_FONT_SIZE)
            .unwrap_or(TERMINAL_FALLBACK_CHAR_WIDTH)
            .max(1.0);
        // A trilha da barra de rolagem não é área de texto.
        let util = (width - TERMINAL_SCROLLBAR_WIDTH).max(caractere);
        ((util / caractere).floor() as u16).max(1)
    }

    /// Abas do painel de terminal, uma por perfil aberto. Terminais não fecham
    /// pela aba: eles pertencem à janela enquanto ela existir.
    fn terminal_tabs(&self) -> Tabs {
        let items = self
            .terminal
            .tabs
            .iter()
            .enumerate()
            .map(|(index, terminal)| {
                TabItem::new(
                    index as u64,
                    terminal.session.selected_profile().kind.label(),
                )
            })
            .collect();
        let mut tabs = Tabs::new(TERMINAL_TABS_ID, items).with_tab_width(TERMINAL_TAB_WIDTH);
        tabs.set_active(self.terminal.active);
        tabs
    }

    /// Repõe no anfitrião a apresentação das abas do terminal e a área delas.
    pub(super) fn sync_terminal_tabs(&mut self) {
        let tabs = self.terminal_tabs();
        self.host.replace(Box::new(tabs));
    }

    pub(super) fn terminal_splitter_for(&self, size: Size) -> Splitter {
        let geometry = self.geometry();
        let editor_x = ACTIVITY_WIDTH + self.sidebar_width(size);
        let maximum =
            (geometry.content_bottom - geometry.content_top - 100.0).max(TERMINAL_MIN_HEIGHT);
        let mut splitter = self.terminal.splitter.clone();
        splitter.layout(
            &self.layout_context(),
            Rect::new(
                editor_x,
                geometry.content_top,
                (size.width - editor_x).max(0.0),
                (geometry.content_bottom - geometry.content_top).max(0.0),
            ),
        );
        splitter.set_range(
            geometry.content_bottom - maximum,
            geometry.content_bottom - TERMINAL_MIN_HEIGHT,
        );
        splitter.set_position(geometry.editor_bottom);
        splitter
    }

    /// Quantas linhas da saída cabem — a altura da faixa, e não uma subtração.
    /// Leva a janela visível do terminal a uma posição do histórico.
    ///
    /// O emulador só rola por deslocamento — não há como dizer "vá para a linha
    /// 12". Então a IDE guarda onde está e converte em passos, e este é o
    /// **único** lugar que faz isso: espelho em dois lugares seria espelho que
    /// diverge.
    pub(super) fn set_terminal_scroll(&mut self, alvo: usize) {
        let active = self.terminal.active;
        let maximo = self.terminal.tabs[active].session.scrollback_len();
        let alvo = alvo.min(maximo);
        let atual = self.terminal.tabs[active].scroll_line;
        let passos = alvo as isize - atual as isize;
        self.terminal.tabs[active].session.scroll_lines(passos);
        self.terminal.tabs[active].scroll_line = alvo;
        // No fim do histórico, a saída seguinte traz a janela junto.
        self.terminal.tabs[active].follow_output = alvo >= maximo;
    }

    pub(super) fn terminal_visible_lines(&self) -> usize {
        let (saida, _) = self.terminal_bands();
        ((saida.size.height / EDITOR_LINE_HEIGHT).floor()).max(1.0) as usize
    }

    pub(super) fn terminal_scrollbar_rect(&self, _size: Size) -> Rect {
        // A trilha acompanha a saída: é o que ela rola.
        let (saida, _) = self.terminal_bands();
        Rect::new(
            saida.origin.x + saida.size.width - TERMINAL_SCROLLBAR_WIDTH,
            saida.origin.y,
            TERMINAL_SCROLLBAR_WIDTH,
            saida.size.height,
        )
    }

    /// Linha e coluna do texto sob um ponto do terminal.
    ///
    /// O tamanho da janela não entra mais na conta: quem sabe onde a saída está e
    /// quanto mede um caractere é o console, desde a última pintura.
    pub(super) fn terminal_position_at(&self, point: Point, _size: Size) -> TextPosition {
        let active = &self.terminal.tabs[self.terminal.active];
        // Quem responde a coluna é o console, pela mesma medição com que
        // desenhou: estimar a largura do caractere aqui punha o clique numa
        // coluna e o realce noutra.
        let (linha, column) = self.terminal.console.position_at(point);
        let line = linha.min(active.session.line_count().saturating_sub(1));
        let line_length = active
            .session
            .lines()
            .nth(line)
            .map_or(0, |value| value.text.chars().count());
        TextPosition {
            line,
            column: column.min(line_length),
        }
    }

    /// O texto marcado, **lido da grade** — a mesma fonte que a tela desenha.
    ///
    /// # Por que isto importa
    ///
    /// Antes daqui ele era lido de `lines()`, uma lista de linhas acumulada à
    /// parte, enquanto o que aparece vem de `grid_rows()`, a viewport do
    /// emulador. Duas fontes para a mesma tela: quando a grade quebra linha,
    /// preenche ou rola, elas divergem — e quem marcava uma coisa colava outra.
    ///
    /// Pior, os números da seleção são **da viewport**: a linha 0 é a primeira
    /// que se vê, e não a primeira que já saiu. Lidos contra a lista acumulada,
    /// eles apontavam para o começo do histórico.
    ///
    /// A pergunta "que colunas desta linha estão marcadas" é a mesma do desenho,
    /// e agora é a mesma função que responde nos dois lugares. É o que impede o
    /// marcado e o copiado de discordarem de novo.
    ///
    /// # Os espaços do fim
    ///
    /// A grade é retangular: toda linha tem a largura inteira, preenchida com
    /// espaço. Marcar até o fim traria essa cauda junto, e colá-la num editor
    /// deixaria espaços que ninguém digitou. Ela sai — só quando a marca alcança
    /// o fim da linha, porque aí os espaços são preenchimento, e não conteúdo.
    /// Marca um trecho da grade, para o teste não depender de arrastar o mouse.
    #[cfg(test)]
    pub(super) fn set_terminal_selection_for_test(
        &mut self,
        anchor: TextPosition,
        focus: TextPosition,
    ) {
        self.terminal.selection = Some(TerminalSelection { anchor, focus });
    }

    pub fn selected_terminal_text(&self) -> String {
        if self.terminal.selection.is_none() {
            return String::new();
        }
        self.active_terminal()
            .grid_rows()
            .iter()
            .enumerate()
            .filter_map(|(numero, linha)| {
                let texto: String = linha.iter().map(|celula| celula.character).collect();
                let (inicio, fim) = selection_columns(self.terminal.selection, numero, &texto)?;
                let trecho: String = texto.chars().skip(inicio).take(fim - inicio).collect();
                let ate_o_fim = fim >= texto.chars().count();
                Some(if ate_o_fim {
                    trecho.trim_end().to_owned()
                } else {
                    trecho
                })
            })
            .collect::<Vec<_>>()
            .join("
")
    }

    /// O botão que recolhe e restaura o painel de terminais.
    /// Refaz a busca na saída do terminal ativo.
    ///
    /// A varredura passeia o histórico e volta — ver `find_all`. Ela roda a cada
    /// tecla, como a do editor, e o custo é proporcional ao histórico: cinco mil
    /// linhas de oitenta colunas. **Não foi medido**, e é a primeira coisa a
    /// olhar se digitar aqui parecer pesado.
    pub(super) fn refresh_terminal_search(&mut self) {
        let procurado = self.editor_area.search_query.clone();
        if !self.editor_area.search_open || procurado.trim().is_empty() {
            self.terminal.busca = None;
            return;
        }
        let ativo = self.terminal.active;
        let onde_estava = self.terminal.tabs[ativo].scroll_line;
        let achados = self.terminal.tabs[ativo]
            .session
            .find_all(&procurado, onde_estava);
        // A primeira ocorrência já nasce em foco: quem procurou quer ver alguma,
        // e exigir um `Enter` para a primeira seria pedir duas vezes.
        let atual = (!achados.is_empty()).then_some(0);
        self.terminal.busca = Some(BuscaNoTerminal { achados, atual });
        self.reveal_terminal_match();
    }

    /// Anda entre as ocorrências da saída, dando a volta nas duas pontas.
    ///
    /// `adiante` falso é o `Shift+Enter`: o mesmo gesto ao contrário, como na
    /// busca do editor. A volta existe pelo mesmo motivo lá e aqui — sem ela,
    /// quem procurou algo que só está atrás fica preso.
    pub(super) fn step_terminal_search(&mut self, adiante: bool) {
        let Some(busca) = self.terminal.busca.as_mut() else {
            return;
        };
        let total = busca.achados.len();
        if total == 0 {
            return;
        }
        busca.atual = Some(match busca.atual {
            Some(atual) if adiante => (atual + 1) % total,
            Some(atual) => (atual + total - 1) % total,
            None => 0,
        });
        self.reveal_terminal_match();
    }

    /// Rola a saída até a ocorrência em foco.
    ///
    /// A linha das ocorrências é **absoluta**, e a rolagem entende o mesmo
    /// número — é por isso que a varredura devolve assim. Uma linha já visível
    /// não move nada: rolar até o que já se vê tira a vista do lugar sem
    /// motivo.
    fn reveal_terminal_match(&mut self) {
        let Some(busca) = self.terminal.busca.as_ref() else {
            return;
        };
        let Some(achado) = busca.atual.and_then(|atual| busca.achados.get(atual)) else {
            return;
        };
        let linha = achado.line;
        let ativo = self.terminal.active;
        let topo = self.terminal.tabs[ativo].scroll_line;
        let altura = self.terminal_visible_lines();
        if linha >= topo && linha < topo + altura {
            return;
        }
        // A ocorrência no meio da tela, e não colada no topo: ver o que veio
        // antes dela é metade do motivo de procurar.
        let alvo = linha.saturating_sub(altura / 2);
        self.set_terminal_scroll(alvo);
    }

    /// O arquivo que a saída aponta sob este ponto, se houver um ali.
    ///
    /// **A célula vem da grade**, pela mesma medição com que ela foi desenhada:
    /// perguntar a largura do caractere aqui poria o clique numa coluna e o
    /// texto em outra.
    ///
    /// O caminho relativo é resolvido contra a raiz do projeto, que é onde o
    /// terminal nasce. Um caminho que não existe **não vira link**: oferecer um
    /// clique que abre nada é pior do que não oferecer.
    pub(super) fn terminal_link_at(&self, point: Point) -> Option<(PathBuf, usize, usize)> {
        let (linha, coluna) = self.terminal.grid.position_at(point);
        let texto: String = self
            .active_terminal()
            .grid_rows()
            .get(linha)?
            .iter()
            .map(|celula| celula.character)
            .collect();
        let link = link_da_saida(&texto, coluna)?;
        let caminho = PathBuf::from(&link.caminho);
        let absoluto = if caminho.is_absolute() {
            caminho
        } else {
            self.workspace_path().join(caminho)
        };
        // Contadas de um no texto do compilador, de zero aqui dentro.
        absoluto
            .is_file()
            .then(|| (absoluto, link.linha - 1, link.coluna - 1))
    }

    /// Onde fica o botão de recolher e mostrar o terminal.
    ///
    /// Fora do tratador porque quem roteia o clique precisa da mesma área: com
    /// a conta em dois lugares, o roteador manda o gesto para um lugar e o
    /// desenho põe o botão em outro.
    pub(super) fn terminal_toggle_rect(&self, size: Size) -> Rect {
        Rect::new(size.width - 30.0, self.geometry().editor_bottom + 4.0, 22.0, 22.0)
    }

    pub(super) fn terminal_toggle_pointer_down(&mut self, point: Point, size: Size) -> bool {
        if !self.terminal_toggle_rect(size).contains(point) {
            return false;
        }
        if self.terminal.minimized {
            self.terminal.minimized = false;
            self.terminal.height = self.terminal.last_height;
        } else {
            self.terminal.last_height = self.terminal.height;
            self.terminal.minimized = true;
        }
        true
    }

    /// Clique no painel de terminais: a aba escolhida, ou o começo de uma marca.
    pub(super) fn terminal_area_pointer_down(&mut self, point: Point, size: Size) {
        let geometry = self.geometry();
        let editor_x = ACTIVITY_WIDTH + self.sidebar_width(size);
        if point.x < editor_x || point.y < geometry.editor_bottom {
            return;
        }
        // Um caminho com linha e coluna na saída é uma coordenada, e a IDE sabe
        // abrir coordenadas. O clique nele abre o arquivo em vez de começar uma
        // seleção — quem clica num link não está marcando texto.
        if let Some((caminho, linha, coluna)) = self.terminal_link_at(point) {
            self.commands
                .push(ApplicationCommand::OpenDocument(
                    OpenDocumentRequest::new(caminho).at(linha, coluna),
                ));
            return;
        }
        self.context.focus = ShellFocus::Terminal;
        if point.y < geometry.editor_bottom + TERMINAL_TAB_HEIGHT {
            self.place_overlay(size);
            if let Some(WidgetAction::TabSelected { tab, .. }) = tab_action(&mut self.host, point) {
                self.terminal.active = tab as usize;
                self.context.status_message = format!(
                    "Terminal: {}",
                    self.active_terminal().selected_profile().kind.label()
                );
            }
        } else if point.y >= self.terminal_bands().0.origin.y {
            let position = self.terminal_position_at(point, size);
            self.terminal.selection = Some(TerminalSelection {
                anchor: position,
                focus: position,
            });
            self.terminal.selecting = true;
        }
    }
}
