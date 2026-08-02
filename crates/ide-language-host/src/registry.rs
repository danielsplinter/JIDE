use std::{collections::HashMap, sync::Arc};

use ide_domain::{DocumentId, LanguageId, ProviderId};
use ide_language_api::{
    LANGUAGE_API_VERSION, LanguageCapabilities, LanguageMetadata, LanguageProvider, ProviderState,
};

use crate::{
    host::{LanguageHostError, ProviderSnapshot},
    routing::{ProviderSelection, Selections, normalize_extension},
    worker::ProviderWorker,
};

pub(super) struct ProviderEntry {
    pub(super) provider: Arc<dyn LanguageProvider>,
    pub(super) metadata: LanguageMetadata,
    pub(super) capabilities: LanguageCapabilities,
    pub(super) state: ProviderState,
    pub(super) worker: Option<Arc<ProviderWorker>>,
    pub(super) last_error: Option<String>,
}

#[derive(Default)]
pub(super) struct Registry {
    pub(super) providers: HashMap<ProviderId, ProviderEntry>,
    pub(super) selections: Selections,
    pub(super) document_routes: HashMap<DocumentId, ProviderId>,
}

impl Registry {
    pub(super) fn register(
        &mut self,
        provider: Arc<dyn LanguageProvider>,
    ) -> Result<(), LanguageHostError> {
        let mut metadata = provider.metadata();
        validate_metadata(&metadata)?;
        metadata.extensions = metadata
            .extensions
            .iter()
            .map(|extension| normalize_extension(extension))
            .collect();
        metadata.extensions.sort();
        metadata.extensions.dedup();
        if self.providers.contains_key(&metadata.provider_id) {
            return Err(LanguageHostError::DuplicateProvider(metadata.provider_id));
        }
        self.providers.insert(
            metadata.provider_id.clone(),
            ProviderEntry {
                capabilities: provider.capabilities(),
                provider,
                metadata,
                state: ProviderState::Registered,
                worker: None,
                last_error: None,
            },
        );
        Ok(())
    }

    pub(super) fn configure_selection(
        &mut self,
        language_id: LanguageId,
        selection: ProviderSelection,
    ) -> Result<(), LanguageHostError> {
        for provider_id in std::iter::once(&selection.primary).chain(&selection.fallbacks) {
            let entry = self
                .providers
                .get(provider_id)
                .ok_or_else(|| LanguageHostError::ProviderNotFound(provider_id.clone()))?;
            if entry.metadata.language_id != language_id {
                return Err(LanguageHostError::InvalidMetadata(format!(
                    "provider {} belongs to language {}, not {}",
                    provider_id.0, entry.metadata.language_id.0, language_id.0
                )));
            }
        }
        self.selections.insert(language_id, selection);
        Ok(())
    }

    pub(super) fn snapshots(&self) -> Vec<ProviderSnapshot> {
        let mut providers = self
            .providers
            .values()
            .map(|entry| ProviderSnapshot {
                metadata: entry.metadata.clone(),
                capabilities: entry.capabilities,
                state: entry.state,
                last_error: entry.last_error.clone(),
            })
            .collect::<Vec<_>>();
        providers.sort_by(|left, right| left.metadata.provider_id.cmp(&right.metadata.provider_id));
        providers
    }

    pub(super) fn candidate_ids(
        &self,
        extension: &str,
        required: LanguageCapabilities,
    ) -> Result<Vec<ProviderId>, LanguageHostError> {
        let extension = normalize_extension(extension);
        let mut matching = self
            .providers
            .iter()
            .filter(|(_, entry)| {
                // `Failed` sai da lista junto com `Disabled`. Um provider que
                // disse ter deixado de existir não pode ser escolhido de novo na
                // reabertura seguinte — senão o documento voltaria para o morto,
                // e a queda da fase 3b não teria acontecido.
                //
                // Voltar é por `enable`, que já existe: um provider falho é
                // reabilitado de propósito, e não por acaso.
                !matches!(entry.state, ProviderState::Disabled | ProviderState::Failed)
                    && entry.metadata.extensions.contains(&extension)
                    && entry.capabilities.contains(required)
            })
            .map(|(id, entry)| (id.clone(), entry.metadata.language_id.clone()))
            .collect::<Vec<_>>();
        matching.sort_by(|left, right| left.0.cmp(&right.0));
        if matching.is_empty() {
            return Err(LanguageHostError::ProviderUnavailable {
                extension,
                capabilities: required,
            });
        }
        let language_id = matching[0].1.clone();
        let mut ordered = Vec::new();
        if let Some(selection) = self.selections.get(&language_id) {
            for id in std::iter::once(&selection.primary).chain(&selection.fallbacks) {
                if matching.iter().any(|candidate| &candidate.0 == id) {
                    ordered.push(id.clone());
                }
            }
        }
        for (id, _) in matching {
            if !ordered.contains(&id) {
                ordered.push(id);
            }
        }
        Ok(ordered)
    }

    pub(super) fn trigger_characters(&self, document_id: DocumentId) -> Vec<char> {
        self.document_routes
            .get(&document_id)
            .and_then(|provider_id| self.providers.get(provider_id))
            .map(|entry| entry.metadata.trigger_characters.clone())
            .unwrap_or_default()
    }

    pub(super) fn route(&mut self, document_id: DocumentId, provider_id: ProviderId) {
        self.document_routes.insert(document_id, provider_id);
    }

    pub(super) fn unroute(&mut self, document_id: DocumentId) -> Option<ProviderId> {
        self.document_routes.remove(&document_id)
    }

    pub(super) fn provider_for_document(
        &self,
        document_id: DocumentId,
    ) -> Result<ProviderId, LanguageHostError> {
        self.document_routes
            .get(&document_id)
            .cloned()
            .ok_or(LanguageHostError::DocumentNotRouted(document_id))
    }

    pub(super) fn remove_provider_routes(&mut self, provider_id: &ProviderId) {
        self.document_routes
            .retain(|_, routed_provider| routed_provider != provider_id);
    }
}

fn validate_metadata(metadata: &LanguageMetadata) -> Result<(), LanguageHostError> {
    if metadata.api_version.major != LANGUAGE_API_VERSION.major {
        return Err(LanguageHostError::IncompatibleApi {
            expected_major: LANGUAGE_API_VERSION.major,
            expected_minor: LANGUAGE_API_VERSION.minor,
            actual_major: metadata.api_version.major,
            actual_minor: metadata.api_version.minor,
        });
    }
    if metadata.provider_id.0.trim().is_empty()
        || metadata.language_id.0.trim().is_empty()
        || metadata.display_name.trim().is_empty()
        || metadata.extensions.is_empty()
    {
        return Err(LanguageHostError::InvalidMetadata(
            "ids, display name and extensions are required".to_owned(),
        ));
    }
    Ok(())
}
