//! A janela de `Generate`: campos com caixa de marcação, **All** e **OK**.
//!
//! Serve às quatro opções do menu — construtor, getter, setter e o par — porque
//! o que muda entre elas é o que a linguagem escreve, não a tela. Como a de
//! renomear, ela decide e o shell executa: a fronteira é o [`GenerateOutcome`].

use ide_domain::{AccessorCandidate, AccessorKind, AccessorPlan, TextPosition};
use ui_api::{EventContext, LayoutContext, PaintContext, Widget};
use ui_components::{
    Button, CellWidth, Checkbox, ComposedCell, ComposedList, ComposedRow, Label, ModalHost,
};
use ui_core::{Point, Rect, ScrollEvent, Size, UiEvent, WidgetId};

use super::primary_pointer;

const MODAL_ID: WidgetId = WidgetId(10_050);
const LIST_ID: WidgetId = WidgetId(10_052);
const ALL_ID: WidgetId = WidgetId(10_053);
const OK_ID: WidgetId = WidgetId(10_054);
const ROW_ID: WidgetId = WidgetId(10_055);
/// Altura de uma linha da lista, também usada pelo passo da roda.
pub(super) const ROW_HEIGHT: f32 = 30.0;
const PANEL_SIZE: Size = Size::new(520.0, 420.0);

/// O que a janela conclui, para o shell executar.
pub(super) enum GenerateOutcome {
    /// O gesto não concluiu nada.
    Idle,
    /// Nada a gerar, com o que dizer na barra de estado.
    Message(String),
    /// Trecho pronto, para entrar no começo da linha indicada.
    Insert {
        text: String,
        line: u32,
        message: String,
    },
}

/// O que a janela mostra e o que o usuário marcou.
struct GenerateState {
    kind: AccessorKind,
    candidates: Vec<AccessorCandidate>,
    insert_at: TextPosition,
    /// Marcados, um por candidato.
    checked: Vec<bool>,
    /// A lista, mantida entre quadros: recriá-la jogaria fora a rolagem e a
    /// deixaria sem receber evento nenhum.
    list: ComposedList,
}

#[derive(Default)]
pub(super) struct GenerateSurface {
    modal: Option<ModalHost>,
    state: Option<GenerateState>,
    /// Acessor pedido cuja resposta da linguagem ainda não chegou.
    pending_kind: Option<AccessorKind>,
    /// Construtor escolhido cuja fonte a linguagem ainda não devolveu.
    pending_constructor: Option<(Vec<String>, TextPosition)>,
}

impl GenerateSurface {
    fn modal(&mut self) -> &mut ModalHost {
        self.modal
            .get_or_insert_with(|| ModalHost::new(MODAL_ID, "Generate", PANEL_SIZE))
    }

    pub(super) fn request(&mut self, kind: AccessorKind) {
        self.pending_kind = Some(kind);
    }

    pub(super) fn take_request(&mut self) -> Option<AccessorKind> {
        self.pending_kind.take()
    }

    pub(super) fn take_constructor_request(&mut self) -> Option<(Vec<String>, TextPosition)> {
        self.pending_constructor.take()
    }

    #[must_use]
    pub(super) fn is_open(&self) -> bool {
        self.modal.as_ref().is_some_and(ModalHost::is_open)
    }

    /// Abre com o que a linguagem propôs gerar.
    ///
    /// O construtor lista **todos** os campos, porque a escolha é sobre quais
    /// entram por parâmetro; os acessores listam só o que falta, porque oferecer
    /// o que já existe daria a escolher algo que não seria escrito.
    pub(super) fn show(&mut self, kind: AccessorKind, plan: AccessorPlan) -> Option<String> {
        let candidates: Vec<AccessorCandidate> = match kind {
            AccessorKind::Constructor => plan.candidates,
            _ => plan
                .candidates
                .into_iter()
                .filter(|candidate| candidate.source.is_some())
                .collect(),
        };
        if candidates.is_empty() && kind != AccessorKind::Constructor {
            return Some("Todos os campos já têm esse acessor".to_owned());
        }
        let checked = vec![false; candidates.len()];
        let list = build_list(&candidates, &checked);
        self.state = Some(GenerateState {
            kind,
            checked,
            candidates,
            insert_at: plan.insert_at,
            list,
        });
        let titulo = match kind {
            AccessorKind::Getter => "Generate — Getter",
            AccessorKind::Setter => "Generate — Setter",
            AccessorKind::Both => "Generate — Getter and Setter",
            AccessorKind::Constructor => "Generate — Constructor",
        };
        let modal = self.modal();
        modal.set_title(titulo);
        modal.open();
        None
    }

