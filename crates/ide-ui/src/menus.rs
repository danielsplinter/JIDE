//! Construção das ações de menus do editor, Explorer e depuração.

use std::path::Path;

use ide_application::{DebugRequest, NewItemTemplate, RecentProject};
use ui_components::{MenuBar, MenuBarItem, MenuEntry, MenuItem};
use ui_core::CommandId;

use crate::explorer::{is_package, is_source_root};

/// Estado da barra de menus principal.
pub(super) struct MenuState {
    pub(super) bar: MenuBar,
    /// Projetos oferecidos em "Recentes", na ordem em que aparecem.
    ///
    /// Guardados aqui porque o clique chega como posição — o menu diz "o
    /// terceiro" — e alguém precisa dizer qual caminho é esse. A barra é
    /// reconstruída junto com esta lista, e as duas nunca se separam.
    ///
    /// A posição é a **desta lista**, e não a da linha no menu: o agrupamento
    /// por linguagem reordena o que se vê, e contar as linhas abriria outro
    /// projeto.
    pub(super) recents: Vec<RecentProject>,
}

/// O prefixo dos comandos de projeto recente; o resto é a posição na lista.
pub(super) const RECENTE: &str = "file.recent.";

/// O menu "Arquivo", com a porta de recentes já montada.
///
/// Existe como função porque o menu é construído em dois lugares — na abertura
/// e a cada catálogo novo — e um menu que muda só num deles é um menu que some.
pub(super) fn file_menu(recents: &[RecentProject]) -> MenuBarItem {
    MenuBarItem::menu(
        "Arquivo",
        vec![
            MenuItem::new("Projeto...", "file.project"),
            MenuItem::submenu("Recentes", recent_entries(recents)),
            MenuItem::new("Duplicar workspace", "file.duplicate"),
            MenuItem::new("Salvar", "file.save"),
        ],
    )
}

/// Os recentes agrupados por linguagem, cada grupo atrás de uma porta.
///
/// **A posição no comando é a posição na lista que a aplicação enviou**, e não
/// a ordem em que os itens aparecem no menu: o agrupamento reordena, e um índice
/// contado aqui abriria o projeto de outra linha.
///
/// Os grupos saem na ordem em que as linguagens aparecem — a mais usada
/// recentemente primeiro —, e dentro de cada um a ordem de uso se mantém. O que
/// a IDE não reconheceu vai para "Outras", **sempre no fim**: um grupo que
/// aparecesse no meio faria as linguagens conhecidas mudarem de lugar por causa
/// de uma pasta qualquer aberta no meio do caminho.
fn recent_entries(recents: &[RecentProject]) -> Vec<MenuItem> {
    let rotulos = rotulos_dos_recentes(recents);
    let mut grupos: Vec<(String, Vec<MenuItem>)> = Vec::new();
    let mut outras = Vec::new();
    for (posicao, recente) in recents.iter().enumerate() {
        let Some(rotulo) = rotulos.get(posicao) else {
            continue;
        };
        let item = MenuItem::new(rotulo.clone(), CommandId(format!("{RECENTE}{posicao}")));
        let Some(linguagem) = recente.language.as_ref() else {
            outras.push(item);
            continue;
        };
        match grupos.iter_mut().find(|(nome, _)| nome == linguagem) {
            Some((_, itens)) => itens.push(item),
            None => grupos.push((linguagem.clone(), vec![item])),
        }
    }
    if !outras.is_empty() {
        grupos.push((OUTRAS.to_owned(), outras));
    }
    grupos
        .into_iter()
        .map(|(linguagem, itens)| MenuItem::submenu(linguagem, itens))
        .collect()
}

/// O grupo de quem a IDE não soube dizer a linguagem.
///
/// O nome não afirma linguagem nenhuma — é justamente o que se sabe daqueles
/// projetos. Uma pasta sem manifesto reconhecido continua sendo um recente
/// legítimo, e precisa de um lugar como as outras.
const OUTRAS: &str = "Outras";

