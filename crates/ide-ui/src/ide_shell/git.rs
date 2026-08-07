//! O gerenciador de Git: a janela que o terceiro botão da barra abre.
//!
//! # O que existe aqui, e o que não existe
//!
//! A **fase 0** da `22`: a janela, a divisão arrastável, a árvore com os quatro
//! nós e a busca acima dela. Só leitura, e só o nó `branches` com conteúdo — os
//! outros três aparecem vazios de propósito. Um nó que só existe depois que a
//! capacidade chega faria a tela mudar de forma a cada fase; um nó vazio diz o
//! que a IDE ainda não sabe fazer, e é honesto.
//!
//! # Esta tela não fala com o Git
//!
//! Ela recebe [`GitView`] pronto e desenha o que recebeu — como a barra de
//! estado faz com a memória, e o Explorer com os crachás. Quem pergunta ao
//! repositório é a aplicação, na thread dela; o shell só guarda a resposta.
//!
//! É por isso que os tipos daqui são da tela e não os do `ide-git`: a fronteira
//! entre uma coisa e outra é o que impede esta janela de crescer para dentro do
//! domínio, e o custo dela são três campos de `String`.

use ui_api::{EventContext, LayoutContext, PaintContext, Widget};
use ui_components::{
    Button, CellWidth, ComposedCell, ComposedList, ComposedRow, ComposedTreeItem, ComposedTreeView,
    IconTint, Label, ModalHost, SplitOrientation, SplitPane, TabItem, Tabs, TextInput,
};
use ui_core::{Point, Rect, ScrollEvent, Size, UiEvent, WidgetId};
use ui_host::UiHost;
use ui_layout_api::{EdgeInsets, LayoutDirection, LayoutStyle};

use ide_application::{ApplicationCommand, GitRequest};

use super::primary_pointer;

const MODAL_ID: WidgetId = WidgetId(10_500);
const BODY_ID: WidgetId = WidgetId(10_501);
const SIDE_ID: WidgetId = WidgetId(10_502);
const SEARCH_ID: WidgetId = WidgetId(10_503);
const TREE_ID: WidgetId = WidgetId(10_504);
const WORK_ID: WidgetId = WidgetId(10_505);
const TABS_ID: WidgetId = WidgetId(10_506);
const CONTENT_ID: WidgetId = WidgetId(10_507);
const SPLIT_ID: WidgetId = WidgetId(10_508);
/// As linhas da árvore, uma célula por identidade.
const ROW_BASE: WidgetId = WidgetId(10_520);
/// O resumo do lado direito, uma linha por estado.
const SUMMARY_BASE: WidgetId = WidgetId(10_512);
/// As duas divisas que empilham os três painéis da aba `status`.
const SPLIT_ALTO_ID: WidgetId = WidgetId(10_513);
const SPLIT_BAIXO_ID: WidgetId = WidgetId(10_514);
/// As três listas, e os títulos delas.
const LISTA_BASE: WidgetId = WidgetId(10_515);
const TITULO_BASE: WidgetId = WidgetId(10_518);
/// As linhas dos três painéis: rótulo e botões.
const ENTRADA_BASE: WidgetId = WidgetId(10_800);

/// A janela é larga: à esquerda a navegação, à direita o trabalho.
const PANEL_SIZE: Size = Size::new(920.0, 560.0);
const ROW_HEIGHT: f32 = 24.0;
/// A largura útil do painel: o que sobra depois das margens dos dois lados.
const BODY_WIDTH: f32 = PANEL_SIZE.width - 32.0;
/// Quanto da largura fica com a navegação, antes de alguém arrastar.
const RATIO_INICIAL: f32 = 0.28;
/// Onde as duas divisas dos painéis empilhados começam.
///
/// Um terço e metade: os três painéis nascem com a mesma altura, e a segunda
/// divisa reparte o que sobrou da primeira.
const RATIO_ALTO: f32 = 0.33;
const RATIO_BAIXO: f32 = 0.5;
/// Altura do título de cada painel.
const TITULO_ALTURA: f32 = 20.0;
/// Largura de um botão de ação de linha.
const ACAO_LARGURA: f32 = 92.0;

/// Em que painel um arquivo aparece.
///
/// É a divisão do `RepositoryStatus`, e não uma invenção da tela: é o que o
/// `--porcelain=v2` devolve, e o que decide o que cada ação faz na linha.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GitFileState {
    Staged,
    Modified,
    Untracked,
    Conflicted,
}

/// Um arquivo alterado, como a linha o mostra.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GitEntry {
    /// O caminho inteiro, que é o que a ação carrega.
    pub path: std::path::PathBuf,
    /// O caminho **relativo à raiz**, que é o que se lê na linha.
    pub label: String,
    pub state: GitFileState,
}

/// O que a margem do editor mostra numa linha.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GitLineChange {
    Added,
    Modified,
    Removed,
}

/// Uma branch, como a tela a mostra.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct BranchItem {
    pub name: String,
    /// Se é para ela que `HEAD` aponta.
    pub current: bool,
}

