//! Construção das ações de menus do editor, Explorer e depuração.

use std::path::Path;

use ide_application::DebugRequest;
use ui_components::{MenuEntry, MenuItem};
use ui_core::CommandId;

pub(super) fn editor_entries(has_selection: bool, debugging: bool) -> Vec<MenuEntry> {
    let copy = MenuItem::new("Copiar", CommandId("editor.copy".to_owned()));
    let mut entries = vec![
        MenuEntry::Item(if has_selection { copy } else { copy.disabled() }),
        MenuEntry::Item(MenuItem::new("Colar", CommandId("editor.paste".to_owned()))),
    ];
    if debugging {
        entries.push(MenuEntry::Separator);
        let inspect = MenuItem::new("Inspecionar", CommandId("debug.inspect".to_owned()));
        entries.push(MenuEntry::Item(if has_selection {
            inspect
        } else {
            inspect.disabled()
        }));
    }
    entries
}

pub(super) fn explorer_entries(target: &Path) -> Vec<MenuEntry> {
    if super::is_java_source_root(target) || super::is_java_package(target) {
        return vec![
            MenuEntry::Item(MenuItem::new(
                "Novo pacote",
                CommandId("explorer.new.package".to_owned()),
            )),
            MenuEntry::Separator,
            MenuEntry::Item(MenuItem::new(
                "Nova classe",
                CommandId("explorer.new.class".to_owned()),
            )),
            MenuEntry::Item(MenuItem::new(
                "Nova interface",
                CommandId("explorer.new.interface".to_owned()),
            )),
        ];
    }
    vec![MenuEntry::Item(MenuItem::new(
        "Nova pasta",
        CommandId("explorer.new.folder".to_owned()),
    ))]
}

pub(super) fn debug_request(command: &str) -> Option<DebugRequest> {
    match command {
        "debug.continue" => Some(DebugRequest::Continue),
        "debug.pause" => Some(DebugRequest::Pause),
        "debug.over" => Some(DebugRequest::StepOver),
        "debug.into" => Some(DebugRequest::StepInto),
        "debug.out" => Some(DebugRequest::StepOut),
        "debug.detach" => Some(DebugRequest::Detach),
        _ => None,
    }
}
