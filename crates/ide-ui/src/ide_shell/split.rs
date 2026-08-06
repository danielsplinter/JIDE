//! A divisão do editor: dois editores lado a lado. Ver a `28`.
//!
//! O que está aqui é composição, e não desenho: quem reparte a área é o
//! `SplitPane` da biblioteca, quem mostra o texto é o `EditorPane` e quem mostra
//! as abas é o `Tabs`. A IDE diz **quais documentos** cada lado mostra e **onde**
//! cada componente vai.

use super::*;
use crate::editor::Divisao;

impl IdeShell {
    /// Se a área do editor está dividida em dois.
    #[must_use]
    pub fn is_split(&self) -> bool {
        self.editor_area.divisao.is_some()
    }

    /// Divide a área, pondo à direita o documento da aba escolhida.
    ///
    /// O documento **continua à esquerda**: dividir é ver a mesma coisa de dois
    /// lugares, e não mudar de lugar. Como os dois lados olham o mesmo
    /// documento, editar de um aparece no outro — é o mesmo texto, e não uma
    /// cópia.
    ///
    /// Dividir de novo, com a área já dividida, acrescenta a aba à direita em
    /// vez de criar uma terceira coluna.
    pub(super) fn dividir_a_direita(&mut self, documento: DocumentId) {
        if self.editor_area.session.document(documento).is_none() {
            return;
        }
        let esquerda = self
            .editor_area
            .session
            .active_id()
            .unwrap_or(documento);
        match self.editor_area.divisao.as_mut() {
            Some(divisao) => {
                if !divisao.abas.contains(&documento) {
                    divisao.abas.push(documento);
                }
                divisao.ativa = documento;
            }
            None => {
                self.editor_area.divisao = Some(Divisao {
                    abas: vec![documento],
                    ativa: documento,
                    ativa_da_esquerda: esquerda,
                    pane: EditorPane::new(EditorCapabilities::full()),
                    focado: false,
                    // Dividir é um gesto sobre este lado: quem acabou de mandar
                    // um arquivo para cá está trabalhando aqui.
                    clicado: true,
                    painel: SplitPane::new(SPLIT_PANE_ID, SplitOrientation::Horizontal, 0.5),
                    tabs: Tabs::new(SPLIT_TABS_ID, Vec::new()).with_tab_width(TAB_WIDTH),
                });
            }
        }
        self.focar_a_direita();
    }

    /// Desfaz a divisão, devolvendo a área inteira à esquerda.
    pub(super) fn desfazer_a_divisao(&mut self) {
        let Some(divisao) = self.editor_area.divisao.take() else {
            return;
        };
        // O documento que a esquerda mostrava volta a ser o ativo: sem isto o
        // ativo continuaria sendo o da direita, que acabou de sair da tela.
        if self.editor_area.session.document(divisao.ativa_da_esquerda).is_some() {
            let _ = self
                .editor_area
                .session
                .activate(divisao.ativa_da_esquerda);
        }
    }

    /// A área inteira que os dois lados repartem: as abas e o editor.
    ///
    /// A divisa atravessa as duas faixas, e por isso o componente recebe as
    /// duas: uma divisa que só cortasse o texto deixaria as abas dizendo que a
    /// área é uma só.
    pub(super) fn split_region(&self, size: Size) -> Rect {
        let geometry = self.geometry();
        Rect::new(
            ACTIVITY_WIDTH + self.sidebar_width(size),
            TITLE_HEIGHT,
            geometry.editor_width,
            (geometry.editor_bottom - TITLE_HEIGHT).max(0.0),
        )
    }

    /// O painel repartidor já arranjado sobre a área de agora.
    ///
    /// Devolvido por valor, como os divisores das outras áreas: quem pergunta a
    /// geometria não precisa de `&mut`, e o estado do arrasto continua no campo.
    pub(super) fn split_panel_for(&self, size: Size) -> Option<SplitPane> {
        let divisao = self.editor_area.divisao.as_ref()?;
        let mut painel = divisao.painel.clone();
        painel.layout(&self.layout_context(), self.split_region(size));
        Some(painel)
    }

    /// As duas colunas: a da esquerda e a da direita.
    fn split_columns(&self, size: Size) -> Option<(Rect, Rect)> {
        let painel = self.split_panel_for(size)?;
        Some((painel.first(), painel.second()))
    }

