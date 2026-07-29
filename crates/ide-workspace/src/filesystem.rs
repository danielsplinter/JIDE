//! Adapter nativo de filesystem para a porta da aplicação.

use std::{fs, path::Path};

use ide_application::{WorkspaceEntry, WorkspacePort, WorkspacePortError};

#[derive(Clone, Copy, Debug, Default)]
pub struct NativeWorkspaceFileSystem;

impl WorkspacePort for NativeWorkspaceFileSystem {
    fn metadata(&self, path: &Path) -> Result<WorkspaceEntry, WorkspacePortError> {
        let metadata = fs::metadata(path)?;
        Ok(WorkspaceEntry {
            path: path.to_path_buf(),
            is_directory: metadata.is_dir(),
            modified: metadata.modified().ok(),
        })
    }

    fn read_directory(&self, path: &Path) -> Result<Vec<WorkspaceEntry>, WorkspacePortError> {
        let mut entries = fs::read_dir(path)?
            .map(|entry| {
                let entry = entry?;
                let metadata = entry.metadata()?;
                Ok(WorkspaceEntry {
                    path: entry.path(),
                    is_directory: metadata.is_dir(),
                    modified: metadata.modified().ok(),
                })
            })
            .collect::<Result<Vec<_>, std::io::Error>>()?;
        entries.sort_by(|left, right| left.path.file_name().cmp(&right.path.file_name()));
        Ok(entries)
    }

    fn read_text(&self, path: &Path) -> Result<String, WorkspacePortError> {
        Ok(fs::read_to_string(path)?)
    }

    fn write_text(&self, path: &Path, contents: &str) -> Result<(), WorkspacePortError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, contents)?;
        Ok(())
    }

    fn create_directory(&self, path: &Path) -> Result<(), WorkspacePortError> {
        fs::create_dir_all(path)?;
        Ok(())
    }

    fn exists(&self, path: &Path) -> bool {
        path.exists()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_path_is_reported_by_the_port() {
        let missing =
            std::env::temp_dir().join(format!("er-ide-workspace-missing-{}", std::process::id()));
        assert!(NativeWorkspaceFileSystem.metadata(&missing).is_err());
    }
}