/// O que a tela sabe do repositório.
///
/// Vem pronto de quem perguntou. `head` é o rótulo — nome de branch ou hash
/// abreviado, porque `HEAD` solto também precisa aparecer —, e `changed` conta
/// **arquivos**, e não linhas de `status`: um arquivo preparado e alterado ao
/// mesmo tempo é um arquivo só na barra de estado.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct GitView {
    pub head: Option<String>,
    pub changed: usize,
    pub staged: usize,
    pub modified: usize,
    pub untracked: usize,
    pub branches: Vec<BranchItem>,
    /// Os arquivos alterados, na ordem em que os três painéis os mostram.
    pub entries: Vec<GitEntry>,
    /// O que dizer quando não há repositório, ou quando o Git falhou.
    ///
    /// Uma pasta sem `.git` é resposta legítima, e não erro: a janela abre e
    /// diz isso, em vez de aparecer vazia sem explicar por quê.
    pub message: Option<String>,
}

impl GitView {
    /// Se há repositório para mostrar.
    #[must_use]
    pub fn has_repository(&self) -> bool {
        self.head.is_some()
    }

    /// O que a barra de estado mostra, quando há o que mostrar.
    #[must_use]
    pub fn status_segment(&self) -> Option<String> {
        let head = self.head.as_ref()?;
        Some(if self.changed == 0 {
            head.clone()
        } else {
            format!("{head} ~{}", self.changed)
        })
    }
}

/// Qual aba do lado direito está na frente.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum Aba {
    #[default]
    Status,
    History,
}

/// A janela do gerenciador.
#[derive(Default)]
pub(super) struct GitSurface {
    modal: Option<ModalHost>,
    view: GitView,
    /// A árvore, mantida entre quadros: recriá-la a cada um jogaria fora a
    /// expansão e a rolagem, que é o estado que ela guarda por identidade.
    tree: Option<ComposedTreeView>,
    split: Option<SplitPane>,
    /// As duas divisas que empilham os três painéis da aba `status`.
    split_alto: Option<SplitPane>,
    split_baixo: Option<SplitPane>,
    /// As três listas, mantidas entre quadros pelo mesmo motivo da árvore: a
    /// rolagem e a seleção são delas.
    listas: Option<[ComposedList; 3]>,
    aba: Aba,
    /// As linhas mudadas de cada arquivo aberto, para a margem do editor.
    ///
    /// Mora aqui, e não num campo do `IdeShell`: é estado do Git, chega pelo
    /// mesmo caminho do resto e some junto quando o projeto troca. A margem é
    /// quem o lê, e ela pergunta a esta superfície.
    marcas: std::collections::HashMap<std::path::PathBuf, Vec<(usize, GitLineChange)>>,
    /// O texto da busca das branches.
    ///
    /// **É a terceira caixa da IDE, e é dela.** A do arquivo e a da saída do
    /// terminal nasceram dividindo um estado só, e uma impedia a outra de abrir;
    /// esta nasce com o texto próprio para que o defeito não tenha por onde
    /// voltar.
    busca: String,
}

/// Declara a janela ao anfitrião.
///
/// A divisão em duas colunas é da moldura, e não de uma conta daqui: a largura
/// da coluna da esquerda é ajustada a cada quadro a partir da proporção da
/// divisa, e o resto se arruma sozinho.
pub(super) fn attach(host: &mut UiHost, layer: WidgetId) {
    let _ = host.declare(
        layer,
        MODAL_ID,
        LayoutStyle {
            width: Some(PANEL_SIZE.width),
            height: Some(PANEL_SIZE.height),
            padding: EdgeInsets::only(56.0, 16.0, 16.0, 16.0),
            ..LayoutStyle::default()
        },
    );
    let _ = host.declare(
        MODAL_ID,
        BODY_ID,
        LayoutStyle {
            direction: LayoutDirection::Row,
            flex_grow: 1.0,
            ..LayoutStyle::default()
        },
    );
    let _ = host.declare(
        BODY_ID,
        SIDE_ID,
        LayoutStyle {
            width: Some(PANEL_SIZE.width * RATIO_INICIAL),
            gap: 8.0,
            ..LayoutStyle::default()
        },
    );
    let _ = host.declare(
        SIDE_ID,
        SEARCH_ID,
        LayoutStyle {
            height: Some(26.0),
            ..LayoutStyle::default()
        },
    );
    let _ = host.declare(
        SIDE_ID,
        TREE_ID,
        LayoutStyle {
            flex_grow: 1.0,
            ..LayoutStyle::default()
        },
    );
    let _ = host.declare(
        BODY_ID,
        WORK_ID,
        LayoutStyle {
            flex_grow: 1.0,
            gap: 8.0,
            // A folga da esquerda é a faixa da divisa: sem ela o conteúdo
            // encostaria na linha que se arrasta, e o clique de um viraria o do
            // outro.
            padding: EdgeInsets::only(0.0, 0.0, 0.0, 10.0),
            ..LayoutStyle::default()
        },
    );
    let _ = host.declare(
        WORK_ID,
        TABS_ID,
        LayoutStyle {
            height: Some(28.0),
            ..LayoutStyle::default()
        },
    );
    let _ = host.declare(
        WORK_ID,
        CONTENT_ID,
        LayoutStyle {
            flex_grow: 1.0,
            ..LayoutStyle::default()
        },
    );
}

