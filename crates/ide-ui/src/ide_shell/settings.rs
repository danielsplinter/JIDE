//! A janela de configurações: as seções da linguagem e a página de depuração.
//!
//! A janela é transacional: o que se mexe fica pendente e só o Salvar aplica —
//! e só o que mudou, porque reaplicar a toolchain que já valia derrubaria o
//! provider e reindexaria a biblioteca padrão por nada. Como as outras
//! superfícies, ela decide e o shell executa.

use ide_application::SettingsSection;
use ui_api::{EventContext, EventResult, LayoutContext, PaintContext, Widget};
use ui_components::{
    Button, ComboBox, ComboBoxItem, IconTint, Label, ListSelection, ListView, ModalHost, TextInput,
};
use ui_core::{
    ColorTokens, KeyEvent, Modifiers, Point, Rect, Size, UiEvent, WidgetAction, WidgetId,
};
use ui_render_api::PaintCommand;

use super::geometry::{SettingsDialogGeometry, settings_dialog_geometry, settings_pages_rect};
use super::{click_widget, fill, label, primary_pointer, stroke};
use crate::settings::SettingsPage;

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

/// Qual das duas escolhas da seção o gesto tocou.
pub(super) enum ToolSlot {
    Primary,
    Secondary,
}

/// O que a janela concluiu, para o shell executar.
pub(super) enum SettingsOutcome {
    /// O gesto não concluiu nada — o que ele mudou já está na janela.
    Idle,
    /// Pedir à aplicação que aponte uma instalação.
    Browse(ToolSlot),
    /// Aplicar o que mudou e nada além disso.
    Save {
        toolchain: Option<usize>,
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
    /// Campo em foco na página de depuração, que não usa `TextInput` focado.
    focus: Option<WidgetId>,
    debug_host: TextInput,
    debug_port: TextInput,
    debug_attach_button: Button,
}

impl Default for SettingsSurface {
    fn default() -> Self {
        Self {
            modal: ModalHost::new(MODAL_ID, "Configurações", Size::new(780.0, 460.0)),
            toolchain_combo: ComboBox::new(TOOLCHAIN_COMBO_ID, Vec::new())
                .with_command_prefix("toolchain.select."),
            secondary_combo: ComboBox::new(SECONDARY_COMBO_ID, Vec::new())
                .with_command_prefix("tool.select."),
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
            focus: None,
            debug_host: TextInput::new(DEBUG_HOST_ID, "127.0.0.1").with_placeholder("host"),
            debug_port: TextInput::new(DEBUG_PORT_ID, "8000").with_placeholder("porta"),
            debug_attach_button: Button::new(DEBUG_ATTACH_ID, "Conectar")
                .with_command("debug.attach"),
        }
    }
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
        self.focus = None;
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

