//! Construção do classpath Java a partir do workspace e do modelo importado.

use std::{
    fs,
    path::{Path, PathBuf},
};

use ide_toolchain_api::Classpath;

#[derive(Clone, Debug, Default)]
pub struct ClasspathBuilder {
    entries: Vec<PathBuf>,
}

impl ClasspathBuilder {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn with_entry(mut self, entry: impl Into<PathBuf>) -> Self {
        let entry = entry.into();
        if entry.exists() && !self.entries.contains(&entry) {
            self.entries.push(entry);
        }
        self
    }

    #[must_use]
    pub fn workspace_defaults(mut self, root: &Path, output_directory: &Path) -> Self {
        self = self.with_entry(output_directory);
        for directory in [
            root.join("lib"),
            root.join("libs"),
            root.join("target").join("classes"),
            root.join("build").join("classes").join("java").join("main"),
        ] {
            self = self.with_entry(&directory);
            if let Ok(entries) = fs::read_dir(directory) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path
                        .extension()
                        .and_then(|extension| extension.to_str())
                        .is_some_and(|extension| extension.eq_ignore_ascii_case("jar"))
                    {
                        self = self.with_entry(path);
                    }
                }
            }
        }
        self
    }

    #[must_use]
    pub fn build(self) -> Classpath {
        Classpath {
            entries: self.entries,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::env;

    use super::*;

    #[test]
    fn classpath_deduplicates_existing_entries() {
        let root = env::temp_dir().join(format!("er-ide-classpath-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        assert!(fs::create_dir_all(&root).is_ok());
        let classpath = ClasspathBuilder::new()
            .with_entry(&root)
            .with_entry(&root)
            .build();
        assert_eq!(classpath.entries, vec![root.clone()]);
        let _ = fs::remove_dir_all(root);
    }
}
