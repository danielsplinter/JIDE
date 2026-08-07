//! A casca da interface: quem existe, onde fica, e de quem é o gesto.
//!
//! O que sobrou aqui é composição — o `IdeShell` com um campo por área, a
//! geometria da janela, o funil que decide qual superfície recebe o evento, e as
//! entradas que a aplicação chama. Cada área leva o que é dela nos módulos
//! abaixo: as janelas em `rename`, `generate`, `type_search`, `new_item`,
//! `settings` e `inspection`; os painéis em `editor_area`, `explorer_area`,
//! `terminal_area` e `debug_area`; e o quadro inteiro em `painting`.
//!
//! Ver `14-ide-shell-decomposition`.

#[cfg(test)]
use crate::debugging::DebugFrameView;
use crate::debugging::{DebugPanelState, DebugView};
use crate::ide_shell::generate::GenerateSurface;
use crate::ide_shell::git::GitSurface;
use crate::ide_shell::inspection::InspectionSurface;
use crate::ide_shell::new_item::NewItemSurface;
use crate::ide_shell::rename::RenameSurface;
use crate::ide_shell::settings::SettingsSurface;
use crate::text::{
    converted_syntax, count_outline, encloses_type, identifier_prefix, is_identifier_character,
    is_navigable, line_column, offset_for_line_column, offset_of_line, position_in_range, token_at,
};

use crate::editor::{
    CachedSyntax, EditorAction, EditorAreaState, EditorCapabilities, EditorPane, NavigationHistory,
    SyntaxView,
};
use crate::explorer::{
    ExplorerState, id as explorer_id, items as explorer_items, visible_row as visible_tree_row,
};
#[cfg(test)]
use crate::explorer::{Especie, NoDoExplorer, nomes as explorer_nomes};
use crate::ide_shell::tab_switcher::{AbaAberta, TabSwitcherSurface};
use crate::ide_shell::type_search::TypeSearchSurface;

use crate::settings::SettingsPage;
use crate::shell::{ShellCommandQueue, ShellFocus};
use crate::terminal::{
    BuscaNoTerminal, ScrollTarget, TerminalPanelState, TerminalSelection, TerminalTab, TextPosition,
    link_da_saida, selection_columns,
};
use ide_application::{
    ApplicationCommand, DebugRequest, FileOccurrences, NavigationRequest, OpenDocumentRequest,
    RecentProject, RenameDocumentRequest, SaveDocumentRequest, TaskId, UiContributionCatalog,
};
#[cfg(test)]
use ide_application::{NewItemTemplateId, SettingsSection, TaskDescriptor};

use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    path::{Path, PathBuf},
    sync::Arc,
};

use crate::layout::{DEBUG_BUTTONS, DebugPanelGeometry, Geometry};
use crate::menus::{
    MenuState, debug_request as debug_request_for, editor_entries as editor_menu_entries,
    explorer_entries as explorer_menu_entries,
};
#[cfg(test)]
use crate::search::search_display_path;
use ide_domain::{
    AccessorKind, CompletionItem, CompletionRequest, DocumentId, DocumentSnapshot, OutlineItem,
    SymbolKind, SyntaxSnapshot, TextPosition as DomainTextPosition, TextRange as DomainTextRange,
};
use ide_terminal::{GridCell, ShellKind, TerminalKey, TerminalModifiers, TerminalSession};
use ide_workspace::{EditorSession, FileNode, TextBuffer, rewrite_occurrences};
use ui_api::{EventContext, LayoutContext, PaintContext, TextMetrics, Widget};
#[cfg(test)]
use ui_components::MenuEntry;
use ui_components::{
    Button, ComposedTreeView, ContextMenu, Icon, IconTint, ListView, MenuBar, MenuBarItem, MenuItem,
    Popup, Scrollbar, ScrollbarOrientation, SplitOrientation, SplitPane, Splitter, StatusBar,
    TabItem, Tabs,
    TerminalCell, TerminalCursor, TerminalView, TextInput,
};
use context_menu::{AlvoDoMenu, ContextMenuSurface};
use routing::Alvo;
use ui_core::{
    ColorTokens, CommandId, EventResult, KeyEvent, Modifiers, Point, PointerButton, PointerEvent,
    Rect, Size, Theme, UiEvent, WidgetAction, WidgetId,
};
use ui_editor::{CodeEditor, GutterMark, LineDecoration};
use ui_host::UiHost;
use ui_layout_api::{CrossAlign, EdgeInsets, LayoutDirection, LayoutStyle, MainAlign};
use ui_layout_taffy::TaffyLayoutEngine;
use ui_render_api::PaintCommand;
use ui_window_api::ClipboardService;

