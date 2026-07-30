//! Onde cada parte das janelas do shell é desenhada.
//!
//! Contas puras sobre retângulos: recebem o painel e devolvem as áreas. Ficam
//! fora do shell porque não dependem de estado nenhum — e porque, junto do
//! resto, escondiam que a mesma conta aparecia na geometria e na pintura.

use ui_core::Rect;

use super::{INSPECTION_DETAIL_FRACTION, INSPECTION_LIST_FRACTION, SETTINGS_PAGE_ROW_HEIGHT};

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
pub(super) fn settings_dialog_geometry(dialog: Rect) -> SettingsDialogGeometry {
    let sidebar = Rect::new(
        dialog.origin.x,
        dialog.origin.y + 52.0,
        210.0,
        dialog.size.height - 52.0,
    );
    let compiler_option = Rect::new(sidebar.origin.x, sidebar.origin.y + 12.0, 210.0, 42.0);
    let combo = Rect::new(
        sidebar.origin.x + sidebar.size.width + 28.0,
        dialog.origin.y + 126.0,
        (dialog.size.width - sidebar.size.width - 178.0).max(190.0),
        36.0,
    );
    let browse = Rect::new(
        combo.origin.x + combo.size.width + 10.0,
        combo.origin.y,
        112.0,
        36.0,
    );
    // O Maven vem logo abaixo do JDK, com a mesma largura: são a mesma escolha
    // feita duas vezes, e alinhá-las é o que deixa isso evidente.
    let secondary_combo = Rect::new(
        combo.origin.x,
        combo.origin.y + combo.size.height + 46.0,
        combo.size.width,
        combo.size.height,
    );
    let secondary_browse = Rect::new(
        secondary_combo.origin.x + secondary_combo.size.width + 10.0,
        secondary_combo.origin.y,
        browse.size.width,
        browse.size.height,
    );
    // Salvar à direita, encostado na borda, e Cancelar à esquerda dele: a ação
    // que confirma fica no canto que a leitura alcança por último.
    let save = Rect::new(
        dialog.origin.x + dialog.size.width - 104.0,
        dialog.origin.y + dialog.size.height - 48.0,
        88.0,
        34.0,
    );
    let close = Rect::new(save.origin.x - 98.0, save.origin.y, 88.0, save.size.height);
    let debug_host = Rect::new(combo.origin.x, combo.origin.y, 220.0, 36.0);
    let debug_port = Rect::new(
        debug_host.origin.x + debug_host.size.width + 12.0,
        debug_host.origin.y,
        96.0,
        36.0,
    );
    let debug_attach = Rect::new(
        debug_host.origin.x,
        debug_host.origin.y + debug_host.size.height + 20.0,
        120.0,
        34.0,
    );
    SettingsDialogGeometry {
        sidebar,
        compiler_option,
        combo,
        secondary_combo,
        secondary_browse,
        browse,
        close,
        save,
        debug_host,
        debug_port,
        debug_attach,
    }
}
/// Área que a lista de páginas ocupa: as linhas, e não a barra inteira.
pub(super) fn settings_pages_rect(geometry: &SettingsDialogGeometry, page_count: usize) -> Rect {
    Rect::new(
        geometry.compiler_option.origin.x,
        geometry.compiler_option.origin.y,
        geometry.compiler_option.size.width,
        SETTINGS_PAGE_ROW_HEIGHT * page_count as f32,
    )
}
/// Os dois painéis da janela de inspeção e os botões.
pub(super) struct InspectionGeometry {
    pub(super) list: Rect,
    pub(super) detail: Rect,
    /// Editor de expressões, na parte de baixo do painel direito.
    pub(super) source: Rect,
    /// Linha da resposta da última execução, acima dos botões.
    pub(super) message: Rect,
    pub(super) run: Rect,
    pub(super) close: Rect,
}
pub(super) fn inspection_geometry(panel: Rect) -> InspectionGeometry {
    let top = panel.origin.y + 56.0;
    let height = (panel.size.height - 112.0).max(80.0);
    let list_width = (panel.size.width - 32.0) * INSPECTION_LIST_FRACTION;
    let list = Rect::new(panel.origin.x + 16.0, top, list_width, height);
    let right_x = list.origin.x + list.size.width + 16.0;
    let right_width = (panel.size.width - list_width - 48.0).max(80.0);
    // O detalhe fica em cima e o editor embaixo: o valor é o que se lê, o código
    // é o que se escreve.
    let detail_height = (height * INSPECTION_DETAIL_FRACTION).max(60.0);
    let detail = Rect::new(right_x, top, right_width, detail_height);
    let source = Rect::new(
        right_x,
        top + detail_height + 18.0,
        right_width,
        (height - detail_height - 18.0).max(60.0),
    );
    let close = Rect::new(
        panel.origin.x + panel.size.width - 104.0,
        panel.origin.y + panel.size.height - 48.0,
        88.0,
        34.0,
    );
    let run = Rect::new(close.origin.x - 108.0, close.origin.y, 98.0, 34.0);
    let message = Rect::new(
        panel.origin.x + 16.0,
        close.origin.y - 22.0,
        (panel.size.width - 32.0).max(80.0),
        18.0,
    );
    InspectionGeometry {
        list,
        detail,
        source,
        message,
        run,
        close,
    }
}
/// Onde cada peça da janela de criação fica dentro do painel.
pub(super) struct NewItemGeometry {
    pub(super) package: Rect,
    pub(super) name: Rect,
    pub(super) create: Rect,
    pub(super) cancel: Rect,
}
pub(super) fn new_item_geometry(panel: Rect) -> NewItemGeometry {
    let field_width = (panel.size.width - 48.0).max(120.0);
    let package = Rect::new(
        panel.origin.x + 24.0,
        panel.origin.y + 76.0,
        field_width,
        34.0,
    );
    let name = Rect::new(package.origin.x, package.origin.y + 64.0, field_width, 34.0);
    // Criar à direita, encostado na borda, como o Salvar das Configurações.
    let create = Rect::new(
        panel.origin.x + panel.size.width - 104.0,
        panel.origin.y + panel.size.height - 48.0,
        88.0,
        34.0,
    );
    let cancel = Rect::new(create.origin.x - 98.0, create.origin.y, 88.0, 34.0);
    NewItemGeometry {
        package,
        name,
        create,
        cancel,
    }
}