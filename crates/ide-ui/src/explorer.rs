//! Identidade, compactação e projeção visual da árvore do Explorer.

use std::{
    collections::{HashMap, HashSet, hash_map::DefaultHasher},
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
};

use ide_domain::SymbolKind;
use ide_workspace::FileNode;
use ui_api::Widget;
use ui_components::{
    Badge, CellWidth, ComposedCell, ComposedRow, ComposedTreeItem, ComposedTreeView, ContextMenu,
    IconTint, Label, Scrollbar, Splitter,
};
use ui_core::WidgetId;

/// Estado completo do painel de arquivos.
///
/// A shell encaminha eventos e fornece geometrias; árvore, expansão, seleção,
/// rolagem e redimensionamento permanecem juntos nesta feature.
pub(super) struct ExplorerState {
    pub(super) workspace_name: String,
    pub(super) workspace: FileNode,
    pub(super) tree: ComposedTreeView,
    pub(super) context_menu: ContextMenu,
    pub(super) context_menu_target: Option<PathBuf>,
    /// Arquivo sob o clique, quando não foi uma pasta.
    ///
    /// `context_menu_target` guarda a **pasta**, porque é nela que a criação
    /// acontece; renomear fala do arquivo, e são caminhos diferentes.
    pub(super) context_menu_file: Option<PathBuf>,
    pub(super) expanded: HashSet<PathBuf>,
    /// Pastas cujos filhos já foram pedidos à aplicação.
    ///
    /// Sem isto, uma pasta **vazia no disco** pede leitura para sempre: ela tem
    /// a mesma forma de uma pasta não lida, e cada resposta faz a reconciliação
    /// da seleção pedir todas as outras de novo. Com quarenta pastas expandidas
    /// vira milhar de leituras por quadro, e a janela não chega a desenhar.
    pub(super) requested: HashSet<PathBuf>,
    pub(super) scroll_x: f32,
    pub(super) scroll_line: usize,
    pub(super) sidebar_width: f32,
    pub(super) splitter: Splitter,
    pub(super) vertical_scrollbar: Scrollbar,
    pub(super) horizontal_scrollbar: Scrollbar,
}

impl ExplorerState {
    #[must_use]
    pub(super) fn workspace_root(&self) -> &Path {
        &self.workspace.path
    }

    #[must_use]
    pub(super) const fn workspace_tree(&self) -> &FileNode {
        &self.workspace
    }

    /// Repõe as linhas da árvore a partir do que está em memória.
    ///
    /// Carregar uma pasta muda o `FileNode` e não a `TreeView`: sem repor os
    /// itens, a árvore continua desenhando a varredura anterior — o arquivo está
    /// lá dentro e não aparece.
    pub(super) fn rebuild_items(
        &mut self,
        source_root_names: &[String],
        kinds: &HashMap<u64, SymbolKind>,
    ) {
        self.tree
            .set_roots(items(&self.workspace, source_root_names, kinds));
    }

    pub(super) fn replace_workspace(
        &mut self,
        workspace: FileNode,
        source_root_names: &[String],
        kinds: &HashMap<u64, SymbolKind>,
    ) {
        self.workspace_name = workspace
            .path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("workspace")
            .to_owned();
        self.workspace = workspace;
        self.tree
            .set_roots(items(&self.workspace, source_root_names, kinds));
        self.expanded
            .retain(|path| path.starts_with(&self.workspace.path));
        self.expanded.insert(self.workspace.path.clone());
        // Outra árvore, outra leitura: recarregar o projeto é justamente o
        // pedido de ler tudo de novo.
        self.requested.clear();
        self.context_menu.close();
        self.context_menu_target = None;
        self.context_menu_file = None;
        self.scroll_x = 0.0;
        self.scroll_line = 0;
    }

    #[must_use]
    pub(super) fn is_expanded(&self, path: &Path) -> bool {
        self.expanded.contains(path)
    }
}

