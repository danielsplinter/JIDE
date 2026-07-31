//! Como cada quadro é desenhado, da moldura ao conteúdo.
//!
//! A ordem é a da profundidade: o que está atrás primeiro, e as janelas por
//! cima, na ordem inversa do funil de eventos.

use super::*;
use ui_components::{ConsoleLine, Label, Panel, SurfaceTone, paint_icon};

impl IdeShell {
    /// Desenha um texto solto com a `Label` da biblioteca.
    ///
    /// A IDE escolhe o papel — título, legenda, aviso — e a posição; cor e
    /// desenho vêm do componente, e é por aí que o tema alcança este texto.
    fn paint_label(
        &self,
        commands: &mut Vec<PaintCommand>,
        id: WidgetId,
        text: &str,
        origin: Point,
        font_size: f32,
        tone: IconTint,
    ) {
        let mut label = Label::new(id, text)
            .with_font_size(font_size)
            .with_tone(tone);
        label.layout(
            &self.layout_context(),
            Rect::new(origin.x, origin.y, 0.0, 0.0),
        );
        let mut paint = self.paint_context();
        label.paint(&mut paint);
        commands.extend(paint.into_commands());
    }

    /// Desenha uma faixa da moldura com a superfície da biblioteca.
    ///
    /// A cor vem do tom, e o tom vem do tema: escrita à mão, cada faixa fixava a
    /// cor no lugar, e a tela deixava de trocar de tema mesmo tendo tema.
    fn paint_surface_band(
        &self,
        commands: &mut Vec<PaintCommand>,
        id: WidgetId,
        area: Rect,
        tone: SurfaceTone,
        border: bool,
    ) {
        let mut panel = Panel::new(id, tone);
        if border {
            panel = panel.with_border();
        }
        panel.layout(&self.layout_context(), area);
        let mut paint = self.paint_context();
        panel.paint(&mut paint);
        commands.extend(paint.into_commands());
    }

    pub fn set_text_metrics(&mut self, metrics: Arc<dyn TextMetrics>) {
        self.host.set_text_metrics(Arc::clone(&metrics));
        self.context.text_metrics = Some(metrics);
    }

    /// Contexto de pintura com a medição disponível, quando houver.
    pub(super) fn paint_context(&self) -> PaintContext {
        let context = PaintContext::with_theme(self.context.theme);
        match self.context.text_metrics.as_ref() {
            Some(metrics) => context.measuring(Arc::clone(metrics)),
            None => context,
        }
    }

    /// Contexto de layout com a medição disponível, quando houver.
    pub(super) fn layout_context(&self) -> LayoutContext {
        match self.context.text_metrics.as_ref() {
            Some(metrics) => LayoutContext::with_text_metrics(Arc::clone(metrics)),
            None => LayoutContext::default(),
        }
    }

    /// Desenha uma barra com o componente da biblioteca.
    pub(super) fn paint_scrollbar(&self, target: ScrollTarget, size: Size) -> Vec<PaintCommand> {
        let (track, content, viewport, offset) = self.scrollbar_range(target, size);
        let orientation = match target {
            ScrollTarget::EditorHorizontal | ScrollTarget::ExplorerHorizontal => {
                ScrollbarOrientation::Horizontal
            }
            _ => ScrollbarOrientation::Vertical,
        };
        let mut bar = Scrollbar::new(WidgetId(0), orientation).with_range(content, viewport);
        bar.layout(&self.layout_context(), track);
        bar.set_offset(offset);
        let mut paint = self.paint_context();
        bar.paint(&mut paint);
        paint.into_commands()
    }

    /// Pontos de parada e a linha em que a execução parou.
    pub(super) fn editor_decorations(&self, path: &Path) -> Vec<LineDecoration> {
        let mut decorations: Vec<LineDecoration> = self
            .debug_panel
            .breakpoints
            .get(path)
            .into_iter()
            .flatten()
            .map(|line| {
                // Confirmado pelo alvo é um disco; ainda não registrado — sem
                // sessão, ou classe não carregada — é só o contorno.
                let mark = if self.breakpoint_is_verified(path, *line) {
                    GutterMark::Breakpoint
                } else {
                    GutterMark::PendingBreakpoint
                };
                LineDecoration::mark(*line as usize, mark)
            })
            .collect();
        let destacar = |decorations: &mut Vec<LineDecoration>, line: usize| match decorations
            .iter_mut()
            .find(|item| item.line == line)
        {
            Some(existing) => *existing = existing.with_highlight(),
            None => decorations.push(LineDecoration::highlight(line)),
        };
        if let Some((_, line)) = self
            .debug_panel
            .view
            .stopped_at
            .as_ref()
            .filter(|(stopped, _)| stopped == path)
        {
            destacar(&mut decorations, *line as usize);
        }
        // Destino da última navegação, enquanto o cursor não sair de lá.
        if let Some((line, cursor)) = self.editor_area.navigated
            && cursor == self.editor_area.pane.cursor()
        {
            destacar(&mut decorations, line);
        }
        decorations
    }

