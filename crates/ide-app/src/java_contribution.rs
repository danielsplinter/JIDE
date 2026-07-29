//! Composição embutida da linguagem Java.

use std::sync::Arc;

use ide_application::{
    LanguageContribution, LanguageDescriptor, NewItemTemplate, NewItemTemplateId, SettingsSection,
    TaskDescriptor, TaskId,
};
use ide_debug_api::DebugAdapter;
use ide_domain::LanguageId;
use ide_language_api::LanguageProvider;
use ide_process::ProcessSupervisor;
use ide_project::build::BuildSystemRegistry;
use ide_toolchain_api::{CompilerAdapter, RuntimeAdapter, TestAdapter, ToolchainProvider};
use java_debug_adapter::JavaDebugAdapter;
use java_gradle_adapter::GradleAdapter;
use java_maven_adapter::MavenAdapter;
use java_toolchain::{JavaToolchainAdapter, JavaToolchainProvider};
use language_java::JavaLanguageProvider;

pub const JAVA_LANGUAGE_ID: &str = "java";

#[must_use]
pub fn language_id() -> LanguageId {
    LanguageId(JAVA_LANGUAGE_ID.to_owned())
}

#[must_use]
pub fn contribution(processes: Arc<dyn ProcessSupervisor>) -> LanguageContribution {
    let provider: Arc<dyn LanguageProvider> = Arc::new(JavaLanguageProvider::new());
    let toolchain: Arc<dyn ToolchainProvider> = Arc::new(JavaToolchainProvider::new());
    let adapter = Arc::new(JavaToolchainAdapter::new(processes));
    let compiler: Arc<dyn CompilerAdapter> = adapter.clone();
    let runtime: Arc<dyn RuntimeAdapter> = adapter.clone();
    let tests: Arc<dyn TestAdapter> = adapter;
    let debugger: Arc<dyn DebugAdapter> = Arc::new(JavaDebugAdapter::new());

    let mut contribution = LanguageContribution::new(
        LanguageDescriptor {
            language_id: language_id(),
            display_name: "Java".to_owned(),
            extensions: vec!["java".to_owned()],
        },
        provider,
    );
    contribution.toolchain = Some(toolchain);
    contribution.compiler = Some(compiler);
    contribution.runtime = Some(runtime);
    contribution.tests = Some(tests);
    contribution.debugger = Some(debugger);
    contribution.new_item_templates = vec![
        NewItemTemplate {
            id: NewItemTemplateId::new("java.package"),
            title: "Novo pacote".to_owned(),
            name_caption: "Classe (opcional)".to_owned(),
            file_extension: None,
            allows_empty_name: true,
        },
        NewItemTemplate {
            id: NewItemTemplateId::new("java.class"),
            title: "Nova classe".to_owned(),
            name_caption: "Nome da classe".to_owned(),
            file_extension: Some("java".to_owned()),
            allows_empty_name: false,
        },
        NewItemTemplate {
            id: NewItemTemplateId::new("java.interface"),
            title: "Nova interface".to_owned(),
            name_caption: "Nome da interface".to_owned(),
            file_extension: Some("java".to_owned()),
            allows_empty_name: false,
        },
    ];
    contribution.settings_sections = vec![SettingsSection {
        id: "java.compiler-vm".to_owned(),
        title: "Compilador e VM".to_owned(),
    }];
    contribution.tasks = vec![
        TaskDescriptor {
            id: TaskId("java.compile".to_owned()),
            title: "Compilar".to_owned(),
            requires_active_document: false,
        },
        TaskDescriptor {
            id: TaskId("java.run".to_owned()),
            title: "Executar".to_owned(),
            requires_active_document: true,
        },
        TaskDescriptor {
            id: TaskId("java.test".to_owned()),
            title: "Testar".to_owned(),
            requires_active_document: true,
        },
    ];
    contribution
}

pub fn register_build_systems(
    registry: &mut BuildSystemRegistry,
    processes: Arc<dyn ProcessSupervisor>,
) {
    registry.register(Arc::new(MavenAdapter::new(processes.clone())));
    registry.register(Arc::new(GradleAdapter::new(processes)));
}

#[cfg(test)]
mod tests {
    use ide_process::NativeProcessSupervisor;

    use super::*;

    #[test]
    fn java_is_described_as_data_in_one_composition_unit() {
        let contribution = contribution(Arc::new(NativeProcessSupervisor::default()));
        assert_eq!(contribution.descriptor.language_id, language_id());
        assert_eq!(contribution.descriptor.extensions, ["java"]);
        assert!(contribution.toolchain.is_some());
        assert!(contribution.compiler.is_some());
        assert!(contribution.runtime.is_some());
        assert!(contribution.tests.is_some());
        assert!(contribution.debugger.is_some());
        assert_eq!(contribution.new_item_templates.len(), 3);
        assert_eq!(contribution.settings_sections.len(), 1);
        assert_eq!(contribution.tasks.len(), 3);
    }
}