pub(super) const ACTIVITY_WIDTH: f32 = 48.0;
const SIDEBAR_WIDTH: f32 = 260.0;
const SIDEBAR_MIN_WIDTH: f32 = 160.0;
pub(super) const TITLE_HEIGHT: f32 = 36.0;
pub(super) const TAB_HEIGHT: f32 = 38.0;
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
/// O corpo do console, e a largura de caractere de reserva para o instante em que
/// ainda não há medição de fonte ligada.
const TERMINAL_FONT_SIZE: f32 = 14.0;
/// A faixa da linha de comando, no pé do painel.
const TERMINAL_INPUT_HEIGHT: f32 = 30.0;
const TERMINAL_FALLBACK_CHAR_WIDTH: f32 = 8.4;
const TERMINAL_SCROLLBAR_WIDTH: f32 = 10.0;
const TERMINAL_DEFAULT_HEIGHT: f32 = 180.0;
const TERMINAL_MIN_HEIGHT: f32 = 120.0;
pub(super) const TERMINAL_COLLAPSED_HEIGHT: f32 = 30.0;
/// O que sobra de editor quando o terminal cresce até onde pode.
const EDITOR_MIN_HEIGHT: f32 = 100.0;
/// Os ícones de ação do título: lado e folga entre eles.
const ACTION_BUTTON_SIDE: f32 = 28.0;
const ACTION_BUTTON_GAP: f32 = 2.0;
const DEBUG_PANEL_WIDTH: f32 = 320.0;
pub(super) const DEBUG_ROW_HEIGHT: f32 = 21.0;
const MENU_BAR_ID: WidgetId = WidgetId(10_001);
/// A árvore da moldura, declarada ao anfitrião.
///
/// São as faixas que a `shell_geometry` calcula hoje. Elas entram na árvore
/// **antes** das camadas de sobreposição, porque é o que as janelas cobrem.
const FRAME_ID: WidgetId = WidgetId(10_460);
const FRAME_TITLE_ID: WidgetId = WidgetId(10_461);
const FRAME_MIDDLE_ID: WidgetId = WidgetId(10_462);
const FRAME_ACTIVITY_ID: WidgetId = WidgetId(10_463);
const FRAME_SIDEBAR_ID: WidgetId = WidgetId(10_464);
const FRAME_CENTER_ID: WidgetId = WidgetId(10_465);
const FRAME_TABS_ID: WidgetId = WidgetId(10_466);
const FRAME_EDITOR_ID: WidgetId = WidgetId(10_467);
const FRAME_TERMINAL_ID: WidgetId = WidgetId(10_468);
const FRAME_DEBUG_ID: WidgetId = WidgetId(10_469);
const FRAME_STATUS_ID: WidgetId = WidgetId(10_470);
/// A fileira de ações do título: parar, executar, depurar.
const FRAME_TITLE_ACTIONS_ID: WidgetId = WidgetId(10_471);
/// A coluna de trabalho: abas, editor e terminal, à esquerda do painel.
const FRAME_WORK_ID: WidgetId = WidgetId(10_472);
/// O interior do painel de depuração: fileira de ações e as duas listas.
const DEBUG_ACTIONS_ID: WidgetId = WidgetId(10_473);
const DEBUG_GAP_BEFORE_FRAMES_ID: WidgetId = WidgetId(10_474);
const DEBUG_GAP_BEFORE_VARS_ID: WidgetId = WidgetId(10_475);
/// Faixas da moldura: superfícies de fundo, sem conteúdo próprio.
const CHROME_BACKGROUND_ID: WidgetId = WidgetId(10_080);
const CHROME_TITLE_ID: WidgetId = WidgetId(10_081);
const CHROME_ACTIVITY_ID: WidgetId = WidgetId(10_082);
const CHROME_SIDEBAR_ID: WidgetId = WidgetId(10_083);
const CHROME_TABS_ID: WidgetId = WidgetId(10_084);
const CHROME_TERMINAL_ID: WidgetId = WidgetId(10_085);
const DEBUG_PANEL_SURFACE_ID: WidgetId = WidgetId(10_086);
const TERMINAL_INPUT_ID: WidgetId = WidgetId(10_087);
/// A saída, entre as abas e a linha de comando.
const TERMINAL_OUTPUT_ID: WidgetId = WidgetId(10_476);
/// A grade do emulador, desenhada dentro da faixa de saída.
const TERMINAL_GRID_ID: WidgetId = WidgetId(10_477);
const TERMINAL_CONSOLE_ID: WidgetId = WidgetId(10_088);
/// Textos da moldura e dos painéis, que são `Label` e não desenho.
const CHROME_TITLE_TEXT_ID: WidgetId = WidgetId(10_090);
const CHROME_EXPLORER_ID: WidgetId = WidgetId(10_091);
const CHROME_WORKSPACE_ID: WidgetId = WidgetId(10_092);
const EDITOR_EMPTY_ID: WidgetId = WidgetId(10_093);
const TERMINAL_COLLAPSED_ID: WidgetId = WidgetId(10_095);
const DEBUG_STATUS_ID: WidgetId = WidgetId(10_096);
const DEBUG_FRAMES_TITLE_ID: WidgetId = WidgetId(10_097);
const DEBUG_VARS_TITLE_ID: WidgetId = WidgetId(10_098);
/// Faixa de ids das células da lista, para não colidir com outros componentes.
/// A janela é larga porque o valor de um objeto costuma ser longo.
/// Fatia da janela ocupada pela lista, à esquerda.
const INSPECTION_LIST_FRACTION: f32 = 0.42;
/// Fatia do painel direito ocupada pelo detalhe; o resto é o editor.
const INSPECTION_DETAIL_FRACTION: f32 = 0.45;
const STOP_BUTTON_ID: WidgetId = WidgetId(10_009);
const RUN_BUTTON_ID: WidgetId = WidgetId(10_010);
const DEBUG_BUTTON_ID: WidgetId = WidgetId(10_011);
const DEBUG_FRAMES_ID: WidgetId = WidgetId(10_012);
/// Primeiro id da faixa de execução; os demais seguem em sequência.
const DEBUG_STEP_BASE_ID: WidgetId = WidgetId(10_076);
const DEBUG_VARIABLES_ID: WidgetId = WidgetId(10_013);
const STATUS_BAR_ID: WidgetId = WidgetId(10_014);
/// O giro que aparece enquanto uma linguagem prepara o projeto.
const PROJECT_LOADING_ID: WidgetId = WidgetId(10_015);
const EDITOR_TABS_ID: WidgetId = WidgetId(10_015);
const TERMINAL_TABS_ID: WidgetId = WidgetId(10_016);
/// A faixa de abas do editor da direita, quando a area esta dividida.
const SPLIT_TABS_ID: WidgetId = WidgetId(10_099);
/// O componente que reparte a area do editor em dois.
const SPLIT_PANE_ID: WidgetId = WidgetId(10_100);
const EDITOR_SCROLLBAR_ID: WidgetId = WidgetId(10_017);
const EDITOR_HORIZONTAL_SCROLLBAR_ID: WidgetId = WidgetId(10_063);
const TERMINAL_SCROLLBAR_ID: WidgetId = WidgetId(10_018);
const EXPLORER_VERTICAL_SCROLLBAR_ID: WidgetId = WidgetId(10_019);
const EXPLORER_HORIZONTAL_SCROLLBAR_ID: WidgetId = WidgetId(10_021);
const EXPLORER_TREE_ID: WidgetId = WidgetId(10_020);
const SIDEBAR_SPLITTER_ID: WidgetId = WidgetId(10_022);
const TERMINAL_SPLITTER_ID: WidgetId = WidgetId(10_023);
const TERMINAL_TOGGLE_ID: WidgetId = WidgetId(10_024);
/// A lupa da barra de atividades.
const ACTIVITY_SEARCH_ID: WidgetId = WidgetId(10_101);
/// O botão que mostra e esconde o painel do Explorer.
const ACTIVITY_SIDEBAR_ID: WidgetId = WidgetId(10_102);
/// O botão que abre o gerenciador de Git.
const ACTIVITY_GIT_ID: WidgetId = WidgetId(10_509);
const EXPLORER_CONTEXT_MENU_ID: WidgetId = WidgetId(10_025);
const COMPLETION_POPUP_ID: WidgetId = WidgetId(10_026);
const COMPLETION_LIST_ID: WidgetId = WidgetId(10_027);
const SEARCH_POPUP_ID: WidgetId = WidgetId(10_028);
const SEARCH_INPUT_ID: WidgetId = WidgetId(10_029);
/// A faixa que leva a barra de busca ao alto do editor, encostada à direita.
///
/// Existe porque um filho de sobreposição com tamanho declarado cola no canto
/// **superior esquerdo**; a faixa preenche a área e alinha ao fim, e é ela que
/// põe a barra do outro lado sem ninguém calcular coordenada.
const SEARCH_STRIP_ID: WidgetId = WidgetId(10_482);
/// A mesma barra de busca, do lado do terminal.
///
/// Outro nó, e não o mesmo movido: a área de cada faixa vem da moldura, e um nó
/// só não pode estar dentro de duas. O **componente** é o mesmo — um `Popup` com
/// um `TextInput` —, e é o que importa para quem usa: a busca é uma, e aparece
/// onde se está procurando.
const SEARCH_STRIP_TERMINAL_ID: WidgetId = WidgetId(10_103);
/// A faixa das abas do terminal, que agora tem duas coisas dentro.
const FRAME_TERMINAL_TOPO_ID: WidgetId = WidgetId(10_106);
const SEARCH_POPUP_TERMINAL_ID: WidgetId = WidgetId(10_104);
const SEARCH_INPUT_TERMINAL_ID: WidgetId = WidgetId(10_105);
/// Linhas visíveis da lista de completação antes de precisar rolar.
const COMPLETION_VISIBLE_ROWS: usize = 8;
const COMPLETION_ROW_HEIGHT: f32 = 24.0;
const COMPLETION_POPUP_WIDTH: f32 = 260.0;
/// Folga entre a borda da superfície flutuante e a lista dentro dela.
///
/// Entra na conta da área ocupada, e é por isso que é uma constante: o clique
/// precisa acertar a mesma área que a pintura desenhou, incluindo a moldura.
const COMPLETION_POPUP_PADDING: f32 = 4.0;
const SEARCH_BOX_WIDTH: f32 = 380.0;
const SEARCH_BOX_HEIGHT: f32 = 42.0;
/// A caixa da busca do terminal, que divide a fileira com as abas.
///
/// Menor que a do editor porque o lugar é outro: ali ela flutua sobre o código e
/// pode ser larga; aqui ela mora **na fileira das abas**, e o que ela ocupar sai
/// do espaço delas.
const SEARCH_BOX_TERMINAL_WIDTH: f32 = 260.0;
/// O que fica reservado à direita, onde mora o botão de recolher.
const TERMINAL_TOGGLE_ROOM: f32 = 34.0;
/// Raiz do anfitrião da tela inteira; não é desenhada.
const SHELL_ROOT_ID: WidgetId = WidgetId(10_200);
/// Primeiro id da faixa reservada às áreas das janelas; uma por superfície.
const SURFACE_LAYER_BASE: WidgetId = WidgetId(10_210);

/// Serviços e estado visual compartilhados apenas pelo coordenador.
struct ShellContext {
    focus: ShellFocus,
    text_metrics: Option<Arc<dyn TextMetrics>>,
    clipboard: Option<Arc<dyn ClipboardService>>,
    theme: Theme,
    status_message: String,
    project_summary: Option<String>,
    /// Onde o giro do carregamento está, quando alguma linguagem prepara o
    /// projeto.
    project_loading: Option<f32>,
    /// O que a IDE custa, já pronto para a barra de estado.
    ///
    /// Vem pronto de quem mede: a tela não soma nem formata número de memória —
    /// ela mostra o texto que recebeu, como faz com todo o resto.
    memory_usage: Option<String>,
    /// Tamanho da janela no último quadro.
    ///
    /// A soltura do ponteiro não recebe tamanho, e as janelas precisam dele
    /// para saber onde estão seus componentes.
    last_size: Size,
    scrollbar_drag: Option<ScrollTarget>,
}

/// As janelas que cobrem a tela, da que fica por cima para a que fica embaixo.
///
/// A ordem é a profundidade: a primeira aberta é a que recebe o gesto, e as de
/// baixo nem são consultadas. Registrar uma janela nova é acrescentar uma linha
/// aqui e um braço em cada `match` do funil — o roteamento não precisa saber que
/// ela existe. Ver `14-ide-shell-decomposition`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SurfaceKind {
    Rename,
    Git,
    Generate,
    TypeSearch,
    Inspection,
    NewItem,
    Settings,
    TabSwitcher,
}

const SURFACES: [SurfaceKind; 8] = [
    SurfaceKind::Rename,
    SurfaceKind::Git,
    SurfaceKind::Generate,
    SurfaceKind::TypeSearch,
    SurfaceKind::Inspection,
    SurfaceKind::NewItem,
    SurfaceKind::Settings,
    SurfaceKind::TabSwitcher,
];

/// Uma camada da tela, do fundo para a frente.
///
/// A lista de completação não é uma janela: nasce colada ao cursor e convive com
/// o que estiver aberto. Mas tem profundidade — cobre a inspeção e o editor, e é
/// coberta pelas janelas que tomam a tela inteira. Aqui isso deixa de ser um
/// número comparado à mão e passa a ser **a posição na pilha**, que é o que o
/// anfitrião consulta ao testar o acerto.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Layer {
    Surface(SurfaceKind),
    Completion,
}

/// A pilha inteira, do fundo para a frente.
///
/// É `SURFACES` lida de trás para frente — a ordem em que as janelas são
/// desenhadas — com a lista de completação declarada no lugar que ela ocupa.
const OVERLAY: [Layer; 9] = [
    Layer::Surface(SurfaceKind::Settings),
    // O gerenciador fica embaixo das janelas que ele pode abrir por cima: ele é
    // uma tela de trabalho, e não um diálogo que interrompe.
    Layer::Surface(SurfaceKind::Git),
    Layer::Surface(SurfaceKind::NewItem),
    Layer::Surface(SurfaceKind::TabSwitcher),
    Layer::Surface(SurfaceKind::Inspection),
    Layer::Completion,
    Layer::Surface(SurfaceKind::TypeSearch),
    Layer::Surface(SurfaceKind::Generate),
    Layer::Surface(SurfaceKind::Rename),
];

