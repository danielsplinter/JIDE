//! O editor de arquivos: painel, abas, barras de rolagem e completação.
//!
//! O que o painel de edição sabe fazer é dele; aqui fica o que o shell precisa
//! decidir em volta — onde ele está, o que a lista mostra, e o que a tecla que
//! ele não tratou significa.

use super::*;
use crate::editor::Posicao;

/// Quantas letras de um nome abrem a lista de completação sozinhas.
///
/// Uma letra abriria com o alfabeto inteiro dentro; duas já é um nome
/// começando, e é onde o VS Code também abre.
const LETRAS_QUE_ABREM: usize = 2;

impl IdeShell {
    pub const fn focus(&self) -> ShellFocus {
        self.context.focus
    }

    pub fn completion_request(&self) -> Option<CompletionRequest> {
        let document = self.editor_area.session.active()?;
        let (line, column) = line_column(document.buffer.text(), self.editor_area.pane.cursor());
        Some(CompletionRequest {
            document_id: document.id,
            position: DomainTextPosition {
                line: line as u32,
                column: column as u32,
            },
            prefix: identifier_prefix(document.buffer.text(), self.editor_area.pane.cursor()),
        })
    }

    /// Se o nome sendo escrito **acabou de chegar** ao tamanho que abre a lista.
    ///
    /// # Por que duas letras, e por que "acabou de chegar"
    ///
    /// Até aqui só o ponto abria a completação, e escrever um nome exigia
    /// `Ctrl+Espaço` — que ninguém lembra de apertar. Uma letra abriria a lista
    /// com o alfabeto inteiro dentro; duas já é um nome começando.
    ///
    /// **E abre uma vez, e não a cada tecla.** A partir daí quem mantém a lista
    /// viva é `completion_follow_up`, que refaz o filtro a cada letra. Se
    /// abrisse sempre que houvesse duas letras ou mais, `Escape` deixaria de
    /// funcionar: a lista voltaria na tecla seguinte.
    #[must_use]
    pub fn completion_opens_now(&self) -> bool {
        let Some(document) = self.editor_area.session.active() else {
            return false;
        };
        identifier_prefix(document.buffer.text(), self.editor_area.pane.cursor())
            .chars()
            .count()
            == LETRAS_QUE_ABREM
    }

    pub fn set_completions(&mut self, items: Vec<CompletionItem>) {
        self.editor_area.completion_items = items;
        self.editor_area.completion_selected = 0;
    }

    /// A lista de completação está à mostra.
    #[must_use]
    pub fn completion_open(&self) -> bool {
        !self.editor_area.completion_items.is_empty()
    }

    pub fn clear_completions(&mut self) {
        self.editor_area.completion_items.clear();
        self.editor_area.completion_selected = 0;
    }

    /// O que o texto recém-digitado faz com a lista já aberta.
    ///
    /// Devolve `true` quando a lista precisa ser pedida de novo: cada letra
    /// digitada encurta o que ainda serve, e é isso que faz a lista **acompanhar
    /// o nome sendo escrito** em vez de congelar no que valia quando ela abriu.
    ///
    /// Um caractere que não faz parte de um nome encerra o nome, e a lista sai.
    /// Chamar com a lista fechada não abre nada: abrir é papel do caractere de
    /// disparo da linguagem, ou do `Ctrl+Space`.
    pub fn completion_follow_up(&mut self, typed: &str) -> bool {
        if !self.completion_open() {
            return false;
        }
        if !typed.is_empty() && typed.chars().all(is_identifier_character) {
            return true;
        }
        self.clear_completions();
        false
    }

    pub fn navigation_hover(&self, point: Point, size: Size, control: bool) -> bool {
        if !control {
            return false;
        }
        let geometry = self.geometry();
        let editor_x = ACTIVITY_WIDTH + self.sidebar_width(size);
        if point.x < editor_x + EDITOR_GUTTER
            || point.y < geometry.content_top
            || point.y >= geometry.editor_bottom
        {
            return false;
        }
        let Some(document) = self.editor_area.session.active() else {
            return false;
        };
        let mut pane = self.editor_area.pane.clone();
        pane.set_bounds(Rect::new(
            editor_x,
            geometry.content_top,
            geometry.editor_width,
            geometry.editor_height,
        ));
        let offset = pane.offset_at_point(&document.buffer, point);
        if token_at(document.buffer.text(), offset).is_none() {
            return false;
        }
        let (line, column) = line_column(document.buffer.text(), offset);
        self.editor_area
            .syntax_snapshots
            .get(&document.id)
            .filter(|snapshot| snapshot.version == document.buffer.revision())
            .is_some_and(|snapshot| {
                snapshot.highlights.iter().any(|highlight| {
                    is_navigable(highlight.kind) && position_in_range(line, column, highlight.range)
                })
            })
    }

    pub fn selected_shell(&self) -> ShellKind {
        self.terminal.selected_shell()
    }

    pub fn editor_scroll_line(&self) -> usize {
        self.editor_area.pane.scroll_line()
    }

