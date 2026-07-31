//! A janela de criar item: o pacote, o nome e os dois botões.
//!
//! A mesma janela serve às três ações do menu — pacote, classe, interface —,
//! mudando só o título, a legenda do nome e se o nome pode ficar vazio; tudo
//! isso vem no `NewItemTemplate` que a linguagem contribui. Como as outras
//! janelas desta fase, ela decide e o shell executa: daqui sai um
//! `NewItemRequest`, não um comando na fila da aplicação.

use std::path::{Path, PathBuf};

use ide_application::{NewItemRequest, NewItemTemplate};
use ui_api::{EventContext, LayoutContext, PaintContext, Widget};
use ui_commands::CommandEvent;
use ui_components::{Button, FormLayout, IconTint, Label, ModalHost, TextInput};
use ui_core::{
    KeyEvent, Modifiers, Point, Rect, Size, TextInputEvent, UiEvent, WidgetAction, WidgetId,
};
use ui_focus::{FocusChange, FocusManager};
use ui_host::UiHost;

use super::primary_pointer;
use crate::explorer::is_source_root;

const MODAL_ID: WidgetId = WidgetId(10_035);
const PACKAGE_ID: WidgetId = WidgetId(10_036);
const NAME_ID: WidgetId = WidgetId(10_037);
const CREATE_ID: WidgetId = WidgetId(10_038);
const CANCEL_ID: WidgetId = WidgetId(10_039);
const PACKAGE_CAPTION_ID: WidgetId = WidgetId(10_041);
const NAME_CAPTION_ID: WidgetId = WidgetId(10_042);
const MESSAGE_ID: WidgetId = WidgetId(10_043);
pub(super) const PANEL_SIZE: Size = Size::new(460.0, 230.0);

/// O que a janela concluiu, para o shell executar.
pub(super) enum NewItemOutcome {
    /// O gesto não concluiu nada — o que ele mudou já está na janela.
    Idle,
    /// Criar o item, com o que foi preenchido.
    Create(NewItemRequest),
}

/// O modelo escolhido e onde ele será criado.
struct NewItemDialog {
    template: NewItemTemplate,
    source_root: PathBuf,
    message: Option<String>,
}

pub(super) struct NewItemSurface {
    modal: ModalHost,
    dialog: Option<NewItemDialog>,
    package: TextInput,
    name: TextInput,
    /// Qual dos dois campos recebe o que for digitado. Quem faz a conta é o
    /// gerenciador da biblioteca: `Tab` percorre, e o clique transfere.
    focus: FocusManager,
}

impl Default for NewItemSurface {
    fn default() -> Self {
        Self {
            modal: ModalHost::new(MODAL_ID, "", PANEL_SIZE),
            dialog: None,
            package: TextInput::new(PACKAGE_ID, String::new()).with_placeholder("br.com.exemplo"),
            name: TextInput::new(NAME_ID, String::new()),
            focus: new_focus(),
        }
    }
}

/// Entrega os botões desta janela ao anfitrião da tela.
///
/// Eles são dele — só emitem, nada é lido deles. Os campos ficam com a janela,
/// que lê o que foi digitado, mas suas áreas são declaradas no mesmo anfitrião:
/// o acerto é dele mesmo quando a instância não é.
pub(super) fn attach(host: &mut UiHost) {
    host.attach(
        Box::new(Button::new(CREATE_ID, "Criar").with_command("new.create")),
        false,
    );
    host.attach(
        Box::new(Button::new(CANCEL_ID, "Cancelar").with_command("new.cancel")),
        false,
    );
}

/// O percurso do `Tab` desta janela: pacote e depois nome.
fn new_focus() -> FocusManager {
    let mut focus = FocusManager::default();
    focus.register(PACKAGE_ID);
    focus.register(NAME_ID);
    focus
}

impl NewItemSurface {
    #[must_use]
    pub(super) const fn is_open(&self) -> bool {
        self.modal.is_open()
    }

