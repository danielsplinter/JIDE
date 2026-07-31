//! O lado do shell de cada janela.
//!
//! A janela decide e devolve um `Outcome`; o que está aqui é a outra metade —
//! executar a decisão contra a sessão de edição, a fila de comandos e a barra
//! de estado, que a janela não alcança. Fica fora dos módulos das janelas
//! porque é código do shell: o guarda de arquitetura exige que um módulo de
//! feature não mencione o `IdeShell`, e é essa fronteira que o mantém honesto.

use super::generate::GenerateOutcome;
use super::inspection::InspectionRequest;
use super::new_item::NewItemOutcome;
use super::rename::RenameOutcome;
use super::settings::{SettingsOutcome, ToolSlot};
use super::type_search::{TypeSearchOutcome, WorkspaceSearchMode};
use super::*;
use crate::debugging::DebugVariableView;
use crate::search::{ContentSearchHit, TypeSearchHit};
use ide_domain::{AccessorPlan, Location};

impl IdeShell {
    // ---- Renomear ----
    /// Pede a renomeação do arquivo escolhido na árvore.
    ///
    /// O menu só marca; quem responde é a aplicação, que pergunta à linguagem
    /// onde o nome aparece no projeto — inclusive em arquivos fechados.
    pub fn request_rename(&mut self, path: PathBuf) {
        self.rename.request(path);
    }

    /// Arquivo cuja renomeação a tela quer ver respondida, se houver um.
    pub fn take_rename_request(&mut self) -> Option<PathBuf> {
        self.rename.take_request()
    }

    /// Abre a janela de renomear com o arquivo e o que será reescrito junto.
    pub fn show_rename(&mut self, path: PathBuf, references: Vec<Location>) {
        self.rename.show(path, references);
    }

    #[must_use]
    pub fn rename_open(&self) -> bool {
        self.rename.is_open()
    }

    /// Nome que está no campo, que é o que a confirmação aplica.
    #[must_use]
    pub fn rename_name(&self) -> String {
        self.rename.name()
    }

    /// Arquivos afetados, como aparecem na lista.
    #[must_use]
    pub fn rename_references(&self) -> Vec<String> {
        self.rename.references()
    }

    pub fn cancel_rename(&mut self) {
        self.rename.cancel();
    }

    /// Confirma a renomeação pendente na janela.
    pub fn apply_rename(&mut self) {
        let outcome = self.rename.confirm();
        self.apply_rename_outcome(outcome);
    }

    /// Executa o que a janela decidiu.
    ///
    /// A janela não alcança a sessão nem a fila de comandos: ela entrega a
    /// decisão, e é aqui que ela vira texto trocado e pedido à aplicação. O que
    /// está aberto é reescrito no buffer — a aba mantém cursor, desfazer e
    /// alterações não salvas —, e o que está fechado vai para o disco.
    pub(super) fn apply_rename_outcome(&mut self, outcome: RenameOutcome) {
        let decision = match outcome {
            RenameOutcome::Idle => return,
            RenameOutcome::Message(message) => {
                self.set_status_message(message);
                return;
            }
            RenameOutcome::Apply(decision) => decision,
        };
        let arquivos = decision.occurrences.len();
        let mut fechados = Vec::new();
        for (caminho, ranges) in decision.occurrences {
            let aberto = self
                .editor_area
                .session
                .tabs()
                .find(|documento| documento.path == caminho)
                .map(|documento| documento.id);
            match aberto {
                Some(id) => {
                    self.rewrite_open_document(id, &ranges, &decision.old_name, &decision.new_name);
                }
                None => fechados.push(FileOccurrences {
                    path: caminho,
                    ranges,
                }),
            }
        }
        self.commands
            .push(ApplicationCommand::RenameDocument(RenameDocumentRequest {
                from: decision.from,
                to: decision.to,
                old_name: decision.old_name,
                new_name: decision.new_name,
                occurrences: fechados,
            }));
        self.set_status_message(format!("Renomeando em {arquivos} arquivo(s)"));
    }