    /// Abas do editor montadas a partir dos documentos abertos.
    ///
    /// A apresentação é reconstruída a cada quadro porque a verdade são os
    /// documentos, e não uma cópia deles: assim nenhuma abertura, gravação ou
    /// fechamento precisa lembrar de sincronizar a barra de abas. Quem recebe a
    /// reconstrução é [`UiHost::replace`], que carrega o estado de interação de
    /// uma instância para a outra — é por isso que a aba sob o ponteiro continua
    /// destacada entre um quadro e o seguinte.
    fn editor_tabs(&self) -> Tabs {
        let items = self
            .editor_area
            .session
            .tabs()
            .map(|document| {
                let title = document
                    .path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("?");
                TabItem::new(document.id.0, title)
                    .closable()
                    .modified(document.buffer.is_dirty())
            })
            .collect();
        let mut tabs = Tabs::new(EDITOR_TABS_ID, items).with_tab_width(TAB_WIDTH);
        if let Some(active) = self.editor_area.session.active_id() {
            tabs.set_active_id(active.0);
        }
        tabs
    }

    /// Repõe no anfitrião a apresentação das abas e a área delas.
    ///
    /// Substituir, e não construir: o destaque sob o ponteiro e a pressão em
    /// curso atravessam a troca, e a IDE deixa de precisar informar à mão onde o
    /// ponteiro está.
    pub(super) fn sync_editor_tabs(&mut self) {
        let tabs = self.editor_tabs();
        self.host.replace(Box::new(tabs));
    }

    /// Trilha, faixa e deslocamento de uma barra, na unidade do seu conteúdo.
    ///
    /// As barras verticais contam linhas e a horizontal conta pontos de largura:
    /// para o componente é a mesma aritmética, e é por isso que uma só barra
    /// serve às quatro áreas.
    pub(super) fn scrollbar_range(
        &self,
        target: ScrollTarget,
        size: Size,
    ) -> (Rect, f32, f32, f32) {
        match target {
            ScrollTarget::Editor => (
                self.editor_scrollbar_rect(size),
                self.active_text().map_or(0, |text| text.lines().count()) as f32,
                self.editor_visible_lines() as f32,
                self.editor_area.pane.scroll_offset() / EDITOR_LINE_HEIGHT,
            ),
            ScrollTarget::Terminal => {
                let active = &self.terminal.tabs[self.terminal.active];
                // A trilha percorre o histórico do emulador, que é o que a
                // rolagem alcança.
                (
                    self.terminal_scrollbar_rect(size),
                    (active.session.scrollback_len() + self.terminal_visible_lines()) as f32,
                    self.terminal_visible_lines() as f32,
                    active.scroll_line as f32,
                )
            }
            ScrollTarget::ExplorerVertical => (
                self.explorer_vertical_scrollbar_rect(size),
                self.visible_entries().len() as f32,
                self.explorer_visible_lines() as f32,
                self.explorer.scroll_line as f32,
            ),
            ScrollTarget::EditorHorizontal => {
                let track = self.editor_horizontal_scrollbar_rect(size);
                (
                    track,
                    self.editor_area.pane.content_width(),
                    (track.size.width - 28.0).max(1.0),
                    self.editor_area.pane.scroll_x(),
                )
            }
            ScrollTarget::ExplorerHorizontal => {
                let track = self.explorer_horizontal_scrollbar_rect(size);
                (
                    track,
                    self.explorer_content_width(size),
                    (track.size.width - 28.0).max(1.0),
                    self.explorer.scroll_x,
                )
            }
        }
    }

    /// Onde o cursor está, em linha e coluna.
    ///
    /// É o que a linguagem precisa para saber de qual tipo se está falando.
    #[must_use]
    pub fn cursor_position(&self) -> Option<DomainTextPosition> {
        let text = self.active_text()?;
        let (line, column) = line_column(text, self.editor_area.pane.cursor());
        Some(DomainTextPosition {
            line: line as u32,
            column: column as u32,
        })
    }

    /// Reescreve um documento aberto, mantendo a aba e o desfazer.
    pub(super) fn rewrite_open_document(
        &mut self,
        document_id: DocumentId,
        ranges: &[DomainTextRange],
        antigo: &str,
        novo: &str,
    ) {
        let Some(document) = self.editor_area.session.document_mut(document_id) else {
            return;
        };
        let texto = rewrite_occurrences(document.buffer.text(), ranges, antigo, novo);
        let total = document.buffer.text().len();
        let _ = document.buffer.replace(0..total, &texto);
    }

    pub(super) fn cursor_inside_type(&mut self, point: Point, size: Size) -> bool {
        self.place_focused_editor(size);
        let Some(buffer) = self
            .editor_area
            .session
            .active()
            .map(|document| &document.buffer)
        else {
            return false;
        };
        let offset = self.editor_area.pane.offset_at_point(buffer, point);
        let (line, column) = line_column(buffer.text(), offset);
        let posicao = DomainTextPosition {
            line: line as u32,
            column: column as u32,
        };
        self.active_outline()
            .iter()
            .any(|item| encloses_type(item, posicao))
    }

    /// Há linha passando da área visível, e portanto barra lateral.
    pub(super) fn editor_scrolls_sideways(&self, size: Size) -> bool {
        self.editor_area.pane.content_width() > self.editor_view_rect(size).size.width
    }

    /// Trilha da barra lateral do editor, rente à borda de baixo da área.
    ///
    /// Ela para antes da barra vertical: duas trilhas cruzadas no canto
    /// disputariam o mesmo clique.
    pub(super) fn editor_horizontal_scrollbar_rect(&self, size: Size) -> Rect {
        let geo = self.geometry();
        let editor_x = ACTIVITY_WIDTH + self.sidebar_width(size);
        Rect::new(
            editor_x,
            geo.editor_bottom - 10.0,
            (geo.editor_width - 10.0).max(0.0),
            10.0,
        )
    }