    /// Abre a janela com o pacote do alvo já preenchido.
    ///
    /// O pacote vem do caminho clicado, em notação de ponto: é o que o usuário vê
    /// no Explorer e o que ele vai editar para criar um pacote abaixo. Sem raiz de
    /// fontes não há pacote, e a janela não abre — o que devolve é o motivo, para
    /// o shell contar na barra de status.
    pub(super) fn open(
        &mut self,
        template: NewItemTemplate,
        target: &Path,
        source_root_names: &[String],
    ) -> Option<String> {
        let Some(source_root) = target
            .ancestors()
            .find(|ancestor| is_source_root(ancestor, source_root_names))
            .map(Path::to_path_buf)
        else {
            return Some("Fora de uma raiz de fontes registrada".to_owned());
        };
        let package = target
            .strip_prefix(&source_root)
            .map(|relative| {
                relative
                    .components()
                    .filter_map(|component| component.as_os_str().to_str())
                    .collect::<Vec<_>>()
                    .join(".")
            })
            .unwrap_or_default();
        self.package.set_value(package);
        self.name.set_value(String::new());
        self.modal.set_title(template.title.clone());
        self.modal.open();
        let allows_empty_name = template.allows_empty_name;
        self.dialog = Some(NewItemDialog {
            template,
            source_root,
            message: None,
        });
        // O pacote já vem preenchido, então o que falta digitar é o nome —
        // exceto ao criar pacote, em que o nome é justamente o que se edita.
        self.focus_field(!allows_empty_name);
        None
    }

    /// Relata o que impediu a criação, mantendo a janela aberta.
    pub(super) fn set_message(&mut self, message: impl Into<String>) {
        if let Some(dialog) = self.dialog.as_mut() {
            dialog.message = Some(message.into());
        }
    }

    pub(super) fn close(&mut self) {
        self.modal.close();
        self.dialog = None;
    }

    /// Monta o pedido a partir do que está nos campos.
    ///
    /// O pacote é obrigatório: sem ele não há onde criar. O nome é obrigatório
    /// para classe e interface, e opcional para pacote — é o que permite criar o
    /// pacote e a primeira classe dele num gesto só.
    pub(super) fn submit(&mut self) -> NewItemOutcome {
        let Some(dialog) = self.dialog.as_ref() else {
            return NewItemOutcome::Idle;
        };
        let template_id = dialog.template.id.clone();
        let source_root = dialog.source_root.clone();
        let allows_empty_name = dialog.template.allows_empty_name;
        let package = self.package.value().trim().to_owned();
        let name = self.name.value().trim().to_owned();
        if package.is_empty() {
            self.set_message("Informe o pacote.");
            return NewItemOutcome::Idle;
        }
        if name.is_empty() && !allows_empty_name {
            self.set_message("Informe o nome.");
            return NewItemOutcome::Idle;
        }
        NewItemOutcome::Create(NewItemRequest {
            template_id,
            package,
            name,
            source_root,
        })
    }

    /// Roteia o clique dentro da janela.
    ///
    /// Quem descobre o alvo é o anfitrião, pelas áreas que ele recebeu. Os
    /// botões ele mesmo aciona, e devolve o comando; os campos são entregues
    /// aqui, porque é esta janela que lê o que foi digitado.
    pub(super) fn pointer_down(&mut self, host: &mut UiHost, point: Point) -> NewItemOutcome {
        let outcome = host.click(point);
        for evento in outcome.commands {
            if let CommandEvent::Action(WidgetAction::Command(command)) = evento {
                match command.0.as_str() {
                    "new.create" => return self.submit(),
                    "new.cancel" => self.close(),
                    _ => {}
                }
            }
        }
        // O clique vai ao campo: onde o cursor fica dentro do texto é ele quem
        // sabe, porque a medição da fonte é dele.
        let naming = match outcome.target {
            Some(NAME_ID) => true,
            Some(PACKAGE_ID) => false,
            _ => return NewItemOutcome::Idle,
        };
        let field = if naming {
            &mut self.name
        } else {
            &mut self.package
        };
        field.event(
            &mut EventContext::default(),
            &UiEvent::PointerDown(primary_pointer(point)),
        );
        self.focus_field(naming);
        NewItemOutcome::Idle
    }

