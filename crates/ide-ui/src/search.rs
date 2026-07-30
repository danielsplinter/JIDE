//! O que a busca por tipos e conteúdo encontra, e como cada acerto se lê.
//!
//! A janela em si mora em `ide_shell::type_search`; aqui ficam os tipos que
//! atravessam a fronteira com a aplicação.

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
