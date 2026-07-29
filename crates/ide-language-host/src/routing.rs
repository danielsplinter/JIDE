use std::path::Path;

use ide_domain::{LanguageId, ProviderId};
use ide_language_api::LanguageRequestContext;

use super::LanguageHostError;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderSelection {
    pub primary: ProviderId,
    pub fallbacks: Vec<ProviderId>,
}

pub(super) fn normalize_extension(extension: &str) -> String {
    extension
        .trim()
        .trim_start_matches('.')
        .to_ascii_lowercase()
}

pub(super) fn document_extension(path: &Path) -> String {
    path.extension()
        .and_then(|extension| extension.to_str())
        .map(normalize_extension)
        .unwrap_or_default()
}

pub(super) fn ensure_not_cancelled(
    context: &LanguageRequestContext,
) -> Result<(), LanguageHostError> {
    if context.cancellation.is_cancelled() {
        Err(LanguageHostError::Cancelled)
    } else {
        Ok(())
    }
}

pub(super) type Selections = std::collections::HashMap<LanguageId, ProviderSelection>;
