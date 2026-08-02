//! Composição embutida da linguagem TypeScript.
//!
//! Por enquanto só o provider nativo: realce, estrutura e erro de sintaxe. Sem
//! toolchain, sem build, sem depuração — eles chegam nas fases 2 a 4 da `23`, e
//! a contribuição cresce por aqui sem que nada acima precise saber.
//!
//! Note o que **não** é declarado: nenhuma seção de configurações, porque não há
//! ferramenta a escolher ainda, e nenhuma tarefa. A tela desenha o que existir.

use std::sync::Arc;

use ide_application::{LanguageContribution, LanguageDescriptor};
use ide_domain::LanguageId;
use ide_language_api::LanguageProvider;
use language_typescript::{TYPESCRIPT_LANGUAGE_ID, TypeScriptLanguageProvider};

#[must_use]
pub fn language_id() -> LanguageId {
    LanguageId(TYPESCRIPT_LANGUAGE_ID.to_owned())
}

#[must_use]
pub fn contribution() -> LanguageContribution {
    let provider: Arc<dyn LanguageProvider> = Arc::new(TypeScriptLanguageProvider::new());
    LanguageContribution::new(
        LanguageDescriptor {
            language_id: language_id(),
            display_name: "TypeScript".to_owned(),
            extensions: vec!["ts".to_owned()],
            // A raiz de fonte de um projeto TypeScript é declarada no
            // `tsconfig.json`, e lê-lo é a fase 2. Até lá, nenhuma convenção é
            // afirmada: dizer `src` por palpite seria a tabela de compatibilidade
            // que a `23` proíbe, com outro nome.
            source_root_names: Vec::new(),
        },
        provider,
    )
}
