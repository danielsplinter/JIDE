//! A árvore de arquivos, o menu dela e a barra lateral.

use super::*;

impl IdeShell {
    /// Abre a pasta e todas as que levam até ela.
    ///
    /// Criar algo dentro de uma pasta fechada esconde o que acabou de nascer;
    /// revelar o caminho é o que faz o resultado aparecer.
    pub fn reveal_in_explorer(&mut self, path: &Path) {
        for ancestor in path.ancestors() {
            if ancestor.starts_with(&self.explorer.workspace.path) {
                self.explorer.expanded.insert(ancestor.to_path_buf());
            }
        }
        debug_assert!(
            path.extension().is_none() || path.is_dir(),
            "revelar espera uma pasta: um arquivo entre os expandidos pediria              leitura que nunca chega"
        );
        // Revelar um caminho abre pastas que podem nunca ter sido lidas.
        self.request_expanded_directories();
        self.sync_explorer_tree();
    }

    pub fn is_expanded(&self, path: &Path) -> bool {
        self.explorer.is_expanded(path)
    }

    pub const fn sidebar_resizing(&self) -> bool {
        self.explorer.splitter.is_dragging()
    }

    /// Se o ponteiro está sobre o divisor da lateral — ou arrastando-o.
    ///
    /// É o que a janela pergunta para trocar o desenho do ponteiro. **Sobre**,
    /// e não só arrastando: era assim antes, e um divisor que só anuncia que se
    /// move depois que alguém o moveu não anuncia nada.
    ///
    /// A resposta inclui o arrasto em curso porque num arrasto rápido o ponteiro
    /// sai da frente da linha, e a seta piscaria de volta no meio do gesto.
    #[must_use]
    pub fn sidebar_divider_hover(&self, point: Point, size: Size) -> bool {
        if self.explorer.recolhido {
            return false;
        }
        self.sidebar_resizing() || self.sidebar_splitter_for(size).hit_area().contains(point)
    }

    /// Área da árvore de arquivos.
    pub(super) fn explorer_tree_rect(&self, size: Size) -> Rect {
        let geo = self.geometry();
        Rect::new(
            ACTIVITY_WIDTH,
            EXPLORER_TOP,
            self.sidebar_width(size),
            (geo.content_bottom - 12.0 - EXPLORER_TOP).max(0.0),
        )
    }

    /// Espelha na árvore o que a IDE considera expandido.
    ///
    /// A expansão continua sendo do shell porque ela é indexada por caminho e
    /// serve a mais gente do que ao desenho; a árvore recebe as identidades
    /// correspondentes.
    pub(super) fn sync_explorer_tree(&mut self) {
        let ids: Vec<u64> = self
            .explorer
            .expanded
            .iter()
            .map(|path| explorer_id(path))
            .collect();
        self.explorer.tree.set_expanded(ids);
    }

    /// Expande, seleciona e revela no Explorer o arquivo da aba ativa.
    ///
    /// A restauração abre as abas antes do primeiro frame. Fazer a reconciliação
    /// aqui deixa a árvore nascer no mesmo documento do editor, em vez de exigir
    /// que o usuário repita toda a navegação manualmente.
    pub(super) fn sync_explorer_to_active(&mut self) {
        let Some(path) = self.active_document_path() else {
            self.explorer.tree.set_selected(None);
            return;
        };
        let target = explorer_id(&path);
        if self.explorer_path_for(target).is_none() {
            // A árvore é rasa: o caminho pode existir em disco e ainda não ter
            // sido lido. Revelar a **pasta** dele — o arquivo em si nunca tem
            // filhos, e pedi-lo faria o pedido se repetir para sempre.
            if let Some(pasta) = path.parent() {
                self.reveal_in_explorer(pasta);
            }
            return;
        }

        for ancestor in path.ancestors().skip(1) {
            if !ancestor.starts_with(&self.explorer.workspace.path) {
                break;
            }
            self.explorer.expanded.insert(ancestor.to_path_buf());
        }
        self.request_expanded_directories();
        self.sync_explorer_tree();
        self.explorer.tree.set_selected(Some(target));

        let expanded = self
            .explorer
            .expanded
            .iter()
            .map(|path| explorer_id(path))
            .collect::<HashSet<_>>();
        if let Some(row) = visible_tree_row(
            &self.explorer.workspace,
            &self.catalog.source_root_names,
            &expanded,
            target,
        ) {
            // **Rolar só quando precisa.** Se a linha já está à vista, mexer na
            // rolagem move a árvore debaixo de quem acabou de clicar nela: o
            // arquivo clicado salta para o alto da lista e tudo o mais desliza
            // junto, sem que ninguém tenha pedido.
            //
            // Duas linhas de contexto ajudam a reconhecer o pacote pai quando a
            // rolagem de fato acontece.
            if !self.explorer_row_visible(row) {
                self.explorer.scroll_line = row.saturating_sub(2);
            }
        }
    }

