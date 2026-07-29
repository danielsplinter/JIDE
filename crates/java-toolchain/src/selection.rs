//! Seleção explícita de uma instalação Java detectada.

use ide_toolchain_api::{ToolchainError, ToolchainId, ToolchainInstallation};

#[derive(Clone, Debug, Default)]
pub struct JavaToolchainSelection {
    installations: Vec<ToolchainInstallation>,
    selected: Option<ToolchainId>,
}

impl JavaToolchainSelection {
    #[must_use]
    pub fn new(installations: Vec<ToolchainInstallation>) -> Self {
        let selected = installations
            .first()
            .map(|installation| installation.id.clone());
        Self {
            installations,
            selected,
        }
    }

    pub fn select(&mut self, id: &ToolchainId) -> Result<(), ToolchainError> {
        if !self
            .installations
            .iter()
            .any(|installation| &installation.id == id)
        {
            return Err(ToolchainError::NotFound);
        }
        self.selected = Some(id.clone());
        Ok(())
    }

    /// Registra a instalação sem escolhê-la e devolve seu índice.
    pub fn add(&mut self, installation: ToolchainInstallation) -> usize {
        if let Some(index) = self
            .installations
            .iter()
            .position(|existing| existing.id == installation.id)
        {
            self.installations[index] = installation;
            return index;
        }
        self.installations.push(installation);
        self.installations.len().saturating_sub(1)
    }

    pub fn add_and_select(&mut self, installation: ToolchainInstallation) {
        let index = self.add(installation);
        self.selected = self
            .installations
            .get(index)
            .map(|installation| installation.id.clone());
    }

    #[must_use]
    pub fn selected(&self) -> Option<&ToolchainInstallation> {
        let selected = self.selected.as_ref()?;
        self.installations
            .iter()
            .find(|installation| &installation.id == selected)
    }

    #[must_use]
    pub fn installations(&self) -> &[ToolchainInstallation] {
        &self.installations
    }

    pub fn select_next(&mut self) -> Option<&ToolchainInstallation> {
        if self.installations.is_empty() {
            self.selected = None;
            return None;
        }
        let current = self.selected.as_ref().and_then(|selected| {
            self.installations
                .iter()
                .position(|installation| &installation.id == selected)
        });
        let next = current.map_or(0, |index| (index + 1) % self.installations.len());
        self.selected = Some(self.installations[next].id.clone());
        self.installations.get(next)
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn installation(id: &str) -> ToolchainInstallation {
        ToolchainInstallation {
            id: ToolchainId(id.to_owned()),
            home: PathBuf::from(id),
            version: None,
        }
    }

    #[test]
    fn explicit_selection_and_rotation_are_stable() {
        let first = installation("jdk-8");
        let second = installation("jdk-17");
        let mut selection = JavaToolchainSelection::new(vec![first.clone(), second.clone()]);
        assert_eq!(selection.selected().map(|jdk| &jdk.id), Some(&first.id));
        assert!(selection.select(&second.id).is_ok());
        assert_eq!(selection.selected().map(|jdk| &jdk.id), Some(&second.id));
        assert_eq!(selection.select_next().map(|jdk| &jdk.id), Some(&first.id));
    }

    #[test]
    fn adding_the_same_installation_replaces_it() {
        let mut selection = JavaToolchainSelection::default();
        let mut first = installation("jdk");
        first.version = Some("17".to_owned());
        selection.add_and_select(first);
        let mut updated = installation("jdk");
        updated.version = Some("21".to_owned());
        assert_eq!(selection.add(updated), 0);
        assert_eq!(
            selection.selected().and_then(|jdk| jdk.version.as_deref()),
            Some("21")
        );
    }
}
