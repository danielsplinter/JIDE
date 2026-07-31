//! A janela de renomear: campo com o nome, lista dos arquivos afetados.
//!
//! Ela decide **o quê** renomear e responde eventos; quem escreve no editor e
//! quem fala com a aplicação continua sendo o shell. A fronteira é o
//! [`RenameOutcome`]: a janela não alcança sessão, comandos nem barra de estado,
//! e por isso não há como ela mexer no documento por um caminho lateral.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use ide_domain::{Location, TextRange};
// A moldura, o campo e a lista são da biblioteca: a IDE não desenha caixa de
// texto nem trilha de rolagem à mão.
use ui_api::{EventContext, LayoutContext, PaintContext, Widget};
use ui_commands::CommandEvent;
use ui_components::{Button, FormLayout, IconTint, Label, ListView, ModalHost, TextInput};
use ui_core::{
    KeyEvent, Modifiers, Point, Rect, Size, TextInputEvent, UiEvent, WidgetAction, WidgetId,
};
use ui_host::UiHost;

use super::primary_pointer;

const MODAL_ID: WidgetId = WidgetId(10_400);
const INPUT_ID: WidgetId = WidgetId(10_401);
const LIST_ID: WidgetId = WidgetId(10_402);
const OK_ID: WidgetId = WidgetId(10_403);
const CANCEL_ID: WidgetId = WidgetId(10_404);
const NAME_CAPTION_ID: WidgetId = WidgetId(10_405);
const LIST_CAPTION_ID: WidgetId = WidgetId(10_406);
/// A janela é larga porque a lista mostra caminho e contagem em cada linha, e
/// estreitá-la só empurraria o trabalho para a barra lateral.
const PANEL_SIZE: Size = Size::new(720.0, 460.0);

/// O que a janela conclui, para o shell executar.
///
/// Ela não renomeia nada: entrega a decisão. É o que a mantém sem acesso à
/// sessão de edição e à fila de comandos da aplicação.
pub(super) enum RenameOutcome {
    /// O gesto não concluiu nada.
    Idle,
    /// Nada a fazer, com o que dizer na barra de estado.
    Message(String),
    /// Renomear, com tudo o que precisa ser trocado junto.
    Apply(RenameDecision),
}

pub(super) struct RenameDecision {
    pub(super) from: PathBuf,
    pub(super) to: PathBuf,
    pub(super) old_name: String,
    pub(super) new_name: String,
    /// Onde o nome antigo aparece, por arquivo, do fim para o começo do texto.
    pub(super) occurrences: Vec<(PathBuf, Vec<TextRange>)>,
}

/// Renomeação em curso: o nome novo e o alcance da mudança.
struct RenameState {
    path: PathBuf,
    old_name: String,
    occurrences: Vec<(PathBuf, Vec<TextRange>)>,
    input: TextInput,
    list: ListView,
}

pub(super) struct RenameSurface {
    modal: ModalHost,
    state: Option<RenameState>,
    /// Arquivo cuja renomeação foi pedida e ainda não foi respondida.
    pending: Option<PathBuf>,
}

impl Default for RenameSurface {
    fn default() -> Self {
        Self {
            modal: ModalHost::new(MODAL_ID, "Renomear", PANEL_SIZE),
            state: None,
            pending: None,
        }
    }
}

/// Entrega os dois botões desta janela ao anfitrião da tela.
pub(super) fn attach(host: &mut UiHost) {
    host.attach(
        Box::new(Button::new(CANCEL_ID, "Cancelar").with_command("rename.cancel")),
        false,
    );
    host.attach(
        Box::new(Button::new(OK_ID, "OK").with_command("rename.confirm")),
        false,
    );
}

impl RenameSurface {
    pub(super) fn request(&mut self, path: PathBuf) {
        self.pending = Some(path);
    }

    pub(super) fn take_request(&mut self) -> Option<PathBuf> {
        self.pending.take()
    }

    #[must_use]
    pub(super) fn is_open(&self) -> bool {
        self.modal.is_open()
    }

    /// Abre com o arquivo e o que será reescrito junto.
    ///
    /// A lista mostra **os arquivos afetados**, com quantas ocorrências cada um
    /// tem: é o alcance da mudança, e é o que o usuário confirma ao clicar OK.
    pub(super) fn show(&mut self, path: PathBuf, references: Vec<Location>) {
        let old_name = path
            .file_stem()
            .and_then(|valor| valor.to_str())
            .unwrap_or_default()
            .to_owned();
        let mut por_arquivo: BTreeMap<PathBuf, Vec<TextRange>> = BTreeMap::new();
        for location in references {
            por_arquivo
                .entry(location.path)
                .or_default()
                .push(location.range);
        }
        let occurrences: Vec<(PathBuf, Vec<TextRange>)> = por_arquivo
            .into_iter()
            .map(|(caminho, mut ranges)| {
                // Do fim para o começo: trocar no começo moveria as seguintes.
                ranges.sort_by(|esquerda, direita| {
                    (direita.start.line, direita.start.column)
                        .cmp(&(esquerda.start.line, esquerda.start.column))
                });
                ranges.dedup();
                (caminho, ranges)
            })
            .collect();
        let rotulos: Vec<String> = occurrences
            .iter()
            .map(|(caminho, ranges)| reference_label(caminho, ranges.len()))
            .collect();
        let mut input = TextInput::new(INPUT_ID, old_name.clone());
        input.event(&mut EventContext::default(), &UiEvent::FocusGained);
        self.state = Some(RenameState {
            input,
            list: ListView::new(LIST_ID, rotulos),
            path,
            old_name,
            occurrences,
        });
        self.modal.open();
    }