    pub(super) fn scrollbar_mut(&mut self, target: ScrollTarget) -> &mut Scrollbar {
        match target {
            ScrollTarget::Editor => &mut self.editor_area.scrollbar,
            ScrollTarget::Terminal => &mut self.terminal.scrollbar,
            ScrollTarget::ExplorerVertical => &mut self.explorer.vertical_scrollbar,
            ScrollTarget::EditorHorizontal => &mut self.editor_area.horizontal_scrollbar,
            ScrollTarget::ExplorerHorizontal => &mut self.explorer.horizontal_scrollbar,
        }
    }

    /// Alinha a barra ao conteúdo atual antes de entregar-lhe um evento.
    pub(super) fn sync_scrollbar(&mut self, target: ScrollTarget, size: Size) {
        let (track, content, viewport, offset) = self.scrollbar_range(target, size);
        let context = self.layout_context();
        let bar = self.scrollbar_mut(target);
        bar.layout(&context, track);
        bar.set_range(content, viewport);
        bar.set_offset(offset);
    }

    /// Traz de volta o deslocamento que a barra escolheu.
    pub(super) fn apply_scrollbar(&mut self, target: ScrollTarget) {
        let offset = self.scrollbar_mut(target).offset();
        match target {
            // A barra também fala em pixels: arredondar para linha aqui
            // devolveria o salto que a rolagem contínua veio tirar.
            ScrollTarget::Editor => self
                .editor_area
                .pane
                .set_scroll_offset((offset * EDITOR_LINE_HEIGHT).max(0.0)),
            ScrollTarget::Terminal => self.set_terminal_scroll(offset.round().max(0.0) as usize),
            ScrollTarget::ExplorerVertical => {
                self.explorer.scroll_line = offset.round().max(0.0) as usize;
            }
            ScrollTarget::EditorHorizontal => self.editor_area.pane.set_scroll_x(offset.max(0.0)),
            ScrollTarget::ExplorerHorizontal => self.explorer.scroll_x = offset.max(0.0),
        }
    }

    /// Entrega o clique à barra cuja trilha o contém.
    pub(super) fn scrollbar_pointer_down(&mut self, point: Point, size: Size) -> bool {
        for target in [
            ScrollTarget::Terminal,
            ScrollTarget::Editor,
            ScrollTarget::EditorHorizontal,
            ScrollTarget::ExplorerHorizontal,
            ScrollTarget::ExplorerVertical,
        ] {
            if target == ScrollTarget::Terminal && self.terminal.minimized {
                continue;
            }
            // A barra lateral do editor só existe quando há linha passando da
            // área. Sem esta guarda ela tomaria o clique da borda do terminal,
            // que fica na mesma altura, sem sequer estar desenhada.
            if target == ScrollTarget::EditorHorizontal && !self.editor_scrolls_sideways(size) {
                continue;
            }
            let (track, ..) = self.scrollbar_range(target, size);
            if !track.contains(point) {
                continue;
            }
            self.sync_scrollbar(target, size);
            let handled = self.scrollbar_mut(target).event(
                &mut EventContext::default(),
                &UiEvent::PointerDown(primary_pointer(point)),
            );
            if matches!(handled, EventResult::Handled) {
                self.apply_scrollbar(target);
                self.context.scrollbar_drag = Some(target);
            }
            // A trilha consome o clique mesmo sem indicador: ali não há
            // conteúdo para reagir embaixo.
            return true;
        }
        false
    }

    pub(super) fn editor_view_rect(&self, size: Size) -> Rect {
        // Dividida, a esquerda ocupa a coluna que o componente deu a ela. A
        // conta da largura e da divisa e dele: refaze-la aqui poria o clique num
        // painel e o desenho no outro.
        if let Some(esquerda) = self.left_editor_rect(size) {
            return esquerda;
        }
        let geometry = self.geometry();
        Rect::new(
            ACTIVITY_WIDTH + self.sidebar_width(size),
            geometry.content_top,
            geometry.editor_width,
            geometry.editor_height,
        )
    }

    /// Alinha o painel de edição ao documento, ao realce e às decorações.
    ///
    /// O texto vive no documento; o painel guarda cursor, seleção, rolagem e a
    /// cópia de desenho, e só refaz esta última quando a revisão muda.
    pub(super) fn sync_editor_pane(&mut self, size: Size) {
        let Some(document) = self.editor_area.session.active() else {
            return;
        };
        let (id, revision, path) = (
            document.id,
            document.buffer.revision(),
            document.path.clone(),
        );
        let decorations = self.editor_decorations(&path);
        let focused = self.context.focus == ShellFocus::Editor;
        let bounds = self.editor_view_rect(size);
        let context = self.layout_context();
        let editor_area = &mut self.editor_area;
        editor_area.pane.set_bounds(bounds);
        // Qual documento o painel edita: trocar de aba precisa jogar fora a cópia
        // de desenho, o desfazer e as marcas, que falam do texto anterior.
        editor_area.pane.set_source(id.0);
        let Some(document) = editor_area.session.active() else {
            return;
        };
        let syntax = editor_area
            .syntax_spans
            .get(&id)
            .filter(|cached| cached.version == revision)
            .map(|cached| SyntaxView {
                version: cached.version,
                spans: &cached.spans,
            });
        editor_area
            .pane
            .sync(&context, &document.buffer, syntax, decorations, focused);
    }