    /// A faixa de abas de um lado, dentro da coluna dele.
    fn tabs_rect(coluna: Rect) -> Rect {
        Rect::new(
            coluna.origin.x,
            TITLE_HEIGHT,
            coluna.size.width,
            TAB_HEIGHT,
        )
    }

    /// A área de texto de um lado, dentro da coluna dele.
    fn editor_rect(coluna: Rect, geometry: &crate::layout::Geometry) -> Rect {
        Rect::new(
            coluna.origin.x,
            geometry.content_top,
            coluna.size.width,
            geometry.editor_height,
        )
    }

    /// A área de texto da esquerda, quando há divisão.
    pub(super) fn left_editor_rect(&self, size: Size) -> Option<Rect> {
        let (esquerda, _) = self.split_columns(size)?;
        Some(Self::editor_rect(esquerda, &self.geometry()))
    }

    /// A coluna inteira da esquerda: a faixa de abas e o texto.
    pub(super) fn left_column(&self, size: Size) -> Option<Rect> {
        let (esquerda, _) = self.split_columns(size)?;
        Some(esquerda)
    }

    /// A coluna inteira da direita.
    pub(super) fn right_column(&self, size: Size) -> Option<Rect> {
        let (_, direita) = self.split_columns(size)?;
        Some(direita)
    }

    /// Se o ponteiro está sobre a divisa dos dois editores — ou arrastando-a.
    ///
    /// Quem troca o desenho do ponteiro é a janela, e ela precisa de uma
    /// pergunta só: a resposta inclui o arrasto porque a divisa continua sendo o
    /// alvo do gesto mesmo quando o ponteiro já saiu de cima dela.
    #[must_use]
    pub fn split_divider_hover(&self, point: Point, size: Size) -> bool {
        self.split_panel_for(size)
            .is_some_and(|painel| painel.is_dragging() || painel.divider().contains(point))
    }

    /// Se é o painel da direita que tem o foco agora.
    pub(super) fn split_focado(&self) -> bool {
        self.editor_area
            .divisao
            .as_ref()
            .is_some_and(|divisao| divisao.focado)
    }

    /// Abre um documento no painel da direita, quando é ele que tem o foco.
    ///
    /// Devolve `true` quando o documento foi para lá. É o que faz clicar num
    /// arquivo do Explorer abrir a aba **no painel em que se está olhando**, em
    /// vez de sempre à esquerda.
    pub(super) fn abrir_no_split(&mut self, documento: DocumentId) -> bool {
        let Some(divisao) = self.editor_area.divisao.as_mut() else {
            return false;
        };
        // **O último clique, e não a passagem do ponteiro.** O caminho do mouse
        // até o Explorer atravessa o painel da esquerda, e essa travessia não
        // significa que se deixou de trabalhar na direita.
        if !divisao.clicado {
            // A esquerda recebe: o foco vai junto com o arquivo aberto, senão
            // ele apareceria num painel e o cursor piscaria no outro.
            let direita = divisao.ativa;
            self.focar_a_esquerda();
            if let Some(divisao) = self.editor_area.divisao.as_mut() {
                // O que a direita mostrava continua o mesmo — a abertura não é
                // dela —, e a esquerda passa a mostrar o que acabou de abrir.
                divisao.ativa = direita;
                divisao.ativa_da_esquerda = documento;
            }
            // `focar_a_esquerda` reativa o documento que a esquerda mostrava
            // antes, e agora ela mostra outro: o que acabou de ser aberto.
            let _ = self.editor_area.session.activate(documento);
            return false;
        }
        let esquerda = divisao.ativa_da_esquerda;
        if !divisao.abas.contains(&documento) {
            divisao.abas.push(documento);
        }
        divisao.ativa = documento;
        self.focar_a_direita();
        if let Some(divisao) = self.editor_area.divisao.as_mut() {
            divisao.ativa_da_esquerda = esquerda;
            divisao.pane.set_cursor(0);
        }
        true
    }

    /// A faixa de abas da esquerda, quando há divisão.
    pub(super) fn left_tabs_rect(&self, size: Size) -> Option<Rect> {
        let (esquerda, _) = self.split_columns(size)?;
        Some(Self::tabs_rect(esquerda))
    }

    /// A área de texto da direita.
    pub(super) fn right_editor_rect(&self, size: Size) -> Option<Rect> {
        let (_, direita) = self.split_columns(size)?;
        Some(Self::editor_rect(direita, &self.geometry()))
    }