    pub(super) fn paint_debug_panel(&self, size: Size, colors: ColorTokens) -> Vec<PaintCommand> {
        let geometry = debug_panel_geometry(
            self.debug_panel_rect(size),
            self.debug_panel.view.frames.len(),
        );
        let panel = geometry.panel;
        let mut commands = Vec::new();
        self.paint_surface_band(
            &mut commands,
            DEBUG_PANEL_SURFACE_ID,
            panel,
            SurfaceTone::Surface,
            false,
        );
        commands.extend([
            raw_fill(
                Rect::new(panel.origin.x, panel.origin.y, 1.0, panel.size.height),
                colors.border,
            ),
            PaintCommand::PushClip(panel),
        ]);
        self.paint_label(
            &mut commands,
            DEBUG_STATUS_ID,
            &self.debug_panel.view.status,
            Point::new(panel.origin.x + 12.0, panel.origin.y + 10.0),
            13.0,
            if self.debug_panel.view.is_stopped() {
                IconTint::Accent
            } else {
                IconTint::Text
            },
        );

        // Sem quadro parado não há passo a dar: o botão desabilitado diz isso
        // pelo próprio desenho, que é o que o rótulo apagado tentava dizer à mão.
        let parado = self.debug_panel.view.is_stopped();
        let mut faixa = self.paint_context();
        for (rect, button) in geometry.buttons.iter().zip(&self.debug_panel.step_buttons) {
            let mut button = button.clone();
            button.set_disabled(!parado);
            button.layout(&self.layout_context(), *rect);
            button.paint(&mut faixa);
        }
        commands.extend(faixa.into_commands());

        self.paint_label(
            &mut commands,
            DEBUG_FRAMES_TITLE_ID,
            "Pilha de chamadas",
            Point::new(panel.origin.x + 12.0, geometry.frames.origin.y - 20.0),
            12.0,
            IconTint::Muted,
        );
        let mut frames = self.debug_panel.frames.clone();
        frames.layout(&self.layout_context(), geometry.frames);
        let mut variables = self.debug_panel.variables.clone();
        variables.layout(&self.layout_context(), geometry.variables);
        let mut lists = self.paint_context();
        frames.paint(&mut lists);

        self.paint_label(
            &mut commands,
            DEBUG_VARS_TITLE_ID,
            "Variáveis",
            Point::new(panel.origin.x + 12.0, geometry.variables.origin.y - 20.0),
            12.0,
            IconTint::Muted,
        );
        variables.paint(&mut lists);
        commands.extend(lists.into_commands());
        commands.push(PaintCommand::PopClip);
        commands
    }

    /// Desenha a janela, se ela estiver aberta.
    pub(super) fn paint_surface(
        &mut self,
        kind: SurfaceKind,
        commands: &mut Vec<PaintCommand>,
        size: Size,
        colors: ColorTokens,
    ) {
        match kind {
            SurfaceKind::Rename => self.paint_rename(commands, size),
            SurfaceKind::Generate => self.paint_generate(commands, size),
            SurfaceKind::TypeSearch => self.paint_type_search(commands, size),
            SurfaceKind::Inspection => self.paint_inspection(commands, size),
            SurfaceKind::NewItem => self.paint_new_item_dialog(commands, size),
            SurfaceKind::Settings => commands.extend(self.settings.paint(
                &self.host,
                &self.layout_context(),
                size,
                &self.catalog.settings_sections,
                colors,
                (self.paint_context(), self.paint_context()),
            )),
        }
    }