    /// Roteia o clique dentro da janela de renomear.
    pub(super) fn rename_pointer_down(&mut self, point: Point, size: Size) {
        self.place_overlay(size);
        let context = self.layout_context();
        let outcome = self
            .rename
            .pointer_down(&mut self.host, &context, point, size);
        self.apply_rename_outcome(outcome);
    }

    /// Movimento e soltura vão à janela: são o arrasto das barras da lista.
    pub(super) fn rename_pointer_event(&mut self, event: &UiEvent, size: Size) {
        let context = self.layout_context();
        self.rename.pointer_event(&context, event, size);
    }

    /// Teclas da janela de renomear: `Enter` confirma, `Esc` desiste.
    pub(super) fn rename_key(&mut self, key: &str, modifiers: Modifiers) -> bool {
        if !self.rename.is_open() {
            return false;
        }
        let outcome = self.rename.key(key, modifiers);
        self.apply_rename_outcome(outcome);
        true
    }

    /// Texto digitado enquanto a janela de renomear está aberta.
    pub(super) fn rename_text_input(&mut self, text: &str) -> bool {
        if !self.rename.is_open() {
            return false;
        }
        self.rename.text_input(text);
        true
    }

    pub(super) fn paint_rename(&mut self, commands: &mut Vec<PaintCommand>, size: Size) {
        let layout = self.layout_context();
        let mut paint = self.paint_context();
        if self.rename.paint(&self.host, &layout, &mut paint, size) {
            commands.extend(paint.into_commands());
        }
    }

    // ---- Gerar ----
    /// Pede à linguagem o plano de acessores. É o que o menu `Generate` aciona.
    pub fn request_accessors(&mut self, kind: AccessorKind) {
        self.generate.request(kind);
    }

    /// Pedido que a tela quer ver respondido, se houver um esperando.
    pub fn take_accessor_request(&mut self) -> Option<AccessorKind> {
        self.generate.take_request()
    }

    /// Campos escolhidos para o construtor, se houver um pedido esperando.
    pub fn take_constructor_request(&mut self) -> Option<(Vec<String>, DomainTextPosition)> {
        self.generate.take_constructor_request()
    }

    /// Escreve o construtor que a linguagem montou.
    ///
    /// `None` é o tipo já ter um construtor com a mesma assinatura: escrever
    /// outro não compilaria, e avisar é melhor do que entregar arquivo quebrado.
    ///
    /// Devolve se o documento mudou — é o que diz a quem chamou que o realce
    /// precisa ser pedido de novo, senão o código gerado fica sem cor até a
    /// primeira tecla.
    pub fn insert_constructor(
        &mut self,
        source: Option<String>,
        insert_at: DomainTextPosition,
    ) -> bool {
        let Some(source) = source else {
            self.set_status_message("Esse construtor já existe");
            return false;
        };
        let Some(document) = self.editor_area.session.active_mut() else {
            return false;
        };
        // A linha vem da linguagem, que sabe onde o tipo abre e fecha.
        let texto = document.buffer.text();
        let inicio = offset_of_line(texto, insert_at.line as usize);
        if document.buffer.replace(inicio..inicio, &source).is_err() {
            return false;
        }
        self.set_status_message("Construtor gerado");
        true
    }

    /// Abre a janela com o que a linguagem propôs gerar.
    pub fn show_accessor_plan(&mut self, kind: AccessorKind, plan: AccessorPlan) {
        if let Some(recusa) = self.generate.show(kind, plan) {
            self.set_status_message(recusa);
        }
    }

    #[must_use]
    pub fn generate_open(&self) -> bool {
        self.generate.is_open()
    }

    /// Campos oferecidos na janela, na ordem em que aparecem.
    #[must_use]
    pub fn generate_fields(&self) -> Vec<String> {
        self.generate.fields()
    }

    pub fn close_generate(&mut self) {
        self.generate.close();
    }

    /// Escreve o que foi escolhido na janela.
    ///
    /// `todos` ignora a marcação — é o botão que gera tudo de uma vez, sem
    /// obrigar a marcar campo por campo quando se quer a classe inteira.
    pub fn apply_generate(&mut self, todos: bool) {
        let outcome = self.generate.confirm(todos);
        self.apply_generate_outcome(outcome);
    }