    /// `Ctrl+F`: abre a barra de busca do arquivo aberto, ou a fecha.
    ///
    /// Fechar apaga o que se procurava **e** as marcas: destaque que sobrevive à
    /// janela vira sujeira na tela, sem nada que diga de onde veio.
    pub fn toggle_search(&mut self) {
        if !self.editor_area.search_open {
            self.editor_area.search_open = true;
            self.context.focus = ShellFocus::Search;
        } else if self.context.focus == ShellFocus::Search {
            // Aberta e com o foco: `Ctrl+F` de novo fecha.
            self.close_search();
        } else {
            // Aberta e sem o foco — porque houve um clique no editor. Aqui
            // `Ctrl+F` devolve o foco em vez de fechar: fechar puniria quem
            // clicou no código e voltou para continuar procurando.
            self.context.focus = ShellFocus::Search;
        }
        self.refresh_search_hits();
    }

    /// Fecha a barra e apaga o que ela deixou na tela.
    pub(super) fn close_search(&mut self) {
        self.editor_area.search_open = false;
        self.editor_area.search_query.clear();
        if self.context.focus == ShellFocus::Search {
            self.context.focus = ShellFocus::Editor;
        }
    }

    /// Remarca as ocorrências do que está escrito na barra.
    ///
    /// Chamado a cada tecla: procurar é sobre o texto **atual**, e uma marca de
    /// duas letras atrás apontaria para outro lugar.
    pub(super) fn refresh_search_hits(&mut self) {
        // O destaque acompanha a **barra**, e não o foco: clicar no editor não
        // pode apagar o que se estava procurando.
        let procurado = if self.editor_area.search_open {
            self.editor_area.search_query.clone()
        } else {
            String::new()
        };
        // O buffer inteiro era clonado aqui a cada tecla. Ele não precisa
        // ser: o que a marcação lê é o texto.
        let Some(document) = self.editor_area.session.active() else {
            return;
        };
        let texto = document.buffer.text().to_owned();
        self.editor_area.pane.set_search_hits(&texto, &procurado);
    }

    /// `Enter` na barra: o cursor vai à próxima ocorrência, dando a volta.
    pub fn go_to_next_search_hit(&mut self) -> bool {
        let Some(document) = self.editor_area.session.active() else {
            return false;
        };
        let texto = document.buffer.text().to_owned();
        self.editor_area
            .pane
            .go_to_next_search_hit(&texto)
            .is_some()
    }

    /// A área que o arranjo deu à barra de busca.
    #[must_use]
    pub fn search_box_area(&self) -> Rect {
        self.host.bounds(SEARCH_POPUP_ID).unwrap_or_default()
    }

    /// Se a barra de busca está na tela.
    #[must_use]
    pub const fn search_is_open(&self) -> bool {
        self.editor_area.search_open
    }

    /// As ocorrências marcadas, para os testes.
    #[cfg(test)]
    pub(super) fn search_hits(&self) -> &[(usize, usize)] {
        self.editor_area.pane.search_hits()
    }

    /// Onde o cursor está agora, na forma que o histórico guarda.
    ///
    /// Documento sem arquivo por trás não entra: voltar para ele significaria
    /// abrir um caminho que não existe.
    pub(super) fn posicao_atual(&self) -> Option<Posicao> {
        let caminho = self.active_document_path()?;
        let posicao = self.cursor_position()?;
        Some(Posicao {
            caminho,
            linha: posicao.line as usize,
            coluna: posicao.column as usize,
        })
    }

    /// `Ctrl+Alt+Esquerda`: volta um passo no caminho do `Ctrl+clique`.
    ///
    /// Devolve se houve para onde voltar, para quem chama saber se a tecla foi
    /// consumida.
    pub fn navigate_back(&mut self) -> bool {
        let Some(atual) = self.posicao_atual() else {
            return false;
        };
        let Some(destino) = self.editor_area.history.back(atual) else {
            return false;
        };
        self.abrir_posicao(&destino);
        true
    }

    /// `Ctrl+Alt+Direita`: avança um passo, desfazendo o último retorno.
    pub fn navigate_forward(&mut self) -> bool {
        let Some(atual) = self.posicao_atual() else {
            return false;
        };
        let Some(destino) = self.editor_area.history.forward(atual) else {
            return false;
        };
        self.abrir_posicao(&destino);
        true
    }

    /// Pede à aplicação o arquivo do passo, na linha e coluna de onde se saiu.
    fn abrir_posicao(&mut self, destino: &Posicao) {
        self.commands.push(ApplicationCommand::OpenDocument(
            OpenDocumentRequest::new(destino.caminho.clone()).at(destino.linha, destino.coluna),
        ));
    }

    /// Quantos passos há de cada lado, para os testes.
    #[cfg(test)]
    pub(super) const fn history_depth(&self) -> (usize, usize) {
        self.editor_area.history.depth()
    }