/// A área que o arranjo deu a uma peça da janela.
fn area(host: &UiHost, id: WidgetId) -> Rect {
    host.bounds(id).unwrap_or_default()
}

impl GitSurface {
    #[must_use]
    pub(super) fn is_open(&self) -> bool {
        self.modal.as_ref().is_some_and(ModalHost::is_open)
    }

    /// Abre a janela, ou a fecha se já estava aberta.
    ///
    /// O mesmo botão faz as duas coisas, como o que recolhe o Explorer: um botão
    /// que só abre deixa quem clicou de novo sem saída a não ser o `Esc`.
    pub(super) fn toggle(&mut self) {
        if self.is_open() {
            self.close();
            return;
        }
        self.busca.clear();
        self.rebuild();
        self.modal
            .get_or_insert_with(|| ModalHost::new(MODAL_ID, "Git", PANEL_SIZE))
            .open();
    }

    pub(super) fn close(&mut self) {
        if let Some(modal) = self.modal.as_mut() {
            modal.close();
        }
    }

    /// Recebe o retrato do repositório.
    ///
    /// A árvore é refeita aqui e não na pintura: quem pinta não decide o que
    /// existe, e refazer a cada quadro apagaria a expansão a cada quadro.
    pub(super) fn set_view(&mut self, view: GitView) {
        if self.view == view {
            return;
        }
        self.view = view;
        self.rebuild();
        self.rebuild_listas();
    }

    /// Monta as três listas da aba `status`, uma por estado.
    ///
    /// Elas são refeitas a cada retrato novo, e é isso que faz a lista **não
    /// ficar velha depois de cada ação**: preparar um arquivo pede o retrato de
    /// novo, e o painel de onde ele saiu perde a linha no mesmo gesto.
    fn rebuild_listas(&mut self) {
        let listas = [
            GitFileState::Staged,
            GitFileState::Modified,
            GitFileState::Untracked,
        ]
        .map(|estado| {
            let linhas = self
                .view
                .entries
                .iter()
                .filter(|entrada| entrada.state == estado)
                .enumerate()
                .map(|(indice, entrada)| linha_de_arquivo(estado, indice, &entrada.label))
                .collect();
            ComposedList::new(WidgetId(LISTA_BASE.0 + estado as u64), linhas)
                .with_row_height(ROW_HEIGHT)
        });
        self.listas = Some(listas);
    }

    /// Os arquivos de um painel, na ordem em que ele os mostra.
    fn entradas(&self, estado: GitFileState) -> Vec<&GitEntry> {
        self.view
            .entries
            .iter()
            .filter(|entrada| entrada.state == estado)
            .collect()
    }

    #[must_use]
    pub(super) fn view(&self) -> &GitView {
        &self.view
    }

    /// O texto da busca, para quem precisa saber o que está filtrado.
    #[cfg(test)]
    pub(super) fn query(&self) -> &str {
        &self.busca
    }

    pub(super) fn text_input(&mut self, texto: &str) {
        if texto.chars().any(char::is_control) {
            return;
        }
        self.busca.push_str(texto);
        self.rebuild();
    }

    /// `Backspace` na busca. Devolve `true` quando a tecla era daqui.
    pub(super) fn key(&mut self, key: &str) -> bool {
        if key != "Backspace" {
            return false;
        }
        self.busca.pop();
        self.rebuild();
        true
    }

    /// Monta a árvore a partir do que se sabe e do que está filtrado.
    fn rebuild(&mut self) {
        let filtro = self.busca.to_lowercase();
        let combina = |nome: &str| filtro.is_empty() || nome.to_lowercase().contains(&filtro);
        let branches: Vec<&BranchItem> = self
            .view
            .branches
            .iter()
            .filter(|branch| combina(&branch.name))
            .collect();
        let mut raizes = Vec::new();
        let filhos: Vec<ComposedTreeItem> = branches
            .iter()
            .enumerate()
            .map(|(indice, branch)| {
                ComposedTreeItem::leaf(
                    100 + indice as u64,
                    linha(indice, &branch.name, branch.current),
                )
            })
            .collect();
        // O nó das branches só some quando a busca o esvaziou: um `branches`
        // aberto e vazio depois de digitar diria que não há branch nenhuma, o
        // que é mentira. Vazio **sem** busca é a verdade de um repositório
        // recém-criado, e continua aparecendo.
        if !filhos.is_empty() || filtro.is_empty() {
            raizes.push(ComposedTreeItem::new(
                1,
                raiz(0, &format!("Branches ({})", branches.len())),
                filhos,
            ));
        }
        // Os três que ainda não têm o que mostrar aparecem quando não há busca:
        // com filtro, "Tags" vazio seria um nó que não casa com o que se digitou.
        if filtro.is_empty() {
            for (indice, (id, nome)) in [(2, "Tags"), (3, "Remotes"), (4, "Stashes")]
                .into_iter()
                .enumerate()
            {
                raizes.push(ComposedTreeItem::leaf(id, raiz(indice + 1, nome)));
            }
        }
        match self.tree.as_mut() {
            // `set_roots` preserva expansão e seleção por identidade: sem ele,
            // digitar uma letra na busca fecharia o nó que estava aberto.
            Some(existente) => existente.set_roots(raizes),
            None => {
                self.tree = Some(ComposedTreeView::new(TREE_ID, raizes).with_row_height(ROW_HEIGHT));
            }
        }
    }

