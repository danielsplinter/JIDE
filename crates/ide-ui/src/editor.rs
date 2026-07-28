//! Painel de edição de código reutilizável, com comportamentos configuráveis.
//!
//! O que a IDE acrescenta ao `CodeEditor` da biblioteca — cursor em bytes,
//! seleção, indentação de bloco, rolagem, navegação por `Ctrl+Click`, calha de
//! breakpoints — estava espalhado pelo shell e só existia para a janela
//! principal. Aqui vira um componente com área própria, que qualquer tela pode
//! abrir.
//!
//! O painel **não é dono do texto**. Ele recebe o buffer em cada operação, e
//! assim a janela principal continua editando o documento aberto enquanto uma
//! segunda tela edita um rascunho, sem cópia nem sincronização entre os dois.

use ide_text::TextBuffer;
use ui_api::{LayoutContext, PaintContext, Widget};
use ui_core::{Point, Rect, Size};
use ui_editor::{CodeEditor, LineDecoration, SyntaxSpan, TextRange as EditorRange};

/// Comportamentos que uma tela pode ligar ou desligar.
///
/// São configuração, e não implementações diferentes: duplicar o painel para
/// remover quatro comportamentos daria duas cópias que divergiriam na primeira
/// correção feita em uma delas.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EditorCapabilities {
    /// `Ctrl+Click` pede a definição do símbolo sob o ponteiro.
    pub navigation: bool,
    /// A calha aceita clique para marcar e desmarcar breakpoints.
    pub breakpoint_gutter: bool,
    /// `Ctrl+S` grava o conteúdo.
    pub save: bool,
    /// O clique secundário abre o menu de contexto.
    pub context_menu: bool,
}

impl EditorCapabilities {
    /// Tudo ligado: é o editor da janela principal.
    #[must_use]
    pub const fn full() -> Self {
        Self {
            navigation: true,
            breakpoint_gutter: true,
            save: true,
            context_menu: true,
        }
    }

    /// Só a edição de texto.
    ///
    /// Serve a telas onde o conteúdo não é um arquivo do projeto: não há o que
    /// salvar, nem definição para navegar, nem linha onde parar a execução.
    #[must_use]
    pub const fn plain() -> Self {
        Self {
            navigation: false,
            breakpoint_gutter: false,
            save: false,
            context_menu: false,
        }
    }
}

impl Default for EditorCapabilities {
    fn default() -> Self {
        Self::full()
    }
}

/// O que o painel pede à tela que o hospeda.
///
/// O painel edita texto; ele não sabe resolver uma definição, gravar um arquivo
/// nem abrir um menu. Quando o gesto significa uma dessas coisas, ele diz — e
/// quem tem os meios decide o que fazer.
#[derive(Clone, Debug, PartialEq)]
pub enum EditorAction {
    None,
    /// `Ctrl+Click` sobre o deslocamento informado.
    Navigate(usize),
    /// Clique na calha, na linha informada.
    ToggleBreakpoint(usize),
    Save,
    /// Clique secundário no ponto informado.
    ContextMenu(Point),
}

/// Painel de edição com área própria.
#[derive(Clone, Debug)]
pub struct EditorPane {
    capabilities: EditorCapabilities,
    bounds: Rect,
    /// Cursor em bytes, que é como a IDE conta o texto.
    cursor: usize,
    /// Âncora e foco da seleção, em bytes.
    selection: Option<(usize, usize)>,
    selecting: bool,
    scroll_line: usize,
    /// Editor da biblioteca usado para desenhar, com a revisão que ele reflete.
    view: Option<(u64, CodeEditor)>,
    pending_reveal: Option<usize>,
}

impl EditorPane {
    #[must_use]
    pub fn new(capabilities: EditorCapabilities) -> Self {
        Self {
            capabilities,
            bounds: Rect::default(),
            cursor: 0,
            selection: None,
            selecting: false,
            scroll_line: 0,
            view: None,
            pending_reveal: None,
        }
    }

    #[must_use]
    pub const fn capabilities(&self) -> EditorCapabilities {
        self.capabilities
    }

    /// Área que o painel ocupa na tela.
    ///
    /// É o retângulo dele, e não o tamanho da janela: sem isso o painel
    /// precisaria conhecer a barra lateral e o terminal para saber onde está, e
    /// não poderia ser aberto em outro lugar.
    pub const fn set_bounds(&mut self, bounds: Rect) {
        self.bounds = bounds;
    }

    #[must_use]
    pub const fn bounds(&self) -> Rect {
        self.bounds
    }

    #[must_use]
    pub const fn cursor(&self) -> usize {
        self.cursor
    }