    /// A faixa de abas da direita.
    pub(super) fn right_tabs_rect(&self, size: Size) -> Option<Rect> {
        let (_, direita) = self.split_columns(size)?;
        Some(Self::tabs_rect(direita))
    }

    /// Põe o foco no lado direito e faz o documento dele ser o ativo.
    ///
    /// # Os painéis trocam de lugar
    ///
    /// `editor_area.pane` é **sempre o painel do lado com foco**, e a divisão
    /// guarda o outro. Trocar o foco troca os dois de lugar.
    ///
    /// Parece indireto e é o contrário: digitar, apagar, indentar, mover o
    /// cursor, colar, buscar e rolar passam por `editor_area.pane` em duas dúzias
    /// de lugares. Se o painel dependesse do foco em cada um deles, bastava
    /// esquecer um para o cursor andar no painel que ninguém está olhando — e
    /// esquecer um, entre vinte e quatro, é questão de tempo. Com a troca, quem
    /// escreve continua escrevendo "o painel", e ele já é o certo.
    fn focar_a_direita(&mut self) {
        let ativo = self.editor_area.session.active_id();
        let editor_area = &mut self.editor_area;
        let Some(divisao) = editor_area.divisao.as_mut() else {
            return;
        };
        if !divisao.focado {
            divisao.ativa_da_esquerda = ativo.unwrap_or(divisao.ativa_da_esquerda);
            std::mem::swap(&mut editor_area.pane, &mut divisao.pane);
            divisao.focado = true;
        }
        let ativa = divisao.ativa;
        let _ = self.editor_area.session.activate(ativa);
        self.context.focus = ShellFocus::Editor;
    }

    /// Põe o foco no lado esquerdo e devolve a ele o documento ativo.
    fn focar_a_esquerda(&mut self) {
        let ativo = self.editor_area.session.active_id();
        let editor_area = &mut self.editor_area;
        let Some(divisao) = editor_area.divisao.as_mut() else {
            return;
        };
        if !divisao.focado {
            return;
        }
        divisao.ativa = ativo.unwrap_or(divisao.ativa);
        std::mem::swap(&mut editor_area.pane, &mut divisao.pane);
        divisao.focado = false;
        let esquerda = divisao.ativa_da_esquerda;
        if self.editor_area.session.document(esquerda).is_some() {
            let _ = self.editor_area.session.activate(esquerda);
        }
        self.context.focus = ShellFocus::Editor;
    }

    /// O documento que o lado **esquerdo** mostra.
    ///
    /// Sem divisão é o ativo da sessão. Com a direita em foco, o ativo da sessão
    /// é o dela: perguntar por ele aqui acenderia, na faixa da esquerda, a aba
    /// de um arquivo que quem clicou abriu do outro lado.
    pub(super) fn left_active_document(&self) -> Option<DocumentId> {
        match self.editor_area.divisao.as_ref() {
            Some(divisao) if divisao.focado => Some(divisao.ativa_da_esquerda),
            _ => self.editor_area.session.active_id(),
        }
    }

    /// A área do lado que **não** tem o foco — a do painel que a divisão guarda.
    pub(super) fn other_editor_rect(&self, size: Size) -> Option<Rect> {
        if self.split_focado() {
            self.left_editor_rect(size)
        } else {
            self.right_editor_rect(size)
        }
    }

    /// O documento mostrado pelo lado que não tem o foco.
    fn other_document(&self) -> Option<DocumentId> {
        let divisao = self.editor_area.divisao.as_ref()?;
        Some(if divisao.focado {
            divisao.ativa_da_esquerda
        } else {
            divisao.ativa
        })
    }

    /// A faixa de abas da direita, remontada a partir dos documentos dela.
    ///
    /// Como a da esquerda: a verdade são os documentos, e a apresentação é
    /// refeita a cada quadro. O estado de interação atravessa a troca, e é por
    /// isso que a aba sob o ponteiro continua destacada.
    pub(super) fn sync_split_tabs(&mut self) {
        let Some(divisao) = self.editor_area.divisao.as_ref() else {
            return;
        };
        let items = divisao
            .abas
            .iter()
            .filter_map(|id| self.editor_area.session.document(*id))
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
        let mut tabs = Tabs::new(SPLIT_TABS_ID, items).with_tab_width(TAB_WIDTH);
        tabs.set_active_id(divisao.ativa.0);
        let Some(divisao) = self.editor_area.divisao.as_mut() else {
            return;
        };
        tabs.restore_interaction(divisao.tabs.interaction());
        divisao.tabs = tabs;
    }