    /// Ajusta a moldura à proporção da divisa, **antes do arranjo do quadro**.
    ///
    /// Ela é chamada de `place_overlay`, e não da pintura, pelo motivo que já
    /// está escrito lá: o que a janela declara e muda com o estado dela entra
    /// antes do arranjo, ou vale só no quadro seguinte. Chamada da pintura, o
    /// arrasto da divisa aparecia um quadro atrasado — a coluna ficava com a
    /// largura da proporção anterior, e o ponteiro já estava noutro lugar.
    pub(super) fn sync_declaration(&mut self, host: &mut UiHost) {
        // A largura sai do **tamanho declarado do painel**, e não da área
        // medida: `place_overlay` limpa o arranjo antes de declarar, e ler a
        // moldura ali devolveria zero. O painel tem tamanho fixo, dito aqui em
        // cima, e por isso a conta não precisa de medida nenhuma.
        let proporcao = self.split.as_ref().map_or(RATIO_INICIAL, SplitPane::ratio);
        let largura = (BODY_WIDTH * proporcao).max(120.0);
        host.set_style(
            SIDE_ID,
            LayoutStyle {
                width: Some(largura),
                gap: 8.0,
                ..LayoutStyle::default()
            },
        );
    }

    /// A divisa, posicionada sobre a área do corpo.
    fn divisa(&mut self, host: &UiHost, context: &LayoutContext) -> Rect {
        let corpo = area(host, BODY_ID);
        let split = self
            .split
            .get_or_insert_with(|| SplitPane::new(SPLIT_ID, SplitOrientation::Horizontal, RATIO_INICIAL));
        split.layout(context, corpo);
        split.divider()
    }

    /// As três faixas empilhadas da aba `status`, de cima para baixo.
    ///
    /// **As áreas saem das divisas, e não de uma conta daqui**: é a mesma regra
    /// da divisão do editor. Com a conta em dois lugares, o clique cai num
    /// painel e o desenho aparece no outro.
    fn faixas(&mut self, conteudo: Rect, context: &LayoutContext) -> [Rect; 3] {
        let alto = self
            .split_alto
            .get_or_insert_with(|| {
                SplitPane::new(SPLIT_ALTO_ID, SplitOrientation::Vertical, RATIO_ALTO)
            });
        alto.layout(context, conteudo);
        let (primeira, resto) = (alto.first(), alto.second());
        let baixo = self.split_baixo.get_or_insert_with(|| {
            SplitPane::new(SPLIT_BAIXO_ID, SplitOrientation::Vertical, RATIO_BAIXO)
        });
        baixo.layout(context, resto);
        [primeira, baixo.first(), baixo.second()]
    }

    /// A área da lista dentro de uma faixa: o que sobra abaixo do título.
    fn area_da_lista(faixa: Rect) -> Rect {
        Rect::new(
            faixa.origin.x,
            faixa.origin.y + TITULO_ALTURA,
            faixa.size.width,
            (faixa.size.height - TITULO_ALTURA).max(0.0),
        )
    }

    /// O clique num dos três painéis: uma ação de linha, ou a diferença.
    ///
    /// **A coluna decide qual das três coisas foi.** Os botões têm largura fixa,
    /// dita aqui e usada pelo desenho: o clique nos últimos noventa e dois
    /// pontos é do botão da direita, o dos noventa e dois anteriores é do outro,
    /// e o resto da linha é o arquivo — que abre a diferença.
    fn clique_no_painel(
        &mut self,
        estado: GitFileState,
        faixa: Rect,
        context: &LayoutContext,
        point: Point,
    ) -> Option<GitRequest> {
        let indice_da_lista = estado as usize;
        let area = Self::area_da_lista(faixa);
        let listas = self.listas.as_mut()?;
        let lista = listas.get_mut(indice_da_lista)?;
        lista.layout(context, area);
        lista.event(
            &mut EventContext::default(),
            &UiEvent::PointerDown(primary_pointer(point)),
        );
        let escolhida = lista.selected()?;
        let caminho = self.entradas(estado).get(escolhida)?.path.clone();
        let direita = area.origin.x + area.size.width;
        let acoes = acoes_de(estado);
        // Da direita para a esquerda, uma faixa por ação: a última declarada é
        // a que fica mais à direita, e é a mesma ordem em que a linha as põe.
        for (posicao, acao) in acoes.iter().rev().enumerate() {
            let fim = direita - posicao as f32 * ACAO_LARGURA;
            if point.x >= fim - ACAO_LARGURA && point.x < fim {
                return Some(match acao {
                    Acao::Preparar => GitRequest::Stage(caminho),
                    Acao::Despreparar => GitRequest::Unstage(caminho),
                    Acao::Descartar => GitRequest::Discard(caminho),
                });
            }
        }
        Some(GitRequest::ShowDiff {
            path: caminho,
            staged: estado == GitFileState::Staged,
        })
    }

