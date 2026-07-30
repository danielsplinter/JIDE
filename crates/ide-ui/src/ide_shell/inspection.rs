//! A janela de inspeção: a árvore do objeto e o código a executar no quadro.
//!
//! Ela não conversa com o depurador — pede. Cada gesto devolve o que precisa ser
//! perguntado ao alvo, e o shell transforma isso em `DebugRequest`. É o que
//! permite a janela funcionar sem saber que existe uma sessão do outro lado.

use std::collections::HashSet;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use ide_workspace::TextBuffer;
use ui_api::{EventContext, LayoutContext, PaintContext, Widget};
use ui_components::{Button, IconTint, Label, ModalHost, TreeItem, TreeView};
use ui_core::{Point, Rect, Size, UiEvent, WidgetId};

use super::geometry::{InspectionGeometry, inspection_geometry};
use super::primary_pointer;
use crate::debugging::DebugVariableView;
use crate::editor::{EditorCapabilities, EditorPane};

const MODAL_ID: WidgetId = WidgetId(10_044);
const TREE_ID: WidgetId = WidgetId(10_045);
const CLOSE_ID: WidgetId = WidgetId(10_046);
const NAME_ID: WidgetId = WidgetId(10_047);
const TYPE_ID: WidgetId = WidgetId(10_048);
const VALUE_ID: WidgetId = WidgetId(10_049);
const EMPTY_ID: WidgetId = WidgetId(10_050);
const RUN_ID: WidgetId = WidgetId(10_052);
const SOURCE_CAPTION_ID: WidgetId = WidgetId(10_053);
const MESSAGE_ID: WidgetId = WidgetId(10_054);
const PANEL_SIZE: Size = Size::new(720.0, 420.0);
pub(super) const ROW_HEIGHT: f32 = 26.0;

/// O que a janela precisa perguntar ao alvo, ou contar na barra.
pub(super) enum InspectionRequest {
    /// Contar isto na barra de estado da janela principal.
    Status(String),
    /// Avaliar esta expressão no quadro atual.
    Evaluate(String),
    /// Pedir os campos deste caminho.
    Expand(String),
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum InspectionFocus {
    #[default]
    Tree,
    Source,
}

/// Instruções ainda por executar, uma de cada vez.
#[derive(Clone, Debug)]
struct InspectionRun {
    current: String,
    remaining: Vec<String>,
    position: usize,
    total: usize,
}

#[derive(Clone, Debug)]
struct InspectionNode {
    path: String,
    variable: DebugVariableView,
    children: Vec<InspectionNode>,
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

