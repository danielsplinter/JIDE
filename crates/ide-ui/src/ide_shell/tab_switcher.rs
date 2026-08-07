//! A troca de abas por `Ctrl+Tab`.
//!
//! Enquanto o `Ctrl` estiver segurado a janela fica aberta e cada `Tab` desce um
//! item; soltar o `Ctrl` ativa a aba destacada e fecha. É o único gesto da IDE em
//! que a **soltura** de um modificador conclui alguma coisa — por isso a
//! aplicação precisa avisar quando ele solta, e não só quando ele desce.

use std::path::{Path, PathBuf};

use super::{JANELA_TITULO};
use ui_api::{LayoutContext, PaintContext, Widget};
use ui_components::{ListView, ModalHost};
use ui_core::{Point, Rect, Size, Spacing, WidgetId};
use ui_host::UiHost;
use ui_layout_api::{EdgeInsets, LayoutStyle};

const MODAL_ID: WidgetId = WidgetId(10_480);
const LIST_ID: WidgetId = WidgetId(10_481);
/// Estreita: cada linha é um nome de arquivo e o caminho dele, não uma frase.
const PANEL_SIZE: Size = Size::new(520.0, 320.0);
const ROW_HEIGHT: f32 = 26.0;
/// Quantas linhas cabem no painel; o resto rola junto com o destaque.
const VISIBLE_ROWS: usize = 10;