    /// Executa o que o painel de edição pediu.
    ///
    /// O painel edita texto; navegar até uma definição, marcar breakpoint,
    /// gravar e abrir menu são coisas que só o shell tem como fazer.
    pub(super) fn handle_editor_action(&mut self, action: EditorAction) {
        match action {
            EditorAction::Navigate(offset) => {
                if let (Some(document_id), Some(token)) = (
                    self.editor_area.session.active_id(),
                    self.active_text().and_then(|text| token_at(text, offset)),
                ) {
                    self.context.status_message = format!("Go to definition: {token}");
                    // De onde se está saindo, antes de sair: é isto que
                    // `Ctrl+Alt+Esquerda` devolve depois.
                    if let Some(origem) = self.posicao_atual() {
                        self.editor_area.history.record(origem);
                    }
                    self.commands
                        .push(ApplicationCommand::Navigate(NavigationRequest {
                            document_id,
                            byte_offset: offset,
                            token,
                        }));
                }
            }
            EditorAction::FindReferences(offset) => {
                if let (Some(document_id), Some(token)) = (
                    self.editor_area.session.active_id(),
                    self.active_text().and_then(|text| token_at(text, offset)),
                ) {
                    self.context.status_message = format!("Procurando usos de {token}…");
                    self.commands
                        .push(ApplicationCommand::FindReferences(NavigationRequest {
                            document_id,
                            byte_offset: offset,
                            token,
                        }));
                }
            }
            EditorAction::ToggleBreakpoint(line) => {
                if let Some(path) = self
                    .editor_area
                    .session
                    .active()
                    .map(|document| document.path.clone())
                {
                    self.toggle_breakpoint(&path, line as u32);
                }
            }
            EditorAction::Save => {
                self.request_save_active_document();
            }
            EditorAction::ContextMenu(_) | EditorAction::None => {}
        }
    }

    pub fn show_location(
        &mut self,
        path: &Path,
        text: impl Into<String>,
        line: usize,
        column: usize,
    ) -> DocumentId {
        let id = self.show_document(path, text);
        let text = self.active_text().unwrap_or_default();
        self.editor_area
            .pane
            .set_cursor(offset_for_line_column(text, line, column));
        // Sem revelar a linha, a navegação move o cursor para fora da área
        // visível e parece que nada aconteceu.
        self.editor_area.pane.reveal_line(line);
        self.editor_area.navigated = Some((line, self.editor_area.pane.cursor()));
        self.context.focus = ShellFocus::Editor;
        self.context.status_message =
            format!("Definition: {}:{}:{}", path.display(), line + 1, column + 1);
        id
    }

    /// Continua a rolagem de um arrasto que saiu da área visível do editor.
    ///
    /// A janela chama isto a cada tique do relógio, porque um arrasto parado
    /// fora da borda não gera evento nenhum — e ainda assim deve seguir
    /// rolando e marcando. Devolve se algo mudou, para a janela redesenhar
    /// só quando há o que mostrar.
    pub fn drag_autoscroll(&mut self, size: Size) -> bool {
        self.place_focused_editor(size);
        self.focused_editor()
            .is_some_and(|(pane, buffer)| pane.drag_autoscroll(buffer))
    }

    pub(super) fn editor_visible_lines(&self) -> usize {
        (self.geometry().editor_height / EDITOR_LINE_HEIGHT)
            .floor()
            .max(1.0) as usize
    }

    pub(super) fn editor_scrollbar_rect(&self, size: Size) -> Rect {
        let geo = self.geometry();
        let editor_x = ACTIVITY_WIDTH + self.sidebar_width(size);
        Rect::new(
            editor_x + geo.editor_width - 10.0,
            geo.content_top,
            10.0,
            geo.editor_height,
        )
    }

    /// Seleciona a palavra sob o ponteiro. É o que o duplo clique pede.
    pub fn select_word_at_point(&mut self, point: Point, size: Size) {
        self.place_focused_editor(size);
        let inside = self
            .focused_editor_ref()
            .is_some_and(|(pane, _)| pane.bounds().contains(point));
        if !inside {
            return;
        }
        // O gesto também leva o foco, como no clique simples.
        if self.inspection.is_open() {
            self.inspection.focus_source();
        } else {
            self.context.focus = ShellFocus::Editor;
        }
        if let Some((pane, buffer)) = self.focused_editor() {
            pane.select_word_at(buffer, point);
        }
    }

    /// Painel de edição que está na frente, com o texto que ele edita.
    ///
    /// A janela de inspeção cobre o editor principal, então **qual é "o editor"
    /// depende do que está na frente**. Responder isso num lugar só é o que
    /// impede cada gesto — arraste, duplo clique, copiar, colar — de escolher por
    /// conta própria e um dia escolher diferente do vizinho.
    pub(super) fn focused_editor(&mut self) -> Option<(&mut EditorPane, &mut TextBuffer)> {
        if self.inspection.is_open() {
            return Some(self.inspection.editor_and_source());
        }
        let document = self.editor_area.session.active_mut()?;
        Some((&mut self.editor_area.pane, &mut document.buffer))
    }

    /// Canto onde a lista de completação nasce, seja qual for o editor da frente.
    ///
    /// Um lugar só, porque a pintura e o clique precisam concordar: se o desenho
    /// e o teste de acerto calculassem a área cada um por si, clicar na borda da
    /// lista faria uma coisa e ver a lista mostraria outra.
    pub(super) fn completion_anchor(&self, size: Size) -> Option<Point> {
        if self.editor_area.completion_items.is_empty() {
            return None;
        }
        if self.inspection.is_open() {
            return self.inspection_completion_anchor();
        }
        if self.context.focus != ShellFocus::Editor {
            return None;
        }
        let text = self.active_text()?;
        let geo = self.geometry();
        let editor_x = ACTIVITY_WIDTH + self.sidebar_width(size);
        let (line, column) = line_column(text, self.editor_area.pane.cursor());
        Some(Point::new(
            (editor_x + EDITOR_GUTTER + column as f32 * EDITOR_CHAR_WIDTH)
                .min(size.width - 270.0)
                .max(editor_x + EDITOR_GUTTER),
            (geo.content_top
                + 36.0
                + line.saturating_sub(self.editor_area.pane.scroll_line()) as f32
                    * EDITOR_LINE_HEIGHT)
                .min(geo.editor_bottom - 190.0),
        ))
    }

