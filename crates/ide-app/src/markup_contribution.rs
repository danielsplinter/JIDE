//! Composição embutida da marcação.
//!
//! Sem toolchain, sem build, sem tarefa: um `.html` não se compila nem se
//! executa por si. O que a IDE oferece é deixar de tratá-lo como texto cru.
//!
//! # Por que ele convive com o analisador, e não disputa com ele
//!
//! Num projeto Angular, o mesmo `.html` é atendido por **dois** providers: este,
//! que dá realce e estrutura, e o analisador de TypeScript, que responde por
//! tipo dentro do template pelo plugin da ADR-029. Eles não competem — cada um
//! anuncia as capacidades que tem, e o host compõe. É a `04` aplicada a um
//! arquivo que pertence a dois assuntos.

use std::sync::Arc;

use ide_application::{LanguageContribution, LanguageDescriptor};
use ide_domain::LanguageId;
use ide_language_api::LanguageProvider;
use language_markup::{MARKUP_LANGUAGE_ID, MarkupLanguageProvider};

#[must_use]
pub fn language_id() -> LanguageId {
    LanguageId(MARKUP_LANGUAGE_ID.to_owned())
}

#[must_use]
pub fn contribution() -> LanguageContribution {
    let provider: Arc<dyn LanguageProvider> = Arc::new(MarkupLanguageProvider::new());
    LanguageContribution::new(
        LanguageDescriptor {
            language_id: language_id(),
            display_name: "Marcação".to_owned(),
            // Uma origem só: ver a nota em `java_contribution`.
            extensions: provider.metadata().extensions,
            // Um documento de marcação não tem raiz própria: ele mora onde o
            // código que o usa mora, e quem declara isso é o projeto da
            // linguagem.
            source_root_names: Vec::new(),
            // Esta linguagem nao reconhece pasta nenhuma sozinha.
            build_systems: Vec::new(),
        },
        provider,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// O descritor e o provider precisam concordar sobre o que atendem.
    ///
    /// É a mesma invariante que a contribuição de TypeScript passou a cobrar
    /// depois do defeito do template: o descritor é o portão, e um provider que
    /// responde por algo que o descritor não reclama nunca é consultado.
    #[test]
    fn o_descritor_e_o_provider_concordam() {
        let contribuicao = contribution();
        assert_eq!(
            contribuicao.descriptor.extensions,
            contribuicao.provider.metadata().extensions
        );
    }
}
