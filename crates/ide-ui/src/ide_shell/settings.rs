//! A janela de configurações: as seções da linguagem e a página de depuração.
//!
//! A janela é transacional: o que se mexe fica pendente e só o Salvar aplica —
//! e só o que mudou, porque reaplicar a toolchain que já valia derrubaria o
//! provider e reindexaria a biblioteca padrão por nada. Como as outras
//! superfícies, ela decide e o shell executa.

use ide_application::SettingsSection;

/// Onde a legenda de um campo fica: uma linha acima dele.
const LEGENDA_ACIMA: f32 = 16.0;
/// Onde o título de uma seção fica: duas linhas acima do primeiro campo.
const TITULO_ACIMA: f32 = 34.0;
/// Uma linha de texto de legenda, para o que se empilha abaixo de um campo.
///
/// É medida de texto, e não espaçamento: duas dicas seguidas ficam a uma linha
/// uma da outra.
const LINHA_DE_LEGENDA: f32 = 18.0;
use ui_api::{EventContext, EventResult, LayoutContext, PaintContext, Widget};
use ui_components::{
    Button, ComboBox, ComboBoxItem, IconTint, Label, ListSelection, ListView, ModalHost, Panel,
    SurfaceTone, TextInput,
};
/// A folga entre duas peças da janela: é ela que abriga a legenda da de baixo.
const FOLGA_ENTRE_CAMPOS: f32 = 20.0;
/// A folga entre duas seções, maior porque separa assuntos e não campos.
const FOLGA_ENTRE_SECOES: f32 = 46.0;
use ui_core::{
    ColorTokens, KeyEvent, Modifiers, Point, Rect, Size, Spacing, Theme, UiEvent, WidgetAction,
    WidgetId,
};
use ui_render_api::PaintCommand;

/// As áreas da janela, como quem desenha e quem roteia precisam vê-las.
///
/// Todas vêm do arranjo: esta forma é só o nome de cada uma.
pub(super) struct SettingsDialogGeometry {
    pub(super) sidebar: Rect,
    /// Primeira linha da navegação; as demais seguem por altura de linha.
    pub(super) compiler_option: Rect,
    pub(super) combo: Rect,
    pub(super) secondary_combo: Rect,
    pub(super) secondary_browse: Rect,
    pub(super) browse: Rect,
    pub(super) close: Rect,
    pub(super) save: Rect,
    pub(super) debug_host: Rect,
    pub(super) debug_port: Rect,
    pub(super) debug_attach: Rect,
}

impl SettingsDialogGeometry {
    /// Área que a lista de páginas ocupa: as linhas, e não a barra inteira.
    pub(super) fn compiler_option_row(&self, page_count: usize) -> Rect {
        Rect::new(
            self.compiler_option.origin.x,
            self.compiler_option.origin.y,
            self.compiler_option.size.width,
            PAGE_ROW_HEIGHT * page_count as f32,
        )
    }
}
use super::{JANELA_TITULO, JANELA_TITULO_ALTO, click_widget, primary_pointer};
use ide_domain::ToolRole;

use crate::settings::SettingsPage;
use ui_focus::FocusManager;
use ui_host::UiHost;
use ui_layout_api::{EdgeInsets, LayoutDirection, LayoutStyle, MainAlign};

const MODAL_ID: WidgetId = WidgetId(10_002);
const CLOSE_ID: WidgetId = WidgetId(10_005);
const SAVE_ID: WidgetId = WidgetId(10_030);
const TITLE_ID: WidgetId = WidgetId(10_031);
const CAPTION_ID: WidgetId = WidgetId(10_032);
const MESSAGE_ID: WidgetId = WidgetId(10_033);
const PAGES_ID: WidgetId = WidgetId(10_034);
const TOOLCHAIN_COMBO_ID: WidgetId = WidgetId(10_070);
const TOOLCHAIN_BROWSE_ID: WidgetId = WidgetId(10_071);
const SECONDARY_CAPTION_ID: WidgetId = WidgetId(10_072);
const SECONDARY_COMBO_ID: WidgetId = WidgetId(10_073);
const SECONDARY_BROWSE_ID: WidgetId = WidgetId(10_074);
const DEBUG_HOST_ID: WidgetId = WidgetId(10_006);
const DEBUG_PORT_ID: WidgetId = WidgetId(10_007);
const DEBUG_ATTACH_ID: WidgetId = WidgetId(10_008);
/// Última página, sempre depois das que a linguagem contribui.
pub(super) const DEBUG_PAGE_TITLE: &str = "Depuração";
pub(super) const PAGE_ROW_HEIGHT: f32 = 42.0;
const SIDEBAR_ID: WidgetId = WidgetId(10_100);
/// As peças de estrutura da moldura: a coluna da direita, a área da página e a
/// fileira de ações.
const RIGHT_ID: WidgetId = WidgetId(10_450);
const PAGE_ID: WidgetId = WidgetId(10_451);
const ACTIONS_ID: WidgetId = WidgetId(10_452);
/// A página de depuração: a coluna com o alvo e o botão de conectar.
const DEBUG_FORM_ID: WidgetId = WidgetId(10_453);
const DEBUG_TARGET_ID: WidgetId = WidgetId(10_454);
/// A página de escolhas: as duas fileiras de combo e botão de procurar.
const CHOICES_ID: WidgetId = WidgetId(10_455);
const PRIMARY_ROW_ID: WidgetId = WidgetId(10_456);
const SECONDARY_ROW_ID: WidgetId = WidgetId(10_457);
/// O painel: largo o bastante para a barra de páginas e o conteúdo lado a lado.
const PANEL_SIZE: Size = Size::new(780.0, 460.0);