    /// Alinha o painel da direita ao documento que ele mostra.
    ///
    /// O mesmo que a esquerda faz, com dois cuidados: o documento é o **dele**,
    /// e não o ativo da sessão, e ele só se desenha com foco quando o foco é
    /// deste lado — dois cursores piscando ao mesmo tempo diriam que os dois
    /// recebem o que se digita.
    pub(super) fn sync_split_pane(&mut self, size: Size) {
        let (Some(bounds), Some(id)) = (self.other_editor_rect(size), self.other_document()) else {
            return;
        };
        // Este é o painel de quem **não** tem o foco: ele nunca se desenha com
        // cursor aceso. Dois cursores piscando diriam que os dois recebem o que
        // se digita.
        let focado = false;
        let Some(document) = self.editor_area.session.document(id) else {
            return;
        };
        let (revision, path) = (document.buffer.revision(), document.path.clone());
        let decorations = self.editor_decorations(&path);
        let focused = focado;
        let context = self.layout_context();
        let syntax = self
            .editor_area
            .syntax_spans
            .get(&id)
            .filter(|cached| cached.version == revision)
            .map(|cached| (cached.version, cached.spans.clone()));
        let Some(document) = self.editor_area.session.document(id) else {
            return;
        };
        let buffer = document.buffer.clone();
        let Some(divisao) = self.editor_area.divisao.as_mut() else {
            return;
        };
        divisao.pane.set_bounds(bounds);
        divisao.pane.set_source(id.0);
        let vista = syntax.as_ref().map(|(version, spans)| SyntaxView {
            version: *version,
            spans,
        });
        divisao
            .pane
            .sync(&context, &buffer, vista, decorations, focused);
    }

    /// Menu de contexto sobre a faixa de abas.
    ///
    /// A aba sob o ponteiro é quem o menu vai dividir, e por isso ela é
    /// guardada: o menu devolve um comando, e comando não carrega alvo. Fora de
    /// uma aba não há menu — um menu que fala de "esta aba" sem aba nenhuma
    /// prometeria uma ação que não tem sobre o que agir.
    pub(super) fn tab_context_menu(&mut self, point: Point, size: Size) {
        let Some(documento) = self.tab_at(point, size) else {
            return;
        };
        self.explorer.context_menu_tab = Some(documento);
        self.explorer
            .context_menu
            .set_entries(crate::menus::tab_entries());
        self.explorer.context_menu.layout(
            &self.layout_context(),
            Rect::new(0.0, 0.0, size.width, size.height),
        );
        self.explorer.context_menu.open_at(point);
    }

    /// Qual documento está sob o ponteiro, em qualquer uma das faixas de abas.
    fn tab_at(&self, point: Point, size: Size) -> Option<DocumentId> {
        if let Some(rect) = self.right_tabs_rect(size)
            && rect.contains(point)
        {
            let indice = ((point.x - rect.origin.x) / TAB_WIDTH).floor() as usize;
            return self
                .editor_area
                .divisao
                .as_ref()
                .and_then(|divisao| divisao.abas.get(indice).copied());
        }
        let esquerda = ACTIVITY_WIDTH + self.sidebar_width(size);
        let indice = ((point.x - esquerda) / TAB_WIDTH).floor() as usize;
        self.editor_area
            .session
            .tabs()
            .nth(indice)
            .map(|documento| documento.id)
    }