    /// Campos oferecidos, na ordem em que aparecem.
    #[must_use]
    pub(super) fn fields(&self) -> Vec<String> {
        self.state
            .as_ref()
            .map(|state| {
                state
                    .candidates
                    .iter()
                    .map(|candidate| candidate.field.clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    pub(super) fn close(&mut self) {
        if let Some(modal) = self.modal.as_mut() {
            modal.close();
        }
        self.state = None;
    }

    /// Confirma. `todos` ignora a marcação — é o botão que gera tudo de uma vez.
    pub(super) fn confirm(&mut self, todos: bool) -> GenerateOutcome {
        let Some(state) = self.state.take() else {
            return GenerateOutcome::Idle;
        };
        if let Some(modal) = self.modal.as_mut() {
            modal.close();
        }
        let escolhido =
            |index: usize| todos || state.checked.get(index).copied().unwrap_or_default();
        if state.kind == AccessorKind::Constructor {
            let fields = state
                .candidates
                .iter()
                .enumerate()
                .filter(|(index, _)| escolhido(*index))
                .map(|(_, candidate)| candidate.field.clone())
                .collect();
            self.pending_constructor = Some((fields, state.insert_at));
            return GenerateOutcome::Idle;
        }
        let texto: String = state
            .candidates
            .iter()
            .enumerate()
            .filter(|(index, _)| escolhido(*index))
            .filter_map(|(_, candidate)| candidate.source.clone())
            .collect();
        if texto.is_empty() {
            return GenerateOutcome::Message("Nenhum campo marcado".to_owned());
        }
        let quantos = state
            .candidates
            .iter()
            .enumerate()
            .filter(|(index, candidate)| candidate.source.is_some() && escolhido(*index))
            .count();
        let nome = match state.kind {
            AccessorKind::Getter => "getter",
            AccessorKind::Setter => "setter",
            // O construtor não passa por aqui: ele tem caminho próprio.
            AccessorKind::Both | AccessorKind::Constructor => "acessor",
        };
        GenerateOutcome::Insert {
            text: texto,
            line: state.insert_at.line,
            message: format!("{quantos} {nome}(s) gerado(s)"),
        }
    }

    pub(super) fn pointer_down(
        &mut self,
        context: &LayoutContext,
        point: Point,
        size: Size,
    ) -> GenerateOutcome {
        let (lista, todos, ok) = self.geometry(context, size);
        if todos.contains(point) {
            return self.confirm(true);
        }
        if ok.contains(point) {
            return self.confirm(false);
        }
        if lista.contains(point) {
            // A lista trata a trilha e diz qual linha foi clicada; a marcação
            // é da tela, que é quem sabe o que está marcado.
            let Some(state) = self.state.as_mut() else {
                return GenerateOutcome::Idle;
            };
            state.list.layout(context, lista);
            let antes = state.list.selected();
            state.list.event(
                &mut EventContext::default(),
                &UiEvent::PointerDown(primary_pointer(point)),
            );
            let agora = state.list.selected();
            // Com barra, um clique que não mudou a linha foi na trilha: marcar
            // ali viraria rolagem e marcação no mesmo gesto.
            if (agora != antes || !state.list.scrolls())
                && let Some(linha) = agora
                && let Some(marcado) = state.checked.get_mut(linha)
            {
                *marcado = !*marcado;
                state.list = build_list(&state.candidates, &state.checked);
            }
            return GenerateOutcome::Idle;
        }
        // Clique fora do painel dispensa a janela.
        if !self.panel_bounds().contains(point) {
            self.close();
        }
        GenerateOutcome::Idle
    }

    fn panel_bounds(&mut self) -> Rect {
        self.modal().panel_bounds()
    }

    /// Roda e arrasto: são da lista, que tem a barra.
    pub(super) fn pointer_event(&mut self, context: &LayoutContext, event: &UiEvent, size: Size) {
        let (lista, ..) = self.geometry(context, size);
        if let Some(state) = self.state.as_mut() {
            state.list.layout(context, lista);
            state.list.event(&mut EventContext::default(), event);
        }
    }

    pub(super) fn scroll(&mut self, context: &LayoutContext, point: Point, lines: f32, size: Size) {
        self.pointer_event(
            context,
            &UiEvent::Scroll(ScrollEvent {
                position: point,
                delta_x: 0.0,
                delta_y: lines * ROW_HEIGHT,
            }),
            size,
        );
    }

    pub(super) fn paint(
        &mut self,
        layout: &LayoutContext,
        paint: &mut PaintContext,
        size: Size,
    ) -> bool {
        if !self.is_open() || self.state.is_none() {
            return false;
        }
        let (lista, todos_rect, ok_rect) = self.geometry(layout, size);
        if let Some(modal) = self.modal.as_ref() {
            let mut copia = modal.clone();
            copia.layout(layout, Rect::new(0.0, 0.0, size.width, size.height));
            copia.paint(paint);
        }
        if let Some(state) = self.state.as_mut() {
            state.list.layout(layout, lista);
            state.list.paint(paint);
        }
        let mut todos = Button::new(ALL_ID, "All");
        todos.layout(layout, todos_rect);
        todos.paint(paint);
        let mut ok = Button::new(OK_ID, "OK");
        ok.layout(layout, ok_rect);
        ok.paint(paint);
        true
    }

    /// Quanto a lista já rolou, e quantos campos estão marcados.
    ///
    /// Só os testes precisam: são o que se observa para dizer que a roda moveu
    /// a lista e que o clique marcou a linha.
    #[cfg(test)]
    pub(super) fn list_scroll(&self) -> f32 {
        self.state
            .as_ref()
            .map_or(0.0, |state| state.list.scroll_offset())
    }

    #[cfg(test)]
    pub(super) fn checked_count(&self) -> usize {
        self.state.as_ref().map_or(0, |state| {
            state.checked.iter().filter(|item| **item).count()
        })
    }

    /// Áreas da janela, para os testes apontarem um gesto dentro dela.
    #[cfg(test)]
    pub(super) fn areas(&mut self, context: &LayoutContext, size: Size) -> (Rect, Rect, Rect) {
        self.geometry(context, size)
    }

    /// Áreas da janela: a lista e os dois botões.
    fn geometry(&mut self, context: &LayoutContext, size: Size) -> (Rect, Rect, Rect) {
        let panel = {
            let modal = self.modal();
            modal.layout(context, Rect::new(0.0, 0.0, size.width, size.height));
            modal.panel_bounds()
        };
        let botoes_y = panel.origin.y + panel.size.height - 52.0;
        let lista = Rect::new(
            panel.origin.x + 16.0,
            panel.origin.y + 56.0,
            panel.size.width - 32.0,
            botoes_y - (panel.origin.y + 56.0) - 12.0,
        );
        let ok = Rect::new(
            panel.origin.x + panel.size.width - 116.0,
            botoes_y,
            100.0,
            36.0,
        );
        let todos = Rect::new(ok.origin.x - 112.0, botoes_y, 100.0, 36.0);
        (lista, todos, ok)
    }
}

/// Monta a lista: uma caixa de marcação e o nome do campo por linha.
///
/// Quem decide o que entra em cada linha é o consumidor da biblioteca — a lista
/// composta só posiciona o que recebe.
fn build_list(candidates: &[AccessorCandidate], checked: &[bool]) -> ComposedList {
    let rows = candidates
        .iter()
        .enumerate()
        .map(|(index, candidate)| {
            let marcado = checked.get(index).copied().unwrap_or_default();
            ComposedRow::new(vec![
                ComposedCell::new(
                    Box::new(Checkbox::new(
                        WidgetId(ROW_ID.0 + index as u64 * 2),
                        "",
                        marcado,
                    )),
                    CellWidth::Fixed(28.0),
                ),
                ComposedCell::new(
                    Box::new(Label::new(
                        WidgetId(ROW_ID.0 + index as u64 * 2 + 1),
                        candidate.field.clone(),
                    )),
                    CellWidth::Fill,
                ),
            ])
        })
        .collect();
    ComposedList::new(LIST_ID, rows).with_row_height(ROW_HEIGHT)
}