/// Declara a moldura da janela: barra de páginas à esquerda, conteúdo à direita,
/// ações no pé.
///
/// Só a moldura. O interior de cada página continua sendo conta, porque cada
/// página tem o seu arranjo — e transformá-las é a etapa seguinte, uma por vez.
/// A barra desce até o pé do painel, por baixo das ações: por isso ela é irmã da
/// coluna da direita, e não parte dela.
pub(super) fn attach(host: &mut UiHost, layer: WidgetId) {
    let _ = host.declare(
        layer,
        MODAL_ID,
        LayoutStyle {
            direction: LayoutDirection::Row,
            width: Some(PANEL_SIZE.width),
            height: Some(PANEL_SIZE.height),
            // O alto é do título, que o `ModalHost` desenha.
            padding: EdgeInsets::only(JANELA_TITULO, 0.0, 0.0, 0.0),
            ..LayoutStyle::default()
        },
    );
    let _ = host.declare(
        MODAL_ID,
        SIDEBAR_ID,
        LayoutStyle {
            width: Some(210.0),
            // A lista não encosta no título: os 12 do alto são o respiro que a
            // barra sempre teve.
            padding: EdgeInsets::only(Spacing::MD, 0.0, 0.0, 0.0),
            ..LayoutStyle::default()
        },
    );
    let _ = host.declare(
        MODAL_ID,
        RIGHT_ID,
        LayoutStyle {
            flex_grow: 1.0,
            padding: EdgeInsets::only(0.0, Spacing::LG, Spacing::MD, 0.0),
            ..LayoutStyle::default()
        },
    );
    let _ = host.declare(
        RIGHT_ID,
        PAGE_ID,
        LayoutStyle {
            flex_grow: 1.0,
            ..LayoutStyle::default()
        },
    );
    let _ = host.declare(
        RIGHT_ID,
        ACTIONS_ID,
        LayoutStyle {
            direction: LayoutDirection::Row,
            main_align: MainAlign::End,
            height: Some(34.0),
            gap: Spacing::SM,
            ..LayoutStyle::default()
        },
    );
    // A lista de páginas mora na barra. A altura dela depende de quantas páginas
    // existem, e por isso é declarada de novo a cada quadro, em `place_widgets`.
    let _ = host.declare(
        SIDEBAR_ID,
        PAGES_ID,
        LayoutStyle {
            height: Some(PAGE_ROW_HEIGHT),
            ..LayoutStyle::default()
        },
    );
    // A página de escolhas: cada fileira tem o combo, que fica com o que sobra,
    // e o botão de procurar, de largura fixa.
    let _ = host.declare(
        PAGE_ID,
        CHOICES_ID,
        LayoutStyle {
            padding: EdgeInsets::only(JANELA_TITULO_ALTO, Spacing::MD, 0.0, Spacing::XL),
            gap: FOLGA_ENTRE_SECOES,
            ..LayoutStyle::default()
        },
    );
    for (row, combo, browse) in [
        (PRIMARY_ROW_ID, TOOLCHAIN_COMBO_ID, TOOLCHAIN_BROWSE_ID),
        (SECONDARY_ROW_ID, SECONDARY_COMBO_ID, SECONDARY_BROWSE_ID),
    ] {
        let _ = host.declare(
            CHOICES_ID,
            row,
            LayoutStyle {
                direction: LayoutDirection::Row,
                height: Some(36.0),
                gap: Spacing::SM,
                ..LayoutStyle::default()
            },
        );
        let _ = host.declare(
            row,
            combo,
            LayoutStyle {
                flex_grow: 1.0,
                min_width: Some(190.0),
                ..LayoutStyle::default()
            },
        );
        let _ = host.declare(
            row,
            browse,
            LayoutStyle {
                width: Some(112.0),
                ..LayoutStyle::default()
            },
        );
    }

    // A página de depuração: host e porta lado a lado, o botão embaixo.
    let _ = host.declare(
        PAGE_ID,
        DEBUG_FORM_ID,
        LayoutStyle {
            hidden: true,
            padding: EdgeInsets::only(JANELA_TITULO_ALTO, 0.0, 0.0, Spacing::XL),
            gap: FOLGA_ENTRE_CAMPOS,
            ..LayoutStyle::default()
        },
    );
    let _ = host.declare(
        DEBUG_FORM_ID,
        DEBUG_TARGET_ID,
        LayoutStyle {
            direction: LayoutDirection::Row,
            height: Some(36.0),
            gap: Spacing::MD,
            ..LayoutStyle::default()
        },
    );
    for (id, width) in [(DEBUG_HOST_ID, 220.0), (DEBUG_PORT_ID, 96.0)] {
        let _ = host.declare(
            DEBUG_TARGET_ID,
            id,
            LayoutStyle {
                width: Some(width),
                height: Some(36.0),
                ..LayoutStyle::default()
            },
        );
    }
    let _ = host.declare(
        DEBUG_FORM_ID,
        DEBUG_ATTACH_ID,
        LayoutStyle {
            width: Some(120.0),
            height: Some(34.0),
            ..LayoutStyle::default()
        },
    );

    // Cancelar e Salvar ainda são instâncias da janela, e por isso entram como
    // nós de estrutura: o arranjo diz onde eles ficam, e a janela os desenha.
    for id in [CLOSE_ID, SAVE_ID] {
        let _ = host.declare(
            ACTIONS_ID,
            id,
            LayoutStyle {
                width: Some(88.0),
                height: Some(34.0),
                ..LayoutStyle::default()
            },
        );
    }
}

