//! Estado de depuração apresentado pela interface.

use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};

use ui_components::{Button, ListView};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DebugFrameView {
    pub name: String,
    pub location: Option<(PathBuf, u32)>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DebugVariableView {
    pub name: String,
    pub value: String,
    pub type_name: Option<String>,
    pub expandable: bool,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DebugView {
    pub attached: bool,
    pub status: String,
    pub stopped_at: Option<(PathBuf, u32)>,
    pub frames: Vec<DebugFrameView>,
    pub selected_frame: usize,
    pub variables: Vec<DebugVariableView>,
}

impl DebugView {
    #[must_use]
    pub const fn is_stopped(&self) -> bool {
        self.stopped_at.is_some()
    }
}

/// Estado do painel de depuração.
pub(super) struct DebugPanelState {
    pub(super) stop_button: Button,
    pub(super) run_button: Button,
    pub(super) debug_button: Button,
    pub(super) breakpoints: BTreeMap<PathBuf, BTreeSet<u32>>,
    pub(super) verified_breakpoints: BTreeMap<PathBuf, BTreeSet<u32>>,
    pub(super) view: DebugView,
    /// Os cinco botões da faixa de execução.
    ///
    /// Vivem aqui, e não são desenhados a cada quadro: é o que faz cada um
    /// acender sob o ponteiro e afundar ao ser pressionado.
    pub(super) step_buttons: Vec<Button>,
    pub(super) frames: ListView,
    pub(super) variables: ListView,
}

impl DebugPanelState {
    #[must_use]
    pub(super) fn breakpoints_for(&self, path: &std::path::Path) -> Vec<u32> {
        self.breakpoints
            .get(path)
            .map_or_else(Vec::new, |lines| lines.iter().copied().collect())
    }

    #[must_use]
    pub(super) const fn attached(&self) -> bool {
        self.view.attached
    }
}
