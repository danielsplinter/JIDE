//! Construção do shell e o que a aplicação contribui depois dela.

use super::*;
use ui_components::Console;

impl IdeShell {
    pub fn from_tree(workspace: FileNode) -> Self {
        let workspace_name = workspace
            .path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("workspace")
            .to_owned();
        let mut expanded = HashSet::new();
        expanded.insert(workspace.path.clone());
        let raiz_lida = workspace.path.clone();
        let terminal_root = if workspace.is_directory {
            workspace.path.clone()
        } else {
            PathBuf::from(".")
        };
        let terminals = TerminalSession::discover_profiles()
            .into_iter()
            .filter_map(|profile| {
                TerminalSession::new(terminal_root.clone(), 2_000, profile.clone())
                    .or_else(|_| TerminalSession::new(PathBuf::from("."), 2_000, profile))
                    .ok()
                    .map(|session| TerminalTab {
                        session,
                        scroll_line: 0,
                        follow_output: true,
                    })
            })
            .collect();
        // Sem espécie nenhuma ainda: o índice não existe no primeiro quadro, e
        // os crachás chegam quando a aplicação os trouxer.
        let explorer_tree = ComposedTreeView::new(
            EXPLORER_TREE_ID,
            explorer_items(&workspace, &[], &HashMap::new()),
        )
        .with_row_height(EXPLORER_ROW_HEIGHT)
        // A barra vertical é do painel, e não da árvore: é ele quem sabe onde
        // ela cabe ao lado da horizontal e do divisor. Com as duas ligadas
        // apareciam **duas trilhas** na mesma borda.
        .with_own_scrollbar(false);
        let mut shell = Self {
            explorer: ExplorerState {
                workspace_name,
                workspace,
                tree: explorer_tree,
                expanded,
                // A varredura de abertura já trouxe os filhos da raiz.
                requested: std::iter::once(raiz_lida).collect(),
                scroll_x: 0.0,
                scroll_line: 0,
                sidebar_width: SIDEBAR_WIDTH,
                recolhido: false,
                splitter: Splitter::new(SIDEBAR_SPLITTER_ID, SplitOrientation::Horizontal),
                vertical_scrollbar: Scrollbar::new(
                    EXPLORER_VERTICAL_SCROLLBAR_ID,
                    ScrollbarOrientation::Vertical,
                ),
                horizontal_scrollbar: Scrollbar::new(
                    EXPLORER_HORIZONTAL_SCROLLBAR_ID,
                    ScrollbarOrientation::Horizontal,
                ),
            },
            editor_area: EditorAreaState {
                session: EditorSession::default(),
                pane: EditorPane::new(EditorCapabilities::full()),
                search_query: String::new(),
                search_open: false,
                navigated: None,
                scrollbar: Scrollbar::new(EDITOR_SCROLLBAR_ID, ScrollbarOrientation::Vertical),
                horizontal_scrollbar: Scrollbar::new(
                    EDITOR_HORIZONTAL_SCROLLBAR_ID,
                    ScrollbarOrientation::Horizontal,
                ),
                syntax_snapshots: HashMap::new(),
                syntax_spans: HashMap::new(),
                completion_items: Vec::new(),
                completion_selected: 0,
                completacao_aceita: None,
                history: NavigationHistory::default(),
                divisao: None,
            },
            terminal: TerminalPanelState {
                busca: None,
                console: Console::new(TERMINAL_CONSOLE_ID, Vec::new()).with_metrics(
                    14.0,
                    EDITOR_LINE_HEIGHT,
                    14.0,
                ),
                grid: TerminalView::new(TERMINAL_GRID_ID, Vec::new()).with_metrics(
                    14.0,
                    EDITOR_LINE_HEIGHT,
                    14.0,
                ),
                tabs: terminals,
                active: 0,
                height: TERMINAL_DEFAULT_HEIGHT,
                last_height: TERMINAL_DEFAULT_HEIGHT,
                minimized: false,
                splitter: Splitter::new(TERMINAL_SPLITTER_ID, SplitOrientation::Vertical),
                scrollbar: Scrollbar::new(TERMINAL_SCROLLBAR_ID, ScrollbarOrientation::Vertical),
                selection: None,
                selecting: false,
                running_terminal: None,
                // 80 é a largura com que o PTY nasce; o primeiro quadro corrige.
                pty_cols: 80,
                pty_rows: 24,
            },
            search: TypeSearchSurface::default(),
            tab_switcher: TabSwitcherSurface::default(),
            inspection: InspectionSurface::default(),
            settings: SettingsSurface::default(),
            debug_panel: DebugPanelState {
                stop_button: Button::icon(STOP_BUTTON_ID, Icon::Stop, "Parar aplicação")
                    .with_tint(IconTint::Muted)
                    .with_command("project.stop"),
                run_button: Button::icon(RUN_BUTTON_ID, Icon::Play, "Executar aplicação")
                    .with_tint(IconTint::Success)
                    .with_command("project.run"),
                debug_button: Button::icon(DEBUG_BUTTON_ID, Icon::Bug, "Executar com depuração")
                    .with_tint(IconTint::Muted)
                    .with_command("debug.run"),
                breakpoints: BTreeMap::new(),
                verified_breakpoints: BTreeMap::new(),
                view: DebugView::default(),
                step_buttons: DEBUG_BUTTONS
                    .iter()
                    .enumerate()
                    .map(|(index, (title, _))| {
                        Button::new(WidgetId(DEBUG_STEP_BASE_ID.0 + index as u64), *title)
                    })
                    .collect(),
                frames: ListView::new(DEBUG_FRAMES_ID, Vec::<String>::new())
                    .with_row_height(DEBUG_ROW_HEIGHT),
                variables: ListView::new(DEBUG_VARIABLES_ID, Vec::<String>::new())
                    .with_row_height(DEBUG_ROW_HEIGHT),
            },
            generate: GenerateSurface::default(),
            new_item: NewItemSurface::default(),
            rename: RenameSurface::default(),
            context_menu: ContextMenuSurface::default(),
            menu: MenuState {
                recents: Vec::new(),
                bar: MenuBar::new(
                    MENU_BAR_ID,
                    vec![
                        crate::menus::file_menu(&[]),
                        MenuBarItem::menu(
                            "Projeto",
                            vec![
                                MenuItem::new("Compilar projeto", "project.build"),
                                MenuItem::new("Reimportar projeto", "project.reimport"),
                                MenuItem::new("Executar aplicação", "project.run"),
                                MenuItem::new("Parar aplicação", "project.stop"),
                            ],
                        ),
                        MenuBarItem::menu(
                            "Depurar",
                            vec![
                                MenuItem::new("Conectar...", "debug.connect"),
                                MenuItem::new("Continuar", "debug.continue"),
                                MenuItem::new("Pausar", "debug.pause"),
                                MenuItem::new("Passo sobre", "debug.over"),
                                MenuItem::new("Entrar", "debug.into"),
                                MenuItem::new("Sair", "debug.out"),
                                MenuItem::new("Desconectar", "debug.detach"),
                            ],
                        ),
                        MenuBarItem::command("Configurações", "settings.open"),
                    ],
                ),
            },
            catalog: UiContributionCatalog::default(),
            declaration_kinds: HashMap::new(),
            context: ShellContext {
                focus: ShellFocus::None,
                busca_no_terminal: false,
                text_metrics: None,
                clipboard: None,
                theme: Theme::default(),
                status_message: "Ready".to_owned(),
                memory_usage: None,
                project_loading: None,
                project_summary: None,
                last_size: Size::new(1280.0, 800.0),
                scrollbar_drag: None,
            },
            commands: ShellCommandQueue::default(),
            host: {
                // Os componentes de cada janela são do anfitrião da tela desde a
                // construção; o que muda com a abertura é a presença deles na
                // pilha. Ver `16-single-host`.
                let mut host = new_host();
                generate::attach(&mut host, surface_layer_id(SurfaceKind::Generate));
                inspection::attach(&mut host, surface_layer_id(SurfaceKind::Inspection));
                settings::attach(&mut host, surface_layer_id(SurfaceKind::Settings));
                new_item::attach(&mut host, surface_layer_id(SurfaceKind::NewItem));
                tab_switcher::attach(
                    &mut host,
                    surface_layer_id(SurfaceKind::TabSwitcher),
                );
                rename::attach(&mut host, surface_layer_id(SurfaceKind::Rename));
                type_search::attach(&mut host, surface_layer_id(SurfaceKind::TypeSearch));
                host
            },
        };
        shell.sync_explorer_tree();
        // A moldura é arranjada já na construção: a geometria passa a ser
        // leitura, e leitura precisa de algo escrito antes. Sem isto, quem
        // perguntasse antes do primeiro quadro receberia faixas vazias.
        let inicial = shell.context.last_size;
        shell.sync_frame(inicial);
        let _ = shell.host.layout(inicial);
        shell
    }