pub fn id(path: &Path) -> u64 {
    let mut hasher = DefaultHasher::new();
    path.hash(&mut hasher);
    hasher.finish()
}

fn label(node: &FileNode) -> &str {
    node.path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("?")
}

pub(super) fn is_source_root(path: &Path, source_root_names: &[String]) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    source_root_names
        .iter()
        .any(|candidate| candidate.eq_ignore_ascii_case(name))
}

pub(super) fn is_package(path: &Path, source_root_names: &[String]) -> bool {
    path.ancestors()
        .skip(1)
        .any(|ancestor| is_source_root(ancestor, source_root_names))
}

fn compact_package_chain<'a>(
    node: &'a FileNode,
    source_root_names: &[String],
) -> (&'a FileNode, String) {
    let mut label = label(node).to_owned();
    let mut current = node;
    while is_package(&current.path, source_root_names) {
        let [only_child] = current.children.as_slice() else {
            break;
        };
        if !only_child.is_directory {
            break;
        }
        label.push('.');
        label.push_str(self::label(only_child));
        current = only_child;
    }
    (current, label)
}

/// Espécie de um nó do Explorer, para o crachá que ele recebe.
///
/// **Neutra por construção.** Pacote sai do caminho, e o resto sai de
/// `SymbolKind` — que é do domínio, e não de linguagem nenhuma. Uma linguagem
/// nova que declare classes ganha o mesmo crachá sem tocar aqui.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Especie {
    Pacote,
    Classe,
    Interface,
    Enumeracao,
    /// Nada a dizer: pasta comum, arquivo que não declara tipo, ou tipo que o
    /// índice ainda não alcançou.
    Nenhuma,
}

impl Especie {
    /// A letra do crachá, e `None` quando ele é um quadrado.
    const fn letra(self) -> Option<&'static str> {
        match self {
            Self::Classe => Some("C"),
            Self::Interface => Some("I"),
            Self::Enumeracao => Some("E"),
            Self::Pacote | Self::Nenhuma => None,
        }
    }

    /// Qual papel do tema pinta o crachá.
    const fn tom(self) -> IconTint {
        match self {
            // O mesmo azul do tema — o de destaque, e não um azul escrito aqui.
            Self::Pacote | Self::Classe => IconTint::Accent,
            Self::Interface => IconTint::Warning,
            Self::Enumeracao => IconTint::Danger,
            Self::Nenhuma => IconTint::Muted,
        }
    }
}

fn especie(
    node: &FileNode,
    source_root_names: &[String],
    kinds: &HashMap<u64, SymbolKind>,
) -> Especie {
    if node.is_directory {
        return if is_package(&node.path, source_root_names) {
            Especie::Pacote
        } else {
            Especie::Nenhuma
        };
    }
    match kinds.get(&id(&node.path)) {
        Some(SymbolKind::Class | SymbolKind::Record) => Especie::Classe,
        Some(SymbolKind::Interface) => Especie::Interface,
        Some(SymbolKind::Enum) => Especie::Enumeracao,
        _ => Especie::Nenhuma,
    }
}

/// Identidade do crachá de um nó.
///
/// Derivada da identidade do nó, e **estável entre quadros**: é ela que faz o
/// componente sobreviver à releitura da árvore, e é o que a `ComposedTreeView`
/// exige de quem monta as células. A constante é a proporção áurea em 64 bits,
/// que espalha bits altos e baixos — somar um bastaria para colidir com o nó
/// vizinho, que na árvore está sempre por perto.
const fn cracha_id(no: u64) -> WidgetId {
    WidgetId(no ^ 0x9E37_79B9_7F4A_7C15)
}

const fn nome_id(no: u64) -> WidgetId {
    WidgetId(no ^ 0xC2B2_AE3D_27D4_EB4F)
}

/// Largura reservada ao crachá, para os nomes alinharem entre si.
const CRACHA: f32 = 16.0;
const FONTE: f32 = 14.0;