    /// Executa o que a janela decidiu.
    ///
    /// A janela não alcança a sessão: ela entrega o trecho e a linha, e é aqui
    /// que isso vira texto no documento.
    pub(super) fn apply_generate_outcome(&mut self, outcome: GenerateOutcome) {
        match outcome {
            GenerateOutcome::Idle => {}
            GenerateOutcome::Message(message) => self.set_status_message(message),
            GenerateOutcome::Insert {
                text,
                line,
                message,
            } => {
                let Some(document) = self.editor_area.session.active_mut() else {
                    return;
                };
                // A linha vem da linguagem, que sabe onde o tipo fecha.
                let inicio = offset_of_line(document.buffer.text(), line as usize);
                if document.buffer.replace(inicio..inicio, &text).is_ok() {
                    self.set_status_message(message);
                }
            }
        }
    }

    pub(super) fn generate_pointer_down(&mut self, point: Point, size: Size) {
        self.place_overlay(size);
        let context = self.layout_context();
        let outcome = self
            .generate
            .pointer_down(&mut self.host, &context, point, size);
        self.apply_generate_outcome(outcome);
    }

    pub(super) fn paint_generate(&mut self, commands: &mut Vec<PaintCommand>, size: Size) {
        let layout = self.layout_context();
        let mut paint = self.paint_context();
        if self.generate.paint(&self.host, &layout, &mut paint, size) {
            commands.extend(paint.into_commands());
        }
    }

    // ---- Buscar ----
    pub(super) fn type_search_pointer_down(&mut self, point: Point, size: Size) {
        self.place_overlay(size);
        let context = self.layout_context();
        let outcome = self
            .search
            .pointer_down(&mut self.host, &context, point, size);
        self.apply_type_search_outcome(outcome);
    }

    pub(super) fn type_search_scroll(&mut self, point: Point, delta_lines: f32, size: Size) {
        let context = self.layout_context();
        self.search.scroll(&context, point, delta_lines, size);
    }

    /// Executa o que a busca decidiu.
    ///
    /// A janela não alcança a fila de comandos: ela diz qual consulta refazer e
    /// o que abrir, e é aqui que isso vira pedido à aplicação.
    pub(super) fn apply_type_search_outcome(&mut self, outcome: TypeSearchOutcome) {
        match outcome {
            TypeSearchOutcome::Idle => {}
            TypeSearchOutcome::Query { text, mode } => {
                let command = match mode {
                    WorkspaceSearchMode::Types => ApplicationCommand::SearchTypes(text),
                    WorkspaceSearchMode::Content => ApplicationCommand::SearchContent(text),
                };
                self.commands.push(command);
            }
            TypeSearchOutcome::Open(location) => {
                self.commands.push(ApplicationCommand::OpenDocument(
                    OpenDocumentRequest::new(location.path).at(
                        location.range.start.line as usize,
                        location.range.start.column as usize,
                    ),
                ));
            }
        }
    }

    /// Abre a busca de tipo por nome. É o que `Ctrl+L` pede.
    pub fn open_type_search(&mut self) {
        let outcome = self.search.open_types();
        self.apply_type_search_outcome(outcome);
    }

    /// Abre a mesma janela da busca de tipos no modo de conteúdo.
    pub fn open_content_search(&mut self) {
        let title = self.catalog.language_names.first().map_or_else(
            || "Buscar conteúdo".to_owned(),
            |language| format!("Buscar conteúdo em {language}"),
        );
        self.search.open_content(title);
    }

    #[must_use]
    pub const fn type_search_open(&self) -> bool {
        self.search.is_open()
    }

    pub fn close_type_search(&mut self) {
        self.search.close();
    }

    /// Entrega o que a linguagem encontrou.
    pub fn set_type_search_results(&mut self, results: Vec<TypeSearchHit>) {
        self.search.set_type_results(results);
    }

    /// Entrega as ocorrências encontradas dentro do escopo fornecido pela aplicação.
    pub fn set_content_search_results(&mut self, results: Vec<ContentSearchHit>) {
        self.search.set_content_results(results);
    }

    #[must_use]
    pub fn type_search_results(&self) -> &[TypeSearchHit] {
        self.search.type_results()
    }