    /// Desenha o quadro.
    ///
    /// Pintar exige acesso mutável porque o shell mantém widgets com estado
    /// próprio — o editor guarda uma cópia do documento ativo, reconstruída
    /// quando o texto muda. Deixar essa reconciliação para os manipuladores de evento faria
    /// cada esquecimento virar um quadro desatualizado.
    pub fn paint(&mut self, size: Size) -> Vec<PaintCommand> {
        self.context.last_size = size;
        // A pilha é declarada antes do quadro: é dela que sai o que o anfitrião
        // desenha, e é ela que o gesto seguinte vai consultar.
        self.place_overlay(size);
        let sidebar = self.sidebar_width(size);
        let editor_x = ACTIVITY_WIDTH + sidebar;
        let geo = self.geometry();
        let colors = self.context.theme.colors;
        let mut commands = Vec::new();
        // As faixas da moldura, de trás para a frente. Cada uma é uma superfície
        // da biblioteca: o tom nomeia o nível, e o tema resolve a cor.
        for (id, area, tone, border) in [
            (
                CHROME_BACKGROUND_ID,
                Rect::new(0.0, 0.0, size.width, size.height),
                SurfaceTone::Background,
                false,
            ),
            (
                CHROME_TITLE_ID,
                Rect::new(0.0, 0.0, size.width, TITLE_HEIGHT),
                SurfaceTone::Elevated,
                false,
            ),
            (
                CHROME_ACTIVITY_ID,
                Rect::new(
                    0.0,
                    TITLE_HEIGHT,
                    ACTIVITY_WIDTH,
                    geo.content_bottom - TITLE_HEIGHT,
                ),
                SurfaceTone::Elevated,
                false,
            ),
            (
                CHROME_SIDEBAR_ID,
                Rect::new(
                    ACTIVITY_WIDTH,
                    TITLE_HEIGHT,
                    sidebar,
                    geo.content_bottom - TITLE_HEIGHT,
                ),
                SurfaceTone::Surface,
                false,
            ),
            (
                CHROME_TABS_ID,
                Rect::new(editor_x, TITLE_HEIGHT, geo.editor_width, TAB_HEIGHT),
                SurfaceTone::Elevated,
                false,
            ),
            (
                CHROME_TERMINAL_ID,
                Rect::new(
                    editor_x,
                    geo.editor_bottom,
                    geo.editor_width,
                    geo.terminal_height,
                ),
                SurfaceTone::Surface,
                true,
            ),
        ] {
            self.paint_surface_band(&mut commands, id, area, tone, border);
        }
        for (id, texto, origem, tamanho, tom) in [
            (
                CHROME_TITLE_TEXT_ID,
                "ER IDE",
                Point::new(14.0, 9.0),
                16.0,
                IconTint::Text,
            ),
            (
                CHROME_EXPLORER_ID,
                "EXPLORER",
                Point::new(ACTIVITY_WIDTH + 14.0, TITLE_HEIGHT + 14.0),
                12.0,
                IconTint::Muted,
            ),
            (
                CHROME_WORKSPACE_ID,
                self.explorer.workspace_name.as_str(),
                Point::new(ACTIVITY_WIDTH + 14.0, TITLE_HEIGHT + 42.0),
                14.0,
                IconTint::Text,
            ),
        ] {
            self.paint_label(&mut commands, id, texto, origem, tamanho, tom);
        }
        // Os ícones da barra de atividades são desenhados pela biblioteca, com
        // primitivas: como texto, dependiam de a fonte do sistema ter aquele
        // glifo, e ficavam fora do tema e da acessibilidade.
        let mut icones = self.paint_context();
        for (icone, topo) in [
            (Icon::Search, TITLE_HEIGHT + 8.0),
            (Icon::Panels, TITLE_HEIGHT + 52.0),
        ] {
            paint_icon(
                &mut icones,
                Rect::new(12.0, topo, 24.0, 24.0),
                icone,
                colors.text,
            );
        }
        commands.extend(icones.into_commands());
        // O botão de recolher o terminal é um botão de verdade: retângulo,
        // borda e glifo à mão não acendiam sob o ponteiro nem chegavam à árvore
        // de acessibilidade.
        let (icone, nome) = if self.terminal.minimized {
            (Icon::ChevronUp, "Mostrar o terminal")
        } else {
            (Icon::ChevronDown, "Recolher o terminal")
        };
        let mut recolher = Button::icon(TERMINAL_TOGGLE_ID, icone, nome);
        recolher.layout(
            &self.layout_context(),
            Rect::new(size.width - 30.0, geo.editor_bottom + 4.0, 22.0, 22.0),
        );
        let mut recolher_paint = self.paint_context();
        recolher.paint(&mut recolher_paint);
        commands.extend(recolher_paint.into_commands());
        commands.push(PaintCommand::PushClip(Rect::new(
            ACTIVITY_WIDTH,
            EXPLORER_TOP - EXPLORER_ROW_HEIGHT,
            self.sidebar_width(size),
            (geo.content_bottom - EXPLORER_TOP + EXPLORER_ROW_HEIGHT - 12.0).max(0.0),
        )));
        // A árvore é um componente: recuo, marcador de expansão, virtualização,
        // seleção e deslocamento horizontal pertencem a ela.
        let tree = self.explorer_tree_for(size);
        let mut tree_paint = self.paint_context();
        tree.paint(&mut tree_paint);
        commands.extend(tree_paint.into_commands());
        commands.push(PaintCommand::PopClip);
        commands.extend(self.paint_scrollbar(ScrollTarget::ExplorerHorizontal, size));
        commands.extend(self.paint_scrollbar(ScrollTarget::ExplorerVertical, size));
        // Os divisores se desenham: a linha é a mesma borda de antes, mas agora
        // ela se destaca sob o ponteiro, que é o que revela que dá para arrastar.
        let mut splitters = self.paint_context();
        self.sidebar_splitter_for(size).paint(&mut splitters);
        if !self.terminal.minimized {
            self.terminal_splitter_for(size).paint(&mut splitters);
        }
        commands.extend(splitters.into_commands());
        // As abas são um componente: largura, faixa da aba ativa, corte do
        // título, ponto de alterado e botão de fechar pertencem a ele. A
        // instância é a do anfitrião — a mesma que recebeu o gesto —, e por isso
        // a aba sob o ponteiro se destaca.
        let mut tabs_paint = self.paint_context();
        if let Some(tabs) = self.host.widget(EDITOR_TABS_ID) {
            tabs.paint(&mut tabs_paint);
        }
        commands.extend(tabs_paint.into_commands());
        commands.push(PaintCommand::PushClip(Rect::new(
            editor_x,
            geo.content_top,
            geo.editor_width,
            geo.editor_height,
        )));
        if self.editor_area.session.active().is_some() {
            // O editor da biblioteca desenha calha, números, realce, marcas de
            // ponto de parada, linha em execução e cursor. A IDE entrega o
            // texto, o realce e as decorações.
            self.sync_editor_pane(size);
            let mut editor_paint = self.paint_context();
            self.editor_area.pane.paint(&mut editor_paint);
            commands.extend(editor_paint.into_commands());
            commands.extend(self.paint_scrollbar(ScrollTarget::Editor, size));
            // A barra lateral só aparece quando há linha passando da área: uma
            // trilha permanente ocuparia altura útil sem servir para nada.
            if self.editor_scrolls_sideways(size) {
                commands.extend(self.paint_scrollbar(ScrollTarget::EditorHorizontal, size));
            }
        } else {
            self.paint_label(
                &mut commands,
                EDITOR_EMPTY_ID,
                "Select a file in Explorer",
                Point::new(editor_x + 55.0, geo.content_top + 30.0),
                16.0,
                IconTint::Muted,
            );
        }
        commands.push(PaintCommand::PopClip);
        if self.debug_panel.view.attached {
            commands.extend(self.paint_debug_panel(size, colors));
        }
        if !self.terminal.minimized {
            let mut terminal_tabs_paint = self.paint_context();
            if let Some(tabs) = self.host.widget(TERMINAL_TABS_ID) {
                tabs.paint(&mut terminal_tabs_paint);
            }
            commands.extend(terminal_tabs_paint.into_commands());
            self.paint_surface_band(
                &mut commands,
                TERMINAL_INPUT_ID,
                Rect::new(editor_x, geo.editor_bottom + 30.0, geo.editor_width, 30.0),
                SurfaceTone::Background,
                false,
            );
            let linha_de_comando = {
                let terminal = &self.terminal.tabs[self.terminal.active];
                format!("{} {}", terminal.session.prompt(), terminal.session.input())
            };
            self.paint_label(
                &mut commands,
                TERMINAL_PROMPT_ID,
                &linha_de_comando,
                Point::new(editor_x + 14.0, geo.editor_bottom + 38.0),
                14.0,
                IconTint::Text,
            );
            let active_terminal = &self.terminal.tabs[self.terminal.active];
            let terminal_visible = ((geo.terminal_height - 62.0) / EDITOR_LINE_HEIGHT)
                .floor()
                .max(1.0) as usize;
            let terminal_offset = active_terminal.scroll_line.min(
                active_terminal
                    .session
                    .line_count()
                    .saturating_sub(terminal_visible),
            );
            // A saída é do console da biblioteca: ele mede a fonte de código e é
            // essa medida que posiciona o realce da seleção e responde onde o
            // clique caiu. A IDE diz quais linhas, quais estão marcadas e onde.
            let linhas: Vec<_> = active_terminal
                .session
                .lines()
                .map(|line| ConsoleLine::new(line.text.clone(), line.is_error))
                .collect();
            let marcadas: Vec<_> = (terminal_offset..terminal_offset + terminal_visible)
                .filter_map(|numero| {
                    let linha = active_terminal.session.lines().nth(numero)?;
                    let (inicio, fim) =
                        selection_columns(self.terminal.selection, numero, &linha.text)?;
                    Some((numero, inicio, fim))
                })
                .collect();
            let contexto = self.layout_context();
            let area = Rect::new(
                editor_x,
                geo.editor_bottom + 66.0,
                geo.editor_width,
                terminal_visible as f32 * EDITOR_LINE_HEIGHT,
            );
            let mut saida = self.paint_context();
            // O console é o mesmo entre quadros: é a medição guardada nele que
            // o clique consulta depois, e as duas precisam concordar.
            let console = &mut self.terminal.console;
            console.set_lines(linhas);
            console.set_first_visible(terminal_offset);
            console.set_selection(marcadas);
            console.layout(&contexto, area);
            console.paint(&mut saida);
            commands.extend(saida.into_commands());
            commands.extend(self.paint_scrollbar(ScrollTarget::Terminal, size));
        } else {
            self.paint_label(
                &mut commands,
                TERMINAL_COLLAPSED_ID,
                "Terminal",
                Point::new(editor_x + 10.0, geo.editor_bottom + 8.0),
                13.0,
                IconTint::Text,
            );
        }
        if !self.inspection.is_open()
            && let Some(anchor) = self.completion_anchor(size)
        {
            self.paint_completion(&mut commands, size, anchor);
        }
        if self.context.focus == ShellFocus::Search {
            // Também da biblioteca: a caixa é um campo de texto sobre uma
            // superfície flutuante, e a IDE só escolhe o canto e o conteúdo.
            let width = SEARCH_BOX_WIDTH.min((geo.editor_width - 24.0).max(100.0));
            let mut surface = Popup::new(SEARCH_POPUP_ID).with_padding(6.0);
            surface.set_content_size(Size::new(width - 12.0, SEARCH_BOX_HEIGHT - 12.0));
            surface.layout(
                &self.layout_context(),
                Rect::new(0.0, 0.0, size.width, size.height),
            );
            surface.open_at(Point::new(
                size.width - width - 12.0,
                geo.content_top + 12.0,
            ));
            let mut search_paint = self.paint_context();
            surface.paint(&mut search_paint);
            if let Some(content) = surface.content_rect() {
                let mut field = TextInput::new(SEARCH_INPUT_ID, &self.editor_area.search_query)
                    .with_placeholder("Buscar no arquivo");
                // A busca só aparece quando tem o foco do shell, então o campo
                // é desenhado no estado focado — é ali que o cursor está.
                field.event(&mut EventContext::default(), &UiEvent::FocusGained);
                field.layout(&self.layout_context(), content);
                field.paint(&mut search_paint);
            }
            commands.extend(search_paint.into_commands());
        }
        let position = self
            .active_text()
            .map(|text| line_column(text, self.editor_area.pane.cursor()))
            .unwrap_or((0, 0));
        // A barra de estado é da biblioteca: superfície, borda, alinhamento e
        // recorte vêm de lá. A IDE só diz o que cada segmento informa — a
        // mensagem da última ação à esquerda, e à direita o que o usuário
        // procura sempre no mesmo lugar.
        let mut status_bar =
            StatusBar::new(STATUS_BAR_ID).with_leading([&self.context.status_message]);
        let mut trailing = vec![
            "UTF-8".to_owned(),
            format!("Ln {}, Col {}", position.0 + 1, position.1 + 1),
        ];
        if let Some(summary) = self.context.project_summary.as_deref() {
            trailing.push(summary.to_owned());
        }
        status_bar.set_trailing(trailing);
        status_bar.layout(
            &self.layout_context(),
            Rect::new(
                0.0,
                geo.content_bottom,
                size.width,
                size.height - geo.content_bottom,
            ),
        );
        let mut status_paint = self.paint_context();
        status_bar.paint(&mut status_paint);
        commands.extend(status_paint.into_commands());
        let mut menu_bar = self.menu.bar.clone();
        menu_bar.layout(
            &self.layout_context(),
            Rect::new(82.0, 0.0, (size.width - 82.0).max(0.0), TITLE_HEIGHT),
        );
        let mut menu_paint = self.paint_context();
        menu_bar.paint(&mut menu_paint);
        commands.extend(menu_paint.into_commands());
        // Os botões de ação são widgets da biblioteca: a IDE define papel e
        // posição, e o desenho do ícone e o tema vêm de lá.
        let rects = action_button_rects(size);
        let mut stop = self.debug_panel.stop_button.clone();
        stop.set_tint(if self.application_running() {
            IconTint::Danger
        } else {
            IconTint::Muted
        });
        stop.set_disabled(!self.application_running());
        let mut debug = self.debug_panel.debug_button.clone();
        debug.set_tint(if self.debug_panel.view.attached {
            IconTint::Accent
        } else {
            IconTint::Muted
        });
        let mut run = self.debug_panel.run_button.clone();
        let mut actions = self.paint_context();
        for (button, rect) in [
            (&mut stop, rects[0]),
            (&mut run, rects[1]),
            (&mut debug, rects[2]),
        ] {
            button.layout(&self.layout_context(), rect);
            button.paint(&mut actions);
        }
        commands.extend(actions.into_commands());
        // As janelas na ordem inversa do funil: a de baixo é desenhada primeiro,
        // e a que recebe o gesto é a última a cobrir a tela. É a mesma lista que
        // roteia o evento, lida de trás para a frente — não há como uma janela
        // nova receber o clique e esquecer de aparecer.
        for kind in SURFACES.into_iter().rev() {
            self.paint_surface(kind, &mut commands, size, colors);
        }
        // Depois da janela de inspeção, ou a lista ficaria atrás dela.
        if self.inspection.is_open()
            && let Some(anchor) = self.inspection_completion_anchor()
        {
            self.paint_completion(&mut commands, size, anchor);
        }
        // O menu de contexto é desenhado por último: ele cobre tudo, inclusive
        // o painel de onde foi aberto.
        if self.explorer.context_menu.is_open() {
            let mut menu = self.explorer.context_menu.clone();
            menu.layout(
                &self.layout_context(),
                Rect::new(0.0, 0.0, size.width, size.height),
            );
            let mut menu_paint = self.paint_context();
            menu.paint(&mut menu_paint);
            commands.extend(menu_paint.into_commands());
        }
        commands
    }