/// A área que uma janela aberta ocupa na pilha.
///
/// Ela cobre a tela inteira — o véu do `ModalHost` é o que está desenhado ali —,
/// e é essa área que decide se o gesto alcança o que ficou embaixo.
/// Declara as faixas da moldura, na ordem em que se empilham.
///
/// As medidas que mudam com o estado — largura da barra lateral, altura do
/// terminal, presença do painel de depuração — são ajustadas a cada quadro por
/// `sync_frame`, antes do arranjo.
fn declare_frame(host: &mut UiHost) {
    let coluna = LayoutStyle::default();
    let linha = LayoutStyle {
        direction: LayoutDirection::Row,
        ..LayoutStyle::default()
    };
    let fixa = |height: f32| LayoutStyle {
        height: Some(height),
        ..LayoutStyle::default()
    };
    let largura = |width: f32| LayoutStyle {
        width: Some(width),
        ..LayoutStyle::default()
    };
    let cresce = |style: LayoutStyle| LayoutStyle {
        flex_grow: 1.0,
        ..style
    };
    let _ = host.declare(SHELL_ROOT_ID, FRAME_ID, coluna);
    let _ = host.declare(FRAME_ID, FRAME_TITLE_ID, fixa(TITLE_HEIGHT));
    // Os três ícones encostam na direita do título e ficam no meio da altura:
    // é o alinhamento que os põe ali, e não a largura da janela menos a soma
    // deles.
    let _ = host.declare(
        FRAME_TITLE_ID,
        FRAME_TITLE_ACTIONS_ID,
        LayoutStyle {
            direction: LayoutDirection::Row,
            main_align: MainAlign::End,
            cross_align: CrossAlign::Center,
            gap: ACTION_BUTTON_GAP,
            padding: EdgeInsets::only(0.0, 10.0, 0.0, 0.0),
            flex_grow: 1.0,
            ..LayoutStyle::default()
        },
    );
    for id in [STOP_BUTTON_ID, RUN_BUTTON_ID, DEBUG_BUTTON_ID] {
        let _ = host.declare(
            FRAME_TITLE_ACTIONS_ID,
            id,
            LayoutStyle {
                width: Some(ACTION_BUTTON_SIDE),
                height: Some(ACTION_BUTTON_SIDE),
                ..LayoutStyle::default()
            },
        );
    }
    let _ = host.declare(FRAME_ID, FRAME_MIDDLE_ID, cresce(linha));
    let _ = host.declare(FRAME_ID, FRAME_STATUS_ID, fixa(StatusBar::HEIGHT));
    let _ = host.declare(FRAME_MIDDLE_ID, FRAME_ACTIVITY_ID, largura(ACTIVITY_WIDTH));
    let _ = host.declare(FRAME_MIDDLE_ID, FRAME_SIDEBAR_ID, largura(SIDEBAR_WIDTH));
    // O centro é uma linha: a coluna de trabalho e, à direita dela, o painel de
    // depuração — uma lateral de altura inteira, como painéis laterais são.
    let _ = host.declare(FRAME_MIDDLE_ID, FRAME_CENTER_ID, cresce(linha));
    let _ = host.declare(FRAME_CENTER_ID, FRAME_WORK_ID, cresce(coluna));
    let _ = host.declare(FRAME_CENTER_ID, FRAME_DEBUG_ID, largura(DEBUG_PANEL_WIDTH));
    let _ = host.declare(FRAME_WORK_ID, FRAME_TABS_ID, fixa(TAB_HEIGHT));
    // As abas ocupam a faixa inteira que lhes cabe: declarar o nó dentro dela é
    // o que tira a área da mão de quem as sincroniza.
    let _ = host.declare(FRAME_TABS_ID, EDITOR_TABS_ID, cresce(coluna));
    // O editor não encolhe além do que ainda é editável: é essa restrição que
    // impede o terminal de engoli-lo, e é ela que o divisor move.
    let _ = host.declare(
        FRAME_WORK_ID,
        FRAME_EDITOR_ID,
        LayoutStyle {
            flex_grow: 1.0,
            min_height: Some(EDITOR_MIN_HEIGHT),
            // Sobreposição: a barra de busca vive **sobre** o código, e não
            // empurrando-o para baixo. O texto continua ocupando a área inteira
            // do nó, que é o que a pintura lê.
            direction: LayoutDirection::Overlay,
            ..coluna
        },
    );
    let _ = host.declare(FRAME_EDITOR_ID, SEARCH_STRIP_ID, LayoutStyle::default());
    let _ = host.declare(
        SEARCH_STRIP_ID,
        SEARCH_POPUP_ID,
        LayoutStyle {
            width: Some(SEARCH_BOX_WIDTH),
            height: Some(SEARCH_BOX_HEIGHT),
            ..LayoutStyle::default()
        },
    );
    // Os cinco passos dividem a fileira em partes iguais, com folga entre eles.
    let _ = host.declare(
        FRAME_DEBUG_ID,
        DEBUG_ACTIONS_ID,
        LayoutStyle {
            direction: LayoutDirection::Row,
            height: Some(26.0),
            gap: 4.0,
            padding: EdgeInsets::only(0.0, 4.0, 0.0, 4.0),
            ..LayoutStyle::default()
        },
    );
    for index in 0..DEBUG_BUTTONS.len() {
        let _ = host.declare(
            DEBUG_ACTIONS_ID,
            WidgetId(DEBUG_STEP_BASE_ID.0 + index as u64),
            LayoutStyle {
                flex_grow: 1.0,
                ..LayoutStyle::default()
            },
        );
    }
    let _ = host.declare(FRAME_DEBUG_ID, DEBUG_GAP_BEFORE_FRAMES_ID, fixa(26.0));
    // A altura da lista de quadros vem da contagem, e por isso é redeclarada a
    // cada quadro em `sync_frame`.
    let _ = host.declare(FRAME_DEBUG_ID, DEBUG_FRAMES_ID, fixa(DEBUG_ROW_HEIGHT));
    let _ = host.declare(FRAME_DEBUG_ID, DEBUG_GAP_BEFORE_VARS_ID, fixa(30.0));
    let _ = host.declare(FRAME_DEBUG_ID, DEBUG_VARIABLES_ID, cresce(coluna));
    let _ = host.declare(
        FRAME_WORK_ID,
        FRAME_TERMINAL_ID,
        fixa(TERMINAL_COLLAPSED_HEIGHT),
    );
    // As três faixas do terminal, na ordem em que se leem: as abas, a saída, e
    // a linha de comando **no pé** — como em qualquer terminal, o que já foi
    // executado sobe e o cursor espera embaixo.
    // A fileira do topo do terminal: as abas à esquerda, e à direita a busca,
    // antes do botão de recolher. Uma linha, e não dois nós soltos — é o arranjo
    // que reparte a largura entre eles, como faz com os ícones do título.
    let _ = host.declare(
        FRAME_TERMINAL_ID,
        FRAME_TERMINAL_TOPO_ID,
        LayoutStyle {
            direction: LayoutDirection::Row,
            cross_align: CrossAlign::Center,
            height: Some(TERMINAL_TAB_HEIGHT),
            ..LayoutStyle::default()
        },
    );
    let _ = host.declare(
        FRAME_TERMINAL_TOPO_ID,
        TERMINAL_TABS_ID,
        LayoutStyle {
            flex_grow: 1.0,
            ..LayoutStyle::default()
        },
    );
    let _ = host.declare(FRAME_TERMINAL_ID, TERMINAL_OUTPUT_ID, cresce(coluna));
    let _ = host.declare(
        FRAME_TERMINAL_TOPO_ID,
        SEARCH_STRIP_TERMINAL_ID,
        LayoutStyle {
            direction: LayoutDirection::Row,
            main_align: MainAlign::End,
            cross_align: CrossAlign::Center,
            width: Some(SEARCH_BOX_TERMINAL_WIDTH + TERMINAL_TOGGLE_ROOM),
            // O espaço da direita é do botão de recolher, que se desenha por
            // cima da fileira: sem a reserva, a busca ficaria embaixo dele.
            padding: EdgeInsets::only(0.0, TERMINAL_TOGGLE_ROOM, 0.0, 0.0),
            ..LayoutStyle::default()
        },
    );
    let _ = host.declare(
        SEARCH_STRIP_TERMINAL_ID,
        SEARCH_POPUP_TERMINAL_ID,
        LayoutStyle {
            width: Some(SEARCH_BOX_TERMINAL_WIDTH),
            height: Some(TERMINAL_TAB_HEIGHT - 4.0),
            ..LayoutStyle::default()
        },
    );
    let _ = host.declare(
        FRAME_TERMINAL_ID,
        TERMINAL_INPUT_ID,
        fixa(TERMINAL_INPUT_HEIGHT),
    );
}

/// O nome da tecla, como a janela a recebe, na tecla que o shell entende.
fn tecla_do_terminal(key: &str) -> Option<TerminalKey> {
    Some(match key.to_ascii_lowercase().as_str() {
        "enter" => TerminalKey::Enter,
        "tab" => TerminalKey::Tab,
        "backspace" => TerminalKey::Backspace,
        "escape" => TerminalKey::Escape,
        "arrowup" => TerminalKey::Up,
        "arrowdown" => TerminalKey::Down,
        "arrowleft" => TerminalKey::Left,
        "arrowright" => TerminalKey::Right,
        "home" => TerminalKey::Home,
        "end" => TerminalKey::End,
        "pageup" => TerminalKey::PageUp,
        "pagedown" => TerminalKey::PageDown,
        "delete" => TerminalKey::Delete,
        // O resto é texto, e chega por `text_input`.
        _ => return None,
    })
}

