use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use async_trait::async_trait;
use ide_application::{
    ContributionRegistry, LanguageContribution, LanguageDescriptor, NewItemTemplate,
    NewItemTemplateId, SettingsSection, TaskDescriptor, TaskId, TaskRegistry,
};
use ide_domain::{
    Diagnostic, DocumentChange, DocumentId, DocumentSnapshot, LanguageId, ProviderId,
};
use ide_language_api::{
    ActiveLanguage, LANGUAGE_API_VERSION, LanguageActivationContext, LanguageCapabilities,
    LanguageError, LanguageMetadata, LanguageProvider, ProviderState,
};
use ide_language_host::LanguageHost;

fn success<T, E: std::fmt::Debug>(result: Result<T, E>) -> T {
    match result {
        Ok(value) => value,
        Err(error) => panic!("operação deveria funcionar: {error:?}"),
    }
}

struct FakeProvider {
    activations: Arc<AtomicUsize>,
}

#[async_trait]
impl LanguageProvider for FakeProvider {
    fn metadata(&self) -> LanguageMetadata {
        LanguageMetadata {
            language_id: LanguageId("fake".to_owned()),
            provider_id: ProviderId("fake.builtin".to_owned()),
            display_name: "Fake".to_owned(),
            extensions: vec!["fake".to_owned()],
            api_version: LANGUAGE_API_VERSION,
            trigger_characters: vec![':'],
        }
    }

    fn capabilities(&self) -> LanguageCapabilities {
        LanguageCapabilities::SYNTAX
    }

    async fn activate(
        &self,
        _context: LanguageActivationContext,
    ) -> Result<Box<dyn ActiveLanguage>, LanguageError> {
        self.activations.fetch_add(1, Ordering::Relaxed);
        Ok(Box::new(FakeLanguage {
            language_id: LanguageId("fake".to_owned()),
        }))
    }
}

struct FakeLanguage {
    language_id: LanguageId,
}

#[async_trait]
impl ActiveLanguage for FakeLanguage {
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

#[test]
fn application_contracts_accept_a_fake_language_without_java_dependencies() {
    let activations = Arc::new(AtomicUsize::new(0));
    let host = LanguageHost::new(".");
    let mut contribution = LanguageContribution::new(
        LanguageDescriptor {
            language_id: LanguageId("fake".to_owned()),
            display_name: "Fake".to_owned(),
            extensions: vec!["fake".to_owned()],
        },
        Arc::new(FakeProvider {
            activations: Arc::clone(&activations),
        }),
    );
    contribution.new_item_templates.push(NewItemTemplate {
        id: NewItemTemplateId::new("fake.module"),
        title: "New fake module".to_owned(),
        name_caption: "Name".to_owned(),
        file_extension: Some("fake".to_owned()),
        allows_empty_name: false,
    });
    contribution.settings_sections.push(SettingsSection {
        id: "fake.runtime".to_owned(),
        title: "Fake runtime".to_owned(),
    });
    contribution.tasks.push(TaskDescriptor {
        id: TaskId("fake.run".to_owned()),
        title: "Run fake".to_owned(),
        requires_active_document: true,
    });

    let mut tasks = TaskRegistry::default();
    success(tasks.register_contribution(&contribution));
    let mut contributions = ContributionRegistry::default();
    success(contributions.register(contribution));
    let fake_id = LanguageId("fake".to_owned());
    let registered = match contributions.get(&fake_id) {
        Some(registered) => registered,
        None => panic!("a contribuição falsa deve estar indexada pela linguagem"),
    };
    success(host.register(registered.provider.clone()));

    assert_eq!(activations.load(Ordering::Relaxed), 0);
    assert!(matches!(
        host.providers().as_deref(),
        Ok([snapshot]) if snapshot.state == ProviderState::Registered
    ));
    assert_eq!(registered.new_item_templates.len(), 1);
    assert_eq!(registered.settings_sections.len(), 1);
    assert_eq!(tasks.len(), 1);

    let provider = success(pollster::block_on(host.open_document(
        host.request_context(),
        DocumentSnapshot {
            id: DocumentId(1),
            path: "sample.fake".into(),
            version: 1,
            text: "fake source".to_owned(),
        },
    )));

    assert_eq!(provider, ProviderId("fake.builtin".to_owned()));
    assert_eq!(activations.load(Ordering::Relaxed), 1);
    assert_eq!(host.trigger_characters(DocumentId(1)), vec![':']);
    assert!(matches!(
        host.providers().as_deref(),
        Ok([snapshot]) if snapshot.state == ProviderState::Active
    ));
}