    /// Área ocupada pela lista de completação na tela.
    pub(super) fn completion_rect(&self, size: Size) -> Option<Rect> {
        let anchor = self.completion_anchor(size)?;
        let rows = self
            .editor_area
            .completion_items
            .len()
            .min(COMPLETION_VISIBLE_ROWS);
        Some(Rect::new(
            anchor.x,
            anchor.y,
            COMPLETION_POPUP_WIDTH + COMPLETION_POPUP_PADDING * 2.0,
            rows as f32 * COMPLETION_ROW_HEIGHT + COMPLETION_POPUP_PADDING * 2.0,
        ))
    }

    /// Declara a lista de completação na pilha, no lugar onde ela flutua.
    ///
    /// A superfície e a lista dentro dela são dois nós: a moldura responde pelo
    /// respiro em volta, e a lista pelas linhas. Os dois significam "o gesto é da
    /// completação", e é por isso que o acerto aceita qualquer um deles.
    pub(super) fn place_completion(&mut self, size: Size) {
        let Some(rect) = self.completion_rect(size) else {
            return;
        };
        let visible = self
            .editor_area
            .completion_items
            .len()
            .min(COMPLETION_VISIBLE_ROWS);
        let mut list = ListView::new(
            COMPLETION_LIST_ID,
            self.editor_area
                .completion_items
                .iter()
                .map(|item| item.label.clone())
                .collect::<Vec<_>>(),
        )
        // Sem cor própria: o texto do tema é o escolhido para se ler sobre a
        // superfície, e é o mesmo em toda a interface.
        .with_row_height(COMPLETION_ROW_HEIGHT);
        list.set_selected(Some(self.editor_area.completion_selected));
        self.host.replace(Box::new(list));
        self.host.place(COMPLETION_POPUP_ID, rect);
        self.host.place(
            COMPLETION_LIST_ID,
            Rect::new(
                rect.origin.x + COMPLETION_POPUP_PADDING,
                rect.origin.y + COMPLETION_POPUP_PADDING,
                COMPLETION_POPUP_WIDTH,
                visible as f32 * COMPLETION_ROW_HEIGHT,
            ),
        );
    }

    /// Clique com a lista aberta. Devolve `true` quando ela consumiu o clique.
    ///
    /// Fora dela, o clique a dispensa: o usuário foi olhar outra coisa, e uma
    /// lista que sobrevive a isso fica pairando sobre um cursor que já se moveu.
    /// Dentro dela, escolhe a linha — e precisa consumir o clique de qualquer
    /// forma, ou ele atravessaria a lista e moveria o cursor no editor de baixo.
    ///
    /// Coberta por uma janela, ela não recebe nem se dispensa: o gesto nunca
    /// chegou até ela, e quem responde por ele é quem está na frente.
    pub(super) fn completion_pointer_down(&mut self, point: Point, size: Size) -> bool {
        let Some(rect) = self.completion_rect(size) else {
            return false;
        };
        self.place_overlay(size);
        let target = self.host.hit_test(point).next();
        if !matches!(target, Some(COMPLETION_POPUP_ID | COMPLETION_LIST_ID)) {
            if !self.covers_completion(target) {
                self.clear_completions();
            }
            return false;
        }
        let row = ((point.y - rect.origin.y - COMPLETION_POPUP_PADDING) / COMPLETION_ROW_HEIGHT)
            .floor()
            .max(0.0) as usize;
        if row < self.editor_area.completion_items.len() {
            self.editor_area.completion_selected = row;
            self.accept_completion();
        }
        true
    }

    /// Canto onde a lista nasce dentro da janela de inspeção.
    pub(super) fn inspection_completion_anchor(&self) -> Option<Point> {
        if self.editor_area.completion_items.is_empty() {
            return None;
        }
        let (editor, source) = self.inspection.editor_and_source_ref();
        let bounds = editor.bounds();
        let (line, column) = line_column(source.text(), editor.cursor());
        Some(Point::new(
            (bounds.origin.x + EDITOR_GUTTER + column as f32 * EDITOR_CHAR_WIDTH)
                .min(bounds.origin.x + bounds.size.width - COMPLETION_POPUP_WIDTH)
                .max(bounds.origin.x),
            bounds.origin.y
                + (line.saturating_sub(editor.scroll_line()) + 1) as f32 * EDITOR_LINE_HEIGHT,
        ))
    }

    /// O mesmo, para quem só precisa ler.
    pub(super) fn focused_editor_ref(&self) -> Option<(&EditorPane, &TextBuffer)> {
        if self.inspection.is_open() {
            return Some(self.inspection.editor_and_source_ref());
        }
        let document = self.editor_area.session.active()?;
        Some((&self.editor_area.pane, &document.buffer))
    }