    /// Digitação na busca de tipo. Devolve `true` quando consumiu.
    pub(super) fn type_search_text_input(&mut self, text: &str) -> bool {
        if !self.search.is_open() {
            return false;
        }
        let outcome = self.search.text_input(text);
        self.apply_type_search_outcome(outcome);
        true
    }

    /// Tecla na busca de tipo. Devolve `true` quando consumiu.
    pub(super) fn type_search_key(&mut self, key: &str) -> bool {
        if !self.search.is_open() {
            return false;
        }
        let outcome = self.search.key(key);
        self.apply_type_search_outcome(outcome);
        true
    }

    /// Desenha a janela de criação por cima de tudo.
    ///
    /// Moldura, véu e título são do `ModalHost`; os campos, os botões e as
    /// legendas são componentes da biblioteca. A IDE diz onde e o que.
    /// Desenha a busca de tipo: campo em cima, resultados embaixo.
    pub(super) fn paint_type_search(&self, commands: &mut Vec<PaintCommand>, size: Size) {
        let layout = self.layout_context();
        let mut paint = self.paint_context();
        if self
            .search
            .paint(&layout, &mut paint, size, &self.catalog.source_root_names)
        {
            commands.extend(paint.into_commands());
        }
    }

    // ---- Criar item ----
    #[must_use]
    pub const fn new_item_dialog_open(&self) -> bool {
        self.new_item.is_open()
    }

    /// Relata o que impediu a criação, mantendo a janela aberta.
    pub fn set_new_item_message(&mut self, message: impl Into<String>) {
        self.new_item.set_message(message);
    }

    pub fn close_new_item_dialog(&mut self) {
        self.new_item.close(&mut self.host);
    }

    /// Executa o que a janela de criação decidiu.
    ///
    /// A janela não alcança a fila de comandos: ela entrega o pedido montado, e
    /// é aqui que ele vira trabalho para a aplicação.
    pub(super) fn apply_new_item_outcome(&mut self, outcome: NewItemOutcome) {
        match outcome {
            NewItemOutcome::Idle => {}
            NewItemOutcome::Create(request) => {
                self.commands.push(ApplicationCommand::CreateItem(request));
            }
        }
    }

    pub(super) fn paint_new_item_dialog(&self, commands: &mut Vec<PaintCommand>, size: Size) {
        let layout = self.layout_context();
        let mut paint = self.paint_context();
        if self.new_item.paint(&self.host, &layout, &mut paint, size) {
            commands.extend(paint.into_commands());
        }
    }

    /// Roteia o clique dentro da janela de criação.
    pub(super) fn new_item_pointer_down(&mut self, point: Point, size: Size) {
        self.place_overlay(size);
        let outcome = self.new_item.pointer_down(&mut self.host, point);
        self.apply_new_item_outcome(outcome);
    }

    /// Tecla dentro da janela de criação. Devolve `true` quando a consumiu.
    pub(super) fn new_item_key(&mut self, key: &str) -> bool {
        if !self.new_item.is_open() {
            return false;
        }
        let outcome = self.new_item.key(&mut self.host, key);
        self.apply_new_item_outcome(outcome);
        true
    }

    /// Texto digitado na janela de criação. Devolve `true` quando o consumiu.
    pub(super) fn new_item_text_input(&mut self, text: &str) -> bool {
        if !self.new_item.is_open() {
            return false;
        }
        self.new_item.text_input(&mut self.host, text);
        true
    }

    // ---- Configurações ----
    pub fn open_settings_dialog(
        &mut self,
        toolchain_items: Vec<String>,
        selected_toolchain: usize,
    ) {
        self.settings.open(toolchain_items, selected_toolchain);
    }

    /// Repõe a lista da segunda escolha da seção, com uma delas marcada.
    pub fn set_secondary_tool_options(&mut self, items: Vec<String>, selected: Option<usize>) {
        self.settings.set_secondary_options(items, selected);
    }

    /// Segunda ferramenta escolhida na janela, para quem for aplicar.
    #[must_use]
    pub fn selected_secondary_tool(&self) -> Option<usize> {
        self.settings.selected_secondary()
    }

