//! Com a feature `producao`, a janela sobe sem o console atrás dela; sem ela —
//! o padrão —, o console fica, e é nele que o `tracing` aparece.
#![cfg_attr(feature = "producao", windows_subsystem = "windows")]

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
