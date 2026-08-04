//! Composição embutida das folhas de estilo.
//!
//! Sem toolchain, sem build, sem tarefa: uma folha de estilo não se compila nem
//! se executa por si. O que a IDE oferece é deixar de tratá-la como texto cru.

use std::sync::Arc;

use ide_application::{LanguageContribution, LanguageDescriptor};
use ide_domain::LanguageId;
use ide_language_api::LanguageProvider;
use language_style::{STYLE_LANGUAGE_ID, StyleLanguageProvider};

#[must_use]
pub fn language_id() -> LanguageId {
    LanguageId(STYLE_LANGUAGE_ID.to_owned())
}

#[must_use]
pub fn contribution() -> LanguageContribution {
    let provider: Arc<dyn LanguageProvider> = Arc::new(StyleLanguageProvider::new());
    LanguageContribution::new(
        LanguageDescriptor {
            language_id: language_id(),
            display_name: "Folhas de estilo".to_owned(),
            // Uma origem só: ver a nota em `java_contribution`.
            extensions: provider.metadata().extensions,
            // Uma folha de estilo não tem raiz própria: ela mora onde o código
            // que a usa mora, e quem declara isso é o projeto da linguagem.
            source_root_names: Vec::new(),
        },
        provider,
    )
}