    /// Nome que está no campo, que é o que a confirmação aplica.
    #[must_use]
    pub(super) fn name(&self) -> String {
        self.state
            .as_ref()
            .map(|state| state.input.value().to_owned())
            .unwrap_or_default()
    }

    /// Arquivos afetados, como aparecem na lista.
    #[must_use]
    pub(super) fn references(&self) -> Vec<String> {
        self.state
            .as_ref()
            .map(|state| {
                state
                    .occurrences
                    .iter()
                    .map(|(caminho, ranges)| reference_label(caminho, ranges.len()))
                    .collect()
            })
            .unwrap_or_default()
    }

    pub(super) fn cancel(&mut self) {
        self.modal.close();
        self.state = None;
    }

    /// Confirma e devolve a decisão para o shell executar.
    pub(super) fn confirm(&mut self) -> RenameOutcome {
        let Some(state) = self.state.take() else {
            return RenameOutcome::Idle;
        };
        self.modal.close();
        let novo = state.input.value().trim().to_owned();
        if novo.is_empty() || novo == state.old_name {
            return RenameOutcome::Message("Nome inalterado".to_owned());
        }
        let extensao = state
            .path
            .extension()
            .and_then(|valor| valor.to_str())
            .map(|valor| format!(".{valor}"))
            .unwrap_or_default();
        let destino = state.path.with_file_name(format!("{novo}{extensao}"));
        RenameOutcome::Apply(RenameDecision {
            from: state.path,
            to: destino,
            old_name: state.old_name,
            new_name: novo,
            occurrences: state.occurrences,
        })
    }

    /// Roteia o clique. Devolve o que o clique concluiu, se concluiu algo.
    pub(super) fn pointer_down(
        &mut self,
        host: &mut UiHost,
        context: &LayoutContext,
        point: Point,
        size: Size,
    ) -> RenameOutcome {
        let geometry = self.geometry(context, size);
        let outcome = host.click(point);
        for evento in outcome.commands {
            if let CommandEvent::Action(WidgetAction::Command(command)) = evento {
                match command.0.as_str() {
                    "rename.confirm" => return self.confirm(),
                    "rename.cancel" => {
                        self.cancel();
                        return RenameOutcome::Idle;
                    }
                    _ => {}
                }
            }
        }
        let Some(state) = self.state.as_mut() else {
            return RenameOutcome::Idle;
        };
        // Campo e lista guardam o que a janela lê — cursor e rolagem —, então a
        // entrega fica aqui; o acerto é do anfitrião.
        match outcome.target {
            // O campo sabe onde o cursor cai dentro do texto: a medição é dele.
            Some(INPUT_ID) => {
                state.input.layout(context, geometry.input);
                state.input.event(
                    &mut EventContext::default(),
                    &UiEvent::PointerDown(primary_pointer(point)),
                );
            }
            Some(LIST_ID) => {
                state.list.layout(context, geometry.list);
                state.list.event(
                    &mut EventContext::default(),
                    &UiEvent::PointerDown(primary_pointer(point)),
                );
            }
            _ => {}
        }
        RenameOutcome::Idle
    }

    /// Declara ao anfitrião da tela a área de cada peça da janela.
    pub(super) fn place_widgets(&mut self, host: &mut UiHost, context: &LayoutContext, size: Size) {
        let geometry = self.geometry(context, size);
        host.place(MODAL_ID, self.modal.panel_bounds());
        host.place(INPUT_ID, geometry.input);
        host.place(LIST_ID, geometry.list);
        host.place(CANCEL_ID, geometry.cancel);
        host.place(OK_ID, geometry.ok);
    }

    /// Movimento e soltura do ponteiro: são da lista, que tem as barras.
    ///
    /// Sem eles o indicador é agarrado e nunca anda — foi assim que o arrasto
    /// deixou de funcionar quando a janela nasceu.
    pub(super) fn pointer_event(&mut self, context: &LayoutContext, event: &UiEvent, size: Size) {
        let geometry = self.geometry(context, size);
        if let Some(state) = self.state.as_mut() {
            state.list.layout(context, geometry.list);
            state.list.event(&mut EventContext::default(), event);
        }
    }