/// Traduz uma célula da grade na célula que a biblioteca desenha.
///
/// A cor ausente **não** vira cor: fica em aberto, e o componente usa a do tema.
/// É o que faz a saída acompanhar a paleta em vez de trazer um cinza fixo. Quem
/// converte os três bytes em cor é a biblioteca — a IDE só repassa o que o
/// programa pediu.
fn celula_da_grade(cell: &GridCell) -> TerminalCell {
    TerminalCell::new(cell.character).with_rgb(cell.foreground, cell.background)
}

/// O estilo de uma camada, aberta ou fechada.
///
/// Aberta, ela cobre a tela: é essa área que decide se o gesto alcança o que
/// ficou embaixo, e é ela que vai centralizar o painel quando a janela passar a
/// ser arranjada pelo motor. Fechada, sai do arranjo — sem sair da árvore, para
/// a ordem não depender do uso.
fn layer_style(open: bool) -> LayoutStyle {
    LayoutStyle {
        hidden: !open,
        main_align: MainAlign::Center,
        cross_align: CrossAlign::Center,
        ..LayoutStyle::default()
    }
}

const fn surface_layer_id(kind: SurfaceKind) -> WidgetId {
    WidgetId(SURFACE_LAYER_BASE.0 + kind as u64)
}

/// Coordenador da interface. Cada feature é dona de seus widgets e seleção.
pub struct IdeShell {
    explorer: ExplorerState,
    editor_area: EditorAreaState,
    terminal: TerminalPanelState,
    search: TypeSearchSurface,
    /// A troca de abas por `Ctrl+Tab`.
    tab_switcher: TabSwitcherSurface,
    inspection: InspectionSurface,
    settings: SettingsSurface,
    debug_panel: DebugPanelState,
    /// As janelas de gerar e de renomear, cada uma com seu estado e seus
    /// eventos. Ver `14-ide-shell-decomposition`.
    generate: GenerateSurface,
    /// O gerenciador de Git, com a árvore de referências e as duas abas.
    git: GitSurface,
    new_item: NewItemSurface,
    rename: RenameSurface,
    menu: MenuState,
    /// O menu de contexto das três áreas que o têm. Ver `context_menu`.
    context_menu: ContextMenuSurface,
    catalog: UiContributionCatalog,
    /// Que espécie de tipo cada arquivo declara, por identidade de nó.
    ///
    /// O Explorer só conhece caminho e pasta; classe, interface e enumeração
    /// exigem saber o que está **dentro** do arquivo, e quem sabe isso é o
    /// índice. A aplicação pergunta fora da thread da janela e deposita a
    /// resposta aqui.
    ///
    /// Chaveado pela mesma identidade que a árvore usa — o hash do caminho —, e
    /// não pelo `PathBuf`: a entrada cai de cerca de duzentos bytes para trinta
    /// e dois, e é exatamente a chave pela qual a árvore pergunta.
    ///
    /// Ausente é uma resposta: um arquivo que o índice não alcançou não ganha
    /// crachá, em vez de ganhar um errado.
    declaration_kinds: HashMap<u64, SymbolKind>,
    context: ShellContext,
    commands: ShellCommandQueue,
    /// O runtime da tela: a pilha de camadas, o acerto e a entrega.
    ///
    /// Um só, e não um por janela: com todas na mesma árvore, a profundidade é a
    /// posição nela e ninguém precisa perguntar de quem é o gesto. Ver
    /// `16-single-host`.
    host: UiHost,
}

/// O anfitrião da tela, com a raiz em camada.
///
/// Em camada porque os filhos se sobrepõem na ordem de declaração: é o que
/// permite uma janela cobrir o conteúdo em vez de ser empurrada para baixo dele.
fn new_host() -> UiHost {
    let mut host = UiHost::new(
        SHELL_ROOT_ID,
        LayoutStyle {
            direction: LayoutDirection::Overlay,
            ..LayoutStyle::default()
        },
        Box::new(TaffyLayoutEngine),
    );
    // As camadas nascem com o shell, na ordem de `OVERLAY`, e não na ordem em
    // que cada janela é aberta pela primeira vez — que é acidental. Enquanto o
    // arranjo vier do consumidor, a ordem de sobreposição é a das chamadas de
    // `place`; quando vier do motor, passa a ser esta. Declará-las aqui é o que
    // torna as duas ordens a mesma.
    declare_frame(&mut host);
    // O conteúdo da moldura vem **antes** das camadas: é o que elas cobrem. A
    // ordem dos irmãos é a de sobreposição, e um nó criado só quando é
    // posicionado pela primeira vez entraria no fim, acima das janelas.
    let arranjado_pelo_consumidor = LayoutStyle {
        hidden: true,
        ..LayoutStyle::default()
    };
    for id in [EDITOR_TABS_ID, TERMINAL_TABS_ID] {
        let _ = host.declare(SHELL_ROOT_ID, id, arranjado_pelo_consumidor);
    }
    for layer in OVERLAY {
        let id = match layer {
            Layer::Surface(kind) => surface_layer_id(kind),
            Layer::Completion => COMPLETION_POPUP_ID,
        };
        // A camada ocupa a tela inteira: é a área que decide se o gesto alcança
        // o que ficou embaixo dela.
        let _ = host.declare(SHELL_ROOT_ID, id, layer_style(false));
        // A lista da completação é o conteúdo da camada dela, e entra logo
        // depois: acima do fundo da lista, abaixo das janelas que vêm a seguir.
        if matches!(layer, Layer::Completion) {
            let _ = host.declare(SHELL_ROOT_ID, COMPLETION_LIST_ID, arranjado_pelo_consumidor);
        }
    }
    host
}
impl IdeShell {
    /// Mensagem atual da barra de estado.
    #[must_use]
    pub fn status_message(&self) -> &str {
        &self.context.status_message
    }

    /// Quanto a IDE custa: o processo dela e o que roda fora.
    ///
    /// **São dois números de propósito.** Contabilmente são processos
    /// separados; fisicamente é a mesma RAM, e quem fica lento é quem está na
    /// frente de quem usa. Mostrar só o nosso esconderia metade da conta.
    ///
    /// Zero externo não aparece: quem só edita Java não precisa ver um campo
    /// vazio a vida toda.
    /// Diz que o projeto está sendo preparado, e onde o giro está.
    ///
    /// **Aparece no meio da tela, e não num canto.** Enquanto isso dura, o que a
    /// IDE responde é incompleto — a busca não acha, a completação não sabe os
    /// tipos —, e atribuir isso à IDE em vez de à espera é o mal-entendido que
    /// este giro existe para evitar. Ele some quando a preparação termina.
    pub fn set_project_loading(&mut self, phase: Option<f32>) {
        self.context.project_loading = phase;
    }

    pub fn set_memory_usage(&mut self, own_mb: u64, external_mb: u64) {
        self.context.memory_usage = Some(if external_mb == 0 {
            format!("{own_mb} MB")
        } else {
            format!("{own_mb} + {external_mb} MB")
        });
    }

    pub fn set_status_message(&mut self, message: impl Into<String>) {
        self.context.status_message = message.into();
    }

    #[must_use]
    pub fn ui_catalog(&self) -> &UiContributionCatalog {
        &self.catalog
    }

    /// Retira, em ordem, todas as intenções produzidas desde a última consulta.
    pub fn drain_application_commands(&mut self) -> Vec<ApplicationCommand> {
        self.commands.drain()
    }

    pub fn tab_count(&self) -> usize {
        self.editor_area.session.tabs().count()
    }

    /// Texto de um documento aberto, para quem vai gravá-lo.
    #[must_use]
    pub fn document_text(&self, document_id: DocumentId) -> Option<String> {
        self.editor_area
            .session
            .document(document_id)
            .map(|document| document.buffer.text().to_owned())
    }

    /// Resumo do projeto importado, apresentado na barra de status.
    pub fn set_project_summary(&mut self, summary: Option<String>) {
        self.context.project_summary = summary;
    }
    pub fn project_summary(&self) -> Option<&str> {
        self.context.project_summary.as_deref()
    }
    pub fn workspace_path(&self) -> &Path {
        &self.explorer.workspace.path
    }

    /// As faixas da moldura, lidas do arranjo.
    ///
    /// Nada é calculado aqui: cada número é a área que o motor deu a uma faixa.
    /// O painel de depuração não precisa mais ser subtraído do editor — ele é
    /// irmão do centro na mesma linha, e ocupar lugar já é o que o encolhe.
    fn geometry(&self) -> Geometry {
        let area = |id| {
            self.host
                .bounds(id)
                .unwrap_or(Rect::new(0.0, 0.0, 0.0, 0.0))
        };
        let tabs = area(FRAME_TABS_ID);
        let editor = area(FRAME_EDITOR_ID);
        let status = area(FRAME_STATUS_ID);
        Geometry {
            content_top: tabs.origin.y + tabs.size.height,
            content_bottom: status.origin.y,
            editor_bottom: editor.origin.y + editor.size.height,
            editor_width: editor.size.width,
            editor_height: editor.size.height,
            terminal_height: area(FRAME_TERMINAL_ID).size.height,
        }
    }