    /// O clique na janela. Devolve o que ele pede ao repositório, se pedir.
    ///
    /// A janela **não fala com o Git**: ela devolve o pedido, e quem o manda é o
    /// shell, pela fila de comandos. É a mesma fronteira do painel de build.
    pub(super) fn pointer_down(
        &mut self,
        host: &mut UiHost,
        context: &LayoutContext,
        point: Point,
    ) -> Option<GitRequest> {
        let divisa = self.divisa(host, context);
        if divisa.contains(point) {
            if let Some(split) = self.split.as_mut() {
                split.event(
                    &mut EventContext::default(),
                    &UiEvent::PointerDown(primary_pointer(point)),
                );
            }
            return None;
        }
        let abas = area(host, TABS_ID);
        if abas.contains(point) {
            // Duas abas de larguras iguais: qual delas é sai da coluna, que é a
            // mesma conta que a faixa desenhada usa.
            let metade = abas.origin.x + abas.size.width / 2.0;
            self.aba = if point.x < metade {
                Aba::Status
            } else {
                Aba::History
            };
            return None;
        }
        let arvore = area(host, TREE_ID);
        if arvore.contains(point)
            && let Some(tree) = self.tree.as_mut()
        {
            tree.layout(context, arvore);
            tree.event(
                &mut EventContext::default(),
                &UiEvent::PointerDown(primary_pointer(point)),
            );
            return None;
        }
        let conteudo = area(host, CONTENT_ID);
        if self.aba == Aba::Status && conteudo.contains(point) {
            let faixas = self.faixas(conteudo, context);
            // As duas divisas primeiro: elas ficam **entre** as faixas, e um
            // clique nelas cairia na lista de baixo se a pergunta viesse depois.
            for split in [self.split_alto.as_mut(), self.split_baixo.as_mut()]
                .into_iter()
                .flatten()
            {
                if split.divider().contains(point) {
                    split.event(
                        &mut EventContext::default(),
                        &UiEvent::PointerDown(primary_pointer(point)),
                    );
                    return None;
                }
            }
            for (estado, faixa) in ESTADOS.into_iter().zip(faixas) {
                if faixa.contains(point) {
                    return self.clique_no_painel(estado, faixa, context, point);
                }
            }
            return None;
        }
        // O que sobrou é o véu, atrás do painel: ali o clique dispensa a janela.
        if !area(host, MODAL_ID).contains(point) {
            self.close();
        }
        None
    }

    /// Movimento e soltura: são da divisa, que é o que se arrasta aqui.
    pub(super) fn pointer_event(&mut self, host: &UiHost, context: &LayoutContext, event: &UiEvent) {
        let corpo = area(host, BODY_ID);
        if let Some(split) = self.split.as_mut() {
            split.layout(context, corpo);
            split.event(&mut EventContext::default(), event);
        }
        // As duas divisas dos painéis empilhados recebem o mesmo gesto: elas
        // também se arrastam, e também precisam do movimento para se anunciar.
        let conteudo = area(host, CONTENT_ID);
        if self.aba == Aba::Status && conteudo.size.height > 0.0 {
            let _ = self.faixas(conteudo, context);
            for split in [self.split_alto.as_mut(), self.split_baixo.as_mut()]
                .into_iter()
                .flatten()
            {
                split.event(&mut EventContext::default(), event);
            }
        }
    }

    /// Se a divisa desta janela responde por este ponto.
    ///
    /// Arrastando **ou** sob o ponteiro: uma divisa que só se anuncia depois de
    /// alguém a mover não anuncia nada. É a mesma regra das outras três.
    #[must_use]
    pub(super) fn divider_hover(&self, point: Point) -> bool {
        self.split
            .as_ref()
            .is_some_and(|split| split.is_dragging() || split.divider().contains(point))
    }

    /// Onde a divisa está, para o teste apontar o arrasto para ela.
    #[cfg(test)]
    pub(super) fn divider_area(&self) -> Rect {
        self.split
            .as_ref()
            .map(SplitPane::divider)
            .unwrap_or_default()
    }

    pub(super) fn scroll(&mut self, host: &UiHost, context: &LayoutContext, point: Point, linhas: f32) {
        let evento = UiEvent::Scroll(ScrollEvent {
            position: point,
            delta_x: 0.0,
            delta_y: linhas * ROW_HEIGHT,
        });
        let arvore = area(host, TREE_ID);
        if arvore.contains(point) {
            if let Some(tree) = self.tree.as_mut() {
                tree.layout(context, arvore);
                tree.event(&mut EventContext::default(), &evento);
            }
            return;
        }
        // A roda é do painel sob o ponteiro, e não do primeiro que a aceite:
        // três listas empilhadas com uma barra cada só se distinguem pela área.
        let conteudo = area(host, CONTENT_ID);
        if self.aba != Aba::Status || !conteudo.contains(point) {
            return;
        }
        let faixas = self.faixas(conteudo, context);
        for (indice, faixa) in faixas.into_iter().enumerate() {
            if !faixa.contains(point) {
                continue;
            }
            let area_da_lista = Self::area_da_lista(faixa);
            if let Some(lista) = self.listas.as_mut().and_then(|listas| listas.get_mut(indice)) {
                lista.layout(context, area_da_lista);
                lista.event(&mut EventContext::default(), &evento);
            }
            return;
        }
    }