/// A área que o arranjo deu a uma peça da moldura.
fn area(host: &UiHost, id: WidgetId) -> Rect {
    host.bounds(id).unwrap_or(Rect::new(0.0, 0.0, 0.0, 0.0))
}

const DEBUG_TITLE_ID: WidgetId = WidgetId(10_101);
const DEBUG_CAPTION_ID: WidgetId = WidgetId(10_102);
const DEBUG_HINT_ID: WidgetId = WidgetId(10_103);
const DEBUG_HINT_SCOPE_ID: WidgetId = WidgetId(10_104);

/// Qual das duas escolhas da seção o gesto tocou.
/// Índice da seção que recebeu o gesto, quando ela é de contribuição.
///
/// A janela sabe qual página está aberta; o que faltava era dizê-lo no
/// resultado. Sem isso, o comando não tinha como identificar a seção, e com uma
/// seção só ninguém percebia. Ver a fase 0 da `23`.
fn contribution_section(page: SettingsPage) -> Option<usize> {
    match page {
        SettingsPage::Contribution(index) => Some(index),
        SettingsPage::Debug => None,
    }
}

/// O que a janela concluiu, para o shell executar.
pub(super) enum SettingsOutcome {
    /// O gesto não concluiu nada — o que ele mudou já está na janela.
    Idle,
    /// Pedir à aplicação que aponte uma instalação para um papel da seção.
    Browse { section: usize, role: ToolRole },
    /// Aplicar o que mudou e nada além disso.
    Save {
        section: usize,
        primary: Option<usize>,
        secondary: Option<usize>,
    },
    /// Conectar ao alvo de depuração informado.
    Attach { host: String, port: u16 },
}

/// O que a janela vai devolver se for cancelada, e o que está pendente.
struct SettingsDialog {
    message: Option<String>,
    pending_toolchain: Option<usize>,
    original_toolchain: Option<usize>,
    /// Segunda ferramenta escolhida na janela e ainda não aplicada.
    pending_secondary: Option<usize>,
    original_secondary: Option<usize>,
    original_debug_host: String,
    original_debug_port: String,
}

pub(super) struct SettingsSurface {
    modal: ModalHost,
    toolchain_combo: ComboBox,
    toolchain_browse_button: Button,
    /// Segunda escolha da seção, ao lado da primeira e com o mesmo gesto.
    secondary_combo: ComboBox,
    secondary_browse_button: Button,
    close_button: Button,
    save_button: Button,
    pages: ListView,
    dialog: Option<SettingsDialog>,
    page: SettingsPage,
    /// Campo em foco na página de depuração. O gerenciador é da biblioteca:
    /// guardar quem tem o foco era a conta que cada tela refazia à mão.
    focus: FocusManager,
    debug_host: TextInput,
    debug_port: TextInput,
    debug_attach_button: Button,
}

impl Default for SettingsSurface {
    fn default() -> Self {
        Self {
            modal: ModalHost::new(MODAL_ID, "Configurações", PANEL_SIZE),
            toolchain_combo: ComboBox::new(TOOLCHAIN_COMBO_ID, Vec::new()),
            secondary_combo: ComboBox::new(SECONDARY_COMBO_ID, Vec::new()),
            secondary_browse_button: Button::new(SECONDARY_BROWSE_ID, "Procurar...")
                .with_command("tool.browse"),
            toolchain_browse_button: Button::new(TOOLCHAIN_BROWSE_ID, "Procurar...")
                .with_command("toolchain.browse"),
            close_button: Button::new(CLOSE_ID, "Cancelar").with_command("settings.cancel"),
            save_button: Button::new(SAVE_ID, "Salvar").with_command("settings.save"),
            pages: ListView::new(PAGES_ID, [DEBUG_PAGE_TITLE])
                .with_row_height(PAGE_ROW_HEIGHT)
                .with_selection(ListSelection::Marker),
            dialog: None,
            page: SettingsPage::default(),
            focus: debug_focus(),
            debug_host: TextInput::new(DEBUG_HOST_ID, "127.0.0.1").with_placeholder("host"),
            debug_port: TextInput::new(DEBUG_PORT_ID, "8000").with_placeholder("porta"),
            debug_attach_button: Button::new(DEBUG_ATTACH_ID, "Conectar")
                .with_command("debug.attach"),
        }
    }
}

/// O percurso do `Tab` da página de depuração: host e depois porta.
fn debug_focus() -> FocusManager {
    let mut focus = FocusManager::default();
    focus.register(DEBUG_HOST_ID);
    focus.register(DEBUG_PORT_ID);
    focus
}

