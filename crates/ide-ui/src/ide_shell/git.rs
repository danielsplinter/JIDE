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
    Button, CellWidth, ComposedCell, ComposedList, ComposedRow, ComposedTable, ComposedTreeItem,
    ComposedTreeView, GraphCell, IconTint, Label, ModalHost, SplitOrientation, SplitPane,
    TabItem, TableColumn, Tabs, TextInput,
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
/// A tabela do histórico, e as células de cada linha dela.
const TABELA_ID: WidgetId = WidgetId(10_519);
const COMMIT_BASE: WidgetId = WidgetId(11_000);
/// A caixa do nome da branch nova, e o botão que a cria.
const NOVA_ID: WidgetId = WidgetId(11_910);
const CRIAR_ID: WidgetId = WidgetId(11_911);
/// Os dois botões do estado intermediário: continuar e abortar.
const CONTINUAR_ID: WidgetId = WidgetId(11_912);
const ABORTAR_ID: WidgetId = WidgetId(11_913);
/// A caixa da mensagem de commit e os dois botões dela.
const MENSAGEM_ID: WidgetId = WidgetId(11_900);
const COMMIT_ID: WidgetId = WidgetId(11_901);
const AMEND_ID: WidgetId = WidgetId(11_902);

/// A janela é larga: à esquerda a navegação, à direita o trabalho.
///
/// A largura é a da tabela do histórico, e não a da árvore: cinco colunas —
/// grafo, descrição, data, autor e hash — com as quatro últimas de largura
/// própria deixavam a descrição espremida em novecentos pontos.
const PANEL_SIZE: Size = Size::new(1200.0, 560.0);
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
/// A altura de um botão **dentro de uma linha de lista**.
///
/// É a exceção que a biblioteca prevê, e a decisão é desta tela: um botão de
/// altura padrão faria cada linha ter quarenta pontos, e um painel de arquivos
/// alterados mostraria três de cada vez. Todo botão fora de linha usa o padrão.
const ALTURA_NA_LINHA: f32 = ROW_HEIGHT - 2.0;
/// Altura da faixa de commit, embaixo dos três painéis.
///
/// A caixa da mensagem, a folga e um botão de altura padrão: as três medidas
/// vêm de quem as define, e o botão define a dele.
const COMMIT_ALTURA: f32 = 28.0 + 8.0 + Button::HEIGHT;
/// Altura da faixa do conflito, no alto da aba `status`.
const CONFLITO_ALTURA: f32 = Button::HEIGHT + 6.0;
/// Largura de um botão de linha da árvore.
///
/// Menor que o das ações de arquivo: a coluna da esquerda é estreita, e dois
/// botões do tamanho dos outros não deixariam nome de branch nenhum aparecer.
const ACAO_DA_ARVORE: f32 = 64.0;
/// Quantos commits cabem numa página do histórico.
///
/// Uma página é o que se pede de cada vez, e não o que se mostra: a tabela é
/// virtualizada, e rolar até o fim pede a seguinte. Um repositório de verdade
/// tem dezenas de milhares de commits.
pub const PAGINA_DO_HISTORICO: usize = 100;

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