    /// Teclas: `Enter` confirma, `Esc` desiste, o resto vai para o campo.
    pub(super) fn key(&mut self, key: &str, modifiers: Modifiers) -> RenameOutcome {
        match key.to_ascii_lowercase().as_str() {
            "enter" => self.confirm(),
            "escape" => {
                self.cancel();
                RenameOutcome::Idle
            }
            outra => {
                if let Some(state) = self.state.as_mut() {
                    state.input.event(
                        &mut EventContext::default(),
                        &UiEvent::KeyDown(KeyEvent {
                            logical_key: outra.to_owned(),
                            repeat: false,
                            modifiers,
                        }),
                    );
                }
                RenameOutcome::Idle
            }
        }
    }

    pub(super) fn text_input(&mut self, text: &str) {
        if let Some(state) = self.state.as_mut() {
            state.input.event(
                &mut EventContext::default(),
                &UiEvent::TextInput(TextInputEvent {
                    text: text.to_owned(),
                }),
            );
        }
    }

    pub(super) fn paint(
        &mut self,
        host: &UiHost,
        layout: &LayoutContext,
        paint: &mut PaintContext,
        size: Size,
    ) -> bool {
        if !self.modal.is_open() || self.state.is_none() {
            return false;
        }
        let geometry = self.geometry(layout, size);
        let mut modal = self.modal.clone();
        modal.layout(layout, Rect::new(0.0, 0.0, size.width, size.height));
        modal.paint(paint);
        let legenda = self
            .state
            .as_ref()
            .map(|state| match state.occurrences.len() {
                0 => "Nada mais no projeto usa este nome".to_owned(),
                1 => "1 arquivo será reescrito".to_owned(),
                quantos => format!("{quantos} arquivos serão reescritos"),
            })
            .unwrap_or_default();
        caption(
            layout,
            paint,
            NAME_CAPTION_ID,
            "Novo nome",
            Point::new(geometry.input.origin.x, geometry.input.origin.y - 18.0),
        );
        caption(
            layout,
            paint,
            LIST_CAPTION_ID,
            &legenda,
            Point::new(geometry.list.origin.x, geometry.list.origin.y - 18.0),
        );
        if let Some(state) = self.state.as_mut() {
            state.input.layout(layout, geometry.input);
            state.input.paint(paint);
            state.list.layout(layout, geometry.list);
            state.list.paint(paint);
        }
        // Os botões são do anfitrião da tela, e por isso respondem ao ponteiro.
        for id in [CANCEL_ID, OK_ID] {
            if let Some(button) = host.widget(id) {
                button.paint(paint);
            }
        }
        true
    }

    /// Área da lista, para quem precisa apontar um gesto dentro dela.
    #[cfg(test)]
    pub(super) fn list_area(&mut self, context: &LayoutContext, size: Size) -> Rect {
        self.geometry(context, size).list
    }

    /// Quanto a lista já rolou, para verificar o arrasto.
    #[cfg(test)]
    pub(super) fn list_scroll(&self) -> f32 {
        self.state
            .as_ref()
            .map(|state| state.list.scroll_offset())
            .unwrap_or_default()
    }

    /// Áreas da janela, com o painel já posicionado.
    fn geometry(&mut self, context: &LayoutContext, size: Size) -> RenameGeometry {
        self.modal
            .layout(context, Rect::new(0.0, 0.0, size.width, size.height));
        geometry(self.modal.panel_bounds())
    }
}

/// Rótulo comum das legendas da janela.
fn caption(
    layout: &LayoutContext,
    paint: &mut PaintContext,
    id: WidgetId,
    text: &str,
    origin: Point,
) {
    let mut label = Label::new(id, text)
        .with_font_size(13.0)
        .with_tone(IconTint::Muted);
    label.layout(layout, Rect::new(origin.x, origin.y, 0.0, 0.0));
    label.paint(paint);
}

/// Como um arquivo afetado aparece na lista.
///
/// O nome do arquivo, a pasta — dois pacotes podem ter arquivos homônimos — e
/// quantas ocorrências serão trocadas ali, que é o que dá a dimensão da mudança
/// antes de confirmá-la.
fn reference_label(path: &Path, ocorrencias: usize) -> String {
    let nome = path
        .file_name()
        .and_then(|valor| valor.to_str())
        .unwrap_or_default();
    let pasta = path
        .parent()
        .and_then(|pai| pai.to_str())
        .filter(|pai| !pai.is_empty());
    match pasta {
        Some(pasta) => format!("{nome}  ({pasta})  —  {ocorrencias}"),
        None => format!("{nome}  —  {ocorrencias}"),
    }
}

/// Áreas da janela de renomear: campo, lista e os dois botões.
struct RenameGeometry {
    input: Rect,
    list: Rect,
    cancel: Rect,
    ok: Rect,
}

fn geometry(panel: Rect) -> RenameGeometry {
    let forma = FormLayout::new(panel);
    let input = forma.field(0);
    let [cancel, ok] = forma.actions();
    // A lista ocupa o que sobra entre o campo e a fileira de ações.
    let topo_lista = input.origin.y + input.size.height + 34.0;
    let list = Rect::new(
        input.origin.x,
        topo_lista,
        input.size.width,
        (ok.origin.y - topo_lista - 16.0).max(40.0),
    );
    RenameGeometry {
        input,
        list,
        cancel,
        ok,
    }
}