    /// Repõe a lista de toolchains e deixa uma delas escolhida, sem sair da transação.
    pub fn set_toolchain_options(&mut self, toolchain_items: Vec<String>, pending: usize) {
        self.settings
            .set_toolchain_options(toolchain_items, pending);
    }

    pub const fn settings_dialog_open(&self) -> bool {
        self.settings.is_open()
    }

    /// Escolhe a página apresentada pela janela de configurações.
    ///
    /// Abrir a janela pelo menu mantém a última página usada; atalhos que
    /// prometem uma página específica precisam declará-la.
    pub fn set_settings_page(&mut self, page: SettingsPage) {
        self.settings.set_page(page);
    }

    #[must_use]
    pub const fn settings_page(&self) -> SettingsPage {
        self.settings.page()
    }

    pub fn set_settings_message(&mut self, message: impl Into<String>) {
        self.settings.set_message(message);
    }

    pub(super) fn settings_dialog_pointer_down(&mut self, point: Point, size: Size) {
        self.place_overlay(size);
        let context = self.layout_context();
        let sections = self.catalog.settings_sections.clone();
        let outcome = self
            .settings
            .pointer_down(&mut self.host, &context, point, size, &sections);
        self.apply_settings_outcome(outcome);
    }

    /// Executa o que a janela de configurações decidiu.
    ///
    /// A janela não alcança a fila de comandos nem a barra de status: ela diz o
    /// que mudou, e é aqui que isso vira pedido à aplicação.
    pub(super) fn apply_settings_outcome(&mut self, outcome: SettingsOutcome) {
        match outcome {
            SettingsOutcome::Idle => {}
            SettingsOutcome::Browse(ToolSlot::Primary) => {
                self.commands.push(ApplicationCommand::BrowseToolchain);
            }
            SettingsOutcome::Browse(ToolSlot::Secondary) => {
                self.commands.push(ApplicationCommand::BrowseSecondaryTool);
            }
            SettingsOutcome::Save {
                toolchain,
                secondary,
            } => {
                if let Some(index) = toolchain {
                    self.commands
                        .push(ApplicationCommand::SelectToolchain(index));
                }
                if let Some(index) = secondary {
                    self.commands
                        .push(ApplicationCommand::SelectSecondaryTool(index));
                }
            }
            SettingsOutcome::Attach { host, port } => {
                self.commands
                    .push(ApplicationCommand::Debug(DebugRequest::Attach {
                        host: host.clone(),
                        port,
                    }));
                self.context.status_message =
                    format!("Conectando ao alvo de depuração {host}:{port}");
            }
        }
    }

    // ---- Inspecionar ----
    /// Recebe uma avaliação vinda do depurador e decide o que ela significa.
    ///
    /// Abrir a inspeção e executar código dentro dela chegam pelo mesmo evento,
    /// mas pedem coisas opostas: a primeira monta a árvore, a segunda **não pode
    /// desmontá-la**. Quem executa `m.setId(4L)` quer continuar olhando `m`, e não
    /// trocar a árvore pelo `void` que o método devolveu.
    pub fn inspection_result(
        &mut self,
        expression: String,
        value: DebugVariableView,
        fields: Vec<DebugVariableView>,
    ) {
        let requests = self.inspection.result(expression, value, fields);
        self.apply_inspection_requests(requests);
    }

    /// Abre a janela de inspeção com o valor avaliado e seus campos.
    pub fn show_inspection(
        &mut self,
        expression: impl Into<String>,
        value: DebugVariableView,
        fields: Vec<DebugVariableView>,
    ) {
        self.inspection.show(expression, value, fields);
    }

    /// Acrescenta os campos que o alvo revelou para um caminho.
    pub fn add_inspection_fields(&mut self, path: &str, fields: Vec<DebugVariableView>) {
        self.inspection.add_fields(path, fields);
    }

    /// Relata na janela o que a última execução respondeu.
    pub fn set_inspection_message(&mut self, message: impl Into<String>) {
        let requests = self.inspection.set_message(message);
        self.apply_inspection_requests(requests);
    }

