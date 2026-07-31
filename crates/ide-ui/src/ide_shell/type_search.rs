//! A janela de busca: por nome de tipo e por conteúdo dos arquivos.
//!
//! As duas são a mesma janela em modos diferentes — muda o título, a legenda do
//! campo e o que a lista mostra. Como as outras superfícies, ela não alcança a
//! fila de comandos: o que sai daqui é a consulta a refazer ou o lugar a abrir.

use ide_domain::Location;
use ui_api::{EventContext, LayoutContext, PaintContext, Widget};
use ui_components::{ListView, ModalHost, TextInput};
use ui_core::{Point, Rect, Size, UiEvent, WidgetId};
use ui_host::UiHost;
use ui_layout_api::LayoutStyle;
use ui_layout_taffy::TaffyLayoutEngine;

use crate::search::{ContentSearchHit, TypeSearchHit};

const MODAL_ID: WidgetId = WidgetId(10_060);
const INPUT_ID: WidgetId = WidgetId(10_061);
const LIST_ID: WidgetId = WidgetId(10_062);
/// A janela é larga porque cada linha traz o caminho junto do nome.
const PANEL_SIZE: Size = Size::new(760.0, 420.0);
const ROW_HEIGHT: f32 = 26.0;
/// Quantas linhas cabem: é o que decide a rolagem e o que a seta revela.
pub(super) const VISIBLE_ROWS: usize = 12;
/// Raiz do anfitrião desta janela; não é desenhada.
const HOST_ROOT_ID: WidgetId = WidgetId(10_067);

/// Em que a janela está buscando.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum WorkspaceSearchMode {
    Types,
    Content,
}

/// O que a janela concluiu, para o shell executar.
pub(super) enum TypeSearchOutcome {
    /// O gesto não concluiu nada.
    Idle,
    /// Refazer a busca com este texto.
    Query {
        text: String,
        mode: WorkspaceSearchMode,
    },
    /// Abrir o que foi escolhido.
    Open(Location),
}

pub(super) struct TypeSearchSurface {
    modal: ModalHost,
    mode: WorkspaceSearchMode,
    query: String,
    type_results: Vec<TypeSearchHit>,
    content_results: Vec<ContentSearchHit>,
    /// Linha destacada, contada sobre a lista inteira.
    selected: usize,
    /// Primeira linha visível, que é onde a rolagem mora.
    first_visible: usize,
    /// O runtime da janela: as áreas e o acerto.
    host: UiHost,
}

impl Default for TypeSearchSurface {
    fn default() -> Self {
        Self {
            modal: ModalHost::new(MODAL_ID, "Ir para o tipo", PANEL_SIZE),
            mode: WorkspaceSearchMode::Types,
            query: String::new(),
            type_results: Vec::new(),
            content_results: Vec::new(),
            selected: 0,
            first_visible: 0,
            host: UiHost::new(
                HOST_ROOT_ID,
                LayoutStyle::default(),
                Box::new(TaffyLayoutEngine),
            ),
        }
    }
}

impl TypeSearchSurface {
    #[must_use]
    pub(super) const fn is_open(&self) -> bool {
        self.modal.is_open()
    }

    /// Abre a busca de tipo por nome. É o que `Ctrl+L` pede.
    ///
    /// A consulta vazia nasce mostrando os tipos existentes, e por isso ela sai
    /// daqui como consulta a fazer.
    pub(super) fn open_types(&mut self) -> TypeSearchOutcome {
        self.reset(WorkspaceSearchMode::Types);
        self.modal.set_title("Ir para o tipo");
        self.modal.open();
        TypeSearchOutcome::Query {
            text: String::new(),
            mode: WorkspaceSearchMode::Types,
        }
    }

    /// Abre a mesma janela no modo de conteúdo, com o título que o shell montou.
    ///
    /// A consulta vazia não é enviada: ao contrário de uma lista de tipos, cada
    /// linha vazia de cada arquivo não é um resultado útil.
    pub(super) fn open_content(&mut self, title: impl Into<String>) {
        self.reset(WorkspaceSearchMode::Content);
        self.modal.set_title(title);
        self.modal.open();
    }

