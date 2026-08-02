//! Bootstrap e decisões puras de composição da aplicação.

use std::path::PathBuf;

use ide_core::{AppConfig, ToolRole, init_logging};
use ide_project::model::{ProjectDescriptor, ProjectModel};
use ide_ui::NewItemRequest;
use language_java::GRADLE_BUILD_SYSTEM_ID;
use language_java::MAVEN_BUILD_SYSTEM_ID;
use winit::event_loop::{ControlFlow, EventLoop};

use super::NativeIde;

pub(super) fn startup_root(
    config: &AppConfig,
    current_directory: Option<PathBuf>,
) -> Option<PathBuf> {
    config.resolved_project().or(current_directory)
}

/// Traduz as escolhas do formato antigo, que tinha um campo por ferramenta.
///
/// Isto mora na raiz de composição, e não em `ide-core`, porque saber que
/// `jdk_home` era a ferramenta principal de Java é conhecimento de linguagem —
/// e o núcleo não pode tê-lo. A guarda de neutralidade obriga, e o desenho fica
/// melhor: quem sabe o que a chave significa é quem compõe as linguagens.
///
/// Vira **padrão global**, e não sobreposição de projeto: era uma escolha
/// global, e transformá-la na escolha de um projeto só a faria sumir dos
/// outros. Devolve `true` quando havia algo a migrar.
pub(super) fn migrate_legacy_toolchains(config: &mut AppConfig) -> bool {
    let legacy = config.toolchains.take_legacy();
    if legacy.is_empty() {
        return false;
    }
    let language = crate::java_contribution::JAVA_LANGUAGE_ID;
    for (key, home) in &legacy {
        let role = match key.as_str() {
            "jdk_home" => ToolRole::Primary,
            "maven_home" => ToolRole::Secondary,
            _ => {
                tracing::warn!(key, "escolha antiga de ferramenta sem tradução conhecida");
                continue;
            }
        };
        config.toolchains.choose(None, language, role, Some(home));
    }
    true
}

pub(super) fn default_goals(descriptor: &ProjectDescriptor) -> Vec<String> {
    match descriptor.build_system.0.as_str() {
        GRADLE_BUILD_SYSTEM_ID => vec!["classes".to_owned()],
        MAVEN_BUILD_SYSTEM_ID => vec!["compile".to_owned()],
        _ => vec!["build".to_owned()],
    }
}

pub(super) fn project_sources(files: Vec<PathBuf>, model: Option<&ProjectModel>) -> Vec<PathBuf> {
    let Some(model) = model else {
        return files;
    };
    let filtered = files
        .iter()
        .filter(|path| model.contains_source(path))
        .cloned()
        .collect::<Vec<_>>();
    if filtered.is_empty() { files } else { filtered }
}

pub(super) fn java_source(request: &NewItemRequest, name: &str) -> String {
    let declaration = if request.package.is_empty() {
        String::new()
    } else {
        format!("package {};\n\n", request.package)
    };
    let keyword = match request.template_id.as_str() {
        "java.interface" => "interface",
        _ => "class",
    };
    format!("{declaration}public {keyword} {name} {{\n}}\n")
}

pub(super) fn run() -> Result<(), Box<dyn std::error::Error>> {
    init_logging("info")?;
    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(ControlFlow::Wait);
    let mut app = NativeIde::default();
    event_loop.run_app(&mut app)?;
    if let Some(error) = app.take_startup_error() {
        return Err(error.into());
    }
    Ok(())
}