    fn sync_splitters(&mut self, size: Size) {
        self.explorer.splitter = self.sidebar_splitter_for(size);
        self.terminal.splitter = self.terminal_splitter_for(size);
    }

    /// Traz de volta o tamanho que cada divisor definiu.
    fn apply_splitters(&mut self) {
        let content_bottom = self.geometry().content_bottom;
        if self.explorer.splitter.is_dragging() {
            self.explorer.sidebar_width = self.explorer.splitter.position() - ACTIVITY_WIDTH;
        }
        if self.terminal.splitter.is_dragging() {
            self.terminal.height = content_bottom - self.terminal.splitter.position();
            self.terminal.last_height = self.terminal.height;
        }
    }

    /// Entrega o clique ao divisor cujo alvo o contém.
    fn splitter_pointer_down(&mut self, point: Point, size: Size) -> bool {
        self.sync_splitters(size);
        let event = UiEvent::PointerDown(primary_pointer(point));
        let mut context = EventContext::default();
        if matches!(
            self.explorer.splitter.event(&mut context, &event),
            EventResult::Handled
        ) {
            return true;
        }
        !self.terminal.minimized
            && matches!(
                self.terminal.splitter.event(&mut context, &event),
                EventResult::Handled
            )
    }

    /// A janela aberta que está por cima, se houver alguma.
    ///
    /// É por aqui que cada entrada de evento pergunta a quem o gesto pertence,
    /// em vez de repetir a cadeia de `if` e um dia repeti-la torta.
    fn open_surface(&self) -> Option<SurfaceKind> {
        SURFACES
            .into_iter()
            .find(|kind| self.surface_is_open(*kind))
    }

    /// Declara ao anfitrião as áreas da pilha, do fundo para a frente.
    ///
    /// A ordem das chamadas **é** a ordem de sobreposição: quem é posicionado por
    /// último cobre os anteriores, para desenhar e para receber o gesto. É o que
    /// substitui a comparação de profundidades escrita à mão.
    pub(super) fn place_overlay(&mut self, size: Size) {
        self.host.clear_placement();
        // Primeiro quem está na tela, depois o arranjo, e só então as áreas que
        // a IDE ainda calcula. A ordem importa: quem posiciona à mão lê a
        // moldura do arranjo — a área do painel, a da barra lateral —, e lê-la
        // antes de calculá-la daria o quadro anterior.
        self.sync_frame(size);
        for layer in OVERLAY {
            if let Layer::Surface(kind) = layer {
                let open = self.surface_is_open(kind);
                self.host
                    .set_style(surface_layer_id(kind), layer_style(open));
                // O que a janela declara e muda com o estado dela — quantas
                // páginas há, qual está aberta — entra **antes** do arranjo, ou
                // valeria só no quadro seguinte.
                if open && kind == SurfaceKind::Settings {
                    let sections = self.catalog.settings_sections.len();
                    self.settings.sync_declaration(&mut self.host, sections);
                }
                // O mesmo vale para a divisa do gerenciador: a largura da coluna
                // sai da proporção dela, e declarada na pintura ficaria um
                // quadro atrás do ponteiro que a arrastou.
                if open && kind == SurfaceKind::Git {
                    self.git.sync_declaration(&mut self.host);
                }
            }
        }
        // A faixa da busca é declarada uma vez e escondida quando a barra não
        // está na tela: nó escondido não entra no instantâneo, e por isso não
        // recebe clique nem ocupa área.
        self.host.set_style(
            SEARCH_STRIP_ID,
            LayoutStyle {
                direction: LayoutDirection::Row,
                main_align: MainAlign::End,
                cross_align: CrossAlign::Start,
                padding: EdgeInsets::only(12.0, 0.0, 0.0, 12.0),
                // Cada faixa aparece quando a busca é da área dela. As duas
                // nunca estão na tela ao mesmo tempo: a barra é uma, e o alvo
                // diz onde ela mora.
                hidden: !self.editor_area.search_open,
                ..LayoutStyle::default()
            },
        );
        self.host.set_style(
            SEARCH_STRIP_TERMINAL_ID,
            LayoutStyle {
                direction: LayoutDirection::Row,
                main_align: MainAlign::End,
                cross_align: CrossAlign::Center,
                width: Some(SEARCH_BOX_TERMINAL_WIDTH + TERMINAL_TOGGLE_ROOM),
                padding: EdgeInsets::only(0.0, TERMINAL_TOGGLE_ROOM, 0.0, 0.0),
                hidden: self.terminal.busca.is_none() || self.terminal.minimized,
                ..LayoutStyle::default()
            },
        );
        let _ = self.host.layout(size);

        // A moldura é o fundo da pilha: as janelas a cobrem.
        self.sync_editor_tabs();
        if !self.terminal.minimized {
            self.sync_terminal_tabs();
        }
        for layer in OVERLAY {
            match layer {
                Layer::Completion => self.place_completion(size),
                // Primeiro a área que a janela cobre — o véu, que é o que
                // engole o gesto do que ficou atrás —, depois o que há dentro
                // dela.
                // A camada aberta cobre a tela por declaração: ela é filha da
                // raiz em camada, e filho de camada ocupa a área inteira.
                Layer::Surface(_) => {}
            }
        }
        // O motor calcula o que foi declarado; o que veio de `place` entra no
        // lugar que a árvore lhe dá. Ver `17-layout-adoption`.
        let _ = self.host.layout(size);
    }

    /// Entrega uma tecla ao programa que está no terminal.
    ///
    /// A codificação é do emulador, que conhece o modo em que o shell está. Um
    /// erro de escrita vira mensagem na barra de estado: perder a tecla em
    /// silêncio seria pior do que dizer que ela não chegou.
    fn send_terminal_key(&mut self, key: TerminalKey) {
        let modificadores = TerminalModifiers::empty();
        if let Err(erro) = self.active_terminal_mut().send_key(key, modificadores) {
            self.context.status_message = erro.to_string();
        }
    }

    /// As faixas do terminal, lidas do arranjo: a saída e a linha de comando.
    /// As mesmas faixas, para o teste conferir onde as coisas ficaram.
    #[cfg(test)]
    pub(super) fn terminal_bands_for_test(&self) -> (Rect, Rect) {
        self.terminal_bands()
    }

    fn terminal_bands(&self) -> (Rect, Rect) {
        let area = |id| {
            self.host
                .bounds(id)
                .unwrap_or(Rect::new(0.0, 0.0, 0.0, 0.0))
        };
        (area(TERMINAL_OUTPUT_ID), area(TERMINAL_INPUT_ID))
    }

    /// O interior do painel de depuração, lido do arranjo.
    fn debug_panel_geometry(&self) -> DebugPanelGeometry {
        let area = |id| {
            self.host
                .bounds(id)
                .unwrap_or(Rect::new(0.0, 0.0, 0.0, 0.0))
        };
        DebugPanelGeometry {
            panel: area(FRAME_DEBUG_ID),
            buttons: (0..DEBUG_BUTTONS.len())
                .map(|index| area(WidgetId(DEBUG_STEP_BASE_ID.0 + index as u64)))
                .collect(),
            frames: area(DEBUG_FRAMES_ID),
            variables: area(DEBUG_VARIABLES_ID),
        }
    }

    /// As áreas dos três ícones do título, na ordem em que aparecem.
    fn action_button_areas(&self) -> [Rect; 3] {
        [STOP_BUTTON_ID, RUN_BUTTON_ID, DEBUG_BUTTON_ID].map(|id| {
            self.host
                .bounds(id)
                .unwrap_or(Rect::new(0.0, 0.0, 0.0, 0.0))
        })
    }

    /// Põe na árvore o que muda com o estado da moldura.
    ///
    /// Largura da barra lateral e altura do terminal são o que os divisores
    /// movem; o painel de depuração entra e sai do arranjo com a sessão. Tudo
    /// **antes** do arranjo, ou valeria só no quadro seguinte.
    fn sync_frame(&mut self, size: Size) {
        let sidebar = self.sidebar_width(size);
        let terminal = if self.terminal.minimized {
            TERMINAL_COLLAPSED_HEIGHT
        } else {
            self.terminal.height
        };
        self.host.set_style(
            FRAME_SIDEBAR_ID,
            LayoutStyle {
                // Recolhido o nó some da moldura, e não apenas encolhe: com a
                // largura mínima ele voltaria a aparecer sozinho.
                hidden: self.sidebar_collapsed(),
                width: Some(sidebar),
                min_width: Some(if self.sidebar_collapsed() {
                    0.0
                } else {
                    SIDEBAR_MIN_WIDTH
                }),
                ..LayoutStyle::default()
            },
        );
        self.host.set_style(
            FRAME_TERMINAL_ID,
            LayoutStyle {
                height: Some(terminal),
                min_height: Some(TERMINAL_COLLAPSED_HEIGHT),
                ..LayoutStyle::default()
            },
        );
        let quadros = self.debug_panel.view.frames.len().clamp(1, 8) as f32;
        self.host.set_style(
            DEBUG_FRAMES_ID,
            LayoutStyle {
                height: Some(quadros * DEBUG_ROW_HEIGHT),
                ..LayoutStyle::default()
            },
        );
        self.host.set_style(
            FRAME_DEBUG_ID,
            LayoutStyle {
                hidden: !self.debug_panel.view.attached,
                width: Some(DEBUG_PANEL_WIDTH),
                padding: EdgeInsets::only(34.0, 6.0, 0.0, 6.0),
                ..LayoutStyle::default()
            },
        );
    }

