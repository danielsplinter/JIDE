//! Testes do shell da IDE, partidos por assunto.
//!
//! # Por que este arquivo é só a montagem
//!
//! Eles moravam num arquivo só, que chegou a **8 519 linhas**. O teto que a `15`
//! pôs nele existia para tornar o crescimento visível, e ele subiu seis vezes —
//! a última com a frase de que não subiria mais sem o arquivo ser partido. Este
//! é o item 4(b) do plano da `26`.
//!
//! O que o tamanho cobrava era concreto, e está registrado na `26`: **já
//! aconteceu de eu editar o teste errado por não achar o certo**. Procurar o
//! teste de uma tela entre duzentos e vinte de dez assuntos é o mesmo problema
//! que o objeto-deus tem, na forma de teste.
//!
//! # O corte
//!
//! Por **assunto da tela**, e não por tipo de verificação. Quem mexe no
//! gerenciador de Git quer ver junto tudo o que fala dele; quem mexe no
//! roteamento do ponteiro não quer ver nada disso. Os ajudantes ficam aqui, no
//! lugar que todos alcançam, porque metade deles serve a mais de um assunto —
//! espalhá-los daria a cópia que diverge na primeira correção.

use super::*;
use ui_core::Color;
// Tipos que o shell deixou de importar quando as funções puras saíram; os
// testes continuam falando deles.
use crate::debugging::DebugVariableView;
use crate::ide_shell::inspection::InspectionGeometry;
use crate::ide_shell::settings::SettingsDialogGeometry;
use crate::search::{ContentSearchHit, TypeSearchHit};
use ide_application::{GitRequest, NewItemRequest, NewItemTemplate};
use ide_domain::{AccessorCandidate, AccessorPlan, Location, SyntaxHighlightKind, ToolRole};
use ui_editor::TokenKind;
fn java_source_roots() -> Vec<String> {
    vec!["java".to_owned()]
}
fn java_catalog() -> UiContributionCatalog {
    UiContributionCatalog {
        language_names: vec!["Java".to_owned()],
        source_root_names: java_source_roots(),
        new_item_templates: vec![
            NewItemTemplate {
                id: NewItemTemplateId::new("java.package"),
                title: "Novo pacote".to_owned(),
                name_caption: "Classe (opcional)".to_owned(),
                file_extension: None,
                allows_empty_name: true,
            },
            NewItemTemplate {
                id: NewItemTemplateId::new("java.class"),
                title: "Nova classe".to_owned(),
                name_caption: "Nome da classe".to_owned(),
                file_extension: Some("java".to_owned()),
                allows_empty_name: false,
            },
            NewItemTemplate {
                id: NewItemTemplateId::new("java.interface"),
                title: "Nova interface".to_owned(),
                name_caption: "Nome da interface".to_owned(),
                file_extension: Some("java".to_owned()),
                allows_empty_name: false,
            },
        ],
        settings_sections: vec![SettingsSection {
            id: "java.compiler-vm".to_owned(),
            title: "Compilador e VM".to_owned(),
            field_caption: "JDK".to_owned(),
            browse_button_title: "Procurar...".to_owned(),
            secondary_caption: None,
        }],
        tasks: vec![TaskDescriptor {
            id: TaskId("java.run".to_owned()),
            title: "Executar".to_owned(),
            requires_active_document: true,
            show_in_toolbar: true,
        }],
    }
}
fn fake_catalog() -> UiContributionCatalog {
    UiContributionCatalog {
        language_names: vec!["Fake".to_owned()],
        source_root_names: vec!["src".to_owned()],
        new_item_templates: vec![NewItemTemplate {
            id: NewItemTemplateId::new("fake.module"),
            title: "Novo módulo fake".to_owned(),
            name_caption: "Nome do módulo".to_owned(),
            file_extension: Some("fake".to_owned()),
            allows_empty_name: false,
        }],
        settings_sections: vec![SettingsSection {
            id: "fake.runtime".to_owned(),
            title: "Runtime fake".to_owned(),
            field_caption: "Runtime".to_owned(),
            browse_button_title: "Localizar...".to_owned(),
            secondary_caption: None,
        }],
        tasks: vec![TaskDescriptor {
            id: TaskId("fake.run".to_owned()),
            title: "Executar fake".to_owned(),
            requires_active_document: false,
            show_in_toolbar: true,
        }],
    }
}
fn open_java_settings(shell: &mut IdeShell, items: Vec<String>, selected: usize) {
    shell.set_ui_catalog(java_catalog());
    shell.open_settings_dialog(items, selected);
}
fn test_shell() -> IdeShell {
    let root = PathBuf::from("workspace");
    let directory = root.join("src");
    IdeShell::from_tree(FileNode {
        path: root,
        is_directory: true,
        children: vec![FileNode {
            path: directory,
            is_directory: true,
            children: Vec::new(),
        }],
    })
}
fn dir(path: &str, children: Vec<FileNode>) -> FileNode {
    FileNode {
        path: PathBuf::from(path),
        is_directory: true,
        children,
    }
}
fn file(path: &str) -> FileNode {
    FileNode {
        path: PathBuf::from(path),
        is_directory: false,
        children: Vec::new(),
    }
}
fn labels(items: &[NoDoExplorer]) -> Vec<&str> {
    items.iter().map(|item| item.label.as_str()).collect()
}
/// Projeto Maven com a cadeia de pacote que a captura mostra.
fn maven_project() -> FileNode {
    dir(
        "demo",
        vec![dir(
            "demo/src",
            vec![dir(
                "demo/src/main",
                vec![dir(
                    "demo/src/main/java",
                    vec![dir(
                        "demo/src/main/java/br",
                        vec![dir(
                            "demo/src/main/java/br/com",
                            vec![dir(
                                "demo/src/main/java/br/com/exemplo",
                                vec![dir(
                                    "demo/src/main/java/br/com/exemplo/endpoints",
                                    vec![
                                        dir(
                                            "demo/src/main/java/br/com/exemplo/endpoints/controller",
                                            Vec::new(),
                                        ),
                                        file(
                                            "demo/src/main/java/br/com/exemplo/endpoints/App.java",
                                        ),
                                    ],
                                )],
                            )],
                        )],
                    )],
                )],
            )],
        )],
    )
}
fn shell_with_java_file() -> (IdeShell, PathBuf) {
    let mut shell = test_shell();
    let path = PathBuf::from("Main.java");
    shell.editor_area.session.open_memory(
        "Main.java",
        "class Main {\n  void run() {\n    int total = 1;\n  }\n}",
    );
    (shell, path)
}
fn entry_labels(entries: &[MenuEntry]) -> Vec<String> {
    entries
        .iter()
        .map(|entry| match entry {
            MenuEntry::Item(item) => item.label.clone(),
            MenuEntry::Submenu { label, .. } => label.clone(),
            MenuEntry::Separator => "—".to_owned(),
        })
        .collect()
}
/// Razão de contraste WCAG entre duas cores opacas.
fn contrast_ratio(first: Color, second: Color) -> f32 {
    fn luminance(color: Color) -> f32 {
        fn channel(value: f32) -> f32 {
            if value <= 0.03928 {
                value / 12.92
            } else {
                ((value + 0.055) / 1.055).powf(2.4)
            }
        }
        0.2126 * channel(color.red) + 0.7152 * channel(color.green) + 0.0722 * channel(color.blue)
    }
    let first = luminance(first);
    let second = luminance(second);
    (first.max(second) + 0.05) / (first.min(second) + 0.05)
}
/// Projeto Maven com um pacote já criado, para o menu agir sobre ele.
fn shell_with_package() -> IdeShell {
    let mut shell = IdeShell::from_tree(dir(
        "demo",
        vec![dir(
            "demo/src/main/java",
            vec![dir(
                "demo/src/main/java/br",
                vec![dir("demo/src/main/java/br/com", Vec::new())],
            )],
        )],
    ));
    shell.set_ui_catalog(java_catalog());
    shell
}
/// Prepara um shell com arquivo aberto e foco no editor.
fn shell_editing(text: &str) -> IdeShell {
    let mut shell = test_shell();
    shell.editor_area.session.open_memory("Pedido.java", text);
    shell.context.focus = ShellFocus::Editor;
    shell.editor_area.pane.set_cursor(0);
    shell
}
fn accessor_plan_para_teste() -> AccessorPlan {
    let candidato = |campo: &str, fonte: Option<&str>| AccessorCandidate {
        field: campo.to_owned(),
        source: fonte.map(str::to_owned),
    };
    AccessorPlan {
        candidates: vec![
            candidato(
                "id",
                Some("\n    public Long getId() {\n        return id;\n    }\n"),
            ),
            // Já tem getter: não deve nem aparecer na janela.
            candidato("nome", None),
            candidato(
                "ativo",
                Some("\n    public boolean isAtivo() {\n        return ativo;\n    }\n"),
            ),
        ],
        insert_at: DomainTextPosition { line: 2, column: 0 },
    }
}
/// Coluna do editor em coordenadas de tela.
fn editor_column(shell: &IdeShell, size: Size, index: usize) -> Point {
    let geometry = shell.geometry();
    let editor_x = ACTIVITY_WIDTH + shell.sidebar_width(size);
    Point::new(
        editor_x + EDITOR_GUTTER + index as f32 * EDITOR_CHAR_WIDTH,
        geometry.content_top + 20.0,
    )
}
/// Área de transferência de teste, sem depender do sistema.
#[derive(Default)]
struct FakeClipboard {
    text: std::sync::Mutex<Option<String>>,
}
impl ClipboardService for FakeClipboard {
    fn get_text(&self) -> Result<Option<String>, ui_window_api::ClipboardError> {
        Ok(self.text.lock().ok().and_then(|text| text.clone()))
    }

