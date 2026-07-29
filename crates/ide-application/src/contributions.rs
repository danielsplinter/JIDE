//! Catálogo de capacidades fornecidas por linguagens.

use std::{collections::BTreeMap, sync::Arc};

use ide_debug_api::DebugAdapter;
use ide_domain::LanguageId;
use ide_language_api::LanguageProvider;
use ide_toolchain_api::{
    CompilerAdapter, RuntimeAdapter, TestAdapter, ToolchainError, ToolchainId,
    ToolchainInstallation, ToolchainProvider,
};
use thiserror::Error;

use crate::NewItemTemplateId;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LanguageDescriptor {
    pub language_id: LanguageId,
    pub display_name: String,
    pub extensions: Vec<String>,
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct TaskId(pub String);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TaskDescriptor {
    pub id: TaskId,
    pub title: String,
    pub requires_active_document: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NewItemTemplate {
    pub id: NewItemTemplateId,
    pub title: String,
    pub name_caption: String,
    pub file_extension: Option<String>,
    pub allows_empty_name: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SettingsSection {
    pub id: String,
    pub title: String,
}

#[derive(Clone)]
pub struct LanguageContribution {
    pub descriptor: LanguageDescriptor,
    pub provider: Arc<dyn LanguageProvider>,
    pub toolchain: Option<Arc<dyn ToolchainProvider>>,
    pub compiler: Option<Arc<dyn CompilerAdapter>>,
    pub runtime: Option<Arc<dyn RuntimeAdapter>>,
    pub tests: Option<Arc<dyn TestAdapter>>,
    pub debugger: Option<Arc<dyn DebugAdapter>>,
    pub new_item_templates: Vec<NewItemTemplate>,
    pub settings_sections: Vec<SettingsSection>,
    pub tasks: Vec<TaskDescriptor>,
}

impl LanguageContribution {
    #[must_use]
    pub fn new(descriptor: LanguageDescriptor, provider: Arc<dyn LanguageProvider>) -> Self {
        Self {
            descriptor,
            provider,
            toolchain: None,
            compiler: None,
            runtime: None,
            tests: None,
            debugger: None,
            new_item_templates: Vec::new(),
            settings_sections: Vec::new(),
            tasks: Vec::new(),
        }
    }
}

#[derive(Default)]
pub struct ContributionRegistry {
    contributions: BTreeMap<LanguageId, LanguageContribution>,
}

impl ContributionRegistry {
    pub fn register(
        &mut self,
        contribution: LanguageContribution,
    ) -> Result<(), ContributionError> {
        validate_contribution(&contribution)?;
        let language_id = contribution.descriptor.language_id.clone();
        if self.contributions.contains_key(&language_id) {
            return Err(ContributionError::DuplicateLanguage(language_id));
        }
        self.contributions.insert(language_id, contribution);
        Ok(())
    }

    #[must_use]
    pub fn get(&self, language_id: &LanguageId) -> Option<&LanguageContribution> {
        self.contributions.get(language_id)
    }

    pub fn iter(&self) -> impl Iterator<Item = &LanguageContribution> {
        self.contributions.values()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.contributions.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.contributions.is_empty()
    }
}

fn validate_contribution(contribution: &LanguageContribution) -> Result<(), ContributionError> {
    let language_id = &contribution.descriptor.language_id;
    let metadata = contribution.provider.metadata();
    if &metadata.language_id != language_id {
        return Err(ContributionError::ProviderLanguageMismatch {
            expected: language_id.clone(),
            actual: metadata.language_id,
        });
    }
    for adapter_language in [
        contribution
            .compiler
            .as_ref()
            .map(|adapter| adapter.supported_language()),
        contribution
            .runtime
            .as_ref()
            .map(|adapter| adapter.supported_language()),
        contribution
            .tests
            .as_ref()
            .map(|adapter| adapter.supported_language()),
        contribution
            .debugger
            .as_ref()
            .map(|adapter| adapter.supported_language()),
    ]
    .into_iter()
    .flatten()
    {
        if &adapter_language != language_id {
            return Err(ContributionError::AdapterLanguageMismatch {
                expected: language_id.clone(),
                actual: adapter_language,
            });
        }
    }
    if let Some(toolchain) = &contribution.toolchain
        && !toolchain.supported_languages().contains(language_id)
    {
        return Err(ContributionError::UnsupportedToolchainLanguage(
            language_id.clone(),
        ));
    }
    Ok(())
}

#[derive(Clone, Debug, Default)]
pub struct ToolchainSelection {
    installations: Vec<ToolchainInstallation>,
    selected: Option<ToolchainId>,
}

impl ToolchainSelection {
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
}

#[derive(Default)]
pub struct ToolchainRegistry {
    selections: BTreeMap<LanguageId, ToolchainSelection>,
}

impl ToolchainRegistry {
    pub fn set_installations(
        &mut self,
        language_id: LanguageId,
        installations: Vec<ToolchainInstallation>,
    ) {
        self.selections
            .insert(language_id, ToolchainSelection::new(installations));
    }

    #[must_use]
    pub fn selection(&self, language_id: &LanguageId) -> Option<&ToolchainSelection> {
        self.selections.get(language_id)
    }

    pub fn selection_mut(&mut self, language_id: &LanguageId) -> Option<&mut ToolchainSelection> {
        self.selections.get_mut(language_id)
    }

    pub fn ensure_selection(&mut self, language_id: LanguageId) -> &mut ToolchainSelection {
        self.selections.entry(language_id).or_default()
    }
}

#[derive(Default)]
pub struct TaskRegistry {
    tasks: BTreeMap<TaskId, (LanguageId, TaskDescriptor)>,
}

impl TaskRegistry {
    pub fn register_contribution(
        &mut self,
        contribution: &LanguageContribution,
    ) -> Result<(), ContributionError> {
        for task in &contribution.tasks {
            if self.tasks.contains_key(&task.id) {
                return Err(ContributionError::DuplicateTask(task.id.clone()));
            }
        }
        for task in &contribution.tasks {
            self.tasks.insert(
                task.id.clone(),
                (contribution.descriptor.language_id.clone(), task.clone()),
            );
        }
        Ok(())
    }

    #[must_use]
    pub fn get(&self, id: &TaskId) -> Option<(&LanguageId, &TaskDescriptor)> {
        self.tasks
            .get(id)
            .map(|(language_id, descriptor)| (language_id, descriptor))
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.tasks.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.tasks.is_empty()
    }
}

#[derive(Debug, Error, Eq, PartialEq)]
pub enum ContributionError {
    #[error("language contribution is already registered: {0:?}")]
    DuplicateLanguage(LanguageId),
    #[error("task is already registered: {0:?}")]
    DuplicateTask(TaskId),
    #[error("provider language mismatch: expected {expected:?}, got {actual:?}")]
    ProviderLanguageMismatch {
        expected: LanguageId,
        actual: LanguageId,
    },
    #[error("adapter language mismatch: expected {expected:?}, got {actual:?}")]
    AdapterLanguageMismatch {
        expected: LanguageId,
        actual: LanguageId,
    },
    #[error("toolchain does not support language {0:?}")]
    UnsupportedToolchainLanguage(LanguageId),
}

#[cfg(test)]
mod tests {
    use async_trait::async_trait;
    use ide_domain::{Diagnostic, DocumentChange, DocumentId, DocumentSnapshot, ProviderId};
    use ide_language_api::{
        ActiveLanguage, LANGUAGE_API_VERSION, LanguageActivationContext, LanguageCapabilities,
        LanguageError, LanguageMetadata,
    };

    use super::*;

    struct FakeProvider {
        language_id: LanguageId,
    }

    #[async_trait]
    impl LanguageProvider for FakeProvider {
        fn metadata(&self) -> LanguageMetadata {
            LanguageMetadata {
                language_id: self.language_id.clone(),
                provider_id: ProviderId(format!("{}.test", self.language_id.0)),
                display_name: self.language_id.0.clone(),
                extensions: vec![self.language_id.0.clone()],
                api_version: LANGUAGE_API_VERSION,
                trigger_characters: Vec::new(),
            }
        }

        fn capabilities(&self) -> LanguageCapabilities {
            LanguageCapabilities::empty()
        }

        async fn activate(
            &self,
            _context: LanguageActivationContext,
        ) -> Result<Box<dyn ActiveLanguage>, LanguageError> {
            Ok(Box::new(FakeActive {
                language_id: self.language_id.clone(),
            }))
        }
    }

    struct FakeActive {
        language_id: LanguageId,
    }

    #[async_trait]
    impl ActiveLanguage for FakeActive {
        fn language_id(&self) -> &LanguageId {
            &self.language_id
        }

        async fn open_document(&self, _document: DocumentSnapshot) -> Result<(), LanguageError> {
            Ok(())
        }

        async fn change_document(&self, _change: DocumentChange) -> Result<(), LanguageError> {
            Ok(())
        }

        async fn close_document(&self, _document_id: DocumentId) -> Result<(), LanguageError> {
            Ok(())
        }

        async fn diagnostics(
            &self,
            _document_id: DocumentId,
        ) -> Result<Vec<Diagnostic>, LanguageError> {
            Ok(Vec::new())
        }

        async fn shutdown(&self) -> Result<(), LanguageError> {
            Ok(())
        }
    }

    fn contribution(language: &str) -> LanguageContribution {
        let language_id = LanguageId(language.to_owned());
        let mut contribution = LanguageContribution::new(
            LanguageDescriptor {
                language_id: language_id.clone(),
                display_name: language.to_owned(),
                extensions: vec![language.to_owned()],
            },
            Arc::new(FakeProvider { language_id }),
        );
        contribution.tasks.push(TaskDescriptor {
            id: TaskId(format!("{language}.run")),
            title: format!("Run {language}"),
            requires_active_document: true,
        });
        contribution
    }

    #[test]
    fn registry_is_indexed_by_language_and_rejects_duplicates() {
        let mut registry = ContributionRegistry::default();
        assert!(registry.register(contribution("fake")).is_ok());
        assert_eq!(registry.len(), 1);
        assert_eq!(
            registry
                .get(&LanguageId("fake".to_owned()))
                .map(|entry| entry.descriptor.display_name.as_str()),
            Some("fake")
        );
        assert_eq!(
            registry.register(contribution("fake")),
            Err(ContributionError::DuplicateLanguage(LanguageId(
                "fake".to_owned()
            )))
        );
    }

    #[test]
    fn tasks_are_registered_from_contribution_data() {
        let contribution = contribution("fake");
        let mut tasks = TaskRegistry::default();
        assert!(tasks.register_contribution(&contribution).is_ok());
        assert_eq!(tasks.len(), 1);
        assert_eq!(
            tasks
                .get(&TaskId("fake.run".to_owned()))
                .map(|(language, task)| (language.0.clone(), task.title.clone())),
            Some(("fake".to_owned(), "Run fake".to_owned()))
        );
    }
}