    /// Desenha o lado direito: a faixa de abas, o texto e a divisa.
    ///
    /// Nenhum comando de pintura nasce aqui — os três são componentes da
    /// biblioteca, e o que esta função faz é dizer onde cada um está e pedir que
    /// se desenhem. O recorte é da IDE porque a área é dela.
    pub(super) fn paint_split(&mut self, size: Size) -> Vec<PaintCommand> {
        let (Some(abas), Some(texto), Some(painel)) = (
            self.right_tabs_rect(size),
            self.other_editor_rect(size),
            self.split_panel_for(size),
        ) else {
            return Vec::new();
        };
        let mut commands = Vec::new();
        self.sync_split_tabs();
        self.sync_split_pane(size);
        let context = self.layout_context();
        let mut pintura = self.paint_context();
        if let Some(divisao) = self.editor_area.divisao.as_mut() {
            divisao.tabs.layout(&context, abas);
            divisao.tabs.paint(&mut pintura);
        }
        commands.extend(pintura.into_commands());
        commands.push(PaintCommand::PushClip(texto));
        let mut pintura = self.paint_context();
        if let Some(divisao) = self.editor_area.divisao.as_ref() {
            divisao.pane.paint(&mut pintura);
        }
        commands.extend(pintura.into_commands());
        commands.push(PaintCommand::PopClip);
        // A divisa por último: ela fica **sobre** os dois lados, e é a única
        // coisa que atravessa a fronteira entre eles.
        let mut pintura = self.paint_context();
        painel.paint(&mut pintura);
        commands.extend(pintura.into_commands());
        commands
    }

    /// Ponteiro sobre a área dividida: a divisa se destaca, e o arrasto a move.
    ///
    /// Devolve `true` quando o gesto foi dela — em arrasto, o que está embaixo
    /// não deve receber um clique que começou na divisa.
    pub(super) fn split_pointer_move(&mut self, point: Point, size: Size) -> bool {
        let Some(mut painel) = self.split_panel_for(size) else {
            return false;
        };
        let mut context = EventContext::default();
        let resultado = painel.event(
            &mut context,
            &UiEvent::PointerMove(primary_pointer(point)),
        );
        let arrastando = painel.is_dragging();
        if let Some(divisao) = self.editor_area.divisao.as_mut() {
            divisao.painel = painel;
        }
        // **O foco segue o ponteiro.** Passar sobre um dos lados o torna o lado
        // ativo, e a partir daí tudo acontece nele: clique, rolagem, digitação.
        // Sem isto, olhar um lado e digitar no outro seria o comportamento
        // normal — e é exatamente o que confunde quem divide a tela.
        //
        // Só enquanto ninguém arrasta: no meio de um arrasto o ponteiro
        // atravessa os dois lados, e trocar o foco ali seria trocá-lo por causa
        // de um gesto que não é sobre foco nenhum.
        if !arrastando {
            if self.right_column(size).is_some_and(|coluna| coluna.contains(point)) {
                self.focar_a_direita();
            } else if self.left_column(size).is_some_and(|coluna| coluna.contains(point)) {
                self.focar_a_esquerda();
            }
        }
        // A faixa de abas da direita também precisa do movimento: é por ele que
        // ela sabe qual aba está sob o ponteiro — e é o que faz aparecer o
        // botão de fechar de uma aba com alterações por gravar.
        if let Some(rect) = self.right_tabs_rect(size) {
            let context = self.layout_context();
            if let Some(divisao) = self.editor_area.divisao.as_mut() {
                divisao.tabs.layout(&context, rect);
                let _ = divisao.tabs.event(
                    &mut EventContext::default(),
                    &UiEvent::PointerMove(primary_pointer(point)),
                );
            }
        }
        arrastando && resultado != EventResult::Ignored
    }

    /// Clique dentro da área dividida. Devolve `true` quando ele foi daqui.
    ///
    /// A ordem importa: as abas da direita primeiro, depois o texto da direita,
    /// e por último a esquerda — o que decide é a coluna em que o ponto caiu, e
    /// não a ordem em que os componentes foram criados.
    pub(super) fn split_pointer_down(&mut self, point: Point, size: Size) -> bool {
        let (Some(abas), Some(texto)) = (self.right_tabs_rect(size), self.right_editor_rect(size))
        else {
            return false;
        };
        // A divisa primeiro: ela fica sobre a fronteira das duas colunas, e um
        // ponto ali pertence a ela, não ao painel que começa logo depois.
        if let Some(mut painel) = self.split_panel_for(size) {
            let mut context = EventContext::default();
            let resultado = painel.event(
                &mut context,
                &UiEvent::PointerDown(primary_pointer(point)),
            );
            let pegou = painel.is_dragging();
            if let Some(divisao) = self.editor_area.divisao.as_mut() {
                divisao.painel = painel;
            }
            if pegou && resultado != EventResult::Ignored {
                return true;
            }
        }
        if abas.contains(point) {
            self.marcar_clique(true);
            self.split_tabs_pointer_down(point, size);
            return true;
        }
        if texto.contains(point) {
            self.marcar_clique(true);
            self.focar_a_direita();
            // **E o clique segue adiante**, para o mesmo caminho que trata o
            // clique no editor de sempre. Ele já opera sobre o painel da frente
            // e sobre a área dele, e a essa altura os dois são os deste lado.
            //
            // Antes daqui o clique era entregue ao painel guardado na divisão —
            // que, depois da troca de foco, é o do **outro** lado. O cursor ia
            // parar no editor da esquerda, e clicar no da direita não movia
            // nada. Um caminho só para os dois painéis é o que impede isso de
            // voltar.
            return false;
        }
        // **Só dentro da coluna da esquerda.** Este caminho recebe todo clique da
        // janela — o do Explorer, o do terminal, o da barra de menus —, e tratar
        // todos eles como "cliquei na esquerda" fazia o clique no Explorer
        // roubar o lado que ia receber o arquivo. Era o defeito: dividir,
        // escolher um arquivo, e vê-lo abrir do outro lado.
        if self
            .left_column(size)
            .is_some_and(|coluna| coluna.contains(point))
        {
            self.marcar_clique(false);
            self.focar_a_esquerda();
        }
        false
    }