    /// Recebe o mecanismo que mede o texto que a janela desenha.
    pub fn set_clipboard(&mut self, clipboard: Arc<dyn ClipboardService>) {
        self.context.clipboard = Some(clipboard);
    }

    /// Instala o modelo visual agregado das contribuições de linguagem.
    ///
    /// Templates, páginas, raízes e tarefas deixam de ser convenções da UI:
    /// trocar o catálogo reconstrói os controles que apresentam esses dados.
    pub fn set_ui_catalog(&mut self, catalog: UiContributionCatalog) {
        let mut settings_titles = catalog
            .settings_sections
            .iter()
            .map(|section| section.title.clone())
            .collect::<Vec<_>>();
        settings_titles.push(settings::DEBUG_PAGE_TITLE.to_owned());
        self.settings.set_pages(settings_titles);
        if let Some(section) = catalog.settings_sections.first() {
            self.settings
                .set_browse_title(section.browse_button_title.clone());
        }
        if let Some(task) = catalog.tasks.iter().find(|task| task.show_in_toolbar) {
            self.debug_panel.run_button =
                Button::icon(RUN_BUTTON_ID, Icon::Play, task.title.clone())
                    .with_tint(IconTint::Success)
                    .with_command(CommandId(format!("task.execute.{}", task.id.0)));
        }
        self.catalog = catalog;
        self.rebuild_menu_bar();
        self.explorer.tree.set_roots(explorer_items(
            &self.explorer.workspace,
            &self.catalog.source_root_names,
            &self.declaration_kinds,
        ));
    }

