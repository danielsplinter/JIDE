//! Onde cada parte das janelas do shell é desenhada.
//!
//! Contas puras sobre retângulos: recebem o painel e devolvem as áreas. Ficam
//! fora do shell porque não dependem de estado nenhum — e porque, junto do
//! resto, escondiam que a mesma conta aparecia na geometria e na pintura.

use ui_core::Rect;

use super::settings::PAGE_ROW_HEIGHT as SETTINGS_PAGE_ROW_HEIGHT;

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
/// As áreas de dentro de uma página das Configurações.
///
/// A moldura — painel, barra lateral e fileira de ações — vem do arranjo, e por
/// isso entra aqui pronta. O que sobra é o interior de cada página, que ainda é
/// conta, e é o que a próxima etapa da `17` converte.
pub(super) fn settings_dialog_geometry(
    dialog: Rect,
    sidebar: Rect,
    pages: Rect,
    close: Rect,
    save: Rect,
    debug: (Rect, Rect, Rect),
) -> SettingsDialogGeometry {
    let (debug_host, debug_port, debug_attach) = debug;
    // A primeira linha da navegação é o alto da lista, que o arranjo posiciona:
    // ler daqui é o que impede a lista ser pintada num lugar e acertada noutro.
    let compiler_option = Rect::new(pages.origin.x, pages.origin.y, 210.0, 42.0);
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
impl SettingsDialogGeometry {
    /// Área que a lista de páginas ocupa: as linhas, e não a barra inteira.
    pub(super) fn compiler_option_row(&self, page_count: usize) -> Rect {
        Rect::new(
            self.compiler_option.origin.x,
            self.compiler_option.origin.y,
            self.compiler_option.size.width,
            SETTINGS_PAGE_ROW_HEIGHT * page_count as f32,
        )
    }
}
