//! Construção das ações de menus do editor, Explorer e depuração.

use std::path::Path;

use ide_application::{DebugRequest, NewItemTemplate};
use ui_components::{MenuBar, MenuEntry, MenuItem};
use ui_core::CommandId;

use crate::explorer::{is_package, is_source_root};

/// Estado da barra de menus principal.
pub(super) struct MenuState {
    pub(super) bar: MenuBar,
}

pub(super) fn editor_entries(
    has_selection: bool,
    debugging: bool,
    inside_type: bool,
) -> Vec<MenuEntry> {
    let copy = MenuItem::new("Copiar", CommandId("editor.copy".to_owned()));
    let mut entries = vec![
        MenuEntry::Item(if has_selection { copy } else { copy.disabled() }),
        MenuEntry::Item(MenuItem::new("Colar", CommandId("editor.paste".to_owned()))),
    ];
    // Só dentro de um tipo: gerar construtor ou acessor fora de uma classe não
    // teria onde escrever, e oferecer a opção prometeria o que não se cumpre.
    if inside_type {
        entries.push(MenuEntry::Separator);
        entries.push(MenuEntry::submenu("Generate", vec![
            MenuEntry::Item(MenuItem::new(
                "Constructor",
                CommandId("editor.generate.constructor".to_owned()),
            )),
            MenuEntry::Item(MenuItem::new(
                "Getter",
                CommandId("editor.generate.getter".to_owned()),
            )),
            MenuEntry::Item(MenuItem::new(
                "Setter",
                CommandId("editor.generate.setter".to_owned()),
            )),
            MenuEntry::Item(MenuItem::new(
                "Getter and Setter",
                CommandId("editor.generate.accessors".to_owned()),
            )),
        ]));
    }
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
    if is_source_root(target, source_root_names) || is_package(target, source_root_names) {
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

#[cfg(test)]
mod generate_tests {
    use super::*;

    fn labels(entries: &[MenuEntry]) -> Vec<String> {
        entries
            .iter()
            .map(|entry| match entry {
                MenuEntry::Item(item) => item.label.clone(),
                MenuEntry::Submenu { label, .. } => label.clone(),
                MenuEntry::Separator => "—".to_owned(),
            })
            .collect()
    }

    /// Dentro de um tipo o menu oferece `Generate`; fora, não.
    ///
    /// Oferecer fora prometeria o que não se cumpre: não há onde escrever um
    /// construtor ou um acessor.
    #[test]
    fn generate_is_offered_only_inside_a_type() {
        let fora = editor_entries(false, false, false);
        assert_eq!(labels(&fora), vec!["Copiar", "Colar"]);

        let dentro = editor_entries(false, false, true);
        assert_eq!(labels(&dentro), vec!["Copiar", "Colar", "—", "Generate"]);

        let Some(MenuEntry::Submenu { entries, .. }) = dentro.last() else {
            panic!("`Generate` precisa ser um submenu, não um comando");
        };
        assert_eq!(labels(entries), vec![
            "Constructor",
            "Getter",
            "Setter",
            "Getter and Setter"
        ]);
    }

    /// Depurando e dentro de um tipo, as duas coisas convivem.
    #[test]
    fn generate_and_inspect_live_together() {
        let entries = editor_entries(true, true, true);
        let rotulos = labels(&entries);
        assert!(rotulos.contains(&"Generate".to_owned()));
        assert!(rotulos.contains(&"Inspecionar".to_owned()));
    }
}
