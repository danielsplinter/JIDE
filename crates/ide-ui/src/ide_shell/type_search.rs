//! A janela de busca: por nome de tipo e por conteúdo dos arquivos.
//!
//! As duas são a mesma janela em modos diferentes — muda o título, a legenda do
//! campo e o que a lista mostra. Como as outras superfícies, ela não alcança a
//! fila de comandos: o que sai daqui é a consulta a refazer ou o lugar a abrir.

use ide_domain::Location;
use super::{JANELA_TITULO};
use ui_api::{EventContext, LayoutContext, PaintContext, Widget};
use ui_components::{ListView, ModalHost, Spinner, TextInput};
use ui_core::{Point, Rect, Size, Spacing, UiEvent, WidgetId};
use ui_host::UiHost;
use ui_layout_api::{EdgeInsets, LayoutStyle};

use crate::search::{ContentSearchHit, TypeSearchHit};

const MODAL_ID: WidgetId = WidgetId(10_410);
const INPUT_ID: WidgetId = WidgetId(10_411);
const LIST_ID: WidgetId = WidgetId(10_412);
const SPINNER_ID: WidgetId = WidgetId(10_413);
/// A janela é larga porque cada linha traz o caminho junto do nome.
const PANEL_SIZE: Size = Size::new(760.0, 420.0);
const ROW_HEIGHT: f32 = 26.0;
/// Quantas linhas cabem: é o que decide a rolagem e o que a seta revela.
pub(super) const VISIBLE_ROWS: usize = 12;

/// Declara a janela ao anfitrião: painel, campo e lista, e como se arrumam.
///
/// A coluna reproduz o que a conta à mão fazia — 16 de margem, o campo a 56 do
/// alto, 12 de folga até a lista —, com a diferença de que agora está dito uma
/// vez, e a lista fica com o que sobra em vez de ter a altura subtraída.
pub(super) fn attach(host: &mut UiHost, layer: WidgetId) {
    let _ = host.declare(
        layer,
        MODAL_ID,
        LayoutStyle {
            width: Some(PANEL_SIZE.width),
            height: Some(PANEL_SIZE.height),
            padding: EdgeInsets::only(JANELA_TITULO, Spacing::LG, Spacing::LG, Spacing::LG),
            gap: Spacing::MD,
            ..LayoutStyle::default()
        },
    );
    let _ = host.declare(
        MODAL_ID,
        INPUT_ID,
        LayoutStyle {
            height: Some(34.0),
            ..LayoutStyle::default()
        },
    );
    let _ = host.declare(
        MODAL_ID,
        LIST_ID,
        LayoutStyle {
            flex_grow: 1.0,
            ..LayoutStyle::default()
        },
    );
}

/// A área que o arranjo deu a uma peça da janela.
fn area(host: &UiHost, id: WidgetId) -> Rect {
    host.bounds(id).unwrap_or(Rect::new(0.0, 0.0, 0.0, 0.0))
}

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
    /// Onde o giro está, quando há busca em curso.
    ///
    /// `None` é "nada rodando". A fase vem de fora a cada quadro: a janela não
    /// tem relógio, e o componente também não.
    searching: Option<f32>,
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
            searching: None,
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

    /// Escreve no campo o que a busca já sabe que se procura.
    ///
    /// Quem chegou aqui por `Ctrl+Shift+clique` não digitou nada, e um campo em
    /// branco esconde **de que nome** aquela lista fala — e tira a chance de
    /// corrigir a pergunta sem refazer o gesto.
    pub(super) fn set_query(&mut self, texto: impl Into<String>) {
        self.query = texto.into();
    }

    pub(super) fn close(&mut self) {
        self.modal.close();
    }

    /// Diz que há busca em curso, e onde o giro está.
    ///
    /// **A espera aparece onde o resultado vai aparecer.** Antes ela era uma
    /// linha na barra de estado, no canto oposto ao que se está olhando: quem
    /// abre a janela de busca olha para a lista, e a lista ficava vazia sem dizer
    /// se estava procurando ou se não achou nada. São duas situações diferentes
    /// que se pareciam.
    pub(super) fn set_searching(&mut self, phase: Option<f32>) {
        self.searching = phase;
    }

    /// Entrega o que a linguagem encontrou.
    pub(super) fn set_type_results(&mut self, results: Vec<TypeSearchHit>) {
        self.searching = None;
        self.type_results = results;
        self.content_results.clear();
        self.selected = 0;
        self.first_visible = 0;
    }

    /// Entrega as ocorrências encontradas dentro do escopo da aplicação.
    pub(super) fn set_content_results(&mut self, results: Vec<ContentSearchHit>) {
        self.searching = None;
        self.content_results = results;
        self.type_results.clear();
        self.selected = 0;
        self.first_visible = 0;
    }

    #[must_use]
    pub(super) fn type_results(&self) -> &[TypeSearchHit] {
        &self.type_results
    }

    pub(super) fn pointer_down(&mut self, host: &mut UiHost, point: Point) -> TypeSearchOutcome {
        let list = area(host, LIST_ID);
        match host.click(point).target {
            Some(LIST_ID) => {}
            // Dentro do painel e fora da lista, o clique não escolhe nada.
            Some(MODAL_ID) => return TypeSearchOutcome::Idle,
            // O que sobrou é o véu, atrás do painel: ali o clique dispensa a
            // janela.
            _ => {
                self.close();
                return TypeSearchOutcome::Idle;
            }
        }
        let row =
            self.first_visible + ((point.y - list.origin.y) / ROW_HEIGHT).floor().max(0.0) as usize;
        if row < self.result_len() {
            self.selected = row;
            return self.open_selected();
        }
        TypeSearchOutcome::Idle
    }

    /// A roda dentro da janela.
    ///
    /// Se ela caiu na lista é o anfitrião quem diz, pelo acerto: a área é a
    /// mesma que foi declarada na pilha, então rolar e clicar concordam sem que
    /// ninguém repita a conta do retângulo.
    pub(super) fn scroll(&mut self, host: &UiHost, point: Point, delta_lines: f32) {
        if host.hit_test(point).next() != Some(LIST_ID) {
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
        host: &UiHost,
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
        let (input, list_rect) = (area(host, INPUT_ID), area(host, LIST_ID));
        let placeholder = match self.mode {
            WorkspaceSearchMode::Types => "Nome da classe, interface, record ou enum",
            WorkspaceSearchMode::Content => "Texto nos arquivos do escopo do projeto",
        };
        let mut field = TextInput::new(INPUT_ID, &self.query).with_placeholder(placeholder);
        field.event(&mut EventContext::default(), &UiEvent::FocusGained);
        field.layout(layout, input);
        field.paint(paint);

        // Enquanto procura, o lugar do resultado mostra o giro — e **só** o
        // giro. Desenhar a lista velha embaixo faria a busca anterior parecer a
        // resposta desta.
        if let Some(phase) = self.searching {
            let mut giro = Spinner::new(SPINNER_ID, "Procurando").with_phase(phase);
            giro.layout(layout, list_rect);
            giro.paint(paint);
            return true;
        }

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

    /// Área da lista, para quem precisa apontar um gesto dentro dela.
    #[cfg(test)]
    pub(super) fn list_area(host: &UiHost) -> Rect {
        area(host, LIST_ID)
    }

    /// Linha destacada e primeira visível, para os testes.
    #[cfg(test)]
    pub(super) const fn scroll_state(&self) -> (usize, usize) {
        (self.selected, self.first_visible)
    }
}
