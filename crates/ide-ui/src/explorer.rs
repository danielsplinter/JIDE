//! Identidade, compactação e projeção visual da árvore do Explorer.

use std::{
    collections::{HashMap, HashSet, hash_map::DefaultHasher},
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
};

use ide_domain::{DocumentId, SymbolKind};
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
    /// A aba sobre a qual o menu foi aberto, quando foi sobre uma.
    ///
    /// Guardada como o alvo do Explorer ao lado: o menu devolve um comando, e
    /// quem o executa precisa saber de qual aba ele falava.
    pub(super) context_menu_tab: Option<DocumentId>,
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
/// **Neutra por construção.** Pasta sai da árvore de arquivos; tipo de arquivo
/// sai da extensão; e classe, interface e enumeração saem de `SymbolKind` — que
/// é do domínio, e não de linguagem nenhuma. Uma linguagem nova que declare
/// classes ganha o mesmo crachá sem tocar aqui.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Especie {
    /// Qualquer pasta. **Sempre**, em qualquer projeto.
    Pasta,
    Classe,
    Interface,
    Enumeracao,
    TypeScript,
    /// Arquivo de teste: `.spec.ts`.
    Teste,
    Marcacao,
    FolhaSass,
    FolhaEstilo,
    /// Nada a dizer: arquivo que não declara tipo, extensão desconhecida, ou
    /// tipo que o índice ainda não alcançou.
    Nenhuma,
}

impl Especie {
    /// O texto do crachá, e `None` quando ele é um quadrado.
    const fn letra(self) -> Option<&'static str> {
        match self {
            Self::Classe => Some("C"),
            Self::Interface => Some("I"),
            Self::Enumeracao => Some("E"),
            Self::TypeScript => Some("TS"),
            Self::Teste => Some("IT"),
            Self::Marcacao => Some("</>"),
            Self::FolhaSass => Some("SC"),
            Self::FolhaEstilo => Some("CS"),
            Self::Pasta | Self::Nenhuma => None,
        }
    }

    /// Qual papel do tema pinta o crachá.
    ///
    /// Nenhuma cor está escrita aqui: são papéis, e quem os resolve é o tema —
    /// inclusive o de alto contraste, que ninguém precisa lembrar de atualizar.
    const fn tom(self) -> IconTint {
        match self {
            // O mesmo azul do tema — o de destaque, e não um azul escrito aqui.
            Self::Pasta | Self::Classe | Self::FolhaEstilo => IconTint::Accent,
            Self::Interface | Self::Marcacao => IconTint::Warning,
            Self::Enumeracao | Self::Teste => IconTint::Danger,
            Self::TypeScript => IconTint::Success,
            Self::FolhaSass => IconTint::Pink,
            Self::Nenhuma => IconTint::Muted,
        }
    }
}

/// A espécie que a **extensão** revela, sem perguntar a ninguém.
///
/// Vem antes do índice de propósito, e por duas razões. A primeira é que ela
/// não custa nada e não espera: aparece no primeiro quadro, enquanto o índice
/// ainda está subindo. A segunda é que ela é mais específica — `.spec.ts` diz
/// mais do que "este arquivo declara uma classe".
///
/// A ordem entre elas também é por especificidade: `.spec.ts` antes de `.ts`,
/// senão o teste nunca seria visto como teste.
fn especie_da_extensao(path: &Path) -> Option<Especie> {
    let nome = path.file_name()?.to_str()?.to_ascii_lowercase();
    if nome.ends_with(".spec.ts") {
        return Some(Especie::Teste);
    }
    let extensao = path.extension()?.to_str()?.to_ascii_lowercase();
    match extensao.as_str() {
        "ts" => Some(Especie::TypeScript),
        "html" | "htm" => Some(Especie::Marcacao),
        "scss" | "sass" => Some(Especie::FolhaSass),
        "css" => Some(Especie::FolhaEstilo),
        _ => None,
    }
}

fn especie(node: &FileNode, kinds: &HashMap<u64, SymbolKind>) -> Especie {
    // **Toda pasta ganha a marca**, e não só a que está sob uma raiz de fontes.
    // Antes o quadrado dependia de a linguagem declarar raízes — e num projeto
    // sem elas, como um Angular, pasta nenhuma era marcada. A árvore fica
    // ilegível quando pasta e arquivo se parecem, e isso não depende do tipo do
    // projeto.
    if node.is_directory {
        return Especie::Pasta;
    }
    if let Some(especie) = especie_da_extensao(&node.path) {
        return especie;
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
///
/// Cabe o mais largo — `</>`, de três caracteres —, e por isso ela é maior do
/// que uma letra pede: coluna que muda de largura conforme o crachá faria os
/// nomes dançarem de linha para linha, e é justamente o alinhamento que torna
/// uma árvore legível.
const CRACHA: f32 = 26.0;
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
                especie: especie(node, kinds),
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
                None if especie == Especie::Pasta => {
                    Box::new(Badge::new(cracha_id(identidade), especie.tom()))
                }
                // Arquivo de extensão desconhecida não ganha marca, mas ganha a
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