    /// Se um alvo está acima da lista de completação na pilha deste quadro.
    fn covers_completion(&self, target: Option<WidgetId>) -> bool {
        let ordem = self.host.snapshot().paint_order();
        let posicao = |procurado: WidgetId| ordem.iter().position(|id| *id == procurado);
        target.and_then(posicao) > posicao(COMPLETION_POPUP_ID)
    }

    fn surface_is_open(&self, kind: SurfaceKind) -> bool {
        match kind {
            SurfaceKind::Rename => self.rename.is_open(),
            SurfaceKind::Git => self.git.is_open(),
            SurfaceKind::Generate => self.generate.is_open(),
            SurfaceKind::TypeSearch => self.search.is_open(),
            SurfaceKind::Inspection => self.inspection.is_open(),
            SurfaceKind::NewItem => self.new_item.is_open(),
            SurfaceKind::TabSwitcher => self.tab_switcher.is_open(),
            SurfaceKind::Settings => self.settings.is_open(),
        }
    }

    /// Esc na janela: cada uma decide o que desistir significa para ela.
    fn surface_escape(&mut self, kind: SurfaceKind) {
        match kind {
            SurfaceKind::Rename => self.cancel_rename(),
            SurfaceKind::Git => self.git.close(),
            SurfaceKind::Generate => self.close_generate(),
            SurfaceKind::TypeSearch => self.close_type_search(),
            SurfaceKind::Inspection => self.close_inspection(),
            SurfaceKind::NewItem => self.close_new_item_dialog(),
            SurfaceKind::TabSwitcher => self.tab_switcher.close(),
            // Esc nas configurações é cancelar: fechar sem descartar o que foi
            // mexido salvaria pela porta dos fundos.
            SurfaceKind::Settings => self.settings.cancel(),
        }
    }

    fn surface_pointer_down(&mut self, kind: SurfaceKind, point: Point, size: Size) {
        match kind {
            SurfaceKind::Rename => self.rename_pointer_down(point, size),
            SurfaceKind::Git => {
                let context = self.layout_context();
                if let Some(pedido) = self.git.pointer_down(&mut self.host, &context, point) {
                    self.pedir_ao_git(pedido);
                }
            }
            SurfaceKind::Generate => self.generate_pointer_down(point, size),
            SurfaceKind::TypeSearch => self.type_search_pointer_down(point, size),
            SurfaceKind::Inspection => self.inspection_pointer_down(point, size),
            SurfaceKind::NewItem => self.new_item_pointer_down(point, size),
            SurfaceKind::TabSwitcher => self.tab_switcher_pointer_down(point, size),
            SurfaceKind::Settings => self.settings_dialog_pointer_down(point, size),
        }
    }

    /// Movimento do ponteiro. `None` é "não é meu, siga adiante".
    fn surface_pointer_move(&mut self, kind: SurfaceKind, point: Point) -> Option<bool> {
        match kind {
            // Arrastar a barra da lista precisa do movimento: só com o clique
            // chegando, o indicador é pego e nunca anda.
            SurfaceKind::Rename => {
                self.rename_pointer_event(&UiEvent::PointerMove(primary_pointer(point)));
                Some(true)
            }
            // A divisa da janela do Git se arrasta, e por isso ela precisa do
            // movimento: só com o clique, a divisa é pega e nunca anda.
            SurfaceKind::Git => {
                let context = self.layout_context();
                self.git.pointer_event(
                    &self.host,
                    &context,
                    &UiEvent::PointerMove(primary_pointer(point)),
                );
                Some(true)
            }
            // A janela cobre o que está atrás: mover ali não arrasta nada.
            SurfaceKind::Settings => Some(false),
            _ => None,
        }
    }

    /// Soltura do ponteiro, que é o que encerra um arrasto começado na janela.
    fn surface_pointer_up(&mut self, kind: SurfaceKind) {
        if kind == SurfaceKind::Rename {
            self.rename_pointer_event(&UiEvent::PointerUp(primary_pointer(Point::ZERO)));
        }
        if kind == SurfaceKind::Git {
            let context = self.layout_context();
            self.git.pointer_event(
                &self.host,
                &context,
                &UiEvent::PointerUp(primary_pointer(Point::ZERO)),
            );
        }
    }

    /// A roda dentro da janela. `false` deixa passar para o que está atrás.
    fn surface_scroll(&mut self, kind: SurfaceKind, point: Point, delta_lines: f32) -> bool {
        match kind {
            // A janela cobre tudo: a roda ali é da lista dela, e nunca do editor
            // atrás — rolar o que está coberto é mexer no que não se vê.
            SurfaceKind::Rename => {
                self.rename_pointer_event(&UiEvent::Scroll(ui_core::ScrollEvent {
                    position: point,
                    delta_x: 0.0,
                    delta_y: delta_lines * generate::ROW_HEIGHT,
                }));
                true
            }
            SurfaceKind::Generate => {
                let context = self.layout_context();
                self.generate
                    .scroll(&self.host, &context, point, delta_lines);
                true
            }
            SurfaceKind::TypeSearch => {
                self.type_search_scroll(point, delta_lines);
                true
            }
            SurfaceKind::Git => {
                let context = self.layout_context();
                if let Some(pedido) = self.git.scroll(&self.host, &context, point, delta_lines) {
                    self.pedir_ao_git(pedido);
                }
                true
            }
            SurfaceKind::Settings => true,
            SurfaceKind::Inspection | SurfaceKind::NewItem | SurfaceKind::TabSwitcher => false,
        }
    }

    /// Tecla na janela. `false` deixa a tecla seguir para quem estiver atrás.
    fn surface_key(&mut self, kind: SurfaceKind, key: &str, modifiers: Modifiers) -> bool {
        match kind {
            SurfaceKind::Rename => self.rename_key(key, modifiers),
            SurfaceKind::Git => self.git.key(key),
            SurfaceKind::Generate => false,
            SurfaceKind::TypeSearch => self.type_search_key(key),
            SurfaceKind::Inspection => self.inspection_key(key, modifiers),
            SurfaceKind::NewItem => self.new_item_key(key),
            // A janela de abas só responde ao ciclo e ao Escape, e os dois
            // chegam por fora: o ciclo antes do roteamento, o Escape pelo
            // fechamento genérico.
            SurfaceKind::TabSwitcher => false,
            SurfaceKind::Settings => {
                let outcome = self.settings.key(key, modifiers);
                self.apply_settings_outcome(outcome);
                true
            }
        }
    }

    /// Se o painel de edição da frente é o de uma janela, e não o do arquivo.
    ///
    /// O que está atrás dela está coberto: arrastar a barra de rolagem ou marcar
    /// no terminal seria o gesto indo parar no que não se vê.
    fn front_editor_is_modal(&self) -> bool {
        self.inspection.is_open()
    }

    /// Texto digitado na janela. `false` deixa o texto seguir adiante.
    fn surface_text_input(&mut self, kind: SurfaceKind, text: &str) -> bool {
        match kind {
            SurfaceKind::Rename => self.rename_text_input(text),
            // Com a janela aberta, o que se digita é da busca das branches: é a
            // única caixa que ela tem, e é onde o cursor está.
            SurfaceKind::Git => {
                self.git.text_input(text);
                true
            }
            SurfaceKind::Generate => false,
            SurfaceKind::TypeSearch => self.type_search_text_input(text),
            SurfaceKind::Inspection => self.inspection_text_input(text),
            SurfaceKind::NewItem => self.new_item_text_input(text),
            SurfaceKind::TabSwitcher => false,
            SurfaceKind::Settings => self.settings.text_input(text),
        }
    }

    pub fn escape(&mut self) {
        // O teclado vai a quem tem o foco, e quem sabe disso é o anfitrião. Ele
        // só alcança um componente que esteja na pilha, então ela é declarada
        // antes: sem isso, a tecla chegaria numa janela que ainda não existe
        // para ele.
        self.place_overlay(self.context.last_size);
        // O menu de contexto é o que está por cima de tudo: é ele que Esc
        // dispensa primeiro.
        if self.context_menu_key("Escape", Modifiers::default()) {
            return;
        }
        if let Some(surface) = self.open_surface() {
            self.surface_escape(surface);
            return;
        }
        if !self.editor_area.completion_items.is_empty() {
            self.editor_area.completion_items.clear();
            return;
        }
        // Desistir da edição múltipla vem antes de largar a busca: são as marcas
        // que estão na frente do usuário, e o texto volta ao que era.
        if let Some(document) = self.editor_area.session.active_mut()
            && self
                .editor_area
                .pane
                .cancel_occurrences(&mut document.buffer)
        {
            return;
        }
        // `Esc` fecha **a busca que tem o foco**, e não as duas: com as duas
        // abertas, largar a da saída não pode largar a do arquivo junto.
        if self.context.focus == ShellFocus::SearchTerminal {
            self.close_terminal_search();
            return;
        }
        if self.editor_area.search_open {
            self.close_search();
            // As marcas saem com a busca. Deixá-las para trás encheria a tela de
            // destaque sem nada que dissesse de onde veio nem como tirar.
            self.refresh_search_hits();
        }
    }

    pub fn pointer_down(&mut self, point: Point, size: Size) {
        self.pointer_down_with_modifiers(point, size, false, false);
    }