    /// Desenha a janela de inspeção: lista à esquerda, detalhe à direita.
    ///
    /// A lista mostra o valor pedido e os campos dele; o painel direito descreve
    /// a entrada destacada. O valor completo cabe ali, e não numa linha de lista,
    /// que o recorte cortaria justamente onde está a informação.
    /// Desenha a lista de completação ancorada num ponto da tela.
    ///
    /// Superfície flutuante e lista são da biblioteca: a IDE só diz onde ancorar,
    /// o que listar e o que está selecionado. Quem chama decide o ponto, e é isso
    /// que permite a mesma lista servir ao editor da janela e ao do depurador,
    /// que estão em lugares diferentes.
    pub(super) fn paint_completion(
        &self,
        commands: &mut Vec<PaintCommand>,
        size: Size,
        anchor: Point,
    ) {
        if self.editor_area.completion_items.is_empty() {
            return;
        }
        let visible = self
            .editor_area
            .completion_items
            .len()
            .min(COMPLETION_VISIBLE_ROWS);
        let mut surface = Popup::new(COMPLETION_POPUP_ID).with_padding(COMPLETION_POPUP_PADDING);
        surface.set_content_size(Size::new(
            COMPLETION_POPUP_WIDTH,
            visible as f32 * COMPLETION_ROW_HEIGHT,
        ));
        surface.layout(
            &self.layout_context(),
            Rect::new(0.0, 0.0, size.width, size.height),
        );
        surface.open_at(anchor);
        let mut popup_paint = self.paint_context();
        surface.paint(&mut popup_paint);
        // As linhas são do anfitrião, que é quem possui a lista: ela é a mesma
        // instância que recebeu o clique, e não uma cópia montada para desenhar.
        if let Some(list) = self.host.widget(COMPLETION_LIST_ID) {
            list.paint(&mut popup_paint);
        }
        commands.extend(popup_paint.into_commands());
    }
}