impl SettingsSurface {
    #[must_use]
    pub(super) const fn is_open(&self) -> bool {
        self.modal.is_open()
    }

    /// Repõe a navegação com as seções contribuídas mais a de depuração.
    pub(super) fn set_pages(&mut self, titles: Vec<String>) {
        self.pages = ListView::new(PAGES_ID, titles)
            .with_row_height(PAGE_ROW_HEIGHT)
            .with_selection(ListSelection::Marker);
    }

    /// O `Procurar...` recebe o título que a seção declarou.
    pub(super) fn set_browse_title(&mut self, title: String) {
        self.toolchain_browse_button =
            Button::new(TOOLCHAIN_BROWSE_ID, title).with_command("toolchain.browse");
    }

    /// Abre a janela e começa a transação.
    pub(super) fn open(&mut self, toolchain_items: Vec<String>, selected_toolchain: usize) {
        self.toolchain_combo.set_items(combo_items(toolchain_items));
        self.toolchain_combo.set_selected(selected_toolchain);
        self.modal.open();
        // Reabrir a janela recomeça a transação: o que ficou pendente de uma
        // abertura anterior foi descartado com ela.
        let segunda = self.selected_secondary();
        self.dialog = Some(SettingsDialog {
            message: None,
            pending_toolchain: None,
            original_toolchain: Some(selected_toolchain),
            pending_secondary: None,
            original_secondary: segunda,
            original_debug_host: self.debug_host.value().to_owned(),
            original_debug_port: self.debug_port.value().to_owned(),
        });
    }

    /// Repõe a lista da segunda escolha da seção, com uma delas marcada.
    ///
    /// Mesma mecânica da primeira: o `Procurar...` põe o que foi apontado na
    /// lista e o deixa pendente, sem recomeçar a transação da janela.
    pub(super) fn set_secondary_options(&mut self, items: Vec<String>, selected: Option<usize>) {
        self.secondary_combo.set_items(combo_items(items));
        if let Some(index) = selected {
            self.secondary_combo.set_selected(index);
        }
        if let Some(dialog) = self.dialog.as_mut() {
            dialog.pending_secondary = selected;
        }
    }

    /// Segunda ferramenta escolhida na janela, para quem for aplicar.
    #[must_use]
    pub(super) fn selected_secondary(&self) -> Option<usize> {
        (self.secondary_combo.item_count() > 0).then(|| self.secondary_combo.selected_index())
    }

    /// Repõe a lista de toolchains e deixa uma escolhida, sem sair da transação.
    ///
    /// É o que o `Procurar...` precisa: a instalação apontada entra na lista e
    /// fica pendente como qualquer escolha feita no combo. Reabrir a janela
    /// recomeçaria a transação e apagaria o que já estava pendente.
    pub(super) fn set_toolchain_options(&mut self, items: Vec<String>, pending: usize) {
        self.toolchain_combo.set_items(combo_items(items));
        self.toolchain_combo.set_selected(pending);
        if let Some(dialog) = self.dialog.as_mut() {
            dialog.pending_toolchain = Some(pending);
            dialog.message = None;
        }
    }

    pub(super) fn set_message(&mut self, message: impl Into<String>) {
        if let Some(dialog) = self.dialog.as_mut() {
            dialog.message = Some(message.into());
        }
    }

    pub(super) fn set_page(&mut self, page: SettingsPage) {
        self.page = page;
        self.focus.request_focus(WidgetId(0));
    }

    #[must_use]
    pub(super) const fn page(&self) -> SettingsPage {
        self.page
    }

    /// Alvo de depuração apresentado na janela.
    pub(super) fn set_debug_target(&mut self, host: &str, port: u16) {
        self.debug_host.set_value(host);
        self.debug_port.set_value(port.to_string());
    }

    /// O alvo, quando host e porta são utilizáveis.
    #[must_use]
    pub(super) fn debug_target(&self) -> Option<(String, u16)> {
        let host = self.debug_host.value().trim().to_owned();
        let port = self.debug_port.value().trim().parse::<u16>().ok()?;
        (!host.is_empty() && port > 0).then_some((host, port))
    }

    /// Descarta tudo o que foi mexido e fecha.
    pub(super) fn cancel(&mut self) {
        if let Some(dialog) = self.dialog.take() {
            if let Some(original) = dialog.original_toolchain {
                self.toolchain_combo.set_selected(original);
            }
            if let Some(original) = dialog.original_secondary {
                self.secondary_combo.set_selected(original);
            }
            self.debug_host.set_value(dialog.original_debug_host);
            self.debug_port.set_value(dialog.original_debug_port);
        }
        self.modal.close();
    }