    pub(super) fn close(&mut self) {
        self.modal.close();
    }

    /// Entrega o que a linguagem encontrou.
    pub(super) fn set_type_results(&mut self, results: Vec<TypeSearchHit>) {
        self.type_results = results;
        self.content_results.clear();
        self.selected = 0;
        self.first_visible = 0;
    }

    /// Entrega as ocorrências encontradas dentro do escopo da aplicação.
    pub(super) fn set_content_results(&mut self, results: Vec<ContentSearchHit>) {
        self.content_results = results;
        self.type_results.clear();
        self.selected = 0;
        self.first_visible = 0;
    }

    #[must_use]
    pub(super) fn type_results(&self) -> &[TypeSearchHit] {
        &self.type_results
    }

    pub(super) fn pointer_down(
        &mut self,
        context: &LayoutContext,
        point: Point,
        size: Size,
    ) -> TypeSearchOutcome {
        let (_, list) = self.geometry(context, size);
        // O painel entra como área para o anfitrião distinguir "dentro da
        // janela" de "fora dela", que é o que decide se ela se dispensa.
        self.host.clear_placement();
        self.host.place(MODAL_ID, self.modal.panel_bounds());
        self.host.place(LIST_ID, list);
        let alvo = self.host.click(point).target;
        if alvo != Some(LIST_ID) {
            // Clicar fora da lista não escolhe nada, e clicar fora do painel
            // dispensa a janela.
            if alvo.is_none() {
                self.close();
            }
            return TypeSearchOutcome::Idle;
        }
        let row =
            self.first_visible + ((point.y - list.origin.y) / ROW_HEIGHT).floor().max(0.0) as usize;
        if row < self.result_len() {
            self.selected = row;
            return self.open_selected();
        }
        TypeSearchOutcome::Idle
    }

    pub(super) fn scroll(
        &mut self,
        context: &LayoutContext,
        point: Point,
        delta_lines: f32,
        size: Size,
    ) {
        let (_, list) = self.geometry(context, size);
        if !list.contains(point) {
            return;
        }
        self.first_visible = self
            .first_visible
            .saturating_add_signed(delta_lines.round() as isize)
            .min(self.max_first_visible());
    }

    /// Tecla na busca.
    pub(super) fn key(&mut self, key: &str) -> TypeSearchOutcome {
        match key.to_ascii_lowercase().as_str() {
            "backspace" => {
                self.query.pop();
                return TypeSearchOutcome::Query {
                    text: self.query.clone(),
                    mode: self.mode,
                };
            }
            "arrowdown" => {
                self.selected = (self.selected + 1).min(self.result_len().saturating_sub(1));
                self.reveal_selection();
            }
            "arrowup" => {
                self.selected = self.selected.saturating_sub(1);
                self.reveal_selection();
            }
            "enter" => return self.open_selected(),
            "escape" => self.close(),
            _ => {}
        }
        TypeSearchOutcome::Idle
    }

    /// Digitação na busca.
    pub(super) fn text_input(&mut self, text: &str) -> TypeSearchOutcome {
        self.query.push_str(text);
        TypeSearchOutcome::Query {
            text: self.query.clone(),
            mode: self.mode,
        }
    }