/// Um commit, como a tabela do histórico o mostra.
///
/// As faixas do grafo vêm calculadas: **a IDE calcula, a biblioteca desenha**.
/// A tela recebe o resultado da conta, e não o histórico para refazê-la.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CommitRow {
    /// O hash inteiro; a tabela mostra o começo dele.
    pub hash: String,
    pub summary: String,
    pub author: String,
    pub date: String,
    /// Em que faixa o ponto fica, quantas faixas há, o que atravessa e para
    /// onde vão os pais.
    pub lane: usize,
    pub lanes: usize,
    pub passing: Vec<usize>,
    pub parents: Vec<usize>,
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
    /// Quantos commits ela tem a mais e a menos que o upstream.
    ///
    /// É a contagem contra o que **já foi buscado**: sem `fetch`, ela fala do
    /// que se sabia da última vez. Prometer o número de agora exigiria falar com
    /// a rede a cada retrato.
    pub ahead: usize,
    pub behind: usize,
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
    /// O histórico já carregado, do mais recente para o mais antigo.
    pub commits: Vec<CommitRow>,
    /// As tags, o que está guardado no `stash`, e as branches remotas.
    pub tags: Vec<String>,
    pub stashes: Vec<String>,
    pub remotes: Vec<String>,
    /// A operação que está no meio do caminho, se há uma.
    ///
    /// Vem do disco, e não da memória da IDE: quem rodou `git merge` no terminal
    /// integrado deixou o repositório assim, e a tela precisa dizer isso.
    pub pending: Option<String>,
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
    /// A tabela do histórico, mantida pelo mesmo motivo.
    tabela: Option<ComposedTable>,
    /// O que se está escrevendo como mensagem do commit.
    mensagem: String,
    /// O nome que se está escrevendo para a branch nova.
    ///
    /// Caixa própria, e não a da busca: procurar e nomear são duas coisas, e uma
    /// caixa que fizesse as duas criaria branch com o texto de um filtro.
    nome_novo: String,
    /// Se o cursor está na caixa do nome da branch nova.
    nomeando: bool,
    /// Se o cursor está na caixa da mensagem.
    ///
    /// Sem isso, digitar no gerenciador iria sempre para a busca das branches —
    /// e quem clicou na caixa da mensagem veria a letra aparecer noutro lugar.
    escrevendo: bool,
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
    // A faixa de criar branch fica **embaixo** da árvore: criar é o que se faz
    // depois de olhar o que já existe, e no alto ela disputaria com a busca.
    let _ = host.declare(
        SIDE_ID,
        NOVA_ID,
        LayoutStyle {
            height: Some(Button::HEIGHT),
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
        self.rebuild_tabela();
    }

    /// Monta a tabela do histórico: cinco colunas, uma linha por commit.
    ///
    /// A coluna `Nó` recebe a célula do grafo, com as faixas que a aplicação
    /// calculou. O `Hash` aparece abreviado — quem copia hash copia o inteiro,
    /// e ele está guardado na linha.
    fn rebuild_tabela(&mut self) {
        let largura_do_grafo = self
            .view
            .commits
            .iter()
            .map(|commit| commit.lanes.max(1))
            .max()
            .unwrap_or(1);
        let linhas = self
            .view
            .commits
            .iter()
            .enumerate()
            .map(|(indice, commit)| linha_de_commit(indice, commit, largura_do_grafo))
            .collect();
        match self.tabela.as_mut() {
            Some(tabela) => tabela.set_rows(linhas),
            None => {
                self.tabela = Some(
                    ComposedTable::new(
                        TABELA_ID,
                        vec![
                            TableColumn::new("Nó", CellWidth::Fixed(GRAFO_LARGURA)),
                            TableColumn::new("Description", CellWidth::Fill),
                            TableColumn::new("Date", CellWidth::Fixed(160.0)),
                            TableColumn::new("Author", CellWidth::Fixed(80.0)),
                            TableColumn::new("Hash", CellWidth::Fixed(80.0)),
                        ],
                        linhas,
                    )
                    .with_row_height(ROW_HEIGHT),
                );
            }
        }
    }

    /// Quantos commits ainda faltam pedir, se a rolagem chegou perto do fim.
    ///
    /// **A tabela é virtualizada e o histórico vem por páginas**: um repositório
    /// de verdade tem dezenas de milhares de commits, e carregar todos para
    /// mostrar quarenta linhas é o oposto do que a `19` e a `20` fizeram no
    /// índice.
    fn precisa_de_mais_historico(&self) -> bool {
        let Some(tabela) = self.tabela.as_ref() else {
            return false;
        };
        if self.view.commits.is_empty() || !tabela.scrolls() {
            return false;
        }
        // Uma tela antes do fim: pedir só ao chegar nele faria a rolagem parar
        // e esperar.
        let fim = self.view.commits.len() as f32 * ROW_HEIGHT;
        tabela.scroll_offset() + ROW_HEIGHT * 20.0 >= fim
    }

    /// A mensagem que se está escrevendo, para o teste ver o que foi digitado.
    #[cfg(test)]
    pub(super) fn mensagem(&self) -> &str {
        &self.mensagem
    }

    pub(super) fn limpar_mensagem(&mut self) {
        self.mensagem.clear();
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
        // Três caixas na mesma janela, e o cursor decide qual recebe. Sem essa
        // pergunta, escrever a mensagem do commit filtraria as branches.
        if self.escrevendo {
            self.mensagem.push_str(texto);
            return;
        }
        if self.nomeando {
            self.nome_novo.push_str(texto);
            return;
        }
        self.busca.push_str(texto);
        self.rebuild();
    }

    /// Tecla na janela. Devolve `true` quando ela era daqui.
    pub(super) fn key(&mut self, key: &str) -> bool {
        if self.escrevendo {
            match key {
                "Backspace" => {
                    self.mensagem.pop();
                    return true;
                }
                // A mensagem de commit tem mais de uma linha: a primeira é o
                // resumo, e o resto é o corpo. `Enter` escreve, e não confirma —
                // confirmar é o botão, que é o gesto que não se dá sem querer.
                "Enter" => {
                    self.mensagem.push('\n');
                    return true;
                }
                _ => return false,
            }
        }
        if self.nomeando {
            if key != "Backspace" {
                return false;
            }
            self.nome_novo.pop();
            return true;
        }
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
                ComposedTreeItem::leaf(100 + indice as u64, linha(indice, branch))
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
        // Os outros três nós. O filtro é das branches, e por isso eles só
        // aparecem sem busca: "Tags" cheio depois de digitar um nome de branch
        // diria que aquelas tags casaram com o que se procurou.
        if filtro.is_empty() {
            let folhas = |base: u64, itens: &[String]| -> Vec<ComposedTreeItem> {
                itens
                    .iter()
                    .enumerate()
                    .map(|(indice, texto)| {
                        ComposedTreeItem::leaf(base + indice as u64, raiz(10, texto))
                    })
                    .collect()
            };
            raizes.push(ComposedTreeItem::new(
                2,
                raiz(1, &format!("Tags ({})", self.view.tags.len())),
                folhas(2_000, &self.view.tags),
            ));
            raizes.push(ComposedTreeItem::new(
                3,
                // A raiz dos remotos é a única linha com botão: `fetch` é do
                // repositório inteiro, e não de uma branch.
                linha_do_remoto(&format!("Remotes ({})", self.view.remotes.len())),
                folhas(4_000, &self.view.remotes),
            ));
            raizes.push(ComposedTreeItem::new(
                4,
                raiz(3, &format!("Stashes ({})", self.view.stashes.len())),
                folhas(3_000, &self.view.stashes),
            ));
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
        // As faixas fixas saem da altura antes de tudo — a do conflito no alto e
        // a do commit embaixo —, e os três painéis repartem o que sobra.
        // Tirá-las depois faria a divisa cair dentro delas.
        let alto = if self.view.pending.is_some() {
            CONFLITO_ALTURA
        } else {
            0.0
        };
        let conteudo = Rect::new(
            conteudo.origin.x,
            conteudo.origin.y + alto,
            conteudo.size.width,
            (conteudo.size.height - COMMIT_ALTURA - alto).max(0.0),
        );
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

    /// Onde ficam a caixa da mensagem e os dois botões.
    ///
    /// Embaixo dos três painéis, e não em cima: a ordem na tela é a do trabalho
    /// — escolher o que entra, depois dizer o que se fez.
    fn faixa_do_commit(conteudo: Rect) -> Rect {
        Rect::new(
            conteudo.origin.x,
            conteudo.origin.y + (conteudo.size.height - COMMIT_ALTURA).max(0.0),
            conteudo.size.width,
            COMMIT_ALTURA.min(conteudo.size.height),
        )
    }

    /// Os dois botões da faixa de commit, da direita para a esquerda.
    fn botoes_do_commit(faixa: Rect) -> [(WidgetId, Rect); 2] {
        // A altura é a do botão, e não um número escrito aqui: dois lugares com
        // o mesmo número divergem na primeira mudança.
        let y = faixa.origin.y + faixa.size.height - Button::HEIGHT;
        let direita = faixa.origin.x + faixa.size.width;
        [
            (
                COMMIT_ID,
                Rect::new(direita - ACAO_LARGURA, y, ACAO_LARGURA, Button::HEIGHT),
            ),
            (
                AMEND_ID,
                Rect::new(
                    direita - ACAO_LARGURA * 2.0 - 8.0,
                    y,
                    ACAO_LARGURA,
                    Button::HEIGHT,
                ),
            ),
        ]
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

    /// O clique na árvore: uma ação de linha, ou só a seleção.
    ///
    /// A coluna decide, como nos três painéis: os botões têm largura fixa, dita
    /// aqui e usada pelo desenho.
    fn clique_na_arvore(
        &mut self,
        arvore: Rect,
        context: &LayoutContext,
        point: Point,
    ) -> Option<GitRequest> {
        let tree = self.tree.as_mut()?;
        tree.layout(context, arvore);
        tree.event(
            &mut EventContext::default(),
            &UiEvent::PointerDown(primary_pointer(point)),
        );
        let escolhido = tree.selected()?;
        let direita = arvore.origin.x + arvore.size.width;
        let na_faixa = |posicao: usize| {
            let fim = direita - posicao as f32 * ACAO_DA_ARVORE;
            point.x >= fim - ACAO_DA_ARVORE && point.x < fim
        };
        // As branches filtradas, na mesma ordem em que a árvore as montou.
        if (100..1_000).contains(&escolhido) {
            let indice = (escolhido - 100) as usize;
            let branch = self.branches_filtradas().get(indice).copied()?;
            let nome = branch.name.clone();
            // Da direita para a esquerda: o último declarado é o mais à
            // direita. A branch atual tem as ações do remoto; as outras, as de
            // trocar e fundir.
            if branch.current {
                if na_faixa(0) {
                    return Some(GitRequest::Push);
                }
                if na_faixa(1) {
                    return Some(GitRequest::Pull);
                }
                return None;
            }
            if na_faixa(0) {
                return Some(GitRequest::Merge(nome));
            }
            if na_faixa(1) {
                return Some(GitRequest::SwitchBranch(nome));
            }
            return None;
        }
        // A raiz dos remotos: o botão dela busca as referências.
        if escolhido == 3 && na_faixa(0) {
            return Some(GitRequest::Fetch);
        }
        // Um item guardado: clicar nele o devolve para a árvore de trabalho.
        if (3_000..4_000).contains(&escolhido) {
            return Some(GitRequest::StashPop((escolhido - 3_000) as usize));
        }
        None
    }

    /// As branches que a busca deixou passar, na ordem da árvore.
    fn branches_filtradas(&self) -> Vec<&BranchItem> {
        let filtro = self.busca.to_lowercase();
        self.view
            .branches
            .iter()
            .filter(|branch| filtro.is_empty() || branch.name.to_lowercase().contains(&filtro))
            .collect()
    }

    /// A faixa do estado intermediário, no alto da aba `status`.
    ///
    /// Ela só existe quando há operação em curso — e é isso que impede a IDE de
    /// ficar presa num estado do qual não se sai: enquanto houver, os dois
    /// botões de saída estão na tela.
    fn faixa_do_conflito(&self, conteudo: Rect) -> Option<Rect> {
        self.view.pending.as_ref()?;
        Some(Rect::new(
            conteudo.origin.x,
            conteudo.origin.y,
            conteudo.size.width,
            CONFLITO_ALTURA,
        ))
    }

    /// Os dois botões de saída, da direita para a esquerda.
    fn botoes_do_conflito(faixa: Rect) -> [(WidgetId, Rect); 2] {
        let y = faixa.origin.y + 3.0;
        let direita = faixa.origin.x + faixa.size.width;
        [
            (
                ABORTAR_ID,
                Rect::new(direita - ACAO_LARGURA, y, ACAO_LARGURA, Button::HEIGHT),
            ),
            (
                CONTINUAR_ID,
                Rect::new(
                    direita - ACAO_LARGURA * 2.0 - 8.0,
                    y,
                    ACAO_LARGURA,
                    Button::HEIGHT,
                ),
            ),
        ]
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
            // O histórico é caro e só é pedido quando alguém vai olhá-lo: quem
            // abre o gerenciador para preparar um arquivo não paga por ele.
            if self.aba == Aba::History && self.view.commits.is_empty() {
                return Some(GitRequest::LoadHistory { ja_carregados: 0 });
            }
            return None;
        }
        let nova = area(host, NOVA_ID);
        if nova.contains(point) {
            // Os últimos pontos da faixa são o botão; o resto é a caixa.
            let botao = nova.origin.x + nova.size.width - ACAO_LARGURA;
            if point.x >= botao {
                let nome = self.nome_novo.trim().to_owned();
                self.nomeando = false;
                if nome.is_empty() {
                    return None;
                }
                self.nome_novo.clear();
                return Some(GitRequest::CreateBranch(nome));
            }
            self.nomeando = true;
            self.escrevendo = false;
            return None;
        }
        let arvore = area(host, TREE_ID);
        if arvore.contains(point) {
            self.nomeando = false;
            return self.clique_na_arvore(arvore, context, point);
        }
        let conteudo = area(host, CONTENT_ID);
        if self.aba == Aba::History && conteudo.contains(point) {
            if let Some(tabela) = self.tabela.as_mut() {
                tabela.layout(context, conteudo);
                tabela.event(
                    &mut EventContext::default(),
                    &UiEvent::PointerDown(primary_pointer(point)),
                );
            }
            return None;
        }
        if self.aba == Aba::Status && conteudo.contains(point) {
            if let Some(faixa) = self.faixa_do_conflito(conteudo)
                && faixa.contains(point)
            {
                for (id, area_do_botao) in Self::botoes_do_conflito(faixa) {
                    if area_do_botao.contains(point) {
                        return Some(if id == ABORTAR_ID {
                            GitRequest::AbortOperation
                        } else {
                            GitRequest::ContinueOperation
                        });
                    }
                }
                return None;
            }
            let faixa_do_commit = Self::faixa_do_commit(conteudo);
            if faixa_do_commit.contains(point) {
                for (id, area_do_botao) in Self::botoes_do_commit(faixa_do_commit) {
                    if !area_do_botao.contains(point) {
                        continue;
                    }
                    // Mensagem vazia não commita: o `git` recusaria, e a recusa
                    // chegaria como falha da ferramenta em vez de como o que é.
                    if self.mensagem.trim().is_empty() && id == COMMIT_ID {
                        return None;
                    }
                    self.escrevendo = false;
                    return Some(GitRequest::Commit {
                        message: self.mensagem.clone(),
                        amend: id == AMEND_ID,
                    });
                }
                // O resto da faixa é a caixa da mensagem.
                self.escrevendo = true;
                return None;
            }
            self.escrevendo = false;
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

    /// A roda dentro da janela. Devolve o pedido que ela provocou, se provocou.
    pub(super) fn scroll(
        &mut self,
        host: &UiHost,
        context: &LayoutContext,
        point: Point,
        linhas: f32,
    ) -> Option<GitRequest> {
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
            return None;
        }
        let conteudo = area(host, CONTENT_ID);
        if self.aba == Aba::History {
            if let Some(tabela) = self.tabela.as_mut() {
                tabela.layout(context, conteudo);
                tabela.event(&mut EventContext::default(), &evento);
            }
            // Perto do fim, a página seguinte: pedir só ao chegar nele faria a
            // rolagem parar e esperar.
            return self.precisa_de_mais_historico().then_some(GitRequest::LoadHistory {
                ja_carregados: self.view.commits.len(),
            });
        }
        // A roda é do painel sob o ponteiro, e não do primeiro que a aceite:
        // três listas empilhadas com uma barra cada só se distinguem pela área.
        if !conteudo.contains(point) {
            return None;
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
            return None;
        }
        None
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

        // A faixa de criar branch, embaixo da árvore.
        let nova = area(host, NOVA_ID);
        let mut nome = TextInput::new(NOVA_ID, &self.nome_novo).with_placeholder("Nova branch");
        if self.nomeando {
            nome.event(&mut EventContext::default(), &UiEvent::FocusGained);
        }
        nome.layout(
            layout,
            Rect::new(
                nova.origin.x,
                nova.origin.y,
                (nova.size.width - ACAO_LARGURA - 4.0).max(0.0),
                nova.size.height,
            ),
        );
        nome.paint(paint);
        let mut criar = Button::new(CRIAR_ID, "Criar");
        criar.set_disabled(self.nome_novo.trim().is_empty());
        criar.layout(
            layout,
            Rect::new(
                nova.origin.x + nova.size.width - ACAO_LARGURA,
                nova.origin.y,
                ACAO_LARGURA,
                nova.size.height,
            ),
        );
        criar.paint(paint);

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
        if self.aba == Aba::History && self.view.has_repository() {
            if self.view.commits.is_empty() {
                let mut vazio = Label::new(
                    SUMMARY_BASE,
                    "Sem commits ainda".to_owned(),
                )
                .with_tone(IconTint::Muted);
                vazio.layout(layout, conteudo);
                vazio.paint(paint);
                return true;
            }
            if let Some(tabela) = self.tabela.as_mut() {
                tabela.layout(layout, conteudo);
                tabela.paint(paint);
            }
            return true;
        }
        if self.aba == Aba::Status && self.view.has_repository() {
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
        self.paint_conflito(conteudo, layout, paint);
        self.paint_commit(conteudo, layout, paint);
        if self.view.changed == 0 {
            let mut limpo = Label::new(
                WidgetId(SUMMARY_BASE.0 + 9),
                "Nada mudou desde o último commit".to_owned(),
            )
            .with_tone(IconTint::Muted);
            let sobra = self.faixa_do_conflito(conteudo).map_or(conteudo, |faixa| {
                Rect::new(
                    conteudo.origin.x,
                    faixa.origin.y + faixa.size.height,
                    conteudo.size.width,
                    (conteudo.size.height - faixa.size.height).max(0.0),
                )
            });
            limpo.layout(layout, sobra);
            limpo.paint(paint);
            return;
        }
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

    /// A faixa do estado intermediário, quando há um.
    ///
    /// Ela diz **qual** operação está no meio do caminho e **quantos** arquivos
    /// faltam resolver, e traz os dois botões de saída. Enquanto ela estiver na
    /// tela, há por onde sair — que é o critério da fase.
    fn paint_conflito(&mut self, conteudo: Rect, layout: &LayoutContext, paint: &mut PaintContext) {
        let Some(faixa) = self.faixa_do_conflito(conteudo) else {
            return;
        };
        let Some(operacao) = self.view.pending.clone() else {
            return;
        };
        let conflitos = self
            .view
            .entries
            .iter()
            .filter(|entrada| entrada.state == GitFileState::Conflicted)
            .count();
        let texto = if conflitos == 0 {
            format!("{operacao} em curso")
        } else {
            format!("{operacao} em curso — {conflitos} arquivo(s) em conflito")
        };
        let mut rotulo = Label::new(WidgetId(SUMMARY_BASE.0 + 8), texto).with_tone(IconTint::Warning);
        rotulo.layout(
            layout,
            Rect::new(faixa.origin.x, faixa.origin.y + 4.0, faixa.size.width, 20.0),
        );
        rotulo.paint(paint);
        for (id, area_do_botao) in Self::botoes_do_conflito(faixa) {
            let rotulo = if id == ABORTAR_ID { "Abortar" } else { "Continuar" };
            let mut botao = Button::new(id, rotulo);
            // Continuar com conflito por resolver não conclui nada: o `git`
            // recusa, e a recusa chegaria como falha da ferramenta.
            botao.set_disabled(id == CONTINUAR_ID && conflitos > 0);
            botao.layout(layout, area_do_botao);
            botao.paint(paint);
        }
    }

    /// A caixa da mensagem e os dois botões, embaixo dos três painéis.
    ///
    /// A caixa é um `TextInput` da biblioteca, como as outras da IDE: o que
    /// muda é onde ela fica e quem lê o que se digitou.
    fn paint_commit(&mut self, conteudo: Rect, layout: &LayoutContext, paint: &mut PaintContext) {
        let faixa = Self::faixa_do_commit(conteudo);
        let mut campo = TextInput::new(MENSAGEM_ID, &self.mensagem)
            .with_placeholder("Mensagem do commit");
        if self.escrevendo {
            campo.event(&mut EventContext::default(), &UiEvent::FocusGained);
        }
        campo.layout(
            layout,
            Rect::new(faixa.origin.x, faixa.origin.y, faixa.size.width, 28.0),
        );
        campo.paint(paint);
        for (id, area_do_botao) in Self::botoes_do_commit(faixa) {
            let rotulo = if id == COMMIT_ID { "Commit" } else { "Amend" };
            let mut botao = Button::new(id, rotulo);
            // Sem mensagem não há o que commitar, e o botão diz isso pelo
            // próprio desenho em vez de deixar o `git` recusar depois.
            botao.set_disabled(id == COMMIT_ID && self.mensagem.trim().is_empty());
            botao.layout(layout, area_do_botao);
            botao.paint(paint);
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
            // Com repositório, quem desenha é `paint_status`: estas linhas são o
            // que sobra para os casos em que não há painel nenhum a mostrar.
            Aba::Status => vec![format!("{} arquivo(s) alterado(s)", self.view.changed)],
            Aba::History => vec!["Sem commits ainda".to_owned()],
        }
    }

    /// A altura de cada linha do histórico, para o teste ver a linha crescer.
    #[cfg(test)]
    pub(super) fn alturas_do_historico(&self) -> Vec<f32> {
        self.tabela
            .as_ref()
            .map(ComposedTable::row_heights)
            .unwrap_or_default()
    }

    /// A faixa do estado intermediário, para o teste apontar nos botões dela.
    #[cfg(test)]
    pub(super) fn faixa_do_conflito_para_teste(&self, host: &UiHost) -> Option<Rect> {
        self.faixa_do_conflito(area(host, CONTENT_ID))
    }

    /// O nome que se está escrevendo para a branch nova.
    #[cfg(test)]
    pub(super) fn nome_novo(&self) -> &str {
        &self.nome_novo
    }

    /// Onde ficam a caixa da mensagem e os botões, para o teste apontar neles.
    #[cfg(test)]
    pub(super) fn faixa_do_commit_para_teste(&self, host: &UiHost) -> Rect {
        Self::faixa_do_commit(area(host, CONTENT_ID))
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
        let precisa_de_retrato = !matches!(
            pedido,
            GitRequest::ShowDiff { .. } | GitRequest::LoadHistory { .. }
        );
        // Trocar de branch, fundir e sair de uma operação mudam **o histórico
        // que está na tela**, e não só a lista de arquivos: a linha de cima
        // passa a ser outra. Recarregar do começo é a única resposta certa.
        let mexeu_no_historico = matches!(
            pedido,
            GitRequest::SwitchBranch(_)
                | GitRequest::CreateBranch(_)
                | GitRequest::Merge(_)
                | GitRequest::ContinueOperation
                | GitRequest::AbortOperation
                // `pull` traz commits; `fetch` e `push` só mexem em referência,
                // e a contagem à frente e atrás sai do retrato.
                | GitRequest::Pull
        );
        // Commitar esvazia a caixa **agora**, e não quando a resposta chegar: a
        // mensagem já foi usada, e deixá-la na tela convida a commitar duas
        // vezes o mesmo texto.
        let commitou = matches!(pedido, GitRequest::Commit { .. });
        if commitou {
            self.git.limpar_mensagem();
        }
        self.commands.push(ApplicationCommand::Git(pedido));
        if precisa_de_retrato {
            self.commands
                .push(ApplicationCommand::Git(GitRequest::Refresh));
        }
        if commitou || mexeu_no_historico {
            // O histórico ganhou uma linha, e a de cima pode ter sido reescrita
            // por um `amend`: recarregar do começo é a única resposta certa
            // para os dois casos.
            self.commands
                .push(ApplicationCommand::Git(GitRequest::LoadHistory {
                    ja_carregados: 0,
                }));
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
                    .with_command(acao.comando())
                    .with_height(ALTURA_NA_LINHA),
            ),
            CellWidth::Fixed(ACAO_LARGURA),
        ));
    }
    ComposedRow::new(celulas)
}

/// Largura da coluna do grafo.
///
/// Fixa, e não `Natural`: a coluna do grafo mediria a linha mais larga da página
/// inteira, e uma fusão distante empurraria a descrição de todas as outras.
const GRAFO_LARGURA: f32 = 72.0;

/// A linha de um commit: o grafo, a descrição, a data, o autor e o hash.
///
/// **Quem monta as células é a IDE**, e a célula do grafo recebe as faixas já
/// calculadas: a biblioteca desenha o ponto e o traço, e não sabe o que é um
/// commit.
fn linha_de_commit(indice: usize, commit: &CommitRow, lanes: usize) -> ComposedRow {
    let base = COMMIT_BASE.0 + indice as u64 * 8;
    let hash = commit.hash.chars().take(7).collect::<String>();
    let rotulo = |deslocamento: u64, texto: &str, tom: IconTint| {
        ComposedCell::new(
            Box::new(Label::new(WidgetId(base + deslocamento), texto).with_tone(tom)),
            CellWidth::Fill,
        )
    };
    // **A descrição quebra; as outras três, não.** Mensagem de commit é a única
    // coluna sem tamanho previsível — data, autor e hash cabem sempre —, e é a
    // única em que o resto da linha some por baixo da coluna vizinha quando não
    // cabe. Quem decide isto é a IDE: a biblioteca não sabe se há para onde
    // crescer, e a linha da tabela cresce junto com a célula mais alta.
    ComposedRow::new(vec![
        ComposedCell::new(
            Box::new(
                GraphCell::new(WidgetId(base), commit.lane, lanes.max(commit.lanes))
                    .with_passing(commit.passing.clone())
                    .with_parents(commit.parents.clone())
                    // A primeira linha da página não tem commit acima: um traço
                    // saindo dela prometeria o que não está na tela.
                    .with_incoming(indice > 0),
            ),
            CellWidth::Fixed(GRAFO_LARGURA),
        ),
        ComposedCell::new(
            Box::new(
                Label::new(WidgetId(base + 1), &commit.summary)
                    .with_tone(IconTint::Text)
                    .with_wrap(true),
            ),
            CellWidth::Fill,
        ),
        rotulo(2, &commit.date, IconTint::Muted),
        rotulo(3, &commit.author, IconTint::Muted),
        rotulo(4, &hash, IconTint::Muted),
    ])
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
fn linha(indice: usize, branch: &BranchItem) -> ComposedRow {
    let base = ROW_BASE.0 + 100 + indice as u64 * 6;
    let atual = branch.current;
    let mut celulas = vec![
        ComposedCell::new(
            Box::new(
                Label::new(WidgetId(base), if atual { "●" } else { "" })
                    .with_tone(IconTint::Accent),
            ),
            CellWidth::Fixed(14.0),
        ),
        ComposedCell::new(
            Box::new(Label::new(WidgetId(base + 1), &branch.name)),
            CellWidth::Fill,
        ),
    ];
    // A contagem só aparece quando há o que contar: um `↑0 ↓0` fixo em toda
    // linha seria ruído, e quem não tem upstream não tem contagem nenhuma.
    if branch.ahead > 0 || branch.behind > 0 {
        let mut texto = String::new();
        if branch.ahead > 0 {
            texto.push_str(&format!("↑{} ", branch.ahead));
        }
        if branch.behind > 0 {
            texto.push_str(&format!("↓{}", branch.behind));
        }
        celulas.push(ComposedCell::new(
            Box::new(
                Label::new(WidgetId(base + 4), texto.trim_end()).with_tone(IconTint::Warning),
            ),
            CellWidth::Fixed(56.0),
        ));
    }
    // **A branch atual não oferece trocar nem fundir.** Trocar para onde já se
    // está não faz nada, e fundir uma branch nela mesma é um comando que o
    // `git` recusa — oferecer os dois seria oferecer erro.
    // A branch atual troca as duas ações pelas do remoto: **empurrar e puxar só
    // fazem sentido onde se está**, e trocar para onde já se está não faz nada.
    let acoes: &[(u64, &str, &str)] = if atual {
        &[(2, "Pull", "git.pull"), (3, "Push", "git.push")]
    } else {
        &[(2, "Trocar", "git.switch"), (3, "Fundir", "git.merge")]
    };
    for (deslocamento, rotulo, comando) in acoes {
        celulas.push(ComposedCell::new(
            Box::new(
                Button::new(WidgetId(base + deslocamento), *rotulo)
                    .with_command(*comando)
                    .with_height(ALTURA_NA_LINHA),
            ),
            CellWidth::Fixed(ACAO_DA_ARVORE),
        ));
    }
    ComposedRow::new(celulas)
}

/// A linha da raiz dos remotos: o nome e o botão de buscar.
fn linha_do_remoto(nome: &str) -> ComposedRow {
    ComposedRow::new(vec![
        ComposedCell::new(
            Box::new(Label::new(WidgetId(ROW_BASE.0 + 2), nome)),
            CellWidth::Fill,
        ),
        ComposedCell::new(
            Box::new(
                Button::new(WidgetId(ROW_BASE.0 + 3), "Fetch")
                    .with_command("git.fetch")
                    .with_height(ALTURA_NA_LINHA),
            ),
            CellWidth::Fixed(ACAO_DA_ARVORE),
        ),
    ])
}
