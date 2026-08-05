mod angular_contribution;
mod bootstrap;
mod bridges;
mod controllers;
mod debug;
mod java_contribution;
mod markup_contribution;
mod native_ide;
mod run;
mod splash;
mod style_contribution;
mod typescript_contribution;
mod ui_bridge;
mod window;

pub(crate) use native_ide::NativeIde;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    bootstrap::run()
}