    pub(super) fn paint(
        &mut self,
        host: &UiHost,
        layout: &LayoutContext,
        paint: &mut PaintContext,
        size: Size,
    ) -> bool {
        if !self.is_open() {
            return false;
        }
        if let Some(modal) = self.modal.as_ref() {
            let mut copia = modal.clone();
            copia.layout(layout, Rect::new(0.0, 0.0, size.width, size.height));
            copia.paint(paint);
        }

        let mut campo = TextInput::new(SEARCH_ID, &self.busca).with_placeholder("Procurar branch");
        campo.event(&mut EventContext::default(), &UiEvent::FocusGained);
        campo.layout(layout, area(host, SEARCH_ID));
        campo.paint(paint);

        let arvore = area(host, TREE_ID);
        if let Some(tree) = self.tree.as_mut() {
            tree.layout(layout, arvore);
            tree.paint(paint);
        }

        let _ = self.divisa(host, layout);
        if let Some(split) = self.split.as_ref() {
            split.paint(paint);
        }

        let mut abas = Tabs::new(
            TABS_ID,
            vec![
                TabItem::new(1, "Status"),
                TabItem::new(2, "History"),
            ],
        );
        abas.set_active(usize::from(self.aba == Aba::History));
        abas.layout(layout, area(host, TABS_ID));
        abas.paint(paint);

        let conteudo = area(host, CONTENT_ID);
        if self.aba == Aba::Status && self.view.has_repository() && self.view.changed > 0 {
            self.paint_status(conteudo, layout, paint);
            return true;
        }
        for (indice, texto) in self.linhas_do_lado_direito().into_iter().enumerate() {
            let mut rotulo = Label::new(WidgetId(SUMMARY_BASE.0 + indice as u64), texto);
            if indice > 0 {
                rotulo = rotulo.with_tone(IconTint::Muted);
            }
            rotulo.layout(
                layout,
                Rect::new(
                    conteudo.origin.x,
                    conteudo.origin.y + indice as f32 * 22.0,
                    conteudo.size.width,
                    20.0,
                ),
            );
            rotulo.paint(paint);
        }
        true
    }

    /// Os três painéis empilhados, com título e lista cada um.
    ///
    /// O título traz a contagem: um painel vazio com nome é o que diz que não há
    /// nada preparado — sem ele, quem olha não sabe se a lista está vazia ou se
    /// a IDE não respondeu.
    fn paint_status(&mut self, conteudo: Rect, layout: &LayoutContext, paint: &mut PaintContext) {
        let faixas = self.faixas(conteudo, layout);
        for (indice, (estado, faixa)) in ESTADOS.into_iter().zip(faixas).enumerate() {
            let quantos = self
                .view
                .entries
                .iter()
                .filter(|entrada| entrada.state == estado)
                .count();
            let mut titulo = Label::new(
                WidgetId(TITULO_BASE.0 + indice as u64),
                format!("{} ({quantos})", nome_do_estado(estado)),
            )
            .with_tone(IconTint::Muted);
            titulo.layout(
                layout,
                Rect::new(faixa.origin.x, faixa.origin.y, faixa.size.width, TITULO_ALTURA),
            );
            titulo.paint(paint);
            let area_da_lista = Self::area_da_lista(faixa);
            if let Some(lista) = self.listas.as_mut().and_then(|listas| listas.get_mut(indice)) {
                lista.layout(layout, area_da_lista);
                lista.paint(paint);
            }
        }
        // As divisas por último: elas ficam **sobre** a borda entre dois
        // painéis, e desenhadas antes some sob a lista de baixo.
        for split in [self.split_alto.as_ref(), self.split_baixo.as_ref()]
            .into_iter()
            .flatten()
        {
            split.paint(paint);
        }
    }

    /// O que o lado direito diz quando não há três painéis para mostrar.
    ///
    /// Repositório nenhum, Git que falhou, ou árvore limpa: são os três casos em
    /// que uma lista vazia não explicaria o que está acontecendo.
    fn linhas_do_lado_direito(&self) -> Vec<String> {
        if let Some(mensagem) = self.view.message.as_deref() {
            return vec![mensagem.to_owned()];
        }
        if !self.view.has_repository() {
            return vec!["Esta pasta não está num repositório Git".to_owned()];
        }
        match self.aba {
            Aba::Status if self.view.changed == 0 => {
                vec!["Nada mudou desde o último commit".to_owned()]
            }
            // Com alteração, quem desenha é `paint_status`: estas linhas são o
            // que sobra para os casos em que não há painel nenhum a mostrar.
            Aba::Status => vec![format!("{} arquivo(s) alterado(s)", self.view.changed)],
            Aba::History => vec!["O histórico ainda não é mostrado aqui".to_owned()],
        }
    }

