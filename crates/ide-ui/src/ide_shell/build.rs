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
        let explorer_tree = TreeView::new(EXPLORER_TREE_ID, explorer_items(&workspace, &[]))
            .with_row_height(EXPLORER_ROW_HEIGHT);
        let mut shell = Self {
            explorer: ExplorerState {
                workspace_name,
                workspace,
                tree: explorer_tree,
                context_menu: ContextMenu::new(EXPLORER_CONTEXT_MENU_ID, Vec::new()),
                context_menu_target: None,
                context_menu_file: None,
                expanded,
                scroll_x: 0.0,
                scroll_line: 0,
                sidebar_width: SIDEBAR_WIDTH,
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
            },
            terminal: TerminalPanelState {
                console: Console::new(TERMINAL_CONSOLE_ID, Vec::new()).with_metrics(
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
            },
            search: TypeSearchSurface::default(),
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
            menu: MenuState {
                bar: MenuBar::new(
                    MENU_BAR_ID,
                    vec![
                        MenuBarItem::menu(
                            "Arquivo",
                            vec![
                                MenuItem::new("Projeto...", "file.project"),
                                MenuItem::new("Salvar", "file.save"),
                            ],
                        ),
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
            context: ShellContext {
                focus: ShellFocus::None,
                text_metrics: None,
                clipboard: None,
                theme: Theme::default(),
                status_message: "Ready".to_owned(),
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
                rename::attach(&mut host, surface_layer_id(SurfaceKind::Rename));
                type_search::attach(&mut host, surface_layer_id(SurfaceKind::TypeSearch));
                host
            },
        };
        shell.sync_explorer_tree();
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
        let mut project_items = vec![
            MenuItem::new("Compilar projeto", "project.build"),
            MenuItem::new("Reimportar projeto", "project.reimport"),
            MenuItem::new("Executar aplicação", "project.run"),
            MenuItem::new("Parar aplicação", "project.stop"),
        ];
        project_items.extend(catalog.tasks.iter().map(|task| {
            MenuItem::new(
                task.title.clone(),
                CommandId(format!("task.execute.{}", task.id.0)),
            )
        }));
        self.menu.bar = MenuBar::new(
            MENU_BAR_ID,
            vec![
                MenuBarItem::menu(
                    "Arquivo",
                    vec![
                        MenuItem::new("Projeto...", "file.project"),
                        MenuItem::new("Salvar", "file.save"),
                    ],
                ),
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
        self.explorer.tree.set_roots(explorer_items(
            &self.explorer.workspace,
            &self.catalog.source_root_names,
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
