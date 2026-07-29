//! Estado e apresentação da busca por tipos e conteúdo.

use std::path::{Path, PathBuf};

use ide_application::NewItemTemplate;
use ide_domain::Location;
use ui_components::{Button, ModalHost, TextInput};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TypeSearchHit {
    pub name: String,
    pub kind: String,
    pub location: Location,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContentSearchHit {
    pub preview: String,
    pub location: Location,
}

impl ContentSearchHit {
    #[must_use]
    pub fn label(&self, source_root_names: &[String]) -> String {
        let path = search_display_path(&self.location.path, source_root_names);
        format!(
            "{}:{}  —  {}",
            path.display(),
            self.location.range.start.line + 1,
            self.preview
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum WorkspaceSearchMode {
    Types,
    Content,
}

/// Janela de criação enquanto está aberta.
pub(super) struct NewItemDialog {
    pub(super) template: NewItemTemplate,
    pub(super) source_root: PathBuf,
    pub(super) message: Option<String>,
    pub(super) naming: bool,
}

/// Estado das pesquisas e dos modais de criação.
pub(super) struct SearchState {
    pub(super) modal: ModalHost,
    pub(super) mode: WorkspaceSearchMode,
    pub(super) query: String,
    pub(super) type_results: Vec<TypeSearchHit>,
    pub(super) content_results: Vec<ContentSearchHit>,
    pub(super) selected: usize,
    pub(super) first_visible: usize,
    pub(super) new_item_modal: ModalHost,
    pub(super) new_item_dialog: Option<NewItemDialog>,
    pub(super) new_item_package: TextInput,
    pub(super) new_item_name: TextInput,
    pub(super) new_item_create_button: Button,
    pub(super) new_item_cancel_button: Button,
}

impl SearchState {
    #[must_use]
    pub(super) fn result_len(&self) -> usize {
        match self.mode {
            WorkspaceSearchMode::Types => self.type_results.len(),
            WorkspaceSearchMode::Content => self.content_results.len(),
        }
    }

    pub(super) fn reset(&mut self, mode: WorkspaceSearchMode) {
        self.mode = mode;
        self.query.clear();
        self.type_results.clear();
        self.content_results.clear();
        self.selected = 0;
        self.first_visible = 0;
    }
}

impl TypeSearchHit {
    #[must_use]
    pub fn label(&self, source_root_names: &[String]) -> String {
        let path = search_display_path(&self.location.path, source_root_names);
        format!("{} ({})  —  {}", self.name, self.kind, path.display())
    }
}

pub(super) fn search_display_path(path: &Path, source_root_names: &[String]) -> PathBuf {
    let components = path.components().collect::<Vec<_>>();
    let source_root = components.iter().rposition(|component| {
        matches!(
            component,
            std::path::Component::Normal(name)
                if source_root_names
                    .iter()
                    .any(|candidate| candidate.eq_ignore_ascii_case(&name.to_string_lossy()))
        )
    });
    if let Some(index) = source_root {
        let relative = components.iter().skip(index + 1).collect::<PathBuf>();
        if !relative.as_os_str().is_empty() {
            return relative;
        }
    }
    path.file_name().map_or_else(PathBuf::new, PathBuf::from)
}