    pub const fn set_cursor(&mut self, cursor: usize) {
        self.cursor = cursor;
        self.selection = None;
    }

    #[must_use]
    pub const fn scroll_line(&self) -> usize {
        self.scroll_line
    }

    pub const fn set_scroll_line(&mut self, line: usize) {
        self.scroll_line = line;
    }

    pub const fn reveal_line(&mut self, line: usize) {
        self.pending_reveal = Some(line);
    }

    /// Intervalo selecionado, em ordem crescente.
    #[must_use]
    pub fn selection_range(&self) -> Option<std::ops::Range<usize>> {
        let (anchor, focus) = self.selection?;
        let (start, end) = if anchor <= focus {
            (anchor, focus)
        } else {
            (focus, anchor)
        };
        (start < end).then_some(start..end)
    }

    pub const fn set_selection(&mut self, selection: Option<(usize, usize)>) {
        self.selection = selection;
    }

    /// Texto selecionado, se houver.
    #[must_use]
    pub fn selected_text<'a>(&self, buffer: &'a TextBuffer) -> Option<&'a str> {
        self.selection_range()
            .and_then(|range| buffer.text().get(range))
    }

    /// Linhas visíveis na altura atual do painel.
    #[must_use]
    pub fn visible_lines(&self) -> usize {
        (self.bounds.size.height / CodeEditor::line_height())
            .floor()
            .max(1.0) as usize
    }

    /// Deslocamento do texto sob um ponto da tela.
    fn offset_at(&self, buffer: &TextBuffer, point: Point) -> usize {
        let text = buffer.text();
        let line_index = self.scroll_line
            + ((point.y - self.bounds.origin.y) / CodeEditor::line_height())
                .floor()
                .max(0.0) as usize;
        let column = ((point.x - self.bounds.origin.x - CodeEditor::gutter_width())
            / CodeEditor::default_char_width())
        .round()
        .max(0.0) as usize;
        let mut offset = 0;
        for (index, line) in text.split('\n').enumerate() {
            if index == line_index {
                return offset + byte_at_column(line, column);
            }
            offset += line.len() + 1;
        }
        text.len()
    }

    /// Linha da calha sob um ponto, quando a calha está ligada.
    fn gutter_line_at(&self, point: Point) -> Option<usize> {
        if !self.capabilities.breakpoint_gutter || !self.bounds.contains(point) {
            return None;
        }
        (point.x < self.bounds.origin.x + CodeEditor::gutter_width()).then(|| {
            self.scroll_line
                + ((point.y - self.bounds.origin.y) / CodeEditor::line_height())
                    .floor()
                    .max(0.0) as usize
        })
    }

    /// Clique primário: posiciona o cursor e começa uma seleção.
    pub fn pointer_down(
        &mut self,
        buffer: &TextBuffer,
        point: Point,
        control: bool,
    ) -> EditorAction {
        if !self.bounds.contains(point) {
            return EditorAction::None;
        }
        // A calha fala sobre a linha inteira, e não sobre uma posição no texto.
        if let Some(line) = self.gutter_line_at(point) {
            return EditorAction::ToggleBreakpoint(line);
        }
        let offset = self.offset_at(buffer, point);
        self.cursor = offset;
        if control && self.capabilities.navigation {
            return EditorAction::Navigate(offset);
        }
        // Pressionar fixa a âncora; o movimento seguinte decide se virou seleção.
        self.selection = Some((offset, offset));
        self.selecting = true;
        EditorAction::None
    }

    /// Clique secundário: pede o menu, quando a tela oferece um.
    pub fn secondary_pointer_down(&mut self, point: Point) -> EditorAction {
        if !self.capabilities.context_menu || !self.bounds.contains(point) {
            return EditorAction::None;
        }
        EditorAction::ContextMenu(point)
    }

    /// Arraste: estende a seleção enquanto o botão está pressionado.
    pub fn pointer_move(&mut self, buffer: &TextBuffer, point: Point) -> bool {
        if !self.selecting {
            return false;
        }
        let focus = self.offset_at(buffer, point);
        self.cursor = focus;
        if let Some((anchor, _)) = self.selection {
            self.selection = Some((anchor, focus));
        }
        true
    }

    pub fn pointer_up(&mut self) {
        self.selecting = false;
        // Pressionar e soltar no mesmo ponto é um clique, não uma seleção vazia.
        if self.selection.is_some_and(|(anchor, focus)| anchor == focus) {
            self.selection = None;
        }
    }

    /// Duplo clique: seleciona a palavra sob o ponteiro.
    ///
    /// A regra do que é palavra é do editor da biblioteca, que tem o texto — o
    /// painel só converte entre bytes e caracteres.
    pub fn select_word_at(&mut self, buffer: &TextBuffer, point: Point) {
        if !self.bounds.contains(point) {
            return;
        }
        let text = buffer.text();
        let offset = self.offset_at(buffer, point);
        let mut editor = CodeEditor::new(SCRATCH_VIEW_ID, text);
        editor.select_word_at(chars_before(text, offset));
        let Some(word) = editor.selection() else {
            return;
        };
        let (start, end) = (byte_at_char(text, word.start), byte_at_char(text, word.end));
        self.cursor = end;
        self.selection = Some((start, end));
        self.selecting = false;
    }

    /// Texto digitado entra no cursor, substituindo o que estiver marcado.
    pub fn insert(&mut self, buffer: &mut TextBuffer, text: &str) -> bool {
        self.delete_selection(buffer);
        let cursor = self.cursor.min(buffer.text().len());
        if buffer.replace(cursor..cursor, text).is_ok() {
            self.cursor = cursor + text.len();
            return true;
        }
        false
    }

    /// Apaga o trecho marcado. Devolve `true` quando havia algo para apagar.
    pub fn delete_selection(&mut self, buffer: &mut TextBuffer) -> bool {
        let Some(range) = self.selection_range() else {
            return false;
        };
        self.selection = None;
        let start = range.start;
        if buffer.replace(range, "").is_ok() {
            self.cursor = start;
            return true;
        }
        false
    }

    /// Tecla de edição ou navegação. Devolve o que a tela precisa fazer.
    pub fn key(
        &mut self,
        buffer: &mut TextBuffer,
        key: &str,
        shift: bool,
        control: bool,
    ) -> EditorAction {
        if control && self.capabilities.save && key.eq_ignore_ascii_case("s") {
            return EditorAction::Save;
        }
        match key.to_ascii_lowercase().as_str() {
            "backspace" => {
                if !self.delete_selection(buffer) {
                    let previous = previous_boundary(buffer.text(), self.cursor);
                    if previous < self.cursor
                        && buffer.replace(previous..self.cursor, "").is_ok()
                    {
                        self.cursor = previous;
                    }
                }
            }
            "enter" => {
                self.insert(buffer, "\n");
            }
            // Com um trecho marcado, Tab desloca o bloco inteiro.
            "tab" if self.selection_range().is_some() => self.shift_lines(buffer, !shift),
            "tab" if shift => self.unindent(buffer),
            "tab" => self.indent(buffer),
            "arrowleft" => {
                let target = previous_boundary(buffer.text(), self.cursor);
                self.move_cursor(target, shift);
            }
            "arrowright" => {
                let target = next_boundary(buffer.text(), self.cursor);
                self.move_cursor(target, shift);
            }
            "arrowup" => self.move_line(buffer, -1, shift),
            "arrowdown" => self.move_line(buffer, 1, shift),
            _ => {}
        }
        EditorAction::None
    }

    /// Move o cursor, estendendo a seleção quando `selecting`.
    fn move_cursor(&mut self, target: usize, selecting: bool) {
        if selecting {
            let anchor = self.selection.map_or(self.cursor, |(anchor, _)| anchor);
            self.selection = Some((anchor, target));
        } else {
            self.selection = None;
        }
        self.cursor = target;
    }

    /// Move uma linha acima ou abaixo, preservando a coluna.
    fn move_line(&mut self, buffer: &TextBuffer, delta: isize, selecting: bool) {
        let text = buffer.text().to_owned();
        let (line, column) = line_column(&text, self.cursor);
        let lines = text.lines().count().max(1);
        let Some(target_line) = line
            .checked_add_signed(delta)
            .filter(|target| *target < lines)
        else {
            return;
        };
        let target = offset_for_line_column(&text, target_line, column);
        self.move_cursor(target, selecting);
    }

    /// Avança até a próxima parada de tabulação, escrevendo espaços.
    fn indent(&mut self, buffer: &mut TextBuffer) {
        let (_, column) = line_column(buffer.text(), self.cursor);
        let spaces = INDENT_WIDTH - column % INDENT_WIDTH;
        self.insert(buffer, &" ".repeat(spaces));
    }

    /// Recolhe a indentação da linha, e não o que está antes do cursor.
    fn unindent(&mut self, buffer: &mut TextBuffer) {
        let text = buffer.text();
        let cursor = self.cursor.min(text.len());
        let line_start = text[..cursor].rfind('\n').map_or(0, |index| index + 1);
        let removable = text[line_start..]
            .chars()
            .take(INDENT_WIDTH)
            .take_while(|value| *value == ' ')
            .count();
        if removable == 0 {
            return;
        }
        if buffer.replace(line_start..line_start + removable, "").is_ok() {
            self.cursor = cursor.saturating_sub(removable).max(line_start);
        }
    }

    /// Desloca as linhas tocadas pela seleção, para dentro ou para fora.
    ///
    /// A regra é do editor da biblioteca: o painel converte os deslocamentos e
    /// devolve o texto resultante ao buffer.
    fn shift_lines(&mut self, buffer: &mut TextBuffer, indent: bool) {
        let Some(range) = self.selection_range() else {
            return;
        };
        let text = buffer.text().to_owned();
        let mut editor = CodeEditor::new(SCRATCH_VIEW_ID, &text);
        editor.set_selection(Some(EditorRange::new(
            chars_before(&text, range.start),
            chars_before(&text, range.end),
        )));
        editor.shift_selected_lines(indent);
        let updated = editor.buffer().to_text();
        let shifted = editor.selection();
        if buffer.replace(0..text.len(), &updated).is_err() {
            return;
        }
        match shifted {
            Some(range) => {
                let start = byte_at_char(&updated, range.start);
                let end = byte_at_char(&updated, range.end);
                self.selection = Some((start, end));
                self.cursor = end;
            }
            None => self.selection = None,
        }
    }

    /// Reconstrói o editor de desenho a partir do buffer e do realce atual.
    ///
    /// A view só é refeita quando a revisão do buffer muda: remontá-la a cada
    /// quadro jogaria fora a rolagem e o trabalho de medir o texto.
    pub fn sync(
        &mut self,
        context: &LayoutContext,
        buffer: &TextBuffer,
        syntax: &[(usize, usize, ui_editor::TokenKind)],
        decorations: Vec<LineDecoration>,
        focused: bool,
    ) {
        let text = buffer.text();
        let revision = buffer.revision();
        let stale = !matches!(&self.view, Some((seen, _)) if *seen == revision);
        if stale {
            self.view = Some((revision, CodeEditor::new(EDITOR_VIEW_ID, text)));
        }
        // A IDE conta bytes e o editor conta caracteres: sem converter, o cursor
        // sairia do lugar no primeiro acento do arquivo.
        let cursor = chars_before(text, self.cursor);
        let selection = self.selection_range().map(|range| {
            EditorRange::new(chars_before(text, range.start), chars_before(text, range.end))
        });
        let spans: Vec<SyntaxSpan> = syntax
            .iter()
            .map(|(start, end, kind)| SyntaxSpan {
                range: EditorRange::new(*start, *end),
                token_kind: *kind,
            })
            .collect();
        let bounds = self.bounds;
        let scroll_line = self.scroll_line;
        let reveal = self.pending_reveal.take();
        let Some((_, editor)) = self.view.as_mut() else {
            return;
        };
        editor.layout(context, bounds);
        editor.set_syntax(spans);
        editor.set_decorations(decorations);
        editor.set_focused(focused);
        editor.set_cursor(cursor);
        // Depois do cursor: `set_cursor` significa "cursor movido, sem seleção".
        editor.set_selection(selection);
        editor.set_scroll_line(scroll_line);
        if let Some(line) = reveal {
            editor.reveal_line(line);
        }
        self.scroll_line = editor.scroll_line();
    }

    /// Desenha o painel. Requer um [`EditorPane::sync`] no mesmo quadro.
    pub fn paint(&self, context: &mut PaintContext) {
        if let Some((_, editor)) = self.view.as_ref() {
            editor.paint(context);
        }
    }

    /// Tamanho que o conteúdo ocupa, para quem dimensiona barras de rolagem.
    #[must_use]
    pub fn content_size(&self, buffer: &TextBuffer) -> Size {
        let lines = buffer.text().lines().count().max(1);
        Size::new(
            self.bounds.size.width,
            lines as f32 * CodeEditor::line_height(),
        )
    }
}