    /// Registra de que lado foi o último clique dentro da área dividida.
    fn marcar_clique(&mut self, na_direita: bool) {
        if let Some(divisao) = self.editor_area.divisao.as_mut() {
            divisao.clicado = na_direita;
        }
    }

    /// Clique na faixa de abas da direita: escolher uma, ou fechá-la.
    fn split_tabs_pointer_down(&mut self, point: Point, size: Size) {
        let Some(rect) = self.right_tabs_rect(size) else {
            return;
        };
        // **Sem remontar a faixa aqui.** Remontá-la troca a instância, e com ela
        // se vai o que ela sabia do ponteiro — inclusive que ele está sobre a
        // aba, que é o que revela o botão de fechar de um arquivo com alterações
        // por gravar. Quem a remonta é o quadro, antes de desenhá-la.
        let context = self.layout_context();
        let mut context_evento = EventContext::default();
        let Some(divisao) = self.editor_area.divisao.as_mut() else {
            return;
        };
        divisao.tabs.layout(&context, rect);
        // A faixa de abas age ao **soltar**, e não ao pressionar: é ali que ela
        // distingue fechar de ativar. Entregar só a pressão fazia o clique não
        // ter efeito nenhum.
        let _ = divisao.tabs.event(
            &mut context_evento,
            &UiEvent::PointerDown(primary_pointer(point)),
        );
        let resultado = divisao.tabs.event(
            &mut context_evento,
            &UiEvent::PointerUp(primary_pointer(point)),
        );
        match resultado {
            EventResult::Action(WidgetAction::TabSelected { tab, .. }) => {
                divisao.ativa = DocumentId(tab);
                divisao.pane.set_cursor(0);
                self.focar_a_direita();
            }
            EventResult::Action(WidgetAction::TabClosed { tab, .. }) => {
                divisao.abas.retain(|id| id.0 != tab);
                match divisao.abas.first().copied() {
                    Some(primeira) => {
                        divisao.ativa = primeira;
                        self.focar_a_direita();
                    }
                    // Sem aba nenhuma não há painel: a divisão se desfaz em vez
                    // de deixar uma metade vazia na tela.
                    None => self.desfazer_a_divisao(),
                }
            }
            _ => {}
        }
    }

    /// Tira dos dois lados um documento que deixou de existir.
    ///
    /// Fechar uma aba à esquerda fecha o documento, e o da direita apontaria
    /// para o que não está mais aberto.
    pub(super) fn split_forget(&mut self, documento: DocumentId) {
        let Some(divisao) = self.editor_area.divisao.as_mut() else {
            return;
        };
        divisao.abas.retain(|id| *id != documento);
        if divisao.ativa == documento {
            match divisao.abas.first().copied() {
                Some(primeira) => divisao.ativa = primeira,
                None => {
                    self.desfazer_a_divisao();
                    return;
                }
            }
        }
        if divisao.ativa_da_esquerda == documento {
            let sobrou = self.editor_area.session.active_id();
            if let Some(divisao) = self.editor_area.divisao.as_mut()
                && let Some(id) = sobrou
            {
                divisao.ativa_da_esquerda = id;
            }
        }
    }
}
