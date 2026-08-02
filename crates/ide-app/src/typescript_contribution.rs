//! Composição embutida da linguagem TypeScript.
//!
//! O provider nativo mais o sistema de build de npm. Sem toolchain de Node e sem
//! depuração ainda — eles chegam nas fases seguintes da `23`, e a contribuição
//! cresce por aqui sem que nada acima precise saber.
//!
//! Note o que **não** é declarado: nenhuma seção de configurações, porque não há
//! ferramenta a escolher ainda, e nenhuma tarefa. A tela desenha o que existir.

use std::sync::Arc;

use ide_application::{LanguageContribution, LanguageDescriptor};
use ide_domain::LanguageId;
use ide_language_api::LanguageProvider;
use ide_process::ProcessSupervisor;
use ide_project::build::BuildSystemRegistry;
use language_typescript::{NpmAdapter, TYPESCRIPT_LANGUAGE_ID, TypeScriptLanguageProvider};

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
            // Vazio de propósito, e não por falta: a raiz de um projeto
            // TypeScript é declarada no `tsconfig.json`, e é de lá que o
            // `ProjectModel` a lê (ADR-027). Um nome de convenção aqui seria uma
            // segunda origem para a mesma pergunta.
            source_root_names: Vec::new(),
        },
        provider,
    )
}

/// Registra o sistema de build de npm na ordem de detecção.
///
/// O `package.json` diz que existe um projeto; o `tsconfig.json` diz do que ele
/// é feito. Um projeto pode ter o primeiro sem TypeScript nenhum, e reconhecê-lo
/// assim é resposta certa.
pub fn register_build_systems(
    registry: &mut BuildSystemRegistry,
    processes: Arc<dyn ProcessSupervisor>,
) {
    registry.register(Arc::new(NpmAdapter::new(processes)));
}