    /// Declara ao anfitrião da tela a área de cada peça da janela.
    ///
    /// A lista de páginas e as três peças da página de depuração: são elas que o
    /// acerto precisa distinguir. O resto da janela responde pelo painel.
    /// Ajusta o que muda com o estado: quantas páginas há, e qual está aberta.
    ///
    /// Não é posicionar — é declarar de novo. A altura da lista vem da contagem
    /// de páginas, e a página de depuração entra e sai do arranjo conforme a
    /// escolha.
    pub(super) fn sync_declaration(&mut self, host: &mut UiHost, section_count: usize) {
        host.set_style(
            PAGES_ID,
            LayoutStyle {
                height: Some(PAGE_ROW_HEIGHT * (section_count + 1) as f32),
                ..LayoutStyle::default()
            },
        );
        host.set_style(
            CHOICES_ID,
            LayoutStyle {
                hidden: self.page == SettingsPage::Debug,
                padding: EdgeInsets::only(JANELA_TITULO_ALTO, Spacing::MD, 0.0, Spacing::XL),
                gap: FOLGA_ENTRE_SECOES,
                ..LayoutStyle::default()
            },
        );
        host.set_style(
            DEBUG_FORM_ID,
            LayoutStyle {
                hidden: self.page != SettingsPage::Debug,
                padding: EdgeInsets::only(JANELA_TITULO_ALTO, 0.0, 0.0, Spacing::XL),
                gap: FOLGA_ENTRE_CAMPOS,
                ..LayoutStyle::default()
            },
        );
    }

    pub(super) fn pointer_down(
        &mut self,
        host: &mut UiHost,
        context: &LayoutContext,
        point: Point,
        sections: &[SettingsSection],
    ) -> SettingsOutcome {
        if !self.modal.is_open() {
            return SettingsOutcome::Idle;
        }
        let geometry = self.geometry(host);
        // Qual página foi clicada é a lista quem sabe: altura de linha e rolagem
        // são dela.
        let mut pages = self.pages_for(context, &geometry, sections.len());
        pages.event(
            &mut EventContext::default(),
            &UiEvent::PointerDown(primary_pointer(point)),
        );
        // Se o clique caiu na navegação é o anfitrião quem diz, pela área que
        // ela ocupa na pilha.
        if let Some(page) = pages.selected().map(|index| {
            if index < sections.len() {
                SettingsPage::Contribution(index)
            } else {
                SettingsPage::Debug
            }
        }) && host.hit_test(point).next() == Some(PAGES_ID)
        {
            self.page = page;
            self.focus.request_focus(WidgetId(0));
            return SettingsOutcome::Idle;
        }
        if self.page == SettingsPage::Debug {
            return self.debug_page_pointer_down(host, context, point, &geometry);
        }
        self.toolchain_combo.layout(context, geometry.combo);
        self.toolchain_browse_button
            .layout(context, geometry.browse);
        // A segunda escolha da seção recebe o clique pelo mesmo caminho: sem
        // isso ela é desenhada e nunca respondia.
        self.secondary_combo
            .layout(context, geometry.secondary_combo);
        self.secondary_browse_button
            .layout(context, geometry.secondary_browse);
        self.close_button.layout(context, geometry.close);
        self.save_button.layout(context, geometry.save);
        let event = UiEvent::PointerDown(primary_pointer(point));
        let mut event_context = EventContext::default();
        let combo_result = self.toolchain_combo.event(&mut event_context, &event);
        let combo_consumed = !matches!(combo_result, EventResult::Ignored);
        let (outcome, handled) = self.handle_action(combo_result);
        if handled || combo_consumed {
            return outcome;
        }
        let browse_result = click_widget(&mut self.toolchain_browse_button, point);
        let (outcome, handled) = self.handle_action(browse_result);
        if handled {
            return outcome;
        }
        let segunda = self.secondary_combo.event(&mut event_context, &event);
        let segunda_consumiu = !matches!(segunda, EventResult::Ignored);
        let (outcome, handled) = self.handle_action(segunda);
        if handled || segunda_consumiu {
            return outcome;
        }
        let segunda_browse = click_widget(&mut self.secondary_browse_button, point);
        let (outcome, handled) = self.handle_action(segunda_browse);
        if handled {
            return outcome;
        }
        let close_result = click_widget(&mut self.close_button, point);
        let (outcome, handled) = self.handle_action(close_result);
        if handled {
            return outcome;
        }
        let save_result = click_widget(&mut self.save_button, point);
        let (outcome, handled) = self.handle_action(save_result);
        if handled {
            return outcome;
        }
        let _ = self.modal.event(&mut event_context, &event);
        SettingsOutcome::Idle
    }

    fn debug_page_pointer_down(
        &mut self,
        host: &mut UiHost,
        context: &LayoutContext,
        point: Point,
        geometry: &SettingsDialogGeometry,
    ) -> SettingsOutcome {
        // Quem estava sob o ponteiro é o anfitrião quem diz, pelas áreas que a
        // página lhe entregou.
        match host.click(point).target {
            Some(DEBUG_HOST_ID) => {
                self.focus.request_focus(DEBUG_HOST_ID);
                return SettingsOutcome::Idle;
            }
            Some(DEBUG_PORT_ID) => {
                self.focus.request_focus(DEBUG_PORT_ID);
                return SettingsOutcome::Idle;
            }
            // O foco é preservado para o usuário corrigir um valor recusado.
            Some(DEBUG_ATTACH_ID) => return self.attach(),
            _ => {}
        }
        self.focus.request_focus(WidgetId(0));
        self.close_button.layout(context, geometry.close);
        let close_result = click_widget(&mut self.close_button, point);
        let (outcome, handled) = self.handle_action(close_result);
        if handled {
            return outcome;
        }
        self.save_button.layout(context, geometry.save);
        let save_result = click_widget(&mut self.save_button, point);
        self.handle_action(save_result).0
    }