/// Declara a janela ao anfitrião: painel e lista.
pub(super) fn attach(host: &mut UiHost, layer: WidgetId) {
    let _ = host.declare(
        layer,
        MODAL_ID,
        LayoutStyle {
            width: Some(PANEL_SIZE.width),
            height: Some(PANEL_SIZE.height),
            padding: EdgeInsets::only(JANELA_TITULO, Spacing::LG, Spacing::LG, Spacing::LG),
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

fn area(host: &UiHost, id: WidgetId) -> Rect {
    host.bounds(id).unwrap_or(Rect::new(0.0, 0.0, 0.0, 0.0))
}

/// Uma aba aberta, como a janela precisa conhecê-la.
pub(super) struct AbaAberta {
    /// A identidade do documento, que é o que ativa a aba depois.
    pub(super) documento: u64,
    pub(super) caminho: PathBuf,
}

pub(super) struct TabSwitcherSurface {
    modal: ModalHost,
    abas: Vec<AbaAberta>,
    /// Linha destacada, contada sobre a lista inteira.
    selected: usize,
    /// Primeira linha visível, que é onde a rolagem mora.
    first_visible: usize,
    /// Raiz do projeto, para encurtar o caminho de cada linha.
    raiz: PathBuf,
}

impl Default for TabSwitcherSurface {
    fn default() -> Self {
        Self {
            modal: ModalHost::new(MODAL_ID, "Abas abertas", PANEL_SIZE),
            abas: Vec::new(),
            selected: 0,
            first_visible: 0,
            raiz: PathBuf::new(),
        }
    }
}

impl TabSwitcherSurface {
    #[must_use]
    pub(super) const fn is_open(&self) -> bool {
        self.modal.is_open()
    }

    /// Abre a janela já com a **próxima** aba destacada.
    ///
    /// Destacar a atual faria o gesto mais curto — um `Ctrl+Tab` — não levar a
    /// lugar nenhum, que é justamente o que ele existe para fazer.
    pub(super) fn open(&mut self, abas: Vec<AbaAberta>, ativa: Option<usize>, raiz: &Path) {
        if abas.len() < 2 {
            // Com uma aba só não há para onde ir, e uma janela que não muda nada
            // é ruído na frente do editor.
            return;
        }
        self.selected = ativa.map_or(0, |indice| (indice + 1) % abas.len());
        self.abas = abas;
        self.raiz = raiz.to_path_buf();
        self.first_visible = 0;
        self.reveal_selection();
        self.modal.open();
    }

    /// Mais um `Tab` com o `Ctrl` segurado: desce um item, e do fim volta ao topo.
    pub(super) fn advance(&mut self) {
        if self.abas.is_empty() {
            return;
        }
        self.selected = (self.selected + 1) % self.abas.len();
        self.reveal_selection();
    }

    /// O `Ctrl` soltou: a aba destacada é a escolhida, e a janela some.
    ///
    /// Devolve `None` quando não havia nada a escolher — assim quem chama pode
    /// perguntar sempre, sem antes checar se a janela estava aberta.
    pub(super) fn commit(&mut self) -> Option<u64> {
        if !self.modal.is_open() {
            return None;
        }
        let escolhida = self.abas.get(self.selected).map(|aba| aba.documento);
        self.close();
        escolhida
    }

    pub(super) fn close(&mut self) {
        self.modal.close();
        self.abas.clear();
        self.selected = 0;
        self.first_visible = 0;
    }

    /// O clique escolhe a linha e conclui, como soltar o `Ctrl` faria.
    pub(super) fn pointer_down(&mut self, host: &mut UiHost, point: Point) -> Option<u64> {
        let list = area(host, LIST_ID);
        match host.click(point).target {
            Some(LIST_ID) => {}
            // Dentro do painel e fora da lista, o clique não escolhe nada.
            Some(MODAL_ID) => return None,
            // Fora do painel, o clique dispensa a janela sem trocar de aba.
            _ => {
                self.close();
                return None;
            }
        }
        let linha =
            self.first_visible + ((point.y - list.origin.y) / ROW_HEIGHT).floor().max(0.0) as usize;
        if linha >= self.abas.len() {
            return None;
        }
        self.selected = linha;
        self.commit()
    }

    pub(super) fn paint(
        &self,
        host: &UiHost,
        layout: &LayoutContext,
        paint: &mut PaintContext,
        size: Size,
    ) -> bool {
        if !self.modal.is_open() {
            return false;
        }
        let mut modal = self.modal.clone();
        modal.layout(layout, Rect::new(0.0, 0.0, size.width, size.height));
        modal.paint(paint);
        let mut list = ListView::new(LIST_ID, self.labels()).with_row_height(ROW_HEIGHT);
        list.set_selected(self.selected.checked_sub(self.first_visible));
        list.layout(layout, area(host, LIST_ID));
        list.paint(paint);
        true
    }

    /// As linhas visíveis: o nome do arquivo e onde ele está.
    fn labels(&self) -> Vec<String> {
        self.abas
            .iter()
            .skip(self.first_visible)
            .take(VISIBLE_ROWS)
            .map(|aba| {
                let nome = aba
                    .caminho
                    .file_name()
                    .and_then(|nome| nome.to_str())
                    .unwrap_or("?");
                let pasta = aba
                    .caminho
                    .parent()
                    .and_then(|pasta| pasta.strip_prefix(&self.raiz).ok().or(Some(pasta)))
                    .map(|pasta| pasta.to_string_lossy().to_string())
                    .unwrap_or_default();
                if pasta.is_empty() {
                    nome.to_owned()
                } else {
                    format!("{nome}  —  {pasta}")
                }
            })
            .collect()
    }

    /// Traz a lista junto quando o destaque sai pelo pé ou pela cabeça.
    fn reveal_selection(&mut self) {
        if self.selected < self.first_visible {
            self.first_visible = self.selected;
        } else if self.selected >= self.first_visible + VISIBLE_ROWS {
            self.first_visible = self.selected + 1 - VISIBLE_ROWS;
        }
        self.first_visible = self
            .first_visible
            .min(self.abas.len().saturating_sub(VISIBLE_ROWS));
    }

    /// Linha destacada e primeira visível, para os testes.
    #[cfg(test)]
    pub(super) const fn scroll_state(&self) -> (usize, usize) {
        (self.selected, self.first_visible)
    }
}