    fn set_text(&self, value: &str) -> Result<(), ui_window_api::ClipboardError> {
        if let Ok(mut text) = self.text.lock() {
            *text = Some(value.to_owned());
        }
        Ok(())
    }
}
/// O objeto inspecionado: um `Pedido` com um campo simples e outro objeto.
fn inspection_value() -> DebugVariableView {
    DebugVariableView {
        name: "pedido".to_owned(),
        value: "Pedido@1a2b".to_owned(),
        type_name: Some("br.com.exemplo.Pedido".to_owned()),
        expandable: true,
    }
}
fn inspection_fields() -> Vec<DebugVariableView> {
    vec![
        DebugVariableView {
            name: "total".to_owned(),
            value: "42".to_owned(),
            type_name: Some("int".to_owned()),
            expandable: false,
        },
        DebugVariableView {
            name: "cliente".to_owned(),
            value: "Cliente@3c4d".to_owned(),
            type_name: Some("br.com.exemplo.Cliente".to_owned()),
            expandable: true,
        },
    ]
}
fn inspection_void() -> DebugVariableView {
    DebugVariableView {
        name: "retorno".to_owned(),
        value: "void".to_owned(),
        type_name: None,
        expandable: false,
    }
}
fn type_hit(name: &str, kind: &str, path: &std::path::Path, line: u32) -> TypeSearchHit {
    TypeSearchHit {
        name: name.to_owned(),
        kind: kind.to_owned(),
        location: Location {
            path: path.into(),
            range: ide_domain::TextRange {
                start: DomainTextPosition { line, column: 0 },
                end: DomainTextPosition { line, column: 0 },
            },
        },
    }
}
fn content_hit(path: &std::path::Path, line: u32, column: u32) -> ContentSearchHit {
    ContentSearchHit {
        preview: "String mensagem = \"conteúdo procurado\";".to_owned(),
        location: Location {
            path: path.into(),
            range: ide_domain::TextRange {
                start: DomainTextPosition { line, column },
                end: DomainTextPosition { line, column },
            },
        },
    }
}
/// Diretório com dois tipos, para a busca ter o que abrir de verdade.
fn type_search_workspace() -> std::path::PathBuf {
    static NEXT_WORKSPACE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let sequence = NEXT_WORKSPACE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!("er-ide-busca-{}-{sequence}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    assert!(std::fs::create_dir_all(&root).is_ok());
    assert!(std::fs::write(root.join("Pedido.java"), "class Pedido {}\n").is_ok());
    assert!(
        std::fs::write(
            root.join("PedidoRepository.java"),
            "interface PedidoRepository {}\n"
        )
        .is_ok()
    );
    root
}
/// As áreas dos ícones do título, depois de um quadro.
///
/// Elas vêm do arranjo, e o arranjo acontece no quadro — pedir antes daria a
/// moldura do tamanho anterior.
fn action_areas(shell: &mut IdeShell, size: Size) -> [Rect; 3] {
    let _ = shell.paint(size);
    shell.action_button_areas()
}
fn inspection_layout(shell: &mut IdeShell, size: Size) -> InspectionGeometry {
    // As áreas vêm do arranjo, e o arranjo acontece no quadro.
    let _ = shell.paint(size);
    inspection::areas(&shell.host)
}
/// O que está marcado no editor da inspeção.
fn inspection_selection(shell: &IdeShell) -> Option<&str> {
    let (editor, source) = shell.inspection.editor_and_source_ref();
    editor.selected_text(source)
}
fn paint_circles(shell: &mut IdeShell, size: Size) -> Vec<f32> {
    shell
        .paint(size)
        .iter()
        .filter_map(|command| match command {
            PaintCommand::FillCircle(circle) => Some(circle.radius),
            _ => None,
        })
        .collect()
}
fn painted_texts(shell: &mut IdeShell, size: Size) -> Vec<String> {
    shell
        .paint(size)
        .iter()
        .filter_map(|command| match command {
            PaintCommand::DrawText(text) => Some(text.text.clone()),
            _ => None,
        })
        .collect()
}
fn open_settings_geometry(shell: &mut IdeShell, size: Size) -> SettingsDialogGeometry {
    // A moldura vem do arranjo, e o arranjo acontece no quadro.
    let _ = shell.paint(size);
    shell.settings.geometry(&shell.host)
}
/// Abre o combo e clica na segunda linha.
fn choose_second_jdk(shell: &mut IdeShell, geometry: &SettingsDialogGeometry, size: Size) {
    shell.pointer_down(
        Point::new(
            geometry.combo.origin.x + 10.0,
            geometry.combo.origin.y + 10.0,
        ),
        size,
    );
    shell.pointer_down(
        Point::new(
            geometry.combo.origin.x + 10.0,
            geometry.combo.origin.y + geometry.combo.size.height + 28.0 + 5.0,
        ),
        size,
    );
}
/// Acessos que só os testes usam para entrar pela porta do shell.
impl IdeShell {
    #[cfg(test)]
    fn open(root: &Path) -> Result<Self, ide_workspace::WorkspaceError> {
        ide_workspace::WorkspaceService::native()
            .scan(root)
            .map(Self::from_tree)
    }

    /// Atende os pedidos de leitura de pasta, como a aplicação faria.
    ///
    /// A varredura é rasa desde a `19`: o shell pede as pastas que precisa, e
    /// quem lê o disco é a aplicação. Nos testes não há laço de aplicação, e sem
    /// isto a árvore ficaria só com o primeiro nível.
    #[cfg(test)]
    fn fulfill_directory_loads(&mut self) {
        let service = ide_workspace::WorkspaceService::native();
        let raiz = self.workspace_root().to_path_buf();
        while let Some(ApplicationCommand::LoadDirectory(path)) = self
            .take_test_command(|command| matches!(command, ApplicationCommand::LoadDirectory(_)))
        {
            self.insert_path_children(service.scan_path(&raiz, &path));
        }
    }

    #[cfg(test)]
    fn open_file(&mut self, path: &Path) -> Result<DocumentId, String> {
        // Abrir um arquivo revela o caminho dele, e revelar pede leitura das
        // pastas: a aplicação atenderia no laço seguinte, e aqui atendemos na
        // hora, antes e depois.
        self.fulfill_directory_loads();
        let resultado = self.open_file_inner(path);
        self.fulfill_directory_loads();
        resultado
    }

    #[cfg(test)]
    fn open_file_inner(&mut self, path: &Path) -> Result<DocumentId, String> {
        if self
            .editor_area
            .session
            .tabs()
            .any(|document| document.path == path)
        {
            return Ok(self.show_document(path, String::new()));
        }
        let text = ide_workspace::WorkspaceService::native()
            .read_document(path)
            .map_err(|error| error.to_string())?;
        Ok(self.show_document(path, text))
    }

    #[cfg(test)]
    fn open_location(
        &mut self,
        path: &Path,
        line: usize,
        column: usize,
    ) -> Result<DocumentId, String> {
        if self
            .editor_area
            .session
            .tabs()
            .any(|document| document.path == path)
        {
            return Ok(self.show_location(path, String::new(), line, column));
        }
        let text = ide_workspace::WorkspaceService::native()
            .read_document(path)
            .map_err(|error| error.to_string())?;
        Ok(self.show_location(path, text, line, column))
    }

    #[cfg(test)]
    fn save_active_document(&mut self) -> bool {
        let Some(document) = self.editor_area.session.active() else {
            return false;
        };
        let id = document.id;
        let path = document.path.clone();
        let text = document.buffer.text().to_owned();
        let revision = document.buffer.revision();
        if ide_workspace::WorkspaceService::native()
            .save_document(&path, &text)
            .is_err()
        {
            return false;
        }
        self.document_saved(id, revision, &path);
        true
    }

    #[cfg(test)]
    fn reload_workspace(&mut self) -> Result<(), ide_workspace::WorkspaceError> {
        let tree = ide_workspace::WorkspaceService::native().scan(&self.explorer.workspace.path)?;
        self.replace_workspace_tree(tree);
        // Trocar a árvore pede de volta as pastas abertas; na aplicação quem
        // atende é o laço, e aqui somos nós.
        self.fulfill_directory_loads();
        Ok(())
    }

    #[cfg(test)]
    fn take_test_command(
        &mut self,
        predicate: impl Fn(&ApplicationCommand) -> bool,
    ) -> Option<ApplicationCommand> {
        let index = self.commands.iter().position(predicate)?;
        Some(self.commands.remove(index))
    }

    #[cfg(test)]
    fn take_settings_jdk_result(&mut self) -> Option<usize> {
        match self.take_test_command(|command| {
            matches!(
                command,
                ApplicationCommand::SelectTool {
                    role: ToolRole::Primary,
                    ..
                }
            )
        }) {
            Some(ApplicationCommand::SelectTool { index, .. }) => Some(index),
            _ => None,
        }
    }

    #[cfg(test)]
    fn take_browse_jdk_request(&mut self) -> bool {
        self.take_test_command(|command| {
            matches!(
                command,
                ApplicationCommand::BrowseTool {
                    role: ToolRole::Primary,
                    ..
                }
            )
        })
        .is_some()
    }

    #[cfg(test)]
    fn take_navigation_request(&mut self) -> Option<NavigationRequest> {
        match self.take_test_command(|command| matches!(command, ApplicationCommand::Navigate(_))) {
            Some(ApplicationCommand::Navigate(request)) => Some(request),
            _ => None,
        }
    }

    #[cfg(test)]
    fn take_open_project_request(&mut self) -> bool {
        self.take_test_command(|command| matches!(command, ApplicationCommand::OpenProject))
            .is_some()
    }

    #[cfg(test)]
    fn take_breakpoints_dirty(&mut self) -> Option<PathBuf> {
        match self.take_test_command(|command| {
            matches!(command, ApplicationCommand::BreakpointsChanged(_))
        }) {
            Some(ApplicationCommand::BreakpointsChanged(path)) => Some(path),
            _ => None,
        }
    }

    #[cfg(test)]
    fn take_debug_requests(&mut self) -> Vec<DebugRequest> {
        let mut requests = Vec::new();
        self.commands.retain(|command| {
            if let ApplicationCommand::Debug(request) = command {
                requests.push(request.clone());
                false
            } else {
                true
            }
        });
        requests
    }

    #[cfg(test)]
    fn take_build_project_request(&mut self) -> bool {
        self.take_test_command(|command| matches!(command, ApplicationCommand::BuildProject))
            .is_some()
    }

    #[cfg(test)]
    fn take_reimport_project_request(&mut self) -> bool {
        self.take_test_command(|command| matches!(command, ApplicationCommand::ReimportProject))
            .is_some()
    }

    #[cfg(test)]
    fn take_run_request(&mut self) -> bool {
        self.take_test_command(|command| matches!(command, ApplicationCommand::RunProject))
            .is_some()
    }

    #[cfg(test)]
    fn take_stop_request(&mut self) -> bool {
        self.take_test_command(|command| matches!(command, ApplicationCommand::StopProject))
            .is_some()
    }

    #[cfg(test)]
    fn take_open_settings_request(&mut self) -> bool {
        self.take_test_command(|command| matches!(command, ApplicationCommand::OpenSettings))
            .is_some()
    }

    #[cfg(test)]
    fn take_new_item_request(&mut self) -> Option<ide_application::NewItemRequest> {
        match self.take_test_command(|command| matches!(command, ApplicationCommand::CreateItem(_)))
        {
            Some(ApplicationCommand::CreateItem(request)) => Some(request),
            _ => None,
        }
    }

    #[cfg(test)]
    fn take_type_search_request(&mut self) -> Option<String> {
        match self
            .take_test_command(|command| matches!(command, ApplicationCommand::SearchTypes(_)))
        {
            Some(ApplicationCommand::SearchTypes(query)) => Some(query),
            _ => None,
        }
    }

    #[cfg(test)]
    fn take_content_search_request(&mut self) -> Option<String> {
        match self
            .take_test_command(|command| matches!(command, ApplicationCommand::SearchContent(_)))
        {
            Some(ApplicationCommand::SearchContent(query)) => Some(query),
            _ => None,
        }
    }
}
/// Um retrato com arquivos alterados, para os testes da aba `status`.
fn retrato_com_alteracoes(raiz: &Path) -> GitView {
    let entrada = |nome: &str, state: GitFileState| GitEntry {
        path: raiz.join(nome),
        label: nome.to_owned(),
        state,
    };
    GitView {
        head: Some("main".to_owned()),
        changed: 3,
        staged: 1,
        modified: 1,
        untracked: 1,
        branches: vec![BranchItem {
            name: "main".to_owned(),
            current: true,
        ..BranchItem::default()
        }],
        entries: vec![
            entrada("preparado.java", GitFileState::Staged),
            entrada("alterado.java", GitFileState::Modified),
            entrada("solto.java", GitFileState::Untracked),
        ],
        commits: Vec::new(),
        tags: Vec::new(),
        stashes: Vec::new(),
        remotes: Vec::new(),
        pending: None,
        message: None,
    }
}
/// Um retrato com duas branches, tags e um item guardado.
fn retrato_com_branches() -> GitView {
    GitView {
        head: Some("main".to_owned()),
        branches: vec![
            BranchItem {
                name: "main".to_owned(),
                current: true,
            ..BranchItem::default()
            },
            BranchItem {
                name: "feature/busca".to_owned(),
                current: false,
            ..BranchItem::default()
            },
        ],
        tags: vec!["v1.0".to_owned()],
        stashes: vec!["no meio do caminho".to_owned()],
        ..GitView::default()
    }
}

/// A árvore do projeto.
mod explorer;
/// O texto e as abas.
mod editor;
/// A área dividida em dois editores.
mod divisao;
/// As caixas de busca.
mod busca;
/// A lista de completação.
mod completacao;
/// O painel de terminais.
mod terminal;
/// O gerenciador de Git.
mod git;
/// A depuração e a inspeção.
mod depuracao;
/// As janelas sobrepostas.
mod janelas;
/// A moldura: barras, menus e roteamento.
mod moldura;