    /// Tecla enquanto a janela está aberta.
    ///
    /// Com uma das listas aberta, as setas e o Enter são dela; o que sobra vai
    /// ao `ModalHost`, e a janela fechada por dentro dele também é cancelamento.
    pub(super) fn key(&mut self, key: &str, modifiers: Modifiers) -> SettingsOutcome {
        if let Some(focus) = self.focus.focused() {
            match key {
                "Backspace" => {
                    let input = if focus == DEBUG_HOST_ID {
                        &mut self.debug_host
                    } else {
                        &mut self.debug_port
                    };
                    let mut value = input.value().to_owned();
                    value.pop();
                    input.set_value(value);
                    return SettingsOutcome::Idle;
                }
                "Enter" => return self.attach(),
                _ => {}
            }
        }
        let event = UiEvent::KeyDown(KeyEvent {
            logical_key: key.to_owned(),
            repeat: false,
            modifiers,
        });
        let mut context = EventContext::default();
        let result = self.toolchain_combo.event(&mut context, &event);
        let (outcome, atendida) = self.handle_action(result);
        let result = if atendida {
            EventResult::Handled
        } else {
            // Com a segunda lista aberta, as setas e o Enter são dela.
            self.secondary_combo.event(&mut context, &event)
        };
        let (segunda, handled) = self.handle_action(result);
        if handled {
            return segunda;
        }
        let _ = self.modal.event(&mut context, &event);
        // A janela fechada por dentro do componente — Esc no `ModalHost` —
        // também é cancelamento.
        if !self.modal.is_open() {
            self.cancel();
        }
        outcome
    }

    /// Digitação enquanto a página de depuração está em foco.
    pub(super) fn text_input(&mut self, text: &str) -> bool {
        let Some(focus) = self.focus.focused() else {
            return false;
        };
        let input = if focus == DEBUG_HOST_ID {
            &mut self.debug_host
        } else {
            &mut self.debug_port
        };
        let mut value = input.value().to_owned();
        value.push_str(text);
        input.set_value(value);
        true
    }

    /// Valida host e porta antes de pedir a conexão.
    fn attach(&mut self) -> SettingsOutcome {
        let host = self.debug_host.value().trim().to_owned();
        let port = self.debug_port.value().trim().parse::<u16>().ok();
        match (host.is_empty(), port) {
            (false, Some(port)) if port > 0 => {
                self.modal.close();
                self.dialog = None;
                self.focus.request_focus(WidgetId(0));
                SettingsOutcome::Attach { host, port }
            }
            _ => {
                self.set_message("Informe um host e uma porta de depuração válidos.");
                SettingsOutcome::Idle
            }
        }
    }

    /// Traduz o que um componente decidiu. O `bool` diz se foi atendido aqui.
    fn handle_action(&mut self, result: EventResult) -> (SettingsOutcome, bool) {
        let EventResult::Action(action) = result else {
            return (SettingsOutcome::Idle, false);
        };
        // Qual das duas listas falou é o id que diz — antes eram dois prefixos
        // combinados entre a montagem e a leitura, um de cada lado da tela.
        if let WidgetAction::ItemSelected { widget_id, index } = action {
            // A escolha fica pendente: quem aplica é o Salvar.
            let pendente = match widget_id {
                TOOLCHAIN_COMBO_ID => self
                    .dialog
                    .as_mut()
                    .map(|dialog| &mut dialog.pending_toolchain),
                SECONDARY_COMBO_ID => self
                    .dialog
                    .as_mut()
                    .map(|dialog| &mut dialog.pending_secondary),
                _ => return (SettingsOutcome::Idle, false),
            };
            if let Some(pendente) = pendente {
                *pendente = Some(index);
            }
            return (SettingsOutcome::Idle, true);
        }
        let WidgetAction::Command(command) = action else {
            return (SettingsOutcome::Idle, false);
        };
        match command.0.as_str() {
            "toolchain.browse" => (self.browse(ToolRole::Primary), true),
            "tool.browse" => (self.browse(ToolRole::Secondary), true),
            "settings.save" => (self.save(), true),
            "settings.cancel" => {
                self.cancel();
                (SettingsOutcome::Idle, true)
            }
            _ => (SettingsOutcome::Idle, false),
        }
    }

    /// Fecha aplicando só o que mudou.
    fn save(&mut self) -> SettingsOutcome {
        let toolchain = self.dialog.as_ref().and_then(|dialog| {
            dialog
                .pending_toolchain
                .filter(|index| dialog.original_toolchain != Some(*index))
        });
        let secondary = self.dialog.as_ref().and_then(|dialog| {
            dialog
                .pending_secondary
                .filter(|index| dialog.original_secondary != Some(*index))
        });
        self.modal.close();
        self.dialog = None;
        let Some(section) = contribution_section(self.page) else {
            return SettingsOutcome::Idle;
        };
        SettingsOutcome::Save {
            section,
            primary: toolchain,
            secondary,
        }
    }

    /// Pede a pasta de uma instalação, dizendo de qual seção e papel.
    fn browse(&self, role: ToolRole) -> SettingsOutcome {
        contribution_section(self.page).map_or(SettingsOutcome::Idle, |section| {
            SettingsOutcome::Browse { section, role }
        })
    }