    /// Se uma linha da árvore está dentro da parte visível do painel.
    ///
    /// Mede pela altura do último desenho: é a única que existe antes de o
    /// próximo quadro acontecer, e é a que o usuário está olhando.
    fn explorer_row_visible(&self, row: usize) -> bool {
        let altura = self.explorer_tree_rect(self.context.last_size).size.height;
        let cabem = (altura / EXPLORER_ROW_HEIGHT).floor().max(1.0) as usize;
        let primeira = self.explorer.scroll_line;
        // A última linha inteira, e não a que aparece pela metade: revelar algo
        // cortado ao meio é revelar pela metade.
        row >= primeira && row < primeira + cabem
    }

    /// Posiciona a árvore de acordo com as barras de rolagem da janela.
    ///
    /// Posiciona a **de verdade**, e não uma cópia dela. A cópia existia porque
    /// esta função recebia `&self`; agora que as linhas são componentes, copiar
    /// deixou de ser possível — um componente não se duplica. E a cópia nunca
    /// foi boa ideia: ela morria no fim da chamada levando junto o destaque sob
    /// o ponteiro, que é a mesma razão pela qual o evento já ia à de verdade.
    pub(super) fn place_explorer_tree(&mut self, size: Size) {
        let context = self.layout_context();
        let bounds = self.explorer_tree_rect(size);
        let offset = Point::new(
            self.explorer.scroll_x,
            self.explorer.scroll_line as f32 * EXPLORER_ROW_HEIGHT,
        );
        let tree = &mut self.explorer.tree;
        // O deslocamento **antes** do posicionamento: as células são colocadas
        // durante ele, e com a ordem trocada elas nasceriam no deslocamento do
        // quadro anterior. A árvore de rótulos não se importava porque desenhava
        // o texto já deslocado na hora de pintar; a de componentes se importa.
        tree.set_scroll_offset(offset);
        tree.layout(&context, bounds);
    }

    /// Entrega o gesto à árvore **de verdade**, e não a uma cópia.
    ///
    /// Posicionar exige acesso mutável, e a pintura recebe `&self` — foi daí que
    /// nasceu o clone. Só que o clone morre no fim da chamada, levando junto o
    /// destaque sob o ponteiro e a marca de que o gesto começou naquela linha.
    /// Quem recebe evento tem de ser quem sobrevive ao quadro.
    pub(super) fn explorer_tree_event(&mut self, point: Point, size: Size) -> Option<u64> {
        let context = self.layout_context();
        let bounds = self.explorer_tree_rect(size);
        let offset = Point::new(
            self.explorer.scroll_x,
            self.explorer.scroll_line as f32 * EXPLORER_ROW_HEIGHT,
        );
        let tree = &mut self.explorer.tree;
        tree.set_scroll_offset(offset);
        tree.layout(&context, bounds);
        tree.event(
            &mut EventContext::default(),
            &UiEvent::PointerDown(primary_pointer(point)),
        );
        tree.selected()
    }

