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
            &explorer_items(&self.explorer.workspace, &self.catalog.source_root_names),
            &expanded,
            target,
        ) {
            // Duas linhas de contexto ajudam a reconhecer o pacote pai. A cópia
            // da TreeView usada na pintura limita o deslocamento no fim da lista.
            self.explorer.scroll_line = row.saturating_sub(2);
        }
    }

    /// Posiciona a árvore de acordo com as barras de rolagem da janela.
    pub(super) fn explorer_tree_for(&self, size: Size) -> TreeView {
        let mut tree = self.explorer.tree.clone();
        tree.layout(&self.layout_context(), self.explorer_tree_rect(size));
        tree.set_scroll_offset(Point::new(
            self.explorer.scroll_x,
            self.explorer.scroll_line as f32 * EXPLORER_ROW_HEIGHT,
        ));
        tree
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
        tree.layout(&context, bounds);
        tree.set_scroll_offset(offset);
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
        self.explorer
            .rebuild_items(&self.catalog.source_root_names);
        self.sync_explorer_tree();
        // A árvore mudou: o documento ativo pode finalmente existir nela.
        self.sync_explorer_to_active();
    }

    /// Guarda os filhos que a aplicação leu para uma pasta expandida.
    fn insert_children_at(&mut self, path: &Path, children: Vec<FileNode>) -> bool {
        fn inserir(node: &mut FileNode, path: &Path, children: &mut Option<Vec<FileNode>>) -> bool {
            if node.path == path {
                let Some(filhos) = children.take() else {
                    return false;
                };
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
        self.explorer.sidebar_width.clamp(
            SIDEBAR_MIN_WIDTH,
            (size.width - 320.0).max(SIDEBAR_MIN_WIDTH),
        )
    }

    /// Clique secundário: abre o menu de contexto sobre o item do Explorer.
    ///
    /// Fora do Explorer o clique só dispensa um menu aberto. Enquanto não
    /// houver menu para as outras áreas, abrir um vazio prometeria ações que
    /// não existem.
    pub fn secondary_pointer_down(&mut self, point: Point, size: Size) {
        self.explorer.context_menu.close();
        self.explorer.context_menu_target = None;
        // O clique secundário nunca escolhe da lista, então ele só a dispensa.
        self.clear_completions();
        if self.settings.is_open() {
            return;
        }
        let geometry = self.geometry();
        let editor_x = ACTIVITY_WIDTH + self.sidebar_width(size);
        // No editor o menu fala do texto: copiar e colar.
        if point.x >= editor_x
            && point.x < editor_x + geometry.editor_width
            && point.y >= geometry.content_top
            && point.y < geometry.editor_bottom
        {
            self.context.focus = ShellFocus::Editor;
            let dentro_de_tipo = self.cursor_inside_type(point, size);
            self.explorer.context_menu.set_entries(editor_menu_entries(
                self.editor_area.pane.selection_range().is_some(),
                self.debug_panel.view.attached,
                dentro_de_tipo,
            ));
            self.explorer.context_menu.layout(
                &self.layout_context(),
                Rect::new(0.0, 0.0, size.width, size.height),
            );
            self.explorer.context_menu.open_at(point);
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
        self.explorer.context_menu_file = (!is_directory).then(|| path.clone());
        // O alvo é o diretório: clicando em um arquivo, é na pasta dele que a
        // criação acontece.
        let target = if is_directory {
            path
        } else {
            path.parent().map(Path::to_path_buf).unwrap_or(path)
        };
        self.explorer
            .context_menu
            .set_entries(explorer_menu_entries(
                &target,
                &self.catalog.source_root_names,
                &self.catalog.new_item_templates,
                !is_directory,
            ));
        self.explorer.context_menu.layout(
            &self.layout_context(),
            Rect::new(0.0, 0.0, size.width, size.height),
        );
        self.explorer.context_menu.open_at(point);
        self.explorer.context_menu_target = Some(target);
    }

    pub fn context_menu_open(&self) -> bool {
        self.explorer.context_menu.is_open()
    }

    /// Entrega o evento ao menu aberto e trata o comando escolhido.
    ///
    /// Devolve `true` quando o menu consumiu o evento — é o sinal de que o
    /// clique ou a tecla não devem seguir para o que está embaixo dele.
    pub(super) fn context_menu_event(&mut self, event: &UiEvent, size: Size) -> bool {
        if !self.explorer.context_menu.is_open() {
            return false;
        }
        self.explorer.context_menu.layout(
            &self.layout_context(),
            Rect::new(0.0, 0.0, size.width, size.height),
        );
        let mut context = EventContext::default();
        let result = self.explorer.context_menu.event(&mut context, event);
        if let EventResult::Action(WidgetAction::Command(command)) = &result {
            self.run_explorer_command(&command.0);
        }
        if !self.explorer.context_menu.is_open() {
            self.explorer.context_menu_target = None;
        }
        result != EventResult::Ignored
    }

    /// Entrega a tecla ao menu aberto.
    ///
    /// Separado do caminho do ponteiro porque navegar por teclado não depende
    /// de onde o menu foi desenhado, e assim não precisa do tamanho da janela.
    pub(super) fn context_menu_key(&mut self, key: &str, modifiers: Modifiers) -> bool {
        if !self.explorer.context_menu.is_open() {
            return false;
        }
        let mut context = EventContext::default();
        let result = self.explorer.context_menu.event(
            &mut context,
            &UiEvent::KeyDown(KeyEvent {
                logical_key: key.to_owned(),
                repeat: false,
                modifiers,
            }),
        );
        if let EventResult::Action(WidgetAction::Command(command)) = &result {
            self.run_explorer_command(&command.0);
        }
        if !self.explorer.context_menu.is_open() {
            self.explorer.context_menu_target = None;
        }
        result != EventResult::Ignored
    }

    pub(super) fn run_explorer_command(&mut self, command: &str) {
        match command {
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
        let Some(target) = self.explorer.context_menu_target.clone() else {
            return;
        };
        if command == "explorer.rename" {
            if let Some(arquivo) = self.explorer.context_menu_file.clone() {
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
    pub(super) fn explorer_content_width(&self, size: Size) -> f32 {
        self.explorer_tree_for(size).content_size().width
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
