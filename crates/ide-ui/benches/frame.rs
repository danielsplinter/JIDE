//! Quanto custa um quadro da IDE hoje, para a fase 0 da especificação `17` ter
//! com o que comparar.
//!
//! O arranjo ainda é calculado à mão e entregue por `place`; o motor de layout
//! só posiciona o que já veio pronto. Este número é a linha de base: se adotar o
//! motor custar mais do que a folga que existe aqui, a adoção precisa de
//! invalidação antes, e não depois.

use std::{
    path::PathBuf,
    time::{Duration, Instant},
};
use ide_ui::IdeShell;
use ide_workspace::FileNode;
use ui_core::Size;

/// Uma árvore de projeto com profundidade e volume de uma real.
fn workspace() -> FileNode {
    let packages = (0..12)
        .map(|package| FileNode {
            path: PathBuf::from(format!("/app/src/pacote{package}")),
            is_directory: true,
            children: (0..20)
                .map(|file| FileNode {
                    path: PathBuf::from(format!("/app/src/pacote{package}/Tipo{file}.java")),
                    is_directory: false,
                    children: Vec::new(),
                })
                .collect(),
        })
        .collect();
    FileNode {
        path: PathBuf::from("/app"),
        is_directory: true,
        children: packages,
    }
}

fn main() {
    const FRAMES: u32 = 100;
    let size = Size::new(1_600.0, 900.0);
    let mut shell = IdeShell::from_tree(workspace());

    // O primeiro quadro constrói o que os seguintes reaproveitam.
    let first_started = Instant::now();
    let commands = shell.paint(size).len();
    let first = first_started.elapsed();

    let started = Instant::now();
    for _ in 0..FRAMES {
        shell.paint(size);
    }
    let each = started.elapsed() / FRAMES;
    let share = each.as_secs_f64() / Duration::from_micros(16_667).as_secs_f64() * 100.0;
    println!("primeiro quadro: {first:?}, {commands} comandos");
    println!("quadro estável:  {each:?} — {share:.1}% do orçamento de 16,7 ms");
}
