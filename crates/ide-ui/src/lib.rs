#![doc = "Shell visual e interativo da IDE baseado no ERLibUi."]

mod editor;
pub use editor::{EditorAction, EditorCapabilities, EditorPane};

use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    hash::{DefaultHasher, Hash, Hasher},
    path::{Path, PathBuf},
    sync::Arc,
};

use ide_domain::{
    CompletionItem, CompletionRequest, DocumentId, DocumentSnapshot, OutlineItem,
    SyntaxHighlightKind, SyntaxSnapshot, TextPosition as DomainTextPosition,
};
use ide_terminal::{ShellKind, TerminalSession};
use ide_text::{EditorSession, TextBuffer};
use ui_editor::{
    CodeEditor, GutterMark, LineDecoration, TokenKind,
};
use ide_workspace::{FileNode, WorkspaceError};
use ui_api::{EventContext, LayoutContext, PaintContext, TextMetrics, Widget};
use ui_components::{
    Button, ComboBox, ComboBoxItem, ContextMenu, Icon, IconTint, Label, ListSelection,
    ListView, MenuBar,
    MenuBarItem,
    Popup,
    MenuEntry, MenuItem, ModalHost, Scrollbar, ScrollbarOrientation, SplitOrientation, Splitter,
    StatusBar, TabItem, Tabs, TextInput, TreeItem, TreeView,
};
use ui_core::{
    Color, ColorTokens, CommandId, EventResult, FontId, KeyEvent, Modifiers, Point, PointerButton,
    PointerEvent, Rect, Size, TextInputEvent, Theme, UiEvent, WidgetAction, WidgetId,
};
use ui_render_api::{DrawTextCommand, FillRectCommand, PaintCommand, StrokeRectCommand};
use ui_window_api::ClipboardService;

const ACTIVITY_WIDTH: f32 = 48.0;
const SIDEBAR_WIDTH: f32 = 260.0;
const SIDEBAR_MIN_WIDTH: f32 = 160.0;
const TITLE_HEIGHT: f32 = 36.0;
const TAB_HEIGHT: f32 = 38.0;
const EXPLORER_ROW_HEIGHT: f32 = 23.0;
const EXPLORER_TOP: f32 = 106.0;
/// Métricas do editor: elas são do componente que desenha o código, e a IDE
/// as consulta para posicionar cursor, popup e cliques no mesmo lugar.
const EDITOR_LINE_HEIGHT: f32 = CodeEditor::line_height();
/// Largura de uma parada de tabulação, em colunas.
const EDITOR_GUTTER: f32 = CodeEditor::gutter_width();
/// Largura do caractere na fonte de código, definida pelo editor que a desenha.
const EDITOR_CHAR_WIDTH: f32 = CodeEditor::default_char_width();
const TAB_WIDTH: f32 = 140.0;
const TERMINAL_TAB_WIDTH: f32 = 110.0;
const TERMINAL_TAB_HEIGHT: f32 = 30.0;
const TERMINAL_DEFAULT_HEIGHT: f32 = 180.0;
const TERMINAL_MIN_HEIGHT: f32 = 120.0;
const TERMINAL_COLLAPSED_HEIGHT: f32 = 30.0;
const TERMINAL_CHAR_WIDTH: f32 = 8.4;
const DEBUG_PANEL_WIDTH: f32 = 320.0;
const DEBUG_ROW_HEIGHT: f32 = 21.0;
const MENU_BAR_ID: WidgetId = WidgetId(10_001);
const SETTINGS_MODAL_ID: WidgetId = WidgetId(10_002);
const JDK_COMBO_ID: WidgetId = WidgetId(10_003);
const JDK_BROWSE_ID: WidgetId = WidgetId(10_004);
const SETTINGS_CLOSE_ID: WidgetId = WidgetId(10_005);
const SETTINGS_SAVE_ID: WidgetId = WidgetId(10_030);
const SETTINGS_TITLE_ID: WidgetId = WidgetId(10_031);
const SETTINGS_CAPTION_ID: WidgetId = WidgetId(10_032);
const SETTINGS_MESSAGE_ID: WidgetId = WidgetId(10_033);
const SETTINGS_PAGES_ID: WidgetId = WidgetId(10_034);
/// Páginas da janela de configurações, na ordem em que aparecem.
const SETTINGS_PAGE_TITLES: [&str; 2] = ["Compilador e VM", "Depuração"];
const SETTINGS_PAGE_ROW_HEIGHT: f32 = 42.0;
const NEW_ITEM_MODAL_ID: WidgetId = WidgetId(10_035);
const NEW_ITEM_PACKAGE_ID: WidgetId = WidgetId(10_036);
const NEW_ITEM_NAME_ID: WidgetId = WidgetId(10_037);
const NEW_ITEM_CREATE_ID: WidgetId = WidgetId(10_038);
const NEW_ITEM_CANCEL_ID: WidgetId = WidgetId(10_039);
const NEW_ITEM_PACKAGE_CAPTION_ID: WidgetId = WidgetId(10_041);
const NEW_ITEM_NAME_CAPTION_ID: WidgetId = WidgetId(10_042);
const NEW_ITEM_MESSAGE_ID: WidgetId = WidgetId(10_043);
/// A janela é pequena de propósito: dois campos e duas ações.
const NEW_ITEM_PANEL_SIZE: Size = Size::new(460.0, 230.0);
const INSPECTION_MODAL_ID: WidgetId = WidgetId(10_044);
const INSPECTION_TREE_ID: WidgetId = WidgetId(10_045);
const INSPECTION_CLOSE_ID: WidgetId = WidgetId(10_046);
const INSPECTION_NAME_ID: WidgetId = WidgetId(10_047);
const INSPECTION_TYPE_ID: WidgetId = WidgetId(10_048);
const INSPECTION_VALUE_ID: WidgetId = WidgetId(10_049);
const INSPECTION_EMPTY_ID: WidgetId = WidgetId(10_050);
const INSPECTION_RUN_ID: WidgetId = WidgetId(10_052);
const INSPECTION_SOURCE_CAPTION_ID: WidgetId = WidgetId(10_053);
const INSPECTION_MESSAGE_ID: WidgetId = WidgetId(10_054);
/// Largura média de caractere na fonte da mensagem, para saber onde cortar.
const INSPECTION_MESSAGE_CHAR_WIDTH: f32 = 6.6;
/// A janela é larga porque o valor de um objeto costuma ser longo.
const INSPECTION_PANEL_SIZE: Size = Size::new(720.0, 420.0);
const INSPECTION_ROW_HEIGHT: f32 = 26.0;
/// Fatia da janela ocupada pela lista, à esquerda.
const INSPECTION_LIST_FRACTION: f32 = 0.42;
/// Fatia do painel direito ocupada pelo detalhe; o resto é o editor.
const INSPECTION_DETAIL_FRACTION: f32 = 0.45;
const DEBUG_HOST_ID: WidgetId = WidgetId(10_006);
const DEBUG_PORT_ID: WidgetId = WidgetId(10_007);
const DEBUG_ATTACH_ID: WidgetId = WidgetId(10_008);
const STOP_BUTTON_ID: WidgetId = WidgetId(10_009);
const RUN_BUTTON_ID: WidgetId = WidgetId(10_010);
const DEBUG_BUTTON_ID: WidgetId = WidgetId(10_011);
const DEBUG_FRAMES_ID: WidgetId = WidgetId(10_012);
const DEBUG_VARIABLES_ID: WidgetId = WidgetId(10_013);
const STATUS_BAR_ID: WidgetId = WidgetId(10_014);
const EDITOR_TABS_ID: WidgetId = WidgetId(10_015);
const TERMINAL_TABS_ID: WidgetId = WidgetId(10_016);
const EDITOR_SCROLLBAR_ID: WidgetId = WidgetId(10_017);
const TERMINAL_SCROLLBAR_ID: WidgetId = WidgetId(10_018);
const EXPLORER_VERTICAL_SCROLLBAR_ID: WidgetId = WidgetId(10_019);
const EXPLORER_HORIZONTAL_SCROLLBAR_ID: WidgetId = WidgetId(10_021);
const EXPLORER_TREE_ID: WidgetId = WidgetId(10_020);
const SIDEBAR_SPLITTER_ID: WidgetId = WidgetId(10_022);
const TERMINAL_SPLITTER_ID: WidgetId = WidgetId(10_023);
const EXPLORER_CONTEXT_MENU_ID: WidgetId = WidgetId(10_025);
const COMPLETION_POPUP_ID: WidgetId = WidgetId(10_026);
const COMPLETION_LIST_ID: WidgetId = WidgetId(10_027);
const SEARCH_POPUP_ID: WidgetId = WidgetId(10_028);
const SEARCH_INPUT_ID: WidgetId = WidgetId(10_029);
/// Linhas visíveis da lista de completação antes de precisar rolar.
const COMPLETION_VISIBLE_ROWS: usize = 8;
const COMPLETION_ROW_HEIGHT: f32 = 24.0;
const COMPLETION_POPUP_WIDTH: f32 = 260.0;
const SEARCH_BOX_WIDTH: f32 = 380.0;
const SEARCH_BOX_HEIGHT: f32 = 42.0;

/// Página ativa da janela de configurações.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum SettingsPage {
    #[default]
    Compiler,
    Debug,
}

/// Pedido da interface para a sessão de depuração.
///
/// A apresentação não conhece protocolo nem servidor: apenas descreve o que o
/// usuário pediu, e a aplicação traduz para a sessão ativa.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DebugRequest {
    Attach {
        host: String,
        port: u16,
    },
    /// Sobe a aplicação do projeto com depuração e conecta nela.
    RunAndAttach {
        host: String,
        port: u16,
    },
    Continue,
    Pause,
    StepOver,
    StepInto,
    StepOut,
    Detach,
    SelectFrame(usize),
    /// Avalia uma expressão no quadro selecionado.
    ///
    /// É o que "Inspecionar" pede sobre o trecho marcado no editor: o valor de um
    /// nome só existe com a execução parada, e é o quadro atual que lhe dá
    /// sentido.
    Evaluate(String),
    /// Revela os campos de um valor já inspecionado, endereçado pelo caminho.
    ///
    /// Os campos são pedidos ao abrir o nó, e não de uma vez: percorrer o grafo
    /// inteiro de um objeto para mostrar o primeiro nível seria caro e, em
    /// estruturas cíclicas, infinito.
    ExpandInspection(String),
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DebugFrameView {
    pub name: String,
    pub location: Option<(PathBuf, u32)>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DebugVariableView {
    pub name: String,
    pub value: String,
    pub type_name: Option<String>,
    /// O valor tem campos que podem ser revelados.
    ///
    /// Sem isso a inspeção não teria como oferecer o triângulo de expansão só
    /// onde há o que abrir, e um número simples pareceria esconder algo.
    pub expandable: bool,
}

/// Estado da depuração apresentado pela interface.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DebugView {
    pub attached: bool,
    pub status: String,
    pub stopped_at: Option<(PathBuf, u32)>,
    pub frames: Vec<DebugFrameView>,
    pub selected_frame: usize,
    pub variables: Vec<DebugVariableView>,
}

impl DebugView {
    #[must_use]
    pub const fn is_stopped(&self) -> bool {
        self.stopped_at.is_some()
    }
}

#[derive(Clone, Copy)]
struct TextPosition {
    line: usize,
    column: usize,
}

#[derive(Clone, Copy)]
struct TerminalSelection {
    anchor: TextPosition,
    focus: TextPosition,
}

/// Qual barra de rolagem está sob o gesto.
///
/// O deslocamento da pegada pertence ao componente; aqui só se registra qual
/// deles está sendo arrastado, para que o mesmo movimento não role duas áreas.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ScrollTarget {
    Editor,
    Terminal,
    ExplorerHorizontal,
    ExplorerVertical,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ShellFocus {
    #[default]
    None,
    Explorer,
    Editor,
    Search,
    Terminal,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NavigationRequest {
    pub document_id: DocumentId,
    pub byte_offset: usize,
    pub token: String,
}

struct TerminalTab {
    session: TerminalSession,
    scroll_line: usize,
    follow_output: bool,
}

/// O que o menu do Explorer pediu para criar.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NewItemKind {
    Package,
    Class,
    Interface,
}

impl NewItemKind {
    /// Título da janela.
    const fn title(self) -> &'static str {
        match self {
            Self::Package => "Novo pacote",
            Self::Class => "Nova classe",
            Self::Interface => "Nova interface",
        }
    }

    /// Legenda do campo de nome.
    ///
    /// Criando um pacote, o nome do tipo é opcional: é o que permite criar
    /// pacote e primeira classe num gesto só.
    const fn name_caption(self) -> &'static str {
        match self {
            Self::Package => "Classe (opcional)",
            Self::Class => "Nome da classe",
            Self::Interface => "Nome da interface",
        }
    }
}

/// Pedido de criação, já validado, que o app executa.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NewItemRequest {
    pub kind: NewItemKind,
    /// Pacote em notação de ponto, como foi digitado.
    pub package: String,
    /// Nome do tipo; vazio quando só o pacote foi pedido.
    pub name: String,
    /// Raiz de fontes sob a qual o pacote vive.
    pub source_root: PathBuf,
}

/// Para onde vão as teclas dentro da janela de inspeção.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum InspectionFocus {
    #[default]
    Tree,
    Source,
}

/// Um valor na árvore de inspeção, com os campos já revelados.
///
/// O caminho é a expressão que endereça o valor a partir da raiz —
/// `pedido.cliente.nome` — e é ele que o alvo entende ao ser perguntado pelos
/// campos. Também é a identidade do nó: o índice mudaria a cada expansão.
#[derive(Clone, Debug)]
struct InspectionNode {
    path: String,
    variable: DebugVariableView,
    children: Vec<InspectionNode>,
    /// Os campos já foram pedidos ao alvo.
    ///
    /// Um objeto sem campos carregados e um sem campos nenhum se pareceriam,
    /// e o segundo pedido de expansão nunca chegaria.
    loaded: bool,
}

impl InspectionNode {
    fn new(path: String, variable: DebugVariableView) -> Self {
        Self {
            path,
            variable,
            children: Vec::new(),
            loaded: false,
        }
    }

    /// Encontra o nó de um caminho, em profundidade.
    fn find_mut(&mut self, path: &str) -> Option<&mut Self> {
        if self.path == path {
            return Some(self);
        }
        // Só desce por onde o caminho passa: os demais ramos não o contêm.
        if !path.starts_with(&self.path) {
            return None;
        }
        self.children
            .iter_mut()
            .find_map(|child| child.find_mut(path))
    }

    fn find(&self, path: &str) -> Option<&Self> {
        if self.path == path {
            return Some(self);
        }
        if !path.starts_with(&self.path) {
            return None;
        }
        self.children.iter().find_map(|child| child.find(path))
    }
}

/// Valor sendo inspecionado, com a árvore de campos revelados até agora.
struct InspectionView {
    expression: String,
    root: InspectionNode,
    /// Caminhos abertos na árvore.
    expanded: HashSet<String>,
    /// Caminho do nó destacado, detalhado no painel direito.
    selected: String,
}

/// Janela de criação enquanto está aberta.
struct NewItemDialog {
    kind: NewItemKind,
    source_root: PathBuf,
    message: Option<String>,
    /// Campo com o foco: `false` é o pacote, `true` é o nome.
    naming: bool,
}

/// Estado da janela de configurações enquanto ela está aberta.
///
/// A janela é uma transação: o que se mexe ali só vale quando o usuário salva.
/// Por isso guarda o que estava valendo na abertura — é para lá que o
/// cancelamento volta — e a escolha pendente, que ainda não saiu daqui.
struct SettingsDialog {
    message: Option<String>,
    /// JDK escolhido na janela, ainda não aplicado.
    pending_jdk: Option<usize>,
    /// Estado no momento da abertura, para o cancelamento restaurar.
    original_jdk: Option<usize>,
    original_debug_host: String,
    original_debug_port: String,
}

pub struct IdeShell {
    workspace_name: String,
    workspace: FileNode,
    /// O Explorer é a `TreeView` da biblioteca. Ela é reconstruída só quando a
    /// árvore ou a expansão mudam — o caminho oposto ao das abas, porque
    /// remontar milhares de nós a cada quadro custaria caro.
    explorer_tree: TreeView,
    /// Menu do clique secundário no Explorer.
    context_menu: ContextMenu,
    /// Diretório sobre o qual o menu aberto age.
    ///
    /// Clicando em um arquivo, o alvo é a pasta dele: criar dentro de um
    /// arquivo não quer dizer nada, e é onde o usuário apontou que importa.
    context_menu_target: Option<PathBuf>,
    expanded: HashSet<PathBuf>,
    editor: EditorSession,
    /// Painel de edição do editor principal, com tudo ligado.
    ///
    /// Cursor, seleção, rolagem e a view de desenho vivem nele — o shell entrega
    /// o buffer do documento ativo e reage ao que o painel pede.
    editor_pane: EditorPane,
    focus: ShellFocus,
    search_query: String,
    terminals: Vec<TerminalTab>,
    active_terminal: usize,
    explorer_scroll_x: f32,
    explorer_scroll_line: usize,
    sidebar_width: f32,
    terminal_height: f32,
    terminal_last_height: f32,
    terminal_minimized: bool,
    /// Medição de texto oferecida pela aplicação.
    ///
    /// Sem ela os componentes estimam por contagem de caracteres; com ela cada
    /// um pergunta à fonte que vai desenhar. A IDE não constrói o mecanismo:
    /// ela recebe a porta e a repassa aos widgets.
    text_metrics: Option<Arc<dyn TextMetrics>>,
    /// Área de transferência do sistema, quando o ambiente oferece uma.
    ///
    /// Sem ela copiar e colar ficam desligados e dizem isso na barra de estado,
    /// em vez de a IDE não abrir.
    clipboard: Option<Arc<dyn ClipboardService>>,
    /// Linha alcançada pela última navegação, com o cursor que ela deixou.
    ///
    /// O destaque vale enquanto o cursor continuar onde a navegação o pôs. Não
    /// há o que limpar: assim que o usuário clica ou digita, o cursor muda e o
    /// destaque deixa de valer sozinho — nenhum caminho novo precisa lembrar de
    /// apagá-lo.
    navigated: Option<(usize, usize)>,
    /// Linha que precisa aparecer no próximo quadro.
    ///
    /// Quem navega não conhece a altura do editor; ela só existe na pintura, que
    /// é onde a revelação acontece.
    ///
    /// A IDE conta bytes e o editor da biblioteca conta caracteres, então a
    /// conversão fica na borda entre os dois — guardar em caracteres faria toda
    /// edição converter de volta.
    /// Arraste de seleção em curso no editor.
    /// Divisores redimensionáveis do layout, com limites em pontos.
    ///
    /// Eles guardam o arraste entre um evento e o seguinte; a posição e os
    /// limites são reconciliados com o tamanho da janela a cada uso.
    sidebar_splitter: Splitter,
    terminal_splitter: Splitter,
    scrollbar_drag: Option<ScrollTarget>,
    /// As quatro barras de rolagem da janela, como widgets da biblioteca.
    ///
    /// Elas guardam o estado do arraste entre um evento e o seguinte; faixa,
    /// trilha e deslocamento são reconciliados com o conteúdo a cada uso.
    editor_scrollbar: Scrollbar,
    terminal_scrollbar: Scrollbar,
    explorer_vertical_scrollbar: Scrollbar,
    explorer_horizontal_scrollbar: Scrollbar,
    terminal_selection: Option<TerminalSelection>,
    terminal_selecting: bool,
    menu_bar: MenuBar,
    settings_modal: ModalHost,
    jdk_combo: ComboBox,
    jdk_browse_button: Button,
    settings_close_button: Button,
    settings_save_button: Button,
    /// Navegação entre as páginas da janela de configurações.
    settings_pages: ListView,
    /// Janela de criação de pacote, classe ou interface.
    new_item_modal: ModalHost,
    new_item_dialog: Option<NewItemDialog>,
    new_item_package: TextInput,
    new_item_name: TextInput,
    new_item_create_button: Button,
    new_item_cancel_button: Button,
    new_item_request: Option<NewItemRequest>,
    /// Janela de inspeção de um valor durante a depuração.
    inspection_modal: ModalHost,
    inspection: Option<InspectionView>,
    inspection_tree: TreeView,
    inspection_close_button: Button,
    /// Editor de expressões do painel direito da inspeção.
    ///
    /// É o mesmo painel da janela principal, com os comportamentos que não fazem
    /// sentido aqui desligados: não há arquivo para salvar, definição para
    /// navegar nem linha onde parar a execução.
    inspection_editor: EditorPane,
    inspection_source: TextBuffer,
    inspection_run_button: Button,
    inspection_focus: InspectionFocus,
    /// Resposta da última execução, mostrada dentro da janela.
    ///
    /// A barra de estado fica atrás do painel: um erro relatado só lá deixa o
    /// usuário sem saber por que nada aconteceu.
    inspection_message: Option<String>,
    open_project_requested: bool,
    open_settings_requested: bool,
    build_project_requested: bool,
    reimport_project_requested: bool,
    run_requested: bool,
    stop_requested: bool,
    /// Aba de terminal em que a aplicação foi iniciada pela IDE.
    running_terminal: Option<usize>,
    project_summary: Option<String>,
    browse_jdk_requested: bool,
    pending_navigation: Option<NavigationRequest>,
    status_message: String,
    syntax_snapshots: HashMap<DocumentId, SyntaxSnapshot>,
    completion_items: Vec<CompletionItem>,
    completion_selected: usize,
    settings_dialog: Option<SettingsDialog>,
    settings_jdk_result: Option<usize>,
    settings_page: SettingsPage,
    settings_focus: Option<WidgetId>,
    stop_button: Button,
    run_button: Button,
    debug_button: Button,
    debug_host: TextInput,
    debug_port: TextInput,
    debug_attach_button: Button,
    theme: Theme,
    breakpoints: BTreeMap<PathBuf, BTreeSet<u32>>,
    /// Linhas que o alvo confirmou, por arquivo.
    verified_breakpoints: BTreeMap<PathBuf, BTreeSet<u32>>,
    breakpoints_dirty: Option<PathBuf>,
    debug: DebugView,
    /// Última posição do ponteiro, entregue às abas reconstruídas a cada quadro
    /// para que o botão de fechar apareça sob o cursor.
    pointer: Point,
    /// Pilha de chamadas e variáveis são listas da biblioteca: rolagem,
    /// seleção, recorte e acessibilidade não se reimplementam aqui.
    debug_frames: ListView,
    debug_variables: ListView,
    debug_requests: Vec<DebugRequest>,
}

impl IdeShell {
    pub fn open(root: &Path) -> Result<Self, WorkspaceError> {
        let workspace = FileNode::scan(root)?;
        Ok(Self::from_tree(workspace))
    }

    /// Relê a árvore do disco preservando o que está expandido.
    ///
    /// O que nasceu depois da última varredura não existe para o Explorer até ele
    /// reler. A expansão é mantida porque ela é a posição do usuário na árvore:
    /// recolher tudo depois de criar um arquivo esconderia justamente o que ele
    /// acabou de criar.
    pub fn reload_workspace(&mut self) -> Result<(), WorkspaceError> {
        let root = self.workspace.path.clone();
        self.workspace = FileNode::scan(&root)?;
        // A `TreeView` guarda os itens dela: reler o disco sem repô-los deixava a
        // árvore desenhando a varredura anterior. `set_roots` preserva expansão e
        // seleção por identidade, então a posição do usuário não se perde.
        self.explorer_tree.set_roots(explorer_items(&self.workspace));
        self.sync_explorer_tree();
        Ok(())
    }

    /// A aba ativa tem alteração ainda não gravada.
    ///
    /// É o que a marca na aba anuncia, e o que decide se fechar sem salvar
    /// perderia trabalho.
    #[must_use]
    pub fn active_document_modified(&self) -> bool {
        self.editor
            .active()
            .is_some_and(|document| document.buffer.is_dirty())
    }

    /// Grava a aba ativa no disco.
    ///
    /// Devolve `true` quando gravou. A barra de estado relata as duas saídas: um
    /// salvamento silencioso não se distingue de um que falhou, e a aba continua
    /// marcada como modificada quando a escrita não deu certo.
    pub fn save_active_document(&mut self) -> bool {
        match self.editor.save_active() {
            Ok(path) => {
                self.status_message = format!("Salvo {}", path.display());
                true
            }
            Err(error) => {
                self.status_message = error.to_string();
                false
            }
        }
    }

    /// Abre a pasta e todas as que levam até ela.
    ///
    /// Criar algo dentro de uma pasta fechada esconde o que acabou de nascer;
    /// revelar o caminho é o que faz o resultado aparecer.
    pub fn reveal_in_explorer(&mut self, path: &Path) {
        for ancestor in path.ancestors() {
            if ancestor.starts_with(&self.workspace.path) && ancestor.is_dir() {
                self.expanded.insert(ancestor.to_path_buf());
            }
        }
        self.sync_explorer_tree();
    }

    pub fn from_tree(workspace: FileNode) -> Self {
        let workspace_name = workspace
            .path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("workspace")
            .to_owned();
        let mut expanded = HashSet::new();
        expanded.insert(workspace.path.clone());
        let terminal_root = if workspace.path.is_dir() {
            workspace.path.clone()
        } else {
            PathBuf::from(".")
        };
        let terminals = TerminalSession::discover_profiles()
            .into_iter()
            .filter_map(|profile| {
                TerminalSession::new(terminal_root.clone(), 2_000, profile)
                    .ok()
                    .map(|session| TerminalTab {
                        session,
                        scroll_line: 0,
                        follow_output: true,
                    })
            })
            .collect();
        let explorer_tree = TreeView::new(EXPLORER_TREE_ID, explorer_items(&workspace))
            .with_row_height(EXPLORER_ROW_HEIGHT);
        let mut shell = Self {
            workspace_name,
            workspace,
            explorer_tree,
            context_menu: ContextMenu::new(EXPLORER_CONTEXT_MENU_ID, Vec::new()),
            context_menu_target: None,
            expanded,
            editor: EditorSession::default(),
            editor_pane: EditorPane::new(EditorCapabilities::full()),
            focus: ShellFocus::None,
            search_query: String::new(),
            terminals,
            active_terminal: 0,
            explorer_scroll_x: 0.0,
            explorer_scroll_line: 0,
            sidebar_width: SIDEBAR_WIDTH,
            terminal_height: TERMINAL_DEFAULT_HEIGHT,
            terminal_last_height: TERMINAL_DEFAULT_HEIGHT,
            terminal_minimized: false,
            text_metrics: None,
            clipboard: None,
            navigated: None,
            sidebar_splitter: Splitter::new(SIDEBAR_SPLITTER_ID, SplitOrientation::Horizontal),
            terminal_splitter: Splitter::new(TERMINAL_SPLITTER_ID, SplitOrientation::Vertical),
            scrollbar_drag: None,
            editor_scrollbar: Scrollbar::new(EDITOR_SCROLLBAR_ID, ScrollbarOrientation::Vertical),
            terminal_scrollbar: Scrollbar::new(
                TERMINAL_SCROLLBAR_ID,
                ScrollbarOrientation::Vertical,
            ),
            explorer_vertical_scrollbar: Scrollbar::new(
                EXPLORER_VERTICAL_SCROLLBAR_ID,
                ScrollbarOrientation::Vertical,
            ),
            explorer_horizontal_scrollbar: Scrollbar::new(
                EXPLORER_HORIZONTAL_SCROLLBAR_ID,
                ScrollbarOrientation::Horizontal,
            ),
            terminal_selection: None,
            terminal_selecting: false,
            menu_bar: MenuBar::new(
                MENU_BAR_ID,
                vec![
                    MenuBarItem::menu(
                        "Arquivo",
                        vec![
                            MenuItem::new("Projeto...", "file.project"),
                            MenuItem::new("Salvar", "file.save"),
                        ],
                    ),
                    MenuBarItem::menu(
                        "Projeto",
                        vec![
                            MenuItem::new("Compilar projeto", "project.build"),
                            MenuItem::new("Reimportar projeto", "project.reimport"),
                            MenuItem::new("Executar aplicação", "project.run"),
                            MenuItem::new("Parar aplicação", "project.stop"),
                        ],
                    ),
                    MenuBarItem::menu(
                        "Depurar",
                        vec![
                            MenuItem::new("Conectar...", "debug.connect"),
                            MenuItem::new("Continuar", "debug.continue"),
                            MenuItem::new("Pausar", "debug.pause"),
                            MenuItem::new("Passo sobre", "debug.over"),
                            MenuItem::new("Entrar", "debug.into"),
                            MenuItem::new("Sair", "debug.out"),
                            MenuItem::new("Desconectar", "debug.detach"),
                        ],
                    ),
                    MenuBarItem::command("Configurações", "settings.open"),
                ],
            ),
            settings_modal: ModalHost::new(
                SETTINGS_MODAL_ID,
                "Configurações",
                Size::new(780.0, 460.0),
            ),
            jdk_combo: ComboBox::new(JDK_COMBO_ID, Vec::new()).with_command_prefix("jdk.select."),
            jdk_browse_button: Button::new(JDK_BROWSE_ID, "Procurar...").with_command("jdk.browse"),
            settings_close_button: Button::new(SETTINGS_CLOSE_ID, "Cancelar")
                .with_command("settings.cancel"),
            settings_save_button: Button::new(SETTINGS_SAVE_ID, "Salvar")
                .with_command("settings.save"),
            settings_pages: ListView::new(SETTINGS_PAGES_ID, SETTINGS_PAGE_TITLES)
                .with_row_height(SETTINGS_PAGE_ROW_HEIGHT)
                .with_selection(ListSelection::Marker),
            new_item_modal: ModalHost::new(NEW_ITEM_MODAL_ID, "", NEW_ITEM_PANEL_SIZE),
            new_item_dialog: None,
            new_item_package: TextInput::new(NEW_ITEM_PACKAGE_ID, String::new())
                .with_placeholder("br.com.exemplo"),
            new_item_name: TextInput::new(NEW_ITEM_NAME_ID, String::new()),
            new_item_create_button: Button::new(NEW_ITEM_CREATE_ID, "Criar")
                .with_command("new.create"),
            new_item_cancel_button: Button::new(NEW_ITEM_CANCEL_ID, "Cancelar")
                .with_command("new.cancel"),
            new_item_request: None,
            inspection_modal: ModalHost::new(
                INSPECTION_MODAL_ID,
                "Inspecionar",
                INSPECTION_PANEL_SIZE,
            ),
            inspection: None,
            inspection_tree: TreeView::new(INSPECTION_TREE_ID, Vec::new())
                .with_row_height(INSPECTION_ROW_HEIGHT),
            inspection_close_button: Button::new(INSPECTION_CLOSE_ID, "Fechar")
                .with_command("inspect.close"),
            inspection_editor: EditorPane::new(EditorCapabilities::plain()),
            inspection_source: TextBuffer::new(String::new()),
            inspection_run_button: Button::new(INSPECTION_RUN_ID, "Executar")
                .with_command("inspect.run"),
            inspection_focus: InspectionFocus::Tree,
            inspection_message: None,
            open_project_requested: false,
            open_settings_requested: false,
            build_project_requested: false,
            reimport_project_requested: false,
            run_requested: false,
            stop_requested: false,
            running_terminal: None,
            project_summary: None,
            browse_jdk_requested: false,
            pending_navigation: None,
            status_message: "Ready".to_owned(),
            syntax_snapshots: HashMap::new(),
            completion_items: Vec::new(),
            completion_selected: 0,
            settings_dialog: None,
            settings_jdk_result: None,
            settings_page: SettingsPage::default(),
            settings_focus: None,
            stop_button: Button::icon(STOP_BUTTON_ID, Icon::Stop, "Parar aplicação")
                .with_tint(IconTint::Muted)
                .with_command("project.stop"),
            run_button: Button::icon(RUN_BUTTON_ID, Icon::Play, "Executar aplicação")
                .with_tint(IconTint::Success)
                .with_command("project.run"),
            debug_button: Button::icon(DEBUG_BUTTON_ID, Icon::Bug, "Executar com depuração")
                .with_tint(IconTint::Muted)
                .with_command("debug.run"),
            debug_host: TextInput::new(DEBUG_HOST_ID, "127.0.0.1").with_placeholder("host"),
            debug_port: TextInput::new(DEBUG_PORT_ID, "8000").with_placeholder("porta"),
            debug_attach_button: Button::new(DEBUG_ATTACH_ID, "Conectar")
                .with_command("debug.attach"),
            theme: Theme::default(),
            breakpoints: BTreeMap::new(),
            verified_breakpoints: BTreeMap::new(),
            breakpoints_dirty: None,
            debug: DebugView::default(),
            pointer: Point::new(-1.0, -1.0),
            debug_frames: ListView::new(DEBUG_FRAMES_ID, Vec::<String>::new())
                .with_row_height(DEBUG_ROW_HEIGHT),
            debug_variables: ListView::new(DEBUG_VARIABLES_ID, Vec::<String>::new())
                .with_row_height(DEBUG_ROW_HEIGHT),
            debug_requests: Vec::new(),
        };
        shell.sync_explorer_tree();
        shell
    }

    /// Recebe o mecanismo que mede o texto que a janela desenha.
    pub fn set_clipboard(&mut self, clipboard: Arc<dyn ClipboardService>) {
        self.clipboard = Some(clipboard);
    }

    pub fn set_text_metrics(&mut self, metrics: Arc<dyn TextMetrics>) {
        self.text_metrics = Some(metrics);
    }

    /// Contexto de pintura com a medição disponível, quando houver.
    fn paint_context(&self) -> PaintContext {
        let context = PaintContext::with_theme(self.theme);
        match self.text_metrics.as_ref() {
            Some(metrics) => context.measuring(Arc::clone(metrics)),
            None => context,
        }
    }

