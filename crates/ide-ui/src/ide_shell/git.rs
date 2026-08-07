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
    Button, ButtonAlign, ButtonFill, CellWidth, ComposedCell, ComposedList, ComposedRow, ComposedTable, ComposedTreeItem,
    ComposedTreeView, GraphCell, Icon, IconTint, Label, ModalHost, Panel, SplitOrientation,
    SplitPane, SurfaceTone, TabItem, TableColumn, Tabs, TextInput, Toolbar, ToolbarAlign,
};
use ui_core::{Constraints, Modifiers, Point, Spacing, Rect, ScrollEvent, Size, TokenKind, UiEvent, WidgetId};
use ui_host::UiHost;
use ui_layout_api::{EdgeInsets, LayoutDirection, LayoutStyle};

use ide_application::{ApplicationCommand, GitRequest};
use ide_domain::DocumentId;
use std::path::PathBuf;

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
/// As abas da janela inteira, e o painel da segunda.
const JANELA_TABS_ID: WidgetId = WidgetId(10_531);
const DIFF_ID: WidgetId = WidgetId(10_532);
/// A divisa que reparte as duas colunas da comparação.
const DIFF_SPLIT_ID: WidgetId = WidgetId(10_533);
/// Um trecho classificado de uma linha, na travessia entre quem analisa e a tela.
///
/// Mesma razão do [`GitSpan`]: uma tupla de três não diz qual número é qual, e
/// esta é a fronteira onde um trocado passa calado.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GitToken {
    pub start: usize,
    pub end: usize,
    pub kind: TokenKind,
}

/// O nome pelo qual um lado da comparação é conhecido por quem analisa.
///
/// **A extensão é a do arquivo, e o nome não é.** A extensão é o que decide qual
/// linguagem responde; o nome tem de ser outro porque o arquivo de verdade pode
/// estar aberto ao mesmo tempo, e dois documentos com o mesmo caminho são o
/// mesmo documento para quem analisa — o realce de um cairia no outro.
fn caminho_do_lado(path: &std::path::Path, lado: &str) -> PathBuf {
    let extensao = path.extension().and_then(|extensao| extensao.to_str());
    let tronco = path
        .file_stem()
        .and_then(|tronco| tronco.to_str())
        .unwrap_or("diff");
    let nome = match extensao {
        Some(extensao) => format!("{tronco}.git-{lado}.{extensao}"),
        None => format!("{tronco}.git-{lado}"),
    };
    path.parent()
        .map_or_else(|| PathBuf::from(&nome), |pasta| pasta.join(&nome))
}

/// Os dois textos da comparação, como documentos para quem analisa linguagem.
///
/// **O realce vem de quem entende a linguagem, e quem entende a linguagem
/// trabalha sobre documentos.** Os dois lados de uma comparação não são
/// documentos abertos — o de então nem existe no disco —, e sem um nome próprio
/// não haveria a quem perguntar de que cor é cada palavra.
///
/// Os números ficam no fim da faixa de propósito: os documentos de verdade são
/// contados a partir do zero, e um encontro entre os dois faria o realce de um
/// arquivo cair no outro.
const DIFF_DOC_ENTAO: DocumentId = DocumentId(u64::MAX - 1);
const DIFF_DOC_AGORA: DocumentId = DocumentId(u64::MAX);

/// As setas que levam uma linha da esquerda para a direita: uma por linha.
///
/// Longe de tudo de propósito. As duas colunas da comparação já reservam cem mil
/// identificadores cada uma a partir de `DIFF_LINHA_BASE`, e uma base de setas
/// logo acima dos botões da barra colidiria com elas na sexagésima linha do
/// arquivo — dois widgets com o mesmo identificador são o mesmo widget para a
/// moldura, e o estado de um cairia no outro.
const APLICAR_BASE: WidgetId = WidgetId(1_000_000);
/// Os comandos do cabeçalho da comparação: anterior, seguinte, e o lado.
const DIFF_ANTERIOR_ID: WidgetId = WidgetId(11_950);
const DIFF_SEGUINTE_ID: WidgetId = WidgetId(11_951);
const DIFF_LADO_ID: WidgetId = WidgetId(11_952);
const DIFF_CONTAGEM_ID: WidgetId = WidgetId(11_953);
const DIFF_BARRA_ID: WidgetId = WidgetId(11_954);
/// Largura e altura das setas que flutuam sobre o texto.
///
/// Menor que a altura padrão de propósito: elas moram **dentro** de uma linha de
/// texto, e um botão de quarenta pontos cobriria as duas vizinhas. Não menor do
/// que isto: um alvo de clique tem de ser alvo.
const APLICAR_LADO: f32 = 26.0;
/// O tamanho do glifo das setas.
///
/// Maior que o padrão dos botões, e por isso pedido: o padrão encolhe para
/// caber, o que é certo para rótulo — palavras se leem mesmo pequenas — e errado
/// para um glifo solto, que pequeno vira um risco na tela.
const APLICAR_GLIFO: f32 = 20.0;
/// As linhas da comparação: uma célula por linha de texto.
const DIFF_LINHA_BASE: WidgetId = WidgetId(12_000);
/// O botão que fecha a janela, no canto de cima.
const FECHAR_ID: WidgetId = WidgetId(11_930);
/// A barra de ferramentas do alto, e os três botões dela.
const TOOLBAR_ID: WidgetId = WidgetId(10_530);
const FETCH_ID: WidgetId = WidgetId(11_920);
const PULL_ID: WidgetId = WidgetId(11_921);
const PUSH_ID: WidgetId = WidgetId(11_922);
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
const PANEL_SIZE: Size = Size::new(1350.0, 680.0);
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
/// A faixa do alto da comparação: o título e os comandos dela.
///
/// Mais alta que a do resumo porque tem botões, e botão de vinte pontos não é
/// alvo de clique — é um risco na tela que por acaso responde.
const DIFF_CABECALHO: f32 = 30.0;
/// Largura dos botões que andam entre as alterações.
///
/// Fixa porque o conteúdo é um glifo, e glifo não cresce. As outras larguras do
/// cabeçalho **não** são fixas: elas saem da medida do texto, que é o único jeito
/// de dois componentes não se sobreporem quando o texto muda de tamanho.
const DIFF_PASSO_LARGURA: f32 = 34.0;
/// Largura de um botão de ação de linha.
const ACAO_LARGURA: f32 = 92.0;
/// Quanto a barra do alto se separa do que está em volta dela.
///
/// **A mesma medida em cima e embaixo**: uma faixa que respira de um lado só
/// parece colada no outro, e é o que ela parecia quando só tinha o espaço de
/// baixo. Metade a mais do que o padrão da biblioteca, e o pedido é desta tela —
/// o que vem logo abaixo é uma caixa de texto, e um campo encostado num botão
/// parece parte dele.
///
/// **Quem sabe fazer a distância é a `Toolbar`**; daqui sai só o quanto.
const ESPACO_DA_BARRA: f32 = Toolbar::SPACE_BELOW * 1.5;
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

/// Um trecho de uma linha, em caracteres.
///
/// Tem nome porque uma tupla de três números não diz qual é qual — e porque a
/// travessia entre o domínio e a tela é onde um número trocado passa calado.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct GitSpan {
    pub line: usize,
    pub start: usize,
    pub end: usize,
}

/// O que uma seta devolve, e no lugar de quê.
///
/// Os três juntos porque separados não dizem nada: a fileira é onde a seta é
/// desenhada, `from` é a faixa que entra do arquivo de então e `to` a que sai do
/// de agora. Faixas meio-abertas, e cada uma pode ser vazia — é o que faz esta
/// mesma estrutura servir para trocar, acrescentar **e apagar**.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Devolucao {
    pub(super) fileira: usize,
    pub(super) from: (usize, usize),
    pub(super) to: (usize, usize),
}

impl GitDiff {
    /// Se o que veio não é texto.
    ///
    /// **Um byte zero é a marca**, e é a mesma que o próprio Git usa para
    /// decidir isso. Não é infalível — um `utf-16` de verdade tem zeros —, e é
    /// o erro certo: um arquivo tratado como binário aparece com um aviso, e um
    /// binário tratado como texto enche a tela de lixo e oferece setas para
    /// devolvê-lo linha a linha.
    #[must_use]
    pub fn e_binario(&self) -> bool {
        self.committed.contains(' ') || self.current.contains(' ')
    }
}

/// Uma fileira da comparação: que linha aparece de cada lado.
///
/// **Os dois lados são opcionais.** Uma linha que só existe no arquivo de então
/// tem `new` vazio, e o lado direito daquela fileira fica em branco; uma que só
/// existe no de agora tem `old` vazio. É o que impede as duas colunas de
/// escorregarem uma em relação à outra na primeira inserção. Quem calcula é o
/// domínio, em `FileDiff::aligned_lines`: a tela não sabe ler um `diff`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct GitLinePair {
    pub old: Option<usize>,
    pub new: Option<usize>,
}