/// Largura de uma parada de tabulação, em colunas.
const INDENT_WIDTH: usize = 4;
const EDITOR_VIEW_ID: ui_core::WidgetId = ui_core::WidgetId(10_024);
/// Editor de rascunho, criado para aplicar uma regra e descartado em seguida.
const SCRATCH_VIEW_ID: ui_core::WidgetId = ui_core::WidgetId(10_051);

fn chars_before(text: &str, offset: usize) -> usize {
    text.get(..offset.min(text.len()))
        .unwrap_or(text)
        .chars()
        .count()
}

fn byte_at_char(text: &str, chars: usize) -> usize {
    text.char_indices()
        .nth(chars)
        .map_or(text.len(), |(index, _)| index)
}

fn byte_at_column(text: &str, column: usize) -> usize {
    text.char_indices()
        .nth(column)
        .map_or(text.len(), |(index, _)| index)
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

fn previous_boundary(text: &str, offset: usize) -> usize {
    let offset = offset.min(text.len());
    text[..offset]
        .char_indices()
        .next_back()
        .map_or(0, |(index, _)| index)
}

fn next_boundary(text: &str, offset: usize) -> usize {
    let offset = offset.min(text.len());
    text[offset..]
        .char_indices()
        .nth(1)
        .map_or(text.len(), |(index, _)| offset + index)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pane(capabilities: EditorCapabilities) -> (EditorPane, TextBuffer) {
        let mut pane = EditorPane::new(capabilities);
        pane.set_bounds(Rect::new(0.0, 0.0, 600.0, 400.0));
        (pane, TextBuffer::new("um\ndois\ntres"))
    }

    /// Clicar posiciona o cursor pela coluna, e arrastar marca o trecho.
    #[test]
    fn the_pane_selects_by_dragging_inside_its_own_bounds() {
        let (mut pane, buffer) = pane(EditorCapabilities::plain());
        let column = |index: usize| {
            Point::new(
                CodeEditor::gutter_width() + index as f32 * CodeEditor::default_char_width(),
                CodeEditor::line_height() + 4.0,
            )
        };
        pane.pointer_down(&buffer, column(0), false);
        pane.pointer_move(&buffer, column(4));
        pane.pointer_up();
        // Segunda linha, do começo ao fim de `dois`.
        assert_eq!(pane.selection_range(), Some(3..7));
        assert_eq!(pane.selected_text(&buffer), Some("dois"));
    }

    /// Sem a capacidade de navegação, `Ctrl+Click` é um clique comum.
    #[test]
    fn navigation_only_answers_when_the_capability_is_on() {
        let (mut plain, buffer) = pane(EditorCapabilities::plain());
        let point = Point::new(CodeEditor::gutter_width() + 4.0, 4.0);
        assert_eq!(plain.pointer_down(&buffer, point, true), EditorAction::None);

        let (mut full, buffer) = pane(EditorCapabilities::full());
        assert!(matches!(
            full.pointer_down(&buffer, point, true),
            EditorAction::Navigate(_)
        ));
    }

    /// A calha só responde onde ela existe.
    #[test]
    fn the_gutter_only_answers_when_the_capability_is_on() {
        let point = Point::new(4.0, CodeEditor::line_height() + 4.0);
        let (mut plain, buffer) = pane(EditorCapabilities::plain());
        assert_eq!(plain.pointer_down(&buffer, point, false), EditorAction::None);

        let (mut full, buffer) = pane(EditorCapabilities::full());
        assert_eq!(
            full.pointer_down(&buffer, point, false),
            EditorAction::ToggleBreakpoint(1)
        );
    }

    /// `Ctrl+S` só pede gravação onde salvar faz sentido.
    #[test]
    fn saving_only_answers_when_the_capability_is_on() {
        let (mut plain, mut buffer) = pane(EditorCapabilities::plain());
        assert_eq!(plain.key(&mut buffer, "s", false, true), EditorAction::None);

        let (mut full, mut buffer) = pane(EditorCapabilities::full());
        assert_eq!(full.key(&mut buffer, "s", false, true), EditorAction::Save);
    }

    /// O menu de contexto é opcional como os demais.
    #[test]
    fn the_context_menu_only_answers_when_the_capability_is_on() {
        let point = Point::new(80.0, 10.0);
        let (mut plain, _) = pane(EditorCapabilities::plain());
        assert_eq!(plain.secondary_pointer_down(point), EditorAction::None);

        let (mut full, _) = pane(EditorCapabilities::full());
        assert_eq!(
            full.secondary_pointer_down(point),
            EditorAction::ContextMenu(point)
        );
    }

    /// Editar não depende de capacidade nenhuma: é o que o painel é.
    #[test]
    fn editing_works_with_every_capability_off() {
        let (mut pane, mut buffer) = pane(EditorCapabilities::plain());
        pane.set_cursor(2);
        pane.insert(&mut buffer, "!");
        assert_eq!(buffer.text(), "um!\ndois\ntres");

        pane.key(&mut buffer, "ArrowDown", false, false);
        assert_eq!(pane.cursor(), "um!\ndoi".len());

        pane.set_selection(Some((0, 3)));
        pane.key(&mut buffer, "Tab", false, false);
        assert_eq!(buffer.text(), "    um!\ndois\ntres");
    }
}