    pub fn pointer_down_with_modifiers(
        &mut self,
        point: Point,
        size: Size,
        control: bool,
        shift: bool,
    ) {
        self.route_pointer_down(point, size, control, shift);
        // O gesto pode ter mudado a moldura — minimizar o terminal, ligar o
        // painel de depuração. A geometria é leitura do arranjo, e quem
        // perguntar em seguida precisa do arranjo já refeito.
        self.place_overlay(size);
    }

    fn route_pointer_down(&mut self, point: Point, size: Size, control: bool, shift: bool) {
        // O menu aberto tem a primeira palavra: escolher uma ação ou dispensá-lo
        // é o que este clique significa, e não o que está embaixo dele.
        if self.context_menu_event(&UiEvent::PointerDown(primary_pointer(point)), size) {
            return;
        }
        // A lista de completação vem antes das janelas que ela cobre: clicar em
        // outro lugar significa desistir dela. Quem decide se ela está por cima
        // é a pilha, e não uma profundidade escrita à mão.
        if self.completion_pointer_down(point, size) {
            return;
        }
        if let Some(surface) = self.open_surface() {
            self.surface_pointer_down(surface, point, size);
            return;
        }
        // A barra de busca está sobre o código: sem perguntar ao anfitrião, o
        // clique nela atravessaria e moveria o cursor do editor. Quem diz que o
        // ponto caiu ali é a área declarada, e não uma conta repetida aqui.
        if self.terminal.busca.is_some()
            && self
                .host
                .hit_test(point)
                .any(|id| id == SEARCH_POPUP_TERMINAL_ID || id == SEARCH_INPUT_TERMINAL_ID)
        {
            self.context.focus = ShellFocus::SearchTerminal;
            return;
        }
        let (popup_da_busca, campo_da_busca) = self.search_widget_ids();
        if self.editor_area.search_open
            && self
                .host
                .hit_test(point)
                .any(|id| id == popup_da_busca || id == campo_da_busca)
        {
            self.context.focus = ShellFocus::Search;
            return;
        }
        // O menu aberto é do mesmo tipo do menu de contexto: enquanto ele está
        // na tela, o clique é dele onde quer que caia. As listas descem sobre o
        // conteúdo, e é assim que se escolhe um item.
        if self.menu.bar.open_menu().is_some() && self.menu_bar_pointer_down(point, size) {
            return;
        }
        // **Daqui em diante o ponto decide sozinho de quem é o gesto.** Um alvo
        // só é resolvido, e só o tratador dele é chamado: nenhum deles vê um
        // clique que não é seu. Ver `routing`.
        let alvo = self.alvo_do_ponto(point, size);
        // De que lado da área dividida foi o clique: a pergunta é do roteamento
        // porque vale para qualquer alvo dentro das colunas, e para nenhum fora
        // delas. Ver `marcar_lado_do_clique`.
        if matches!(alvo, Alvo::Editor | Alvo::EditorDaDireita | Alvo::Abas) {
            self.marcar_lado_do_clique(point, size);
        }
        match alvo {
            Alvo::Topo => {
                if !self.action_buttons_pointer_down(point) {
                    self.menu_bar_pointer_down(point, size);
                }
            }
            Alvo::Atividades => {
                self.activity_pointer_down(point, size);
            }
            Alvo::RecolherTerminal => {
                self.terminal_toggle_pointer_down(point, size);
            }
            Alvo::Barra(alvo) => self.scrollbar_grab(alvo, point, size),
            Alvo::Divisor => {
                self.splitter_pointer_down(point, size);
            }
            Alvo::DivisaDosEditores | Alvo::EditorDaDireita => {
                if !self.split_pointer_down(point, size) {
                    self.editor_pointer_down(point, size, control, shift);
                }
            }
            Alvo::Abas => {
                if !self.split_pointer_down(point, size) {
                    self.editor_tabs_pointer_down(point, size);
                }
            }
            Alvo::Explorer => {
                self.explorer_pointer_down(point, size);
            }
            Alvo::Depuracao => {
                self.debug_panel_area_pointer_down(point, size);
            }
            Alvo::Editor => {
                self.editor_pointer_down(point, size, control, shift);
            }
            Alvo::Terminal => self.terminal_area_pointer_down(point, size),
            Alvo::Nenhum => {}
        }
    }

    pub fn pointer_move(&mut self, point: Point, size: Size) -> bool {
        // O movimento vai ao anfitrião antes de tudo: é dele o destaque do que
        // está sob o ponteiro, e sem o evento nenhum componente tem como saber
        // que ele passou por cima. Não é consumo: quem se destaca não decide
        // nada, e o gesto continua o caminho abaixo.
        self.place_overlay(size);
        self.host
            .event(&UiEvent::PointerMove(primary_pointer(point)));
        // A barra de menus vem cedo: o que passa por cima dela é dela, aberta ou
        // fechada — fechada ela ainda precisa do movimento para acender o item
        // apontado. Sob o ponteiro, o gesto para aqui; fora dela, segue adiante,
        // e o `true` de saída é só para apagar o realce que ficou aceso.
        let realce_mudou = self.menu_bar_pointer_move(point, size);
        if realce_mudou || self.menu.bar.hovering() {
            return realce_mudou;
        }
        // Com o menu aberto, o destaque acompanha o ponteiro dentro dele.
        if self.context_menu.is_open() {
            return self.context_menu_event(&UiEvent::PointerMove(primary_pointer(point)), size);
        }
        if let Some(surface) = self.open_surface()
            && let Some(handled) = self.surface_pointer_move(surface, point)
        {
            return handled;
        }
        // Com a inspeção aberta, o gesto é dela: o resto da janela está atrás do
        // painel, e arrastar sobre o que não se vê seria o gesto indo parar no
        // lugar errado.
        let inspecting = self.front_editor_is_modal();
        if !inspecting && let Some(target) = self.context.scrollbar_drag {
            self.sync_scrollbar(target, size);
            self.scrollbar_mut(target).event(
                &mut EventContext::default(),
                &UiEvent::PointerMove(primary_pointer(point)),
            );
            self.apply_scrollbar(target);
            return true;
        }
        // A divisa dos dois editores: mesmo parada ela precisa do movimento para
        // se destacar sob o ponteiro, e em arrasto é ela quem manda no gesto.
        if self.split_pointer_move(point, size) {
            return true;
        }
        // O arraste no editor é do painel, que sabe se um gesto começou nele.
        self.place_focused_editor(size);
        if let Some((pane, buffer)) = self.focused_editor()
            && pane.pointer_move(buffer, point)
        {
            return true;
        }
        if inspecting {
            return false;
        }
        if self.terminal.selecting {
            let position = self.terminal_position_at(point, size);
            if let Some(selection) = self.terminal.selection.as_mut() {
                selection.focus = position;
            }
            return true;
        }
        // O movimento vai sempre aos divisores: mesmo parados, eles precisam
        // saber que o ponteiro passou por cima para se destacar.
        let dragging = self.sidebar_resizing() || self.terminal_resizing();
        self.sync_splitters(size);
        let event = UiEvent::PointerMove(primary_pointer(point));
        self.explorer
            .splitter
            .event(&mut EventContext::default(), &event);
        self.terminal
            .splitter
            .event(&mut EventContext::default(), &event);
        if dragging {
            self.apply_splitters();
            return true;
        }
        // Parado, o retorno diz se o ponteiro está sobre o divisor do terminal,
        // para a janela trocar o cursor.
        !self.terminal.minimized && self.terminal.splitter.hit_area().contains(point)
    }

    pub fn pointer_up(&mut self) {
        // A soltura encerra o arrasto começado na janela: sem ela a lista
        // continuaria achando que o gesto está em curso e seguiria o ponteiro.
        if let Some(surface) = self.open_surface() {
            self.surface_pointer_up(surface);
        }
        // Encerrar o gesto é do painel, que sabe se ele virou seleção.
        self.editor_area.pane.pointer_up();
        self.inspection.editor_and_source().0.pointer_up();
        let event = UiEvent::PointerUp(primary_pointer(Point::ZERO));
        self.explorer
            .splitter
            .event(&mut EventContext::default(), &event);
        self.terminal
            .splitter
            .event(&mut EventContext::default(), &event);
        // A divisa dos dois editores também: sem o soltar ela continuaria em
        // arrasto para sempre, e o ponteiro ficaria preso nela sem ninguém estar
        // segurando o botão.
        if let Some(divisao) = self.editor_area.divisao.as_mut() {
            let _ = divisao
                .painel
                .event(&mut EventContext::default(), &event);
        }
        // A barra também precisa saber que o gesto acabou: ela é quem guarda o
        // ponto da pegada.
        if let Some(target) = self.context.scrollbar_drag.take() {
            self.scrollbar_mut(target).event(
                &mut EventContext::default(),
                &UiEvent::PointerUp(primary_pointer(Point::ZERO)),
            );
        }
        self.terminal.selecting = false;
        // Soltar um divisor muda a moldura, e quem pergunta a geometria logo
        // depois precisa da nova — não da do quadro anterior.
        self.place_overlay(self.context.last_size);
    }

