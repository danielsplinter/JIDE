use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use async_trait::async_trait;
use ide_domain::{
    Diagnostic, DocumentChange, DocumentId, DocumentSnapshot, LanguageId, ProviderId,
};
use ide_language_api::{
    ActiveLanguage, LANGUAGE_API_VERSION, LanguageActivationContext, LanguageCapabilities,
    LanguageContribution, LanguageError, LanguageMetadata, LanguageProvider,
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
    success(
        host.register_contribution(LanguageContribution::new(Arc::new(FakeProvider {
            activations: Arc::clone(&activations),
        }))),
    );

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
}