    /// Contexto de layout com a medição disponível, quando houver.
    fn layout_context(&self) -> LayoutContext {
        match self.text_metrics.as_ref() {
            Some(metrics) => LayoutContext::with_text_metrics(Arc::clone(metrics)),
            None => LayoutContext::default(),
        }
    }

    pub fn open_file(&mut self, path: &Path) -> Result<DocumentId, String> {
        let id = self.editor.open(path).map_err(|error| error.to_string())?;
        self.editor_pane.set_cursor(0);
        self.focus = ShellFocus::Editor;
        self.status_message = format!("Opened {}", path.display());
        Ok(id)
    }

    pub const fn focus(&self) -> ShellFocus {
        self.focus
    }
    pub const fn active_document(&self) -> Option<DocumentId> {
        self.editor.active_id()
    }
    pub fn active_text(&self) -> Option<&str> {
        self.editor.active().map(|document| document.buffer.text())
    }
    pub fn document_snapshots(&self) -> Vec<DocumentSnapshot> {
        self.editor
            .tabs()
            .map(|document| DocumentSnapshot {
                id: document.id,
                path: document.path.clone(),
                version: document.buffer.revision(),
                text: document.buffer.text().to_owned(),
            })
            .collect()
    }
    pub fn set_syntax_snapshot(&mut self, snapshot: SyntaxSnapshot) {
        if self.editor.document(snapshot.document_id).is_none() {
            return;
        }
        let error_count = snapshot.diagnostics.len();
        let symbol_count = count_outline(&snapshot.outline);
        let import_count = snapshot.imports.len();
        self.status_message = format!(
            "Java: {error_count} error(s), {symbol_count} symbol(s), {import_count} import(s)"
        );
        self.syntax_snapshots.insert(snapshot.document_id, snapshot);
    }
    pub fn syntax_snapshot(&self, document_id: DocumentId) -> Option<&SyntaxSnapshot> {
        self.syntax_snapshots.get(&document_id)
    }
    pub fn active_outline(&self) -> &[OutlineItem] {
        self.active_document()
            .and_then(|id| self.syntax_snapshots.get(&id))
            .map_or(&[], |snapshot| snapshot.outline.as_slice())
    }
    pub fn completion_request(&self) -> Option<CompletionRequest> {
        let document = self.editor.active()?;
        let (line, column) = line_column(document.buffer.text(), self.editor_pane.cursor());
        Some(CompletionRequest {
            document_id: document.id,
            position: DomainTextPosition {
                line: line as u32,
                column: column as u32,
            },
            prefix: identifier_prefix(document.buffer.text(), self.editor_pane.cursor()),
        })
    }
    pub fn set_completions(&mut self, items: Vec<CompletionItem>) {
        self.completion_items = items;
        self.completion_selected = 0;
    }
    /// Mensagem atual da barra de estado.
    #[must_use]
    pub fn status_message(&self) -> &str {
        &self.status_message
    }

    pub fn set_status_message(&mut self, message: impl Into<String>) {
        self.status_message = message.into();
    }
    pub fn open_settings_dialog(&mut self, jdk_items: Vec<String>, selected_jdk: usize) {
        self.jdk_combo.set_items(
            jdk_items
                .into_iter()
                .enumerate()
                .map(|(index, label)| ComboBoxItem::new(label, index.to_string()))
                .collect(),
        );
        self.jdk_combo.set_selected(selected_jdk);
        self.settings_modal.open();
        // Reabrir a janela recomeça a transação: o que ficou pendente de uma
        // abertura anterior foi descartado com ela.
        self.settings_dialog = Some(SettingsDialog {
            message: None,
            pending_jdk: None,
            original_jdk: Some(selected_jdk),
            original_debug_host: self.debug_host.value().to_owned(),
            original_debug_port: self.debug_port.value().to_owned(),
        });
        self.settings_jdk_result = None;
        self.browse_jdk_requested = false;
    }

    /// Repõe a lista de JDKs e deixa um deles escolhido, sem sair da transação.
    ///
    /// É o que o `Procurar...` precisa: a instalação apontada entra na lista e
    /// fica pendente como qualquer escolha feita no combo. Reabrir a janela
    /// recomeçaria a transação e apagaria o que já estava pendente.
    pub fn set_jdk_options(&mut self, jdk_items: Vec<String>, pending: usize) {
        self.jdk_combo.set_items(
            jdk_items
                .into_iter()
                .enumerate()
                .map(|(index, label)| ComboBoxItem::new(label, index.to_string()))
                .collect(),
        );
        self.jdk_combo.set_selected(pending);
        if let Some(dialog) = self.settings_dialog.as_mut() {
            dialog.pending_jdk = Some(pending);
            dialog.message = None;
        }
    }

    pub const fn settings_dialog_open(&self) -> bool {
        self.settings_modal.is_open()
    }
    /// Troca o tema da interface.
    ///
    /// O tema vem da ERLibUi e vale para tudo — inclusive para os componentes da
    /// biblioteca, que o recebem pelo contexto de pintura. A IDE não guarda cor
    /// própria.
    pub fn set_theme(&mut self, theme: Theme) {
        self.theme = theme;
    }

    #[must_use]
    pub const fn theme(&self) -> &Theme {
        &self.theme
    }

    /// Alvo de depuração apresentado na janela e usado pelo botão de depurar.
    pub fn set_debug_target(&mut self, host: &str, port: u16) {
        self.debug_host.set_value(host);
        self.debug_port.set_value(port.to_string());
    }

    #[must_use]
    pub fn debug_target(&self) -> Option<(String, u16)> {
        let host = self.debug_host.value().trim().to_owned();
        let port = self.debug_port.value().trim().parse::<u16>().ok()?;
        (!host.is_empty() && port > 0).then_some((host, port))
    }

    /// Executa um comando na aba de terminal ativa, como se o usuário digitasse.
    pub fn run_in_terminal(&mut self, command: &str) -> Result<(), String> {
        let active = self.active_terminal;
        let Some(tab) = self.terminals.get_mut(active) else {
            return Err("Nenhum terminal disponível".to_owned());
        };
        tab.session
            .run(command)
            .map_err(|error| error.to_string())?;
        tab.follow_output = true;
        self.terminal_minimized = false;
        self.running_terminal = Some(active);
        Ok(())
    }

    /// Interrompe a aplicação iniciada pela IDE, como um `Ctrl+C` do usuário.
    pub fn stop_application(&mut self) -> Result<(), String> {
        let Some(index) = self.running_terminal else {
            return Err("Nenhuma aplicação iniciada pela IDE".to_owned());
        };
        let Some(tab) = self.terminals.get_mut(index) else {
            self.running_terminal = None;
            return Err("O terminal da aplicação não existe mais".to_owned());
        };
        tab.session.interrupt().map_err(|error| error.to_string())?;
        tab.follow_output = true;
        self.active_terminal = index;
        self.running_terminal = None;
        self.status_message = "Aplicação interrompida".to_owned();
        Ok(())
    }

    /// Indica que a IDE iniciou uma aplicação e ainda não a interrompeu.
    #[must_use]
    pub const fn application_running(&self) -> bool {
        self.running_terminal.is_some()
    }