    /// Desenha a janela inteira, na ordem em que ela se sobrepõe.
    ///
    /// Os dois contextos vêm de fora porque o tema e a fonte são do shell; aqui
    /// se decide o que cada componente mostra e onde ele fica.
    pub(super) fn paint(
        &self,
        host: &UiHost,
        layout: &LayoutContext,
        size: Size,
        sections: &[SettingsSection],
        colors: ColorTokens,
        // Dois contextos: o véu e o painel vão num, o conteúdo no outro, para a
        // janela ficar por cima do que a moldura desenhou.
        (mut modal_paint, mut component_paint): (PaintContext, PaintContext),
    ) -> Vec<PaintCommand> {
        if !self.modal.is_open() {
            return Vec::new();
        }
        let mut modal = self.modal.clone();
        modal.layout(layout, Rect::new(0.0, 0.0, size.width, size.height));
        let geometry = self.geometry(host);
        modal.paint(&mut modal_paint);
        let mut commands = modal_paint.into_commands();
        // A barra lateral da janela é uma superfície da biblioteca.
        let mut sidebar = Panel::new(SIDEBAR_ID, SurfaceTone::Surface);
        sidebar.layout(layout, geometry.sidebar);
        sidebar.paint(&mut component_paint);
        // A navegação entre páginas é a `ListView` da biblioteca, no estilo de
        // marcador: a linha ativa ganha a barra de destaque e continua sendo um
        // rótulo para ler.
        self.pages_for(layout, &geometry, sections.len())
            .paint(&mut component_paint);
        match self.page {
            SettingsPage::Contribution(index) => {
                self.paint_contribution_page(
                    layout,
                    &mut component_paint,
                    &geometry,
                    sections,
                    index,
                );
            }
            SettingsPage::Debug => {
                commands.extend(self.paint_debug_page(&geometry, colors));
                for (widget, id, rect) in [
                    (&self.debug_host, DEBUG_HOST_ID, geometry.debug_host),
                    (&self.debug_port, DEBUG_PORT_ID, geometry.debug_port),
                ] {
                    let mut widget = widget.clone();
                    // Quem desenha o foco é o campo: a cópia que vai à tela
                    // precisa saber que o tem. Antes a janela contornava o
                    // retângulo por fora, e por isso o cursor não aparecia.
                    if self.focus.focused() == Some(id) {
                        widget.event(&mut EventContext::default(), &UiEvent::FocusGained);
                    }
                    widget.layout(layout, rect);
                    widget.paint(&mut component_paint);
                }
                let mut attach = self.debug_attach_button.clone();
                attach.layout(layout, geometry.debug_attach);
                attach.paint(&mut component_paint);
            }
        }
        for (button, rect) in [
            (&self.close_button, geometry.close),
            (&self.save_button, geometry.save),
        ] {
            let mut button = button.clone();
            button.layout(layout, rect);
            button.paint(&mut component_paint);
        }
        if let Some(message) = self
            .dialog
            .as_ref()
            .and_then(|dialog| dialog.message.as_ref())
        {
            text(
                layout,
                &mut component_paint,
                MESSAGE_ID,
                message,
                Point::new(
                    geometry.combo.origin.x,
                    geometry.combo.origin.y + geometry.combo.size.height + Spacing::MD,
                ),
                13.0,
                IconTint::Danger,
            );
        }
        commands.extend(component_paint.into_commands());
        commands
    }

    /// A página de uma seção contribuída: título, legenda e as escolhas.
    fn paint_contribution_page(
        &self,
        layout: &LayoutContext,
        paint: &mut PaintContext,
        geometry: &SettingsDialogGeometry,
        sections: &[SettingsSection],
        index: usize,
    ) {
        let section = sections.get(index);
        // Título e legenda são `Label` da biblioteca: tamanho e cor vêm do tema,
        // não de números escritos aqui.
        text(
            layout,
            paint,
            TITLE_ID,
            section.map_or("Ferramenta", |section| section.title.as_str()),
            Point::new(geometry.combo.origin.x, geometry.combo.origin.y - TITULO_ACIMA),
            17.0,
            IconTint::Text,
        );
        text(
            layout,
            paint,
            CAPTION_ID,
            section.map_or("Toolchain", |section| section.field_caption.as_str()),
            Point::new(geometry.combo.origin.x, geometry.combo.origin.y - LEGENDA_ACIMA),
            13.0,
            IconTint::Muted,
        );
        let mut combo = self.toolchain_combo.clone();
        combo.layout(layout, geometry.combo);
        combo.paint(paint);
        let mut browse = self.toolchain_browse_button.clone();
        browse.layout(layout, geometry.browse);
        browse.paint(paint);
        // A segunda escolha só existe se a seção declarar uma: a tela desenha o
        // que lhe dizem, sem saber o que é.
        let Some(caption) = sections
            .first()
            .and_then(|section| section.secondary_caption.clone())
        else {
            return;
        };
        text(
            layout,
            paint,
            SECONDARY_CAPTION_ID,
            &caption,
            Point::new(
                geometry.secondary_combo.origin.x,
                geometry.secondary_combo.origin.y - LEGENDA_ACIMA,
            ),
            13.0,
            IconTint::Muted,
        );
        let mut combo = self.secondary_combo.clone();
        combo.layout(layout, geometry.secondary_combo);
        combo.paint(paint);
        let mut browse = self.secondary_browse_button.clone();
        browse.layout(layout, geometry.secondary_browse);
        browse.paint(paint);
    }