    /// Declara ao anfitrião da tela a área de cada peça da janela.
    ///
    /// O arranjo continua sendo desta tela — o `FormLayout` sabe onde cada campo
    /// e cada botão ficam. O que o anfitrião recebe é o resultado, e com ele o
    /// acerto e a entrega deixam de ser escritos à mão.
    ///
    /// Só enquanto a janela está aberta: é a presença destes nós na pilha que
    /// decide a sobreposição, e fechá-la é tirá-los de lá.
    pub(super) fn place_widgets(&mut self, host: &mut UiHost, context: &LayoutContext, size: Size) {
        let geometry = self.geometry(context, size);
        host.place(PACKAGE_ID, geometry.package);
        host.place(NAME_ID, geometry.name);
        host.place(CANCEL_ID, geometry.cancel);
        host.place(CREATE_ID, geometry.create);
        self.package.layout(context, geometry.package);
        self.name.layout(context, geometry.name);
    }

    /// Tecla dentro da janela.
    pub(super) fn key(&mut self, key: &str) -> NewItemOutcome {
        if self.dialog.is_none() {
            return NewItemOutcome::Idle;
        }
        match key.to_ascii_lowercase().as_str() {
            "enter" => return self.submit(),
            "escape" => self.close(),
            "tab" => {
                let change = self.focus.focus_next(false);
                self.deliver_focus(change);
            }
            // Apagar e mover o cursor são do campo: ele conhece as fronteiras de
            // caractere e a posição atual.
            _ => {
                let event = UiEvent::KeyDown(KeyEvent {
                    logical_key: key.to_owned(),
                    repeat: false,
                    modifiers: Modifiers::default(),
                });
                if let Some(field) = self.focused_field() {
                    field.event(&mut EventContext::default(), &event);
                }
            }
        }
        NewItemOutcome::Idle
    }

    /// Texto digitado na janela.
    ///
    /// O texto entra pelo componente, e não por concatenação: é assim que ele
    /// aparece onde o cursor está, inclusive depois de um clique no meio do
    /// caminho já digitado.
    pub(super) fn text_input(&mut self, text: &str) {
        let event = UiEvent::TextInput(TextInputEvent {
            text: text.to_owned(),
        });
        if let Some(field) = self.focused_field() {
            field.event(&mut EventContext::default(), &event);
        }
        if let Some(dialog) = self.dialog.as_mut() {
            // Digitar é corrigir: a mensagem do erro anterior sai de cena.
            dialog.message = None;
        }
    }

    /// Desenha a janela. Devolve `false` quando não há nada aberto.
    pub(super) fn paint(
        &self,
        host: &UiHost,
        layout: &LayoutContext,
        paint: &mut PaintContext,
        size: Size,
    ) -> bool {
        let Some(dialog) = self.dialog.as_ref() else {
            return false;
        };
        let mut modal = self.modal.clone();
        // O painel se centraliza na área que recebe no layout. Sem esse layout a
        // área é zero, e a janela nasce no canto superior esquerdo.
        modal.layout(layout, Rect::new(0.0, 0.0, size.width, size.height));
        let geometry = geometry(modal.panel_bounds());
        // O título é do `ModalHost`, que já o desenha: escrever outro por cima
        // era o que aparecia duplicado.
        modal.paint(paint);
        let forma = FormLayout::new(modal.panel_bounds());
        caption(
            layout,
            paint,
            PACKAGE_CAPTION_ID,
            "Pacote",
            forma.caption(0),
            IconTint::Muted,
        );
        caption(
            layout,
            paint,
            NAME_CAPTION_ID,
            &dialog.template.name_caption,
            forma.caption(1),
            IconTint::Muted,
        );
        for (field, rect) in [
            (&self.package, geometry.package),
            (&self.name, geometry.name),
        ] {
            // O foco já está no campo de verdade; aqui é só desenhar.
            let mut field = field.clone();
            field.layout(layout, rect);
            field.paint(paint);
        }
        // Os botões são desenhados pelo anfitrião da tela, que é quem os possui —
        // e por isso eles acendem sob o ponteiro e afundam ao ser pressionados.
        for id in [CANCEL_ID, CREATE_ID] {
            if let Some(button) = host.widget(id) {
                button.paint(paint);
            }
        }
        if let Some(message) = dialog.message.as_ref() {
            caption(
                layout,
                paint,
                MESSAGE_ID,
                message,
                Point::new(geometry.name.origin.x, geometry.name.origin.y + 44.0),
                IconTint::Danger,
            );
        }
        true
    }