    pub(super) fn pointer_down(
        &mut self,
        context: &LayoutContext,
        point: Point,
        size: Size,
        sections: &[SettingsSection],
    ) -> SettingsOutcome {
        if !self.modal.is_open() {
            return SettingsOutcome::Idle;
        }
        let geometry = self.geometry(context, size);
        // Qual página foi clicada é a lista quem sabe: altura de linha e rolagem
        // são dela.
        let mut pages = self.pages_for(context, &geometry, sections.len());
        pages.event(
            &mut EventContext::default(),
            &UiEvent::PointerDown(primary_pointer(point)),
        );
        if let Some(page) = pages.selected().map(|index| {
            if index < sections.len() {
                SettingsPage::Contribution(index)
            } else {
                SettingsPage::Debug
            }
        }) && settings_pages_rect(&geometry, sections.len() + 1).contains(point)
        {
            self.page = page;
            self.focus = None;
            return SettingsOutcome::Idle;
        }
        if self.page == SettingsPage::Debug {
            return self.debug_page_pointer_down(context, point, &geometry);
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
        context: &LayoutContext,
        point: Point,
        geometry: &SettingsDialogGeometry,
    ) -> SettingsOutcome {
        if geometry.debug_host.contains(point) {
            self.focus = Some(DEBUG_HOST_ID);
            return SettingsOutcome::Idle;
        }
        if geometry.debug_port.contains(point) {
            self.focus = Some(DEBUG_PORT_ID);
            return SettingsOutcome::Idle;
        }
        if geometry.debug_attach.contains(point) {
            // O foco é preservado para o usuário corrigir um valor recusado.
            return self.attach();
        }
        self.focus = None;
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
        if let Some(focus) = self.focus {
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
        let Some(focus) = self.focus else {
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
                self.focus = None;
                SettingsOutcome::Attach { host, port }
            }
            _ => {
                self.set_message("Informe um host e uma porta de depuração válidos.");
                SettingsOutcome::Idle
            }
        }
    }

    /// Traduz o comando de um componente. O `bool` diz se ele foi atendido aqui.
    fn handle_action(&mut self, result: EventResult) -> (SettingsOutcome, bool) {
        let EventResult::Action(WidgetAction::Command(command)) = result else {
            return (SettingsOutcome::Idle, false);
        };
        if let Some(index) = command
            .0
            .strip_prefix("toolchain.select.")
            .and_then(|value| value.parse::<usize>().ok())
        {
            // A escolha fica pendente: quem aplica é o Salvar.
            if let Some(dialog) = self.dialog.as_mut() {
                dialog.pending_toolchain = Some(index);
            }
            return (SettingsOutcome::Idle, true);
        }
        if let Some(index) = command
            .0
            .strip_prefix("tool.select.")
            .and_then(|value| value.parse::<usize>().ok())
        {
            // Como na primeira escolha: fica pendente, e quem aplica é o Salvar.
            if let Some(dialog) = self.dialog.as_mut() {
                dialog.pending_secondary = Some(index);
            }
            return (SettingsOutcome::Idle, true);
        }
        match command.0.as_str() {
            "toolchain.browse" => (SettingsOutcome::Browse(ToolSlot::Primary), true),
            "tool.browse" => (SettingsOutcome::Browse(ToolSlot::Secondary), true),
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
        SettingsOutcome::Save {
            toolchain,
            secondary,
        }
    }

    /// Desenha a janela inteira, na ordem em que ela se sobrepõe.
    ///
    /// Os dois contextos vêm de fora porque o tema e a fonte são do shell; aqui
    /// se decide o que cada componente mostra e onde ele fica.
    pub(super) fn paint(
        &self,
        layout: &LayoutContext,
        size: Size,
        sections: &[SettingsSection],
        colors: ColorTokens,
        mut modal_paint: PaintContext,
        mut component_paint: PaintContext,
    ) -> Vec<PaintCommand> {
        if !self.modal.is_open() {
            return Vec::new();
        }
        let mut modal = self.modal.clone();
        modal.layout(layout, Rect::new(0.0, 0.0, size.width, size.height));
        let geometry = settings_dialog_geometry(modal.panel_bounds());
        modal.paint(&mut modal_paint);
        let mut commands = modal_paint.into_commands();
        commands.push(fill(geometry.sidebar, colors.surface));
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
                for (widget, rect) in [
                    (&self.debug_host, geometry.debug_host),
                    (&self.debug_port, geometry.debug_port),
                ] {
                    let mut widget = widget.clone();
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
                Point::new(geometry.combo.origin.x, geometry.combo.origin.y + 54.0),
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
            Point::new(geometry.combo.origin.x, geometry.combo.origin.y - 34.0),
            17.0,
            IconTint::Text,
        );
        text(
            layout,
            paint,
            CAPTION_ID,
            section.map_or("Toolchain", |section| section.field_caption.as_str()),
            Point::new(geometry.combo.origin.x, geometry.combo.origin.y - 16.0),
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
                geometry.secondary_combo.origin.y - 16.0,
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
        let mut commands = vec![
            label(
                "Depuração",
                Point::new(origin.x, origin.y - 34.0),
                colors.text,
                17.0,
            ),
            label(
                "Host e porta de depuração do processo em execução",
                Point::new(origin.x, origin.y - 16.0),
                colors.muted_text,
                13.0,
            ),
            label(
                "Inicie o servidor com -agentlib:jdwp=transport=dt_socket,server=y,suspend=n,address=*:8000",
                Point::new(origin.x, geometry.debug_attach.origin.y + 48.0),
                colors.muted_text,
                12.0,
            ),
            label(
                "Vale para qualquer processo depurado: servidor, container ou ferramenta.",
                Point::new(origin.x, geometry.debug_attach.origin.y + 66.0),
                colors.muted_text,
                12.0,
            ),
        ];
        for (rect, id) in [
            (geometry.debug_host, DEBUG_HOST_ID),
            (geometry.debug_port, DEBUG_PORT_ID),
        ] {
            if self.focus == Some(id) {
                commands.push(stroke(rect, colors.accent));
            }
        }
        commands
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
        pages.layout(layout, settings_pages_rect(geometry, section_count + 1));
        pages
    }

    /// As áreas da janela, já centralizada numa tela deste tamanho.
    pub(super) fn geometry(
        &mut self,
        context: &LayoutContext,
        size: Size,
    ) -> SettingsDialogGeometry {
        self.modal
            .layout(context, Rect::new(0.0, 0.0, size.width, size.height));
        settings_dialog_geometry(self.modal.panel_bounds())
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