    fn find_mut(&mut self, path: &str) -> Option<&mut Self> {
        if self.path == path {
            return Some(self);
        }
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

/// O objeto aberto: a raiz, o que está expandido e o que está destacado.
struct InspectionView {
    expression: String,
    root: InspectionNode,
    expanded: HashSet<String>,
    selected: String,
}

pub(super) struct InspectionSurface {
    modal: ModalHost,
    view: Option<InspectionView>,
    tree: TreeView,
    close_button: Button,
    /// O mesmo painel do editor principal, com os comportamentos de arquivo
    /// desligados: ali não há arquivo, só a expressão a executar.
    editor: EditorPane,
    source: TextBuffer,
    run_button: Button,
    focus: InspectionFocus,
    message: Option<String>,
    run: Option<InspectionRun>,
}

impl Default for InspectionSurface {
    fn default() -> Self {
        Self {
            modal: ModalHost::new(MODAL_ID, "Inspecionar", PANEL_SIZE),
            view: None,
            tree: TreeView::new(TREE_ID, Vec::new()).with_row_height(ROW_HEIGHT),
            close_button: Button::new(CLOSE_ID, "Fechar").with_command("inspect.close"),
            editor: EditorPane::new(EditorCapabilities::plain()),
            source: TextBuffer::new(String::new()),
            run_button: Button::new(RUN_ID, "Executar").with_command("inspect.run"),
            focus: InspectionFocus::Tree,
            message: None,
            run: None,
        }
    }
}

impl InspectionSurface {
    #[must_use]
    pub(super) const fn is_open(&self) -> bool {
        self.modal.is_open()
    }

    /// Expressão que está sendo inspecionada.
    #[must_use]
    pub(super) fn expression(&self) -> Option<&str> {
        self.view.as_ref().map(|view| view.expression.as_str())
    }

    /// O painel de edição e o texto dele, para quem for editar.
    pub(super) fn editor_and_source(&mut self) -> (&mut EditorPane, &mut TextBuffer) {
        (&mut self.editor, &mut self.source)
    }

    /// O mesmo, para quem só precisa ler.
    #[must_use]
    pub(super) const fn editor_and_source_ref(&self) -> (&EditorPane, &TextBuffer) {
        (&self.editor, &self.source)
    }

    #[must_use]
    pub(super) fn source_text(&self) -> &str {
        self.source.text()
    }

    #[must_use]
    pub(super) fn is_source_focused(&self) -> bool {
        self.focus == InspectionFocus::Source
    }

    pub(super) fn focus_source(&mut self) {
        self.focus = InspectionFocus::Source;
    }

    /// Um resultado de avaliação chegou.
    ///
    /// Abrir a inspeção e executar código dentro dela chegam pelo mesmo evento,
    /// mas pedem coisas opostas: a primeira monta a árvore, a segunda **não pode
    /// desmontá-la**. Quem executa `m.setId(4L)` quer continuar olhando `m`, e
    /// não trocar a árvore pelo `void` que o método devolveu.
    pub(super) fn result(
        &mut self,
        expression: String,
        value: DebugVariableView,
        fields: Vec<DebugVariableView>,
    ) -> Vec<InspectionRequest> {
        if self
            .run
            .as_ref()
            .is_some_and(|run| run.current == expression)
        {
            return self.advance_run(&expression, &value.value);
        }
        // A mesma raiz chegando de novo é a releitura pedida depois de uma
        // execução: os valores mudam, o que estava aberto continua aberto.
        let same_root = self
            .view
            .as_ref()
            .is_some_and(|view| view.expression == expression);
        if same_root && self.modal.is_open() {
            return self.refresh(value, fields);
        }
        self.show(expression, value, fields);
        Vec::new()
    }

    /// Uma instrução terminou: manda a próxima, ou fecha a execução.
    fn advance_run(&mut self, expression: &str, value: &str) -> Vec<InspectionRequest> {
        let Some(run) = self.run.as_mut() else {
            return Vec::new();
        };
        let total = run.total;
        if run.remaining.is_empty() {
            self.run = None;
            // Com uma instrução só, o relato é o retorno dela; com várias, o que
            // interessa é que todas passaram e o que a última respondeu.
            self.message = Some(if total > 1 {
                format!("{total} instruções executadas — {expression} → {value}")
            } else {
                format!("{expression} → {value}")
            });
            return self.reload();
        }
        let next = run.remaining.remove(0);
        run.position += 1;
        run.current.clone_from(&next);
        let position = run.position;
        vec![
            InspectionRequest::Status(format!("Executando {next} ({position} de {total})")),
            InspectionRequest::Evaluate(next),
        ]
    }

    /// Pede ao depurador o valor atual da raiz da árvore.
    fn reload(&self) -> Vec<InspectionRequest> {
        self.view.as_ref().map_or_else(Vec::new, |view| {
            vec![InspectionRequest::Evaluate(view.expression.clone())]
        })
    }

    /// Troca os valores da árvore sem mexer no que está aberto nem no destacado.
    fn refresh(
        &mut self,
        value: DebugVariableView,
        fields: Vec<DebugVariableView>,
    ) -> Vec<InspectionRequest> {
        let Some(view) = self.view.as_mut() else {
            return Vec::new();
        };
        let expression = view.expression.clone();
        view.root.variable = value;
        view.root.loaded = true;
        view.root.children = fields
            .into_iter()
            .map(|field| InspectionNode::new(format!("{expression}.{}", field.name), field))
            .collect();
        // Só a raiz veio com campos; os níveis abertos abaixo dela precisam ser
        // relidos, ou mostrariam o valor de antes da execução.
        let deeper: Vec<String> = view
            .expanded
            .iter()
            .filter(|path| **path != expression)
            .cloned()
            .collect();
        self.sync_tree();
        deeper.into_iter().map(InspectionRequest::Expand).collect()
    }

    /// Abre a janela com o valor avaliado e seus campos.
    pub(super) fn show(
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
        self.modal.set_title(format!("Inspecionar — {expression}"));
        self.modal.open();
        self.view = Some(InspectionView {
            selected: expression.clone(),
            expression,
            root,
            expanded,
        });
        self.sync_tree();
    }

    /// Acrescenta os campos que o alvo revelou para um caminho.
    pub(super) fn add_fields(&mut self, path: &str, fields: Vec<DebugVariableView>) {
        let Some(view) = self.view.as_mut() else {
            return;
        };
        let Some(node) = view.root.find_mut(path) else {
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
            view.expanded.remove(path);
        }
        self.sync_tree();
    }

    /// Reconstrói os itens da árvore a partir dos nós carregados.
    fn sync_tree(&mut self) {
        let Some(view) = self.view.as_ref() else {
            return;
        };
        let roots = vec![items(&view.root)];
        let expanded: Vec<u64> = view.expanded.iter().map(|path| id(path)).collect();
        let selected = id(&view.selected);
        self.tree.set_roots(roots);
        self.tree.set_expanded(expanded);
        self.tree.set_selected(Some(selected));
    }

    /// Relata na janela o que a última execução respondeu.
    ///
    /// Enquanto a janela está aberta ela cobre a barra de estado, então é aqui
    /// que a resposta precisa aparecer.
    pub(super) fn set_message(&mut self, message: impl Into<String>) -> Vec<InspectionRequest> {
        let message = message.into();
        // A avaliação responde com o valor ou com este relato, nunca com os dois:
        // chegar aqui encerra a execução que estava em curso.
        let interrupted = self.run.take();
        if !self.modal.is_open() {
            return Vec::new();
        }
        let Some(run) = interrupted else {
            self.message = Some(message);
            return Vec::new();
        };
        // As instruções seguintes não rodam: cada uma esperava o estado que a
        // anterior deixaria, e ele não existe.
        self.message = Some(if run.total > 1 {
            format!(
                "parou na instrução {} de {}: {message}",
                run.position, run.total
            )
        } else {
            message
        });
        // O que rodou antes da falha teve efeito, e a árvore precisa mostrá-lo.
        // Só aqui: relê a cada relato qualquer e uma releitura que falhasse
        // pediria outra, sem fim.
        if run.position > 1 {
            return self.reload();
        }
        Vec::new()
    }

    pub(super) fn close(&mut self) {
        self.message = None;
        self.run = None;
        self.modal.close();
        self.view = None;
    }

    /// Executa o que está escrito no editor, no quadro atual.
    ///
    /// `attached` é a sessão viva: sem ela não há quadro onde executar, e dizer
    /// isso na janela é melhor do que mandar uma pergunta que ninguém responde.
    pub(super) fn run_source(&mut self, attached: bool) -> Vec<InspectionRequest> {
        if !attached {
            self.message =
                Some("A sessão de depuração terminou; reconecte para executar".to_owned());
            return Vec::new();
        }
        let mut statements = statements(self.source.text());
        if statements.is_empty() {
            return vec![InspectionRequest::Status(
                "Escreva a expressão a executar".to_owned(),
            )];
        }
        let total = statements.len();
        let first = statements.remove(0);
        let status = if total > 1 {
            format!("Executando {first} (1 de {total})")
        } else {
            format!("Executando {first}")
        };
        self.message = None;
        self.run = Some(InspectionRun {
            current: first.clone(),
            remaining: statements,
            position: 1,
            total,
        });
        vec![
            InspectionRequest::Status(status),
            InspectionRequest::Evaluate(first),
        ]
    }

    /// Tipo em execução de um nome que está na árvore.
    #[must_use]
    pub(super) fn type_of(&self, name: &str) -> Option<String> {
        let view = self.view.as_ref()?;
        if view.expression == name {
            return view.root.variable.type_name.clone();
        }
        view.root
            .children
            .iter()
            .find(|child| child.variable.name == name)
            .and_then(|field| field.variable.type_name.clone())
    }

    /// Digitação no editor da janela.
    pub(super) fn text_input(&mut self, text: &str) {
        self.editor.insert(&mut self.source, text);
    }

    /// Tecla no editor da janela, depois de o shell resolver o que é dele.
    pub(super) fn editor_key(&mut self, key: &str, shift: bool, control: bool) {
        self.editor.key(&mut self.source, key, shift, control);
    }

    /// Roteia o clique dentro da janela.
    pub(super) fn pointer_down(
        &mut self,
        context: &LayoutContext,
        point: Point,
        size: Size,
        attached: bool,
    ) -> Vec<InspectionRequest> {
        let geometry = self.layout_editor(context, size);
        if geometry.close.contains(point) {
            self.close();
            return Vec::new();
        }
        if geometry.run.contains(point) {
            return self.run_source(attached);
        }
        if geometry.source.contains(point) {
            // O editor cuida do próprio cursor e da própria seleção.
            self.editor.pointer_down(&self.source, point, false, false);
            self.focus = InspectionFocus::Source;
            return Vec::new();
        }
        self.focus = InspectionFocus::Tree;
        if !geometry.list.contains(point) {
            return Vec::new();
        }
        // Qual nó foi clicado é a árvore quem sabe: recuo, marcador de expansão e
        // rolagem são dela.
        let mut tree = self.tree.clone();
        tree.layout(context, geometry.list);
        tree.event(
            &mut EventContext::default(),
            &UiEvent::PointerDown(primary_pointer(point)),
        );
        let Some(selected) = tree.selected() else {
            return Vec::new();
        };
        let Some(view) = self.view.as_mut() else {
            return Vec::new();
        };
        let Some(path) = path_of(&view.root, selected) else {
            return Vec::new();
        };
        view.selected = path.clone();
        let expandable = view
            .root
            .find(&path)
            .is_some_and(|node| node.variable.expandable);
        let mut requests = Vec::new();
        if expandable {
            // Clicar em um valor com campos abre e fecha, como no Explorer.
            if !view.expanded.remove(&path) {
                view.expanded.insert(path.clone());
                let pending = view.root.find(&path).is_some_and(|node| !node.loaded);
                if pending {
                    // Os campos só são pedidos ao abrir: perguntar por tudo de
                    // uma vez percorreria o grafo inteiro do objeto.
                    requests.push(InspectionRequest::Expand(path));
                }
            }
        }
        self.sync_tree();
        requests
    }

    /// Põe o painel de edição na área que ele ocupa agora.
    ///
    /// O painel só sabe converter ponto em posição do texto depois de saber onde
    /// está, e a janela é centrada: a área muda com o tamanho da tela.
    pub(super) fn layout_editor(
        &mut self,
        context: &LayoutContext,
        size: Size,
    ) -> InspectionGeometry {
        self.modal
            .layout(context, Rect::new(0.0, 0.0, size.width, size.height));
        let geometry = inspection_geometry(self.modal.panel_bounds());
        self.editor.set_bounds(geometry.source);
        geometry
    }

    /// Desenha a janela. Devolve `false` quando não há nada aberto.
    pub(super) fn paint(
        &self,
        layout: &LayoutContext,
        paint: &mut PaintContext,
        size: Size,
        attached: bool,
    ) -> bool {
        let Some(view) = self.view.as_ref() else {
            return false;
        };
        let mut modal = self.modal.clone();
        modal.layout(layout, Rect::new(0.0, 0.0, size.width, size.height));
        let geometry = inspection_geometry(modal.panel_bounds());
        modal.paint(paint);

        let mut tree = self.tree.clone();
        tree.set_selected(Some(id(&view.selected)));
        tree.layout(layout, geometry.list);
        tree.paint(paint);

        let detail = geometry.detail;
        match self.selected_variable() {
            Some(entry) => {
                text(
                    layout,
                    paint,
                    NAME_ID,
                    &entry.name,
                    Point::new(detail.origin.x, detail.origin.y),
                    17.0,
                    IconTint::Text,
                );
                text(
                    layout,
                    paint,
                    TYPE_ID,
                    entry.type_name.as_deref().unwrap_or("tipo desconhecido"),
                    Point::new(detail.origin.x, detail.origin.y + 26.0),
                    13.0,
                    IconTint::Muted,
                );
                text(
                    layout,
                    paint,
                    VALUE_ID,
                    &entry.value,
                    Point::new(detail.origin.x, detail.origin.y + 56.0),
                    14.0,
                    IconTint::Text,
                );
            }
            None => text(
                layout,
                paint,
                EMPTY_ID,
                "Sem valor para mostrar",
                Point::new(detail.origin.x, detail.origin.y),
                14.0,
                IconTint::Muted,
            ),
        }

        text(
            layout,
            paint,
            SOURCE_CAPTION_ID,
            "Código a executar no quadro atual",
            Point::new(geometry.source.origin.x, geometry.source.origin.y - 16.0),
            13.0,
            IconTint::Muted,
        );
        let mut editor = self.editor.clone();
        editor.set_bounds(geometry.source);
        editor.sync(layout, &self.source, None, Vec::new(), true);
        editor.paint(paint);

        if let Some(message) = self.message.as_ref() {
            // A mensagem tem a linha inteira, acima dos botões: dividir a linha
            // com eles a fazia passar por baixo do Executar, ilegível justamente
            // quando é ela que explica por que o clique não fez nada.
            text(
                layout,
                paint,
                MESSAGE_ID,
                &super::clipped_message(message, geometry.message.size.width),
                geometry.message.origin,
                13.0,
                IconTint::Danger,
            );
        }
        let mut run = self.run_button.clone();
        // Sem sessão viva não há quadro onde executar: o botão apagado diz isso
        // antes do clique, em vez de a mensagem dizer depois.
        run.set_disabled(!attached);
        run.layout(layout, geometry.run);
        run.paint(paint);
        let mut close = self.close_button.clone();
        close.layout(layout, geometry.close);
        close.paint(paint);
        true
    }

    /// Troca o rascunho do editor, para os testes.
    #[cfg(test)]
    pub(super) fn set_source(&mut self, text: &str) {
        self.source = TextBuffer::new(text);
    }

    /// O que a janela está relatando, para os testes.
    #[cfg(test)]
    pub(super) fn message(&self) -> Option<&str> {
        self.message.as_deref()
    }

    /// Entrada destacada na árvore, que é a detalhada no painel direito.
    pub(super) fn selected_variable(&self) -> Option<&DebugVariableView> {
        let view = self.view.as_ref()?;
        view.root.find(&view.selected).map(|node| &node.variable)
    }
}

fn text(
    layout: &LayoutContext,
    paint: &mut PaintContext,
    widget_id: WidgetId,
    content: &str,
    origin: Point,
    font_size: f32,
    tone: IconTint,
) {
    let mut widget = Label::new(widget_id, content)
        .with_font_size(font_size)
        .with_tone(tone);
    widget.layout(layout, Rect::new(origin.x, origin.y, 0.0, 0.0));
    widget.paint(paint);
}

/// Rótulo de uma entrada na lista da inspeção.
fn variable_label(entry: &DebugVariableView) -> String {
    match entry.type_name.as_deref() {
        Some(type_name) => format!("{} = ({type_name}) {}", entry.name, entry.value),
        None => format!("{} = {}", entry.name, entry.value),
    }
}

/// Caminho do nó com aquela identidade, procurando em profundidade.
fn path_of(node: &InspectionNode, target: u64) -> Option<String> {
    if id(&node.path) == target {
        return Some(node.path.clone());
    }
    node.children
        .iter()
        .find_map(|child| path_of(child, target))
}

/// Identidade de um nó da árvore, derivada do caminho.
///
/// O caminho sobrevive à chegada de campos novos; um índice mudaria a cada
/// expansão, e a árvore perderia o que estava aberto.
fn id(path: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    path.hash(&mut hasher);
    hasher.finish()
}

/// Converte um nó carregado em item da biblioteca.
///
/// Um valor expansível ainda sem campos recebe um filho de espera: é o que faz a
/// árvore desenhar o triângulo antes de o alvo ter respondido, e é onde o clique
/// de expansão acontece.
fn items(node: &InspectionNode) -> TreeItem {
    let children = if node.children.is_empty() && node.variable.expandable && !node.loaded {
        vec![TreeItem::new(
            id(&format!("{}.\u{2026}", node.path)),
            "carregando…",
            Vec::new(),
        )]
    } else {
        node.children.iter().map(items).collect()
    };
    TreeItem::new(id(&node.path), variable_label(&node.variable), children)
}

/// Separa o texto do editor nas instruções que o compõem.
///
/// O `;` e a quebra de linha terminam uma instrução, mas **não dentro de aspas**:
/// `setNome("a; b")` é uma instrução só, e partir no ponto e vírgula do meio
/// entregaria duas metades sem sentido ao alvo.
pub(super) fn statements(source: &str) -> Vec<String> {
    let mut statements = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    let mut escaped = false;
    for character in source.chars() {
        if quoted {
            current.push(character);
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == '"' {
                quoted = false;
            }
            continue;
        }
        match character {
            '"' => {
                quoted = true;
                current.push(character);
            }
            ';' | '\n' | '\r' => {
                if !current.trim().is_empty() {
                    statements.push(current.trim().to_owned());
                }
                current.clear();
            }
            _ => current.push(character),
        }
    }
    if !current.trim().is_empty() {
        statements.push(current.trim().to_owned());
    }
    statements
}
