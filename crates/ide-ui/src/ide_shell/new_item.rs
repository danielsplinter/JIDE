//! A janela de criar item: o pacote, o nome e os dois botões.
//!
//! A mesma janela serve às três ações do menu — pacote, classe, interface —,
//! mudando só o título, a legenda do nome e se o nome pode ficar vazio; tudo
//! isso vem no `NewItemTemplate` que a linguagem contribui. Como as outras
//! janelas desta fase, ela decide e o shell executa: daqui sai um
//! `NewItemRequest`, não um comando na fila da aplicação.

use std::path::{Path, PathBuf};

use ide_application::{NewItemRequest, NewItemTemplate};

use super::{JANELA_TITULO_ALTO};
use ui_api::{LayoutContext, PaintContext, Widget};
use ui_commands::CommandEvent;
use ui_components::{Button, IconTint, Label, ModalHost, TextInput};
use ui_core::{
    KeyEvent, Modifiers, Point, Rect, Size, Spacing, TextInputEvent, UiEvent, WidgetAction,
    WidgetId,
};
use ui_host::{Node, UiHost};
use ui_layout_api::{EdgeInsets, LayoutDirection, LayoutStyle, MainAlign};

use crate::explorer::is_source_root;

const MODAL_ID: WidgetId = WidgetId(10_035);
const PACKAGE_ID: WidgetId = WidgetId(10_036);
const NAME_ID: WidgetId = WidgetId(10_037);
const CREATE_ID: WidgetId = WidgetId(10_038);
const CANCEL_ID: WidgetId = WidgetId(10_039);
const PACKAGE_CAPTION_ID: WidgetId = WidgetId(10_041);
const NAME_CAPTION_ID: WidgetId = WidgetId(10_042);
const MESSAGE_ID: WidgetId = WidgetId(10_043);
/// A fileira de ações, e as duas folgas que separam as peças da coluna.
const ACTIONS_ID: WidgetId = WidgetId(10_430);
const FIELD_GAP_ID: WidgetId = WidgetId(10_431);
const FILL_ID: WidgetId = WidgetId(10_432);
/// Onde a legenda fica em relação ao campo que ela nomeia.
const CAPTION_OFFSET: f32 = 18.0;
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
}

impl Default for NewItemSurface {
    fn default() -> Self {
        Self {
            modal: ModalHost::new(MODAL_ID, "", PANEL_SIZE),
            dialog: None,
        }
    }
}

/// Os componentes desta janela pertencem ao anfitrião da tela.
///
/// Inclusive os dois campos: quem entrega `FocusGained` e `FocusLost` é quem tem
/// o mapa de id para componente, e esse é o anfitrião. A janela lê de volta o que
/// foi digitado por `widget_as`.
pub(super) fn attach(host: &mut UiHost, layer: WidgetId) {
    let campo = || LayoutStyle {
        height: Some(34.0),
        ..LayoutStyle::default()
    };
    let _ = host.declare(
        layer,
        MODAL_ID,
        LayoutStyle {
            width: Some(PANEL_SIZE.width),
            height: Some(PANEL_SIZE.height),
            padding: EdgeInsets::only(JANELA_TITULO_ALTO, Spacing::XL, Spacing::MD, Spacing::XL),
            ..LayoutStyle::default()
        },
    );
    let _ = host.insert(
        MODAL_ID,
        Node::new(Box::new(
            TextInput::new(PACKAGE_ID, String::new()).with_placeholder("br.com.exemplo"),
        ))
        .with_style(campo()),
    );
    // A folga entre os dois campos é uma peça vazia, e não uma soma escondida na
    // posição do seguinte: é ela que abriga a legenda do campo de baixo.
    let _ = host.declare(
        MODAL_ID,
        FIELD_GAP_ID,
        LayoutStyle {
            height: Some(30.0),
            ..LayoutStyle::default()
        },
    );
    let _ = host.insert(
        MODAL_ID,
        Node::new(Box::new(TextInput::new(NAME_ID, String::new()))).with_style(campo()),
    );
    // O que sobra empurra a fileira de ações para o pé do painel.
    let _ = host.declare(
        MODAL_ID,
        FILL_ID,
        LayoutStyle {
            flex_grow: 1.0,
            ..LayoutStyle::default()
        },
    );
    let _ = host.declare(
        MODAL_ID,
        ACTIONS_ID,
        LayoutStyle {
            direction: LayoutDirection::Row,
            main_align: MainAlign::End,
            height: Some(34.0),
            gap: Spacing::SM,
            ..LayoutStyle::default()
        },
    );
    for (id, label, command) in [
        (CANCEL_ID, "Cancelar", "new.cancel"),
        (CREATE_ID, "Criar", "new.create"),
    ] {
        let _ = host.insert(
            ACTIONS_ID,
            Node::new(Box::new(Button::new(id, label).with_command(command))).with_style(
                LayoutStyle {
                    width: Some(88.0),
                    height: Some(34.0),
                    ..LayoutStyle::default()
                },
            ),
        );
    }
}

/// A área que o arranjo deu a uma peça da janela.
fn area(host: &UiHost, id: WidgetId) -> Rect {
    host.bounds(id).unwrap_or(Rect::new(0.0, 0.0, 0.0, 0.0))
}

/// O percurso do `Tab` desta janela: pacote e depois nome.
///
/// Como escopo, e não como ordem global: enquanto a janela está aberta o `Tab`
/// fica preso nela, e ao fechá-la o foco volta a quem o tinha.
const FOCUS_SCOPE: [WidgetId; 2] = [PACKAGE_ID, NAME_ID];

