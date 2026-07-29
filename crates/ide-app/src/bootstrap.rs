//! Bootstrap e decisões puras de composição da aplicação.

use std::path::PathBuf;

use ide_core::{AppConfig, init_logging};
use ide_domain::DocumentSnapshot;
use ide_project::model::{ProjectDescriptor, ProjectModel};
use ide_ui::{NewItemKind, NewItemRequest};
use java_gradle_adapter::GRADLE_BUILD_SYSTEM_ID;
use java_maven_adapter::MAVEN_BUILD_SYSTEM_ID;
use winit::event_loop::{ControlFlow, EventLoop};

use super::NativeIde;

pub(super) fn startup_root(
    config: &AppConfig,
    current_directory: Option<PathBuf>,
) -> Option<PathBuf> {
    config.resolved_project().or(current_directory)
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

pub(super) fn main_class_name(document: &DocumentSnapshot) -> Option<String> {
    if !document
        .path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("java"))
    {
        return None;
    }
    let class = document.path.file_stem()?.to_str()?;
    let package = document.text.lines().find_map(|line| {
        line.trim()
            .strip_prefix("package ")
            .and_then(|value| value.strip_suffix(';'))
            .map(str::trim)
    });
    Some(package.map_or_else(|| class.to_owned(), |package| format!("{package}.{class}")))
}

pub(super) fn java_source(request: &NewItemRequest, name: &str) -> String {
    let declaration = if request.package.is_empty() {
        String::new()
    } else {
        format!("package {};\n\n", request.package)
    };
    let keyword = match request.kind {
        NewItemKind::Interface => "interface",
        NewItemKind::Class | NewItemKind::Package => "class",
    };
    format!("{declaration}public {keyword} {name} {{\n}}\n")
}

pub(super) fn run() -> Result<(), Box<dyn std::error::Error>> {
    init_logging("info")?;
    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(ControlFlow::Wait);
    let mut app = NativeIde::default();
    event_loop.run_app(&mut app)?;
    if let Some(error) = app.startup_error {
        return Err(error.into());
    }
    Ok(())
}