    /// Recebe os projetos que "Arquivo → Recentes" vai oferecer.
    ///
    /// Chega pronta de fora: quem sabe onde a lista mora e o que dela ainda
    /// existe é a aplicação. A tela só a apresenta — e guarda a ordem, porque é
    /// por ela que o clique volta a virar caminho.
    pub fn set_recent_projects(&mut self, recents: Vec<RecentProject>) {
        if self.menu.recents == recents {
            return;
        }
        self.menu.recents = recents;
        self.rebuild_menu_bar();
    }

    /// Remonta a barra a partir do catálogo e dos recentes de agora.
    ///
    /// Uma barra só, montada num lugar só: as tarefas do catálogo e a lista de
    /// recentes chegam em momentos diferentes, e quem chegasse depois apagaria o
    /// que o outro tinha posto se cada um montasse a sua.
    fn rebuild_menu_bar(&mut self) {
        let mut project_items = vec![
            MenuItem::new("Compilar projeto", "project.build"),
            MenuItem::new("Reimportar projeto", "project.reimport"),
            MenuItem::new("Executar aplicação", "project.run"),
            MenuItem::new("Parar aplicação", "project.stop"),
        ];
        project_items.extend(self.catalog.tasks.iter().map(|task| {
            MenuItem::new(
                task.title.clone(),
                CommandId(format!("task.execute.{}", task.id.0)),
            )
        }));
        self.menu.bar = MenuBar::new(
            MENU_BAR_ID,
            vec![
                crate::menus::file_menu(&self.menu.recents),
                MenuBarItem::menu("Projeto", project_items),
                MenuBarItem::menu(
                    "Depurar",
                    vec![
                        MenuItem::new("Conectar...", "debug.connect"),
                        MenuItem::new("Continuar", "debug.continue"),
                        MenuItem::new("Pausar", "debug.pause"),
                        MenuItem::new("Passo sobre", "debug.over"),
                        MenuItem::new("Entrar", "debug.into"),
                        MenuItem::new("Sair", "debug.out"),
                        MenuItem::new("Desconectar", "debug.detach"),
                    ],
                ),
                MenuBarItem::command("Configurações", "settings.open"),
            ],
        );
    }

    /// Recebe da aplicação que espécie de tipo cada arquivo declara.
    ///
    /// Vem de fora porque quem sabe é o índice, e perguntar ao índice não é
    /// trabalho de quem desenha. Chega **depois** do primeiro quadro: até lá o
    /// Explorer mostra os nomes sem crachá, que é a verdade — ninguém sabia
    /// ainda, e um crachá chutado seria pior do que nenhum.
    ///
    /// Sai cedo quando nada mudou: remontar a árvore constrói um componente por
    /// nó, e a aplicação pode repetir a resposta a cada reindexação.
    pub fn set_declaration_kinds(&mut self, kinds: HashMap<u64, SymbolKind>) {
        if self.declaration_kinds == kinds {
            return;
        }
        self.declaration_kinds = kinds;
        self.explorer.tree.set_roots(explorer_items(
            &self.explorer.workspace,
            &self.catalog.source_root_names,
            &self.declaration_kinds,
        ));
    }

    /// Troca o tema da interface.
    ///
    /// O tema vem da ERLibUi e vale para tudo — inclusive para os componentes da
    /// biblioteca, que o recebem pelo contexto de pintura. A IDE não guarda cor
    /// própria.
    pub fn set_theme(&mut self, theme: Theme) {
        self.context.theme = theme;
    }

    #[must_use]
    pub const fn theme(&self) -> &Theme {
        &self.context.theme
    }
}