/// A comparação de um arquivo, como a aba `Diff` a mostra.
///
/// Os dois textos inteiros, e não os trechos: quem olha uma diferença costuma
/// querer ver o que está em volta dela, e recortar aqui obrigaria a pedir de
/// novo a cada rolagem.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct GitDiff {
    /// O caminho relativo, que é o que se lê no título.
    pub label: String,
    /// O caminho inteiro, que é o que uma ação sobre este arquivo carrega.
    ///
    /// Os dois, e não um: o de cima é para ler, e este é para agir. Guardar só
    /// o relativo obrigaria a tela a saber onde o projeto começa para remontar o
    /// outro — e a raiz é do shell.
    pub path: std::path::PathBuf,
    /// O arquivo como está no último commit.
    pub committed: String,
    /// O arquivo como está agora.
    pub current: String,
    /// As linhas do arquivo de agora que mudaram, para realçar o lado direito.
    pub marks: Vec<(usize, GitLineChange)>,
    /// As linhas do arquivo de então que saíram, para realçar o lado esquerdo.
    ///
    /// Números do **outro** arquivo: uma linha removida não existe no de agora,
    /// e é por isso que ela precisa de uma lista própria.
    pub removed: Vec<usize>,
    /// Os trechos acrescentados, dentro das linhas do arquivo de agora.
    pub added_spans: Vec<GitSpan>,
    /// Os trechos removidos, dentro das linhas do arquivo de então.
    pub removed_spans: Vec<GitSpan>,
    /// De que lado é esta comparação: o preparado, ou a árvore de trabalho.
    ///
    /// São duas diferenças distintas sobre o mesmo arquivo, e quem já preparou
    /// parte do trabalho precisa saber qual está vendo — senão conclui que o
    /// resto se perdeu.
    pub staged: bool,
    /// As fileiras: que linha de cada lado ocupa cada altura da tela.
    ///
    /// Vazio quer dizer "não sei emparelhar", e aí as duas colunas voltam a ser
    /// cada texto numerado do zero — que é o certo quando não há diferença
    /// nenhuma, e o menos errado quando a resposta do Git não veio.
    pub pairs: Vec<GitLinePair>,
}

/// O que a margem do editor mostra numa linha.
///
/// **Duas, e não três.** Onde entrou código a marca é verde, tendo saído algo
/// dali junto ou não: quem olha a margem quer saber onde há código novo para
/// reler. A vermelha fica para a linha que só perdeu.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GitLineChange {
    /// Recebeu código — tendo perdido algum ou não.
    Added,
    /// Só perdeu: a marca fica na linha que ficou no lugar.
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