    fn paint_debug_page(
        &self,
        geometry: &SettingsDialogGeometry,
        colors: ColorTokens,
    ) -> Vec<PaintCommand> {
        let origin = geometry.debug_host.origin;
        // Título e explicações são `Label`: o tom diz o papel, e o tema resolve a
        // cor. Escritos como texto cru, ficavam de fora dos dois.
        let mut paint = PaintContext::with_theme(Theme {
            colors,
            ..Theme::default()
        });
        for (id, texto, origem, tamanho, tom) in [
            (
                DEBUG_TITLE_ID,
                "Depuração",
                Point::new(origin.x, origin.y - TITULO_ACIMA),
                17.0,
                IconTint::Text,
            ),
            (
                DEBUG_CAPTION_ID,
                "Host e porta de depuração do processo em execução",
                Point::new(origin.x, origin.y - LEGENDA_ACIMA),
                13.0,
                IconTint::Muted,
            ),
            (
                DEBUG_HINT_ID,
                "Inicie o servidor com -agentlib:jdwp=transport=dt_socket,server=y,suspend=n,address=*:8000",
                Point::new(
                    origin.x,
                    geometry.debug_attach.origin.y
                        + geometry.debug_attach.size.height
                        + Spacing::MD,
                ),
                12.0,
                IconTint::Muted,
            ),
            (
                DEBUG_HINT_SCOPE_ID,
                "Vale para qualquer processo depurado: servidor, container ou ferramenta.",
                Point::new(
                    origin.x,
                    geometry.debug_attach.origin.y
                        + geometry.debug_attach.size.height
                        + Spacing::MD
                        + LINHA_DE_LEGENDA,
                ),
                12.0,
                IconTint::Muted,
            ),
        ] {
            text(
                &LayoutContext::default(),
                &mut paint,
                id,
                texto,
                origem,
                tamanho,
                tom,
            );
        }
        paint.into_commands()
    }

    /// Lista de páginas posicionada, com a página atual selecionada.
    ///
    /// A área ocupada é a das linhas, não a barra inteira: a lista responde pelo
    /// que ela desenha, e o resto da barra é fundo do painel.
    fn pages_for(
        &self,
        layout: &LayoutContext,
        geometry: &SettingsDialogGeometry,
        section_count: usize,
    ) -> ListView {
        let mut pages = self.pages.clone();
        pages.set_selected(Some(match self.page {
            SettingsPage::Contribution(index) => index,
            SettingsPage::Debug => section_count,
        }));
        // A área é a que o arranjo deu à lista: pintar noutra faria a linha
        // acertada não ser a linha vista.
        pages.layout(layout, geometry.compiler_option_row(section_count + 1));
        pages
    }

    /// As áreas da janela, já centralizada numa tela deste tamanho.
    pub(super) fn geometry(&self, host: &UiHost) -> SettingsDialogGeometry {
        let pages = area(host, PAGES_ID);
        SettingsDialogGeometry {
            sidebar: area(host, SIDEBAR_ID),
            // A primeira linha da navegação é o alto da lista, que o arranjo
            // posiciona: ler daqui é o que impede a lista ser pintada num lugar
            // e acertada noutro.
            compiler_option: Rect::new(pages.origin.x, pages.origin.y, 210.0, PAGE_ROW_HEIGHT),
            combo: area(host, TOOLCHAIN_COMBO_ID),
            browse: area(host, TOOLCHAIN_BROWSE_ID),
            secondary_combo: area(host, SECONDARY_COMBO_ID),
            secondary_browse: area(host, SECONDARY_BROWSE_ID),
            close: area(host, CLOSE_ID),
            save: area(host, SAVE_ID),
            debug_host: area(host, DEBUG_HOST_ID),
            debug_port: area(host, DEBUG_PORT_ID),
            debug_attach: area(host, DEBUG_ATTACH_ID),
        }
    }

    /// A toolchain marcada no combo, para os testes.
    #[cfg(test)]
    pub(super) fn selected_toolchain(&self) -> usize {
        self.toolchain_combo.selected_index()
    }
}

/// Um texto da janela, com a `Label` da biblioteca.
///
/// A IDE escolhe o papel — título, legenda, mensagem de erro — e a posição. Cor
/// e desenho vêm do componente, e é assim que o tema alcança este texto.
fn text(
    layout: &LayoutContext,
    paint: &mut PaintContext,
    id: WidgetId,
    content: &str,
    origin: Point,
    font_size: f32,
    tone: IconTint,
) {
    let mut widget = Label::new(id, content)
        .with_font_size(font_size)
        .with_tone(tone);
    widget.layout(layout, Rect::new(origin.x, origin.y, 0.0, 0.0));
    widget.paint(paint);
}

/// Os rótulos viram itens de combo, indexados na ordem em que chegaram.
fn combo_items(items: Vec<String>) -> Vec<ComboBoxItem> {
    items
        .into_iter()
        .enumerate()
        .map(|(index, label)| ComboBoxItem::new(label, index.to_string()))
        .collect()
}