    pub(super) fn explorer_path_for(&self, id: u64) -> Option<(PathBuf, bool)> {
        fn visit(node: &FileNode, id: u64) -> Option<(PathBuf, bool)> {
            if explorer_id(&node.path) == id {
                return Some((node.path.clone(), node.is_directory));
            }
            node.children.iter().find_map(|child| visit(child, id))
        }
        visit(&self.explorer.workspace, id)
    }

    /// Divisor da barra lateral posicionado pelo layout atual.
    ///
    /// A barra lateral é limitada pela largura mínima dela e pela do editor; o
    /// terminal, pela altura mínima dele e pelo espaço que o editor precisa
    /// manter. São limites em pontos, não proporções.
    pub(super) fn sidebar_splitter_for(&self, size: Size) -> Splitter {
        let geometry = self.geometry();
        let mut splitter = self.explorer.splitter.clone();
        splitter.layout(
            &self.layout_context(),
            Rect::new(
                0.0,
                TITLE_HEIGHT,
                size.width,
                (geometry.content_bottom - TITLE_HEIGHT).max(0.0),
            ),
        );
        splitter.set_range(
            ACTIVITY_WIDTH + SIDEBAR_MIN_WIDTH,
            ACTIVITY_WIDTH + (size.width - 320.0).max(SIDEBAR_MIN_WIDTH),
        );
        splitter.set_position(ACTIVITY_WIDTH + self.sidebar_width(size));
        splitter
    }

    /// Pede à aplicação os filhos de tudo o que está expandido.
    ///
    /// Da raiz para as folhas: carregar um pai substitui a lista de filhos dele,
    /// e fazer isso **depois** de carregar um neto apagaria o neto. A ordem por
    /// profundidade é o que garante que cada carga só acrescente.
    pub(super) fn request_expanded_directories(&mut self) {
        let mut pastas: Vec<PathBuf> = self
            .explorer
            .expanded
            .iter()
            .filter(|pasta| self.directory_needs_children(pasta))
            .cloned()
            .collect();
        pastas.sort_by_key(|caminho| caminho.components().count());
        for pasta in pastas {
            self.explorer.requested.insert(pasta.clone());
            self.commands
                .push(ApplicationCommand::LoadDirectory(pasta));
        }
    }