    /// As três faixas da aba `status`, para o teste apontar um gesto nelas.
    #[cfg(test)]
    pub(super) fn faixas_do_status(
        &mut self,
        host: &UiHost,
        context: &LayoutContext,
    ) -> [Rect; 3] {
        let conteudo = area(host, CONTENT_ID);
        self.faixas(conteudo, context)
    }

    /// Áreas da janela, para os testes apontarem um gesto dentro dela.
    #[cfg(test)]
    pub(super) fn areas(host: &UiHost) -> (Rect, Rect, Rect, Rect) {
        (
            area(host, MODAL_ID),
            area(host, SEARCH_ID),
            area(host, TREE_ID),
            area(host, TABS_ID),
        )
    }

    /// Quantas linhas a árvore mostra agora, para o teste ver o filtro agir.
    ///
    /// Sai da altura do conteúdo, que é o que a árvore publica: ela não conta
    /// linhas para fora, e é a mesma altura que dimensiona a rolagem.
    #[cfg(test)]
    pub(super) fn visible_rows(&self) -> usize {
        self.tree.as_ref().map_or(0, |tree| {
            (tree.content_size().height / ROW_HEIGHT).round() as usize
        })
    }
}

impl super::IdeShell {
    /// Abre o gerenciador, ou o fecha se já estava aberto.
    ///
    /// Abrir pede o retrato de novo: entre a última resposta e agora o usuário
    /// pode ter commitado no terminal integrado, e uma janela que mostra o
    /// estado de dez minutos atrás não avisa que está errada — ela só está
    /// errada. Observar o repositório sozinha é fase 4.
    pub fn toggle_git(&mut self) {
        self.git.toggle();
        if self.git.is_open() {
            self.commands
                .push(ApplicationCommand::Git(GitRequest::Refresh));
        }
    }

    /// Manda ao repositório o que a janela pediu, e pede o retrato de novo.
    ///
    /// **Os dois juntos, sempre.** Preparar um arquivo e deixar a lista como
    /// estava faria quem preparou ver a linha continuar em "alterados" — e
    /// desfazer o que acabou de fazer. É o critério da fase 1: a lista não fica
    /// velha depois de cada ação.
    pub(super) fn pedir_ao_git(&mut self, pedido: GitRequest) {
        let precisa_de_retrato = !matches!(pedido, GitRequest::ShowDiff { .. });
        self.commands.push(ApplicationCommand::Git(pedido));
        if precisa_de_retrato {
            self.commands
                .push(ApplicationCommand::Git(GitRequest::Refresh));
        }
    }

    /// Recebe o retrato do repositório, de quem o pediu.
    ///
    /// A tela não pergunta nada ao Git: quem pergunta é a aplicação, fora da
    /// linha de execução da interface, e o que chega aqui é a resposta pronta.
    pub fn set_git_view(&mut self, view: GitView) {
        self.git.set_view(view);
    }

    /// Se a divisa do gerenciador está sob o ponteiro, ou sendo arrastada.
    ///
    /// A janela é modal: com ela aberta, o ponteiro só pode falar dela.
    #[must_use]
    pub fn git_divider_hover(&self, point: Point) -> bool {
        self.git.is_open() && self.git.divider_hover(point)
    }

    /// O que a tela sabe do repositório agora.
    #[must_use]
    pub fn git_view(&self) -> &GitView {
        self.git.view()
    }

    /// Guarda o que mudou num arquivo, para a margem do editor mostrar.
    ///
    /// Lista vazia **apaga** as marcas daquele arquivo, e é assim de propósito:
    /// commitar deixa o arquivo igual ao commit, e uma margem que continuasse
    /// riscada estaria contando o trabalho de antes.
    pub fn set_git_line_marks(
        &mut self,
        path: std::path::PathBuf,
        marks: Vec<(usize, GitLineChange)>,
    ) {
        if marks.is_empty() {
            self.git.marcas.remove(&path);
            return;
        }
        self.git.marcas.insert(path, marks);
    }

    /// As linhas mudadas de um arquivo, para quem desenha a margem.
    #[must_use]
    pub(super) fn git_line_marks(&self, path: &std::path::Path) -> &[(usize, GitLineChange)] {
        self.git
            .marcas
            .get(path)
            .map_or(&[][..], |marcas| marcas.as_slice())
    }

