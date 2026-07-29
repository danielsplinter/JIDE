//! Bootstrap e decisões puras de composição da aplicação.

use std::path::PathBuf;

use ide_core::{AppConfig, init_logging};
use ide_project::model::{ProjectDescriptor, ProjectModel};
use ide_ui::NewItemRequest;
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
    if let Some(error) = app.startup_error {
        return Err(error.into());
    }
    Ok(())
}
