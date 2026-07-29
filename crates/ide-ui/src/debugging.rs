//! Estado de depuração apresentado pela interface.

use std::{
    collections::{BTreeMap, BTreeSet, HashSet},
    path::PathBuf,
};

use ide_workspace::TextBuffer;
use ui_components::{Button, ListView, ModalHost, TreeView};

use crate::editor::EditorPane;

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

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) enum InspectionFocus {
    #[default]
    Tree,
    Source,
}

#[derive(Clone, Debug)]
pub(super) struct InspectionRun {
    pub(super) current: String,
    pub(super) remaining: Vec<String>,
    pub(super) position: usize,
    pub(super) total: usize,
}

#[derive(Clone, Debug)]
pub(super) struct InspectionNode {
    pub(super) path: String,
    pub(super) variable: DebugVariableView,
    pub(super) children: Vec<InspectionNode>,
    pub(super) loaded: bool,
}

impl InspectionNode {
    pub(super) fn new(path: String, variable: DebugVariableView) -> Self {
        Self {
            path,
            variable,
            children: Vec::new(),
            loaded: false,
        }
    }

    pub(super) fn find_mut(&mut self, path: &str) -> Option<&mut Self> {
        if self.path == path {
            return Some(self);
        }
        if !path.starts_with(&self.path) {
            return None;
        }
        self.children
            .iter_mut()
            .find_map(|child| child.find_mut(path))
    }

    pub(super) fn find(&self, path: &str) -> Option<&Self> {
        if self.path == path {
            return Some(self);
        }
        if !path.starts_with(&self.path) {
            return None;
        }
        self.children.iter().find_map(|child| child.find(path))
    }
}

pub(super) struct InspectionView {
    pub(super) expression: String,
    pub(super) root: InspectionNode,
    pub(super) expanded: HashSet<String>,
    pub(super) selected: String,
}

pub(super) struct InspectionState {
    pub(super) modal: ModalHost,
    pub(super) view: Option<InspectionView>,
    pub(super) tree: TreeView,
    pub(super) close_button: Button,
    pub(super) editor: EditorPane,
    pub(super) source: TextBuffer,
    pub(super) run_button: Button,
    pub(super) focus: InspectionFocus,
    pub(super) message: Option<String>,
    pub(super) run: Option<InspectionRun>,
}

/// Estado do painel de depuração e da inspeção suspensa.
pub(super) struct DebugPanelState {
    pub(super) inspection: InspectionState,
    pub(super) stop_button: Button,
    pub(super) run_button: Button,
    pub(super) debug_button: Button,
    pub(super) breakpoints: BTreeMap<PathBuf, BTreeSet<u32>>,
    pub(super) verified_breakpoints: BTreeMap<PathBuf, BTreeSet<u32>>,
    pub(super) view: DebugView,
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