    /// Áreas da janela, já centralizada numa tela deste tamanho.
    pub(super) fn geometry(&mut self, context: &LayoutContext, size: Size) -> NewItemGeometry {
        self.modal
            .layout(context, Rect::new(0.0, 0.0, size.width, size.height));
        geometry(self.modal.panel_bounds())
    }

    /// Move o foco entre os dois campos.
    ///
    /// O foco fica nos campos de verdade, e não num clone da pintura: é ele que
    /// decide onde a digitação entra e onde o cursor aparece. O par ganhar/perder
    /// é do grupo, que vê os dois campos de uma vez.
    fn focus_field(&mut self, naming: bool) {
        let field = if naming { NAME_ID } else { PACKAGE_ID };
        if let Some(change) = self.focus.request_focus(field) {
            self.deliver_focus(change);
        }
    }

    /// Entrega o par ganhar/perder aos campos.
    ///
    /// Temporário: entregar evento é trabalho de quem possui os componentes, e
    /// esse dono é o anfitrião que a biblioteca ainda não tem. Sai daqui na fase
    /// 4 de `15-event-runtime-adoption`.
    fn deliver_focus(&mut self, change: FocusChange) {
        for (id, event) in [
            (change.previous, UiEvent::FocusLost),
            (change.current, UiEvent::FocusGained),
        ] {
            let Some(id) = id else {
                continue;
            };
            let field = if id == NAME_ID {
                &mut self.name
            } else {
                &mut self.package
            };
            field.event(&mut EventContext::default(), &event);
        }
    }

    /// O campo que está recebendo o que for digitado.
    fn focused_field(&mut self) -> Option<&mut TextInput> {
        match self.focus.focused()? {
            NAME_ID => Some(&mut self.name),
            _ => Some(&mut self.package),
        }
    }

    /// O que está nos dois campos, para os testes.
    #[cfg(test)]
    pub(super) fn values(&self) -> (&str, &str) {
        (self.package.value(), self.name.value())
    }

    /// O título do modelo aberto, para os testes.
    #[cfg(test)]
    pub(super) fn title(&self) -> Option<&str> {
        self.dialog
            .as_ref()
            .map(|dialog| dialog.template.title.as_str())
    }

    /// O que impediu a criação, para os testes.
    #[cfg(test)]
    pub(super) fn message(&self) -> Option<String> {
        self.dialog
            .as_ref()
            .and_then(|dialog| dialog.message.clone())
    }
}

/// Uma legenda solta, do tamanho e do tom das outras janelas.
fn caption(
    layout: &LayoutContext,
    paint: &mut PaintContext,
    id: WidgetId,
    text: &str,
    origin: Point,
    tone: IconTint,
) {
    let mut label = Label::new(id, text).with_font_size(13.0).with_tone(tone);
    label.layout(layout, Rect::new(origin.x, origin.y, 0.0, 0.0));
    label.paint(paint);
}

/// Onde cada peça da janela de criação fica dentro do painel.
pub(super) struct NewItemGeometry {
    pub(super) package: Rect,
    pub(super) name: Rect,
    pub(super) create: Rect,
    pub(super) cancel: Rect,
}

fn geometry(panel: Rect) -> NewItemGeometry {
    let forma = FormLayout::new(panel);
    // Criar à direita, encostado na borda: é o canto que a leitura alcança por
    // último, e vale para todas as janelas.
    let [cancel, create] = forma.actions();
    NewItemGeometry {
        package: forma.field(0),
        name: forma.field(1),
        create,
        cancel,
    }
}