/// Qual aba **da janela** está na frente.
///
/// São duas coisas diferentes das outras abas: aquelas repartem o lado direito
/// do trabalho, e estas repartem a janela inteira. `Geral` é tudo o que o
/// gerenciador já fazia; `Diff` é o lugar da comparação, que ainda vai ser
/// construída — e nasce vazio de propósito, como os nós da árvore nasceram: uma
/// aba que só aparece quando a capacidade chega faz a janela mudar de forma a
/// cada passo.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum AbaDaJanela {
    #[default]
    Geral,
    Diff,
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
    /// Qual aba da janela está na frente.
    aba_da_janela: AbaDaJanela,
    /// A comparação que a aba `Diff` mostra, quando alguém pediu uma.
    diff: Option<GitDiff>,
    /// As duas colunas da comparação, mantidas entre quadros: a rolagem é delas.
    colunas_do_diff: Option<[ComposedList; 2]>,
    /// O realce de cada lado, uma lista de trechos por linha.
    ///
    /// Chega depois das colunas: perguntar a que classe pertence cada palavra é
    /// trabalho de outra camada, e esperar por ela deixaria a comparação em
    /// branco no instante em que se pede.
    realce_do_diff: [Vec<Vec<(usize, usize, TokenKind)>>; 2],
    /// Quantas comparações já passaram por aqui.
    ///
    /// É a versão dos dois documentos acima. Sem ela, abrir a comparação de
    /// outro arquivo não avisaria ninguém — os identificadores são os mesmos, e
    /// quem compara versões concluiria que nada mudou.
    versao_do_diff: u64,
    /// As fileiras, os blocos e as setas, calculados quando a comparação muda.
    ///
    /// **Uma vez, e não a cada quadro.** Tudo isto sai da mesma resposta do Git,
    /// e nada disso muda entre um quadro e o seguinte — mas era refeito em todos:
    /// dois mapas de dez mil entradas, três varreduras e uma cópia da lista de
    /// fileiras, trinta vezes por segundo. Num arquivo de dez mil linhas o quadro
    /// levava 64 ms; medido, e não suposto.
    fileiras_do_diff: Vec<GitLinePair>,
    blocos_do_diff: Vec<(usize, usize)>,
    /// As setas de linha, em ordem de fileira — é o que deixa achar as visíveis
    /// por busca binária em vez de percorrer o arquivo inteiro.
    setas_do_diff: Vec<Devolucao>,
    /// As setas de trecho, e a última linha de então que cada uma leva.
    trechos_do_diff: Vec<(Devolucao, usize)>,
    /// Em que alteração se está, entre as que o arquivo tem.
    ///
    /// **Nenhuma quando a comparação abre**: a primeira tecla leva à primeira,
    /// e começar já dentro de uma faria a segunda parecer a primeira.
    bloco_atual: Option<usize>,
    split_do_diff: Option<SplitPane>,
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
    // As abas da janela vêm antes de tudo: elas repartem o painel inteiro, e o
    // que está embaixo delas é o conteúdo de uma delas.
    // Sem folga própria embaixo: quem afasta a barra das abas é a **barra**, e
    // duas folgas somadas dariam uma distância que ninguém escolheu.
    let _ = host.declare(
        MODAL_ID,
        JANELA_TABS_ID,
        LayoutStyle {
            height: Some(28.0),
            ..LayoutStyle::default()
        },
    );
    // A barra de ferramentas no alto, atravessando a janela inteira: o que
    // está nela vale para o **repositório**, e não para a linha em que alguém
    // clicou. Ver a `22`.
    // A altura da faixa é a que a barra pede — o botão mais alto **mais** o que
    // ela separa do que vem abaixo. Escrever o número aqui seria a mesma medida
    // em dois lugares.
    let _ = host.declare(
        MODAL_ID,
        TOOLBAR_ID,
        LayoutStyle {
            height: Some(Button::HEIGHT + ESPACO_DA_BARRA * 2.0),
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
    // O painel da aba `Diff`: ele ocupa o mesmo lugar do conteúdo da `Geral`, e
    // um dos dois está sempre escondido.
    let _ = host.declare(
        MODAL_ID,
        DIFF_ID,
        LayoutStyle {
            flex_grow: 1.0,
            hidden: true,
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

    /// Recebe a comparação e a põe na frente.
    ///
    /// **Ela abre a aba `Diff`, e não o editor.** A comparação é o assunto desta
    /// janela; abri-la atrás dela obrigava a fechar a janela para ver o que se
    /// tinha acabado de pedir — e quem tinha uma mensagem de commit escrita a
    /// perdia no caminho.
    pub(super) fn mostrar_diff(&mut self, diff: GitDiff) {
        // **Arquivo diferente começa do topo; o mesmo arquivo fica onde estava.**
        // Devolver uma linha refaz a comparação, e as colunas são remontadas —
        // jogar a rolagem para o topo a cada devolução fazia quem clicou perder
        // o lugar em que estava lendo, que é o lugar onde acabou de mexer.
        if self.diff.as_ref().map(|atual| &atual.path) != Some(&diff.path) {
            self.colunas_do_diff = None;
            self.bloco_atual = None;
        }
        self.diff = Some(diff);
        self.aba_da_janela = AbaDaJanela::Diff;
        // O realce é do arquivo anterior: mantê-lo pintaria as palavras deste
        // com as classes daquele, que é pior do que não pintar nada.
        self.realce_do_diff = [Vec::new(), Vec::new()];
        self.versao_do_diff += 1;
        self.rebuild_diff();
    }

    /// Guarda o realce de um dos lados, repartido por linha.
    ///
    /// Chega em deslocamentos do texto inteiro, e as linhas da comparação são
    /// desenhadas uma a uma: repartir aqui, uma vez, evita a mesma conta em cada
    /// linha de cada quadro.
    pub(super) fn set_realce_do_diff(&mut self, de_agora: bool, texto: &str, spans: &[GitToken]) {
        let mut por_linha: Vec<Vec<(usize, usize, TokenKind)>> = Vec::new();
        let mut inicio_da_linha = 0usize;
        for linha in texto.split('\n') {
            let comprimento = linha.chars().count();
            let fim_da_linha = inicio_da_linha + comprimento;
            por_linha.push(
                spans
                    .iter()
                    .filter(|token| token.end > inicio_da_linha && token.start < fim_da_linha)
                    .map(|token| {
                        (
                            token.start.max(inicio_da_linha) - inicio_da_linha,
                            token.end.min(fim_da_linha) - inicio_da_linha,
                            token.kind,
                        )
                    })
                    .collect(),
            );
            // O `+ 1` é a quebra, que conta no deslocamento e não é desenhada.
            inicio_da_linha = fim_da_linha + 1;
        }
        self.realce_do_diff[usize::from(de_agora)] = por_linha;
        self.rebuild_diff();
    }

    /// Se este documento é um dos dois lados da comparação.
    pub(super) const fn aceita_realce(&self, id: DocumentId) -> bool {
        id.0 == DIFF_DOC_ENTAO.0 || id.0 == DIFF_DOC_AGORA.0
    }

    /// De que lado vem este realce.
    pub(super) const fn realce_e_do_lado_de_agora(&self, id: DocumentId) -> bool {
        id.0 == DIFF_DOC_AGORA.0
    }

    /// O texto de um dos lados, para repartir o realce que chegou sobre ele.
    pub(super) fn texto_do_lado(&self, de_agora: bool) -> Option<String> {
        let diff = self.diff.as_ref()?;
        Some(if de_agora {
            diff.current.clone()
        } else {
            diff.committed.clone()
        })
    }

    /// Os dois textos da comparação, para quem sabe analisá-los.
    ///
    /// Vazio quando não há comparação nenhuma na tela: analisar um arquivo que
    /// ninguém está olhando é trabalho jogado fora.
    pub(super) fn textos_do_diff(&self) -> Vec<(DocumentId, PathBuf, u64, String)> {
        // Com a janela fechada, ninguém está olhando: analisar os dois textos
        // seria trabalho jogado fora, e some desta lista quem some da tela —
        // que é o que fecha os dois documentos do lado de quem analisa.
        if !self.is_open() {
            return Vec::new();
        }
        let Some(diff) = self.diff.as_ref() else {
            return Vec::new();
        };
        vec![
            (
                DIFF_DOC_ENTAO,
                caminho_do_lado(&diff.path, "then"),
                self.versao_do_diff,
                diff.committed.clone(),
            ),
            (
                DIFF_DOC_AGORA,
                caminho_do_lado(&diff.path, "now"),
                self.versao_do_diff,
                diff.current.clone(),
            ),
        ]
    }

    /// Monta as duas colunas da comparação, uma fileira por par de linhas.
    ///
    /// **Fileira, e não linha de texto.** As duas colunas mostram o mesmo número
    /// de fileiras, e cada uma delas tem a linha de então à esquerda e a de agora
    /// à direita — ou um vazio, do lado que não tem par. Enquanto cada coluna era
    /// o seu texto numerado do zero, a primeira inserção deslocava tudo o que
    /// vinha abaixo e as duas nunca mais se reencontravam.
    ///
    /// O lado direito é tingido pelo que mudou: é a mesma marcação da margem do
    /// editor, e ela já vem calculada de quem leu o repositório.
    fn rebuild_diff(&mut self) {
        self.recalcular_o_diff();
        let Some(diff) = self.diff.as_ref().filter(|diff| !diff.e_binario()) else {
            self.colunas_do_diff = None;
            return;
        };
        let marcas: std::collections::HashMap<usize, GitLineChange> =
            diff.marks.iter().copied().collect();
        let removidas: std::collections::HashSet<usize> = diff.removed.iter().copied().collect();
        // **A linha que mudou é azul dos dois lados; o que mudou dentro dela tem
        // cor própria.** São duas informações diferentes: uma diz onde olhar, e
        // a outra diz o que olhar. Pintar a linha inteira de verde ou vermelho
        // faria procurar o que mudou dentro do que foi marcado como mudado.
        let trechos = |lista: &[GitSpan]| {
            let mut mapa: std::collections::HashMap<usize, (usize, usize)> =
                std::collections::HashMap::new();
            for trecho in lista {
                mapa.insert(trecho.line, (trecho.start, trecho.end));
            }
            mapa
        };
        let trechos_de_agora = trechos(&diff.added_spans);
        let trechos_de_entao = trechos(&diff.removed_spans);
        let fileiras = self.fileiras_do_diff.clone();
        let Some(diff) = self.diff.as_ref() else {
            return;
        };
        let coluna = |texto: &str, base: u64, de_agora: bool| {
            let classes = &self.realce_do_diff[usize::from(de_agora)];
            let linhas: Vec<String> = texto.lines().map(ToOwned::to_owned).collect();
            let linhas = fileiras
                .iter()
                .enumerate()
                .map(|(fileira, par)| {
                    // Qual linha do texto esta fileira mostra — e nenhuma, se o
                    // lado está vazio. A fileira existe do mesmo jeito: é ela
                    // que segura o outro lado na altura certa.
                    let numero = if de_agora { par.new } else { par.old };
                    let Some(numero) = numero else {
                        return Self::fileira_vazia(base, fileira);
                    };
                    let linha = linhas.get(numero).cloned().unwrap_or_default();
                    // A mesma cor nos dois lados: o azul só diz que esta linha
                    // entrou na comparação.
                    let mudou = if de_agora {
                        matches!(marcas.get(&numero), Some(GitLineChange::Added))
                    } else {
                        removidas.contains(&numero)
                    };
                    let realce = mudou.then_some(IconTint::Accent);
                    // E o trecho, com a cor do que aconteceu com ele.
                    let trecho = if de_agora {
                        trechos_de_agora
                            .get(&numero)
                            .map(|(inicio, fim)| (*inicio, *fim, IconTint::Success))
                    } else {
                        trechos_de_entao
                            .get(&numero)
                            .map(|(inicio, fim)| (*inicio, *fim, IconTint::Danger))
                    };
                    let celulas = vec![
                        // O número da linha antes do texto, como no editor: sem
                        // ele, duas colunas lado a lado não se conferem. E é o
                        // número do **arquivo**, não o da fileira: é por ele que
                        // se acha a linha no editor.
                        ComposedCell::new(
                            Box::new(
                                Label::new(
                                    WidgetId(base + fileira as u64 * 2),
                                    format!("{:>4}", numero + 1),
                                )
                                .with_tone(IconTint::Muted),
                            ),
                            CellWidth::Fixed(40.0),
                        ),
                        ComposedCell::new(
                            Box::new({
                                let rotulo =
                                    Label::new(WidgetId(base + fileira as u64 * 2 + 1), linha);
                                // O realce da linguagem é a tinta; a marca do que
                                // mudou é o fundo. As duas convivem, e é isso que
                                // deixa ver **o que** mudou e **em que** mudou.
                                let rotulo = match classes.get(numero) {
                                    Some(classes) if !classes.is_empty() => {
                                        rotulo.with_syntax(classes.clone())
                                    }
                                    _ => rotulo,
                                };
                                match trecho {
                                    Some((inicio, fim, tint)) => {
                                        rotulo.with_marked_range(inicio, fim, tint)
                                    }
                                    None => rotulo,
                                }
                            }),
                            // `Natural`: a linha vale a largura que o texto pede,
                            // e é isso que dá o que rolar para a barra de baixo.
                            CellWidth::Natural,
                        ),
                    ];
                    let linha = ComposedRow::new(celulas);
                    match realce {
                        Some(tint) => linha.with_highlight(tint),
                        None => linha,
                    }
                })
                .collect();
            ComposedList::new(WidgetId(base), linhas).with_row_height(ROW_HEIGHT)
        };
        // Onde as colunas estavam, para as novas continuarem ali. Elas são
        // construídas do zero a cada resposta que chega — o realce chega depois
        // da comparação, e é outra remontagem —, e uma lista nova nasce no topo.
        let onde = self.colunas_do_diff.as_ref().map(|listas| {
            [
                (
                    listas[0].scroll_offset(),
                    listas[0].scroll_x(),
                    listas[0].selected(),
                ),
                (
                    listas[1].scroll_offset(),
                    listas[1].scroll_x(),
                    listas[1].selected(),
                ),
            ]
        });
        let mut novas = [
            coluna(&diff.committed, DIFF_LINHA_BASE.0, false),
            coluna(&diff.current, DIFF_LINHA_BASE.0 + 100_000, true),
        ];
        if let Some(onde) = onde {
            for (lista, (y, x, escolhida)) in novas.iter_mut().zip(onde) {
                lista.set_scroll_offset(y);
                lista.set_scroll_x(x);
                lista.set_selected(escolhida);
            }
        }
        self.colunas_do_diff = Some(novas);
    }

    /// A fileira do lado que não tem nada a mostrar.
    ///
    /// Ela existe para segurar o outro lado na altura certa, e por isso não pode
    /// ser simplesmente omitida. O fundo apagado a distingue de uma linha em
    /// branco do arquivo: uma diz "aqui não há nada deste lado", a outra é
    /// conteúdo, e confundir as duas é ler o arquivo errado.
    fn fileira_vazia(base: u64, fileira: usize) -> ComposedRow {
        ComposedRow::new(vec![
            ComposedCell::new(
                Box::new(Label::new(WidgetId(base + fileira as u64 * 2), String::new())),
                CellWidth::Fixed(40.0),
            ),
            ComposedCell::new(
                Box::new(Label::new(
                    WidgetId(base + fileira as u64 * 2 + 1),
                    String::new(),
                )),
                CellWidth::Natural,
            ),
        ])
        .with_highlight(IconTint::Muted)
    }

    /// As duas colunas da comparação, repartidas pela divisa.
    fn colunas_da_comparacao(&mut self, area_do_diff: Rect, context: &LayoutContext) -> [Rect; 2] {
        let split = self.split_do_diff.get_or_insert_with(|| {
            SplitPane::new(DIFF_SPLIT_ID, SplitOrientation::Horizontal, 0.5)
        });
        // O título fica acima das duas colunas, e a divisa reparte o que sobra.
        let corpo = Rect::new(
            area_do_diff.origin.x,
            area_do_diff.origin.y + DIFF_CABECALHO,
            area_do_diff.size.width,
            (area_do_diff.size.height - DIFF_CABECALHO).max(0.0),
        );
        split.layout(context, corpo);
        [split.first(), split.second()]
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
    pub(super) fn key(&mut self, key: &str, modifiers: Modifiers) -> bool {
        // `F7` anda entre as alterações, e `Shift+F7` volta — o mesmo gesto ao
        // contrário, como em toda busca desta IDE. Vem antes das caixas de
        // texto porque nenhuma delas escreve `F7`.
        if key.eq_ignore_ascii_case("f7") && self.aba_da_janela == AbaDaJanela::Diff {
            self.andar_entre_alteracoes(!modifiers.shift);
            return true;
        }
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
        // Uma aba de cada vez: o que não está na frente sai do arranjo, e não
        // fica desenhado por baixo do que está.
        let geral = self.aba_da_janela == AbaDaJanela::Geral;
        host.set_style(
            TOOLBAR_ID,
            LayoutStyle {
                height: Some(Button::HEIGHT + ESPACO_DA_BARRA * 2.0),
                hidden: !geral,
                ..LayoutStyle::default()
            },
        );
        host.set_style(
            BODY_ID,
            LayoutStyle {
                direction: LayoutDirection::Row,
                flex_grow: 1.0,
                hidden: !geral,
                ..LayoutStyle::default()
            },
        );
        host.set_style(
            DIFF_ID,
            LayoutStyle {
                flex_grow: 1.0,
                hidden: geral,
                ..LayoutStyle::default()
            },
        );
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

    /// Onde fica o botão que fecha a janela.
    ///
    /// No canto de cima à direita, dentro da faixa do título — que é onde quem
    /// usa qualquer janela procura por ele antes de procurar em qualquer outro
    /// lugar.
    fn botao_de_fechar(painel: Rect) -> Rect {
        Rect::new(
            painel.origin.x + painel.size.width - Button::HEIGHT - 8.0,
            painel.origin.y + 8.0,
            Button::HEIGHT,
            Button::HEIGHT,
        )
    }

    /// Onde ficam as setas que levam uma linha da esquerda para a direita.
    ///
    /// **Uma por linha marcada, e todas ao mesmo tempo.** A seta chegou a
    /// acompanhar a linha escolhida, e era pior: obrigava a escolher antes de
    /// agir, e quem lê uma comparação quer devolver duas ou três linhas seguidas
    /// sem clicar duas vezes em cada uma.
    ///
    /// Só as que estão na vista. Rolar a coluna não pode deixar uma seta presa na
    /// borda falando de uma linha que já saiu da tela, e desenhar as de fora
    /// custaria uma por linha do arquivo.
    fn botoes_de_aplicar(&self, coluna: Rect) -> Vec<(Devolucao, Rect)> {
        let (primeira, ultima) = self.faixa_visivel(coluna);
        let comeco = self
            .setas_do_diff
            .partition_point(|seta| seta.fileira < primeira);
        self.setas_do_diff
            .get(comeco..)
            .unwrap_or_default()
            .iter()
            .take_while(|seta| seta.fileira <= ultima)
            .filter_map(|seta| {
                let topo = self.topo_da_fileira(coluna, seta.fileira)?;
                Some((
                    *seta,
                    Rect::new(
                        coluna.origin.x + coluna.size.width - APLICAR_LADO - self.margem_das_setas(),
                        topo + (ROW_HEIGHT - APLICAR_LADO) / 2.0,
                        APLICAR_LADO,
                        APLICAR_LADO,
                    ),
                ))
            })
            .collect()
    }

    /// Põe as duas colunas na mesma altura depois de um gesto.
    ///
    /// **A roda já chegava às duas; o arrasto da barra, não.** Quem arrasta
    /// segura *uma* barra, e a outra coluna não recebe o gesto — ela ficava para
    /// trás, e a comparação deixava de comparar: a linha 40 de um lado ao lado
    /// de outra qualquer do outro.
    ///
    /// Quem mudou manda, e o lado é o de baixo. Se as duas mudaram — a roda —,
    /// já estão iguais e o segundo `if` não faz nada.
    fn casar_a_rolagem(&mut self, antes: [(f32, f32); 2]) {
        let Some(listas) = self.colunas_do_diff.as_mut() else {
            return;
        };
        let depois = [
            (listas[0].scroll_offset(), listas[0].scroll_x()),
            (listas[1].scroll_offset(), listas[1].scroll_x()),
        ];
        if depois[0] != antes[0] {
            listas[1].set_scroll_offset(depois[0].0);
            listas[1].set_scroll_x(depois[0].1);
        } else if depois[1] != antes[1] {
            listas[0].set_scroll_offset(depois[1].0);
            listas[0].set_scroll_x(depois[1].1);
        }
    }

    /// Onde as duas colunas estão agora, para saber qual delas se mexeu.
    fn rolagem_das_colunas(&self) -> [(f32, f32); 2] {
        self.colunas_do_diff.as_ref().map_or(
            [(0.0, 0.0); 2],
            |listas| {
                [
                    (listas[0].scroll_offset(), listas[0].scroll_x()),
                    (listas[1].scroll_offset(), listas[1].scroll_x()),
                ]
            },
        )
    }

    /// A área de dentro do painel da comparação.
    ///
    /// **O que sobra depois da moldura e do respiro dela.** Antes daqui, o
    /// conteúdo era posto na área inteira: o caminho do arquivo começava no
    /// mesmo ponto da borda, e a linha vertical à esquerda parecia parte do
    /// texto. Quanto respirar é o painel quem diz — escrever o número aqui faria
    /// esta tela respirar diferente da vizinha.
    ///
    /// Um lugar só, e não seis: a pintura, o clique, a roda e o arrasto
    /// perguntam todos aqui. Com a conta repetida, o desenho aparece num lugar e
    /// o clique cai noutro.
    fn area_util_do_diff(host: &UiHost, context: &LayoutContext) -> Rect {
        let mut painel = Self::painel_do_diff();
        painel.layout(context, area(host, DIFF_ID));
        painel.content()
    }

    /// O painel que emoldura a comparação.
    fn painel_do_diff() -> Panel {
        Panel::new(DIFF_ID, SurfaceTone::Surface).with_border()
    }

    /// A barra de comandos do cabeçalho: o lado da comparação e os dois passos.
    ///
    /// **Quem arruma é a barra**, com o espaçamento padrão dela: três botões
    /// postos à mão, com larguras escritas por mim, foi o que fez "Árvore de
    /// trabalho" passar por cima da contagem. A barra mede cada um e nenhum
    /// encosta no vizinho.
    ///
    /// Encostada na borda direita, que é a que não se move: o nome do arquivo
    /// cresce e encolhe, e ancorar nele faria os botões dançarem a cada arquivo.
    fn barra_do_cabecalho(&self, area_do_diff: Rect, context: &LayoutContext) -> Toolbar {
        let staged = self.diff.as_ref().is_some_and(|diff| diff.staged);
        let alto = DIFF_CABECALHO - 6.0;
        let mut barra = Toolbar::new(
            DIFF_BARRA_ID,
            vec![
                Button::new(
                    DIFF_LADO_ID,
                    if staged {
                        "Preparado"
                    } else {
                        "Árvore de trabalho"
                    },
                )
                .with_height(alto)
                .with_fill(ButtonFill::Transparent),
                Button::new(DIFF_ANTERIOR_ID, "\u{2191}")
                    .with_height(alto)
                    .with_width(DIFF_PASSO_LARGURA)
                    .with_fill(ButtonFill::Transparent),
                Button::new(DIFF_SEGUINTE_ID, "\u{2193}")
                    .with_height(alto)
                    .with_width(DIFF_PASSO_LARGURA)
                    .with_fill(ButtonFill::Transparent),
            ],
        )
        .with_align(ToolbarAlign::End)
        .with_space_below(0.0);
        barra.layout(
            context,
            Rect::new(
                area_do_diff.origin.x,
                area_do_diff.origin.y + 3.0,
                area_do_diff.size.width - 4.0,
                alto,
            ),
        );
        barra
    }

    /// As alterações do arquivo, cada uma como a faixa de fileiras que ocupa.
    ///
    /// **Uma alteração é um bloco, e não uma linha.** Trocar três linhas
    /// seguidas é *uma* coisa que aconteceu, e contá-las como três faria a
    /// contagem dizer doze onde quem olha vê quatro — e obrigaria a apertar a
    /// tecla três vezes para sair de um lugar só.
    fn blocos_alterados(&self) -> &[(usize, usize)] {
        &self.blocos_do_diff
    }

    /// Leva à alteração seguinte, ou à anterior. `true` quando havia para onde ir.
    ///
    /// Dá a volta nas duas pontas, como toda busca desta IDE: quem chega ao fim
    /// procurando alterações quer recomeçar, e não bater numa parede.
    pub(super) fn andar_entre_alteracoes(&mut self, adiante: bool) -> bool {
        let quantos = self.blocos_do_diff.len();
        if quantos == 0 {
            return false;
        }
        let escolhido = match (self.bloco_atual, adiante) {
            (None, true) => 0,
            (None, false) => quantos - 1,
            (Some(atual), true) => (atual + 1) % quantos,
            (Some(atual), false) => (atual + quantos - 1) % quantos,
        };
        self.bloco_atual = Some(escolhido);
        let (comeco, _) = self.blocos_do_diff[escolhido];
        // Duas linhas de folga acima: uma alteração encostada na borda de cima
        // não deixa ver de onde ela vem.
        let alvo = (comeco as f32 - 2.0).max(0.0) * ROW_HEIGHT;
        if let Some(listas) = self.colunas_do_diff.as_mut() {
            for lista in listas.iter_mut() {
                lista.set_scroll_offset(alvo);
            }
        }
        true
    }

    /// Onde se está e quantas há, para o cabeçalho dizer.
    fn contagem_das_alteracoes(&self) -> String {
        let total = self.blocos_alterados().len();
        if total == 0 {
            return "sem alterações".to_owned();
        }
        match (self.bloco_atual, total) {
            (Some(atual), _) => format!("{} de {total}", atual + 1),
            // Uma só não são "1 alterações": o plural errado num canto da tela é
            // do tamanho de qualquer outro descuido.
            (None, 1) => "1 alteração".to_owned(),
            (None, _) => format!("{total} alterações"),
        }
    }

    /// A seta do trecho inteiro, no alto de cada bloco de mais de uma linha.
    ///
    /// **Só quando o bloco tem mais de uma linha.** Num bloco de uma, ela faria
    /// exatamente o que a seta da linha já faz, e duas setas iguais lado a lado
    /// só fazem parar para descobrir qual é qual.
    ///
    /// Fica ao lado da seta da linha, com a barra a mais no desenho: é o mesmo
    /// gesto, para mais coisa de uma vez.
    fn botoes_de_trecho(&self, coluna: Rect) -> Vec<(Devolucao, usize, Rect)> {
        let (primeira, ultima) = self.faixa_visivel(coluna);
        let comeco = self
            .trechos_do_diff
            .partition_point(|(seta, _)| seta.fileira < primeira);
        self.trechos_do_diff
            .get(comeco..)
            .unwrap_or_default()
            .iter()
            .take_while(|(seta, _)| seta.fileira <= ultima)
            .filter_map(|(seta, fim)| {
                let topo = self.topo_da_fileira(coluna, seta.fileira)?;
                Some((
                    *seta,
                    *fim,
                    Rect::new(
                        coluna.origin.x + coluna.size.width
                            - APLICAR_LADO * 2.0
                            - self.margem_das_setas()
                            - Spacing::XS,
                        topo + (ROW_HEIGHT - APLICAR_LADO) / 2.0,
                        APLICAR_LADO,
                        APLICAR_LADO,
                    ),
                ))
            })
            .collect()
    }

    /// Refaz tudo o que se deduz de uma comparação nova.
    ///
    /// Chamado quando a comparação chega, e só então: nada disto muda entre dois
    /// quadros, e refazê-lo em cada um era o que fazia um arquivo de dez mil
    /// linhas custar 64 ms por quadro.
    fn recalcular_o_diff(&mut self) {
        let Some(diff) = self.diff.as_ref() else {
            self.fileiras_do_diff = Vec::new();
            self.blocos_do_diff = Vec::new();
            self.setas_do_diff = Vec::new();
            self.trechos_do_diff = Vec::new();
            return;
        };
        // As fileiras: as que o domínio emparelhou, ou cada texto numerado do
        // zero quando não há diferença nenhuma a emparelhar.
        self.fileiras_do_diff = if diff.pairs.is_empty() {
            let de_entao = diff.committed.lines().count();
            let de_agora = diff.current.lines().count();
            (0..de_entao.max(de_agora))
                .map(|numero| GitLinePair {
                    old: (numero < de_entao).then_some(numero),
                    new: (numero < de_agora).then_some(numero),
                })
                .collect()
        } else {
            diff.pairs.clone()
        };

        let marcas: std::collections::HashMap<usize, GitLineChange> =
            diff.marks.iter().copied().collect();
        let removidas: std::collections::HashSet<usize> = diff.removed.iter().copied().collect();
        let mudou = |par: &GitLinePair| match (par.old, par.new) {
            // Um lado vazio é linha que entrou ou saiu: mudou, sempre.
            (None, _) | (_, None) => true,
            (Some(antiga), Some(nova)) => {
                removidas.contains(&antiga)
                    || matches!(marcas.get(&nova), Some(GitLineChange::Added))
            }
        };

        let mut blocos: Vec<(usize, usize)> = Vec::new();
        let mut setas: Vec<Devolucao> = Vec::new();
        // Onde estava cada lado ao chegar nesta fileira. Serve para a faixa
        // vazia dizer **onde**: uma linha que só existe de um lado não tem
        // número do outro, e sem isso não haveria posição a que se referir.
        let mut proxima_antiga = 0usize;
        let mut proxima_nova = 0usize;
        for (fileira, par) in self.fileiras_do_diff.iter().enumerate() {
            let antiga = par.old.unwrap_or(proxima_antiga);
            let nova = par.new.unwrap_or(proxima_nova);
            if mudou(par) {
                match blocos.last_mut() {
                    // Encostada na anterior: é a mesma alteração continuando.
                    Some(ultimo) if ultimo.1 + 1 == fileira => ultimo.1 = fileira,
                    _ => blocos.push((fileira, fileira)),
                }
                // **A seta existe em toda alteração, e não só onde saiu código.**
                // Faltava justamente o contrário: uma linha acrescentada não tem
                // par do lado de então, e devolvê-la é *apagá-la* — que é o
                // gesto mais comum de todos, o de desfazer o que se acabou de
                // escrever. Sem ela, a comparação mostrava a linha nova e não
                // oferecia nada.
                let entra = par.old.map_or((antiga, antiga), |linha| (linha, linha + 1));
                let sai = par.new.map_or((nova, nova), |linha| (linha, linha + 1));
                setas.push(Devolucao {
                    fileira,
                    from: entra,
                    to: sai,
                });
            }
            if let Some(linha) = par.old {
                proxima_antiga = linha + 1;
            }
            if let Some(linha) = par.new {
                proxima_nova = linha + 1;
            }
        }

        // As setas de trecho saem dos blocos: a faixa inteira que entra e a
        // inteira que sai, de uma vez.
        let mut trechos = Vec::new();
        for (comeco, fim) in &blocos {
            let no_bloco: Vec<&Devolucao> = setas
                .iter()
                .filter(|seta| seta.fileira >= *comeco && seta.fileira <= *fim)
                .collect();
            let (Some(primeira), Some(ultima)) = (no_bloco.first(), no_bloco.last()) else {
                continue;
            };
            // Bloco de uma fileira só não ganha seta de trecho: ela faria o que
            // a seta da linha já faz, e duas iguais lado a lado só fazem parar
            // para descobrir qual é qual.
            if no_bloco.len() < 2 {
                continue;
            }
            trechos.push((
                Devolucao {
                    fileira: *comeco,
                    from: (primeira.from.0, ultima.from.1),
                    to: (primeira.to.0, ultima.to.1),
                },
                *fim,
            ));
        }

        self.blocos_do_diff = blocos;
        self.setas_do_diff = setas;
        self.trechos_do_diff = trechos;
    }

    /// A faixa de fileiras que cabe na coluna, dado o que já rolou.
    ///
    /// As setas vêm em ordem de fileira, e por isso as visíveis se acham por
    /// busca binária: sem ela, cada quadro percorreria o arquivo inteiro para
    /// desenhar as poucas que estão na tela.
    fn faixa_visivel(&self, coluna: Rect) -> (usize, usize) {
        let rolagem = self
            .colunas_do_diff
            .as_ref()
            .and_then(|colunas| colunas.first())
            .map_or(0.0, ComposedList::scroll_offset);
        let primeira = (rolagem / ROW_HEIGHT).floor().max(0.0) as usize;
        let quantas = (coluna.size.height / ROW_HEIGHT).ceil() as usize + 1;
        (primeira, primeira + quantas)
    }

    /// A que distância da borda direita as setas ficam.
    ///
    /// Fora da trilha da barra de rolagem, quando há uma: encostadas nela, as
    /// setas cobriam quatro dos dez pontos da trilha, e ali o clique passava a
    /// ser delas — a barra deixava de se arrastar naquele trecho. Quanto a barra
    /// ocupa é a lista quem diz.
    fn margem_das_setas(&self) -> f32 {
        self.colunas_do_diff
            .as_ref()
            .and_then(|colunas| colunas.first())
            .map_or(0.0, ComposedList::gutter)
            + 6.0
    }

    /// Onde uma fileira aparece na coluna, se aparecer inteira.
    fn topo_da_fileira(&self, coluna: Rect, fileira: usize) -> Option<f32> {
        let rolagem = self
            .colunas_do_diff
            .as_ref()
            .and_then(|colunas| colunas.first())
            .map_or(0.0, ComposedList::scroll_offset);
        let topo = coluna.origin.y + fileira as f32 * ROW_HEIGHT - rolagem;
        (topo >= coluna.origin.y && topo + ROW_HEIGHT <= coluna.origin.y + coluna.size.height)
            .then_some(topo)
    }

    /// O caminho do arquivo que está sendo comparado.
    fn caminho_do_diff(&self) -> Option<std::path::PathBuf> {
        self.diff.as_ref().map(|diff| diff.path.clone())
    }

    /// A comparação que está na tela, para quem precisa do texto dela.
    pub(super) fn view_diff(&self) -> Option<&GitDiff> {
        self.diff.as_ref()
    }

    /// A comparação, quando há uma pedida.
    ///
    /// Sem nenhuma, o painel fica vazio com uma linha dizendo por onde se pede
    /// uma — um painel vazio sem explicação parece defeito.
    fn paint_diff(&mut self, area_do_diff: Rect, layout: &LayoutContext, paint: &mut PaintContext) {
        // Binário não se compara linha a linha: desenhar os bytes de um `.png`
        // como texto enche a tela de lixo e ainda oferece setas para devolvê-lo.
        if self.diff.as_ref().is_some_and(GitDiff::e_binario) {
            let mut aviso = Label::new(
                WidgetId(SUMMARY_BASE.0 + 8),
                "Arquivo binário: não há comparação linha a linha".to_owned(),
            )
            .with_tone(IconTint::Muted);
            aviso.layout(layout, area_do_diff);
            aviso.paint(paint);
            return;
        }
        let Some(titulo) = self.diff.as_ref().map(|diff| diff.label.clone()) else {
            let mut vazio = Label::new(
                WidgetId(SUMMARY_BASE.0 + 7),
                "Escolha um arquivo na aba Geral para ver a diferença".to_owned(),
            )
            .with_tone(IconTint::Muted);
            vazio.layout(layout, area_do_diff);
            vazio.paint(paint);
            return;
        };
        // **Da direita para a esquerda, cada um no espaço que sobrou do
        // anterior.** É o que impede dois componentes de ocuparem o mesmo lugar:
        // a barra mede os botões dela, a contagem mede o texto dela, e o nome do
        // arquivo fica com o resto — encurtado com reticências se não couber.
        let barra = self.barra_do_cabecalho(area_do_diff, layout);
        barra.paint(paint);

        let meio = area_do_diff.origin.y + (DIFF_CABECALHO - TITULO_ALTURA) / 2.0;
        let mut contagem =
            Label::new(DIFF_CONTAGEM_ID, self.contagem_das_alteracoes()).with_tone(IconTint::Muted);
        let largura_da_contagem = contagem
            .measure(&layout.measuring(), Constraints::UNBOUNDED)
            .width;
        let contagem_x = barra.left_edge() - Toolbar::GAP - largura_da_contagem;
        contagem.layout(
            layout,
            Rect::new(contagem_x, meio, largura_da_contagem, TITULO_ALTURA),
        );
        contagem.paint(paint);

        // O caminho é comprido e o espaço é o que sobrou: reticências no fim são
        // melhores que letras passando por baixo da contagem.
        let sobra = (contagem_x - Toolbar::GAP - area_do_diff.origin.x).max(0.0);
        let mut cabecalho = Label::new(WidgetId(SUMMARY_BASE.0 + 6), titulo).with_max_width(sobra);
        cabecalho.layout(
            layout,
            Rect::new(area_do_diff.origin.x, meio, sobra, TITULO_ALTURA),
        );
        cabecalho.paint(paint);

        let colunas = self.colunas_da_comparacao(area_do_diff, layout);
        if let Some(listas) = self.colunas_do_diff.as_mut() {
            for (lista, coluna) in listas.iter_mut().zip(colunas) {
                lista.layout(layout, coluna);
                lista.paint(paint);
            }
        }
        // A divisa por último: ela fica **sobre** a borda entre as duas colunas.
        if let Some(split) = self.split_do_diff.as_ref() {
            split.paint(paint);
        }
        // E as setas depois dela: elas flutuam sobre o texto, e o que flutua é
        // desenhado por cima do que está embaixo.
        for (devolucao, _, area_do_botao) in self.botoes_de_trecho(colunas[0]) {
            let mut trecho = Button::new(
                WidgetId(APLICAR_BASE.0 + 500_000 + devolucao.fileira as u64),
                "⇒",
            )
            .with_fill(ButtonFill::Transparent)
            // **Preto, e não a cor de texto nem a de acento.** A seta flutua
            // sobre código: com a cor de texto ficava igual ao código de baixo,
            // e com a de acento some dentro da própria faixa realçada — que é
            // pintada em acento. Um controle que se confunde com o conteúdo é
            // um controle que ninguém vê.
            .with_tone(IconTint::Ink)
            .with_font_size(APLICAR_GLIFO)
            .with_height(APLICAR_LADO);
            trecho.layout(layout, area_do_botao);
            trecho.paint(paint);
        }
        for (devolucao, area_do_botao) in self.botoes_de_aplicar(colunas[0]) {
            // Um identificador por fileira: dois botões com o mesmo id seriam o
            // mesmo botão para a moldura, e o estado de um cairia no outro.
            let mut aplicar = Button::new(
                WidgetId(APLICAR_BASE.0 + devolucao.fileira as u64),
                "→",
            )
                // Fundo transparente: a seta está **sobre** o texto que se está
                // lendo para decidir se clica, e um fundo cheio esconderia
                // justamente isso. A borda continua, e é ela que diz que ali se
                // clica.
                .with_fill(ButtonFill::Transparent)
                .with_tone(IconTint::Ink)
                .with_font_size(APLICAR_GLIFO)
                .with_height(APLICAR_LADO);
            aplicar.layout(layout, area_do_botao);
            aplicar.paint(paint);
        }
    }

    /// O botão de fechar, no canto de cima.
    ///
    /// Fora do `if` da aba porque ele é da **janela**, e não do que está dentro
    /// dela: uma aba que escondesse o botão de fechar deixaria quem a abriu sem
    /// saída a não ser o `Esc`.
    ///
    /// Só ícone, e por isso quadrado na altura de botão — o nome acessível é
    /// obrigatório, porque um ícone não é legível por tecnologia assistiva.
    fn paint_fechar(&self, host: &UiHost, layout: &LayoutContext, paint: &mut PaintContext) {
        let mut fechar = Button::icon(FECHAR_ID, Icon::Close, "Fechar")
            .with_align(ButtonAlign::Center)
            .with_tint(IconTint::Muted);
        fechar.layout(layout, Self::botao_de_fechar(area(host, MODAL_ID)));
        fechar.paint(paint);
    }

    /// A barra do alto, com as três ações do repositório.
    ///
    /// **Quem separa os botões e a barra do resto é a `Toolbar`**, e não esta
    /// tela: a folga entre dois botões e o espaço abaixo da barra são aparência,
    /// e aparência é da biblioteca. Daqui sai só *o que* está na barra — e o
    /// pedido de um pouco mais de ar embaixo, porque o que vem logo abaixo é uma
    /// caixa de texto e um campo encostado num botão parece parte dele.
    ///
    /// A ordem é a do trabalho: buscar, trazer, mandar.
    fn barra_do_alto(&self) -> Toolbar {
        let habilitado = self.view.has_repository();
        let botao = |id: WidgetId, rotulo: &str| {
            let mut botao = Button::new(id, rotulo).with_width(ACAO_LARGURA);
            // Sem repositório não há o que buscar, trazer nem mandar, e o botão
            // diz isso pelo próprio desenho.
            botao.set_disabled(!habilitado);
            botao
        };
        Toolbar::new(
            TOOLBAR_ID,
            vec![
                botao(FETCH_ID, "Fetch"),
                botao(PULL_ID, "Pull"),
                botao(PUSH_ID, "Push"),
            ],
        )
        .with_space_above(ESPACO_DA_BARRA)
        .with_space_below(ESPACO_DA_BARRA)
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
            // **A branch atual não tem botão nenhum**, e por isso a coluna não
            // decide nada nela: sem esta linha, clicar no vazio à direita do
            // nome pediria uma ação que a linha nem oferece.
            if branch.current {
                return None;
            }
            // Da direita para a esquerda: o último declarado é o mais à direita.
            if na_faixa(0) {
                return Some(GitRequest::Merge(nome));
            }
            if na_faixa(1) {
                return Some(GitRequest::SwitchBranch(nome));
            }
            return None;
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
        if Self::botao_de_fechar(area(host, MODAL_ID)).contains(point) {
            self.close();
            return None;
        }
        let abas_da_janela = area(host, JANELA_TABS_ID);
        if abas_da_janela.contains(point) {
            // Duas abas de larguras iguais, como as do lado direito: qual delas
            // é sai da coluna, e é a mesma conta que a faixa desenhada usa.
            let metade = abas_da_janela.origin.x + abas_da_janela.size.width / 2.0;
            self.aba_da_janela = if point.x < metade {
                AbaDaJanela::Geral
            } else {
                AbaDaJanela::Diff
            };
            return None;
        }
        if self.aba_da_janela == AbaDaJanela::Diff {
            let area_do_diff = Self::area_util_do_diff(host, context);
            if self.diff.is_some() && area_do_diff.contains(point) {
                // O cabeçalho antes de tudo: ele fica acima das colunas, e um
                // clique nele não é clique em linha nenhuma.
                // Quem diz qual botão está sob o ponto é a própria barra:
                // refazer a conta aqui daria duas respostas que divergem.
                let barra = self.barra_do_cabecalho(area_do_diff, context);
                if barra.hit(point) == Some(DIFF_ANTERIOR_ID) {
                    self.andar_entre_alteracoes(false);
                    return None;
                }
                if barra.hit(point) == Some(DIFF_SEGUINTE_ID) {
                    self.andar_entre_alteracoes(true);
                    return None;
                }
                if barra.hit(point) == Some(DIFF_LADO_ID) {
                    // Trocar de lado é outra pergunta sobre o mesmo arquivo, e
                    // quem responde é o repositório: daqui sai o pedido.
                    let diff = self.diff.as_ref()?;
                    let (caminho, staged) = (diff.path.clone(), diff.staged);
                    return Some(GitRequest::ShowDiff {
                        path: caminho,
                        staged: !staged,
                    });
                }
                let colunas = self.colunas_da_comparacao(area_do_diff, context);
                // As setas antes das colunas: elas flutuam sobre a de então, e
                // um clique nelas cairia na linha de baixo se a pergunta viesse
                // depois.
                if let Some((devolucao, _, _)) = self
                    .botoes_de_trecho(colunas[0])
                    .into_iter()
                    .find(|(_, _, area)| area.contains(point))
                    && let Some(caminho) = self.caminho_do_diff()
                {
                    return Some(GitRequest::RestoreRange {
                        path: caminho,
                        from: devolucao.from,
                        to: devolucao.to,
                    });
                }
                if let Some((devolucao, _)) = self
                    .botoes_de_aplicar(colunas[0])
                    .into_iter()
                    .find(|(_, area)| area.contains(point))
                    && let Some(caminho) = self.caminho_do_diff()
                {
                    return Some(GitRequest::RestoreRange {
                        path: caminho,
                        from: devolucao.from,
                        to: devolucao.to,
                    });
                }
                if let Some(split) = self.split_do_diff.as_mut()
                    && split.divider().contains(point)
                {
                    split.event(
                        &mut EventContext::default(),
                        &UiEvent::PointerDown(primary_pointer(point)),
                    );
                    return None;
                }
                // Clicar na trilha da barra salta a rolagem, e é gesto como
                // qualquer outro: as duas colunas andam juntas depois dele.
                let antes = self.rolagem_das_colunas();
                let mut escolhida = None;
                if let Some(listas) = self.colunas_do_diff.as_mut() {
                    for (indice, (lista, coluna)) in listas.iter_mut().zip(colunas).enumerate() {
                        if coluna.contains(point) {
                            lista.layout(context, coluna);
                            lista.event(
                                &mut EventContext::default(),
                                &UiEvent::PointerDown(primary_pointer(point)),
                            );
                            escolhida = lista.selected().map(|fileira| (indice, fileira));
                            break;
                        }
                    }
                }
                // A mesma fileira nos dois lados: escolher à esquerda e a
                // direita continuar noutra linha é a comparação dizendo duas
                // coisas ao mesmo tempo. A fileira é a unidade, e ela já
                // emparelha as duas versões.
                if let Some((clicada, fileira)) = escolhida
                    && let Some(listas) = self.colunas_do_diff.as_mut()
                {
                    listas[1 - clicada].set_selected(Some(fileira));
                }
                self.casar_a_rolagem(antes);

            }
            return None;
        }
        let faixa = area(host, TOOLBAR_ID);
        if faixa.contains(point) {
            // Quem diz qual botão está sob o ponto é a própria barra: refazer a
            // conta aqui daria duas respostas que divergem na primeira folga
            // diferente.
            let mut barra = self.barra_do_alto();
            barra.layout(context, faixa);
            return barra.hit(point).map(|id| {
                if id == FETCH_ID {
                    GitRequest::Fetch
                } else if id == PULL_ID {
                    GitRequest::Pull
                } else {
                    GitRequest::Push
                }
            });
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
        // **O clique fora não fecha.** Esta janela é uma tela de trabalho, e não
        // um aviso: quem está escrevendo a mensagem de um commit e erra o alvo
        // do clique perderia o que escreveu. Fecham-na o botão do canto e o
        // `Esc`, que são os dois gestos que se dá de propósito.
        None
    }

    /// Movimento e soltura: são da divisa, que é o que se arrasta aqui.
    /// O movimento e a soltura do ponteiro, para tudo o que está na janela.
    ///
    /// # Por que isto vai para todo mundo, e não para quem está sob o ponteiro
    ///
    /// **Um arrasto começa dentro de um componente e continua fora dele.** Quem
    /// agarra a alça de uma barra de rolagem e arrasta sai da trilha nos
    /// primeiros pixels; se o movimento fosse entregue só a quem está debaixo do
    /// ponteiro, a alça ficaria onde foi agarrada enquanto o ponteiro seguisse
    /// sozinho. Foi o que aconteceu aqui: o clique chegava, o movimento não, e
    /// nenhuma barra desta janela rolava.
    ///
    /// Cada componente sabe se o gesto é dele — todos guardam se estão em
    /// arrasto —, e para quem não está, um movimento é hover: barato e certo.
    pub(super) fn pointer_event(&mut self, host: &UiHost, context: &LayoutContext, event: &UiEvent) {
        let corpo = area(host, BODY_ID);
        if let Some(split) = self.split.as_mut() {
            split.layout(context, corpo);
            split.event(&mut EventContext::default(), event);
        }
        let conteudo = area(host, CONTENT_ID);
        if self.aba_da_janela == AbaDaJanela::Diff {
            let area_do_diff = Self::area_util_do_diff(host, context);
            let colunas = self.colunas_da_comparacao(area_do_diff, context);
            if let Some(split) = self.split_do_diff.as_mut() {
                split.event(&mut EventContext::default(), event);
            }
            let antes = self.rolagem_das_colunas();
            if let Some(listas) = self.colunas_do_diff.as_mut() {
                for (lista, coluna) in listas.iter_mut().zip(colunas) {
                    lista.layout(context, coluna);
                    lista.event(&mut EventContext::default(), event);
                }
            }
            self.casar_a_rolagem(antes);

            return;
        }
        // A árvore da esquerda: ela tem barra vertical própria.
        let arvore = area(host, TREE_ID);
        if let Some(tree) = self.tree.as_mut() {
            tree.layout(context, arvore);
            tree.event(&mut EventContext::default(), event);
        }
        if self.aba == Aba::History {
            if let Some(tabela) = self.tabela.as_mut() {
                tabela.layout(context, conteudo);
                tabela.event(&mut EventContext::default(), event);
            }
            return;
        }
        if conteudo.size.height <= 0.0 {
            return;
        }
        // As duas divisas dos painéis empilhados recebem o mesmo gesto: elas
        // também se arrastam, e também precisam do movimento para se anunciar.
        let faixas = self.faixas(conteudo, context);
        for split in [self.split_alto.as_mut(), self.split_baixo.as_mut()]
            .into_iter()
            .flatten()
        {
            split.event(&mut EventContext::default(), event);
        }
        for (indice, faixa) in faixas.into_iter().enumerate() {
            let area_da_lista = Self::area_da_lista(faixa);
            if let Some(lista) = self.listas.as_mut().and_then(|listas| listas.get_mut(indice)) {
                lista.layout(context, area_da_lista);
                lista.event(&mut EventContext::default(), event);
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
        if self.aba_da_janela == AbaDaJanela::Diff {
            // As duas colunas rolam juntas: comparar dois textos exige que a
            // linha 40 de um fique ao lado da linha 40 do outro, e duas rolagens
            // independentes desfazem a comparação a cada gesto.
            let area_do_diff = Self::area_util_do_diff(host, context);
            let colunas = self.colunas_da_comparacao(area_do_diff, context);
            let antes = self.rolagem_das_colunas();
            if let Some(listas) = self.colunas_do_diff.as_mut() {
                for (lista, coluna) in listas.iter_mut().zip(colunas) {
                    lista.layout(context, coluna);
                    lista.event(&mut EventContext::default(), &evento);
                }
            }
            self.casar_a_rolagem(antes);
            return None;
        }
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

        // As abas da janela, antes de tudo o que elas repartem.
        let mut abas_da_janela = Tabs::new(
            JANELA_TABS_ID,
            vec![TabItem::new(1, "Geral"), TabItem::new(2, "Diff")],
        );
        abas_da_janela.set_active(usize::from(self.aba_da_janela == AbaDaJanela::Diff));
        abas_da_janela.layout(layout, area(host, JANELA_TABS_ID));
        abas_da_janela.paint(paint);

        if self.aba_da_janela == AbaDaJanela::Diff {
            let mut painel = Self::painel_do_diff();
            painel.layout(layout, area(host, DIFF_ID));
            painel.paint(paint);
            // O conteúdo vai **dentro** da moldura, e não sobre ela.
            self.paint_diff(painel.content(), layout, paint);
            self.paint_fechar(host, layout, paint);
            return true;
        }

        self.paint_fechar(host, layout, paint);

        // A barra do alto: as três ações do repositório.
        let mut barra = self.barra_do_alto();
        barra.layout(layout, area(host, TOOLBAR_ID));
        barra.paint(paint);

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

    /// Quanto a árvore já rolou, para o teste ver o arrasto agir.
    #[cfg(test)]
    pub(super) fn rolagem_da_arvore(&self) -> f32 {
        self.tree
            .as_ref()
            .map_or(0.0, |tree| tree.scroll_offset().y)
    }

    /// Onde as duas colunas da comparação estão, para o teste ver se andam juntas.
    #[cfg(test)]
    pub(super) fn rolagem_do_diff(&self) -> [f32; 2] {
        self.colunas_do_diff
            .as_ref()
            .map_or([0.0; 2], |listas| {
                [listas[0].scroll_offset(), listas[1].scroll_offset()]
            })
    }

    /// As duas colunas da comparação, para o teste apontar nelas.
    #[cfg(test)]
    pub(super) fn colunas_do_diff_para_teste(&mut self, host: &UiHost) -> [Rect; 2] {
        let context = LayoutContext::default();
        let area_do_diff = Self::area_util_do_diff(host, &context);
        self.colunas_da_comparacao(area_do_diff, &context)
    }

    /// As setas de trecho, para o teste clicar nelas.
    #[cfg(test)]
    pub(super) fn botoes_de_trecho_para_teste(
        &mut self,
        host: &UiHost,
    ) -> Vec<(Devolucao, usize, Rect)> {
        let colunas = self.colunas_do_diff_para_teste(host);
        self.botoes_de_trecho(colunas[0])
    }

    /// Onde cada comando do cabeçalho ficou, para o teste apontar neles.
    #[cfg(test)]
    pub(super) fn comandos_do_cabecalho_para_teste(
        &self,
        host: &UiHost,
        context: &LayoutContext,
    ) -> Vec<(WidgetId, Rect)> {
        self.barra_do_cabecalho(Self::area_util_do_diff(host, context), context)
            .item_bounds()
    }

    /// A fileira escolhida em cada coluna, para o teste ver as duas concordarem.
    #[cfg(test)]
    pub(super) fn escolhidas_do_diff(&self) -> [Option<usize>; 2] {
        self.colunas_do_diff.as_ref().map_or([None; 2], |listas| {
            [listas[0].selected(), listas[1].selected()]
        })
    }

    /// As setas que devolvem linhas, para o teste clicar nelas.
    #[cfg(test)]
    pub(super) fn botoes_de_aplicar_para_teste(
        &mut self,
        host: &UiHost,
    ) -> Vec<(Devolucao, Rect)> {
        let colunas = self.colunas_do_diff_para_teste(host);
        self.botoes_de_aplicar(colunas[0])
    }

    /// As abas da janela, para o teste apontar nelas.
    #[cfg(test)]
    pub(super) fn abas_da_janela_para_teste(&self, host: &UiHost) -> Rect {
        area(host, JANELA_TABS_ID)
    }

    /// A barra do alto, para o teste apontar nos botões dela.
    #[cfg(test)]
    pub(super) fn barra_para_teste(&self, host: &UiHost) -> Rect {
        area(host, TOOLBAR_ID)
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
    /// Abre o gerenciador.
    ///
    /// Abrir pede o retrato de novo: entre a última resposta e agora o usuário
    /// pode ter commitado no terminal integrado, e uma janela que mostra o
    /// estado de dez minutos atrás não avisa que está errada — ela só está
    /// errada.
    ///
    /// **Fechar é do botão do canto e do `Esc`.** Com a janela aberta o gesto é
    /// dela, e nem este botão a alcança: ela é tela de trabalho, e fechar sem
    /// querer custaria a mensagem de commit que estava sendo escrita.
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
    /// **Lista vazia é resposta, e não ausência de resposta.** Ela fica
    /// guardada: "perguntei, e este arquivo está igual ao commit" é diferente de
    /// "ainda não perguntei", e é essa diferença que impede a IDE de perguntar
    /// de novo a cada quadro por um arquivo que não mudou.
    pub fn set_git_line_marks(
        &mut self,
        path: std::path::PathBuf,
        marks: Vec<(usize, GitLineChange)>,
    ) {
        self.git.marcas.insert(path, marks);
    }

    /// Reescreve um documento aberto com o texto que está no disco.
    ///
    /// **Sem abrir, sem ativar e sem tirar o foco de quem pediu.** Devolver uma
    /// linha grava o arquivo, e o editor principal continuaria mostrando o texto
    /// de antes; abrir pelo caminho de sempre traria o foco junto, e quem clicou
    /// está na janela do Git, que não fechou.
    ///
    /// Devolve `false` quando o arquivo não está aberto: aí não há o que
    /// refrescar, e isso não é erro.
    pub fn refresh_document(&mut self, path: &std::path::Path, text: &str) -> bool {
        let Some(id) = self
            .editor_area
            .session
            .tabs()
            .find(|documento| documento.path == path)
            .map(|documento| documento.id)
        else {
            return false;
        };
        let Some(documento) = self.editor_area.session.document_mut(id) else {
            return false;
        };
        // `replace` do texto inteiro, e não um buffer novo: é ele que faz a
        // revisão subir, e é pela revisão que o editor sabe que precisa se
        // refazer.
        let tamanho = documento.buffer.text().len();
        documento.buffer.replace(0..tamanho, text).is_ok()
    }

    /// A linha do arquivo **de então**, como a comparação a mostra.
    ///
    /// Quem devolve uma linha ao arquivo de agora precisa do texto dela, e ele
    /// já está aqui: foi o que a coluna da esquerda desenhou.
    #[must_use]
    pub fn git_diff_line(&self, line: usize) -> Option<String> {
        self.git
            .view_diff()?
            .committed
            .lines()
            .nth(line)
            .map(ToOwned::to_owned)
    }

    /// O arquivo na frente, quando ainda não se sabe o que mudou nele.
    ///
    /// Quem pergunta ao repositório é a aplicação, e ela pergunta isto a cada
    /// quadro: é uma consulta a um mapa, e cobre **todas** as formas de trocar
    /// de aba — o clique, o `Ctrl+Tab`, a divisão, a navegação — sem que cada
    /// uma delas precise lembrar de pedir.
    #[must_use]
    pub fn git_marks_missing(&self) -> Option<std::path::PathBuf> {
        let caminho = self.active_document_path()?;
        (!self.git.marcas.contains_key(&caminho)).then_some(caminho)
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
    pub fn abrir_comparacao(&mut self, path: &std::path::Path, diff: GitDiff) -> bool {
        // **Uma comparação é um valor só**, e chega assim: eram sete parâmetros,
        // e sete parâmetros na mesma ordem são um erro de posição esperando
        // acontecer — dois `Vec` de números trocados entre si compilam.
        //
        // O rótulo é o único que não vem de fora: a raiz do projeto é do shell,
        // e quem leu o repositório tem o caminho inteiro e não onde ele começa.
        let label = path
            .strip_prefix(self.workspace_root())
            .unwrap_or(path)
            .display()
            .to_string()
            .replace('\\', "/");
        self.git.mostrar_diff(GitDiff {
            label,
            path: path.to_path_buf(),
            ..diff
        });
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
    // **A branch atual não oferece nada**: trocar para onde já se está não faz
    // nada, e fundir uma branch nela mesma é comando que o `git` recusa. O que
    // ela tinha — puxar e empurrar — subiu para a barra do alto, porque é ação
    // do repositório e não da linha.
    let acoes: &[(u64, &str, &str)] = if atual {
        &[]
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

/// A linha da raiz dos remotos: só o nome.
///
/// O `Fetch` morava aqui e subiu para a barra do alto: ele traz as referências
/// **todas** de uma vez, e um botão na linha de um nó fazia parecer que buscava
/// só aquele.
fn linha_do_remoto(nome: &str) -> ComposedRow {
    ComposedRow::new(vec![ComposedCell::new(
        Box::new(Label::new(WidgetId(ROW_BASE.0 + 2), nome)),
        CellWidth::Fill,
    )])
}
