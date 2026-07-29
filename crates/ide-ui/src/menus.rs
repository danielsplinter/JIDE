//! Construção das ações de menus do editor, Explorer e depuração.

use std::path::Path;

use ide_application::{DebugRequest, NewItemTemplate};
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

pub(super) fn explorer_entries(
    target: &Path,
    source_root_names: &[String],
    templates: &[NewItemTemplate],
) -> Vec<MenuEntry> {
    if super::is_source_root(target, source_root_names)
        || super::is_package(target, source_root_names)
    {
        let mut entries = Vec::new();
        for (index, template) in templates.iter().enumerate() {
            entries.push(MenuEntry::Item(MenuItem::new(
                template.title.clone(),
                CommandId(format!("explorer.new.{}", template.id.as_str())),
            )));
            if template.file_extension.is_none() && index + 1 < templates.len() {
                entries.push(MenuEntry::Separator);
            }
        }
        return entries;
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