/// A árvore do Explorer **em nomes**, antes de virar componentes.
///
/// Existe para separar a regra da aparência: quem decide que uma cadeia de
/// pacotes vira uma linha só, e que espécie cada nó tem, não precisa de widget
/// nenhum — e é o que faz essa regra caber num teste que não constrói tela.
pub(super) struct NoDoExplorer {
    pub(super) id: u64,
    pub(super) label: String,
    pub(super) especie: Especie,
    pub(super) children: Vec<NoDoExplorer>,
}

pub(super) fn nomes(
    node: &FileNode,
    source_root_names: &[String],
    kinds: &HashMap<u64, SymbolKind>,
) -> Vec<NoDoExplorer> {
    node.children
        .iter()
        .map(|child| {
            let (node, label) = compact_package_chain(child, source_root_names);
            NoDoExplorer {
                id: id(&node.path),
                label,
                especie: especie(node, source_root_names, kinds),
                children: nomes(node, source_root_names, kinds),
            }
        })
        .collect()
}

pub(super) fn items(
    node: &FileNode,
    source_root_names: &[String],
    kinds: &HashMap<u64, SymbolKind>,
) -> Vec<ComposedTreeItem> {
    componentes(nomes(node, source_root_names, kinds))
}

fn componentes(nos: Vec<NoDoExplorer>) -> Vec<ComposedTreeItem> {
    nos.into_iter()
        .map(|no| {
            let NoDoExplorer {
                id: identidade,
                label,
                especie,
                children,
            } = no;
            let cracha: Box<dyn Widget> = match especie.letra() {
                Some(letra) => Box::new(
                    Label::new(cracha_id(identidade), letra)
                        .with_font_size(FONTE)
                        .with_tone(especie.tom()),
                ),
                None if especie == Especie::Pacote => {
                    Box::new(Badge::new(cracha_id(identidade), especie.tom()))
                }
                // Pasta comum e arquivo sem tipo não ganham marca, mas ganham a
                // coluna: sem ela o nome dançaria de linha para linha.
                None => Box::new(Label::new(cracha_id(identidade), "").with_font_size(FONTE)),
            };
            ComposedTreeItem::new(
                identidade,
                ComposedRow::new(vec![
                    ComposedCell::new(cracha, CellWidth::Fixed(CRACHA)),
                    // Natural, e não Fill: é o que deixa a linha ser mais larga
                    // que o painel, e é disso que a barra horizontal vive. Com
                    // Fill, o nome longo seria cortado na borda sem apelação.
                    ComposedCell::new(
                        Box::new(Label::new(nome_id(identidade), label).with_font_size(FONTE)),
                        CellWidth::Natural,
                    ),
                ]),
                componentes(children),
            )
        })
        .collect()
}

/// Em que linha visível um nó está, contando pela **árvore de arquivos**.
///
/// Conta pelo `FileNode`, e não pelos itens montados: montar os itens só para
/// achar uma linha construiria um componente por nó do projeto inteiro, e a
/// pergunta é sobre posição, não sobre aparência.
pub(super) fn visible_row(
    node: &FileNode,
    source_root_names: &[String],
    expanded: &HashSet<u64>,
    target: u64,
) -> Option<usize> {
    fn visit(
        node: &FileNode,
        source_root_names: &[String],
        expanded: &HashSet<u64>,
        target: u64,
        row: &mut usize,
    ) -> Option<usize> {
        for child in &node.children {
            let (child, _) = compact_package_chain(child, source_root_names);
            let identidade = id(&child.path);
            if identidade == target {
                return Some(*row);
            }
            *row += 1;
            if expanded.contains(&identidade)
                && let Some(found) = visit(child, source_root_names, expanded, target, row)
            {
                return Some(found);
            }
        }
        None
    }
    let mut row = 0;
    visit(node, source_root_names, expanded, target, &mut row)
}