/// O texto de um dos campos, lido do anfitrião.
fn field(host: &UiHost, id: WidgetId) -> &str {
    host.widget_as::<TextInput>(id)
        .map_or("", |campo| campo.value())
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
        host: &mut UiHost,
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
        if let Some(campo) = host.widget_as_mut::<TextInput>(PACKAGE_ID) {
            campo.set_value(package);
        }
        if let Some(campo) = host.widget_as_mut::<TextInput>(NAME_ID) {
            campo.set_value(String::new());
        }
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
        host.push_focus_scope(FOCUS_SCOPE.to_vec());
        host.request_focus(if allows_empty_name {
            PACKAGE_ID
        } else {
            NAME_ID
        });
        None
    }

    /// Relata o que impediu a criação, mantendo a janela aberta.
    pub(super) fn set_message(&mut self, message: impl Into<String>) {
        if let Some(dialog) = self.dialog.as_mut() {
            dialog.message = Some(message.into());
        }
    }

    pub(super) fn close(&mut self, host: &mut UiHost) {
        if self.modal.is_open() {
            host.pop_focus_scope();
        }
        self.modal.close();
        self.dialog = None;
    }

    /// Monta o pedido a partir do que está nos campos.
    ///
    /// O pacote é obrigatório: sem ele não há onde criar. O nome é obrigatório
    /// para classe e interface, e opcional para pacote — é o que permite criar o
    /// pacote e a primeira classe dele num gesto só.
    pub(super) fn submit(&mut self, host: &mut UiHost) -> NewItemOutcome {
        let Some(dialog) = self.dialog.as_ref() else {
            return NewItemOutcome::Idle;
        };
        let template_id = dialog.template.id.clone();
        let source_root = dialog.source_root.clone();
        let allows_empty_name = dialog.template.allows_empty_name;
        let package = field(host, PACKAGE_ID).trim().to_owned();
        let name = field(host, NAME_ID).trim().to_owned();
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
                    "new.create" => return self.submit(host),
                    "new.cancel" => self.close(host),
                    _ => {}
                }
            }
        }
        // Onde o cursor cai dentro do texto e para onde o foco vai já são do
        // anfitrião: ele entregou o clique ao campo e moveu o foco para ele.
        NewItemOutcome::Idle
    }

    /// Tecla dentro da janela.
    pub(super) fn key(&mut self, host: &mut UiHost, key: &str) -> NewItemOutcome {
        if self.dialog.is_none() {
            return NewItemOutcome::Idle;
        }
        match key.to_ascii_lowercase().as_str() {
            "enter" => return self.submit(host),
            "escape" => self.close(host),
            // O percurso está preso ao escopo desta janela: o `Tab` não escapa
            // para o que está atrás dela.
            "tab" => host.focus_next(false),
            // Apagar e mover o cursor são do campo: ele conhece as fronteiras de
            // caractere e a posição atual, e quem o alcança é o anfitrião, pelo
            // foco.
            _ => {
                host.event(&UiEvent::KeyDown(KeyEvent {
                    logical_key: key.to_owned(),
                    repeat: false,
                    modifiers: Modifiers::default(),
                }));
            }
        }
        NewItemOutcome::Idle
    }

    /// Texto digitado na janela.
    ///
    /// O texto entra pelo componente, e não por concatenação: é assim que ele
    /// aparece onde o cursor está, inclusive depois de um clique no meio do
    /// caminho já digitado.
    pub(super) fn text_input(&mut self, host: &mut UiHost, text: &str) {
        host.event(&UiEvent::TextInput(TextInputEvent {
            text: text.to_owned(),
        }));
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
        let (package, name) = (area(host, PACKAGE_ID), area(host, NAME_ID));
        // O título é do `ModalHost`, que já o desenha: escrever outro por cima
        // era o que aparecia duplicado.
        modal.paint(paint);
        caption(
            layout,
            paint,
            PACKAGE_CAPTION_ID,
            "Pacote",
            Point::new(package.origin.x, package.origin.y - CAPTION_OFFSET),
            IconTint::Muted,
        );
        caption(
            layout,
            paint,
            NAME_CAPTION_ID,
            &dialog.template.name_caption,
            Point::new(name.origin.x, name.origin.y - CAPTION_OFFSET),
            IconTint::Muted,
        );
        // Campos e botões são desenhados pelo anfitrião da tela, que é quem os
        // possui — e por isso eles acendem sob o ponteiro, afundam ao ser
        // pressionados e mostram o cursor onde o foco está.
        for id in [PACKAGE_ID, NAME_ID, CANCEL_ID, CREATE_ID] {
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
                // Abaixo do campo, e o campo diz onde acaba: a altura dele
                // está ali, na área que ele recebeu. Um número escrito aqui
                // seria a soma de duas coisas — a altura e a folga — que
                // deixaria de bater assim que uma das duas mudasse.
                Point::new(
                    name.origin.x,
                    name.origin.y + name.size.height + Spacing::SM,
                ),
                IconTint::Danger,
            );
        }
        true
    }

    /// Área de um campo, para quem precisa apontar um gesto dentro dele.
    #[cfg(test)]
    pub(super) fn field_area(host: &UiHost, package: bool) -> Rect {
        area(host, if package { PACKAGE_ID } else { NAME_ID })
    }

    /// O que está nos dois campos, para os testes.
    #[cfg(test)]
    pub(super) fn values(host: &UiHost) -> (&str, &str) {
        (field(host, PACKAGE_ID), field(host, NAME_ID))
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