    pub fn scroll(&mut self, point: Point, delta_lines: f32, size: Size) {
        // A pilha é declarada antes: é dela que sai a resposta sobre de quem é a
        // roda, quando ela cai dentro de uma janela.
        self.place_overlay(size);
        if let Some(surface) = self.open_surface()
            && self.surface_scroll(surface, point, delta_lines)
        {
            return;
        }
        let geo = self.geometry();
        if point.x >= ACTIVITY_WIDTH
            && point.x < ACTIVITY_WIDTH + self.sidebar_width(size)
            && point.y >= EXPLORER_TOP - EXPLORER_ROW_HEIGHT
            && point.y < geo.content_bottom
        {
            let max = self
                .visible_entries()
                .len()
                .saturating_sub(self.explorer_visible_lines());
            // Explorer e terminal são listas de linhas inteiras: aqui a
            // fração não tem onde aparecer, e o passo volta a ser em linhas.
            self.explorer.scroll_line = self
                .explorer
                .scroll_line
                .saturating_add_signed(delta_lines.round() as isize)
                .min(max);
        } else if point.y >= geo.content_top && point.y < geo.editor_bottom {
            // Em pixels, e por uma fração de linha a cada passo: rolar de linha
            // inteira faz o texto saltar, que é o que se sente como travado.
            let total = self.active_text().map_or(0, |text| text.lines().count());
            let visible = (geo.editor_height / EDITOR_LINE_HEIGHT).floor().max(1.0) as usize;
            let maximo = total.saturating_sub(visible) as f32 * EDITOR_LINE_HEIGHT;
            let passo = delta_lines * EDITOR_LINE_HEIGHT;
            // A roda gira o painel do lado apontado — e ele já é o painel da
            // frente, porque o movimento do ponteiro levou o foco para lá.
            let destino =
                (self.editor_area.pane.scroll_offset() + passo).clamp(0.0, maximo.max(0.0));
            self.editor_area.pane.set_scroll_offset(destino);
        } else if point.y >= geo.editor_bottom && point.y < geo.content_bottom {
            let active = self.terminal.active;
            let atual = self.terminal.tabs[active].scroll_line;
            let alvo = atual.saturating_add_signed(delta_lines.round() as isize);
            self.set_terminal_scroll(alvo);
        }
    }

    /// Se o texto digitado agora chega ao editor de código.
    ///
    /// **A resposta não é "há um documento aberto".** Com a janela de busca na
    /// frente, existe um documento aberto atrás dela e ele não está recebendo
    /// nada — mas quem perguntasse pelo documento ativo receberia `Some` e agiria
    /// como se estivesse. Foi o que acontecia: digitar um ponto na busca abria o
    /// menu de completação no editor, sobre uma janela que não era a dele.
    ///
    /// Isto vale para **todas** as janelas sobrepostas, e não só para a busca: a
    /// pergunta é a mesma que `text_input` já responde para si mesma, dita em voz
    /// alta para quem precisa dela do lado de fora.
    #[must_use]
    pub fn text_reaches_editor(&self) -> bool {
        self.open_surface().is_none() && self.context.focus == ShellFocus::Editor
    }

    pub fn text_input(&mut self, text: &str) {
        // O teclado vai a quem tem o foco, e quem sabe disso é o anfitrião. Ele
        // só alcança um componente que esteja na pilha, então ela é declarada
        // antes: sem isso, a tecla chegaria numa janela que ainda não existe
        // para ele.
        self.place_overlay(self.context.last_size);
        if let Some(surface) = self.open_surface()
            && self.surface_text_input(surface, text)
        {
            return;
        }
        match self.context.focus {
            ShellFocus::Editor => self.edit_active(text),
            ShellFocus::Search => {
                self.editor_area.search_query.push_str(text);
                self.refresh_search_hits();
            }
            ShellFocus::SearchTerminal => self.type_in_terminal_search(text),
            // O texto digitado vai ao shell caractere a caractere; o eco dele
            // é que aparece na grade. A IDE não guarda linha de comando.
            ShellFocus::Terminal => {
                for caractere in text.chars() {
                    self.send_terminal_key(TerminalKey::Char(caractere));
                }
            }
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
        // Antes de tudo: com o `Ctrl` segurado, `Tab` é troca de aba e não
        // indentação. Precisa vir antes do roteamento por janela porque a
        // própria janela de abas é quem estaria aberta, e ela não recebe teclas.
        if modifiers.control && key.eq_ignore_ascii_case("tab") {
            self.cycle_tab();
            return;
        }
        // O caminho de volta do `Ctrl+clique`. Vem antes do editor porque as
        // setas são dele, e com `Ctrl+Alt` elas deixam de ser.
        if modifiers.control && modifiers.alt {
            if key.eq_ignore_ascii_case("arrowleft") {
                self.navigate_back();
                return;
            }
            if key.eq_ignore_ascii_case("arrowright") {
                self.navigate_forward();
                return;
            }
        }
        if self.context_menu_key(key, modifiers) {
            return;
        }
        // O teclado vai a quem tem o foco, e quem sabe disso é o anfitrião. Ele
        // só alcança um componente que esteja na pilha, então ela é declarada
        // antes: sem isso, a tecla chegaria numa janela que ainda não existe
        // para ele.
        self.place_overlay(self.context.last_size);

        // A janela aberta fica com as teclas — a menos que ela devolva o que não
        // é dela, como a inspeção faz quando o foco está na árvore.
        if let Some(surface) = self.open_surface()
            && self.surface_key(surface, key, modifiers)
        {
            return;
        }
        if self.completion_key(key) {
            return;
        }
        if key.eq_ignore_ascii_case("backspace") {
            match self.context.focus {
                ShellFocus::Editor => self.backspace(),
                ShellFocus::Search => {
                    self.editor_area.search_query.pop();
                    self.refresh_search_hits();
                }
                // Apagar é do shell: ele é quem sabe o que há na linha, e é
                // ele que redesenha. A IDE só entrega a tecla.
                ShellFocus::Terminal => self.send_terminal_key(TerminalKey::Backspace),
                _ => {}
            }
        } else if self.context.focus == ShellFocus::Search {
            // Na barra, `Enter` é o gesto inteiro: leva o cursor à próxima
            // ocorrência e dá a volta no fim do arquivo. Com `Shift`, à
            // anterior — o mesmo gesto ao contrário, e é a convenção de todo
            // editor. O resto do texto entra por `text_input`, e as setas
            // continuam sendo do editor.
            if key.eq_ignore_ascii_case("enter") {
                if modifiers.shift {
                    self.go_to_previous_search_hit();
                } else {
                    self.go_to_next_search_hit();
                }
            }
        } else if self.context.focus == ShellFocus::SearchTerminal {
            // A mesma convenção da caixa do editor, na janela do terminal:
            // `Enter` anda, `Shift+Enter` volta, e `Backspace` apaga.
            if key.eq_ignore_ascii_case("enter") {
                self.step_terminal_search(!modifiers.shift);
            } else if key.eq_ignore_ascii_case("backspace") {
                self.backspace_in_terminal_search();
            }
        } else if self.context.focus == ShellFocus::Terminal {
            // Seta, `Tab`, `Enter`, `Ctrl+C`: todas vão ao shell, e é ele quem
            // responde. Histórico e completação funcionam sem a IDE saber que
            // existem.
            if let Some(tecla) = tecla_do_terminal(key) {
                self.send_terminal_key(tecla);
            }
        } else if self.context.focus == ShellFocus::Editor {
            // Edição, seleção e movimento são do painel. O shell cuida do que
            // sobra: a lista de completação, a marca de modificado e as ações
            // que o painel não tem como executar.
            self.editor_area.completion_items.clear();
            let Some(document) = self.editor_area.session.active_mut() else {
                return;
            };
            let before = document.buffer.revision();
            // `Enter` logo depois de um abridor abre o bloco: linha em branco um
            // nível mais fundo, e o fechamento na linha dele. Fora desse caso a
            // tecla é do painel como sempre, e a linha nova só herda a
            // indentação da anterior.
            let abriu_bloco = key.eq_ignore_ascii_case("enter")
                && !modifiers.shift
                && !modifiers.control
                && self.editor_area.pane.enter_pairing(&mut document.buffer);
            let action = if abriu_bloco {
                EditorAction::None
            } else {
                self.editor_area.pane.key(
                    &mut document.buffer,
                    key,
                    modifiers.shift,
                    modifiers.control,
                )
            };
            if document.buffer.revision() != before {
                self.context.status_message = "Modified".to_owned();
            }
            self.handle_editor_action(action);
        }
    }

    fn paint_inspection(&self, commands: &mut Vec<PaintCommand>, size: Size) {
        let layout = self.layout_context();
        let mut paint = self.paint_context();
        let attached = self.debug_panel.view.attached;
        if self
            .inspection
            .paint(&self.host, &layout, &mut paint, size, attached)
        {
            commands.extend(paint.into_commands());
        }
    }
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

/// Entrega o clique ao anfitrião e devolve o que a fileira de abas decidiu.
///
/// Um clique é pressionar e soltar; a interface da IDE só encaminha o
/// pressionar, então os dois eventos vão juntos. O que volta é a ação do
/// componente — a identidade da aba, e não um texto para desmontar.
fn tab_action(host: &mut UiHost, point: Point) -> Option<WidgetAction> {
    host.click(point)
        .commands
        .into_iter()
        .find_map(|evento| match evento {
            ui_commands::CommandEvent::Action(action) => Some(action),
            _ => None,
        })
}

mod build;
mod context_menu;
mod debug_area;
mod documents;
mod editor_area;
mod explorer_area;
mod generate;
mod git;
pub use git::{
    BranchItem, CommitRow, GitEntry, GitFileState, GitLineChange, GitView, PAGINA_DO_HISTORICO,
};
mod inspection;
mod menu_bar;
mod new_item;
mod painting;
mod rename;
mod routing;
mod settings;
mod split;
mod surfaces;
mod tab_switcher;
mod terminal_area;
mod type_search;

#[cfg(test)]
mod tests;