    /// Põe o painel da frente na área que ele ocupa agora.
    ///
    /// Converter ponto em posição do texto depende de saber onde o painel está, e
    /// as duas áreas mudam com o tamanho da janela.
    pub(super) fn place_focused_editor(&mut self, size: Size) {
        if self.inspection.is_open() {
            self.inspection.layout_editor(&self.host);
            return;
        }
        let bounds = self.editor_view_rect(size);
        self.editor_area.pane.set_bounds(bounds);
    }

    /// Pede a avaliação do trecho marcado no quadro atual da depuração.
    pub(super) fn inspect_selection(&mut self) {
        let Some(range) = self.editor_area.pane.selection_range() else {
            return;
        };
        let Some(expression) = self
            .active_text()
            .and_then(|text| text.get(range))
            .map(str::trim)
            .filter(|expression| !expression.is_empty())
            .map(str::to_owned)
        else {
            return;
        };
        self.context.status_message = format!("Inspecionando {expression}");
        self.commands
            .push(ApplicationCommand::Debug(DebugRequest::Evaluate(
                expression,
            )));
    }

    /// Copia o trecho selecionado para a área de transferência do sistema.
    pub fn copy_selection(&mut self) -> bool {
        let selected = self
            .focused_editor_ref()
            .and_then(|(pane, buffer)| pane.selected_text(buffer))
            .map(str::to_owned);
        let Some(text) = selected else {
            self.context.status_message = "Nada selecionado".to_owned();
            return false;
        };
        let Some(clipboard) = self.context.clipboard.as_ref() else {
            self.context.status_message = "Área de transferência indisponível".to_owned();
            return false;
        };
        match clipboard.set_text(&text) {
            Ok(()) => {
                self.context.status_message =
                    format!("Copiado {} caractere(s)", text.chars().count());
                true
            }
            Err(error) => {
                self.context.status_message = error.to_string();
                false
            }
        }
    }

    /// Cola o conteúdo da área de transferência no cursor.
    ///
    /// Havendo trecho selecionado, ele é substituído — colar sobre uma seleção é
    /// trocar aquele texto, e é o que qualquer editor faz.
    pub fn paste_clipboard(&mut self) -> bool {
        let Some(clipboard) = self.context.clipboard.as_ref() else {
            self.context.status_message = "Área de transferência indisponível".to_owned();
            return false;
        };
        match clipboard.get_text() {
            Ok(Some(text)) if !text.is_empty() => {
                self.edit_focused(&text);
                true
            }
            Ok(_) => {
                self.context.status_message = "Área de transferência vazia".to_owned();
                false
            }
            Err(error) => {
                self.context.status_message = error.to_string();
                false
            }
        }
    }

    /// Escreve no painel que está na frente.
    ///
    /// Marcar o documento como modificado e fechar o autocomplete são efeitos do
    /// editor de arquivos; o rascunho da inspeção não tem nenhum dos dois.
    pub(super) fn edit_focused(&mut self, text: &str) {
        if self.inspection.is_open() {
            self.inspection.text_input(text);
            return;
        }
        self.edit_active(text);
    }

    /// Escreve no documento ativo pelo painel de edição.
    pub(super) fn edit_active(&mut self, text: &str) {
        self.editor_area.completion_items.clear();
        let Some(document) = self.editor_area.session.active_mut() else {
            return;
        };
        // `insert_pairing`, e não `insert`: escrever código é o único lugar da
        // IDE onde abrir um parêntese pede o fechamento. A caixa de busca e o
        // rascunho da inspeção continuam escrevendo o que se digita.
        //
        // A revisão diz se o texto mudou, e a resposta do painel não: passar por
        // cima de um `)` que já estava lá é movimento do cursor, e anunciar
        // "Modified" ali seria dizer que o arquivo mudou quando ele não mudou.
        let before = document.buffer.revision();
        self.editor_area
            .pane
            .insert_pairing(&mut document.buffer, text);
        if document.buffer.revision() != before {
            self.context.status_message = "Modified".to_owned();
        }
    }

    pub(super) fn backspace(&mut self) {
        self.editor_area.completion_items.clear();
        let Some(document) = self.editor_area.session.active_mut() else {
            return;
        };
        let before = document.buffer.revision();
        if !self
            .editor_area
            .pane
            .backspace_pairing(&mut document.buffer)
        {
            self.editor_area
                .pane
                .key(&mut document.buffer, "backspace", false, false);
        }
        if document.buffer.revision() != before {
            self.context.status_message = "Modified".to_owned();
        }
    }

    /// Tecla dirigida à lista de completação aberta. `true` quando ela consumiu.
    ///
    /// Com a lista à mostra, as setas andam nela e não no texto — é o que se
    /// espera de qualquer editor, e vale igual nos dois editores da IDE.
    pub(super) fn completion_key(&mut self, key: &str) -> bool {
        if self.editor_area.completion_items.is_empty() {
            return false;
        }
        match key.to_ascii_lowercase().as_str() {
            "arrowdown" => {
                self.editor_area.completion_selected = (self.editor_area.completion_selected + 1)
                    .min(self.editor_area.completion_items.len() - 1);
                true
            }
            "arrowup" => {
                self.editor_area.completion_selected =
                    self.editor_area.completion_selected.saturating_sub(1);
                true
            }
            "enter" | "tab" => {
                self.accept_completion();
                true
            }
            "escape" => {
                self.editor_area.completion_items.clear();
                true
            }
            _ => false,
        }
    }