    /// Abre a comparação: o commitado à esquerda, o de agora à direita.
    ///
    /// **O texto de então não vira arquivo no disco.** Ele entra como documento
    /// de memória, que a sessão já sabe abrir: materializá-lo num temporário
    /// daria a quem abrisse uma cópia editável do passado, que salva por cima de
    /// nada e some sem avisar.
    ///
    /// A janela do gerenciador fecha ao abrir a comparação, e é a resposta à
    /// pergunta que a `22` deixou registrada: a diferença abre **no editor**, e
    /// o editor está atrás do véu. Quem escolheu ver a diferença quer ver a
    /// diferença.
    pub fn abrir_comparacao(
        &mut self,
        path: &std::path::Path,
        conteudo_de_agora: String,
        texto: String,
    ) -> bool {
        // O arquivo de agora entra pelo mesmo caminho de sempre — quem lê disco
        // é a aplicação, e ela já leu para chegar aqui.
        let atual = self.show_document(path, conteudo_de_agora);
        let nome = path
            .file_name()
            .map(|nome| nome.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string());
        let antigo = self
            .editor_area
            .session
            .open_memory(std::path::PathBuf::from(format!("{nome} @ HEAD")), texto);
        // O de agora fica onde já estava, e o de então vai para o lado: a
        // divisão é a mesma que a aba do editor oferece, e não uma segunda.
        self.dividir_a_direita(antigo);
        let _ = self.editor_area.session.activate(atual);
        self.git.close();
        true
    }

    /// A janela do gerenciador, para os testes olharem dentro dela.
    #[cfg(test)]
    pub(super) fn git_surface(&self) -> &GitSurface {
        &self.git
    }
}

/// Os três painéis, na ordem em que se empilham.
const ESTADOS: [GitFileState; 3] = [
    GitFileState::Staged,
    GitFileState::Modified,
    GitFileState::Untracked,
];

/// O que a linha de um painel oferece fazer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Acao {
    Preparar,
    Despreparar,
    Descartar,
}

impl Acao {
    const fn rotulo(self) -> &'static str {
        match self {
            Self::Preparar => "Preparar",
            Self::Despreparar => "Despreparar",
            Self::Descartar => "Descartar",
        }
    }

    const fn comando(self) -> &'static str {
        match self {
            Self::Preparar => "git.stage",
            Self::Despreparar => "git.unstage",
            Self::Descartar => "git.discard",
        }
    }
}

/// O que cada painel oferece, e por quê.
///
/// **Não rastreado não tem "Descartar"**, e a ausência é a decisão: descartar
/// um arquivo que o Git não conhece seria apagá-lo do disco, e não há de onde
/// trazê-lo de volta. Quem quiser apagá-lo apaga pelo Explorer, onde apagar é o
/// que se espera de apagar.
const fn acoes_de(estado: GitFileState) -> &'static [Acao] {
    match estado {
        GitFileState::Staged => &[Acao::Despreparar],
        GitFileState::Modified | GitFileState::Conflicted => &[Acao::Preparar, Acao::Descartar],
        GitFileState::Untracked => &[Acao::Preparar],
    }
}

const fn nome_do_estado(estado: GitFileState) -> &'static str {
    match estado {
        GitFileState::Staged => "Preparados",
        GitFileState::Modified => "Alterados",
        GitFileState::Untracked => "Não rastreados",
        GitFileState::Conflicted => "Em conflito",
    }
}

/// A linha de um arquivo: o caminho e os botões daquele painel.
///
/// Quem monta as células é a IDE — a lista composta só posiciona o que recebe —,
/// e é por isso que a ação de cada painel pode ser diferente sem a lista saber
/// de nada.
fn linha_de_arquivo(estado: GitFileState, indice: usize, caminho: &str) -> ComposedRow {
    let base = ENTRADA_BASE.0 + (estado as u64) * 1_000 + indice as u64 * 4;
    let mut celulas = vec![ComposedCell::new(
        Box::new(Label::new(WidgetId(base), caminho)),
        CellWidth::Fill,
    )];
    for (posicao, acao) in acoes_de(estado).iter().enumerate() {
        celulas.push(ComposedCell::new(
            Box::new(
                Button::new(WidgetId(base + 1 + posicao as u64), acao.rotulo())
                    .with_command(acao.comando()),
            ),
            CellWidth::Fixed(ACAO_LARGURA),
        ));
    }
    ComposedRow::new(celulas)
}

/// Uma linha de raiz: só o nome, que é o que um agrupador tem.
fn raiz(indice: usize, nome: &str) -> ComposedRow {
    ComposedRow::new(vec![ComposedCell::new(
        Box::new(Label::new(WidgetId(ROW_BASE.0 + indice as u64), nome)),
        CellWidth::Natural,
    )])
}

/// Uma linha de branch: a marca da atual e o nome.
///
/// A marca é uma célula, e não um prefixo no texto: quem monta as células é a
/// IDE, e um `●` colado ao nome iria junto numa cópia e numa busca.
fn linha(indice: usize, nome: &str, atual: bool) -> ComposedRow {
    let base = ROW_BASE.0 + 100 + indice as u64 * 2;
    ComposedRow::new(vec![
        ComposedCell::new(
            Box::new(
                Label::new(WidgetId(base), if atual { "●" } else { "" })
                    .with_tone(IconTint::Accent),
            ),
            CellWidth::Fixed(14.0),
        ),
        ComposedCell::new(
            Box::new(Label::new(WidgetId(base + 1), nome)),
            CellWidth::Natural,
        ),
    ])
}
