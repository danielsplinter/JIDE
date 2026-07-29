//! Estado e apresentação da busca por tipos e conteúdo.

use std::path::{Path, PathBuf};

use ide_domain::Location;

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
    pub fn label(&self) -> String {
        let path = type_search_display_path(&self.location.path);
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

impl TypeSearchHit {
    #[must_use]
    pub fn label(&self) -> String {
        let path = type_search_display_path(&self.location.path);
        format!("{} ({})  —  {}", self.name, self.kind, path.display())
    }
}

pub(super) fn type_search_display_path(path: &Path) -> PathBuf {
    let components = path.components().collect::<Vec<_>>();
    let java = components.iter().rposition(|component| {
        matches!(
            component,
            std::path::Component::Normal(name)
                if name.to_string_lossy().eq_ignore_ascii_case("java")
        )
    });
    if let Some(index) = java {
        let relative = components.iter().skip(index + 1).collect::<PathBuf>();
        if !relative.as_os_str().is_empty() {
            return relative;
        }
    }
    path.file_name().map_or_else(PathBuf::new, PathBuf::from)
}