    pub(super) fn accept_completion(&mut self) {
        let Some(item) = self
            .editor_area
            .completion_items
            .get(self.editor_area.completion_selected)
            .cloned()
        else {
            return;
        };
        // Trocar o que já foi digitado pelo item escolhido é a mesma operação nos
        // dois editores; o que muda é qual deles está na frente.
        if let Some((pane, buffer)) = self.focused_editor() {
            let cursor = pane.cursor().min(buffer.text().len());
            let prefix = identifier_prefix(buffer.text(), cursor);
            let start = cursor.saturating_sub(prefix.len());
            if buffer.replace(start..cursor, &item.label).is_ok() {
                pane.set_cursor(start + item.label.len());
                self.context.status_message = format!("Completed {}", item.label);
                // **Escrever o nome pode não bastar.** Um tipo de outro módulo
                // precisa do `import`, e quem sabe qual é a linguagem. Aqui só
                // se anota o que foi escolhido; quem pergunta é a aplicação.
                self.editor_area.completacao_aceita = Some(item.label.clone());
            }
        }
        self.editor_area.completion_items.clear();
    }

    /// O item aceito desde a última vez que se perguntou, **e o consome**.
    ///
    /// Consome porque a pergunta que vem depois dele — o que mais muda no
    /// arquivo — só faz sentido uma vez: repeti-la escreveria o mesmo `import`
    /// a cada quadro.
    pub fn completacao_aceita(&mut self) -> Option<String> {
        self.editor_area.completacao_aceita.take()
    }

    /// Aplica as trocas que a linguagem pediu, de trás para frente.
    ///
    /// **A ordem não é capricho.** Cada troca vem com posições do texto de
    /// antes; aplicar da primeira para a última desloca todas as seguintes.
    /// De trás para frente, o que já foi aplicado está sempre depois do que
    /// falta.
    pub fn aplicar_trocas(&mut self, trocas: &[ide_domain::TextEdit]) {
        let Some((pane, buffer)) = self.focused_editor() else {
            return;
        };
        let cursor = pane.cursor();
        let mut ordenadas: Vec<&ide_domain::TextEdit> = trocas.iter().collect();
        ordenadas.sort_by_key(|troca| (troca.range.start.line, troca.range.start.column));
        let mut movido = 0usize;
        for troca in ordenadas.iter().rev() {
            let texto = buffer.text().to_owned();
            let inicio = offset_for_line_column(
                &texto,
                troca.range.start.line as usize,
                troca.range.start.column as usize,
            );
            let fim = offset_for_line_column(
                &texto,
                troca.range.end.line as usize,
                troca.range.end.column as usize,
            );
            if inicio <= fim && buffer.replace(inicio..fim, &troca.text).is_ok() && inicio <= cursor
            {
                movido += troca.text.len();
                movido = movido.saturating_sub(fim - inicio);
            }
        }
        // O `import` entra acima de onde se estava escrevendo, e sem isto o
        // cursor ficaria tantos caracteres atrás quantos ele acrescentou.
        pane.set_cursor(cursor + movido);
    }

    /// Clique na fileira de abas do editor: escolher uma, ou fechá-la.
    ///
    /// Quem decide entre ativar e fechar é o componente; a IDE traduz o comando
    /// recebido para o documento correspondente.
    pub(super) fn editor_tabs_pointer_down(&mut self, point: Point, size: Size) -> bool {
        let editor_x = ACTIVITY_WIDTH + self.sidebar_width(size);
        if point.y < TITLE_HEIGHT || point.y >= TITLE_HEIGHT + TAB_HEIGHT || point.x < editor_x {
            return false;
        }
        self.place_overlay(size);
        match tab_action(&mut self.host, point) {
            Some(WidgetAction::TabSelected { tab, .. }) => {
                let _ = self.editor_area.session.activate(DocumentId(tab));
                self.editor_area.pane.set_cursor(0);
                self.context.focus = ShellFocus::Editor;
                self.sync_explorer_to_active();
            }
            Some(WidgetAction::TabClosed { tab, .. }) => {
                let id = DocumentId(tab);
                if self.editor_area.session.close(id).is_ok() {
                    self.editor_area.syntax_snapshots.remove(&id);
                    self.editor_area.syntax_spans.remove(&id);
                    // O outro lado pode estar mostrando o mesmo documento, e ele
                    // acabou de deixar de existir.
                    self.split_forget(id);
                    self.editor_area
                        .pane
                        .set_cursor(self.active_text().map_or(0, str::len));
                    self.context.status_message = "Tab closed".to_owned();
                    self.sync_explorer_to_active();
                }
            }
            _ => {}
        }
        true
    }

    /// Clique dentro do texto. O painel cuida de cursor, âncora e calha; o shell
    /// só reage ao que ele pede.
    pub(super) fn editor_pointer_down(
        &mut self,
        point: Point,
        size: Size,
        control: bool,
        shift: bool,
    ) -> bool {
        let geometry = self.geometry();
        let editor_x = ACTIVITY_WIDTH + self.sidebar_width(size);
        if point.x < editor_x
            || point.x >= editor_x + geometry.editor_width
            || point.y < geometry.content_top
            || point.y >= geometry.editor_bottom
        {
            return false;
        }
        self.context.focus = ShellFocus::Editor;
        let bounds = self.editor_view_rect(size);
        self.editor_area.pane.set_bounds(bounds);
        let Some(document) = self.editor_area.session.active() else {
            return true;
        };
        let action = self
            .editor_area
            .pane
            .pointer_down(&document.buffer, point, control, shift);
        self.handle_editor_action(action);
        true
    }
}