    /// Se a pasta ainda não teve os filhos lidos.
    ///
    /// Pasta vazia e pasta não lida têm a mesma forma na árvore — distinguir ali
    /// custaria um campo em todo lugar que monta um nó. Quem separa as duas é
    /// `requested`: perguntar uma vez por pasta responde a pergunta de vez, e
    /// uma pasta que veio vazia veio vazia mesmo.
    fn directory_needs_children(&self, path: &Path) -> bool {
        if self.explorer.requested.contains(path) {
            return false;
        }
        fn procurar<'arvore>(node: &'arvore FileNode, path: &Path) -> Option<&'arvore FileNode> {
            if node.path == path {
                return Some(node);
            }
            node.children
                .iter()
                .find_map(|filho| procurar(filho, path))
        }
        // Ausente da árvore também precisa: os ancestrais dela vêm antes, e a
        // ordem por profundidade garante que, quando este pedido for atendido, o
        // pai já esteja lá.
        procurar(&self.explorer.workspace, path).is_none_or(|node| node.children.is_empty())
    }

    /// Guarda os níveis de um caminho, da raiz para a folha.
    ///
    /// Em ordem, e por isso cada inserção encontra o pai já na árvore. É o que
    /// substitui pedir pasta a pasta, que perdia o nível fundo por ele chegar
    /// antes do pai.
    pub fn insert_path_children(&mut self, niveis: Vec<(PathBuf, Vec<FileNode>)>) {
        // Antes de inserir: é a diferença entre "faltava e chegou" e "já estava
        // lá", e depois da inserção as duas se parecem.
        let ativo_faltava = self.active_document_missing_from_tree();
        let mut mudou = false;
        for (pasta, filhos) in niveis {
            // Veio, logo não se pergunta mais: uma leitura por caminho traz
            // vários níveis, e nenhum deles precisa ser pedido outra vez.
            self.explorer.requested.insert(pasta.clone());
            mudou |= self.insert_children_at(&pasta, filhos);
        }
        if !mudou {
            return;
        }
        self.expand_single_child_chains();
        self.explorer
            .rebuild_items(&self.catalog.source_root_names, &self.declaration_kinds);
        self.sync_explorer_tree();
        // A árvore mudou, e o documento ativo pode **finalmente** existir nela —
        // era esse o caso que isto veio resolver: a árvore é lida por partes, e
        // o arquivo aberto só aparece quando a pasta dele chega.
        //
        // **Só nesse caso.** Reconciliar a cada carga arrastava a árvore para o
        // arquivo da aba toda vez que alguém expandia um pacote: quem clicou
        // via a lista pular para outro lugar e a seleção mudar sozinha, sem
        // nada a ver com o que tinha acabado de abrir.
        if ativo_faltava {
            self.sync_explorer_to_active();
        }
    }

    /// Abre também os elos de uma cadeia de pasta única.
    ///
    /// A árvore junta `br`, `com` e `exemplo` numa linha só, e a identidade
    /// dessa linha é a do **último** diretório da cadeia. Quem clicou, porém,
    /// abriu o **primeiro** — era o único que existia na árvore quando o clique
    /// aconteceu.
    ///
    /// Sem isto, a linha juntada nasce fechada: os filhos chegaram, o nome
    /// virou `br.com.exemplo`, e ela continua pedindo mais um clique para
    /// mostrar o que já está em memória. Marcar a cadeia inteira faz o clique
    /// valer até onde há o que ver.
    fn expand_single_child_chains(&mut self) {
        fn descer(node: &FileNode, saida: &mut Vec<PathBuf>) {
            let mut atual = node;
            loop {
                let [unico] = atual.children.as_slice() else {
                    return;
                };
                if !unico.is_directory {
                    return;
                }
                saida.push(unico.path.clone());
                atual = unico;
            }
        }
        fn achar<'a>(node: &'a FileNode, path: &Path) -> Option<&'a FileNode> {
            if node.path == path {
                return Some(node);
            }
            node.children.iter().find_map(|filho| achar(filho, path))
        }
        let mut novos = Vec::new();
        for aberto in &self.explorer.expanded {
            if let Some(node) = achar(&self.explorer.workspace, aberto) {
                descer(node, &mut novos);
            }
        }
        self.explorer.expanded.extend(novos);
    }

    /// Se o documento ativo ainda não existe na árvore.
    ///
    /// Ele pode faltar por a árvore ser rasa: cada pasta é lida quando alguém a
    /// expande, e o arquivo aberto pode estar sob uma que ninguém abriu ainda.
    fn active_document_missing_from_tree(&self) -> bool {
        self.active_document_path()
            .is_some_and(|path| self.explorer_path_for(explorer_id(&path)).is_none())
    }

    /// Guarda os filhos que a aplicação leu para uma pasta expandida.
    fn insert_children_at(&mut self, path: &Path, children: Vec<FileNode>) -> bool {
        /// Mantém as subárvores já carregadas dos filhos que voltaram vazios.
        ///
        /// A leitura de um caminho traz **todos os níveis até ele**, cada um com
        /// os filhos imediatos — e filho imediato vem sem netos. Trocar a lista
        /// inteira pela recém-lida apagava o que já estava aberto **nos irmãos**:
        /// clicar em `resources` esvaziava `java`, e os pacotes que estavam à
        /// vista sumiam da tela.
        ///
        /// Uma leitura rasa do pai diz **quais** entradas existem; ela não diz
        /// que elas estão vazias por dentro, porque nunca olhou lá. Quem pode
        /// dizer isso é a leitura da própria pasta, e essa chega como um nível
        /// só dela.
        fn preservar_subarvores(antigos: &[FileNode], novos: &mut [FileNode]) {
            for novo in novos {
                if !novo.is_directory || !novo.children.is_empty() {
                    continue;
                }
                if let Some(antigo) = antigos
                    .iter()
                    .find(|antigo| antigo.is_directory && antigo.path == novo.path)
                {
                    novo.children.clone_from(&antigo.children);
                }
            }
        }
        fn inserir(node: &mut FileNode, path: &Path, children: &mut Option<Vec<FileNode>>) -> bool {
            if node.path == path {
                let Some(mut filhos) = children.take() else {
                    return false;
                };
                preservar_subarvores(&node.children, &mut filhos);
                // Mudou só se o conteúdo é outro. Responder "mudou" por ter
                // achado o nó faz a reconciliação da seleção pedir a mesma
                // leitura de novo, e nada garante que esse ciclo termine.
                if node.children == filhos {
                    return false;
                }
                node.children = filhos;
                return true;
            }
            node.children
                .iter_mut()
                .any(|filho| inserir(filho, path, children))
        }
        let mut children = Some(children);
        // Devolve se mudou algo: quem chama reconcilia uma vez, ao fim da
        // cadeia, e não a cada nível.
        inserir(&mut self.explorer.workspace, path, &mut children)
    }

    pub(super) fn sidebar_width(&self, size: Size) -> f32 {
        // Recolhido, o painel não ocupa nada, e o que sobra vai para o editor e
        // o terminal. A largura escolhida continua guardada: reabrir devolve a
        // que a pessoa tinha, e não a de fábrica.
        if self.explorer.recolhido {
            return 0.0;
        }
        self.explorer.sidebar_width.clamp(
            SIDEBAR_MIN_WIDTH,
            (size.width - 320.0).max(SIDEBAR_MIN_WIDTH),
        )
    }

    /// Os dois botões da barra de atividades, na ordem em que aparecem.
    ///
    /// Montados a cada quadro a partir do estado, como as abas: o nome do
    /// segundo diz o que o clique **vai fazer**, e ele muda quando o painel
    /// recolhe.
    pub(super) fn activity_buttons(&self) -> [Button; 3] {
        let painel = if self.sidebar_collapsed() {
            "Mostrar o Explorer"
        } else {
            "Esconder o Explorer"
        };
        [
            Button::icon(ACTIVITY_SEARCH_ID, Icon::Search, "Buscar")
                .with_command("activity.search"),
            Button::icon(ACTIVITY_SIDEBAR_ID, Icon::Panels, painel)
                .with_command("activity.sidebar"),
            Button::icon(ACTIVITY_GIT_ID, Icon::Branch, "Git")
                .with_command("activity.git"),
        ]
    }

    /// Onde cada botão da barra de atividades fica.
    ///
    /// Um lugar só para o desenho e para o clique: com a conta em dois lugares,
    /// clicar na borda de um faria uma coisa e o desenho mostraria outra.
    pub(super) fn activity_rect(id: WidgetId) -> Rect {
        let topo = if id == ACTIVITY_SEARCH_ID {
            TITLE_HEIGHT + 8.0
        } else if id == ACTIVITY_SIDEBAR_ID {
            TITLE_HEIGHT + 52.0
        } else {
            TITLE_HEIGHT + 88.0
        };
        Rect::new(12.0, topo, 24.0, 24.0)
    }

    /// Clique na barra de atividades. Devolve `true` quando foi de um botão.
    pub(super) fn activity_pointer_down(&mut self, point: Point, size: Size) -> bool {
        if point.x >= ACTIVITY_WIDTH || point.y < TITLE_HEIGHT {
            return false;
        }
        let escolhido = self
            .activity_buttons()
            .into_iter()
            .find(|botao| Self::activity_rect(botao.id()).contains(point))
            .map(|botao| botao.id());
        match escolhido {
            Some(id) if id == ACTIVITY_SEARCH_ID => {
                self.open_content_search();
                true
            }
            Some(id) if id == ACTIVITY_GIT_ID => {
                self.toggle_git();
                true
            }
            Some(_) => {
                self.toggle_sidebar();
                // A moldura mudou de largura, e quem perguntar a geometria em
                // seguida precisa dela já refeita.
                self.place_overlay(size);
                true
            }
            // A faixa consome o clique mesmo fora dos botões: ali não há
            // conteúdo para reagir embaixo.
            None => true,
        }
    }

    /// Se o painel do Explorer está recolhido.
    #[must_use]
    pub const fn sidebar_collapsed(&self) -> bool {
        self.explorer.recolhido
    }

    /// Mostra ou esconde o painel do Explorer.
    pub const fn toggle_sidebar(&mut self) {
        self.explorer.recolhido = !self.explorer.recolhido;
    }

    /// Clique secundário: abre o menu de contexto sobre o item do Explorer.
    ///
    /// Fora do Explorer o clique só dispensa um menu aberto. Enquanto não
    /// houver menu para as outras áreas, abrir um vazio prometeria ações que
    /// não existem.
    pub fn secondary_pointer_down(&mut self, point: Point, size: Size) {
        self.context_menu.close();
        // O clique secundário nunca escolhe da lista, então ele só a dispensa.
        self.clear_completions();
        if self.settings.is_open() {
            return;
        }
        let geometry = self.geometry();
        let editor_x = ACTIVITY_WIDTH + self.sidebar_width(size);
        // Sobre uma aba o menu fala da aba: dividir a area a partir dela.
        if point.y >= TITLE_HEIGHT && point.y < TITLE_HEIGHT + TAB_HEIGHT && point.x >= editor_x {
            self.tab_context_menu(point, size);
            return;
        }
        // No editor o menu fala do texto: copiar e colar.
        if point.x >= editor_x
            && point.x < editor_x + geometry.editor_width
            && point.y >= geometry.content_top
            && point.y < geometry.editor_bottom
        {
            self.context.focus = ShellFocus::Editor;
            let dentro_de_tipo = self.cursor_inside_type(point, size);
            self.context_menu.menu.set_entries(editor_menu_entries(
                self.editor_area.pane.selection_range().is_some(),
                self.debug_panel.view.attached,
                dentro_de_tipo,
            ));
            self.context_menu.menu.layout(
                &self.layout_context(),
                Rect::new(0.0, 0.0, size.width, size.height),
            );
            self.context_menu.menu.open_at(point);
            // O alvo é o editor: as ações daqui falam do cursor e da seleção, e
            // por isso não carregam caminho nenhum. Dizer isso é melhor do que
            // deixar o alvo vazio — vazio quer dizer "não há menu aberto".
            self.context_menu.alvo = Some(AlvoDoMenu::Editor);
            return;
        }
        if point.x < ACTIVITY_WIDTH || point.x >= editor_x || point.y < EXPLORER_TOP {
            return;
        }
        // Qual nó está sob o ponteiro é a árvore quem sabe: recuo, deslocamento
        // horizontal e virtualização são dela.
        let selecionado = self.explorer_tree_event(point, size);
        let Some((path, is_directory)) = selecionado.and_then(|id| self.explorer_path_for(id))
        else {
            return;
        };
        self.context.focus = ShellFocus::Explorer;
        self.explorer.tree.set_selected(Some(explorer_id(&path)));
        // O arquivo clicado, quando foi um: `target` abaixo é a pasta, porque é
        // nela que a criação acontece, mas renomear fala do arquivo.
        let arquivo = (!is_directory).then(|| path.clone());
        // O alvo é o diretório: clicando em um arquivo, é na pasta dele que a
        // criação acontece.
        let target = if is_directory {
            path
        } else {
            path.parent().map(Path::to_path_buf).unwrap_or(path)
        };
        self.context_menu.menu.set_entries(explorer_menu_entries(
            &target,
            &self.catalog.source_root_names,
            &self.catalog.new_item_templates,
            !is_directory,
        ));
        self.context_menu.menu.layout(
            &self.layout_context(),
            Rect::new(0.0, 0.0, size.width, size.height),
        );
        self.context_menu.menu.open_at(point);
        self.context_menu.alvo = Some(AlvoDoMenu::Explorer {
            pasta: target,
            arquivo,
        });
    }

    pub fn context_menu_open(&self) -> bool {
        self.context_menu.is_open()
    }

    /// Entrega o evento ao menu aberto e trata o comando escolhido.
    ///
    /// Devolve `true` quando o menu consumiu o evento — é o sinal de que o
    /// clique ou a tecla não devem seguir para o que está embaixo dele.
    pub(super) fn context_menu_event(&mut self, event: &UiEvent, size: Size) -> bool {
        if !self.context_menu.is_open() {
            return false;
        }
        self.context_menu.menu.layout(
            &self.layout_context(),
            Rect::new(0.0, 0.0, size.width, size.height),
        );
        let mut context = EventContext::default();
        let result = self.context_menu.menu.event(&mut context, event);
        if let EventResult::Action(WidgetAction::Command(command)) = &result {
            self.run_context_command(&command.0);
        }
        if !self.context_menu.is_open() {
            self.context_menu.alvo = None;
        }
        result != EventResult::Ignored
    }

    /// Entrega a tecla ao menu aberto.
    ///
    /// Separado do caminho do ponteiro porque navegar por teclado não depende
    /// de onde o menu foi desenhado, e assim não precisa do tamanho da janela.
    pub(super) fn context_menu_key(&mut self, key: &str, modifiers: Modifiers) -> bool {
        if !self.context_menu.is_open() {
            return false;
        }
        let mut context = EventContext::default();
        let result = self.context_menu.menu.event(
            &mut context,
            &UiEvent::KeyDown(KeyEvent {
                logical_key: key.to_owned(),
                repeat: false,
                modifiers,
            }),
        );
        if let EventResult::Action(WidgetAction::Command(command)) = &result {
            self.run_context_command(&command.0);
        }
        if !self.context_menu.is_open() {
            self.context_menu.alvo = None;
        }
        result != EventResult::Ignored
    }

    /// Executa o comando que o menu de contexto devolveu.
    ///
    /// Chamava-se `run_explorer_command` e tratava `editor.copy`, `editor.paste`
    /// e `editor.split.right`: quem procurava onde o Copiar era tratado não
    /// olhava no Explorer. O menu é de três áreas, e o nome agora diz isso.
    pub(super) fn run_context_command(&mut self, command: &str) {
        match command {
            "editor.split.right" => {
                if let Some(documento) = self.context_menu.take_aba() {
                    self.dividir_a_direita(documento);
                }
                return;
            }
            // A geração em si ainda não existe: o menu já escolhe o que gerar, e
            // dizer isso é melhor do que um clique que não faz nada.
            "editor.generate.getter" => {
                self.request_accessors(AccessorKind::Getter);
                return;
            }
            "editor.generate.setter" => {
                self.request_accessors(AccessorKind::Setter);
                return;
            }
            "editor.generate.accessors" => {
                self.request_accessors(AccessorKind::Both);
                return;
            }
            "editor.generate.constructor" => {
                self.request_accessors(AccessorKind::Constructor);
                return;
            }
            "editor.copy" => {
                self.copy_selection();
                return;
            }
            "editor.paste" => {
                self.paste_clipboard();
                return;
            }
            "debug.inspect" => {
                self.inspect_selection();
                return;
            }
            _ => {}
        }
        let Some(target) = self.context_menu.pasta().cloned() else {
            return;
        };
        if command == "explorer.rename" {
            if let Some(arquivo) = self.context_menu.arquivo().cloned() {
                self.request_rename(arquivo);
            }
            return;
        }
        if command == "explorer.new.folder" {
            self.context.status_message = format!("Nova pasta em {}", target.display());
            return;
        }
        let Some(template_id) = command.strip_prefix("explorer.new.") else {
            return;
        };
        let Some(template) = self
            .catalog
            .new_item_templates
            .iter()
            .find(|template| template.id.as_str() == template_id)
            .cloned()
        else {
            return;
        };
        let source_roots = self.catalog.source_root_names.clone();
        if let Some(refusal) =
            self.new_item
                .open(&mut self.host, template, &target, &source_roots)
        {
            self.context.status_message = refusal;
        }
    }

    pub(super) fn explorer_visible_lines(&self) -> usize {
        let geo = self.geometry();
        ((geo.content_bottom - 12.0 - EXPLORER_TOP) / EXPLORER_ROW_HEIGHT)
            .floor()
            .max(1.0) as usize
    }

    pub(super) fn explorer_horizontal_scrollbar_rect(&self, size: Size) -> Rect {
        let geo = self.geometry();
        Rect::new(
            ACTIVITY_WIDTH,
            geo.content_bottom - 12.0,
            self.sidebar_width(size),
            12.0,
        )
    }

    pub(super) fn explorer_vertical_scrollbar_rect(&self, size: Size) -> Rect {
        let geo = self.geometry();
        Rect::new(
            ACTIVITY_WIDTH + self.sidebar_width(size) - 16.0,
            EXPLORER_TOP - EXPLORER_ROW_HEIGHT,
            10.0,
            (geo.content_bottom - 12.0 - EXPLORER_TOP + EXPLORER_ROW_HEIGHT).max(0.0),
        )
    }

    /// Largura do conteúdo da árvore, medida pela árvore já posicionada.
    ///
    /// A instância persistente nunca passa por `layout` — quem é posicionada é a
    /// cópia usada para desenhar —, e é ela quem conhece a medida.
    pub(super) fn explorer_content_width(&self, _size: Size) -> f32 {
        self.explorer.tree.content_size().width
    }

    pub(super) fn visible_entries(&self) -> Vec<(usize, &FileNode)> {
        fn visit<'a>(
            node: &'a FileNode,
            depth: usize,
            expanded: &HashSet<PathBuf>,
            output: &mut Vec<(usize, &'a FileNode)>,
        ) {
            if depth > 0 {
                output.push((depth - 1, node));
            }
            if node.is_directory && expanded.contains(&node.path) {
                for child in &node.children {
                    visit(child, depth + 1, expanded, output);
                }
            }
        }
        let mut output = Vec::new();
        visit(
            &self.explorer.workspace,
            0,
            &self.explorer.expanded,
            &mut output,
        );
        output
    }

    /// Clique na árvore: abrir o arquivo, ou dobrar e desdobrar a pasta.
    pub(super) fn explorer_pointer_down(&mut self, point: Point, size: Size) -> bool {
        let editor_x = ACTIVITY_WIDTH + self.sidebar_width(size);
        if point.x < ACTIVITY_WIDTH || point.x >= editor_x || point.y < EXPLORER_TOP {
            return false;
        }
        // Qual nó foi clicado é a árvore quem sabe: o recuo, o deslocamento
        // horizontal e a virtualização são dela.
        let selecionado = self.explorer_tree_event(point, size);
        let entry = selecionado.and_then(|id| self.explorer_path_for(id));
        if let Some((path, is_directory)) = entry {
            self.context.focus = ShellFocus::Explorer;
            self.explorer.tree.set_selected(Some(explorer_id(&path)));
            if is_directory {
                if self.explorer.expanded.remove(&path) {
                    self.sync_explorer_tree();
                } else {
                    // Abrir uma pasta é o momento de lê-la: a árvore guarda só o
                    // que já foi aberto, e é isso que tira a varredura inteira
                    // do caminho da abertura do projeto.
                    if self.directory_needs_children(&path) {
                        self.explorer.requested.insert(path.clone());
                        self.commands
                            .push(ApplicationCommand::LoadDirectory(path.clone()));
                    }
                    self.explorer.expanded.insert(path);
                    self.sync_explorer_tree();
                }
            } else {
                self.commands
                    .push(ApplicationCommand::OpenDocument(OpenDocumentRequest::new(
                        path,
                    )));
            }
        }
        true
    }
}
