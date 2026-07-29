use std::{collections::HashMap, sync::Arc};

use ide_domain::{DocumentId, ProviderId};
use ide_language_api::{LanguageCapabilities, LanguageMetadata, LanguageProvider, ProviderState};

use super::{ProviderWorker, routing::Selections};

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