/// Como cada projeto recente se apresenta na lista.
///
/// O nome da pasta basta quase sempre e é o que a pessoa reconhece. **Quando
/// dois recentes têm o mesmo nome** — e é justamente entre esses que a lista
/// mais serve, `frontend` de um cliente e `frontend` de outro — o nome sozinho
/// mentiria; então a pasta que os contém entra junto para separá-los.
fn rotulos_dos_recentes(recents: &[RecentProject]) -> Vec<String> {
    let nome = |caminho: &Path| {
        caminho
            .file_name()
            .map_or_else(|| caminho.display().to_string(), |n| n.to_string_lossy().into_owned())
    };
    recents
        .iter()
        .map(|recente| {
            let caminho = &recente.path;
            let proprio = nome(caminho);
            let repetido = recents
                .iter()
                .map(|outro| &outro.path)
                .filter(|outro| outro.as_path() != caminho.as_path())
                .any(|outro| nome(outro) == proprio);
            match caminho.parent().filter(|_| repetido) {
                Some(pai) => format!("{proprio} — {}", nome(pai)),
                None => proprio,
            }
        })
        .collect()
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
        entries.push(MenuEntry::submenu(
            "Generate",
            vec![
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
            ],
        ));
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
    is_file: bool,
) -> Vec<MenuEntry> {
    // Renomear é de arquivo: uma pasta não tem tipo dentro dela nem referências
    // por nome, e prometer o mesmo gesto para as duas coisas seria mentira.
    let renomear = MenuEntry::Item(MenuItem::new(
        "Renomear",
        CommandId("explorer.rename".to_owned()),
    ));
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
        if is_file {
            entries.push(MenuEntry::Separator);
            entries.push(renomear);
        }
        return entries;
    }
    if is_file {
        return vec![renomear];
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
mod recentes_tests {
    use super::*;

    fn recente(caminho: &str, linguagem: Option<&str>) -> RecentProject {
        RecentProject {
            path: std::path::PathBuf::from(caminho),
            language: linguagem.map(ToOwned::to_owned),
        }
    }

    /// O nome da pasta basta; nomes repetidos ganham a pasta que os contém.
    #[test]
    fn dois_recentes_de_mesmo_nome_se_distinguem() {
        let rotulos = rotulos_dos_recentes(&[
            recente("/casa/loja/frontend", None),
            recente("/casa/banco/frontend", None),
            recente("/casa/relatorios", None),
        ]);
        assert_eq!(
            rotulos,
            vec!["frontend — loja", "frontend — banco", "relatorios"],
            "só o nome repetido precisa do pai"
        );
    }

    /// Cada linguagem é uma porta, e o projeto mora dentro dela.
    ///
    /// O comando leva a posição **na lista enviada**, e não a linha do menu: o
    /// agrupamento reordena o que se vê, e contar as linhas abriria outro
    /// projeto. É o que este teste fixa — `loja` é o terceiro da lista e
    /// aparece na primeira linha do segundo grupo.
    #[test]
    fn os_recentes_se_agrupam_por_linguagem() {
        let entradas = recent_entries(&[
            recente("/casa/faturamento", Some("Java")),
            recente("/casa/cadastro", Some("Java")),
            recente("/casa/loja", Some("TypeScript")),
            recente("/casa/rascunho", None),
        ]);

        let rotulos: Vec<_> = entradas.iter().map(|item| item.label.clone()).collect();
        assert_eq!(
            rotulos,
            vec!["Java", "TypeScript", "Outras"],
            "uma porta por linguagem, na ordem de uso, e o desconhecido no fim"
        );
        assert_eq!(
            entradas[2].children()[0].label,
            "rascunho",
            "a pasta sem linguagem reconhecida mora em Outras"
        );
        let java: Vec<_> = entradas[0]
            .children()
            .iter()
            .map(|item| item.label.clone())
            .collect();
        assert_eq!(java, vec!["faturamento", "cadastro"]);
        assert_eq!(
            entradas[1].children()[0].command,
            CommandId(format!("{RECENTE}2")),
            "a posição é a da lista enviada, não a linha do menu"
        );
    }

    /// Uma porta sem projetos fica desabilitada em vez de sumir.
    #[test]
    fn sem_recentes_a_porta_continua_no_menu() {
        let arquivo = file_menu(&[]);
        let Some(recentes) = arquivo
            .children
            .iter()
            .find(|item| item.label == "Recentes")
        else {
            panic!("o menu Arquivo deveria ter a porta de recentes");
        };
        assert!(recentes.children().is_empty());
        assert!(!recentes.enabled);
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
        assert_eq!(
            labels(entries),
            vec!["Constructor", "Getter", "Setter", "Getter and Setter"]
        );
    }

    /// Renomear é do arquivo: aparece sobre um, e não sobre uma pasta.
    #[test]
    fn rename_is_offered_for_files_and_not_for_folders() {
        let templates = Vec::new();
        let raizes = Vec::new();

        let pasta = labels(&explorer_entries(
            Path::new("projeto/src"),
            &raizes,
            &templates,
            false,
        ));
        assert!(
            !pasta.iter().any(|item| item == "Renomear"),
            "pasta não tem tipo dentro nem referências por nome: {pasta:?}"
        );

        let arquivo = labels(&explorer_entries(
            Path::new("projeto/src"),
            &raizes,
            &templates,
            true,
        ));
        assert!(
            arquivo.iter().any(|item| item == "Renomear"),
            "sobre um arquivo a opção aparece: {arquivo:?}"
        );
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