    /// Escolhe a página apresentada pela janela de configurações.
    ///
    /// Abrir a janela pelo menu mantém a última página usada; atalhos que
    /// prometem uma página específica precisam declará-la.
    pub fn set_settings_page(&mut self, page: SettingsPage) {
        self.settings_page = page;
        self.settings_focus = None;
    }
    #[must_use]
    pub const fn settings_page(&self) -> SettingsPage {
        self.settings_page
    }
    pub fn take_settings_jdk_result(&mut self) -> Option<usize> {
        self.settings_jdk_result.take()
    }
    pub fn take_browse_jdk_request(&mut self) -> bool {
        std::mem::take(&mut self.browse_jdk_requested)
    }
    pub fn set_settings_message(&mut self, message: impl Into<String>) {
        if let Some(dialog) = self.settings_dialog.as_mut() {
            dialog.message = Some(message.into());
        }
    }
    pub fn append_tool_output(&mut self, text: &str, is_error: bool) {
        let active = self.active_terminal;
        self.terminals[active]
            .session
            .append_external_output(text, is_error);
        self.terminals[active].follow_output = true;
        self.terminals[active].scroll_line = self.terminals[active].session.line_count();
    }
    pub fn java_source_files(&self) -> Vec<PathBuf> {
        fn collect(node: &FileNode, output: &mut Vec<PathBuf>) {
            if node.is_directory {
                for child in &node.children {
                    collect(child, output);
                }
            } else if node
                .path
                .extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| extension.eq_ignore_ascii_case("java"))
            {
                output.push(node.path.clone());
            }
        }
        let mut files = Vec::new();
        collect(&self.workspace, &mut files);
        files
    }
    pub fn navigation_hover(&self, point: Point, size: Size, control: bool) -> bool {
        if !control {
            return false;
        }
        let geometry = self.geometry(size);
        let editor_x = ACTIVITY_WIDTH + self.sidebar_width(size);
        if point.x < editor_x + EDITOR_GUTTER
            || point.y < geometry.content_top
            || point.y >= geometry.editor_bottom
        {
            return false;
        }
        let Some(document) = self.editor.active() else {
            return false;
        };
        let mut pane = self.editor_pane.clone();
        pane.set_bounds(Rect::new(
            editor_x,
            geometry.content_top,
            geometry.editor_width,
            geometry.editor_height,
        ));
        let offset = pane.offset_at_point(&document.buffer, point);
        if token_at(document.buffer.text(), offset).is_none() {
            return false;
        }
        let (line, column) = line_column(document.buffer.text(), offset);
        self.syntax_snapshots
            .get(&document.id)
            .filter(|snapshot| snapshot.version == document.buffer.revision())
            .is_some_and(|snapshot| {
                snapshot.highlights.iter().any(|highlight| {
                    is_navigable(highlight.kind) && position_in_range(line, column, highlight.range)
                })
            })
    }
    pub fn tab_count(&self) -> usize {
        self.editor.tabs().count()
    }

    /// Caminhos das abas abertas, na ordem em que aparecem.
    ///
    /// Documentos criados em memória não têm arquivo por trás e ficam de fora:
    /// registrar um caminho que não existe só produziria uma aba impossível de
    /// reabrir.
    #[must_use]
    pub fn open_document_paths(&self) -> Vec<PathBuf> {
        self.editor
            .tabs()
            .filter(|document| document.path.is_file())
            .map(|document| document.path.clone())
            .collect()
    }

    /// Caminho do documento em foco.
    #[must_use]
    pub fn active_document_path(&self) -> Option<PathBuf> {
        self.editor
            .active()
            .map(|document| document.path.clone())
            .filter(|path| path.is_file())
    }
    pub fn is_expanded(&self, path: &Path) -> bool {
        self.expanded.contains(path)
    }
    pub fn selected_shell(&self) -> ShellKind {
        self.active_terminal().selected_profile().kind
    }
    pub const fn editor_scroll_line(&self) -> usize {
        self.editor_pane.scroll_line()
    }
    pub fn terminal_scroll_line(&self) -> usize {
        self.terminals[self.active_terminal].scroll_line
    }
    pub const fn active_terminal_index(&self) -> usize {
        self.active_terminal
    }
    pub const fn terminal_height(&self) -> f32 {
        self.terminal_height
    }
    pub const fn terminal_minimized(&self) -> bool {
        self.terminal_minimized
    }
    pub const fn terminal_resizing(&self) -> bool {
        self.terminal_splitter.is_dragging()
    }
    pub const fn sidebar_resizing(&self) -> bool {
        self.sidebar_splitter.is_dragging()
    }
    pub fn active_terminal_lines(&self) -> impl Iterator<Item = &str> {
        self.active_terminal()
            .lines()
            .map(|line| line.text.as_str())
    }
    pub fn take_navigation_request(&mut self) -> Option<NavigationRequest> {
        self.pending_navigation.take()
    }
    pub fn take_open_project_request(&mut self) -> bool {
        std::mem::take(&mut self.open_project_requested)
    }
    /// Alterna o breakpoint de uma linha e marca o arquivo para sincronização.
    pub fn toggle_breakpoint(&mut self, path: &Path, line: u32) {
        let lines = self.breakpoints.entry(path.to_path_buf()).or_default();
        if !lines.remove(&line) {
            lines.insert(line);
        }
        if lines.is_empty() {
            self.breakpoints.remove(path);
        }
        self.breakpoints_dirty = Some(path.to_path_buf());
        self.status_message = format!("Breakpoints: {}", self.breakpoint_count());
    }

    /// Alterna o breakpoint na linha do cursor do documento ativo.
    pub fn toggle_breakpoint_at_cursor(&mut self) {
        let Some(document) = self.editor.active() else {
            return;
        };
        let path = document.path.clone();
        let (line, _) = line_column(document.buffer.text(), self.editor_pane.cursor());
        self.toggle_breakpoint(&path, line as u32);
    }

    #[must_use]
    pub fn breakpoints_for(&self, path: &Path) -> Vec<u32> {
        self.breakpoints
            .get(path)
            .map(|lines| lines.iter().copied().collect())
            .unwrap_or_default()
    }

    #[must_use]
    pub fn breakpoint_count(&self) -> usize {
        self.breakpoints.values().map(BTreeSet::len).sum()
    }

    /// Registra quais linhas o alvo confirmou, para diferenciá-las na calha.
    pub fn set_verified_breakpoints(&mut self, path: &Path, lines: &[u32]) {
        if lines.is_empty() {
            self.verified_breakpoints.remove(path);
        } else {
            self.verified_breakpoints
                .insert(path.to_path_buf(), lines.iter().copied().collect());
        }
    }

    #[must_use]
    pub fn breakpoint_is_verified(&self, path: &Path, line: u32) -> bool {
        self.verified_breakpoints
            .get(path)
            .is_some_and(|lines| lines.contains(&line))
    }

    /// Arquivo cujos breakpoints mudaram desde a última consulta.
    pub fn take_breakpoints_dirty(&mut self) -> Option<PathBuf> {
        self.breakpoints_dirty.take()
    }

    pub fn take_debug_requests(&mut self) -> Vec<DebugRequest> {
        std::mem::take(&mut self.debug_requests)
    }

    pub fn request_debug(&mut self, request: DebugRequest) {
        self.debug_requests.push(request);
    }

    /// Substitui o estado de depuração apresentado.
    pub fn set_debug_view(&mut self, view: DebugView) {
        if let Some((path, line)) = view.stopped_at.clone()
            && self.debug.stopped_at.as_ref() != Some(&(path.clone(), line))
        {
            let _ = self.open_location(&path, line as usize, 0);
        }
        self.debug = view;
        self.debug_frames.set_items(
            self.debug
                .frames
                .iter()
                .map(|frame| match &frame.location {
                    Some((_, line)) => format!("{}:{}", frame.name, line + 1),
                    None => frame.name.clone(),
                })
                .collect::<Vec<_>>(),
        );
        self.debug_frames.set_selected(Some(self.debug.selected_frame));
        self.debug_variables.set_items(
            self.debug
                .variables
                .iter()
                .map(|variable| format!("{} = {}", variable.name, variable.value))
                .collect::<Vec<_>>(),
        );
    }

    #[must_use]
    pub const fn debug_view(&self) -> &DebugView {
        &self.debug
    }

    #[must_use]
    pub const fn debug_attached(&self) -> bool {
        self.debug.attached
    }

    pub fn take_build_project_request(&mut self) -> bool {
        std::mem::take(&mut self.build_project_requested)
    }
    pub fn take_reimport_project_request(&mut self) -> bool {
        std::mem::take(&mut self.reimport_project_requested)
    }
    /// Pedido de executar a aplicação, sem depuração.
    pub fn take_run_request(&mut self) -> bool {
        std::mem::take(&mut self.run_requested)
    }
    /// Pedido de interromper a aplicação iniciada pela IDE.
    pub fn take_stop_request(&mut self) -> bool {
        std::mem::take(&mut self.stop_requested)
    }
    /// Resumo do projeto importado, apresentado na barra de status.
    pub fn set_project_summary(&mut self, summary: Option<String>) {
        self.project_summary = summary;
    }
    pub fn project_summary(&self) -> Option<&str> {
        self.project_summary.as_deref()
    }
    pub fn take_open_settings_request(&mut self) -> bool {
        std::mem::take(&mut self.open_settings_requested)
    }
    pub fn workspace_path(&self) -> &Path {
        &self.workspace.path
    }
    pub fn active_terminal_input(&self) -> &str {
        self.active_terminal().input()
    }

    fn active_terminal(&self) -> &TerminalSession {
        &self.terminals[self.active_terminal].session
    }

    fn active_terminal_mut(&mut self) -> &mut TerminalSession {
        &mut self.terminals[self.active_terminal].session
    }

    pub fn update_terminals(&mut self, size: Size) -> bool {
        let geo = self.geometry(size);
        let rows = ((geo.terminal_height - 62.0) / EDITOR_LINE_HEIGHT).max(1.0) as u16;
        let mut changed = false;
        for terminal in &mut self.terminals {
            let received = terminal.session.drain_output();
            changed |= received > 0;
            if received > 0 && terminal.follow_output {
                terminal.scroll_line = terminal.session.line_count().saturating_sub(rows as usize);
            }
        }
        changed
    }

    fn geometry(&self, size: Size) -> Geometry {
        let mut geometry = geometry(
            size,
            if self.terminal_minimized {
                TERMINAL_COLLAPSED_HEIGHT
            } else {
                self.terminal_height
            },
            self.sidebar_width(size),
        );
        // O painel de depuração ocupa a direita do editor enquanto há sessão,
        // em vez de cobrir o código.
        if self.debug.attached {
            geometry.editor_width =
                (geometry.editor_width - DEBUG_PANEL_WIDTH).max(EDITOR_GUTTER * 2.0);
        }
        geometry
    }

    /// Abas do editor montadas a partir dos documentos abertos.
    ///
    /// O widget é reconstruído a cada uso porque a verdade são os documentos, e
    /// não uma cópia deles: assim nenhuma abertura, gravação ou fechamento
    /// precisa lembrar de sincronizar a barra de abas.
    fn editor_tabs(&self) -> Tabs {
        let items = self
            .editor
            .tabs()
            .map(|document| {
                let title = document
                    .path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("?");
                TabItem::new(document.id.0, title)
                    .closable()
                    .modified(document.buffer.is_dirty())
            })
            .collect();
        let mut tabs = Tabs::new(EDITOR_TABS_ID, items)
            .with_tab_width(TAB_WIDTH)
            .with_pointer(self.pointer);
        if let Some(active) = self.editor.active_id() {
            tabs.set_active_id(active.0);
        }
        tabs
    }

    fn editor_tabs_rect(&self, size: Size) -> Rect {
        let geo = self.geometry(size);
        Rect::new(
            ACTIVITY_WIDTH + self.sidebar_width(size),
            TITLE_HEIGHT,
            geo.editor_width,
            TAB_HEIGHT,
        )
    }

    /// Abas do painel de terminal, uma por perfil aberto. Terminais não fecham
    /// pela aba: eles pertencem à janela enquanto ela existir.
    fn terminal_tabs(&self) -> Tabs {
        let items = self
            .terminals
            .iter()
            .enumerate()
            .map(|(index, terminal)| {
                TabItem::new(index as u64, terminal.session.selected_profile().kind.label())
            })
            .collect();
        let mut tabs = Tabs::new(TERMINAL_TABS_ID, items).with_tab_width(TERMINAL_TAB_WIDTH);
        tabs.set_active(self.active_terminal);
        tabs
    }

    fn terminal_tabs_rect(&self, size: Size) -> Rect {
        let geo = self.geometry(size);
        Rect::new(
            ACTIVITY_WIDTH + self.sidebar_width(size),
            geo.editor_bottom,
            geo.editor_width,
            TERMINAL_TAB_HEIGHT,
        )
    }

    /// Trilha, faixa e deslocamento de uma barra, na unidade do seu conteúdo.
    ///
    /// As barras verticais contam linhas e a horizontal conta pontos de largura:
    /// para o componente é a mesma aritmética, e é por isso que uma só barra
    /// serve às quatro áreas.
    fn scrollbar_range(&self, target: ScrollTarget, size: Size) -> (Rect, f32, f32, f32) {
        match target {
            ScrollTarget::Editor => (
                self.editor_scrollbar_rect(size),
                self.active_text().map_or(0, |text| text.lines().count()) as f32,
                self.editor_visible_lines(size) as f32,
                self.editor_pane.scroll_line() as f32,
            ),
            ScrollTarget::Terminal => {
                let active = &self.terminals[self.active_terminal];
                (
                    self.terminal_scrollbar_rect(size),
                    active.session.line_count() as f32,
                    self.terminal_visible_lines(size) as f32,
                    active.scroll_line as f32,
                )
            }
            ScrollTarget::ExplorerVertical => (
                self.explorer_vertical_scrollbar_rect(size),
                self.visible_entries().len() as f32,
                self.explorer_visible_lines(size) as f32,
                self.explorer_scroll_line as f32,
            ),
            ScrollTarget::ExplorerHorizontal => {
                let track = self.explorer_horizontal_scrollbar_rect(size);
                (
                    track,
                    self.explorer_content_width(size),
                    (track.size.width - 28.0).max(1.0),
                    self.explorer_scroll_x,
                )
            }
        }
    }

    fn scrollbar_mut(&mut self, target: ScrollTarget) -> &mut Scrollbar {
        match target {
            ScrollTarget::Editor => &mut self.editor_scrollbar,
            ScrollTarget::Terminal => &mut self.terminal_scrollbar,
            ScrollTarget::ExplorerVertical => &mut self.explorer_vertical_scrollbar,
            ScrollTarget::ExplorerHorizontal => &mut self.explorer_horizontal_scrollbar,
        }
    }

    /// Alinha a barra ao conteúdo atual antes de entregar-lhe um evento.
    fn sync_scrollbar(&mut self, target: ScrollTarget, size: Size) {
        let (track, content, viewport, offset) = self.scrollbar_range(target, size);
        let context = self.layout_context();
        let bar = self.scrollbar_mut(target);
        bar.layout(&context, track);
        bar.set_range(content, viewport);
        bar.set_offset(offset);
    }

    /// Traz de volta o deslocamento que a barra escolheu.
    fn apply_scrollbar(&mut self, target: ScrollTarget) {
        let offset = self.scrollbar_mut(target).offset();
        match target {
            ScrollTarget::Editor => self
                .editor_pane
                .set_scroll_line(offset.round().max(0.0) as usize),
            ScrollTarget::Terminal => {
                let maximum = self.terminal_scrollbar.max_offset();
                let active = self.active_terminal;
                self.terminals[active].scroll_line = offset.round().max(0.0) as usize;
                // Chegar ao fim volta a acompanhar a saída; parar no meio é
                // pedir para ficar onde está.
                self.terminals[active].follow_output = offset >= maximum;
            }
            ScrollTarget::ExplorerVertical => {
                self.explorer_scroll_line = offset.round().max(0.0) as usize;
            }
            ScrollTarget::ExplorerHorizontal => self.explorer_scroll_x = offset.max(0.0),
        }
    }

    /// Entrega o clique à barra cuja trilha o contém.
    fn scrollbar_pointer_down(&mut self, point: Point, size: Size) -> bool {
        for target in [
            ScrollTarget::Terminal,
            ScrollTarget::Editor,
            ScrollTarget::ExplorerHorizontal,
            ScrollTarget::ExplorerVertical,
        ] {
            if target == ScrollTarget::Terminal && self.terminal_minimized {
                continue;
            }
            let (track, ..) = self.scrollbar_range(target, size);
            if !track.contains(point) {
                continue;
            }
            self.sync_scrollbar(target, size);
            let handled = self.scrollbar_mut(target).event(
                &mut EventContext::default(),
                &UiEvent::PointerDown(primary_pointer(point)),
            );
            if matches!(handled, EventResult::Handled) {
                self.apply_scrollbar(target);
                self.scrollbar_drag = Some(target);
            }
            // A trilha consome o clique mesmo sem indicador: ali não há
            // conteúdo para reagir embaixo.
            return true;
        }
        false
    }

    /// Desenha uma barra com o componente da biblioteca.
    fn paint_scrollbar(&self, target: ScrollTarget, size: Size) -> Vec<PaintCommand> {
        let (track, content, viewport, offset) = self.scrollbar_range(target, size);
        let orientation = match target {
            ScrollTarget::ExplorerHorizontal => ScrollbarOrientation::Horizontal,
            _ => ScrollbarOrientation::Vertical,
        };
        let mut bar = Scrollbar::new(WidgetId(0), orientation).with_range(content, viewport);
        bar.layout(&self.layout_context(), track);
        bar.set_offset(offset);
        let mut paint = self.paint_context();
        bar.paint(&mut paint);
        paint.into_commands()
    }

    /// Área da árvore de arquivos.
    fn explorer_tree_rect(&self, size: Size) -> Rect {
        let geo = self.geometry(size);
        Rect::new(
            ACTIVITY_WIDTH,
            EXPLORER_TOP,
            self.sidebar_width(size),
            (geo.content_bottom - 12.0 - EXPLORER_TOP).max(0.0),
        )
    }

    /// Espelha na árvore o que a IDE considera expandido.
    ///
    /// A expansão continua sendo do shell porque ela é indexada por caminho e
    /// serve a mais gente do que ao desenho; a árvore recebe as identidades
    /// correspondentes.
    fn sync_explorer_tree(&mut self) {
        let ids: Vec<u64> = self.expanded.iter().map(|path| explorer_id(path)).collect();
        self.explorer_tree.set_expanded(ids);
    }

    /// Posiciona a árvore de acordo com as barras de rolagem da janela.
    fn explorer_tree_for(&self, size: Size) -> TreeView {
        let mut tree = self.explorer_tree.clone();
        tree.layout(&self.layout_context(), self.explorer_tree_rect(size));
        tree.set_scroll_offset(Point::new(
            self.explorer_scroll_x,
            self.explorer_scroll_line as f32 * EXPLORER_ROW_HEIGHT,
        ));
        tree
    }

    fn explorer_path_for(&self, id: u64) -> Option<(PathBuf, bool)> {
        fn visit(node: &FileNode, id: u64) -> Option<(PathBuf, bool)> {
            if explorer_id(&node.path) == id {
                return Some((node.path.clone(), node.is_directory));
            }
            node.children.iter().find_map(|child| visit(child, id))
        }
        visit(&self.workspace, id)
    }

    /// Divisor da barra lateral posicionado pelo layout atual.
    ///
    /// A barra lateral é limitada pela largura mínima dela e pela do editor; o
    /// terminal, pela altura mínima dele e pelo espaço que o editor precisa
    /// manter. São limites em pontos, não proporções.
    fn sidebar_splitter_for(&self, size: Size) -> Splitter {
        let geometry = self.geometry(size);
        let mut splitter = self.sidebar_splitter.clone();
        splitter.layout(
            &self.layout_context(),
            Rect::new(
                0.0,
                TITLE_HEIGHT,
                size.width,
                (geometry.content_bottom - TITLE_HEIGHT).max(0.0),
            ),
        );
        splitter.set_range(
            ACTIVITY_WIDTH + SIDEBAR_MIN_WIDTH,
            ACTIVITY_WIDTH + (size.width - 320.0).max(SIDEBAR_MIN_WIDTH),
        );
        splitter.set_position(ACTIVITY_WIDTH + self.sidebar_width(size));
        splitter
    }

    fn terminal_splitter_for(&self, size: Size) -> Splitter {
        let geometry = self.geometry(size);
        let editor_x = ACTIVITY_WIDTH + self.sidebar_width(size);
        let maximum =
            (geometry.content_bottom - geometry.content_top - 100.0).max(TERMINAL_MIN_HEIGHT);
        let mut splitter = self.terminal_splitter.clone();
        splitter.layout(
            &self.layout_context(),
            Rect::new(
                editor_x,
                geometry.content_top,
                (size.width - editor_x).max(0.0),
                (geometry.content_bottom - geometry.content_top).max(0.0),
            ),
        );
        splitter.set_range(
            geometry.content_bottom - maximum,
            geometry.content_bottom - TERMINAL_MIN_HEIGHT,
        );
        splitter.set_position(geometry.editor_bottom);
        splitter
    }

    fn sync_splitters(&mut self, size: Size) {
        self.sidebar_splitter = self.sidebar_splitter_for(size);
        self.terminal_splitter = self.terminal_splitter_for(size);
    }

    /// Traz de volta o tamanho que cada divisor definiu.
    fn apply_splitters(&mut self, size: Size) {
        let content_bottom = self.geometry(size).content_bottom;
        if self.sidebar_splitter.is_dragging() {
            self.sidebar_width = self.sidebar_splitter.position() - ACTIVITY_WIDTH;
        }
        if self.terminal_splitter.is_dragging() {
            self.terminal_height = content_bottom - self.terminal_splitter.position();
            self.terminal_last_height = self.terminal_height;
        }
    }

    /// Entrega o clique ao divisor cujo alvo o contém.
    fn splitter_pointer_down(&mut self, point: Point, size: Size) -> bool {
        self.sync_splitters(size);
        let event = UiEvent::PointerDown(primary_pointer(point));
        let mut context = EventContext::default();
        if matches!(
            self.sidebar_splitter.event(&mut context, &event),
            EventResult::Handled
        ) {
            return true;
        }
        !self.terminal_minimized
            && matches!(
                self.terminal_splitter.event(&mut context, &event),
                EventResult::Handled
            )
    }


    fn editor_view_rect(&self, size: Size) -> Rect {
        let geometry = self.geometry(size);
        Rect::new(
            ACTIVITY_WIDTH + self.sidebar_width(size),
            geometry.content_top,
            geometry.editor_width,
            geometry.editor_height,
        )
    }

    /// Alinha o painel de edição ao documento, ao realce e às decorações.
    ///
    /// O texto vive no documento; o painel guarda cursor, seleção, rolagem e a
    /// cópia de desenho, e só refaz esta última quando a revisão muda.
    fn sync_editor_pane(&mut self, size: Size) {
        let Some(document) = self.editor.active() else {
            return;
        };
        let (id, revision, path) = (document.id, document.buffer.revision(), document.path.clone());
        let syntax = self.editor_syntax(id, revision);
        let decorations = self.editor_decorations(&path);
        let focused = self.focus == ShellFocus::Editor;
        let bounds = self.editor_view_rect(size);
        let context = self.layout_context();
        self.editor_pane.set_bounds(bounds);
        let Some(document) = self.editor.active() else {
            return;
        };
        self.editor_pane
            .sync(&context, &document.buffer, &syntax, decorations, focused);
    }

    /// Converte o realce da IDE, que fala em linha e coluna, para os intervalos
    /// absolutos que o editor da biblioteca usa.
    /// Converte o realce, que fala em linha e coluna, para deslocamentos em
    /// caracteres — que é como o editor da biblioteca conta.
    fn editor_syntax(&self, id: DocumentId, revision: u64) -> Vec<(usize, usize, TokenKind)> {
        let Some(snapshot) = self
            .syntax_snapshots
            .get(&id)
            .filter(|snapshot| snapshot.version == revision)
        else {
            return Vec::new();
        };
        let Some(text) = self.active_text() else {
            return Vec::new();
        };
        snapshot
            .highlights
            .iter()
            .map(|highlight| {
                (
                    char_offset_of(text, highlight.range.start),
                    char_offset_of(text, highlight.range.end),
                    token_kind_for(highlight.kind),
                )
            })
            .collect()
    }

    /// Pontos de parada e a linha em que a execução parou.
    fn editor_decorations(&self, path: &Path) -> Vec<LineDecoration> {
        let mut decorations: Vec<LineDecoration> = self
            .breakpoints
            .get(path)
            .into_iter()
            .flatten()
            .map(|line| {
                // Confirmado pelo alvo é um disco; ainda não registrado — sem
                // sessão, ou classe não carregada — é só o contorno.
                let mark = if self.breakpoint_is_verified(path, *line) {
                    GutterMark::Breakpoint
                } else {
                    GutterMark::PendingBreakpoint
                };
                LineDecoration::mark(*line as usize, mark)
            })
            .collect();
        let destacar = |decorations: &mut Vec<LineDecoration>, line: usize| {
            match decorations.iter_mut().find(|item| item.line == line) {
                Some(existing) => *existing = existing.with_highlight(),
                None => decorations.push(LineDecoration::highlight(line)),
            }
        };
        if let Some((_, line)) = self
            .debug
            .stopped_at
            .as_ref()
            .filter(|(stopped, _)| stopped == path)
        {
            destacar(&mut decorations, *line as usize);
        }
        // Destino da última navegação, enquanto o cursor não sair de lá.
        if let Some((line, cursor)) = self.navigated
            && cursor == self.editor_pane.cursor()
        {
            destacar(&mut decorations, line);
        }
        decorations
    }

    fn debug_panel_rect(&self, size: Size) -> Rect {
        let geometry = self.geometry(size);
        let x = ACTIVITY_WIDTH + self.sidebar_width(size) + geometry.editor_width;
        Rect::new(
            x,
            geometry.content_top,
            (size.width - x).max(0.0),
            geometry.editor_height,
        )
    }

    fn paint_debug_panel(&self, size: Size, colors: ColorTokens) -> Vec<PaintCommand> {
        let geometry = debug_panel_geometry(self.debug_panel_rect(size), self.debug.frames.len());
        let panel = geometry.panel;
        let mut commands = vec![
            fill(panel, colors.surface),
            fill(
                Rect::new(panel.origin.x, panel.origin.y, 1.0, panel.size.height),
                colors.border,
            ),
            PaintCommand::PushClip(panel),
            label(
                &self.debug.status,
                Point::new(panel.origin.x + 12.0, panel.origin.y + 10.0),
                if self.debug.is_stopped() {
                    colors.accent
                } else {
                    colors.text
                },
                13.0,
            ),
        ];

        for (rect, (title, _)) in geometry.buttons.iter().zip(DEBUG_BUTTONS) {
            commands.push(fill(*rect, colors.elevated));
            commands.push(stroke(*rect, colors.border));
            commands.push(label(
                title,
                Point::new(rect.origin.x + 8.0, rect.origin.y + 7.0),
                if self.debug.is_stopped() {
                    colors.text
                } else {
                    colors.muted_text
                },
                12.0,
            ));
        }

        commands.push(label(
            "Pilha de chamadas",
            Point::new(panel.origin.x + 12.0, geometry.frames.origin.y - 20.0),
            colors.muted_text,
            12.0,
        ));
        let mut frames = self.debug_frames.clone();
        frames.layout(&self.layout_context(), geometry.frames);
        let mut variables = self.debug_variables.clone();
        variables.layout(&self.layout_context(), geometry.variables);
        let mut lists = self.paint_context();
        frames.paint(&mut lists);

        commands.push(label(
            "Variáveis",
            Point::new(panel.origin.x + 12.0, geometry.variables.origin.y - 20.0),
            colors.muted_text,
            12.0,
        ));
        variables.paint(&mut lists);
        commands.extend(lists.into_commands());
        commands.push(PaintCommand::PopClip);
        commands
    }

    fn debug_panel_pointer_down(&mut self, point: Point, size: Size) {
        let geometry = debug_panel_geometry(self.debug_panel_rect(size), self.debug.frames.len());
        for (rect, (_, request)) in geometry.buttons.iter().zip(DEBUG_BUTTONS) {
            if rect.contains(point) {
                self.debug_requests.push(request);
                return;
            }
        }
        // A lista resolve qual linha foi clicada; a IDE só reage à escolha.
        // Clicar fora das linhas é ignorado por ela, e é isso que distingue uma
        // escolha de um clique no vazio do painel.
        self.debug_frames.layout(&self.layout_context(), geometry.frames);
        let result = self.debug_frames.event(
            &mut EventContext::default(),
            &UiEvent::PointerDown(primary_pointer(point)),
        );
        if matches!(result, EventResult::Ignored) {
            return;
        }
        let Some(row) = self.debug_frames.selected() else {
            return;
        };
        self.debug.selected_frame = row;
        self.debug_requests.push(DebugRequest::SelectFrame(row));
        if let Some((path, line)) = self
            .debug
            .frames
            .get(row)
            .and_then(|frame| frame.location.clone())
        {
            let _ = self.open_location(&path, line as usize, 0);
        }
    }

    fn sidebar_width(&self, size: Size) -> f32 {
        self.sidebar_width.clamp(
            SIDEBAR_MIN_WIDTH,
            (size.width - 320.0).max(SIDEBAR_MIN_WIDTH),
        )
    }

    pub fn toggle_search(&mut self) {
        if self.focus == ShellFocus::Search {
            self.search_query.clear();
            self.focus = ShellFocus::Editor;
        } else {
            self.focus = ShellFocus::Search;
        }
    }

    pub fn escape(&mut self) {
        // O menu de contexto é o que está por cima de tudo: é ele que Esc
        // dispensa primeiro.
        if self.context_menu_key("Escape", Modifiers::default()) {
            return;
        }
        if self.inspection_modal.is_open() {
            self.close_inspection();
            return;
        }
        if self.new_item_modal.is_open() {
            self.close_new_item_dialog();
            return;
        }
        // Esc na janela de configurações é cancelar: fechar sem descartar o que
        // foi mexido salvaria pela porta dos fundos.
        if self.settings_modal.is_open() {
            self.cancel_settings();
            return;
        }
        if !self.completion_items.is_empty() {
            self.completion_items.clear();
            return;
        }
        if self.focus == ShellFocus::Search {
            self.search_query.clear();
            self.focus = ShellFocus::Editor;
        }
    }

    pub fn pointer_down(&mut self, point: Point, size: Size) {
        self.pointer_down_with_modifiers(point, size, false);
    }

    /// Clique secundário: abre o menu de contexto sobre o item do Explorer.
    ///
    /// Fora do Explorer o clique só dispensa um menu aberto. Enquanto não
    /// houver menu para as outras áreas, abrir um vazio prometeria ações que
    /// não existem.
    pub fn secondary_pointer_down(&mut self, point: Point, size: Size) {
        self.context_menu.close();
        self.context_menu_target = None;
        if self.settings_modal.is_open() {
            return;
        }
        let geometry = self.geometry(size);
        let editor_x = ACTIVITY_WIDTH + self.sidebar_width(size);
        // No editor o menu fala do texto: copiar e colar.
        if point.x >= editor_x
            && point.x < editor_x + geometry.editor_width
            && point.y >= geometry.content_top
            && point.y < geometry.editor_bottom
        {
            self.focus = ShellFocus::Editor;
            self.context_menu.set_entries(editor_menu_entries(
                self.editor_pane.selection_range().is_some(),
                self.debug.attached,
            ));
            self.context_menu.layout(
                &self.layout_context(),
                Rect::new(0.0, 0.0, size.width, size.height),
            );
            self.context_menu.open_at(point);
            return;
        }
        if point.x < ACTIVITY_WIDTH || point.x >= editor_x || point.y < EXPLORER_TOP {
            return;
        }
        // Qual nó está sob o ponteiro é a árvore quem sabe: recuo, deslocamento
        // horizontal e virtualização são dela.
        let mut tree = self.explorer_tree_for(size);
        tree.event(
            &mut EventContext::default(),
            &UiEvent::PointerDown(primary_pointer(point)),
        );
        let Some((path, is_directory)) = tree.selected().and_then(|id| self.explorer_path_for(id))
        else {
            return;
        };
        self.focus = ShellFocus::Explorer;
        self.explorer_tree.set_selected(Some(explorer_id(&path)));
        // O alvo é o diretório: clicando em um arquivo, é na pasta dele que a
        // criação acontece.
        let target = if is_directory {
            path
        } else {
            path.parent().map(Path::to_path_buf).unwrap_or(path)
        };
        self.context_menu.set_entries(explorer_menu_entries(&target));
        self.context_menu.layout(
            &self.layout_context(),
            Rect::new(0.0, 0.0, size.width, size.height),
        );
        self.context_menu.open_at(point);
        self.context_menu_target = Some(target);
    }

    pub fn context_menu_open(&self) -> bool {
        self.context_menu.is_open()
    }

    /// Entrega o evento ao menu aberto e trata o comando escolhido.
    ///
    /// Devolve `true` quando o menu consumiu o evento — é o sinal de que o
    /// clique ou a tecla não devem seguir para o que está embaixo dele.
    fn context_menu_event(&mut self, event: &UiEvent, size: Size) -> bool {
        if !self.context_menu.is_open() {
            return false;
        }
        self.context_menu.layout(
            &self.layout_context(),
            Rect::new(0.0, 0.0, size.width, size.height),
        );
        let mut context = EventContext::default();
        let result = self.context_menu.event(&mut context, event);
        if let EventResult::Action(WidgetAction::Command(command)) = &result {
            self.run_explorer_command(&command.0);
        }
        if !self.context_menu.is_open() {
            self.context_menu_target = None;
        }
        result != EventResult::Ignored
    }

    /// Entrega a tecla ao menu aberto.
    ///
    /// Separado do caminho do ponteiro porque navegar por teclado não depende
    /// de onde o menu foi desenhado, e assim não precisa do tamanho da janela.
    fn context_menu_key(&mut self, key: &str, modifiers: Modifiers) -> bool {
        if !self.context_menu.is_open() {
            return false;
        }
        let mut context = EventContext::default();
        let result = self.context_menu.event(
            &mut context,
            &UiEvent::KeyDown(KeyEvent {
                logical_key: key.to_owned(),
                repeat: false,
                modifiers,
            }),
        );
        if let EventResult::Action(WidgetAction::Command(command)) = &result {
            self.run_explorer_command(&command.0);
        }
        if !self.context_menu.is_open() {
            self.context_menu_target = None;
        }
        result != EventResult::Ignored
    }

    fn run_explorer_command(&mut self, command: &str) {
        match command {
            "editor.copy" => {
                self.copy_selection();
                return;
            }
            "editor.paste" => {
                self.paste_clipboard();
                return;
            }
            "debug.inspect" => {
                self.inspect_selection();
                return;
            }
            _ => {}
        }
        let Some(target) = self.context_menu_target.clone() else {
            return;
        };
        let kind = match command {
            "explorer.new.package" => NewItemKind::Package,
            "explorer.new.class" => NewItemKind::Class,
            "explorer.new.interface" => NewItemKind::Interface,
            "explorer.new.folder" => {
                self.status_message = format!("Nova pasta em {}", target.display());
                return;
            }
            _ => return,
        };
        self.open_new_item_dialog(kind, &target);
    }

    /// Abre a janela de criação com o pacote do alvo já preenchido.
    ///
    /// O pacote vem do caminho clicado, em notação de ponto: é o que o usuário vê
    /// no Explorer e o que ele vai editar para criar um pacote abaixo. Sem raiz de
    /// fontes não há pacote, e a janela não abre — o menu que oferece essas ações
    /// só aparece dentro dela.
    fn open_new_item_dialog(&mut self, kind: NewItemKind, target: &Path) {
        let Some(source_root) = target
            .ancestors()
            .find(|ancestor| is_java_source_root(ancestor))
            .map(Path::to_path_buf)
        else {
            self.status_message = "Fora de uma raiz de fontes Java".to_owned();
            return;
        };
        let package = target
            .strip_prefix(&source_root)
            .map(|relative| {
                relative
                    .components()
                    .filter_map(|component| component.as_os_str().to_str())
                    .collect::<Vec<_>>()
                    .join(".")
            })
            .unwrap_or_default();
        self.new_item_package.set_value(package);
        self.new_item_name.set_value(String::new());
        self.new_item_modal.set_title(kind.title());
        self.new_item_modal.open();
        self.new_item_dialog = Some(NewItemDialog {
            kind,
            source_root,
            message: None,
            naming: false,
        });
        // O pacote já vem preenchido, então o que falta digitar é o nome —
        // exceto ao criar pacote, em que o nome é justamente o que se edita.
        self.focus_new_item_field(kind != NewItemKind::Package);
        self.new_item_request = None;
    }

    pub const fn new_item_dialog_open(&self) -> bool {
        self.new_item_modal.is_open()
    }

    /// Pedido de criação pronto para o app executar.
    pub fn take_new_item_request(&mut self) -> Option<NewItemRequest> {
        self.new_item_request.take()
    }

    /// Relata o que impediu a criação, mantendo a janela aberta.
    pub fn set_new_item_message(&mut self, message: impl Into<String>) {
        if let Some(dialog) = self.new_item_dialog.as_mut() {
            dialog.message = Some(message.into());
        }
    }

    pub fn close_new_item_dialog(&mut self) {
        self.new_item_modal.close();
        self.new_item_dialog = None;
    }

    /// Monta o pedido a partir do que está nos campos.
    ///
    /// O pacote é obrigatório: sem ele não há onde criar. O nome é obrigatório
    /// para classe e interface, e opcional para pacote — é o que permite criar o
    /// pacote e a primeira classe dele num gesto só.
    fn submit_new_item(&mut self) {
        let Some(dialog) = self.new_item_dialog.as_ref() else {
            return;
        };
        let kind = dialog.kind;
        let source_root = dialog.source_root.clone();
        let package = self.new_item_package.value().trim().to_owned();
        let name = self.new_item_name.value().trim().to_owned();
        if package.is_empty() {
            self.set_new_item_message("Informe o pacote.");
            return;
        }
        if name.is_empty() && kind != NewItemKind::Package {
            self.set_new_item_message("Informe o nome.");
            return;
        }
        self.new_item_request = Some(NewItemRequest {
            kind,
            package,
            name,
            source_root,
        });
    }

    pub fn pointer_down_with_modifiers(&mut self, point: Point, size: Size, control: bool) {
        // O menu aberto tem a primeira palavra: escolher uma ação ou dispensá-lo
        // é o que este clique significa, e não o que está embaixo dele.
        if self.context_menu_event(&UiEvent::PointerDown(primary_pointer(point)), size) {
            return;
        }
        if self.inspection_modal.is_open() {
            self.inspection_pointer_down(point, size);
            return;
        }
        if self.new_item_modal.is_open() {
            self.new_item_pointer_down(point, size);
            return;
        }
        if self.settings_modal.is_open() {
            self.settings_dialog_pointer_down(point, size);
            return;
        }
        if point.y < TITLE_HEIGHT && self.action_buttons_pointer_down(point, size) {
            return;
        }
        self.menu_bar.layout(
            &self.layout_context(),
            Rect::new(82.0, 0.0, (size.width - 82.0).max(0.0), TITLE_HEIGHT),
        );
        let mut menu_context = EventContext::default();
        let menu_result = self.menu_bar.event(
            &mut menu_context,
            &UiEvent::PointerDown(primary_pointer(point)),
        );
        match menu_result {
            EventResult::Action(WidgetAction::Command(command)) if command.0 == "file.project" => {
                self.open_project_requested = true;
                self.status_message = "Select a project folder".to_owned();
                return;
            }
            EventResult::Action(WidgetAction::Command(command)) if command.0 == "file.save" => {
                // Salvar é do shell, que é dono da sessão do editor: não há o que
                // pedir ao app.
                self.save_active_document();
                return;
            }
            EventResult::Action(WidgetAction::Command(command)) if command.0 == "settings.open" => {
                self.open_settings_requested = true;
                return;
            }
            EventResult::Action(WidgetAction::Command(command)) if command.0 == "project.build" => {
                self.build_project_requested = true;
                return;
            }
            EventResult::Action(WidgetAction::Command(command))
                if command.0 == "project.reimport" =>
            {
                self.reimport_project_requested = true;
                return;
            }
            EventResult::Action(WidgetAction::Command(command)) if command.0 == "project.run" => {
                self.run_requested = true;
                self.status_message = "Executando a aplicação".to_owned();
                return;
            }
            EventResult::Action(WidgetAction::Command(command)) if command.0 == "project.stop" => {
                self.stop_requested = true;
                return;
            }
            EventResult::Action(WidgetAction::Command(command)) if command.0 == "debug.connect" => {
                self.settings_page = SettingsPage::Debug;
                self.open_settings_requested = true;
                return;
            }
            EventResult::Action(WidgetAction::Command(command))
                if command.0.starts_with("debug.") =>
            {
                if let Some(request) = debug_request_for(&command.0) {
                    self.debug_requests.push(request);
                }
                return;
            }
            EventResult::Handled | EventResult::Action(_) => return,
            EventResult::Ignored => {}
        }
        if point.y < TITLE_HEIGHT {
            return;
        }
        let sidebar = self.sidebar_width(size);
        let editor_x = ACTIVITY_WIDTH + sidebar;
        let geometry = self.geometry(size);
        let toggle = Rect::new(size.width - 30.0, geometry.editor_bottom + 4.0, 22.0, 22.0);
        if toggle.contains(point) {
            if self.terminal_minimized {
                self.terminal_minimized = false;
                self.terminal_height = self.terminal_last_height;
            } else {
                self.terminal_last_height = self.terminal_height;
                self.terminal_minimized = true;
            }
            return;
        }
        if self.scrollbar_pointer_down(point, size) {
            return;
        }
        if self.splitter_pointer_down(point, size) {
            return;
        }
        if point.y >= TITLE_HEIGHT && point.y < TITLE_HEIGHT + TAB_HEIGHT && point.x >= editor_x {
            // Quem decide entre ativar e fechar é o componente; a IDE traduz o
            // comando recebido para o documento correspondente.
            self.pointer = point;
            let mut tabs = self.editor_tabs();
            tabs.layout(&self.layout_context(), self.editor_tabs_rect(size));
            match tab_command(&mut tabs, point) {
                Some(TabCommand::Select(id)) => {
                    let _ = self.editor.activate(DocumentId(id));
                    self.editor_pane.set_cursor(0);
                    self.focus = ShellFocus::Editor;
                }
                Some(TabCommand::Close(id)) => {
                    let id = DocumentId(id);
                    if self.editor.close(id).is_ok() {
                        self.syntax_snapshots.remove(&id);
                        self.editor_pane.set_cursor(self.active_text().map_or(0, str::len));
                        self.status_message = "Tab closed".to_owned();
                    }
                }
                None => {}
            }
            return;
        }
        if point.x >= ACTIVITY_WIDTH && point.x < editor_x && point.y >= EXPLORER_TOP {
            // Qual nó foi clicado é a árvore quem sabe: o recuo, o deslocamento
            // horizontal e a virtualização são dela.
            let mut tree = self.explorer_tree_for(size);
            tree.event(
                &mut EventContext::default(),
                &UiEvent::PointerDown(primary_pointer(point)),
            );
            let entry = tree.selected().and_then(|id| self.explorer_path_for(id));
            if let Some((path, is_directory)) = entry {
                self.focus = ShellFocus::Explorer;
                self.explorer_tree.set_selected(Some(explorer_id(&path)));
                if is_directory {
                    if !self.expanded.remove(&path) {
                        self.expanded.insert(path);
                    }
                    self.sync_explorer_tree();
                } else if let Err(error) = self.open_file(&path) {
                    self.status_message = error;
                }
            }
            return;
        }
        if self.debug.attached
            && point.x >= editor_x + geometry.editor_width
            && point.y >= geometry.content_top
            && point.y < geometry.editor_bottom
        {
            self.debug_panel_pointer_down(point, size);
            return;
        }
        if point.x >= editor_x
            && point.x < editor_x + geometry.editor_width
            && point.y >= geometry.content_top
            && point.y < geometry.editor_bottom
        {
            self.focus = ShellFocus::Editor;
            // O painel cuida de cursor, âncora e calha; o shell só reage ao que
            // ele pede.
            let bounds = self.editor_view_rect(size);
            self.editor_pane.set_bounds(bounds);
            let Some(document) = self.editor.active() else {
                return;
            };
            let action = self.editor_pane.pointer_down(&document.buffer, point, control);
            self.handle_editor_action(action);
        } else if point.x >= editor_x && point.y >= geometry.editor_bottom {
            self.focus = ShellFocus::Terminal;
            if point.y < geometry.editor_bottom + TERMINAL_TAB_HEIGHT {
                let mut tabs = self.terminal_tabs();
                tabs.layout(&self.layout_context(), self.terminal_tabs_rect(size));
                if let Some(TabCommand::Select(index)) = tab_command(&mut tabs, point) {
                    self.active_terminal = index as usize;
                    self.status_message = format!(
                        "Terminal: {}",
                        self.active_terminal().selected_profile().kind.label()
                    );
                }
            } else if point.y >= geometry.editor_bottom + 60.0 {
                let position = self.terminal_position_at(point, size);
                self.terminal_selection = Some(TerminalSelection {
                    anchor: position,
                    focus: position,
                });
                self.terminal_selecting = true;
            }
        }
    }

    /// Executa o que o painel de edição pediu.
    ///
    /// O painel edita texto; navegar até uma definição, marcar breakpoint,
    /// gravar e abrir menu são coisas que só o shell tem como fazer.
    fn handle_editor_action(&mut self, action: EditorAction) {
        match action {
            EditorAction::Navigate(offset) => {
                if let (Some(document_id), Some(token)) = (
                    self.editor.active_id(),
                    self.active_text().and_then(|text| token_at(text, offset)),
                ) {
                    self.status_message = format!("Go to definition: {token}");
                    self.pending_navigation = Some(NavigationRequest {
                        document_id,
                        byte_offset: offset,
                        token,
                    });
                }
            }
            EditorAction::ToggleBreakpoint(line) => {
                if let Some(path) = self.editor.active().map(|document| document.path.clone()) {
                    self.toggle_breakpoint(&path, line as u32);
                }
            }
            EditorAction::Save => {
                self.save_active_document();
            }
            EditorAction::ContextMenu(_) | EditorAction::None => {}
        }
    }

    pub fn open_location(
        &mut self,
        path: &Path,
        line: usize,
        column: usize,
    ) -> Result<DocumentId, String> {
        let id = self.open_file(path)?;
        let text = self.active_text().unwrap_or_default();
        self.editor_pane.set_cursor(offset_for_line_column(text, line, column));
        // Sem revelar a linha, a navegação move o cursor para fora da área
        // visível e parece que nada aconteceu.
        self.editor_pane.reveal_line(line);
        self.navigated = Some((line, self.editor_pane.cursor()));
        self.focus = ShellFocus::Editor;
        self.status_message = format!("Definition: {}:{}:{}", path.display(), line + 1, column + 1);
        Ok(id)
    }

    pub fn pointer_move(&mut self, point: Point, size: Size) -> bool {
        self.pointer = point;
        // Com o menu aberto, o destaque acompanha o ponteiro dentro dele.
        if self.context_menu.is_open() {
            return self.context_menu_event(&UiEvent::PointerMove(primary_pointer(point)), size);
        }
        if self.settings_modal.is_open() {
            return false;
        }
        if let Some(target) = self.scrollbar_drag {
            self.sync_scrollbar(target, size);
            self.scrollbar_mut(target).event(
                &mut EventContext::default(),
                &UiEvent::PointerMove(primary_pointer(point)),
            );
            self.apply_scrollbar(target);
            return true;
        }
        // O arraste no editor é do painel, que sabe se um gesto começou nele.
        let bounds = self.editor_view_rect(size);
        self.editor_pane.set_bounds(bounds);
        if let Some(document) = self.editor.active()
            && self.editor_pane.pointer_move(&document.buffer, point)
        {
            return true;
        }
        if self.terminal_selecting {
            let position = self.terminal_position_at(point, size);
            if let Some(selection) = self.terminal_selection.as_mut() {
                selection.focus = position;
            }
            return true;
        }
        // O movimento vai sempre aos divisores: mesmo parados, eles precisam
        // saber que o ponteiro passou por cima para se destacar.
        let dragging = self.sidebar_resizing() || self.terminal_resizing();
        self.sync_splitters(size);
        let event = UiEvent::PointerMove(primary_pointer(point));
        self.sidebar_splitter
            .event(&mut EventContext::default(), &event);
        self.terminal_splitter
            .event(&mut EventContext::default(), &event);
        if dragging {
            self.apply_splitters(size);
            return true;
        }
        // Parado, o retorno diz se o ponteiro está sobre o divisor do terminal,
        // para a janela trocar o cursor.
        !self.terminal_minimized && self.terminal_splitter.hit_area().contains(point)
    }

    pub fn pointer_up(&mut self) {
        // Encerrar o gesto é do painel, que sabe se ele virou seleção.
        self.editor_pane.pointer_up();
        let event = UiEvent::PointerUp(primary_pointer(Point::ZERO));
        self.sidebar_splitter
            .event(&mut EventContext::default(), &event);
        self.terminal_splitter
            .event(&mut EventContext::default(), &event);
        // A barra também precisa saber que o gesto acabou: ela é quem guarda o
        // ponto da pegada.
        if let Some(target) = self.scrollbar_drag.take() {
            self.scrollbar_mut(target).event(
                &mut EventContext::default(),
                &UiEvent::PointerUp(primary_pointer(Point::ZERO)),
            );
        }
        self.terminal_selecting = false;
    }

    pub fn scroll(&mut self, point: Point, delta_lines: isize, size: Size) {
        if self.settings_modal.is_open() {
            return;
        }
        let geo = self.geometry(size);
        if point.x >= ACTIVITY_WIDTH
            && point.x < ACTIVITY_WIDTH + self.sidebar_width(size)
            && point.y >= EXPLORER_TOP - EXPLORER_ROW_HEIGHT
            && point.y < geo.content_bottom
        {
            let max = self
                .visible_entries()
                .len()
                .saturating_sub(self.explorer_visible_lines(size));
            self.explorer_scroll_line = self
                .explorer_scroll_line
                .saturating_add_signed(delta_lines)
                .min(max);
        } else if point.y >= geo.content_top && point.y < geo.editor_bottom {
            let total = self.active_text().map_or(0, |text| text.lines().count());
            let visible = (geo.editor_height / EDITOR_LINE_HEIGHT).floor().max(1.0) as usize;
            let max = total.saturating_sub(visible);
            let scrolled = self
                .editor_pane
                .scroll_line()
                .saturating_add_signed(delta_lines)
                .min(max);
            self.editor_pane.set_scroll_line(scrolled);
        } else if point.y >= geo.editor_bottom && point.y < geo.content_bottom {
            let visible = ((geo.terminal_height - 62.0) / EDITOR_LINE_HEIGHT)
                .floor()
                .max(1.0) as usize;
            let active = self.active_terminal;
            let max = self.terminals[active]
                .session
                .line_count()
                .saturating_sub(visible);
            self.terminals[active].scroll_line = self.terminals[active]
                .scroll_line
                .saturating_add_signed(delta_lines)
                .min(max);
            self.terminals[active].follow_output = self.terminals[active].scroll_line >= max;
        }
    }

    fn editor_visible_lines(&self, size: Size) -> usize {
        (self.geometry(size).editor_height / EDITOR_LINE_HEIGHT)
            .floor()
            .max(1.0) as usize
    }

    fn terminal_visible_lines(&self, size: Size) -> usize {
        ((self.geometry(size).terminal_height - 62.0) / EDITOR_LINE_HEIGHT)
            .floor()
            .max(1.0) as usize
    }

    fn explorer_visible_lines(&self, size: Size) -> usize {
        let geo = self.geometry(size);
        ((geo.content_bottom - 12.0 - EXPLORER_TOP) / EXPLORER_ROW_HEIGHT)
            .floor()
            .max(1.0) as usize
    }

    fn editor_scrollbar_rect(&self, size: Size) -> Rect {
        let geo = self.geometry(size);
        let editor_x = ACTIVITY_WIDTH + self.sidebar_width(size);
        Rect::new(
            editor_x + geo.editor_width - 10.0,
            geo.content_top,
            10.0,
            geo.editor_height,
        )
    }

    fn terminal_scrollbar_rect(&self, size: Size) -> Rect {
        let geo = self.geometry(size);
        let editor_x = ACTIVITY_WIDTH + self.sidebar_width(size);
        Rect::new(
            editor_x + geo.editor_width - 10.0,
            geo.editor_bottom + 60.0,
            10.0,
            (geo.terminal_height - 60.0).max(0.0),
        )
    }

    fn explorer_horizontal_scrollbar_rect(&self, size: Size) -> Rect {
        let geo = self.geometry(size);
        Rect::new(
            ACTIVITY_WIDTH,
            geo.content_bottom - 12.0,
            self.sidebar_width(size),
            12.0,
        )
    }

    fn explorer_vertical_scrollbar_rect(&self, size: Size) -> Rect {
        let geo = self.geometry(size);
        Rect::new(
            ACTIVITY_WIDTH + self.sidebar_width(size) - 16.0,
            EXPLORER_TOP - EXPLORER_ROW_HEIGHT,
            10.0,
            (geo.content_bottom - 12.0 - EXPLORER_TOP + EXPLORER_ROW_HEIGHT).max(0.0),
        )
    }

    /// Largura do conteúdo da árvore, medida pela árvore já posicionada.
    ///
    /// A instância persistente nunca passa por `layout` — quem é posicionada é a
    /// cópia usada para desenhar —, e é ela quem conhece a medida.
    fn explorer_content_width(&self, size: Size) -> f32 {
        self.explorer_tree_for(size).content_size().width
    }


    fn terminal_position_at(&self, point: Point, size: Size) -> TextPosition {
        let geo = self.geometry(size);
        let editor_x = ACTIVITY_WIDTH + self.sidebar_width(size);
        let visible = self.terminal_visible_lines(size);
        let active = &self.terminals[self.active_terminal];
        let max = active.session.line_count().saturating_sub(visible);
        let first = active.scroll_line.min(max);
        let row = ((point.y - (geo.editor_bottom + 68.0)) / EDITOR_LINE_HEIGHT)
            .floor()
            .max(0.0) as usize;
        let line = (first + row).min(active.session.line_count().saturating_sub(1));
        let line_length = active
            .session
            .lines()
            .nth(line)
            .map_or(0, |value| value.text.chars().count());
        let column = ((point.x - (editor_x + 14.0)) / TERMINAL_CHAR_WIDTH)
            .round()
            .max(0.0) as usize;
        TextPosition {
            line,
            column: column.min(line_length),
        }
    }

    pub fn selected_terminal_text(&self) -> String {
        let Some(selection) = self.terminal_selection else {
            return String::new();
        };
        let (start, end) = ordered_selection(selection);
        self.active_terminal()
            .lines()
            .enumerate()
            .filter_map(|(line_index, line)| {
                if line_index < start.line || line_index > end.line {
                    return None;
                }
                let from = if line_index == start.line {
                    start.column
                } else {
                    0
                };
                let to = if line_index == end.line {
                    end.column
                } else {
                    line.text.chars().count()
                };
                Some(
                    line.text
                        .chars()
                        .skip(from)
                        .take(to.saturating_sub(from))
                        .collect::<String>(),
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn text_input(&mut self, text: &str) {
        if self.inspection_text_input(text) {
            return;
        }
        if self.new_item_text_input(text) {
            return;
        }
        if self.settings_modal.is_open() {
            let _ = self.settings_text_input(text);
            return;
        }
        match self.focus {
            ShellFocus::Editor => self.edit_active(text),
            ShellFocus::Search => self.search_query.push_str(text),
            ShellFocus::Terminal => self.active_terminal_mut().input_mut().push_str(text),
            _ => {}
        }
    }

    pub fn key_down(&mut self, key: &str) {
        self.key_down_with_modifiers(key, Modifiers::default());
    }

    /// Tecla com os modificadores que o sistema entregou.
    ///
    /// `Tab` e `Shift+Tab` são a mesma tecla lógica com sentidos opostos, então
    /// o estado das teclas modificadoras precisa chegar aqui — o nome da tecla
    /// sozinho não diz se é para indentar ou recolher.
    pub fn key_down_with_modifiers(&mut self, key: &str, modifiers: Modifiers) {
        if self.context_menu_key(key, modifiers) {
            return;
        }
        if self.inspection_key(key, modifiers) {
            return;
        }
        if self.new_item_key(key) {
            return;
        }
        if self.settings_modal.is_open() {
            if self.settings_key_down(key) {
                return;
            }
            let event = UiEvent::KeyDown(KeyEvent {
                logical_key: key.to_owned(),
                repeat: false,
                modifiers,
            });
            let mut context = EventContext::default();
            let result = self.jdk_combo.event(&mut context, &event);
            if !self.handle_settings_action(result) {
                let _ = self.settings_modal.event(&mut context, &event);
                // A janela fechada por dentro do componente — Esc no
                // `ModalHost` — também é cancelamento.
                if !self.settings_modal.is_open() {
                    self.cancel_settings();
                }
            }
            return;
        }
        if !self.completion_items.is_empty() {
            match key.to_ascii_lowercase().as_str() {
                "arrowdown" => {
                    self.completion_selected =
                        (self.completion_selected + 1).min(self.completion_items.len() - 1);
                    return;
                }
                "arrowup" => {
                    self.completion_selected = self.completion_selected.saturating_sub(1);
                    return;
                }
                "enter" | "tab" => {
                    self.accept_completion();
                    return;
                }
                _ => {}
            }
        }
        if key.eq_ignore_ascii_case("backspace") {
            match self.focus {
                ShellFocus::Editor => self.backspace(),
                ShellFocus::Search => {
                    self.search_query.pop();
                }
                ShellFocus::Terminal => {
                    self.active_terminal_mut().input_mut().pop();
                }
                _ => {}
            }
        } else if self.focus == ShellFocus::Terminal && key.eq_ignore_ascii_case("enter") {
            match self.active_terminal_mut().submit() {
                Ok(()) => self.status_message = "Command sent to terminal".to_owned(),
                Err(error) => self.status_message = error.to_string(),
            }
            let active = self.active_terminal;
            self.terminals[active].scroll_line = self.terminals[active]
                .session
                .line_count()
                .saturating_sub(1);
        } else if self.focus == ShellFocus::Editor {
            // Edição, seleção e movimento são do painel. O shell cuida do que
            // sobra: a lista de completação, a marca de modificado e as ações
            // que o painel não tem como executar.
            self.completion_items.clear();
            let Some(document) = self.editor.active_mut() else {
                return;
            };
            let before = document.buffer.revision();
            let action = self.editor_pane.key(
                &mut document.buffer,
                key,
                modifiers.shift,
                modifiers.control,
            );
            if document.buffer.revision() != before {
                self.status_message = "Modified".to_owned();
            }
            self.handle_editor_action(action);
        }
    }

    /// Seleciona a palavra sob o ponteiro. É o que o duplo clique pede.
    pub fn select_word_at_point(&mut self, point: Point, size: Size) {
        let bounds = self.editor_view_rect(size);
        self.editor_pane.set_bounds(bounds);
        if !bounds.contains(point) {
            return;
        }
        self.focus = ShellFocus::Editor;
        let Some(document) = self.editor.active() else {
            return;
        };
        self.editor_pane.select_word_at(&document.buffer, point);
    }

    /// Abre a janela de inspeção com o valor avaliado e seus campos.
    pub fn show_inspection(
        &mut self,
        expression: impl Into<String>,
        value: DebugVariableView,
        fields: Vec<DebugVariableView>,
    ) {
        let expression = expression.into();
        let mut root = InspectionNode::new(expression.clone(), value);
        root.loaded = true;
        root.children = fields
            .into_iter()
            .map(|field| InspectionNode::new(format!("{expression}.{}", field.name), field))
            .collect();
        // A raiz nasce aberta quando tem campos: quem manda inspecionar um objeto
        // quer ver o que há dentro, não um triângulo para clicar.
        let mut expanded = HashSet::new();
        if !root.children.is_empty() {
            expanded.insert(expression.clone());
        }
        self.inspection_modal
            .set_title(format!("Inspecionar — {expression}"));
        self.inspection_modal.open();
        self.inspection = Some(InspectionView {
            selected: expression.clone(),
            expression,
            root,
            expanded,
        });
        self.sync_inspection_tree();
    }

    /// Acrescenta os campos que o alvo revelou para um caminho.
    pub fn add_inspection_fields(&mut self, path: &str, fields: Vec<DebugVariableView>) {
        let Some(inspection) = self.inspection.as_mut() else {
            return;
        };
        let Some(node) = inspection.root.find_mut(path) else {
            return;
        };
        node.loaded = true;
        node.children = fields
            .into_iter()
            .map(|field| InspectionNode::new(format!("{path}.{}", field.name), field))
            .collect();
        // Sem campos não há o que abrir; manter aberto deixaria um triângulo
        // apontando para nada.
        if node.children.is_empty() {
            inspection.expanded.remove(path);
        }
        self.sync_inspection_tree();
    }

    /// Reconstrói os itens da árvore a partir dos nós carregados.
    fn sync_inspection_tree(&mut self) {
        let Some(inspection) = self.inspection.as_ref() else {
            return;
        };
        let roots = vec![inspection_items(&inspection.root)];
        let expanded: Vec<u64> = inspection
            .expanded
            .iter()
            .map(|path| inspection_id(path))
            .collect();
        let selected = inspection_id(&inspection.selected);
        self.inspection_tree.set_roots(roots);
        self.inspection_tree.set_expanded(expanded);
        self.inspection_tree.set_selected(Some(selected));
    }

    /// Relata na janela o que a última execução respondeu.
    ///
    /// Enquanto a janela está aberta ela cobre a barra de estado, então é aqui
    /// que a resposta precisa aparecer.
    pub fn set_inspection_message(&mut self, message: impl Into<String>) {
        if self.inspection_modal.is_open() {
            self.inspection_message = Some(message.into());
        }
    }

    pub const fn inspection_open(&self) -> bool {
        self.inspection_modal.is_open()
    }

    /// Texto do editor de expressões da inspeção.
    #[must_use]
    pub fn inspection_source(&self) -> &str {
        self.inspection_source.text()
    }

    /// Executa o que está escrito no editor, no quadro atual.
    pub fn run_inspection_source(&mut self) {
        if !self.debug.attached {
            self.inspection_message =
                Some("A sessão de depuração terminou; reconecte para executar".to_owned());
            return;
        }
        let code = self.inspection_source.text().trim().to_owned();
        if code.is_empty() {
            self.status_message = "Escreva a expressão a executar".to_owned();
            return;
        }
        self.status_message = format!("Executando {code}");
        self.inspection_message = None;
        self.debug_requests.push(DebugRequest::Evaluate(code));
    }

    /// Digitação dentro da janela de inspeção. Devolve `true` quando consumiu.
    fn inspection_text_input(&mut self, text: &str) -> bool {
        if !self.inspection_modal.is_open() || self.inspection_focus != InspectionFocus::Source {
            return false;
        }
        self.inspection_editor
            .insert(&mut self.inspection_source, text);
        true
    }

    /// Tecla dentro da janela de inspeção. Devolve `true` quando consumiu.
    fn inspection_key(&mut self, key: &str, modifiers: Modifiers) -> bool {
        if !self.inspection_modal.is_open() || self.inspection_focus != InspectionFocus::Source {
            return false;
        }
        // Ctrl+Enter executa: a mão já está no teclado, escrevendo a expressão.
        if modifiers.control && key.eq_ignore_ascii_case("enter") {
            self.run_inspection_source();
            return true;
        }
        self.inspection_editor.key(
            &mut self.inspection_source,
            key,
            modifiers.shift,
            modifiers.control,
        );
        true
    }

    /// Expressão que está sendo inspecionada.
    #[must_use]
    pub fn inspected_expression(&self) -> Option<&str> {
        self.inspection
            .as_ref()
            .map(|inspection| inspection.expression.as_str())
    }

    pub fn close_inspection(&mut self) {
        self.inspection_message = None;
        self.inspection_modal.close();
        self.inspection = None;
    }

    /// Entrada destacada na árvore, que é a detalhada no painel direito.
    fn inspection_selected(&self) -> Option<&DebugVariableView> {
        let inspection = self.inspection.as_ref()?;
        inspection
            .root
            .find(&inspection.selected)
            .map(|node| &node.variable)
    }

    /// Pede a avaliação do trecho marcado no quadro atual da depuração.
    fn inspect_selection(&mut self) {
        let Some(range) = self.editor_pane.selection_range() else {
            return;
        };
        let Some(expression) = self
            .active_text()
            .and_then(|text| text.get(range))
            .map(str::trim)
            .filter(|expression| !expression.is_empty())
            .map(str::to_owned)
        else {
            return;
        };
        self.status_message = format!("Inspecionando {expression}");
        self.debug_requests.push(DebugRequest::Evaluate(expression));
    }

    /// Copia o trecho selecionado para a área de transferência do sistema.
    pub fn copy_selection(&mut self) -> bool {
        let Some(range) = self.editor_pane.selection_range() else {
            self.status_message = "Nada selecionado".to_owned();
            return false;
        };
        let Some(text) = self
            .active_text()
            .and_then(|text| text.get(range).map(str::to_owned))
        else {
            return false;
        };
        let Some(clipboard) = self.clipboard.as_ref() else {
            self.status_message = "Área de transferência indisponível".to_owned();
            return false;
        };
        match clipboard.set_text(&text) {
            Ok(()) => {
                self.status_message = format!("Copiado {} caractere(s)", text.chars().count());
                true
            }
            Err(error) => {
                self.status_message = error.to_string();
                false
            }
        }
    }

    /// Cola o conteúdo da área de transferência no cursor.
    ///
    /// Havendo trecho selecionado, ele é substituído — colar sobre uma seleção é
    /// trocar aquele texto, e é o que qualquer editor faz.
    pub fn paste_clipboard(&mut self) -> bool {
        let Some(clipboard) = self.clipboard.as_ref() else {
            self.status_message = "Área de transferência indisponível".to_owned();
            return false;
        };
        match clipboard.get_text() {
            Ok(Some(text)) if !text.is_empty() => {
                self.edit_active(&text);
                true
            }
            Ok(_) => {
                self.status_message = "Área de transferência vazia".to_owned();
                false
            }
            Err(error) => {
                self.status_message = error.to_string();
                false
            }
        }
    }


    /// Escreve no documento ativo pelo painel de edição.
    fn edit_active(&mut self, text: &str) {
        self.completion_items.clear();
        let Some(document) = self.editor.active_mut() else {
            return;
        };
        if self.editor_pane.insert(&mut document.buffer, text) {
            self.status_message = "Modified".to_owned();
        }
    }



    fn backspace(&mut self) {
        self.completion_items.clear();
        let Some(document) = self.editor.active_mut() else {
            return;
        };
        let before = document.buffer.revision();
        self.editor_pane
            .key(&mut document.buffer, "backspace", false, false);
        if document.buffer.revision() != before {
            self.status_message = "Modified".to_owned();
        }
    }

    fn accept_completion(&mut self) {
        let Some(item) = self.completion_items.get(self.completion_selected).cloned() else {
            return;
        };
        if let Some(document) = self.editor.active_mut() {
            let cursor = self.editor_pane.cursor().min(document.buffer.text().len());
            let prefix = identifier_prefix(document.buffer.text(), cursor);
            let start = cursor.saturating_sub(prefix.len());
            if document.buffer.replace(start..cursor, &item.label).is_ok() {
                self.editor_pane.set_cursor(start + item.label.len());
                self.status_message = format!("Completed {}", item.label);
            }
        }
        self.completion_items.clear();
    }


    fn visible_entries(&self) -> Vec<(usize, &FileNode)> {
        fn visit<'a>(
            node: &'a FileNode,
            depth: usize,
            expanded: &HashSet<PathBuf>,
            output: &mut Vec<(usize, &'a FileNode)>,
        ) {
            if depth > 0 {
                output.push((depth - 1, node));
            }
            if node.is_directory && expanded.contains(&node.path) {
                for child in &node.children {
                    visit(child, depth + 1, expanded, output);
                }
            }
        }
        let mut output = Vec::new();
        visit(&self.workspace, 0, &self.expanded, &mut output);
        output
    }

    /// Desenha o quadro.
    ///
    /// Pintar exige acesso mutável porque o shell mantém widgets com estado
    /// próprio — o editor guarda uma cópia do documento ativo, reconstruída
    /// quando o texto muda. Deixar essa reconciliação para os manipuladores de evento faria
    /// cada esquecimento virar um quadro desatualizado.
    pub fn paint(&mut self, size: Size) -> Vec<PaintCommand> {
        let sidebar = self.sidebar_width(size);
        let editor_x = ACTIVITY_WIDTH + sidebar;
        let geo = self.geometry(size);
        let colors = self.theme.colors;
        let mut commands = vec![
            fill(
                Rect::new(0.0, 0.0, size.width, size.height),
                colors.background,
            ),
            fill(
                Rect::new(0.0, 0.0, size.width, TITLE_HEIGHT),
                colors.elevated,
            ),
            fill(
                Rect::new(
                    0.0,
                    TITLE_HEIGHT,
                    ACTIVITY_WIDTH,
                    geo.content_bottom - TITLE_HEIGHT,
                ),
                colors.elevated,
            ),
            fill(
                Rect::new(
                    ACTIVITY_WIDTH,
                    TITLE_HEIGHT,
                    sidebar,
                    geo.content_bottom - TITLE_HEIGHT,
                ),
                colors.surface,
            ),
            fill(
                Rect::new(editor_x, TITLE_HEIGHT, geo.editor_width, TAB_HEIGHT),
                colors.elevated,
            ),
            fill(
                Rect::new(
                    editor_x,
                    geo.editor_bottom,
                    geo.editor_width,
                    geo.terminal_height,
                ),
                colors.surface,
            ),
            stroke(
                Rect::new(
                    editor_x,
                    geo.editor_bottom,
                    geo.editor_width,
                    geo.terminal_height,
                ),
                colors.border,
            ),
            label("ER IDE", Point::new(14.0, 9.0), colors.text, 16.0),
            label(
                "EXPLORER",
                Point::new(ACTIVITY_WIDTH + 14.0, TITLE_HEIGHT + 14.0),
                colors.muted_text,
                12.0,
            ),
            label(
                &self.workspace_name,
                Point::new(ACTIVITY_WIDTH + 14.0, TITLE_HEIGHT + 42.0),
                colors.text,
                14.0,
            ),
            label(
                "⌕",
                Point::new(15.0, TITLE_HEIGHT + 18.0),
                colors.text,
                22.0,
            ),
            label(
                "▣",
                Point::new(15.0, TITLE_HEIGHT + 62.0),
                colors.text,
                20.0,
            ),
        ];
        commands.push(fill(
            Rect::new(size.width - 30.0, geo.editor_bottom + 4.0, 22.0, 22.0),
            colors.elevated,
        ));
        commands.push(stroke(
            Rect::new(size.width - 30.0, geo.editor_bottom + 4.0, 22.0, 22.0),
            colors.border,
        ));
        commands.push(label(
            if self.terminal_minimized { "^" } else { "v" },
            Point::new(size.width - 24.0, geo.editor_bottom + 7.0),
            colors.text,
            14.0,
        ));
        commands.push(PaintCommand::PushClip(Rect::new(
            ACTIVITY_WIDTH,
            EXPLORER_TOP - EXPLORER_ROW_HEIGHT,
            self.sidebar_width(size),
            (geo.content_bottom - EXPLORER_TOP + EXPLORER_ROW_HEIGHT - 12.0).max(0.0),
        )));
        // A árvore é um componente: recuo, marcador de expansão, virtualização,
        // seleção e deslocamento horizontal pertencem a ela.
        let tree = self.explorer_tree_for(size);
        let mut tree_paint = self.paint_context();
        tree.paint(&mut tree_paint);
        commands.extend(tree_paint.into_commands());
        commands.push(PaintCommand::PopClip);
        commands.extend(self.paint_scrollbar(ScrollTarget::ExplorerHorizontal, size));
        commands.extend(self.paint_scrollbar(ScrollTarget::ExplorerVertical, size));
        // Os divisores se desenham: a linha é a mesma borda de antes, mas agora
        // ela se destaca sob o ponteiro, que é o que revela que dá para arrastar.
        let mut splitters = self.paint_context();
        self.sidebar_splitter_for(size).paint(&mut splitters);
        if !self.terminal_minimized {
            self.terminal_splitter_for(size).paint(&mut splitters);
        }
        commands.extend(splitters.into_commands());
        // As abas são um componente: largura, faixa da aba ativa, corte do
        // título, ponto de alterado e botão de fechar pertencem a ele.
        let mut editor_tabs = self.editor_tabs();
        editor_tabs.layout(&self.layout_context(), self.editor_tabs_rect(size));
        let mut tabs_paint = self.paint_context();
        editor_tabs.paint(&mut tabs_paint);
        commands.extend(tabs_paint.into_commands());
        commands.push(PaintCommand::PushClip(Rect::new(
            editor_x,
            geo.content_top,
            geo.editor_width,
            geo.editor_height,
        )));
        if self.editor.active().is_some() {
            // O editor da biblioteca desenha calha, números, realce, marcas de
            // ponto de parada, linha em execução e cursor. A IDE entrega o
            // texto, o realce e as decorações.
            self.sync_editor_pane(size);
            let mut editor_paint = self.paint_context();
            self.editor_pane.paint(&mut editor_paint);
            commands.extend(editor_paint.into_commands());
            commands.extend(self.paint_scrollbar(ScrollTarget::Editor, size));
        } else {
            commands.push(label(
                "Select a file in Explorer",
                Point::new(editor_x + 55.0, geo.content_top + 30.0),
                colors.muted_text,
                16.0,
            ));
        }
        commands.push(PaintCommand::PopClip);
        if self.debug.attached {
            commands.extend(self.paint_debug_panel(size, colors));
        }
        if !self.terminal_minimized {
            let mut terminal_tabs = self.terminal_tabs();
            terminal_tabs.layout(&self.layout_context(), self.terminal_tabs_rect(size));
            let mut terminal_tabs_paint = self.paint_context();
            terminal_tabs.paint(&mut terminal_tabs_paint);
            commands.extend(terminal_tabs_paint.into_commands());
            commands.push(fill(
                Rect::new(editor_x, geo.editor_bottom + 30.0, geo.editor_width, 30.0),
                colors.background,
            ));
            let active_terminal = &self.terminals[self.active_terminal];
            commands.push(label(
                &format!(
                    "{} {}",
                    active_terminal.session.prompt(),
                    active_terminal.session.input()
                ),
                Point::new(editor_x + 14.0, geo.editor_bottom + 38.0),
                colors.text,
                14.0,
            ));
            let terminal_visible = ((geo.terminal_height - 62.0) / EDITOR_LINE_HEIGHT)
                .floor()
                .max(1.0) as usize;
            let terminal_offset = active_terminal.scroll_line.min(
                active_terminal
                    .session
                    .line_count()
                    .saturating_sub(terminal_visible),
            );
            for (index, line) in active_terminal
                .session
                .lines()
                .skip(terminal_offset)
                .take(terminal_visible)
                .enumerate()
            {
                let absolute_line = terminal_offset + index;
                if let Some((start, end)) =
                    selection_columns(self.terminal_selection, absolute_line, &line.text)
                {
                    commands.push(fill(
                        Rect::new(
                            editor_x + 14.0 + start as f32 * TERMINAL_CHAR_WIDTH,
                            geo.editor_bottom + 66.0 + index as f32 * EDITOR_LINE_HEIGHT,
                            (end.saturating_sub(start) as f32 * TERMINAL_CHAR_WIDTH).max(2.0),
                            EDITOR_LINE_HEIGHT,
                        ),
                        colors.selection,
                    ));
                }
                commands.push(label(
                    &line.text,
                    Point::new(
                        editor_x + 14.0,
                        geo.editor_bottom + 68.0 + index as f32 * EDITOR_LINE_HEIGHT,
                    ),
                    if line.is_error {
                        colors.danger
                    } else {
                        colors.muted_text
                    },
                    14.0,
                ));
            }
            commands.extend(self.paint_scrollbar(ScrollTarget::Terminal, size));
        } else {
            commands.push(label(
                "Terminal",
                Point::new(editor_x + 10.0, geo.editor_bottom + 8.0),
                colors.text,
                13.0,
            ));
        }
        if !self.completion_items.is_empty()
            && self.focus == ShellFocus::Editor
            && let Some(text) = self.active_text()
        {
            let (line, column) = line_column(text, self.editor_pane.cursor());
            let popup_x = (editor_x + EDITOR_GUTTER + column as f32 * EDITOR_CHAR_WIDTH)
                .min(size.width - 270.0)
                .max(editor_x + EDITOR_GUTTER);
            let popup_y = (geo.content_top
                + 36.0
                + line.saturating_sub(self.editor_pane.scroll_line()) as f32 * EDITOR_LINE_HEIGHT)
                .min(geo.editor_bottom - 190.0);
            // Superfície flutuante e lista são da biblioteca: a IDE só diz onde
            // ancorar, o que listar e o que está selecionado.
            let visible = self.completion_items.len().min(COMPLETION_VISIBLE_ROWS);
            let mut surface = Popup::new(COMPLETION_POPUP_ID).with_padding(4.0);
            surface.set_content_size(Size::new(
                COMPLETION_POPUP_WIDTH,
                visible as f32 * COMPLETION_ROW_HEIGHT,
            ));
            surface.layout(
                &self.layout_context(),
                Rect::new(0.0, 0.0, size.width, size.height),
            );
            surface.open_at(Point::new(popup_x, popup_y));
            let mut popup_paint = self.paint_context();
            surface.paint(&mut popup_paint);
            if let Some(content) = surface.content_rect() {
                let mut list = ListView::new(
                    COMPLETION_LIST_ID,
                    self.completion_items
                        .iter()
                        .map(|item| item.label.clone())
                        .collect::<Vec<_>>(),
                )
                .with_row_height(COMPLETION_ROW_HEIGHT);
                list.set_selected(Some(self.completion_selected));
                list.layout(&self.layout_context(), content);
                list.paint(&mut popup_paint);
            }
            commands.extend(popup_paint.into_commands());
        }
        if self.focus == ShellFocus::Search {
            // Também da biblioteca: a caixa é um campo de texto sobre uma
            // superfície flutuante, e a IDE só escolhe o canto e o conteúdo.
            let width = SEARCH_BOX_WIDTH.min((geo.editor_width - 24.0).max(100.0));
            let mut surface = Popup::new(SEARCH_POPUP_ID).with_padding(6.0);
            surface.set_content_size(Size::new(width - 12.0, SEARCH_BOX_HEIGHT - 12.0));
            surface.layout(
                &self.layout_context(),
                Rect::new(0.0, 0.0, size.width, size.height),
            );
            surface.open_at(Point::new(
                size.width - width - 12.0,
                geo.content_top + 12.0,
            ));
            let mut search_paint = self.paint_context();
            surface.paint(&mut search_paint);
            if let Some(content) = surface.content_rect() {
                let mut field = TextInput::new(SEARCH_INPUT_ID, &self.search_query)
                    .with_placeholder("Buscar no arquivo");
                // A busca só aparece quando tem o foco do shell, então o campo
                // é desenhado no estado focado — é ali que o cursor está.
                field.event(&mut EventContext::default(), &UiEvent::FocusGained);
                field.layout(&self.layout_context(), content);
                field.paint(&mut search_paint);
            }
            commands.extend(search_paint.into_commands());
        }
        let position = self
            .active_text()
            .map(|text| line_column(text, self.editor_pane.cursor()))
            .unwrap_or((0, 0));
        // A barra de estado é da biblioteca: superfície, borda, alinhamento e
        // recorte vêm de lá. A IDE só diz o que cada segmento informa — a
        // mensagem da última ação à esquerda, e à direita o que o usuário
        // procura sempre no mesmo lugar.
        let mut status_bar = StatusBar::new(STATUS_BAR_ID).with_leading([&self.status_message]);
        let mut trailing = vec![
            "UTF-8".to_owned(),
            format!("Ln {}, Col {}", position.0 + 1, position.1 + 1),
        ];
        if let Some(summary) = self.project_summary.as_deref() {
            trailing.push(summary.to_owned());
        }
        status_bar.set_trailing(trailing);
        status_bar.layout(
            &self.layout_context(),
            Rect::new(
                0.0,
                geo.content_bottom,
                size.width,
                size.height - geo.content_bottom,
            ),
        );
        let mut status_paint = self.paint_context();
        status_bar.paint(&mut status_paint);
        commands.extend(status_paint.into_commands());
        let mut menu_bar = self.menu_bar.clone();
        menu_bar.layout(
            &self.layout_context(),
            Rect::new(82.0, 0.0, (size.width - 82.0).max(0.0), TITLE_HEIGHT),
        );
        let mut menu_paint = self.paint_context();
        menu_bar.paint(&mut menu_paint);
        commands.extend(menu_paint.into_commands());
        // Os botões de ação são widgets da biblioteca: a IDE define papel e
        // posição, e o desenho do ícone e o tema vêm de lá.
        let rects = action_button_rects(size);
        let mut stop = self.stop_button.clone();
        stop.set_tint(if self.application_running() {
            IconTint::Danger
        } else {
            IconTint::Muted
        });
        stop.set_disabled(!self.application_running());
        let mut debug = self.debug_button.clone();
        debug.set_tint(if self.debug.attached {
            IconTint::Accent
        } else {
            IconTint::Muted
        });
        let mut run = self.run_button.clone();
        let mut actions = self.paint_context();
        for (button, rect) in [
            (&mut stop, rects[0]),
            (&mut run, rects[1]),
            (&mut debug, rects[2]),
        ] {
            button.layout(&self.layout_context(), rect);
            button.paint(&mut actions);
        }
        commands.extend(actions.into_commands());
        if self.settings_modal.is_open() {
            let mut modal = self.settings_modal.clone();
            modal.layout(&self.layout_context(), Rect::new(0.0, 0.0, size.width, size.height));
            let geometry = settings_dialog_geometry(modal.panel_bounds());
            let mut modal_paint = self.paint_context();
            modal.paint(&mut modal_paint);
            commands.extend(modal_paint.into_commands());
            commands.push(fill(geometry.sidebar, colors.surface));
            let mut component_paint = self.paint_context();
            // A navegação entre páginas é a `ListView` da biblioteca, no estilo
            // de marcador: a linha ativa ganha a barra de destaque e continua
            // sendo um rótulo para ler.
            let pages = self.settings_pages_for(&geometry);
            pages.paint(&mut component_paint);
            match self.settings_page {
                SettingsPage::Compiler => {
                    // Título e legenda são `Label` da biblioteca: tamanho e cor
                    // vêm do tema, não de números escritos aqui.
                    self.paint_settings_text(
                        &mut component_paint,
                        SETTINGS_TITLE_ID,
                        "Compilador e VM",
                        Point::new(geometry.combo.origin.x, geometry.combo.origin.y - 34.0),
                        17.0,
                        IconTint::Text,
                    );
                    self.paint_settings_text(
                        &mut component_paint,
                        SETTINGS_CAPTION_ID,
                        "JDK",
                        Point::new(geometry.combo.origin.x, geometry.combo.origin.y - 16.0),
                        13.0,
                        IconTint::Muted,
                    );
                    let mut combo = self.jdk_combo.clone();
                    combo.layout(&self.layout_context(), geometry.combo);
                    combo.paint(&mut component_paint);
                    let mut browse = self.jdk_browse_button.clone();
                    browse.layout(&self.layout_context(), geometry.browse);
                    browse.paint(&mut component_paint);
                }
                SettingsPage::Debug => {
                    commands.extend(self.paint_debug_settings(&geometry, colors));
                    let mut host = self.debug_host.clone();
                    host.layout(&self.layout_context(), geometry.debug_host);
                    host.paint(&mut component_paint);
                    let mut port = self.debug_port.clone();
                    port.layout(&self.layout_context(), geometry.debug_port);
                    port.paint(&mut component_paint);
                    let mut attach = self.debug_attach_button.clone();
                    attach.layout(&self.layout_context(), geometry.debug_attach);
                    attach.paint(&mut component_paint);
                }
            }
            let mut close = self.settings_close_button.clone();
            close.layout(&self.layout_context(), geometry.close);
            close.paint(&mut component_paint);
            let mut save = self.settings_save_button.clone();
            save.layout(&self.layout_context(), geometry.save);
            save.paint(&mut component_paint);
            if let Some(message) = self
                .settings_dialog
                .as_ref()
                .and_then(|dialog| dialog.message.as_ref())
            {
                self.paint_settings_text(
                    &mut component_paint,
                    SETTINGS_MESSAGE_ID,
                    message,
                    Point::new(geometry.combo.origin.x, geometry.combo.origin.y + 54.0),
                    13.0,
                    IconTint::Danger,
                );
            }
            commands.extend(component_paint.into_commands());
        }
        // A janela de criação cobre o conteúdo, e o menu de contexto cobre ela.
        self.paint_new_item_dialog(&mut commands, size);
        self.paint_inspection(&mut commands, size);
        // O menu de contexto é desenhado por último: ele cobre tudo, inclusive
        // o painel de onde foi aberto.
        if self.context_menu.is_open() {
            let mut menu = self.context_menu.clone();
            menu.layout(
                &self.layout_context(),
                Rect::new(0.0, 0.0, size.width, size.height),
            );
            let mut menu_paint = self.paint_context();
            menu.paint(&mut menu_paint);
            commands.extend(menu_paint.into_commands());
        }
        commands
    }

    /// Desenha a janela de inspeção: lista à esquerda, detalhe à direita.
    ///
    /// A lista mostra o valor pedido e os campos dele; o painel direito descreve
    /// a entrada destacada. O valor completo cabe ali, e não numa linha de lista,
    /// que o recorte cortaria justamente onde está a informação.
    fn paint_inspection(&self, commands: &mut Vec<PaintCommand>, size: Size) {
        let Some(inspection) = self.inspection.as_ref() else {
            return;
        };
        let mut modal = self.inspection_modal.clone();
        modal.layout(
            &self.layout_context(),
            Rect::new(0.0, 0.0, size.width, size.height),
        );
        let geometry = inspection_geometry(modal.panel_bounds());
        let mut paint = self.paint_context();
        modal.paint(&mut paint);

        let mut tree = self.inspection_tree.clone();
        tree.set_selected(Some(inspection_id(&inspection.selected)));
        tree.layout(&self.layout_context(), geometry.list);
        tree.paint(&mut paint);

        let detail = geometry.detail;
        match self.inspection_selected() {
            Some(entry) => {
                self.paint_settings_text(
                    &mut paint,
                    INSPECTION_NAME_ID,
                    &entry.name,
                    Point::new(detail.origin.x, detail.origin.y),
                    17.0,
                    IconTint::Text,
                );
                self.paint_settings_text(
                    &mut paint,
                    INSPECTION_TYPE_ID,
                    entry.type_name.as_deref().unwrap_or("tipo desconhecido"),
                    Point::new(detail.origin.x, detail.origin.y + 26.0),
                    13.0,
                    IconTint::Muted,
                );
                self.paint_settings_text(
                    &mut paint,
                    INSPECTION_VALUE_ID,
                    &entry.value,
                    Point::new(detail.origin.x, detail.origin.y + 56.0),
                    14.0,
                    IconTint::Text,
                );
            }
            None => self.paint_settings_text(
                &mut paint,
                INSPECTION_EMPTY_ID,
                "Sem valor para mostrar",
                Point::new(detail.origin.x, detail.origin.y),
                14.0,
                IconTint::Muted,
            ),
        }

        // O editor de expressões é o mesmo painel da janela principal, com os
        // comportamentos de arquivo desligados.
        self.paint_settings_text(
            &mut paint,
            INSPECTION_SOURCE_CAPTION_ID,
            "Código a executar no quadro atual",
            Point::new(geometry.source.origin.x, geometry.source.origin.y - 16.0),
            13.0,
            IconTint::Muted,
        );
        let mut editor = self.inspection_editor.clone();
        editor.set_bounds(geometry.source);
        editor.sync(
            &self.layout_context(),
            &self.inspection_source,
            &[],
            Vec::new(),
            true,
        );
        editor.paint(&mut paint);

        if let Some(message) = self.inspection_message.as_ref() {
            // A mensagem tem a linha inteira, acima dos botões: dividir a linha
            // com eles a fazia passar por baixo do Executar, ilegível justamente
            // quando é ela que explica por que o clique não fez nada.
            self.paint_settings_text(
                &mut paint,
                INSPECTION_MESSAGE_ID,
                &clipped_message(message, geometry.message.size.width),
                geometry.message.origin,
                13.0,
                IconTint::Danger,
            );
        }
        let mut run = self.inspection_run_button.clone();
        // Sem sessão viva não há quadro onde executar: o botão apagado diz isso
        // antes do clique, em vez de a mensagem dizer depois.
        run.set_disabled(!self.debug.attached);
        run.layout(&self.layout_context(), geometry.run);
        run.paint(&mut paint);
        let mut close = self.inspection_close_button.clone();
        close.layout(&self.layout_context(), geometry.close);
        close.paint(&mut paint);
        commands.extend(paint.into_commands());
    }

    /// Roteia o clique dentro da janela de inspeção.
    fn inspection_pointer_down(&mut self, point: Point, size: Size) {
        self.inspection_modal.layout(
            &self.layout_context(),
            Rect::new(0.0, 0.0, size.width, size.height),
        );
        let geometry = inspection_geometry(self.inspection_modal.panel_bounds());
        if geometry.close.contains(point) {
            self.close_inspection();
            return;
        }
        if geometry.run.contains(point) {
            self.run_inspection_source();
            return;
        }
        if geometry.source.contains(point) {
            // O editor cuida do próprio cursor e da própria seleção.
            self.inspection_editor.set_bounds(geometry.source);
            self.inspection_editor
                .pointer_down(&self.inspection_source, point, false);
            self.inspection_focus = InspectionFocus::Source;
            return;
        }
        self.inspection_focus = InspectionFocus::Tree;
        if !geometry.list.contains(point) {
            return;
        }
        // Qual nó foi clicado é a árvore quem sabe: recuo, marcador de expansão e
        // rolagem são dela.
        let mut tree = self.inspection_tree.clone();
        tree.layout(&self.layout_context(), geometry.list);
        tree.event(
            &mut EventContext::default(),
            &UiEvent::PointerDown(primary_pointer(point)),
        );
        let Some(id) = tree.selected() else {
            return;
        };
        let Some(inspection) = self.inspection.as_mut() else {
            return;
        };
        let Some(path) = inspection_path_of(&inspection.root, id) else {
            return;
        };
        inspection.selected = path.clone();
        let expandable = inspection
            .root
            .find(&path)
            .is_some_and(|node| node.variable.expandable);
        if expandable {
            // Clicar em um valor com campos abre e fecha, como no Explorer.
            if !inspection.expanded.remove(&path) {
                inspection.expanded.insert(path.clone());
                let pending = inspection
                    .root
                    .find(&path)
                    .is_some_and(|node| !node.loaded);
                if pending {
                    // Os campos só são pedidos ao abrir: perguntar por tudo de
                    // uma vez percorreria o grafo inteiro do objeto.
                    self.debug_requests
                        .push(DebugRequest::ExpandInspection(path));
                }
            }
        }
        self.sync_inspection_tree();
    }

    /// Desenha a janela de criação por cima de tudo.
    ///
    /// Moldura, véu e título são do `ModalHost`; os campos, os botões e as
    /// legendas são componentes da biblioteca. A IDE diz onde e o que.
    fn paint_new_item_dialog(&self, commands: &mut Vec<PaintCommand>, size: Size) {
        let Some(dialog) = self.new_item_dialog.as_ref() else {
            return;
        };
        let mut modal = self.new_item_modal.clone();
        // O painel se centraliza na área que recebe no layout. Sem esse layout a
        // área é zero, e a janela nasce no canto superior esquerdo.
        modal.layout(
            &self.layout_context(),
            Rect::new(0.0, 0.0, size.width, size.height),
        );
        let panel = modal.panel_bounds();
        let geometry = new_item_geometry(panel);
        let mut paint = self.paint_context();
        // O título é do `ModalHost`, que já o desenha: escrever outro por cima
        // era o que aparecia duplicado.
        modal.paint(&mut paint);
        self.paint_settings_text(
            &mut paint,
            NEW_ITEM_PACKAGE_CAPTION_ID,
            "Pacote",
            Point::new(geometry.package.origin.x, geometry.package.origin.y - 18.0),
            13.0,
            IconTint::Muted,
        );
        self.paint_settings_text(
            &mut paint,
            NEW_ITEM_NAME_CAPTION_ID,
            dialog.kind.name_caption(),
            Point::new(geometry.name.origin.x, geometry.name.origin.y - 18.0),
            13.0,
            IconTint::Muted,
        );
        for (field, rect) in [
            (&self.new_item_package, geometry.package),
            (&self.new_item_name, geometry.name),
        ] {
            // O foco já está no campo de verdade; aqui é só desenhar.
            let mut field = field.clone();
            field.layout(&self.layout_context(), rect);
            field.paint(&mut paint);
        }
        for (button, rect) in [
            (&self.new_item_cancel_button, geometry.cancel),
            (&self.new_item_create_button, geometry.create),
        ] {
            let mut button = button.clone();
            button.layout(&self.layout_context(), rect);
            button.paint(&mut paint);
        }
        if let Some(message) = dialog.message.as_ref() {
            self.paint_settings_text(
                &mut paint,
                NEW_ITEM_MESSAGE_ID,
                message,
                Point::new(geometry.name.origin.x, geometry.name.origin.y + 44.0),
                13.0,
                IconTint::Danger,
            );
        }
        commands.extend(paint.into_commands());
    }

    /// Roteia o clique dentro da janela de criação.
    fn new_item_pointer_down(&mut self, point: Point, size: Size) {
        self.new_item_modal.layout(
            &self.layout_context(),
            Rect::new(0.0, 0.0, size.width, size.height),
        );
        let geometry = new_item_geometry(self.new_item_modal.panel_bounds());
        // O clique vai ao campo: onde o cursor fica dentro do texto é ele quem
        // sabe, porque a medição da fonte é dele.
        for (naming, rect) in [(false, geometry.package), (true, geometry.name)] {
            if !rect.contains(point) {
                continue;
            }
            let context = self.layout_context();
            let field = if naming {
                &mut self.new_item_name
            } else {
                &mut self.new_item_package
            };
            field.layout(&context, rect);
            field.event(
                &mut EventContext::default(),
                &UiEvent::PointerDown(primary_pointer(point)),
            );
            self.focus_new_item_field(naming);
            return;
        }
        if geometry.create.contains(point) {
            self.submit_new_item();
            return;
        }
        if geometry.cancel.contains(point) {
            self.close_new_item_dialog();
        }
    }

    /// Move o foco entre os dois campos.
    ///
    /// O foco fica nos campos de verdade, e não num clone da pintura: é ele que
    /// decide onde a digitação entra e onde o cursor aparece.
    fn focus_new_item_field(&mut self, naming: bool) {
        if let Some(dialog) = self.new_item_dialog.as_mut() {
            dialog.naming = naming;
        }
        let mut context = EventContext::default();
        let (focused, blurred) = if naming {
            (&mut self.new_item_name, &mut self.new_item_package)
        } else {
            (&mut self.new_item_package, &mut self.new_item_name)
        };
        focused.event(&mut context, &UiEvent::FocusGained);
        blurred.event(&mut context, &UiEvent::FocusLost);
    }

    /// O campo que está recebendo o que for digitado.
    fn new_item_field(&mut self) -> Option<&mut TextInput> {
        let naming = self.new_item_dialog.as_ref()?.naming;
        Some(if naming {
            &mut self.new_item_name
        } else {
            &mut self.new_item_package
        })
    }

    /// Tecla dentro da janela de criação. Devolve `true` quando a consumiu.
    fn new_item_key(&mut self, key: &str) -> bool {
        if !self.new_item_modal.is_open() {
            return false;
        }
        let Some(naming) = self.new_item_dialog.as_ref().map(|dialog| dialog.naming) else {
            return false;
        };
        match key.to_ascii_lowercase().as_str() {
            "enter" => self.submit_new_item(),
            "escape" => self.close_new_item_dialog(),
            "tab" => self.focus_new_item_field(!naming),
            // Apagar e mover o cursor são do campo: ele conhece as fronteiras de
            // caractere e a posição atual.
            _ => {
                let event = UiEvent::KeyDown(KeyEvent {
                    logical_key: key.to_owned(),
                    repeat: false,
                    modifiers: Modifiers::default(),
                });
                if let Some(field) = self.new_item_field() {
                    field.event(&mut EventContext::default(), &event);
                }
            }
        }
        true
    }

    /// Texto digitado na janela de criação. Devolve `true` quando o consumiu.
    ///
    /// O texto entra pelo componente, e não por concatenação: é assim que ele
    /// aparece onde o cursor está, inclusive depois de um clique no meio do
    /// caminho já digitado.
    fn new_item_text_input(&mut self, text: &str) -> bool {
        if !self.new_item_modal.is_open() {
            return false;
        }
        let event = UiEvent::TextInput(TextInputEvent {
            text: text.to_owned(),
        });
        if let Some(field) = self.new_item_field() {
            field.event(&mut EventContext::default(), &event);
        }
        if let Some(dialog) = self.new_item_dialog.as_mut() {
            // Digitar é corrigir: a mensagem do erro anterior sai de cena.
            dialog.message = None;
        }
        true
    }

    /// Lista de páginas posicionada, com a página atual selecionada.
    ///
    /// A área ocupada é a das duas linhas, não a barra inteira: a lista responde
    /// pelo que ela desenha, e o resto da barra é fundo do painel.
    fn settings_pages_for(&self, geometry: &SettingsDialogGeometry) -> ListView {
        let mut pages = self.settings_pages.clone();
        pages.set_selected(Some(match self.settings_page {
            SettingsPage::Compiler => 0,
            SettingsPage::Debug => 1,
        }));
        pages.layout(&self.layout_context(), settings_pages_rect(geometry));
        pages
    }

    /// Desenha um texto da janela de configurações com a `Label` da biblioteca.
    ///
    /// A IDE escolhe o papel — título, legenda, mensagem de erro — e a posição.
    /// Cor e desenho vêm do componente, e é assim que o tema alcança este texto.
    fn paint_settings_text(
        &self,
        context: &mut PaintContext,
        id: WidgetId,
        text: &str,
        origin: Point,
        font_size: f32,
        tone: IconTint,
    ) {
        let mut label = Label::new(id, text)
            .with_font_size(font_size)
            .with_tone(tone);
        label.layout(
            &self.layout_context(),
            Rect::new(origin.x, origin.y, 0.0, 0.0),
        );
        label.paint(context);
    }

    fn settings_dialog_pointer_down(&mut self, point: Point, size: Size) {
        if !self.settings_modal.is_open() {
            return;
        }
        self.settings_modal
            .layout(&self.layout_context(), Rect::new(0.0, 0.0, size.width, size.height));
        let geometry = settings_dialog_geometry(self.settings_modal.panel_bounds());
        // Qual página foi clicada é a lista quem sabe: altura de linha e rolagem
        // são dela.
        let mut pages = self.settings_pages_for(&geometry);
        pages.event(
            &mut EventContext::default(),
            &UiEvent::PointerDown(primary_pointer(point)),
        );
        if let Some(page) = pages.selected().and_then(|index| match index {
            0 => Some(SettingsPage::Compiler),
            1 => Some(SettingsPage::Debug),
            _ => None,
        }) && settings_pages_rect(&geometry).contains(point)
        {
            self.settings_page = page;
            self.settings_focus = None;
            return;
        }
        if self.settings_page == SettingsPage::Debug {
            self.debug_page_pointer_down(point, &geometry);
            return;
        }
        self.jdk_combo.layout(&self.layout_context(), geometry.combo);
        self.jdk_browse_button
            .layout(&self.layout_context(), geometry.browse);
        self.settings_close_button
            .layout(&self.layout_context(), geometry.close);
        self.settings_save_button
            .layout(&self.layout_context(), geometry.save);
        let event = UiEvent::PointerDown(primary_pointer(point));
        let mut context = EventContext::default();
        let combo_result = self.jdk_combo.event(&mut context, &event);
        let combo_consumed = !matches!(combo_result, EventResult::Ignored);
        if self.handle_settings_action(combo_result) || combo_consumed {
            return;
        }
        let browse_result = click_widget(&mut self.jdk_browse_button, point);
        if self.handle_settings_action(browse_result) {
            return;
        }
        let close_result = click_widget(&mut self.settings_close_button, point);
        if self.handle_settings_action(close_result) {
            return;
        }
        let save_result = click_widget(&mut self.settings_save_button, point);
        if self.handle_settings_action(save_result) {
            return;
        }
        let _ = self.settings_modal.event(&mut context, &event);
    }

    fn paint_debug_settings(
        &self,
        geometry: &SettingsDialogGeometry,
        colors: ColorTokens,
    ) -> Vec<PaintCommand> {
        let origin = geometry.debug_host.origin;
        let mut commands = vec![
            label(
                "Depuração",
                Point::new(origin.x, origin.y - 34.0),
                colors.text,
                17.0,
            ),
            label(
                "Host e porta de depuração do processo em execução",
                Point::new(origin.x, origin.y - 16.0),
                colors.muted_text,
                13.0,
            ),
            label(
                "Inicie o servidor com -agentlib:jdwp=transport=dt_socket,server=y,suspend=n,address=*:8000",
                Point::new(origin.x, geometry.debug_attach.origin.y + 48.0),
                colors.muted_text,
                12.0,
            ),
            label(
                "Vale para qualquer processo Java: servidor, container ou ferramenta.",
                Point::new(origin.x, geometry.debug_attach.origin.y + 66.0),
                colors.muted_text,
                12.0,
            ),
        ];
        for (rect, id) in [
            (geometry.debug_host, DEBUG_HOST_ID),
            (geometry.debug_port, DEBUG_PORT_ID),
        ] {
            if self.settings_focus == Some(id) {
                commands.push(stroke(rect, colors.accent));
            }
        }
        commands
    }

    fn debug_page_pointer_down(&mut self, point: Point, geometry: &SettingsDialogGeometry) {
        if geometry.debug_host.contains(point) {
            self.settings_focus = Some(DEBUG_HOST_ID);
            return;
        }
        if geometry.debug_port.contains(point) {
            self.settings_focus = Some(DEBUG_PORT_ID);
            return;
        }
        if geometry.debug_attach.contains(point) {
            // O foco é preservado para o usuário corrigir um valor recusado.
            self.attach_debug_target();
            return;
        }
        self.settings_focus = None;
        self.settings_close_button
            .layout(&self.layout_context(), geometry.close);
        let close_result = click_widget(&mut self.settings_close_button, point);
        if self.handle_settings_action(close_result) {
            return;
        }
        self.settings_save_button
            .layout(&self.layout_context(), geometry.save);
        let save_result = click_widget(&mut self.settings_save_button, point);
        let _ = self.handle_settings_action(save_result);
    }

    /// Botões de ação da barra, na ordem em que aparecem.
    #[must_use]
    pub fn action_buttons(&self) -> [&Button; 3] {
        [&self.stop_button, &self.run_button, &self.debug_button]
    }

    /// Roteia o clique para os botões de ação, que são widgets da biblioteca.
    fn action_buttons_pointer_down(&mut self, point: Point, size: Size) -> bool {
        let rects = action_button_rects(size);
        self.stop_button.layout(&self.layout_context(), rects[0]);
        self.run_button.layout(&self.layout_context(), rects[1]);
        self.debug_button.layout(&self.layout_context(), rects[2]);
        let commands = [
            click_widget(&mut self.stop_button, point),
            click_widget(&mut self.run_button, point),
            click_widget(&mut self.debug_button, point),
        ];
        for result in commands {
            if let EventResult::Action(WidgetAction::Command(command)) = result {
                match command.0.as_str() {
                    "project.stop" => self.stop_requested = true,
                    "project.run" => {
                        self.run_requested = true;
                        self.status_message = "Executando a aplicação".to_owned();
                    }
                    "debug.run" => self.request_run_and_attach(),
                    _ => continue,
                }
                return true;
            }
        }
        false
    }

    /// Botão de depurar: sobe a aplicação e conecta, com o alvo configurado.
    fn request_run_and_attach(&mut self) {
        if self.debug.attached {
            self.status_message = "Depuração já conectada".to_owned();
            return;
        }
        match self.debug_target() {
            Some((host, port)) => {
                self.debug_requests
                    .push(DebugRequest::RunAndAttach { host, port });
            }
            None => {
                self.settings_page = SettingsPage::Debug;
                self.open_settings_requested = true;
                self.status_message = "Informe um host e uma porta de depuração válidos".to_owned();
            }
        }
    }

    /// Valida host e porta antes de pedir a conexão à aplicação.
    fn attach_debug_target(&mut self) {
        let host = self.debug_host.value().trim().to_owned();
        let port = self.debug_port.value().trim().parse::<u16>().ok();
        match (host.is_empty(), port) {
            (false, Some(port)) if port > 0 => {
                self.debug_requests.push(DebugRequest::Attach {
                    host: host.clone(),
                    port,
                });
                self.settings_modal.close();
                self.settings_dialog = None;
                self.settings_focus = None;
                self.status_message = format!("Conectando ao alvo de depuração {host}:{port}");
            }
            _ => {
                self.set_settings_message("Informe um host e uma porta de depuração válidos.");
            }
        }
    }

    /// Digitação enquanto a página de depuração está em foco.
    fn settings_text_input(&mut self, text: &str) -> bool {
        let Some(focus) = self.settings_focus else {
            return false;
        };
        let input = if focus == DEBUG_HOST_ID {
            &mut self.debug_host
        } else {
            &mut self.debug_port
        };
        let mut value = input.value().to_owned();
        value.push_str(text);
        input.set_value(value);
        true
    }

    fn settings_key_down(&mut self, key: &str) -> bool {
        let Some(focus) = self.settings_focus else {
            return false;
        };
        match key {
            "Backspace" => {
                let input = if focus == DEBUG_HOST_ID {
                    &mut self.debug_host
                } else {
                    &mut self.debug_port
                };
                let mut value = input.value().to_owned();
                value.pop();
                input.set_value(value);
                true
            }
            "Enter" => {
                self.attach_debug_target();
                true
            }
            _ => false,
        }
    }

    fn handle_settings_action(&mut self, result: EventResult) -> bool {
        let EventResult::Action(WidgetAction::Command(command)) = result else {
            return false;
        };
        if let Some(index) = command
            .0
            .strip_prefix("jdk.select.")
            .and_then(|value| value.parse::<usize>().ok())
        {
            // A escolha fica pendente: quem aplica é o Salvar.
            if let Some(dialog) = self.settings_dialog.as_mut() {
                dialog.pending_jdk = Some(index);
            }
            return true;
        }
        match command.0.as_str() {
            "jdk.browse" => {
                self.browse_jdk_requested = true;
                true
            }
            "settings.save" => {
                self.save_settings();
                true
            }
            "settings.cancel" => {
                self.cancel_settings();
                true
            }
            _ => false,
        }
    }

    /// Aplica o que foi mexido e fecha.
    ///
    /// Só o que mudou sai daqui: sem escolha pendente, salvar não reaplica o JDK
    /// que já estava valendo — reaplicar derrubaria o provider de linguagem e
    /// reindexaria a biblioteca padrão por nada.
    fn save_settings(&mut self) {
        if let Some(dialog) = self.settings_dialog.as_ref()
            && let Some(index) = dialog.pending_jdk
            && dialog.original_jdk != Some(index)
        {
            self.settings_jdk_result = Some(index);
        }
        self.settings_modal.close();
        self.settings_dialog = None;
    }

    /// Descarta tudo o que foi mexido e fecha.
    fn cancel_settings(&mut self) {
        if let Some(dialog) = self.settings_dialog.take() {
            if let Some(original) = dialog.original_jdk {
                self.jdk_combo.set_selected(original);
            }
            self.debug_host.set_value(dialog.original_debug_host);
            self.debug_port.set_value(dialog.original_debug_port);
        }
        self.settings_jdk_result = None;
        self.settings_modal.close();
    }
}

struct SettingsDialogGeometry {
    sidebar: Rect,
    /// Primeira linha da navegação; as demais seguem por altura de linha.
    compiler_option: Rect,
    combo: Rect,
    browse: Rect,
    close: Rect,
    save: Rect,
    debug_host: Rect,
    debug_port: Rect,
    debug_attach: Rect,
}

fn primary_pointer(point: Point) -> PointerEvent {
    PointerEvent {
        position: point,
        button: Some(PointerButton::Primary),
    }
}

fn click_widget(widget: &mut dyn Widget, point: Point) -> EventResult {
    let mut context = EventContext::default();
    let pointer = primary_pointer(point);
    let _ = widget.event(&mut context, &UiEvent::PointerDown(pointer));
    widget.event(&mut context, &UiEvent::PointerUp(pointer))
}

/// Rótulo de uma entrada na lista da inspeção.
fn inspection_label(entry: &DebugVariableView) -> String {
    match entry.type_name.as_deref() {
        Some(type_name) => format!("{} = ({type_name}) {}", entry.name, entry.value),
        None => format!("{} = {}", entry.name, entry.value),
    }
}

/// Caminho do nó com aquela identidade, procurando em profundidade.
fn inspection_path_of(node: &InspectionNode, id: u64) -> Option<String> {
    if inspection_id(&node.path) == id {
        return Some(node.path.clone());
    }
    node.children
        .iter()
        .find_map(|child| inspection_path_of(child, id))
}

/// Identidade de um nó da árvore, derivada do caminho.
///
/// O caminho sobrevive à chegada de campos novos; um índice mudaria a cada
/// expansão, e a árvore perderia o que estava aberto.
fn inspection_id(path: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    path.hash(&mut hasher);
    hasher.finish()
}

/// Converte um nó carregado em item da biblioteca.
///
/// Um valor expansível ainda sem campos recebe um filho de espera: é o que faz a
/// árvore desenhar o triângulo antes de o alvo ter respondido, e é onde o clique
/// de expansão acontece.
fn inspection_items(node: &InspectionNode) -> TreeItem {
    let children = if node.children.is_empty() && node.variable.expandable && !node.loaded {
        vec![TreeItem::new(
            inspection_id(&format!("{}.\u{2026}", node.path)),
            "carregando…",
            Vec::new(),
        )]
    } else {
        node.children.iter().map(inspection_items).collect()
    };
    TreeItem::new(
        inspection_id(&node.path),
        inspection_label(&node.variable),
        children,
    )
}

/// Os dois painéis da janela de inspeção e os botões.
struct InspectionGeometry {
    list: Rect,
    detail: Rect,
    /// Editor de expressões, na parte de baixo do painel direito.
    source: Rect,
    /// Linha da resposta da última execução, acima dos botões.
    message: Rect,
    run: Rect,
    close: Rect,
}

fn inspection_geometry(panel: Rect) -> InspectionGeometry {
    let top = panel.origin.y + 56.0;
    let height = (panel.size.height - 112.0).max(80.0);
    let list_width = (panel.size.width - 32.0) * INSPECTION_LIST_FRACTION;
    let list = Rect::new(panel.origin.x + 16.0, top, list_width, height);
    let right_x = list.origin.x + list.size.width + 16.0;
    let right_width = (panel.size.width - list_width - 48.0).max(80.0);
    // O detalhe fica em cima e o editor embaixo: o valor é o que se lê, o código
    // é o que se escreve.
    let detail_height = (height * INSPECTION_DETAIL_FRACTION).max(60.0);
    let detail = Rect::new(right_x, top, right_width, detail_height);
    let source = Rect::new(
        right_x,
        top + detail_height + 18.0,
        right_width,
        (height - detail_height - 18.0).max(60.0),
    );
    let close = Rect::new(
        panel.origin.x + panel.size.width - 104.0,
        panel.origin.y + panel.size.height - 48.0,
        88.0,
        34.0,
    );
    let run = Rect::new(close.origin.x - 108.0, close.origin.y, 98.0, 34.0);
    let message = Rect::new(
        panel.origin.x + 16.0,
        close.origin.y - 22.0,
        (panel.size.width - 32.0).max(80.0),
        18.0,
    );
    InspectionGeometry {
        list,
        detail,
        source,
        message,
        run,
        close,
    }
}

/// Encurta a mensagem para caber na largura disponível.
///
/// Sem isso ela sai pela borda da janela e o fim — que costuma ser a causa —
/// desaparece.
fn clipped_message(message: &str, width: f32) -> String {
    let limit = (width / INSPECTION_MESSAGE_CHAR_WIDTH).floor().max(8.0) as usize;
    if message.chars().count() <= limit {
        return message.to_owned();
    }
    let head: String = message.chars().take(limit.saturating_sub(1)).collect();
    format!("{head}…")
}

/// Onde cada peça da janela de criação fica dentro do painel.
struct NewItemGeometry {
    package: Rect,
    name: Rect,
    create: Rect,
    cancel: Rect,
}

fn new_item_geometry(panel: Rect) -> NewItemGeometry {
    let field_width = (panel.size.width - 48.0).max(120.0);
    let package = Rect::new(
        panel.origin.x + 24.0,
        panel.origin.y + 76.0,
        field_width,
        34.0,
    );
    let name = Rect::new(
        package.origin.x,
        package.origin.y + 64.0,
        field_width,
        34.0,
    );
    // Criar à direita, encostado na borda, como o Salvar das Configurações.
    let create = Rect::new(
        panel.origin.x + panel.size.width - 104.0,
        panel.origin.y + panel.size.height - 48.0,
        88.0,
        34.0,
    );
    let cancel = Rect::new(create.origin.x - 98.0, create.origin.y, 88.0, 34.0);
    NewItemGeometry {
        package,
        name,
        create,
        cancel,
    }
}

/// Área que a lista de páginas ocupa: as linhas, e não a barra inteira.
fn settings_pages_rect(geometry: &SettingsDialogGeometry) -> Rect {
    Rect::new(
        geometry.compiler_option.origin.x,
        geometry.compiler_option.origin.y,
        geometry.compiler_option.size.width,
        SETTINGS_PAGE_ROW_HEIGHT * SETTINGS_PAGE_TITLES.len() as f32,
    )
}

fn settings_dialog_geometry(dialog: Rect) -> SettingsDialogGeometry {
    let sidebar = Rect::new(
        dialog.origin.x,
        dialog.origin.y + 52.0,
        210.0,
        dialog.size.height - 52.0,
    );
    let compiler_option = Rect::new(sidebar.origin.x, sidebar.origin.y + 12.0, 210.0, 42.0);
    let combo = Rect::new(
        sidebar.origin.x + sidebar.size.width + 28.0,
        dialog.origin.y + 126.0,
        (dialog.size.width - sidebar.size.width - 178.0).max(190.0),
        36.0,
    );
    let browse = Rect::new(
        combo.origin.x + combo.size.width + 10.0,
        combo.origin.y,
        112.0,
        36.0,
    );
    // Salvar à direita, encostado na borda, e Cancelar à esquerda dele: a ação
    // que confirma fica no canto que a leitura alcança por último.
    let save = Rect::new(
        dialog.origin.x + dialog.size.width - 104.0,
        dialog.origin.y + dialog.size.height - 48.0,
        88.0,
        34.0,
    );
    let close = Rect::new(
        save.origin.x - 98.0,
        save.origin.y,
        88.0,
        save.size.height,
    );
    let debug_host = Rect::new(combo.origin.x, combo.origin.y, 220.0, 36.0);
    let debug_port = Rect::new(
        debug_host.origin.x + debug_host.size.width + 12.0,
        debug_host.origin.y,
        96.0,
        36.0,
    );
    let debug_attach = Rect::new(
        debug_host.origin.x,
        debug_host.origin.y + debug_host.size.height + 20.0,
        120.0,
        34.0,
    );
    SettingsDialogGeometry {
        sidebar,
        compiler_option,
        combo,
        browse,
        close,
        save,
        debug_host,
        debug_port,
        debug_attach,
    }
}

struct Geometry {
    content_top: f32,
    content_bottom: f32,
    editor_bottom: f32,
    editor_width: f32,
    editor_height: f32,
    terminal_height: f32,
}

/// Barra de ações no canto direito da barra de menus: parar, executar, depurar.
///
/// A ordem e o desenho dos ícones pertencem à ERLibUi; aqui só existe a posição
/// dos botões na janela.
fn action_button_rects(size: Size) -> [Rect; 3] {
    const SIDE: f32 = 28.0;
    const GAP: f32 = 2.0;
    let top = (TITLE_HEIGHT - SIDE) / 2.0;
    let first = (size.width - 10.0 - SIDE * 3.0 - GAP * 2.0).max(0.0);
    [0.0, 1.0, 2.0].map(|index| Rect::new(first + index * (SIDE + GAP), top, SIDE, SIDE))
}

/// Comandos do menu `Depurar` que viram pedidos diretos à sessão.
fn debug_request_for(command: &str) -> Option<DebugRequest> {
    Some(match command {
        "debug.continue" => DebugRequest::Continue,
        "debug.pause" => DebugRequest::Pause,
        "debug.over" => DebugRequest::StepOver,
        "debug.into" => DebugRequest::StepInto,
        "debug.out" => DebugRequest::StepOut,
        "debug.detach" => DebugRequest::Detach,
        _ => return None,
    })
}

/// Botões do painel, na ordem em que aparecem.
const DEBUG_BUTTONS: [(&str, DebugRequest); 5] = [
    ("Cont.", DebugRequest::Continue),
    ("Sobre", DebugRequest::StepOver),
    ("Entrar", DebugRequest::StepInto),
    ("Sair", DebugRequest::StepOut),
    ("Fim", DebugRequest::Detach),
];

struct DebugPanelGeometry {
    panel: Rect,
    buttons: Vec<Rect>,
    /// Área das listas, já no formato que os widgets da biblioteca recebem.
    frames: Rect,
    variables: Rect,
}

fn debug_panel_geometry(panel: Rect, frame_count: usize) -> DebugPanelGeometry {
    let button_width = (panel.size.width - 20.0) / DEBUG_BUTTONS.len() as f32;
    let buttons = (0..DEBUG_BUTTONS.len())
        .map(|index| {
            Rect::new(
                panel.origin.x + 10.0 + index as f32 * button_width,
                panel.origin.y + 34.0,
                button_width - 4.0,
                26.0,
            )
        })
        .collect();
    let frames_top = panel.origin.y + 86.0;
    let visible_frames = frame_count.clamp(1, 8) as f32;
    let frames_height = visible_frames * DEBUG_ROW_HEIGHT;
    let list_x = panel.origin.x + 6.0;
    let list_width = (panel.size.width - 12.0).max(0.0);
    let variables_top = frames_top + frames_height + 30.0;
    DebugPanelGeometry {
        panel,
        buttons,
        frames: Rect::new(list_x, frames_top, list_width, frames_height),
        variables: Rect::new(
            list_x,
            variables_top,
            list_width,
            (panel.origin.y + panel.size.height - variables_top).max(0.0),
        ),
    }
}

fn geometry(size: Size, requested_terminal_height: f32, sidebar_width: f32) -> Geometry {
    let content_top = TITLE_HEIGHT + TAB_HEIGHT;
    // O rodapé é a barra de estado da biblioteca; a altura é dela.
    let content_bottom = size.height - StatusBar::HEIGHT;
    let terminal_height = requested_terminal_height
        .min((content_bottom - content_top - 100.0).max(TERMINAL_COLLAPSED_HEIGHT));
    let editor_height = (content_bottom - content_top - terminal_height).max(0.0);
    Geometry {
        content_top,
        content_bottom,
        editor_bottom: content_top + editor_height,
        editor_width: (size.width - ACTIVITY_WIDTH - sidebar_width).max(0.0),
        editor_height,
        terminal_height,
    }
}

fn previous_boundary(text: &str, cursor: usize) -> usize {
    text.get(..cursor.min(text.len()))
        .and_then(|prefix| prefix.char_indices().next_back().map(|(index, _)| index))
        .unwrap_or(0)
}
fn next_boundary(text: &str, cursor: usize) -> usize {
    text.get(cursor..)
        .and_then(|suffix| suffix.chars().next())
        .map_or(cursor, |value| cursor + value.len_utf8())
        .min(text.len())
}
fn byte_at_column(text: &str, column: usize) -> usize {
    text.char_indices()
        .nth(column)
        .map_or(text.len(), |(index, _)| index)
}
/// Deslocamento em caracteres de uma posição em linha e coluna.
///
/// O realce chega em linha e coluna, e o editor da biblioteca conta caracteres.
fn char_offset_of(text: &str, position: DomainTextPosition) -> usize {
    let mut offset = 0;
    for (index, line) in text.lines().enumerate() {
        if index == position.line as usize {
            return offset + (position.column as usize).min(line.chars().count());
        }
        offset += line.chars().count() + 1;
    }
    offset
}

fn line_column(text: &str, cursor: usize) -> (usize, usize) {
    let prefix = &text[..cursor.min(text.len())];
    let line = prefix.bytes().filter(|byte| *byte == b'\n').count();
    let column = prefix
        .rsplit('\n')
        .next()
        .unwrap_or_default()
        .chars()
        .count();
    (line, column)
}

fn offset_for_line_column(text: &str, target_line: usize, target_column: usize) -> usize {
    let mut offset = 0;
    for (line, value) in text.split('\n').enumerate() {
        if line == target_line {
            return offset + byte_at_column(value, target_column);
        }
        offset += value.len() + 1;
    }
    text.len()
}

fn token_at(text: &str, offset: usize) -> Option<String> {
    if text.is_empty() {
        return None;
    }
    let offset = offset.min(text.len());
    let mut start = offset;
    while start > 0 {
        let previous = previous_boundary(text, start);
        let character = text[previous..start].chars().next()?;
        if !is_identifier_character(character) {
            break;
        }
        start = previous;
    }
    let mut end = offset;
    while end < text.len() {
        let next = next_boundary(text, end);
        let character = text[end..next].chars().next()?;
        if !is_identifier_character(character) {
            break;
        }
        end = next;
    }
    (start < end).then(|| text[start..end].to_owned())
}

fn identifier_prefix(text: &str, offset: usize) -> String {
    let offset = offset.min(text.len());
    let mut start = offset;
    while start > 0 {
        let previous = previous_boundary(text, start);
        let Some(character) = text[previous..start].chars().next() else {
            break;
        };
        if !is_identifier_character(character) {
            break;
        }
        start = previous;
    }
    text[start..offset].to_owned()
}

fn position_in_range(line: usize, column: usize, range: ide_domain::TextRange) -> bool {
    let start = (range.start.line as usize, range.start.column as usize);
    let end = (range.end.line as usize, range.end.column as usize);
    (line, column) >= start && (line, column) < end
}

fn is_identifier_character(character: char) -> bool {
    character == '_' || character.is_alphanumeric()
}

fn fill(rect: Rect, color: Color) -> PaintCommand {
    PaintCommand::FillRect(FillRectCommand { rect, color })
}
fn stroke(rect: Rect, color: Color) -> PaintCommand {
    PaintCommand::StrokeRect(StrokeRectCommand {
        rect,
        color,
        width: 1.0,
    })
}
fn label(text: &str, origin: Point, color: Color, size: f32) -> PaintCommand {
    PaintCommand::DrawText(DrawTextCommand {
        font_id: FontId(0),
        text: text.to_owned(),
        origin,
        color,
        size,
    })
}

/// Se um token realçado pode levar a uma definição.
///
/// O cursor precisa concordar com o clique. Enquanto só `Type` acendia a mão, o
/// clique navegava em método, campo e variável sem que nada na tela dissesse que
/// era possível — e o usuário concluía, com razão, que ali não funcionava.
///
/// Palavra-chave, literal, comentário e operador ficam de fora: nenhum deles
/// declara nada, e uma mão sobre cada palavra do arquivo não informa coisa
/// alguma.
const fn is_navigable(kind: SyntaxHighlightKind) -> bool {
    matches!(
        kind,
        SyntaxHighlightKind::Type
            | SyntaxHighlightKind::Function
            | SyntaxHighlightKind::Field
            | SyntaxHighlightKind::Variable
            | SyntaxHighlightKind::Annotation
    )
}

/// Papel do realce da IDE no vocabulário do editor da biblioteca.
const fn token_kind_for(kind: SyntaxHighlightKind) -> TokenKind {
    match kind {
        SyntaxHighlightKind::Keyword | SyntaxHighlightKind::Operator => TokenKind::Keyword,
        SyntaxHighlightKind::Type => TokenKind::Type,
        SyntaxHighlightKind::Function => TokenKind::Function,
        SyntaxHighlightKind::String => TokenKind::String,
        SyntaxHighlightKind::Number => TokenKind::Number,
        SyntaxHighlightKind::Comment => TokenKind::Comment,
        // Anotação e nomes comuns não têm token próprio no editor: seguem o
        // texto, que é o que o tema define para código sem classificação.
        SyntaxHighlightKind::Annotation
        | SyntaxHighlightKind::Field
        | SyntaxHighlightKind::Variable => TokenKind::Plain,
    }
}

/// Identidade estável de um nó do Explorer.
///
/// A árvore da biblioteca identifica nós por número e o Explorer os identifica
/// por caminho; o caminho é o que sobrevive a uma releitura do disco, então ele
/// é a origem do número.
fn explorer_id(path: &Path) -> u64 {
    let mut hasher = DefaultHasher::new();
    path.hash(&mut hasher);
    hasher.finish()
}

/// Nome de um nó como aparece na árvore.
fn explorer_label(node: &FileNode) -> &str {
    node.path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("?")
}

/// Raiz de fontes Java pelo layout de Maven e Gradle: `src/<conjunto>/java`.
///
/// O modelo de projeto ainda não chega até o shell, então a convenção de
/// diretório é o que existe para reconhecer uma raiz de fontes. Ela cobre os
/// dois construtores que a IDE já entende, e enquanto o modelo não chegar é
/// preferível acertar o caso comum a não comprimir nada.
fn is_java_source_root(path: &Path) -> bool {
    if path.file_name().and_then(|name| name.to_str()) != Some("java") {
        return false;
    }
    let mut ancestors = path.ancestors().skip(1);
    let parent = ancestors.next().and_then(Path::file_name);
    let grandparent = ancestors.next().and_then(Path::file_name);
    parent.is_some_and(|name| name == "src") || grandparent.is_some_and(|name| name == "src")
}

/// O diretório é um pacote, isto é, está dentro de uma raiz de fontes.
///
/// A raiz em si não é pacote: `java` continua sendo uma linha própria, e a
/// compressão começa no primeiro diretório abaixo dela.
fn is_java_package(path: &Path) -> bool {
    path.ancestors().skip(1).any(is_java_source_root)
}

/// Junta uma cadeia de pacotes de filho único em um nó só.
///
/// `br` que contém apenas `com`, que contém apenas `exemplo`, não carrega
/// informação nenhuma: os três existem porque o Java exige que o diretório
/// espelhe o pacote. Devolve o diretório final da cadeia — é ele que responde
/// pelo nó, então o identificador segue sendo o de um caminho real e o clique
/// resolve para o diretório que de fato tem conteúdo.
///
/// Um diretório com um arquivo ao lado do subdiretório tem mais de um filho e
/// interrompe a cadeia, como no IntelliJ.
fn compact_package_chain(node: &FileNode) -> (&FileNode, String) {
    let mut label = explorer_label(node).to_owned();
    let mut current = node;
    while is_java_package(&current.path) {
        let [only_child] = current.children.as_slice() else {
            break;
        };
        if !only_child.is_directory {
            break;
        }
        label.push('.');
        label.push_str(explorer_label(only_child));
        current = only_child;
    }
    (current, label)
}

/// Ações do menu de contexto do editor.
///
/// Copiar sem seleção não tem o que copiar, então aparece desabilitado em vez de
/// sumir: um item que troca de lugar entre duas aberturas faz o usuário procurar
/// a ação onde ela não está mais.
fn editor_menu_entries(has_selection: bool, debugging: bool) -> Vec<MenuEntry> {
    let copy = MenuItem::new("Copiar", CommandId("editor.copy".to_owned()));
    let mut entries = vec![
        MenuEntry::Item(if has_selection { copy } else { copy.disabled() }),
        MenuEntry::Item(MenuItem::new(
            "Colar",
            CommandId("editor.paste".to_owned()),
        )),
    ];
    // Inspecionar só existe com uma sessão de depuração de pé: fora dela não há
    // quadro que dê valor ao nome, e o item prometeria o que não pode cumprir.
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

/// Ações que fazem sentido no diretório clicado.
///
/// Dentro de uma raiz de fontes Java o que se cria é pacote, classe e
/// interface: ali um diretório *é* um pacote e um arquivo solto *é* um tipo, e
/// oferecer "nova pasta" faria o usuário criar um pacote sem saber que criou.
/// Fora dela não há pacote nem classe, então resta a pasta.
///
/// A própria pasta `java` conta como dentro: ela é a raiz onde o primeiro
/// pacote nasce.
fn explorer_menu_entries(target: &Path) -> Vec<MenuEntry> {
    if is_java_source_root(target) || is_java_package(target) {
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

/// Converte a árvore de arquivos em itens da biblioteca.
fn explorer_items(node: &FileNode) -> Vec<TreeItem> {
    node.children
        .iter()
        .map(|child| {
            let (node, label) = compact_package_chain(child);
            TreeItem::new(explorer_id(&node.path), label, explorer_items(node))
        })
        .collect()
}

/// O que uma barra de abas pediu para um clique.
enum TabCommand {
    Select(u64),
    Close(u64),
}

/// Entrega o clique ao componente de abas e traduz o comando emitido.
///
/// Um clique é pressionar e soltar; a interface da IDE só encaminha o
/// pressionar, então os dois eventos vão juntos.
fn tab_command(tabs: &mut Tabs, point: Point) -> Option<TabCommand> {
    let mut context = EventContext::default();
    let event = UiEvent::PointerDown(primary_pointer(point));
    tabs.event(&mut context, &event);
    let result = tabs.event(&mut context, &UiEvent::PointerUp(primary_pointer(point)));
    let EventResult::Action(WidgetAction::Command(CommandId(command))) = result else {
        return None;
    };
    if let Some(id) = command.strip_prefix("tabs.close.") {
        return id.parse().ok().map(TabCommand::Close);
    }
    command
        .strip_prefix("tabs.select.")
        .and_then(|id| id.parse().ok())
        .map(TabCommand::Select)
}

fn ordered_selection(selection: TerminalSelection) -> (TextPosition, TextPosition) {
    if (selection.anchor.line, selection.anchor.column)
        <= (selection.focus.line, selection.focus.column)
    {
        (selection.anchor, selection.focus)
    } else {
        (selection.focus, selection.anchor)
    }
}

fn selection_columns(
    selection: Option<TerminalSelection>,
    line: usize,
    text: &str,
) -> Option<(usize, usize)> {
    let (start, end) = ordered_selection(selection?);
    if line < start.line || line > end.line {
        return None;
    }
    let length = text.chars().count();
    let from = if line == start.line {
        start.column.min(length)
    } else {
        0
    };
    let to = if line == end.line {
        end.column.min(length)
    } else {
        length
    };
    (to > from).then_some((from, to))
}
fn count_outline(items: &[OutlineItem]) -> usize {
    items
        .iter()
        .map(|item| 1 + count_outline(&item.children))
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_shell() -> IdeShell {
        let root = PathBuf::from("workspace");
        let directory = root.join("src");
        IdeShell::from_tree(FileNode {
            path: root,
            is_directory: true,
            children: vec![FileNode {
                path: directory,
                is_directory: true,
                children: Vec::new(),
            }],
        })
    }

    /// O Explorer desenha pela árvore da biblioteca, e a rolagem horizontal
    /// desloca as linhas em vez de cortá-las.
    #[test]
    fn the_explorer_paints_through_the_tree_and_slides_horizontally() {
        let root = PathBuf::from("workspace");
        let mut shell = IdeShell::from_tree(FileNode {
            path: root.clone(),
            is_directory: true,
            children: vec![FileNode {
                path: root.join("um_arquivo_de_nome_bem_longo_para_exceder_o_painel.rs"),
                is_directory: false,
                children: Vec::new(),
            }],
        });
        let size = Size::new(1280.0, 800.0);
        let origin_of = |shell: &mut IdeShell| {
            shell
                .paint(size)
                .iter()
                .find_map(|command| match command {
                    PaintCommand::DrawText(text) if text.text.contains("um_arquivo") => {
                        Some(text.origin.x)
                    }
                    _ => None,
                })
                .unwrap_or_default()
        };
        let before = origin_of(&mut shell);
        assert!(before > 0.0, "o Explorer precisa desenhar o arquivo");

        shell.explorer_scroll_x = 20.0;
        assert!(
            (before - origin_of(&mut shell) - 20.0).abs() < 0.1,
            "a linha desliza com a rolagem horizontal"
        );
    }

    fn dir(path: &str, children: Vec<FileNode>) -> FileNode {
        FileNode {
            path: PathBuf::from(path),
            is_directory: true,
            children,
        }
    }

    fn file(path: &str) -> FileNode {
        FileNode {
            path: PathBuf::from(path),
            is_directory: false,
            children: Vec::new(),
        }
    }

    fn labels(items: &[TreeItem]) -> Vec<&str> {
        items.iter().map(|item| item.label.as_str()).collect()
    }

    /// Projeto Maven com a cadeia de pacote que a captura mostra.
    fn maven_project() -> FileNode {
        dir(
            "demo",
            vec![dir(
                "demo/src",
                vec![dir(
                    "demo/src/main",
                    vec![dir(
                        "demo/src/main/java",
                        vec![dir(
                            "demo/src/main/java/br",
                            vec![dir(
                                "demo/src/main/java/br/com",
                                vec![dir(
                                    "demo/src/main/java/br/com/exemplo",
                                    vec![dir(
                                        "demo/src/main/java/br/com/exemplo/endpoints",
                                        vec![
                                            dir(
                                                "demo/src/main/java/br/com/exemplo/endpoints/controller",
                                                Vec::new(),
                                            ),
                                            file(
                                                "demo/src/main/java/br/com/exemplo/endpoints/App.java",
                                            ),
                                        ],
                                    )],
                                )],
                            )],
                        )],
                    )],
                )],
            )],
        )
    }

    /// `br`, `com` e `exemplo` só existem porque o diretório espelha o pacote:
    /// viram uma linha só, e `src`, `main` e `java` continuam separados porque
    /// não são pacotes.
    #[test]
    fn explorer_joins_single_child_java_packages_into_one_row() {
        let items = explorer_items(&maven_project());
        let src = &items[0];
        assert_eq!(labels(&items), vec!["src"]);
        let main = &src.children[0];
        assert_eq!(labels(&src.children), vec!["main"]);
        let java = &main.children[0];
        assert_eq!(labels(&main.children), vec!["java"]);
        assert_eq!(labels(&java.children), vec!["br.com.exemplo.endpoints"]);
        assert_eq!(
            labels(&java.children[0].children),
            vec!["controller", "App.java"]
        );
    }

    /// O nó comprimido responde pelo diretório final da cadeia — é assim que o
    /// clique continua resolvendo para um caminho que existe.
    #[test]
    fn a_joined_package_keeps_the_identity_of_the_deepest_directory() {
        let items = explorer_items(&maven_project());
        let package = &items[0].children[0].children[0].children[0];
        assert_eq!(
            package.id,
            explorer_id(Path::new("demo/src/main/java/br/com/exemplo/endpoints"))
        );
    }

    /// Um arquivo ao lado do subdiretório interrompe a cadeia: `br` passa a ter
    /// conteúdo próprio e merece a linha dele.
    #[test]
    fn a_file_beside_the_subdirectory_stops_the_chain() {
        let tree = dir(
            "demo/src/main/java",
            vec![dir(
                "demo/src/main/java/br",
                vec![
                    dir("demo/src/main/java/br/com", Vec::new()),
                    file("demo/src/main/java/br/leiame.md"),
                ],
            )],
        );
        assert_eq!(labels(&explorer_items(&tree)), vec!["br"]);
    }

    /// Fora de uma raiz de fontes não há pacote, e juntar nomes com ponto diria
    /// algo que não é verdade sobre aquelas pastas.
    #[test]
    fn directories_outside_a_source_root_are_left_alone() {
        let tree = dir(
            "demo",
            vec![dir(
                "demo/docs",
                vec![dir("demo/docs/adr", vec![file("demo/docs/adr/0001.md")])],
            )],
        );
        let items = explorer_items(&tree);
        assert_eq!(labels(&items), vec!["docs"]);
        assert_eq!(labels(&items[0].children), vec!["adr"]);
    }

    #[test]
    fn explorer_click_toggles_directory() {
        let mut shell = test_shell();
        let directory = PathBuf::from("workspace").join("src");
        assert!(!shell.is_expanded(&directory));
        shell.pointer_down(
            Point::new(80.0, EXPLORER_TOP + 2.0),
            Size::new(1280.0, 800.0),
        );
        assert!(shell.is_expanded(&directory));
    }

    fn shell_with_java_file() -> (IdeShell, PathBuf) {
        let mut shell = test_shell();
        let path = PathBuf::from("Main.java");
        shell.editor.open_memory(
            "Main.java",
            "class Main {\n  void run() {\n    int total = 1;\n  }\n}",
        );
        (shell, path)
    }

    /// Tab escreve espaços até a próxima parada de tabulação. A partir da
    /// coluna 2 são dois espaços, não quatro — o texto alinha com a grade que o
    /// editor desenha.
    #[test]
    fn tab_indents_the_editor_to_the_next_stop() {
        let mut shell = test_shell();
        shell.editor.open_memory("Example.java", "ab");
        shell.focus = ShellFocus::Editor;
        shell.editor_pane.set_cursor(2);
        shell.key_down("Tab");
        assert_eq!(shell.active_text(), Some("ab  "));
        assert_eq!(shell.editor_pane.cursor(), 4);
    }

    /// Shift+Tab recolhe a margem da linha inteira, com o cursor no meio do
    /// código, e o cursor acompanha o deslocamento.
    #[test]
    fn shift_tab_unindents_the_current_line() {
        let mut shell = test_shell();
        shell
            .editor
            .open_memory("Example.java", "class A {\n    int valor;\n}");
        shell.focus = ShellFocus::Editor;
        shell.editor_pane.set_cursor(14);
        shell.key_down_with_modifiers(
            "Tab",
            Modifiers {
                shift: true,
                ..Modifiers::default()
            },
        );
        assert_eq!(shell.active_text(), Some("class A {\nint valor;\n}"));
        assert_eq!(shell.editor_pane.cursor(), 10);
    }

    fn entry_labels(entries: &[MenuEntry]) -> Vec<String> {
        entries
            .iter()
            .map(|entry| match entry {
                MenuEntry::Item(item) => item.label.clone(),
                MenuEntry::Separator => "—".to_owned(),
            })
            .collect()
    }

    /// Dentro da raiz de fontes o que se cria é pacote e tipo. A própria pasta
    /// `java` conta como dentro: é a raiz onde o primeiro pacote nasce.
    #[test]
    fn inside_the_java_source_root_the_menu_offers_packages_and_types() {
        for target in [
            "demo/src/main/java",
            "demo/src/main/java/br",
            "demo/src/test/java/br/com/exemplo",
        ] {
            assert_eq!(
                entry_labels(&explorer_menu_entries(Path::new(target))),
                vec!["Novo pacote", "—", "Nova classe", "Nova interface"],
                "alvo {target}"
            );
        }
    }

    /// Fora dela não há pacote nem classe: resta a pasta.
    #[test]
    fn outside_the_source_root_the_menu_offers_a_folder() {
        for target in ["demo", "demo/docs", "demo/src/main/resources"] {
            assert_eq!(
                entry_labels(&explorer_menu_entries(Path::new(target))),
                vec!["Nova pasta"],
                "alvo {target}"
            );
        }
    }

    /// O clique secundário abre o menu sobre a linha apontada, e a escolha
    /// relata o diretório onde a ação aconteceria.
    #[test]
    fn the_secondary_click_on_the_explorer_opens_the_menu_for_that_row() {
        let mut shell = IdeShell::from_tree(maven_project());
        let size = Size::new(1280.0, 800.0);
        let row = Point::new(80.0, EXPLORER_TOP + 2.0);
        shell.secondary_pointer_down(row, size);
        assert!(shell.context_menu_open());
        assert_eq!(shell.context_menu_target, Some(PathBuf::from("demo/src")));
        assert_eq!(
            entry_labels(shell.context_menu.entries()),
            vec!["Nova pasta"]
        );
    }

    /// Clicando em um arquivo, o alvo é a pasta dele: criar dentro de um
    /// arquivo não quer dizer nada.
    #[test]
    fn a_file_hands_the_menu_over_to_its_directory() {
        let tree = dir(
            "demo",
            vec![dir(
                "demo/src/main/java",
                vec![file("demo/src/main/java/App.java")],
            )],
        );
        let mut shell = IdeShell::from_tree(tree);
        let size = Size::new(1280.0, 800.0);
        shell.expanded.insert(PathBuf::from("demo/src/main/java"));
        shell.sync_explorer_tree();
        shell.secondary_pointer_down(
            Point::new(80.0, EXPLORER_TOP + EXPLORER_ROW_HEIGHT + 2.0),
            size,
        );
        assert_eq!(
            shell.context_menu_target,
            Some(PathBuf::from("demo/src/main/java"))
        );
        assert_eq!(
            entry_labels(shell.context_menu.entries()),
            vec!["Novo pacote", "—", "Nova classe", "Nova interface"]
        );
    }

    /// Esc dispensa o menu antes de qualquer outra coisa que Esc faria.
    #[test]
    fn escape_dismisses_the_context_menu_first() {
        let mut shell = IdeShell::from_tree(maven_project());
        let size = Size::new(1280.0, 800.0);
        shell.focus = ShellFocus::Search;
        shell.search_query = "consulta".to_owned();
        shell.secondary_pointer_down(Point::new(80.0, EXPLORER_TOP + 2.0), size);
        shell.escape();
        assert!(!shell.context_menu_open());
        assert_eq!(shell.search_query, "consulta");
    }

    /// A calha mostra a diferença entre pedido e confirmado, e a linha parada
    /// é destacada inteira.
    #[test]
    fn the_gutter_shows_pending_and_confirmed_breakpoints_and_the_stopped_line() {
        let mut shell = test_shell();
        shell.editor.open_memory("A.java", "um\ndois\ntres\nquatro");
        let path = PathBuf::from("A.java");
        let size = Size::new(1280.0, 800.0);
        shell.toggle_breakpoint(&path, 1);
        // Só as marcas da calha interessam; a janela desenha outros círculos.
        let gutter = ACTIVITY_WIDTH + SIDEBAR_WIDTH + EDITOR_GUTTER;
        let circles = |shell: &mut IdeShell| {
            shell
                .paint(size)
                .iter()
                .fold((0, 0), |(filled, outlined), command| match command {
                    PaintCommand::FillCircle(circle) if circle.center.x < gutter => {
                        (filled + 1, outlined)
                    }
                    PaintCommand::StrokeCircle(circle) if circle.center.x < gutter => {
                        (filled, outlined + 1)
                    }
                    _ => (filled, outlined),
                })
        };
        assert_eq!(circles(&mut shell), (0, 1), "sem sessão, o ponto é pendente");

        shell.set_verified_breakpoints(&path, &[1]);
        assert_eq!(circles(&mut shell), (1, 0), "confirmado vira disco");

        shell.set_debug_view(DebugView {
            attached: true,
            stopped_at: Some((path, 2)),
            ..DebugView::default()
        });
        let highlight = Theme::default().colors.highlight;
        assert!(
            shell.paint(size).iter().any(|command| matches!(
                command,
                PaintCommand::FillRect(fill) if fill.color == highlight
            )),
            "a linha em execução é destacada"
        );
    }

    #[test]
    fn clicking_the_gutter_toggles_a_breakpoint_and_marks_the_file() {
        let (mut shell, path) = shell_with_java_file();
        let size = Size::new(1280.0, 800.0);
        let geometry = shell.geometry(size);
        let editor_x = ACTIVITY_WIDTH + shell.sidebar_width(size);
        // Terceira linha visível, dentro da calha.
        let point = Point::new(
            editor_x + 20.0,
            geometry.content_top + 15.0 + 2.0 * EDITOR_LINE_HEIGHT + 2.0,
        );

        shell.pointer_down(point, size);
        assert_eq!(shell.breakpoints_for(&path), vec![2]);
        assert_eq!(
            shell.take_breakpoints_dirty().as_deref(),
            Some(path.as_path())
        );
        assert_eq!(shell.breakpoint_count(), 1);
        assert!(
            shell
                .paint(size)
                .iter()
                .any(|command| matches!(command, PaintCommand::StrokeCircle(_))),
            "sem confirmação do alvo, o marcador aparece apenas como contorno"
        );

        shell.set_verified_breakpoints(&path, &[2]);
        assert!(shell.breakpoint_is_verified(&path, 2));
        assert!(
            shell
                .paint(size)
                .iter()
                .any(|command| matches!(command, PaintCommand::FillCircle(_))),
            "confirmado pelo alvo, o marcador fica cheio"
        );

        shell.pointer_down(point, size);
        assert!(shell.breakpoints_for(&path).is_empty());
        assert_eq!(shell.breakpoint_count(), 0);
    }

    #[test]
    fn toggling_from_the_keyboard_uses_the_cursor_line() {
        let (mut shell, path) = shell_with_java_file();
        shell.editor_pane.set_cursor(20); // segunda linha
        shell.toggle_breakpoint_at_cursor();
        assert_eq!(shell.breakpoints_for(&path), vec![1]);
    }

    #[test]
    fn debug_panel_shows_stack_and_variables_and_selects_a_frame() {
        let (mut shell, path) = shell_with_java_file();
        let size = Size::new(1280.0, 800.0);
        shell.set_debug_view(DebugView {
            attached: true,
            status: "Parado em Main.run".to_owned(),
            stopped_at: Some((path.clone(), 2)),
            frames: vec![
                DebugFrameView {
                    name: "Main.run".to_owned(),
                    location: Some((path.clone(), 2)),
                },
                DebugFrameView {
                    name: "Main.main".to_owned(),
                    location: Some((path, 3)),
                },
            ],
            selected_frame: 0,
            variables: vec![DebugVariableView {
                name: "total".to_owned(),
                value: "1".to_owned(),
                type_name: None,
                expandable: false,
            }],
        });

        let texts: Vec<String> = shell
            .paint(size)
            .iter()
            .filter_map(|command| match command {
                PaintCommand::DrawText(text) => Some(text.text.clone()),
                _ => None,
            })
            .collect();
        assert!(texts.iter().any(|text| text == "Parado em Main.run"));
        assert!(texts.iter().any(|text| text.starts_with("Main.run:3")));
        assert!(texts.iter().any(|text| text == "total = 1"));
        assert!(texts.iter().any(|text| text == "Pilha de chamadas"));

        let panel = debug_panel_geometry(shell.debug_panel_rect(size), 2);
        shell.pointer_down(
            Point::new(
                panel.panel.origin.x + 40.0,
                panel.frames.origin.y + DEBUG_ROW_HEIGHT + 2.0,
            ),
            size,
        );
        assert_eq!(
            shell.take_debug_requests(),
            vec![DebugRequest::SelectFrame(1)]
        );
        assert_eq!(shell.debug_view().selected_frame, 1);

        // Clicar abaixo do último quadro não é escolha de quadro nenhum.
        shell.pointer_down(
            Point::new(
                panel.panel.origin.x + 40.0,
                panel.frames.origin.y + panel.frames.size.height + 6.0,
            ),
            size,
        );
        assert_eq!(shell.take_debug_requests(), vec![]);
        assert_eq!(shell.debug_view().selected_frame, 1);
    }

    /// A barra de estado informa em segmentos: a mensagem à esquerda, e o que
    /// se procura sempre no mesmo lugar ancorado à direita.
    #[test]
    fn the_status_bar_reports_message_and_position_in_separate_segments() {
        let mut shell = test_shell();
        shell.set_status_message("Compilação concluída");
        let size = Size::new(1_000.0, 700.0);
        let texts: Vec<String> = shell
            .paint(size)
            .iter()
            .filter_map(|command| match command {
                PaintCommand::DrawText(text) => Some(text.text.clone()),
                _ => None,
            })
            .collect();
        assert!(texts.iter().any(|text| text == "Compilação concluída"));
        assert!(texts.iter().any(|text| text == "UTF-8"));
        assert!(texts.iter().any(|text| text == "Ln 1, Col 1"));
    }

    #[test]
    fn debug_panel_buttons_and_menu_emit_session_requests() {
        let (mut shell, _) = shell_with_java_file();
        let size = Size::new(1280.0, 800.0);
        shell.set_debug_view(DebugView {
            attached: true,
            status: "Parado".to_owned(),
            ..DebugView::default()
        });

        let panel = debug_panel_geometry(shell.debug_panel_rect(size), 0);
        let button = panel.buttons[1];
        shell.pointer_down(
            Point::new(button.origin.x + 4.0, button.origin.y + 4.0),
            size,
        );
        assert_eq!(shell.take_debug_requests(), vec![DebugRequest::StepOver]);

        // Menu `Depurar` → `Continuar`.
        shell.pointer_down(Point::new(280.0, 10.0), size);
        shell.pointer_down(Point::new(280.0, TITLE_HEIGHT + 38.0), size);
        assert_eq!(shell.take_debug_requests(), vec![DebugRequest::Continue]);
    }

    #[test]
    fn the_action_buttons_are_library_widgets_with_accessible_names() {
        let shell = test_shell();
        let mut context = PaintContext::with_theme(*shell.theme());
        let mut accessibility = ui_api::AccessibilityContext::default();
        for (button, rect) in shell
            .action_buttons()
            .into_iter()
            .zip(action_button_rects(Size::new(1_280.0, 800.0)))
        {
            let mut button = button.clone();
            button.layout(&LayoutContext::default(), rect);
            button.paint(&mut context);
            button.accessibility(&mut accessibility);
        }
        let names: Vec<&str> = accessibility
            .nodes()
            .iter()
            .map(|node| node.label.as_str())
            .collect();
        assert_eq!(
            names,
            vec![
                "Parar aplicação",
                "Executar aplicação",
                "Executar com depuração"
            ],
            "um ícone não é legível: quem o expõe é a biblioteca"
        );
    }

    #[test]
    fn the_play_button_requests_a_plain_run() {
        let mut shell = test_shell();
        let size = Size::new(1_280.0, 800.0);
        let [_, run, debug] = action_button_rects(size);
        assert!(
            run.origin.x + run.size.width <= debug.origin.x,
            "o play fica à esquerda do inseto, sem sobrepor"
        );
        let colors = Theme::default().colors;
        assert!(
            shell
                .paint(size)
                .iter()
                .filter(|command| matches!(command, PaintCommand::FillRect(rect)
                    if rect.color == colors.success && run.contains(rect.rect.origin)))
                .count()
                >= 5,
            "o triângulo de play é desenhado com a cor de ação da paleta"
        );

        shell.pointer_down(Point::new(run.origin.x + 6.0, run.origin.y + 6.0), size);
        assert!(shell.take_run_request());
        assert!(!shell.take_run_request(), "o pedido é consumido uma vez");
        assert!(
            shell.take_debug_requests().is_empty(),
            "executar sem depuração não abre sessão"
        );
    }

    #[test]
    fn the_stop_button_sits_left_of_play_and_only_acts_after_a_run() {
        let mut shell = test_shell();
        let size = Size::new(1_280.0, 800.0);
        let [stop, run, _] = action_button_rects(size);
        assert!(
            stop.origin.x + stop.size.width <= run.origin.x,
            "a ordem é parar, executar, depurar"
        );

        let colors = Theme::default().colors;
        let icon_color = |shell: &mut IdeShell| {
            shell.paint(size).iter().find_map(|command| match command {
                PaintCommand::FillRect(rect)
                    if stop.contains(rect.rect.origin) && rect.rect.size.width < 20.0 =>
                {
                    Some(rect.color)
                }
                _ => None,
            })
        };
        assert_eq!(
            icon_color(&mut shell),
            Some(colors.muted_text),
            "sem aplicação iniciada, o ícone fica apagado"
        );
        assert!(!shell.application_running());

        shell.pointer_down(Point::new(stop.origin.x + 6.0, stop.origin.y + 6.0), size);
        assert!(shell.take_stop_request());
        assert!(
            shell.stop_application().is_err(),
            "sem aplicação iniciada não há o que interromper"
        );
    }

    #[test]
    fn the_project_menu_also_runs_the_application() {
        let mut shell = test_shell();
        let size = Size::new(1_000.0, 700.0);
        shell.pointer_down(Point::new(200.0, 10.0), size);
        shell.pointer_down(Point::new(200.0, TITLE_HEIGHT + 66.0), size);
        assert!(shell.take_run_request());
        assert!(!shell.take_build_project_request());
        assert!(!shell.take_reimport_project_request());
    }

    #[test]
    fn the_bug_button_runs_and_attaches_with_the_configured_target() {
        let mut shell = test_shell();
        let size = Size::new(1_280.0, 800.0);
        shell.set_debug_target("10.0.0.20", 8787);

        let button = action_button_rects(size)[2];
        assert!(
            button.origin.x + button.size.width < size.width && button.origin.x > size.width - 60.0,
            "o botão fica no canto direito da barra de menus"
        );
        assert!(
            shell
                .paint(size)
                .iter()
                .filter(|command| matches!(command, PaintCommand::FillCircle(circle)
                    if button.contains(circle.center)))
                .count()
                >= 2,
            "o ícone desenha corpo e cabeça do inseto dentro do botão"
        );

        shell.pointer_down(
            Point::new(button.origin.x + 6.0, button.origin.y + 6.0),
            size,
        );
        assert_eq!(
            shell.take_debug_requests(),
            vec![DebugRequest::RunAndAttach {
                host: "10.0.0.20".to_owned(),
                port: 8787,
            }]
        );
    }

    #[test]
    fn the_bug_button_asks_for_a_target_when_it_is_invalid() {
        let mut shell = test_shell();
        let size = Size::new(1_280.0, 800.0);
        shell.set_debug_target("", 0);

        let button = action_button_rects(size)[2];
        shell.pointer_down(
            Point::new(button.origin.x + 6.0, button.origin.y + 6.0),
            size,
        );

        assert!(shell.take_debug_requests().is_empty());
        assert!(
            shell.take_open_settings_request(),
            "sem alvo válido, o botão abre a página de depuração"
        );
        assert_eq!(shell.settings_page(), SettingsPage::Debug);
    }

    #[test]
    fn the_debug_menu_opens_the_settings_window_on_the_debug_page() {
        let mut shell = test_shell();
        let size = Size::new(1_000.0, 700.0);
        assert_eq!(shell.settings_page(), SettingsPage::Compiler);

        // Menu `Depurar` → `Conectar...`.
        shell.pointer_down(Point::new(280.0, 10.0), size);
        shell.pointer_down(Point::new(280.0, TITLE_HEIGHT + 10.0), size);
        assert!(shell.take_open_settings_request());
        assert_eq!(shell.settings_page(), SettingsPage::Debug);

        shell.open_settings_dialog(vec!["JDK 17".to_owned()], 0);
        let texts: Vec<String> = shell
            .paint(size)
            .iter()
            .filter_map(|command| match command {
                PaintCommand::DrawText(text) => Some(text.text.clone()),
                _ => None,
            })
            .collect();
        assert!(texts.iter().any(|text| text.contains("Host e porta")));
        assert!(!texts.iter().any(|text| text == "JDK"));

        // O atalho do compilador troca a página de volta.
        shell.set_settings_page(SettingsPage::Compiler);
        let texts: Vec<String> = shell
            .paint(size)
            .iter()
            .filter_map(|command| match command {
                PaintCommand::DrawText(text) => Some(text.text.clone()),
                _ => None,
            })
            .collect();
        assert!(texts.iter().any(|text| text == "JDK"));
    }

    #[test]
    fn debug_settings_page_validates_the_target_before_connecting() {
        let mut shell = test_shell();
        let size = Size::new(1_000.0, 700.0);
        shell.open_settings_dialog(vec!["JDK 8".to_owned()], 0);
        shell
            .settings_modal
            .layout(&LayoutContext::default(), Rect::new(0.0, 0.0, size.width, size.height));
        let geometry = settings_dialog_geometry(shell.settings_modal.panel_bounds());

        // Segunda linha da navegação é a página de Depuração.
        shell.pointer_down(
            Point::new(
                geometry.compiler_option.origin.x + 20.0,
                geometry.compiler_option.origin.y + SETTINGS_PAGE_ROW_HEIGHT + 10.0,
            ),
            size,
        );
        let texts: Vec<String> = shell
            .paint(size)
            .iter()
            .filter_map(|command| match command {
                PaintCommand::DrawText(text) => Some(text.text.clone()),
                _ => None,
            })
            .collect();
        assert!(texts.iter().any(|text| text == "Depuração"));
        assert!(texts.iter().any(|text| text.contains("agentlib:jdwp")));

        shell.pointer_down(
            Point::new(
                geometry.debug_port.origin.x + 10.0,
                geometry.debug_port.origin.y + 10.0,
            ),
            size,
        );
        shell.key_down("Backspace");
        shell.key_down("Backspace");
        shell.key_down("Backspace");
        shell.key_down("Backspace");
        shell.text_input("porta");
        shell.pointer_down(
            Point::new(
                geometry.debug_attach.origin.x + 10.0,
                geometry.debug_attach.origin.y + 10.0,
            ),
            size,
        );
        assert!(
            shell.take_debug_requests().is_empty(),
            "porta inválida não conecta"
        );
        assert!(shell.settings_dialog_open());

        shell.key_down("Backspace");
        shell.key_down("Backspace");
        shell.key_down("Backspace");
        shell.key_down("Backspace");
        shell.key_down("Backspace");
        shell.text_input("5005");
        shell.key_down("Enter");
        assert_eq!(
            shell.take_debug_requests(),
            vec![DebugRequest::Attach {
                host: "127.0.0.1".to_owned(),
                port: 5005,
            }]
        );
        assert!(!shell.settings_dialog_open());
    }

    #[test]
    fn java_syntax_snapshot_drives_highlighting_and_outline() {
        let mut shell = test_shell();
        let document_id = shell
            .editor
            .open_memory("Example.java", "public class Example {}");
        shell.set_syntax_snapshot(ide_domain::SyntaxSnapshot {
            document_id,
            version: 0,
            tree: ide_domain::SyntaxNode {
                kind: "program".to_owned(),
                range: ide_domain::TextRange::default(),
                has_error: false,
                children: Vec::new(),
            },
            outline: vec![ide_domain::OutlineItem {
                name: "Example".to_owned(),
                kind: ide_domain::OutlineKind::Class,
                range: ide_domain::TextRange::default(),
                children: Vec::new(),
            }],
            highlights: vec![ide_domain::SyntaxHighlight {
                range: ide_domain::TextRange {
                    start: ide_domain::TextPosition { line: 0, column: 0 },
                    end: ide_domain::TextPosition { line: 0, column: 6 },
                },
                kind: SyntaxHighlightKind::Keyword,
            }],
            imports: Vec::new(),
            diagnostics: Vec::new(),
        });

        assert_eq!(shell.active_outline()[0].name, "Example");
        let colors = Theme::default().colors;
        assert!(shell.paint(Size::new(1280.0, 800.0)).iter().any(|command| {
            matches!(
                command,
                PaintCommand::DrawText(text)
                    if text.text == "public" && text.color == colors.syntax_keyword
            )
        }));
    }

    #[test]
    fn completion_popup_can_apply_selected_item() {
        let mut shell = test_shell();
        shell.editor.open_memory("Example.java", "Exa");
        shell.focus = ShellFocus::Editor;
        shell.editor_pane.set_cursor(3);
        shell.set_completions(vec![CompletionItem {
            label: "Example".to_owned(),
            detail: Some("class".to_owned()),
            kind: ide_domain::CompletionKind::Class,
        }]);
        assert!(shell.paint(Size::new(1280.0, 800.0)).iter().any(|command| {
            matches!(command, PaintCommand::DrawText(text) if text.text == "Example")
        }));
        shell.key_down("Enter");
        assert_eq!(shell.active_text(), Some("Example"));
    }

    /// A mão acende no que dá para navegar, não só em tipo.
    ///
    /// Enquanto só `Type` acendia, o clique navegava em método, campo e variável
    /// sem que nada na tela dissesse que era possível.
    #[test]
    fn the_navigation_cursor_agrees_with_what_the_click_resolves() {
        let mut shell = test_shell();
        //                                    0         1         2         3
        //                                    0123456789012345678901234567890
        let document_id = shell.editor.open_memory("A.java", "void metodo() { int x = y; }");
        let realce = |coluna_inicial: u32, coluna_final: u32, kind| ide_domain::SyntaxHighlight {
            range: ide_domain::TextRange {
                start: ide_domain::TextPosition {
                    line: 0,
                    column: coluna_inicial,
                },
                end: ide_domain::TextPosition {
                    line: 0,
                    column: coluna_final,
                },
            },
            kind,
        };
        shell.set_syntax_snapshot(ide_domain::SyntaxSnapshot {
            document_id,
            version: 0,
            tree: ide_domain::SyntaxNode {
                kind: "program".to_owned(),
                range: ide_domain::TextRange::default(),
                has_error: false,
                children: Vec::new(),
            },
            outline: Vec::new(),
            highlights: vec![
                realce(0, 4, SyntaxHighlightKind::Keyword),
                realce(5, 11, SyntaxHighlightKind::Function),
                realce(20, 21, SyntaxHighlightKind::Variable),
                realce(24, 25, SyntaxHighlightKind::Field),
            ],
            imports: Vec::new(),
            diagnostics: Vec::new(),
        });

        let size = Size::new(1280.0, 800.0);
        let editor_x = ACTIVITY_WIDTH + shell.sidebar_width(size);
        let sobre = |coluna: f32| {
            Point::new(
                editor_x + EDITOR_GUTTER + coluna * EDITOR_CHAR_WIDTH,
                shell.geometry(size).content_top + 15.0,
            )
        };
        assert!(shell.navigation_hover(sobre(7.0), size, true), "método");
        assert!(shell.navigation_hover(sobre(20.0), size, true), "variável");
        assert!(shell.navigation_hover(sobre(24.0), size, true), "campo");
        assert!(
            !shell.navigation_hover(sobre(1.0), size, true),
            "palavra-chave não leva a lugar nenhum"
        );
    }

    #[test]
    fn control_hover_over_java_type_uses_navigation_cursor_state() {
        let mut shell = test_shell();
        let document_id = shell.editor.open_memory("Example.java", "class Example {}");
        shell.set_syntax_snapshot(ide_domain::SyntaxSnapshot {
            document_id,
            version: 0,
            tree: ide_domain::SyntaxNode {
                kind: "program".to_owned(),
                range: ide_domain::TextRange::default(),
                has_error: false,
                children: Vec::new(),
            },
            outline: Vec::new(),
            highlights: vec![ide_domain::SyntaxHighlight {
                range: ide_domain::TextRange {
                    start: ide_domain::TextPosition { line: 0, column: 6 },
                    end: ide_domain::TextPosition {
                        line: 0,
                        column: 13,
                    },
                },
                kind: SyntaxHighlightKind::Type,
            }],
            imports: Vec::new(),
            diagnostics: Vec::new(),
        });
        let size = Size::new(1280.0, 800.0);
        let editor_x = ACTIVITY_WIDTH + shell.sidebar_width(size);
        let point = Point::new(
            editor_x + EDITOR_GUTTER + 8.0 * EDITOR_CHAR_WIDTH,
            shell.geometry(size).content_top + 15.0,
        );
        assert!(!shell.navigation_hover(point, size, false));
        assert!(shell.navigation_hover(point, size, true));
    }

    #[test]
    fn java_tool_output_is_appended_to_terminal() {
        let mut shell = test_shell();
        shell.append_tool_output("compile ok\nruntime failure", true);
        let lines = shell.active_terminal_lines().collect::<Vec<_>>();
        assert!(lines.contains(&"compile ok"));
        assert!(lines.contains(&"runtime failure"));
    }

    #[test]
    fn explorer_horizontal_scrollbar_keeps_long_names_inside_sidebar() {
        let mut shell = IdeShell::from_tree(FileNode {
            path: PathBuf::from("workspace"),
            is_directory: true,
            children: vec![FileNode {
                path: PathBuf::from("workspace")
                    .join("a_very_long_project_filename_that_must_not_overflow_into_the_editor.rs"),
                is_directory: false,
                children: Vec::new(),
            }],
        });
        let size = Size::new(1280.0, 800.0);
        let track = shell.explorer_horizontal_scrollbar_rect(size);
        // O nome não cabe na largura visível, então há o que rolar.
        let (_, content, viewport, _) = shell.scrollbar_range(ScrollTarget::ExplorerHorizontal, size);
        assert!(content > viewport);
        shell.pointer_down(
            Point::new(
                track.origin.x + track.size.width - 1.0,
                track.origin.y + 5.0,
            ),
            size,
        );
        assert!(shell.explorer_scroll_x > 0.0);
        let rendered = shell.paint(size);
        assert!(rendered.iter().any(|command| {
            matches!(
                command,
                PaintCommand::PushClip(rect)
                    if rect.origin.x == ACTIVITY_WIDTH
                        && rect.size.width == shell.sidebar_width(size)
            )
        }));
    }

    #[test]
    fn file_project_menu_requests_a_folder_picker() {
        let mut shell = test_shell();
        let size = Size::new(1280.0, 800.0);
        shell.pointer_down(Point::new(100.0, 15.0), size);
        let menu_is_visible = shell.paint(size).into_iter().any(|command| {
            matches!(
                command,
                PaintCommand::DrawText(command) if command.text == "Projeto..."
            )
        });
        assert!(menu_is_visible);
        shell.pointer_down(Point::new(110.0, TITLE_HEIGHT + 15.0), size);
        assert!(shell.take_open_project_request());
        assert!(!shell.take_open_project_request());
    }

    #[test]
    fn tab_click_changes_active_document_and_typing_edits_it() {
        let mut shell = test_shell();
        let first = shell.editor.open_memory("first.rs", "one");
        let second = shell.editor.open_memory("second.rs", "two");
        assert_eq!(shell.active_document(), Some(second));
        let editor_x = ACTIVITY_WIDTH + SIDEBAR_WIDTH;
        shell.pointer_down(
            Point::new(editor_x + 10.0, TITLE_HEIGHT + 10.0),
            Size::new(1280.0, 800.0),
        );
        assert_eq!(shell.active_document(), Some(first));
        shell.pointer_down(
            Point::new(editor_x + EDITOR_GUTTER, TITLE_HEIGHT + TAB_HEIGHT + 15.0),
            Size::new(1280.0, 800.0),
        );
        shell.text_input("X");
        assert_eq!(shell.active_text(), Some("Xone"));
    }

    #[test]
    fn tab_close_button_removes_only_the_clicked_document() {
        let mut shell = test_shell();
        let first = shell.editor.open_memory("first.rs", "one");
        shell.editor.open_memory("second.rs", "two");
        let size = Size::new(1280.0, 800.0);
        let editor_x = ACTIVITY_WIDTH + SIDEBAR_WIDTH;
        shell.pointer_down(
            Point::new(
                editor_x + TAB_WIDTH * 2.0 - 15.0,
                TITLE_HEIGHT + TAB_HEIGHT / 2.0,
            ),
            size,
        );
        assert_eq!(shell.tab_count(), 1);
        assert_eq!(shell.active_document(), Some(first));
    }

    /// Documento alterado e não gravado é sinalizado na aba.
    #[test]
    fn an_unsaved_document_is_marked_on_its_tab() {
        let mut shell = test_shell();
        shell.editor.open_memory("first.rs", "one");
        let size = Size::new(1280.0, 800.0);
        let marks = |shell: &mut IdeShell| {
            shell
                .paint(size)
                .iter()
                .filter_map(|command| match command {
                    PaintCommand::DrawText(text) => Some(text.text.clone()),
                    _ => None,
                })
                .any(|text| text == "●")
        };
        assert!(!marks(&mut shell), "documento intacto não é marcado");

        shell.focus = ShellFocus::Editor;
        shell.edit_active("x");
        assert!(marks(&mut shell), "documento alterado é marcado");
    }

    #[test]
    fn long_tab_titles_are_clipped_and_ellipsized_before_close_button() {
        let mut shell = test_shell();
        shell
            .editor
            .open_memory("ExplosionEffectManager.ts", "content");
        let rendered = shell.paint(Size::new(1280.0, 800.0));
        let texts = rendered
            .iter()
            .filter_map(|command| match command {
                PaintCommand::DrawText(command) => Some(command.text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>();
        // O nome inteiro não cabe: aparece encurtado, com a marca do corte, e o
        // desenho fica contido na faixa das abas.
        assert!(!texts.contains(&"ExplosionEffectManager.ts"));
        assert!(
            texts
                .iter()
                .any(|text| text.starts_with("Explosion") && text.ends_with('…')),
            "esperava um título encurtado, veio {texts:?}"
        );
        let tabs = shell.editor_tabs_rect(Size::new(1280.0, 800.0));
        assert!(rendered.iter().any(|command| {
            matches!(command, PaintCommand::PushClip(rect) if *rect == tabs)
        }));
    }

    /// O divisor é desenhado no lugar certo desde o primeiro quadro, antes de
    /// qualquer evento de ponteiro, e se destaca quando o ponteiro se aproxima.
    #[test]
    fn the_sidebar_divider_is_painted_in_place_and_highlights_under_the_pointer() {
        let mut shell = test_shell();
        let size = Size::new(1280.0, 800.0);
        let divider_color = |shell: &mut IdeShell| {
            let x = ACTIVITY_WIDTH + shell.sidebar_width(size);
            shell
                .paint(size)
                .iter()
                .find_map(|command| match command {
                    PaintCommand::FillRect(fill)
                        if (fill.rect.origin.x - x).abs() < 0.01
                            && fill.rect.size.width == Splitter::THICKNESS =>
                    {
                        Some(fill.color)
                    }
                    _ => None,
                })
        };
        assert_eq!(divider_color(&mut shell), Some(shell.theme().colors.border));

        shell.pointer_move(Point::new(ACTIVITY_WIDTH + SIDEBAR_WIDTH, 300.0), size);
        assert_eq!(divider_color(&mut shell), Some(shell.theme().colors.accent));
    }

    #[test]
    fn sidebar_border_resizes_explorer_editor_and_terminal_widths_together() {
        let mut shell = test_shell();
        let size = Size::new(1280.0, 800.0);
        let before = shell.geometry(size).editor_width;
        let border = ACTIVITY_WIDTH + SIDEBAR_WIDTH;
        shell.pointer_down(Point::new(border, 300.0), size);
        assert!(shell.sidebar_resizing());
        shell.pointer_move(Point::new(border + 80.0, 300.0), size);
        shell.pointer_up();
        assert_eq!(shell.sidebar_width(size), SIDEBAR_WIDTH + 80.0);
        assert_eq!(shell.geometry(size).editor_width, before - 80.0);
        assert!(!shell.sidebar_resizing());
    }

    #[test]
    fn explorer_vertical_scrollbar_and_wheel_reach_later_entries() {
        let children = (0..80)
            .map(|index| FileNode {
                path: PathBuf::from("workspace").join(format!("file_{index:03}.rs")),
                is_directory: false,
                children: Vec::new(),
            })
            .collect();
        let mut shell = IdeShell::from_tree(FileNode {
            path: PathBuf::from("workspace"),
            is_directory: true,
            children,
        });
        let size = Size::new(1280.0, 800.0);
        let track = shell.explorer_vertical_scrollbar_rect(size);
        shell.scroll(
            Point::new(ACTIVITY_WIDTH + 40.0, EXPLORER_TOP + 40.0),
            5,
            size,
        );
        assert_eq!(shell.explorer_scroll_line, 5);
        shell.pointer_down(
            Point::new(
                track.origin.x + 5.0,
                track.origin.y + track.size.height - 1.0,
            ),
            size,
        );
        assert!(shell.explorer_scroll_line > 5);
    }

    #[test]
    fn editor_wheel_scrolls_and_terminal_profile_is_selectable() {
        let mut shell = test_shell();
        shell.editor.open_memory(
            "long.rs",
            (0..100)
                .map(|line| format!("line {line}"))
                .collect::<Vec<_>>()
                .join("\n"),
        );
        let size = Size::new(1280.0, 800.0);
        let editor_x = ACTIVITY_WIDTH + SIDEBAR_WIDTH;
        shell.scroll(Point::new(editor_x + 100.0, 200.0), 8, size);
        assert_eq!(shell.editor_scroll_line(), 8);
        let terminal_y = shell.geometry(size).editor_bottom + 10.0;
        shell.pointer_down(Point::new(editor_x + 115.0, terminal_y), size);
        assert_eq!(shell.selected_shell(), ShellKind::Cmd);
    }

    #[test]
    fn terminal_tabs_keep_input_and_content_isolated() {
        let mut shell = test_shell();
        let size = Size::new(1280.0, 800.0);
        let editor_x = ACTIVITY_WIDTH + SIDEBAR_WIDTH;
        let terminal_y = shell.geometry(size).editor_bottom + 10.0;

        shell.pointer_down(Point::new(editor_x + 10.0, terminal_y), size);
        shell.text_input("Get-Location");
        assert_eq!(shell.active_terminal_input(), "Get-Location");

        shell.pointer_down(Point::new(editor_x + 115.0, terminal_y), size);
        assert_eq!(shell.active_terminal_index(), 1);
        assert_eq!(shell.active_terminal_input(), "");
        shell.text_input("dir");
        assert_eq!(shell.active_terminal_input(), "dir");
        let rendered = shell
            .paint(size)
            .into_iter()
            .filter_map(|command| match command {
                PaintCommand::DrawText(command) => Some(command.text),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert!(rendered.iter().any(|text| text.ends_with("> dir")));
        assert!(!rendered.iter().any(|text| text.contains("Get-Location")));

        shell.pointer_down(Point::new(editor_x + 10.0, terminal_y), size);
        assert_eq!(shell.active_terminal_index(), 0);
        assert_eq!(shell.active_terminal_input(), "Get-Location");
    }

    #[cfg(windows)]
    #[test]
    fn terminal_input_is_above_command_and_output() {
        let mut shell = test_shell();
        let size = Size::new(1280.0, 800.0);
        let editor_x = ACTIVITY_WIDTH + SIDEBAR_WIDTH;
        let terminal_y = shell.geometry(size).editor_bottom + 10.0;
        shell.pointer_down(Point::new(editor_x + 10.0, terminal_y), size);
        shell.text_input("Write-Output RESULT_BELOW");
        shell.key_down("Enter");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while std::time::Instant::now() < deadline {
            shell.update_terminals(size);
            if shell
                .active_terminal_lines()
                .any(|line| line.contains("RESULT_BELOW"))
            {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        assert!(
            shell
                .active_terminal_lines()
                .any(|line| line.contains("RESULT_BELOW"))
        );

        let geo = shell.geometry(size);
        let input_y = geo.editor_bottom + 38.0;
        let first_output_y = geo.editor_bottom + 68.0;
        assert!(first_output_y > input_y);
    }

    /// Clicar no fim da trilha do editor leva ao fim do documento, e arrastar
    /// de volta traz o conteúdo junto.
    #[test]
    fn the_editor_scrollbar_maps_click_and_drag_to_content_offsets() {
        let mut shell = test_shell();
        let text = (0..200)
            .map(|line| format!("linha {line}"))
            .collect::<Vec<_>>()
            .join("
");
        shell.editor.open_memory("longo.rs", &text);
        let size = Size::new(1280.0, 800.0);
        let track = shell.editor_scrollbar_rect(size);
        let visible = shell.editor_visible_lines(size);

        shell.pointer_down(
            Point::new(track.origin.x + 5.0, track.origin.y + track.size.height),
            size,
        );
        assert_eq!(shell.editor_scroll_line(), 200 - visible);

        shell.pointer_move(Point::new(track.origin.x + 5.0, track.origin.y), size);
        assert_eq!(shell.editor_scroll_line(), 0);

        shell.pointer_up();
        shell.pointer_move(
            Point::new(track.origin.x + 5.0, track.origin.y + track.size.height),
            size,
        );
        assert_eq!(shell.editor_scroll_line(), 0, "soltar encerra o arraste");
    }

    #[test]
    fn terminal_selection_supports_forward_and_reverse_drag() {
        let forward = TerminalSelection {
            anchor: TextPosition { line: 2, column: 1 },
            focus: TextPosition { line: 2, column: 4 },
        };
        let reverse = TerminalSelection {
            anchor: forward.focus,
            focus: forward.anchor,
        };
        assert_eq!(selection_columns(Some(forward), 2, "abcdef"), Some((1, 4)));
        assert_eq!(selection_columns(Some(reverse), 2, "abcdef"), Some((1, 4)));
    }

    #[cfg(windows)]
    #[test]
    fn terminal_wheel_and_scrollbar_change_the_visible_offset() {
        let mut shell = test_shell();
        let size = Size::new(1280.0, 800.0);
        let editor_x = ACTIVITY_WIDTH + SIDEBAR_WIDTH;
        let terminal_y = shell.geometry(size).editor_bottom + 10.0;
        shell.pointer_down(Point::new(editor_x + 10.0, terminal_y), size);
        shell.text_input("1..80 | ForEach-Object { Write-Output \"scroll-$_\" }");
        shell.key_down("Enter");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while std::time::Instant::now() < deadline {
            shell.update_terminals(size);
            if shell.active_terminal().line_count() >= 80 {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        let active = shell.active_terminal;
        let bottom = shell.terminals[active].scroll_line;
        assert!(bottom > 0);

        let content_point = Point::new(editor_x + 100.0, shell.geometry(size).editor_bottom + 90.0);
        shell.scroll(content_point, -5, size);
        assert!(shell.terminals[active].scroll_line < bottom);

        let track = shell.terminal_scrollbar_rect(size);
        shell.pointer_down(Point::new(track.origin.x + 5.0, track.origin.y + 1.0), size);
        assert_eq!(shell.terminals[active].scroll_line, 0);
        shell.pointer_move(
            Point::new(track.origin.x + 5.0, track.origin.y + track.size.height),
            size,
        );
        assert!(shell.terminals[active].scroll_line > 0);
        shell.pointer_up();
    }

    #[cfg(windows)]
    #[test]
    fn vertically_resizing_terminal_never_changes_its_content() {
        let mut shell = test_shell();
        let size = Size::new(1280.0, 800.0);
        let editor_x = ACTIVITY_WIDTH + SIDEBAR_WIDTH;
        let terminal_header = shell.geometry(size).editor_bottom + 10.0;
        shell.pointer_down(Point::new(editor_x + 10.0, terminal_header), size);
        shell.text_input("Write-Output RESIZE_STABLE");
        shell.key_down("Enter");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while std::time::Instant::now() < deadline {
            shell.update_terminals(size);
            if shell
                .active_terminal_lines()
                .any(|line| line.trim() == "RESIZE_STABLE")
            {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        std::thread::sleep(std::time::Duration::from_millis(150));
        shell.update_terminals(size);
        let before = shell
            .active_terminal_lines()
            .map(str::to_owned)
            .collect::<Vec<_>>();

        let border = shell.geometry(size).editor_bottom;
        shell.pointer_down(Point::new(editor_x + 200.0, border), size);
        for y in [border - 20.0, border - 60.0, border - 100.0, border - 40.0] {
            shell.pointer_move(Point::new(editor_x + 200.0, y), size);
            shell.update_terminals(size);
        }
        shell.pointer_up();
        std::thread::sleep(std::time::Duration::from_millis(150));
        shell.update_terminals(size);

        let after = shell
            .active_terminal_lines()
            .map(str::to_owned)
            .collect::<Vec<_>>();
        assert_eq!(after, before);
    }

    #[test]
    fn terminal_button_minimizes_and_restores_previous_height() {
        let mut shell = test_shell();
        let size = Size::new(1280.0, 800.0);
        let original = shell.terminal_height();
        let toggle = Point::new(size.width - 20.0, shell.geometry(size).editor_bottom + 12.0);
        shell.pointer_down(toggle, size);
        assert!(shell.terminal_minimized());
        assert_eq!(
            shell.geometry(size).terminal_height,
            TERMINAL_COLLAPSED_HEIGHT
        );

        let restore = Point::new(size.width - 20.0, shell.geometry(size).editor_bottom + 12.0);
        shell.pointer_down(restore, size);
        assert!(!shell.terminal_minimized());
        assert_eq!(shell.terminal_height(), original);
    }

    #[test]
    fn dragging_terminal_top_border_changes_height_with_limits() {
        let mut shell = test_shell();
        let size = Size::new(1280.0, 800.0);
        let editor_x = ACTIVITY_WIDTH + SIDEBAR_WIDTH;
        let border_y = shell.geometry(size).editor_bottom;
        shell.pointer_down(Point::new(editor_x + 100.0, border_y), size);
        assert!(shell.terminal_resizing());
        assert!(shell.pointer_move(Point::new(editor_x + 100.0, border_y - 70.0), size));
        assert_eq!(shell.terminal_height(), TERMINAL_DEFAULT_HEIGHT + 70.0);
        shell.pointer_move(Point::new(editor_x + 100.0, size.height), size);
        assert_eq!(shell.terminal_height(), TERMINAL_MIN_HEIGHT);
        shell.pointer_up();
        assert!(!shell.terminal_resizing());
    }

    #[test]
    fn control_click_emits_language_neutral_navigation_request() {
        let mut shell = test_shell();
        let document_id = shell.editor.open_memory("main.rs", "fn target() {}\n");
        let size = Size::new(1280.0, 800.0);
        let editor_x = ACTIVITY_WIDTH + SIDEBAR_WIDTH;
        let target_x = editor_x + EDITOR_GUTTER + 5.0 * EDITOR_CHAR_WIDTH;
        shell.pointer_down_with_modifiers(
            Point::new(target_x, TITLE_HEIGHT + TAB_HEIGHT + 15.0),
            size,
            true,
        );
        assert_eq!(
            shell.take_navigation_request(),
            Some(NavigationRequest {
                document_id,
                byte_offset: 5,
                token: "target".to_owned(),
            })
        );
    }

    /// Ir para a definição rola o editor até o destino.
    ///
    /// Antes o cursor era movido e mais nada: um método declarado abaixo da área
    /// visível continuava fora da tela, e a navegação parecia não ter acontecido.
    #[test]
    fn going_to_a_definition_scrolls_the_target_line_into_view() {
        let mut shell = test_shell();
        let texto = (0..200)
            .map(|linha| format!("linha {linha}"))
            .collect::<Vec<_>>()
            .join("
");
        shell.editor.open_memory("Longo.java", &texto);
        let size = Size::new(1280.0, 800.0);
        let visiveis = shell.editor_visible_lines(size);
        assert!(120 > visiveis, "o destino precisa estar fora da tela");
        assert_eq!(shell.editor_scroll_line(), 0);

        assert!(shell.open_location(Path::new("Longo.java"), 120, 0).is_ok());
        // A revelação acontece na pintura, que é onde a altura do editor existe.
        let _ = shell.paint(size);

        let topo = shell.editor_scroll_line();
        assert!(
            topo <= 120 && 120 < topo + visiveis,
            "a linha 120 precisa ficar visível; topo={topo}, visíveis={visiveis}"
        );
        // E rolou o mínimo necessário, não saltou para o fim do arquivo.
        assert!(topo > 0);
    }

    /// A linha de destino fica destacada até o cursor sair de lá.
    #[test]
    fn the_navigated_line_is_highlighted_until_the_cursor_moves() {
        let mut shell = test_shell();
        let texto = (0..60)
            .map(|linha| format!("linha {linha}"))
            .collect::<Vec<_>>()
            .join("
");
        shell.editor.open_memory("Longo.java", &texto);
        let size = Size::new(1280.0, 800.0);
        let destaque = Theme::default().colors.highlight;
        let destacadas = |shell: &mut IdeShell| {
            shell
                .paint(size)
                .iter()
                .filter(|command| {
                    matches!(command, PaintCommand::FillRect(fill) if fill.color == destaque)
                })
                .count()
        };
        assert_eq!(destacadas(&mut shell), 0, "sem navegação, nada destacado");

        assert!(shell.open_location(Path::new("Longo.java"), 30, 0).is_ok());
        assert_eq!(destacadas(&mut shell), 1, "o destino fica destacado");

        // Clicar em outro lugar tira o destaque, sem ninguém precisar apagá-lo.
        let editor_x = ACTIVITY_WIDTH + SIDEBAR_WIDTH;
        shell.pointer_down(
            Point::new(
                editor_x + EDITOR_GUTTER + 2.0 * EDITOR_CHAR_WIDTH,
                TITLE_HEIGHT + TAB_HEIGHT + 3.0 * EDITOR_LINE_HEIGHT + 5.0,
            ),
            size,
        );
        assert_eq!(destacadas(&mut shell), 0, "mover o cursor encerra o destaque");
    }

    #[test]
    fn open_location_opens_file_and_positions_cursor() {
        let mut shell = test_shell();
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
        assert!(shell.open_location(&path, 1, 3).is_ok());
        let position = line_column(shell.active_text().unwrap_or_default(), shell.editor_pane.cursor());
        assert_eq!(position, (1, 3));
        assert_eq!(shell.focus(), ShellFocus::Editor);
    }


    #[test]
    fn project_menu_requests_build_and_reimport() {
        let mut shell = test_shell();
        let size = Size::new(1_000.0, 700.0);

        shell.pointer_down(Point::new(200.0, 10.0), size);
        shell.pointer_down(Point::new(200.0, TITLE_HEIGHT + 10.0), size);
        assert!(shell.take_build_project_request());
        assert!(!shell.take_reimport_project_request());

        shell.pointer_down(Point::new(200.0, 10.0), size);
        shell.pointer_down(Point::new(200.0, TITLE_HEIGHT + 38.0), size);
        assert!(shell.take_reimport_project_request());
        assert!(!shell.take_build_project_request());
    }

    /// A interface não define cor própria: todas vêm do tema da ERLibUi.
    ///
    /// Sem esta trava, uma cor solta volta a aparecer na primeira pressa — foi
    /// o que aconteceu com a barra de status e com o marcador de breakpoint.
    #[test]
    fn the_interface_does_not_hardcode_colors() {
        let source = include_str!("lib.rs");
        let tests_start = source
            .find("\nmod tests {")
            .unwrap_or_else(|| panic!("módulo de testes não encontrado"));
        for (number, line) in source[..tests_start].lines().enumerate() {
            assert!(
                !line.contains("Color::rgba"),
                "linha {} usa cor fixa; use um token de `Theme`: {}",
                number + 1,
                line.trim()
            );
        }
    }

    #[test]
    fn the_theme_comes_from_the_library_and_reaches_its_components() {
        let mut shell = test_shell();
        let size = Size::new(1_000.0, 700.0);
        assert_eq!(shell.theme(), &Theme::dark(), "o tema padrão é o da lib");

        let dark: Vec<Color> = shell
            .paint(size)
            .iter()
            .filter_map(|command| match command {
                PaintCommand::DrawText(text) => Some(text.color),
                _ => None,
            })
            .collect();

        shell.set_theme(Theme::high_contrast());
        let contrast: Vec<Color> = shell
            .paint(size)
            .iter()
            .filter_map(|command| match command {
                PaintCommand::DrawText(text) => Some(text.color),
                _ => None,
            })
            .collect();

        assert_ne!(dark, contrast, "trocar o tema muda o que é pintado");
        assert!(
            contrast.contains(&Theme::high_contrast().colors.text),
            "o texto usa o token do tema ativo"
        );
        // A barra de menus é um componente da lib: ela recebe o tema pelo
        // contexto de pintura, sem a IDE redesenhá-la.
        assert!(
            shell.paint(size).iter().any(|command| matches!(
                command,
                PaintCommand::DrawText(text)
                    if text.text == "Arquivo"
                        && text.color == Theme::high_contrast().colors.text
            )),
            "os componentes da biblioteca seguem o tema da aplicação"
        );
    }

    #[test]
    fn status_bar_uses_palette_colors_with_readable_contrast() {
        let mut shell = test_shell();
        shell.set_status_message("Pronto");
        let size = Size::new(1_000.0, 700.0);
        let colors = Theme::default().colors;
        let geometry = shell.geometry(size);
        let commands = shell.paint(size);

        let background = commands.iter().find_map(|command| match command {
            PaintCommand::FillRect(rect)
                if rect.rect.origin.y == geometry.content_bottom && rect.rect.size.height > 1.0 =>
            {
                Some(rect.color)
            }
            _ => None,
        });
        assert_eq!(
            background,
            Some(colors.surface),
            "a barra usa a superfície da paleta, não a cor de destaque"
        );

        let text_color = commands.iter().find_map(|command| match command {
            PaintCommand::DrawText(text) if text.text.starts_with("Pronto") => Some(text.color),
            _ => None,
        });
        assert_eq!(
            text_color,
            Some(colors.text),
            "o texto usa a cor de texto da paleta, não branco puro"
        );
        assert!(
            contrast_ratio(colors.text, colors.surface) >= 7.0,
            "texto e fundo da barra precisam de contraste confortável"
        );
    }

    /// Razão de contraste WCAG entre duas cores opacas.
    fn contrast_ratio(first: Color, second: Color) -> f32 {
        fn luminance(color: Color) -> f32 {
            fn channel(value: f32) -> f32 {
                if value <= 0.03928 {
                    value / 12.92
                } else {
                    ((value + 0.055) / 1.055).powf(2.4)
                }
            }
            0.2126 * channel(color.red)
                + 0.7152 * channel(color.green)
                + 0.0722 * channel(color.blue)
        }
        let first = luminance(first);
        let second = luminance(second);
        (first.max(second) + 0.05) / (first.min(second) + 0.05)
    }

    #[test]
    fn status_bar_shows_the_imported_project_summary() {
        let mut shell = test_shell();
        shell.set_project_summary(Some("Maven • demo • 2 módulo(s)".to_owned()));
        assert_eq!(shell.project_summary(), Some("Maven • demo • 2 módulo(s)"));
        assert!(
            shell
                .paint(Size::new(1_000.0, 700.0))
                .iter()
                .any(|command| matches!(
                    command,
                    PaintCommand::DrawText(text) if text.text.contains("Maven • demo • 2 módulo(s)")
                ))
        );
    }

    #[test]
    fn settings_menu_opens_compiler_and_vm_page() {
        let mut shell = test_shell();
        let size = Size::new(1_000.0, 700.0);
        shell.pointer_down(Point::new(340.0, 10.0), size);
        assert!(shell.take_open_settings_request());

        shell.open_settings_dialog(vec!["JDK 8".to_owned(), "JDK 17".to_owned()], 0);
        assert!(shell.settings_dialog_open());
        let paint = shell.paint(size);
        let labels = paint
            .iter()
            .filter_map(|command| match command {
                PaintCommand::DrawText(text) => Some(text.text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert!(labels.contains(&"Configurações"));
        assert!(labels.contains(&"Compilador e VM"));
        assert!(labels.contains(&"JDK 8"));
        assert!(labels.contains(&"Procurar..."));
        assert!(labels.contains(&"PowerShell"));
        assert!(labels.contains(&"ER IDE"));
        assert!(
            paint
                .iter()
                .any(|command| matches!(command, PaintCommand::LayerBreak))
        );
    }

    #[test]
    fn settings_jdk_combo_and_browse_button_emit_requests() {
        let mut shell = test_shell();
        let size = Size::new(1_000.0, 700.0);
        shell.open_settings_dialog(vec!["JDK 8".to_owned(), "JDK 17".to_owned()], 0);
        shell
            .settings_modal
            .layout(&LayoutContext::default(), Rect::new(0.0, 0.0, size.width, size.height));
        let geometry = settings_dialog_geometry(shell.settings_modal.panel_bounds());
        shell.pointer_down(
            Point::new(
                geometry.combo.origin.x + 10.0,
                geometry.combo.origin.y + 10.0,
            ),
            size,
        );
        shell.pointer_down(
            Point::new(
                geometry.combo.origin.x + 10.0,
                geometry.combo.origin.y + geometry.combo.size.height + 28.0 + 5.0,
            ),
            size,
        );
        // A escolha fica pendente: a janela é uma transação.
        assert_eq!(shell.take_settings_jdk_result(), None);

        shell.pointer_down(
            Point::new(
                geometry.browse.origin.x + 10.0,
                geometry.browse.origin.y + 10.0,
            ),
            size,
        );
        assert!(shell.take_browse_jdk_request());
    }

    /// Salvar aplica o que foi escolhido e fecha.
    #[test]
    fn saving_the_settings_applies_the_chosen_jdk() {
        let mut shell = test_shell();
        let size = Size::new(1_000.0, 700.0);
        shell.open_settings_dialog(vec!["JDK 8".to_owned(), "JDK 17".to_owned()], 0);
        let geometry = open_settings_geometry(&mut shell, size);
        choose_second_jdk(&mut shell, &geometry, size);
        shell.pointer_down(
            Point::new(geometry.save.origin.x + 10.0, geometry.save.origin.y + 10.0),
            size,
        );
        assert_eq!(shell.take_settings_jdk_result(), Some(1));
        assert!(!shell.settings_dialog_open());
    }

    /// Cancelar descarta o que foi mexido, e o combo volta ao que estava.
    #[test]
    fn cancelling_the_settings_discards_every_change() {
        let mut shell = test_shell();
        let size = Size::new(1_000.0, 700.0);
        shell.open_settings_dialog(vec!["JDK 8".to_owned(), "JDK 17".to_owned()], 0);
        let geometry = open_settings_geometry(&mut shell, size);
        choose_second_jdk(&mut shell, &geometry, size);
        shell.pointer_down(
            Point::new(
                geometry.close.origin.x + 10.0,
                geometry.close.origin.y + 10.0,
            ),
            size,
        );
        assert_eq!(shell.take_settings_jdk_result(), None);
        assert!(!shell.settings_dialog_open());
        assert_eq!(shell.jdk_combo.selected_index(), 0);
    }

    /// Projeto Maven com um pacote já criado, para o menu agir sobre ele.
    fn shell_with_package() -> IdeShell {
        IdeShell::from_tree(dir(
            "demo",
            vec![dir(
                "demo/src/main/java",
                vec![dir(
                    "demo/src/main/java/br",
                    vec![dir("demo/src/main/java/br/com", Vec::new())],
                )],
            )],
        ))
    }

    /// O menu abre a janela com o pacote do alvo já preenchido.
    ///
    /// Quem clicou com o botão direito sobre um pacote não deveria ter que
    /// digitar de novo onde está.
    #[test]
    fn the_new_item_dialog_opens_with_the_clicked_package() {
        let mut shell = shell_with_package();
        shell.context_menu_target = Some(PathBuf::from("demo/src/main/java/br/com"));
        shell.run_explorer_command("explorer.new.class");
        assert!(shell.new_item_dialog_open());
        assert_eq!(shell.new_item_package.value(), "br.com");
        assert_eq!(shell.new_item_name.value(), "");
    }

    /// A mesma janela serve as três ações, mudando só o título e a legenda.
    #[test]
    fn the_three_menu_actions_share_one_window() {
        let mut shell = shell_with_package();
        shell.context_menu_target = Some(PathBuf::from("demo/src/main/java/br/com"));
        for (command, title) in [
            ("explorer.new.package", "Novo pacote"),
            ("explorer.new.class", "Nova classe"),
            ("explorer.new.interface", "Nova interface"),
        ] {
            shell.run_explorer_command(command);
            let kind = shell
                .new_item_dialog
                .as_ref()
                .map(|dialog| dialog.kind)
                .unwrap_or(NewItemKind::Package);
            assert_eq!(kind.title(), title);
            assert_eq!(shell.new_item_package.value(), "br.com");
        }
    }

    /// Enter com só o pacote pede o pacote; o nome fica vazio.
    #[test]
    fn enter_with_only_the_package_asks_for_the_package() {
        let mut shell = shell_with_package();
        shell.context_menu_target = Some(PathBuf::from("demo/src/main/java/br/com"));
        shell.run_explorer_command("explorer.new.package");
        // O foco começa no pacote, com o cursor no fim do que veio preenchido.
        shell.text_input(".exemplo");
        shell.key_down("Enter");
        let request = shell.take_new_item_request();
        assert_eq!(
            request,
            Some(NewItemRequest {
                kind: NewItemKind::Package,
                package: "br.com.exemplo".to_owned(),
                name: String::new(),
                source_root: PathBuf::from("demo/src/main/java"),
            })
        );
    }

    /// Prepara um shell com arquivo aberto e foco no editor.
    fn shell_editing(text: &str) -> IdeShell {
        let mut shell = test_shell();
        shell.editor.open_memory("Pedido.java", text);
        shell.focus = ShellFocus::Editor;
        shell.editor_pane.set_cursor(0);
        shell
    }

    /// Coluna do editor em coordenadas de tela.
    fn editor_column(shell: &IdeShell, size: Size, index: usize) -> Point {
        let geometry = shell.geometry(size);
        let editor_x = ACTIVITY_WIDTH + shell.sidebar_width(size);
        Point::new(
            editor_x + EDITOR_GUTTER + index as f32 * EDITOR_CHAR_WIDTH,
            geometry.content_top + 20.0,
        )
    }

    /// Área de transferência de teste, sem depender do sistema.
    #[derive(Default)]
    struct FakeClipboard {
        text: std::sync::Mutex<Option<String>>,
    }

    impl ClipboardService for FakeClipboard {
        fn get_text(&self) -> Result<Option<String>, ui_window_api::ClipboardError> {
            Ok(self.text.lock().ok().and_then(|text| text.clone()))
        }

        fn set_text(&self, value: &str) -> Result<(), ui_window_api::ClipboardError> {
            if let Ok(mut text) = self.text.lock() {
                *text = Some(value.to_owned());
            }
            Ok(())
        }
    }

    /// O duplo clique seleciona a palavra, e a regra vem do editor da biblioteca.
    #[test]
    fn a_double_click_selects_the_word_under_the_pointer() {
        let mut shell = shell_editing("int total = 10;");
        let size = Size::new(1280.0, 800.0);
        // Coluna 6 cai no meio de `total`.
        shell.select_word_at_point(editor_column(&shell, size, 6), size);
        assert_eq!(shell.editor_pane.selection_range(), Some(4..9));
        assert_eq!(
            shell
                .active_text()
                .and_then(|text| text.get(4..9))
                .map(str::to_owned),
            Some("total".to_owned())
        );
    }

    /// Copiar leva o trecho para a área de transferência; colar o traz de volta.
    #[test]
    fn copying_and_pasting_go_through_the_clipboard() {
        let clipboard = Arc::new(FakeClipboard::default());
        let mut shell = shell_editing("total");
        shell.set_clipboard(clipboard.clone());
        shell.editor_pane.set_cursor(5);
        shell.editor_pane.set_selection(Some((0, 5)));
        assert!(shell.copy_selection());
        assert_eq!(
            clipboard.get_text().unwrap_or_default(),
            Some("total".to_owned())
        );

        shell.editor_pane.set_selection(None);
        shell.editor_pane.set_cursor(5);
        assert!(shell.paste_clipboard());
        assert_eq!(shell.active_text(), Some("totaltotal"));
    }

    /// Colar sobre uma seleção troca o trecho marcado.
    #[test]
    fn pasting_over_a_selection_replaces_it() {
        let clipboard = Arc::new(FakeClipboard::default());
        assert!(clipboard.set_text("novo").is_ok());
        let mut shell = shell_editing("abcdef");
        shell.set_clipboard(clipboard);
        shell.editor_pane.set_cursor(4);
        shell.editor_pane.set_selection(Some((1, 4)));
        assert!(shell.paste_clipboard());
        assert_eq!(shell.active_text(), Some("anovoef"));
    }

    /// Sem área de transferência, copiar avisa em vez de fingir que copiou.
    #[test]
    fn copying_without_a_clipboard_reports_it() {
        let mut shell = shell_editing("total");
        shell.editor_pane.set_selection(Some((0, 5)));
        assert!(!shell.copy_selection());
        assert_eq!(shell.status_message(), "Área de transferência indisponível");
    }

    /// O clique direito no editor abre o menu de copiar e colar; sem seleção,
    /// copiar aparece desabilitado.
    #[test]
    fn the_editor_context_menu_offers_copy_and_paste() {
        let mut shell = shell_editing("total");
        let size = Size::new(1280.0, 800.0);
        shell.secondary_pointer_down(editor_column(&shell, size, 2), size);
        assert!(shell.context_menu_open());
        let entries = shell.context_menu.entries();
        assert_eq!(entry_labels(entries), vec!["Copiar", "Colar"]);
        let copy_enabled = |entries: &[MenuEntry]| match &entries[0] {
            MenuEntry::Item(item) => item.enabled,
            MenuEntry::Separator => false,
        };
        assert!(!copy_enabled(entries), "sem seleção não há o que copiar");

        shell.editor_pane.set_selection(Some((0, 5)));
        shell.secondary_pointer_down(editor_column(&shell, size, 2), size);
        assert!(copy_enabled(shell.context_menu.entries()));
    }

    /// O objeto inspecionado: um `Pedido` com um campo simples e outro objeto.
    fn inspection_value() -> DebugVariableView {
        DebugVariableView {
            name: "pedido".to_owned(),
            value: "Pedido@1a2b".to_owned(),
            type_name: Some("br.com.exemplo.Pedido".to_owned()),
            expandable: true,
        }
    }

    fn inspection_fields() -> Vec<DebugVariableView> {
        vec![
            DebugVariableView {
                name: "total".to_owned(),
                value: "42".to_owned(),
                type_name: Some("int".to_owned()),
                expandable: false,
            },
            DebugVariableView {
                name: "cliente".to_owned(),
                value: "Cliente@3c4d".to_owned(),
                type_name: Some("br.com.exemplo.Cliente".to_owned()),
                expandable: true,
            },
        ]
    }

    /// A janela abre com o objeto na lista e o detalhe do item destacado.
    #[test]
    fn the_inspection_window_lists_the_object_and_details_the_selection() {
        let mut shell = shell_editing("int total = 10;");
        let size = Size::new(1280.0, 800.0);
        shell.show_inspection("pedido", inspection_value(), inspection_fields());
        assert!(shell.inspection_open());
        assert_eq!(shell.inspected_expression(), Some("pedido"));

        let texts = painted_texts(&mut shell, size);
        // Painel esquerdo: a raiz aberta, com os campos abaixo dela.
        assert!(
            texts.iter().any(|text| {
                text.contains("pedido = (br.com.exemplo.Pedido) Pedido@1a2b")
            }),
            "a árvore precisa mostrar o objeto: {texts:?}"
        );
        assert!(
            texts.iter().any(|text| text.contains("total = (int) 42")),
            "a árvore precisa mostrar os campos: {texts:?}"
        );
        // Painel direito: detalhe da raiz, que abre destacada.
        assert!(
            texts.iter().any(|text| text == "br.com.exemplo.Pedido"),
            "o detalhe precisa mostrar o tipo: {texts:?}"
        );
    }

    /// Clicar em um campo troca o que o painel direito detalha.
    #[test]
    fn clicking_a_field_changes_the_detail_panel() {
        let mut shell = shell_editing("int total = 10;");
        let size = Size::new(1280.0, 800.0);
        shell.show_inspection("pedido", inspection_value(), inspection_fields());
        let geometry = inspection_layout(&mut shell, size);
        // Segunda linha da árvore: o primeiro campo, com a raiz já aberta.
        shell.pointer_down(
            Point::new(
                geometry.list.origin.x + 30.0,
                geometry.list.origin.y + INSPECTION_ROW_HEIGHT + 4.0,
            ),
            size,
        );
        assert_eq!(
            shell.inspection_selected().map(|entry| entry.name.clone()),
            Some("total".to_owned())
        );
    }

    /// Abrir um campo que é objeto pede os campos dele ao alvo.
    ///
    /// Os níveis seguintes não vêm juntos: o grafo de um objeto pode ser fundo e
    /// cíclico, e percorrê-lo inteiro para mostrar o primeiro nível travaria.
    #[test]
    fn expanding_a_nested_object_asks_the_target_for_its_fields() {
        let mut shell = shell_editing("int total = 10;");
        let size = Size::new(1280.0, 800.0);
        shell.show_inspection("pedido", inspection_value(), inspection_fields());
        let geometry = inspection_layout(&mut shell, size);
        // Terceira linha: `cliente`, que é objeto.
        shell.pointer_down(
            Point::new(
                geometry.list.origin.x + 30.0,
                geometry.list.origin.y + INSPECTION_ROW_HEIGHT * 2.0 + 4.0,
            ),
            size,
        );
        assert_eq!(
            shell.take_debug_requests(),
            vec![DebugRequest::ExpandInspection("pedido.cliente".to_owned())]
        );

        // Os campos chegam e passam a aparecer sob o nó aberto.
        shell.add_inspection_fields(
            "pedido.cliente",
            vec![DebugVariableView {
                name: "nome".to_owned(),
                value: "João da Silva".to_owned(),
                type_name: Some("String".to_owned()),
                expandable: false,
            }],
        );
        assert!(
            painted_texts(&mut shell, size)
                .iter()
                .any(|text| text.contains("nome = (String) João da Silva")),
            "o campo do objeto aninhado precisa aparecer"
        );
    }

    /// Sem sessão viva não há onde executar, e a janela diz isso.
    ///
    /// A árvore continua mostrando o que foi lido enquanto a execução estava
    /// parada, então sem esse aviso o usuário clicaria em Executar achando que a
    /// sessão ainda está de pé.
    #[test]
    fn running_without_a_live_session_explains_itself() {
        let mut shell = shell_editing("int total = 10;");
        shell.show_inspection("pedido", inspection_value(), inspection_fields());
        shell.inspection_source = TextBuffer::new("m.setId(4L);");
        shell.run_inspection_source();
        assert!(shell.take_debug_requests().is_empty());
        assert_eq!(
            shell.inspection_message.as_deref(),
            Some("A sessão de depuração terminou; reconecte para executar")
        );
    }

    /// O painel direito reusa o editor, com os comportamentos de arquivo
    /// desligados: escrever ali não é editar um arquivo do projeto.
    #[test]
    fn the_inspection_editor_reuses_the_pane_without_file_behaviours() {
        let mut shell = shell_editing("int total = 10;");
        let size = Size::new(1280.0, 800.0);
        shell.show_inspection("pedido", inspection_value(), inspection_fields());
        let geometry = inspection_layout(&mut shell, size);
        let capabilities = shell.inspection_editor.capabilities();
        assert!(!capabilities.save, "não há arquivo para salvar");
        assert!(!capabilities.navigation, "não há definição para navegar");
        assert!(!capabilities.breakpoint_gutter, "não há linha onde parar");
        assert!(!capabilities.context_menu);

        // Clicar no editor leva o foco e a digitação para lá.
        shell.pointer_down(
            Point::new(geometry.source.origin.x + 60.0, geometry.source.origin.y + 8.0),
            size,
        );
        shell.text_input("pedido.total");
        assert_eq!(shell.inspection_source(), "pedido.total");
        // O documento aberto no editor principal não foi tocado.
        assert_eq!(shell.active_text(), Some("int total = 10;"));
    }

    /// Executar pede a avaliação do que foi digitado.
    #[test]
    fn running_the_inspection_source_asks_for_its_evaluation() {
        let mut shell = shell_editing("int total = 10;");
        shell.debug.attached = true;
        let size = Size::new(1280.0, 800.0);
        shell.show_inspection("pedido", inspection_value(), inspection_fields());
        let geometry = inspection_layout(&mut shell, size);
        shell.pointer_down(
            Point::new(geometry.source.origin.x + 60.0, geometry.source.origin.y + 8.0),
            size,
        );
        shell.text_input("pedido.cliente.nome");
        shell.pointer_down(
            Point::new(geometry.run.origin.x + 10.0, geometry.run.origin.y + 10.0),
            size,
        );
        assert_eq!(
            shell.take_debug_requests(),
            vec![DebugRequest::Evaluate("pedido.cliente.nome".to_owned())]
        );
    }

    /// Sem nada escrito, Executar avisa em vez de pedir a avaliação do vazio.
    #[test]
    fn running_an_empty_source_asks_nothing() {
        let mut shell = shell_editing("int total = 10;");
        shell.debug.attached = true;
        shell.show_inspection("pedido", inspection_value(), inspection_fields());
        shell.run_inspection_source();
        assert!(shell.take_debug_requests().is_empty());
        assert_eq!(shell.status_message(), "Escreva a expressão a executar");
    }

    /// Esc e o botão Fechar dispensam a janela.
    #[test]
    fn the_inspection_window_closes() {
        let mut shell = shell_editing("int total = 10;");
        shell.show_inspection("pedido", inspection_value(), inspection_fields());
        shell.escape();
        assert!(!shell.inspection_open());

        let size = Size::new(1280.0, 800.0);
        shell.show_inspection("pedido", inspection_value(), inspection_fields());
        let geometry = inspection_layout(&mut shell, size);
        shell.pointer_down(
            Point::new(
                geometry.close.origin.x + 10.0,
                geometry.close.origin.y + 10.0,
            ),
            size,
        );
        assert!(!shell.inspection_open());
    }

    fn inspection_layout(shell: &mut IdeShell, size: Size) -> InspectionGeometry {
        shell.inspection_modal.layout(
            &LayoutContext::default(),
            Rect::new(0.0, 0.0, size.width, size.height),
        );
        inspection_geometry(shell.inspection_modal.panel_bounds())
    }

    fn painted_texts(shell: &mut IdeShell, size: Size) -> Vec<String> {
        shell
            .paint(size)
            .iter()
            .filter_map(|command| match command {
                PaintCommand::DrawText(text) => Some(text.text.clone()),
                _ => None,
            })
            .collect()
    }

    /// Sem depuração em curso o menu do editor não oferece Inspecionar.
    ///
    /// Fora de uma sessão não há quadro que dê valor ao nome, e o item prometeria
    /// o que não pode cumprir.
    #[test]
    fn inspect_only_appears_while_debugging() {
        let mut shell = shell_editing("int total = 10;");
        let size = Size::new(1280.0, 800.0);
        shell.editor_pane.set_selection(Some((4, 9)));
        shell.secondary_pointer_down(editor_column(&shell, size, 6), size);
        assert_eq!(
            entry_labels(shell.context_menu.entries()),
            vec!["Copiar", "Colar"]
        );

        shell.debug.attached = true;
        shell.secondary_pointer_down(editor_column(&shell, size, 6), size);
        assert_eq!(
            entry_labels(shell.context_menu.entries()),
            vec!["Copiar", "Colar", "—", "Inspecionar"]
        );
    }

    /// Inspecionar pede a avaliação do trecho marcado.
    #[test]
    fn inspecting_asks_to_evaluate_the_selected_text() {
        let mut shell = shell_editing("int total = 10;");
        shell.debug.attached = true;
        shell.editor_pane.set_selection(Some((4, 9)));
        shell.run_explorer_command("debug.inspect");
        assert_eq!(
            shell.take_debug_requests(),
            vec![DebugRequest::Evaluate("total".to_owned())]
        );
        assert_eq!(shell.status_message(), "Inspecionando total");
    }

    /// Sem seleção, Inspecionar aparece desabilitado e nada é pedido.
    #[test]
    fn inspecting_without_a_selection_asks_nothing() {
        let mut shell = shell_editing("int total = 10;");
        let size = Size::new(1280.0, 800.0);
        shell.debug.attached = true;
        shell.secondary_pointer_down(editor_column(&shell, size, 6), size);
        let entries = shell.context_menu.entries();
        let enabled = match &entries[3] {
            MenuEntry::Item(item) => item.enabled,
            MenuEntry::Separator => true,
        };
        assert!(!enabled, "sem seleção não há o que inspecionar");

        shell.run_explorer_command("debug.inspect");
        assert!(shell.take_debug_requests().is_empty());
    }

    /// As setas verticais movem o cursor entre linhas, preservando a coluna.
    #[test]
    fn vertical_arrows_move_the_cursor_between_lines() {
        let mut shell = shell_editing("primeira
segunda
ab");
        shell.editor_pane.set_cursor(4);
        shell.key_down("ArrowDown");
        // Mesma coluna na linha de baixo.
        assert_eq!(shell.editor_pane.cursor(), "primeira
".len() + 4);

        // Descer para uma linha curta para no fim dela, e não num ponto inexistente.
        shell.key_down("ArrowDown");
        assert_eq!(shell.editor_pane.cursor(), "primeira
segunda
ab".len());

        // Na última linha, descer de novo não faz nada.
        shell.key_down("ArrowDown");
        assert_eq!(shell.editor_pane.cursor(), "primeira
segunda
ab".len());

        shell.key_down("ArrowUp");
        assert_eq!(shell.editor_pane.cursor(), "primeira
".len() + 2);
    }

    /// Shift com as setas verticais estende a seleção por linhas.
    #[test]
    fn shift_with_vertical_arrows_extends_the_selection() {
        let mut shell = shell_editing("um
dois
tres");
        shell.editor_pane.set_cursor(0);
        let shift = Modifiers {
            shift: true,
            ..Modifiers::default()
        };
        shell.key_down_with_modifiers("ArrowDown", shift);
        assert_eq!(shell.editor_pane.selection_range(), Some(0..3));
    }

    /// Tab com um bloco marcado desloca todas as linhas dele.
    #[test]
    fn tab_shifts_the_selected_block() {
        let mut shell = shell_editing("um
dois
tres");
        // Da segunda linha até o meio da terceira.
        shell.editor_pane.set_cursor(9);
        shell.editor_pane.set_selection(Some((3, 9)));
        shell.key_down("Tab");
        assert_eq!(shell.active_text(), Some("um
    dois
    tres"));
        // A seleção segue cobrindo o bloco, para indentar de novo sem remarcar.
        assert_eq!(shell.editor_pane.selection_range(), Some(3..20));

        let shift = Modifiers {
            shift: true,
            ..Modifiers::default()
        };
        shell.key_down_with_modifiers("Tab", shift);
        assert_eq!(shell.active_text(), Some("um
dois
tres"));
    }

    /// Arrastar no editor seleciona, e digitar substitui o trecho marcado.
    #[test]
    fn dragging_in_the_editor_selects_and_typing_replaces() {
        let mut shell = shell_editing("abcdef");
        let size = Size::new(1280.0, 800.0);
        shell.pointer_down(editor_column(&shell, size, 1), size);
        shell.pointer_move(editor_column(&shell, size, 4), size);
        shell.pointer_up();
        assert_eq!(shell.editor_pane.selection_range(), Some(1..4));

        shell.text_input("Z");
        assert_eq!(shell.active_text(), Some("aZef"));
        assert_eq!(shell.editor_pane.selection_range(), None);
    }

    /// A seleção chega ao editor da biblioteca, que é quem a desenha.
    #[test]
    fn the_selection_is_painted_by_the_library_editor() {
        let mut shell = shell_editing("abcdef");
        let size = Size::new(1280.0, 800.0);
        shell.pointer_down(editor_column(&shell, size, 1), size);
        shell.pointer_move(editor_column(&shell, size, 4), size);
        shell.pointer_up();
        let selection = shell.theme.colors.selection;
        assert!(
            shell.paint(size).iter().any(|command| matches!(
                command,
                PaintCommand::FillRect(fill) if fill.color == selection
            )),
            "o trecho selecionado precisa aparecer pintado"
        );
    }

    /// Shift+setas estende a seleção; sem Shift, mover desfaz.
    #[test]
    fn shift_arrows_extend_the_selection() {
        let mut shell = shell_editing("abcdef");
        let shift = Modifiers {
            shift: true,
            ..Modifiers::default()
        };
        shell.key_down_with_modifiers("ArrowRight", shift);
        shell.key_down_with_modifiers("ArrowRight", shift);
        assert_eq!(shell.editor_pane.selection_range(), Some(0..2));

        shell.key_down("ArrowRight");
        assert_eq!(shell.editor_pane.selection_range(), None);
    }

    /// Backspace com trecho marcado apaga o trecho, não um caractere.
    #[test]
    fn backspace_removes_the_selection() {
        let mut shell = shell_editing("abcdef");
        shell.editor_pane.set_cursor(4);
        shell.editor_pane.set_selection(Some((1, 4)));
        shell.key_down("Backspace");
        assert_eq!(shell.active_text(), Some("aef"));
        assert_eq!(shell.editor_pane.cursor(), 1);
    }

    /// Salvar grava o conteúdo da aba e limpa a marca de modificado.
    #[test]
    fn saving_writes_the_active_tab_to_disk() {
        let root = std::env::temp_dir().join(format!("er-ide-save-{}", std::process::id()));
        assert!(std::fs::create_dir_all(&root).is_ok());
        let file = root.join("Pedido.java");
        assert!(std::fs::write(&file, "class Pedido {}").is_ok());
        let Ok(mut shell) = IdeShell::open(&root) else {
            panic!("workspace de teste não abriu");
        };
        let Ok(_) = shell.open_file(&file) else {
            panic!("arquivo de teste não abriu");
        };
        shell.focus = ShellFocus::Editor;
        shell.editor_pane.set_cursor(0);
        shell.text_input("// nota\n");
        assert!(shell.active_document_modified(), "a edição deixa a aba suja");

        assert!(shell.save_active_document());
        assert_eq!(
            std::fs::read_to_string(&file).unwrap_or_default(),
            "// nota\nclass Pedido {}"
        );
        assert!(
            !shell.active_document_modified(),
            "depois de gravar a aba deixa de estar suja"
        );
        assert!(shell.status_message().starts_with("Salvo "));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// O item "Salvar" do menu Arquivo faz o mesmo que o atalho.
    #[test]
    fn the_file_menu_saves_the_active_tab() {
        let root = std::env::temp_dir().join(format!("er-ide-menu-save-{}", std::process::id()));
        assert!(std::fs::create_dir_all(&root).is_ok());
        let file = root.join("Pedido.java");
        assert!(std::fs::write(&file, "class Pedido {}").is_ok());
        let Ok(mut shell) = IdeShell::open(&root) else {
            panic!("workspace de teste não abriu");
        };
        let Ok(_) = shell.open_file(&file) else {
            panic!("arquivo de teste não abriu");
        };
        shell.focus = ShellFocus::Editor;
        shell.editor_pane.set_cursor(0);
        shell.text_input("// pelo menu\n");
        let size = Size::new(1280.0, 800.0);
        // Abre o menu Arquivo e escolhe a segunda entrada.
        shell.pointer_down(Point::new(100.0, TITLE_HEIGHT / 2.0), size);
        shell.pointer_down(Point::new(100.0, TITLE_HEIGHT + 42.0), size);
        assert_eq!(
            std::fs::read_to_string(&file).unwrap_or_default(),
            "// pelo menu\nclass Pedido {}"
        );
        assert!(!shell.active_document_modified());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A janela nasce no centro da tela.
    ///
    /// O painel se centraliza na área que recebe no layout; sem esse layout a
    /// área era zero e ele aparecia no canto superior esquerdo.
    #[test]
    fn the_new_item_dialog_opens_centered() {
        let mut shell = shell_with_package();
        let size = Size::new(1280.0, 800.0);
        shell.context_menu_target = Some(PathBuf::from("demo/src/main/java/br/com"));
        shell.run_explorer_command("explorer.new.class");
        // O painel do `ModalHost` é o retângulo de superfície desenhado sobre o
        // véu, do tamanho declarado para a janela.
        let surface = shell.theme.colors.surface;
        let panel = shell
            .paint(size)
            .iter()
            .filter_map(|command| match command {
                PaintCommand::FillRect(fill) if fill.color == surface => Some(fill.rect),
                _ => None,
            })
            .find(|rect| rect.size == NEW_ITEM_PANEL_SIZE)
            .unwrap_or_default();
        let center_x = panel.origin.x + panel.size.width / 2.0;
        let center_y = panel.origin.y + panel.size.height / 2.0;
        assert!(
            (center_x - size.width / 2.0).abs() < 1.0,
            "centro horizontal em {center_x}, esperado {}",
            size.width / 2.0
        );
        assert!(
            (center_y - size.height / 2.0).abs() < 1.0,
            "centro vertical em {center_y}, esperado {}",
            size.height / 2.0
        );
    }

    /// Reler o disco repõe os itens da árvore, e não só a expansão.
    ///
    /// O pacote e a classe eram criados e não apareciam: a `TreeView` guarda os
    /// itens dela, e a IDE relia o `FileNode` sem repô-los.
    #[test]
    fn reloading_the_workspace_shows_what_was_created() {
        let root = std::env::temp_dir().join(format!("er-ide-reload-{}", std::process::id()));
        let package = root.join("src/main/java/br/com");
        assert!(std::fs::create_dir_all(&package).is_ok());
        let Ok(mut shell) = IdeShell::open(&root) else {
            panic!("workspace de teste não abriu");
        };
        shell.reveal_in_explorer(&package);
        let size = Size::new(1280.0, 800.0);
        let shows = |shell: &mut IdeShell, needle: &str| {
            shell.paint(size).iter().any(|command| match command {
                PaintCommand::DrawText(text) => text.text.contains(needle),
                _ => false,
            })
        };
        assert!(
            !shows(&mut shell, "Pedido"),
            "a classe ainda não existe"
        );

        assert!(std::fs::write(package.join("Pedido.java"), "class Pedido {}").is_ok());
        assert!(shell.reload_workspace().is_ok());
        shell.reveal_in_explorer(&package);
        assert!(
            shows(&mut shell, "Pedido.java"),
            "a classe criada precisa aparecer na árvore"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Clicar no campo move o cursor, e o que se digita entra ali.
    ///
    /// O clique é entregue ao componente, que conhece a medição da fonte; a IDE
    /// não tenta adivinhar em que caractere o ponteiro caiu.
    #[test]
    fn clicking_a_field_moves_the_cursor_before_typing() {
        let mut shell = shell_with_package();
        let size = Size::new(1_000.0, 700.0);
        shell.context_menu_target = Some(PathBuf::from("demo/src/main/java/br/com"));
        shell.run_explorer_command("explorer.new.package");
        shell.new_item_modal.layout(
            &LayoutContext::default(),
            Rect::new(0.0, 0.0, size.width, size.height),
        );
        let geometry = new_item_geometry(shell.new_item_modal.panel_bounds());
        // Clique antes do primeiro caractere leva o cursor para o começo.
        shell.pointer_down(
            Point::new(
                geometry.package.origin.x + 1.0,
                geometry.package.origin.y + 8.0,
            ),
            size,
        );
        shell.text_input("dev.");
        shell.key_down("Enter");
        assert_eq!(
            shell.take_new_item_request().map(|request| request.package),
            Some("dev.br.com".to_owned())
        );
    }

    /// Com o nome preenchido, o tipo é pedido dentro do pacote informado.
    #[test]
    fn enter_with_a_name_asks_for_the_type_inside_the_package() {
        let mut shell = shell_with_package();
        shell.context_menu_target = Some(PathBuf::from("demo/src/main/java/br/com"));
        shell.run_explorer_command("explorer.new.interface");
        // Ao criar um tipo o foco já está no nome.
        shell.text_input("Repositorio");
        shell.key_down("Enter");
        assert_eq!(
            shell.take_new_item_request(),
            Some(NewItemRequest {
                kind: NewItemKind::Interface,
                package: "br.com".to_owned(),
                name: "Repositorio".to_owned(),
                source_root: PathBuf::from("demo/src/main/java"),
            })
        );
    }

    /// Tab troca o campo, então o pacote também é editável ao criar um tipo.
    #[test]
    fn tab_moves_between_the_two_fields() {
        let mut shell = shell_with_package();
        shell.context_menu_target = Some(PathBuf::from("demo/src/main/java/br/com"));
        shell.run_explorer_command("explorer.new.class");
        shell.key_down("Tab");
        shell.text_input(".exemplo");
        shell.key_down("Tab");
        shell.text_input("Pedido");
        shell.key_down("Enter");
        assert_eq!(
            shell.take_new_item_request(),
            Some(NewItemRequest {
                kind: NewItemKind::Class,
                package: "br.com.exemplo".to_owned(),
                name: "Pedido".to_owned(),
                source_root: PathBuf::from("demo/src/main/java"),
            })
        );
    }

    /// Classe sem nome não é pedido válido, e a janela fica aberta dizendo o quê.
    #[test]
    fn a_type_without_a_name_is_refused_without_closing() {
        let mut shell = shell_with_package();
        shell.context_menu_target = Some(PathBuf::from("demo/src/main/java/br/com"));
        shell.run_explorer_command("explorer.new.class");
        shell.key_down("Enter");
        assert_eq!(shell.take_new_item_request(), None);
        assert!(shell.new_item_dialog_open());
        assert_eq!(
            shell
                .new_item_dialog
                .as_ref()
                .and_then(|dialog| dialog.message.clone()),
            Some("Informe o nome.".to_owned())
        );
    }

    /// Esc fecha sem pedir nada.
    #[test]
    fn escape_closes_the_new_item_dialog_without_creating() {
        let mut shell = shell_with_package();
        shell.context_menu_target = Some(PathBuf::from("demo/src/main/java/br/com"));
        shell.run_explorer_command("explorer.new.class");
        shell.escape();
        assert!(!shell.new_item_dialog_open());
        assert_eq!(shell.take_new_item_request(), None);
    }

    /// Esc é cancelar: fechar sem descartar salvaria pela porta dos fundos.
    #[test]
    fn escape_in_the_settings_discards_every_change() {
        let mut shell = test_shell();
        let size = Size::new(1_000.0, 700.0);
        shell.open_settings_dialog(vec!["JDK 8".to_owned(), "JDK 17".to_owned()], 0);
        let geometry = open_settings_geometry(&mut shell, size);
        choose_second_jdk(&mut shell, &geometry, size);
        shell.escape();
        assert_eq!(shell.take_settings_jdk_result(), None);
        assert!(!shell.settings_dialog_open());
        assert_eq!(shell.jdk_combo.selected_index(), 0);
    }

    fn open_settings_geometry(shell: &mut IdeShell, size: Size) -> SettingsDialogGeometry {
        shell.settings_modal.layout(
            &LayoutContext::default(),
            Rect::new(0.0, 0.0, size.width, size.height),
        );
        settings_dialog_geometry(shell.settings_modal.panel_bounds())
    }

    /// Abre o combo e clica na segunda linha.
    fn choose_second_jdk(shell: &mut IdeShell, geometry: &SettingsDialogGeometry, size: Size) {
        shell.pointer_down(
            Point::new(
                geometry.combo.origin.x + 10.0,
                geometry.combo.origin.y + 10.0,
            ),
            size,
        );
        shell.pointer_down(
            Point::new(
                geometry.combo.origin.x + 10.0,
                geometry.combo.origin.y + geometry.combo.size.height + 28.0 + 5.0,
            ),
            size,
        );
    }
}