    /// Desenha a busca: campo em cima, resultados embaixo.
    ///
    /// Janela, campo e lista são da biblioteca; a IDE diz o que cada um mostra.
    pub(super) fn paint(
        &self,
        layout: &LayoutContext,
        paint: &mut PaintContext,
        size: Size,
        source_root_names: &[String],
    ) -> bool {
        if !self.modal.is_open() {
            return false;
        }
        let mut modal = self.modal.clone();
        modal.layout(layout, Rect::new(0.0, 0.0, size.width, size.height));
        modal.paint(paint);
        let (input, list_rect) = panel_geometry(modal.panel_bounds());
        let placeholder = match self.mode {
            WorkspaceSearchMode::Types => "Nome da classe, interface, record ou enum",
            WorkspaceSearchMode::Content => "Texto nos arquivos do escopo do projeto",
        };
        let mut field = TextInput::new(INPUT_ID, &self.query).with_placeholder(placeholder);
        field.event(&mut EventContext::default(), &UiEvent::FocusGained);
        field.layout(layout, input);
        field.paint(paint);

        let mut list =
            ListView::new(LIST_ID, self.labels(source_root_names)).with_row_height(ROW_HEIGHT);
        list.set_selected(self.selected.checked_sub(self.first_visible));
        list.layout(layout, list_rect);
        list.paint(paint);
        true
    }

    /// As linhas visíveis, já rotuladas com o caminho encurtado.
    fn labels(&self, source_root_names: &[String]) -> Vec<String> {
        match self.mode {
            WorkspaceSearchMode::Types => self
                .type_results
                .iter()
                .skip(self.first_visible)
                .take(VISIBLE_ROWS)
                .map(|hit| hit.label(source_root_names))
                .collect(),
            WorkspaceSearchMode::Content => self
                .content_results
                .iter()
                .skip(self.first_visible)
                .take(VISIBLE_ROWS)
                .map(|hit| hit.label(source_root_names))
                .collect(),
        }
    }

    /// Abre o item destacado e fecha a janela.
    fn open_selected(&mut self) -> TypeSearchOutcome {
        let location = match self.mode {
            WorkspaceSearchMode::Types => self
                .type_results
                .get(self.selected)
                .map(|hit| hit.location.clone()),
            WorkspaceSearchMode::Content => self
                .content_results
                .get(self.selected)
                .map(|hit| hit.location.clone()),
        };
        let Some(location) = location else {
            return TypeSearchOutcome::Idle;
        };
        self.close();
        TypeSearchOutcome::Open(location)
    }

    /// Traz a lista junto quando a seleção sai pelo pé ou pela cabeça.
    fn reveal_selection(&mut self) {
        if self.selected < self.first_visible {
            self.first_visible = self.selected;
        } else if self.selected >= self.first_visible + VISIBLE_ROWS {
            self.first_visible = self.selected + 1 - VISIBLE_ROWS;
        }
        self.first_visible = self.first_visible.min(self.max_first_visible());
    }

    fn max_first_visible(&self) -> usize {
        self.result_len().saturating_sub(VISIBLE_ROWS)
    }

    fn result_len(&self) -> usize {
        match self.mode {
            WorkspaceSearchMode::Types => self.type_results.len(),
            WorkspaceSearchMode::Content => self.content_results.len(),
        }
    }

    fn reset(&mut self, mode: WorkspaceSearchMode) {
        self.mode = mode;
        self.query.clear();
        self.type_results.clear();
        self.content_results.clear();
        self.selected = 0;
        self.first_visible = 0;
    }

    /// O campo e a lista, num lugar só, para o clique acertar o que foi desenhado.
    pub(super) fn geometry(&mut self, context: &LayoutContext, size: Size) -> (Rect, Rect) {
        self.modal
            .layout(context, Rect::new(0.0, 0.0, size.width, size.height));
        panel_geometry(self.modal.panel_bounds())
    }

    /// Linha destacada e primeira visível, para os testes.
    #[cfg(test)]
    pub(super) const fn scroll_state(&self) -> (usize, usize) {
        (self.selected, self.first_visible)
    }
}

fn panel_geometry(panel: Rect) -> (Rect, Rect) {
    let input = Rect::new(
        panel.origin.x + 16.0,
        panel.origin.y + 56.0,
        panel.size.width - 32.0,
        34.0,
    );
    let list = Rect::new(
        panel.origin.x + 16.0,
        input.origin.y + input.size.height + 12.0,
        panel.size.width - 32.0,
        (panel.origin.y + panel.size.height - 16.0) - (input.origin.y + input.size.height + 12.0),
    );
    (input, list)
}