    /// Executa o que a janela de inspeção pediu.
    ///
    /// A janela não alcança o depurador nem a barra de estado: ela diz o que
    /// precisa ser perguntado, e é aqui que isso vira pedido à aplicação.
    pub(super) fn apply_inspection_requests(&mut self, requests: Vec<InspectionRequest>) {
        for request in requests {
            match request {
                InspectionRequest::Status(message) => self.context.status_message = message,
                InspectionRequest::Evaluate(expression) => {
                    self.commands
                        .push(ApplicationCommand::Debug(DebugRequest::Evaluate(
                            expression,
                        )));
                }
                InspectionRequest::Expand(path) => {
                    self.commands
                        .push(ApplicationCommand::Debug(DebugRequest::ExpandInspection(
                            path,
                        )));
                }
            }
        }
    }

    #[must_use]
    pub const fn inspection_open(&self) -> bool {
        self.inspection.is_open()
    }

    /// Texto do editor de expressões da inspeção.
    #[must_use]
    pub fn inspection_source(&self) -> &str {
        self.inspection.source_text()
    }

    /// Executa o que está escrito no editor, no quadro atual.
    pub fn run_inspection_source(&mut self) {
        let attached = self.debug_panel.view.attached;
        let requests = self.inspection.run_source(attached);
        self.apply_inspection_requests(requests);
    }

    /// Tipo e filtro pedidos pelo ponto digitado no editor da inspeção.
    ///
    /// Ali não existe arquivo, então a completação comum — que descobre o tipo
    /// pela declaração — não tem de onde partir. O tipo do receptor vem de duas
    /// origens, nesta ordem: o que está parado no depurador, que é a única fonte
    /// para uma variável de quadro como `m`; e, se o nome não for uma delas, o
    /// próprio texto digitado, que passa a ser lido como nome de tipo. É o que
    /// faz uma classe alheia ao código depurado ser reconhecida como as outras —
    /// quem responde pelos membros é o índice do projeto, não o processo.
    #[must_use]
    pub fn inspection_member_context(&self) -> Option<(String, usize)> {
        if !self.inspection.is_open() {
            return None;
        }
        let (editor, source) = self.inspection.editor_and_source_ref();
        Some((source.text().to_owned(), editor.cursor()))
    }

    #[must_use]
    pub fn inspection_member_target(&self, receiver: &str, prefix: String) -> (String, String) {
        let type_name = self
            .debug_type_of(receiver)
            .unwrap_or_else(|| receiver.to_owned());
        (type_name, prefix)
    }

    /// Digitação dentro da janela de inspeção. Devolve `true` quando consumiu.
    pub(super) fn inspection_text_input(&mut self, text: &str) -> bool {
        if !self.inspection.is_open() || !self.inspection.is_source_focused() {
            return false;
        }
        self.inspection.text_input(text);
        true
    }

    /// Tecla dentro da janela de inspeção. Devolve `true` quando consumiu.
    pub(super) fn inspection_key(&mut self, key: &str, modifiers: Modifiers) -> bool {
        if !self.inspection.is_open() || !self.inspection.is_source_focused() {
            return false;
        }
        // Ctrl+Enter executa: a mão já está no teclado, escrevendo a expressão.
        if modifiers.control && key.eq_ignore_ascii_case("enter") {
            self.run_inspection_source();
            return true;
        }
        // A lista aberta tem precedência sobre o texto, como no editor da janela.
        if self.completion_key(key) {
            return true;
        }
        self.inspection
            .editor_key(key, modifiers.shift, modifiers.control);
        true
    }

    /// Expressão que está sendo inspecionada.
    #[must_use]
    pub fn inspected_expression(&self) -> Option<&str> {
        self.inspection.expression()
    }

    pub fn close_inspection(&mut self) {
        self.inspection.close();
    }

    /// Roteia o clique dentro da janela de inspeção.
    pub(super) fn inspection_pointer_down(&mut self, point: Point, size: Size) {
        self.place_overlay(size);
        let context = self.layout_context();
        let attached = self.debug_panel.view.attached;
        let requests =
            self.inspection
                .pointer_down(&mut self.host, &context, point, size, attached);
        self.apply_inspection_requests(requests);
    }
}
