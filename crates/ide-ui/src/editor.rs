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

use std::collections::HashMap;

use ide_domain::{AccessorCandidate, AccessorKind, CompletionItem, DocumentId, SyntaxSnapshot};
use ide_workspace::{EditorSession, TextBuffer};
use ui_api::{LayoutContext, PaintContext, Widget};
use ui_components::Scrollbar;
use ui_core::{Point, Rect, Size};
use ui_editor::{
    CodeEditor, EditSnapshot, LineDecoration, SyntaxSpan, TextRange as EditorRange, UndoHistory,
    next_occurrence,
};

/// Estado da área central de edição.
pub(super) struct EditorAreaState {
    pub(super) session: EditorSession,
    pub(super) pane: EditorPane,
    pub(super) search_query: String,
    pub(super) navigated: Option<(usize, usize)>,
    pub(super) scrollbar: Scrollbar,
    /// Barra lateral, para as linhas que passam da área visível.
    pub(super) horizontal_scrollbar: Scrollbar,
    pub(super) syntax_snapshots: HashMap<DocumentId, SyntaxSnapshot>,
    /// Realce já convertido de linha/coluna para deslocamentos em caracteres.
    ///
    /// A conversão é proporcional ao documento e deve acontecer quando chega
    /// uma nova revisão, nunca durante cada quadro de pintura.
    pub(super) syntax_spans: HashMap<DocumentId, CachedSyntax>,
    pub(super) completion_items: Vec<CompletionItem>,
    pub(super) completion_selected: usize,
    /// Geração de acessores em curso, aberta pelo menu `Generate`.
    pub(super) generate: Option<GenerateState>,
    /// Acessor pedido cuja resposta da linguagem ainda não chegou.
    pub(super) generate_pending: Option<AccessorKind>,
    /// Construtor escolhido cuja fonte a linguagem ainda não devolveu.
    ///
    /// O construtor sai de **um** texto montado a partir do conjunto marcado, e
    /// não da soma de trechos por campo: só depois da escolha dá para pedi-lo.
    pub(super) constructor_pending: Option<ConstructorRequest>,
    /// Janela da geração de acessores.
    pub(super) generate_modal: ui_components::ModalHost,
}

/// Construtor escolhido, à espera do texto que a linguagem vai montar.
///
/// Guarda os campos marcados e onde o texto entra. Lista vazia é um construtor
/// **sem parâmetros**, e não "nada escolhido": é o que o usuário pede quando
/// abre a janela e confirma sem marcar nada.
pub(super) struct ConstructorRequest {
    pub(super) fields: Vec<String>,
    pub(super) insert_at: ide_domain::TextPosition,
}

/// O que a janela de geração mostra e o que o usuário marcou.
///
/// Os textos vêm prontos da linguagem; a tela só decide quais entram. É o que
/// mantém a IDE sem saber o que é um getter.
pub(super) struct GenerateState {
    pub(super) kind: AccessorKind,
    pub(super) candidates: Vec<AccessorCandidate>,
    pub(super) insert_at: ide_domain::TextPosition,
    /// Marcados, um por candidato gerável.
    pub(super) checked: Vec<bool>,
    /// A lista, mantida entre quadros.
    ///
    /// Recriá-la a cada pintura jogava fora a rolagem e a deixava sem receber
    /// evento nenhum — a barra não se movia e o clique não chegava.
    pub(super) list: ui_components::ComposedList,
}

pub(super) struct CachedSyntax {
    pub(super) version: u64,
    pub(super) spans: Vec<CachedSyntaxSpan>,
}

pub(super) type CachedSyntaxSpan = (usize, usize, ui_editor::TokenKind);

pub struct SyntaxView<'a> {
    pub(super) version: u64,
    pub(super) spans: &'a [CachedSyntaxSpan],
}

impl EditorAreaState {
    #[must_use]
    pub(super) const fn active_document(&self) -> Option<DocumentId> {
        self.session.active_id()
    }

    #[must_use]
    pub(super) fn active_text(&self) -> Option<&str> {
        self.session.active().map(|document| document.buffer.text())
    }
}

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
    /// Último ponto do ponteiro durante o arrasto.
    ///
    /// Guardado para que a rolagem continue com o ponteiro parado fora da
    /// borda: sem ele, só haveria passo quando chegasse um evento de
    /// movimento, e segurar não roleria nada.
    last_pointer: Option<Point>,
    /// Rolagem vertical, em pixels.
    ///
    /// Em pixels, e não em linhas: rolar de linha em linha faz o texto saltar
    /// a cada passo, e é isso que se sente como travado.
    scroll_offset: f32,
    /// Rolagem lateral, em pixels.
    scroll_x: f32,
    /// Cursor cuja coluna já foi trazida à vista.
    revealed_cursor: Option<usize>,
    /// Editor da biblioteca usado para desenhar, com o texto que ele reflete.
    ///
    /// A chave é a origem **e** a revisão. Só a revisão não bastava: todo buffer
    /// nasce na revisão zero, então abrir um segundo arquivo reaproveitava a view
    /// do primeiro — a aba trocava de nome e o conteúdo não.
    view: Option<(u64, u64, CodeEditor)>,
    /// Revisão do realce já instalada na view.
    syntax_key: Option<(u64, u64)>,
    /// Origem que o painel está editando agora.
    source: Option<u64>,
    pending_reveal: Option<usize>,
    /// Estados anteriores, para `Ctrl+Z`. A pilha e o teto são da biblioteca.
    history: UndoHistory,
    /// Edição em várias ocorrências, aberta pelo `Ctrl+D`.
    multi: Option<MultiEdit>,
}

/// Ocorrências marcadas juntas, editadas ao mesmo tempo.
///
/// A linha de base existe para o `Esc`: desfazer uma edição múltipla passo a
/// passo gastaria vários dos dez do histórico, e o que o usuário pede ao
/// desistir é voltar ao começo, não recuar uma tecla.
#[derive(Clone, Debug)]
struct MultiEdit {
    /// Trechos marcados, em ordem crescente, em bytes.
    ///
    /// **Não são seleção.** A seleção é uma só e responde a copiar, colar, `Tab`
    /// e `Backspace`; deixar as marcas se passarem por ela fazia cada um desses
    /// caminhos mirar a última ocorrência sem saber que havia outras. Aqui elas
    /// são apenas trechos realçados que a edição múltipla conhece.
    ranges: Vec<(usize, usize)>,
    /// Texto que originou a marcação, para achar as próximas ocorrências.
    ///
    /// Guardado porque a seleção deixa de existir assim que a marcação começa —
    /// e, depois da primeira tecla, o que está marcado já não é o que se procura.
    needle: String,
    /// Texto e cursor de antes da primeira alteração.
    baseline: EditSnapshot,
    /// Onde a edição acontece dentro de cada marca, em bytes a partir do início.
    ///
    /// É relativo, e não absoluto, porque a mesma alteração vale para todas as
    /// ocorrências: um deslocamento por marca deixaria as posições divergirem à
    /// primeira letra. Como todas recebem as mesmas teclas, o conteúdo delas é
    /// sempre igual, e um só deslocamento descreve as várias.
    caret: usize,
    /// Alguma tecla já foi digitada sobre as marcas.
    typed: bool,
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
            last_pointer: None,
            scroll_offset: 0.0,
            scroll_x: 0.0,
            revealed_cursor: None,
            view: None,
            syntax_key: None,
            source: None,
            pending_reveal: None,
            history: UndoHistory::default(),
            multi: None,
        }
    }

    /// Diz qual conteúdo o painel passa a editar.
    ///
    /// Trocar de origem joga fora a cópia de desenho, o desfazer e as marcas:
    /// todos falam do texto anterior. Um `Ctrl+Z` que sobrevivesse à troca
    /// escreveria o texto de um arquivo dentro de outro.
    ///
    /// Cursor e rolagem não são zerados: quem os posiciona é a tela — abrir uma
    /// definição põe o cursor na linha certa **antes** do primeiro desenho, e
    /// limpá-los aqui desfaria isso.
    pub fn set_source(&mut self, source: u64) {
        if self.source == Some(source) {
            return;
        }
        let primeira = self.source.is_none();
        self.source = Some(source);
        if primeira {
            return;
        }
        self.view = None;
        self.syntax_key = None;
        self.history.clear();
        self.multi = None;
        self.selection = None;
        self.selecting = false;
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
    pub fn scroll_line(&self) -> usize {
        (self.scroll_offset / CodeEditor::line_height()).round().max(0.0) as usize
    }

    /// Rolagem vertical em pixels, que é como ela é guardada.
    #[must_use]
    pub const fn scroll_offset(&self) -> f32 {
        self.scroll_offset
    }

    pub const fn set_scroll_offset(&mut self, scroll_offset: f32) {
        self.scroll_offset = scroll_offset;
    }

    /// Rolagem lateral, em pixels.
    ///
    /// O painel guarda o valor e o editor da biblioteca o aplica ao desenhar e
    /// ao converter clique em posição — é lá que a largura do caractere vive.
    #[must_use]
    pub fn scroll_x(&self) -> f32 {
        self.scroll_x
    }

    pub const fn set_scroll_x(&mut self, scroll_x: f32) {
        self.scroll_x = scroll_x;
    }

    /// Largura da linha mais comprida, em pixels.
    #[must_use]
    pub fn content_width(&self) -> f32 {
        self.view
            .as_ref()
            .map_or(0.0, |(_, _, view)| view.content_width())
    }

    pub fn set_scroll_line(&mut self, line: usize) {
        self.scroll_offset = line as f32 * CodeEditor::line_height();
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
    /// Deslocamento do texto sob um ponto da tela.
    #[must_use]
    pub fn offset_at_point(&self, buffer: &TextBuffer, point: Point) -> usize {
        let text = buffer.text();
        // Converter ponto em posição é do editor da biblioteca: é lá que a
        // largura do caractere é medida na fonte que vai desenhar, e é lá que a
        // rolagem lateral existe. Refazer a conta aqui com a largura estimada
        // errava mais a cada coluna — a alguns caracteres de distância do
        // clique já no meio de uma linha.
        if let Some((_, _, view)) = self.view.as_ref() {
            return Self::cursor_stop(text, byte_at_char(text, view.offset_at_point(point)), false);
        }
        // Antes do primeiro desenho não há view, e a estimativa é o que existe.
        let line_index = self.scroll_line()
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
            self.scroll_line()
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
        shift: bool,
    ) -> EditorAction {
        if !self.bounds.contains(point) {
            return EditorAction::None;
        }
        // A calha fala sobre a linha inteira, e não sobre uma posição no texto.
        if let Some(line) = self.gutter_line_at(point) {
            return EditorAction::ToggleBreakpoint(line);
        }
        let offset = self.offset_at_point(buffer, point);
        if control && self.capabilities.navigation {
            self.cursor = offset;
            return EditorAction::Navigate(offset);
        }
        // Com `Shift`, o clique **estende** do que já estava marcado até aqui: a
        // âncora é a que existe, ou o cursor de antes do clique. Fixar uma nova
        // apagaria a seleção que o usuário quer alargar.
        if shift {
            let anchor = self
                .selection
                .map_or(self.cursor, |(existente, _)| existente);
            self.cursor = offset;
            self.selection = Some((anchor, offset));
            self.selecting = true;
            return EditorAction::None;
        }
        self.cursor = offset;
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
        // Guardado para o relógio: o passo de rolagem é dado por tempo, e não
        // por movimento, senão parar o ponteiro fora da borda pararia a vista.
        self.last_pointer = Some(point);
        self.extend_to(buffer, point);
        true
    }

    /// Um passo de rolagem enquanto o arrasto continua fora da área visível.
    ///
    /// Chamado pelo relógio da janela, e não por evento de ponteiro: sem isso,
    /// segurar o botão parado além da borda não levaria a vista a lugar nenhum
    /// — seria preciso mexer o mouse para arrancar cada linha. Devolve se algo
    /// mudou, que é o que decide se vale redesenhar.
    pub fn drag_autoscroll(&mut self, buffer: &TextBuffer) -> bool {
        let Some(point) = self.last_pointer else {
            return false;
        };
        if !self.selecting || !self.autoscroll(point) {
            return false;
        }
        self.extend_to(buffer, point);
        true
    }

    /// Leva o cursor — e o fim da seleção — até o ponto do ponteiro.
    fn extend_to(&mut self, buffer: &TextBuffer, point: Point) {
        let focus = self.offset_at_point(buffer, point);
        self.cursor = focus;
        if let Some((anchor, _)) = self.selection {
            self.selection = Some((anchor, focus));
        }
    }

    /// Move a vista quando o arrasto passa da borda da área.
    ///
    /// O passo cresce com a distância além da borda, mas com teto: sem ele o
    /// ponteiro no canto da tela varreria o arquivo inteiro num piscar. Devolve
    /// se houve rolagem, isto é, se o ponteiro está mesmo fora.
    fn autoscroll(&mut self, point: Point) -> bool {
        let bounds = self.bounds;
        let linha = CodeEditor::line_height();
        let mut rolou = false;
        let acima = bounds.origin.y - point.y;
        let abaixo = point.y - (bounds.origin.y + bounds.size.height);
        if acima > 0.0 {
            self.scroll_offset = (self.scroll_offset - linha * ritmo(acima, linha)).max(0.0);
            rolou = true;
        } else if abaixo > 0.0 {
            self.scroll_offset += linha * ritmo(abaixo, linha);
            rolou = true;
        }
        let coluna = CodeEditor::default_char_width() * AUTOSCROLL_COLUMNS;
        let gutter = CodeEditor::gutter_width();
        let esquerda = bounds.origin.x + gutter - point.x;
        let direita = point.x - (bounds.origin.x + bounds.size.width);
        if esquerda > 0.0 {
            self.scroll_x = (self.scroll_x - coluna * ritmo(esquerda, coluna)).max(0.0);
            rolou = true;
        } else if direita > 0.0 {
            self.scroll_x += coluna * ritmo(direita, coluna);
            rolou = true;
        }
        // Os limites são do editor da biblioteca, que conhece o conteúdo; aqui
        // só o piso, para não pedir rolagem negativa.
        rolou
    }

    pub fn pointer_up(&mut self) {
        self.selecting = false;
        self.last_pointer = None;
        // Pressionar e soltar no mesmo ponto é um clique, não uma seleção vazia.
        if self
            .selection
            .is_some_and(|(anchor, focus)| anchor == focus)
        {
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
        let offset = self.offset_at_point(buffer, point);
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

    /// Limite da palavra vizinha, em bytes, para o salto com `Ctrl`.
    ///
    /// Como no duplo clique, quem sabe onde a palavra começa e acaba é o editor
    /// da biblioteca; aqui só se converte entre bytes e caracteres.
    fn word_boundary(&self, buffer: &TextBuffer, forward: bool) -> usize {
        let text = buffer.text();
        let editor = CodeEditor::new(SCRATCH_VIEW_ID, text);
        let from = chars_before(text, self.cursor);
        let target = if forward {
            editor.next_word(from)
        } else {
            editor.previous_word(from)
        };
        byte_at_char(text, target)
    }

    /// Estado atual, para guardar antes de mexer no texto.
    fn snapshot(&self, buffer: &TextBuffer) -> EditSnapshot {
        EditSnapshot {
            text: buffer.text().to_owned(),
            cursor: self.cursor,
            selection: self
                .selection_range()
                .map(|range| EditorRange::new(range.start, range.end)),
        }
    }

    /// Guarda o estado de antes da alteração que vem a seguir.
    fn remember(&mut self, buffer: &TextBuffer) {
        let snapshot = self.snapshot(buffer);
        self.history.record(snapshot);
    }

    /// Volta ao estado anterior. Devolve `true` quando havia o que desfazer.
    ///
    /// Desfazer encerra uma edição múltipla em curso: as marcas apontam para
    /// trechos do texto que acabou de deixar de existir.
    pub fn undo(&mut self, buffer: &mut TextBuffer) -> bool {
        let Some(previous) = self.history.undo() else {
            return false;
        };
        self.multi = None;
        let length = buffer.text().len();
        if buffer.replace(0..length, &previous.text).is_err() {
            return false;
        }
        self.cursor = previous.cursor.min(buffer.text().len());
        self.selection = previous.selection.map(|range| (range.start, range.end));
        true
    }

    #[must_use]
    pub fn undo_depth(&self) -> usize {
        self.history.depth()
    }

    /// Trechos marcados por `Ctrl+D`, em bytes.
    #[must_use]
    pub fn occurrences(&self) -> Vec<(usize, usize)> {
        self.multi
            .as_ref()
            .map(|multi| multi.ranges.clone())
            .unwrap_or_default()
    }

    /// Marca a próxima ocorrência do trecho selecionado.
    ///
    /// Sem seleção não há o que procurar: a primeira vez precisa de um trecho, e
    /// é o usuário quem diz qual. Repetir vai somando ocorrências.
    pub fn select_next_occurrence(&mut self, buffer: &TextBuffer) -> bool {
        let text = buffer.text();
        // A primeira marcação nasce da seleção; as seguintes, do texto guardado —
        // depois da primeira tecla o marcado já não é o que se procura.
        let mut multi = match self.multi.take() {
            Some(multi) => multi,
            None => {
                let Some(range) = self.selection_range() else {
                    return false;
                };
                let Some(needle) = text.get(range.clone()).map(str::to_owned) else {
                    return false;
                };
                // A edição começa onde o cursor está dentro do trecho marcado —
                // marcar não move o ponto de digitação para o fim da palavra.
                let caret = self.cursor.clamp(range.start, range.end) - range.start;
                MultiEdit {
                    ranges: vec![(range.start, range.end)],
                    needle,
                    caret,
                    baseline: self.snapshot(buffer),
                    typed: false,
                }
            }
        };
        let needle = multi.needle.clone();
        // A busca continua depois da última marca, e dá a volta no arquivo.
        let last = multi.ranges.iter().map(|(_, end)| *end).max().unwrap_or(0);
        let found = next_occurrence(text, &needle, chars_before(text, last));
        let Some(found) = found else {
            self.multi = Some(multi);
            return false;
        };
        let novo = (
            byte_at_char(text, found.start),
            byte_at_char(text, found.end),
        );
        if multi.ranges.contains(&novo) {
            // Deu a volta inteira: não há ocorrência nova a acrescentar.
            self.multi = Some(multi);
            return false;
        }
        multi.ranges.push(novo);
        multi.ranges.sort_unstable();
        self.cursor = novo.1;
        // A marcação não é seleção: deixá-la se passar por uma faria copiar,
        // colar e `Tab` mirarem a última ocorrência ignorando as outras.
        self.selection = None;
        self.multi = Some(multi);
        true
    }

    /// Confirma a edição múltipla, mantendo o texto como está.
    pub fn confirm_occurrences(&mut self) -> bool {
        if self.multi.take().is_none() {
            return false;
        }
        self.selection = None;
        true
    }

    /// Desiste da edição múltipla e devolve o texto ao que era antes dela.
    pub fn cancel_occurrences(&mut self, buffer: &mut TextBuffer) -> bool {
        let Some(multi) = self.multi.take() else {
            return false;
        };
        let length = buffer.text().len();
        if buffer.replace(0..length, &multi.baseline.text).is_ok() {
            self.cursor = multi.baseline.cursor.min(buffer.text().len());
        }
        self.selection = None;
        true
    }

    /// Escreve `text` no fim de cada trecho marcado, do fim para o começo.
    ///
    /// Do fim para o começo porque escrever num trecho move tudo o que vem
    /// depois dele: indo na ordem contrária, cada escrita invalidaria os
    /// deslocamentos das seguintes.
    ///
    /// Cada marca é um **cursor**, e não uma seleção: o texto que já estava lá
    /// permanece, e o que se digita é acrescentado em todas as ocorrências, letra
    /// a letra. Substituir o trecho na primeira tecla apagaria de uma vez o que o
    /// usuário quer apenas alterar.
    fn replace_occurrences(&mut self, buffer: &mut TextBuffer, text: &str) -> bool {
        let Some(multi) = self.multi.as_mut() else {
            return false;
        };
        let caret = multi.caret;
        for (start, end) in multi.ranges.iter().rev() {
            let ponto = (*start + caret).min(*end);
            if buffer.replace(ponto..ponto, text).is_err() {
                return false;
            }
        }
        // A marca cresce para conter o que foi digitado, deslocada pelo que as
        // anteriores acrescentaram.
        let mut deslocamento: isize = 0;
        for range in &mut multi.ranges {
            let antes = range.1 - range.0;
            let start = (range.0 as isize + deslocamento).max(0) as usize;
            *range = (start, start + antes + text.len());
            deslocamento += text.len() as isize;
        }
        multi.caret = caret + text.len();
        multi.typed = true;
        self.cursor = multi
            .ranges
            .last()
            .map_or(self.cursor, |(start, _)| start + multi.caret);
        self.selection = None;
        true
    }

    /// Apaga um caractere no fim de cada trecho marcado.
    ///
    /// Um caractere de cada, e não o trecho inteiro: as marcas são cursores, e
    /// `Backspace` num cursor tira uma letra.
    fn backspace_occurrences(&mut self, buffer: &mut TextBuffer) -> bool {
        let Some(multi) = self.multi.as_mut() else {
            return false;
        };
        // Do fim para o começo, pelo mesmo motivo da escrita.
        let caret = multi.caret;
        let mut removidos: Vec<usize> = Vec::with_capacity(multi.ranges.len());
        for (start, end) in multi.ranges.iter().rev() {
            let ponto = (*start + caret).min(*end);
            let previous = previous_boundary(buffer.text(), ponto);
            let removivel = if previous >= *start {
                ponto - previous
            } else {
                0
            };
            if removivel > 0 && buffer.replace(previous..ponto, "").is_err() {
                return false;
            }
            removidos.push(removivel);
        }
        removidos.reverse();
        let removido = removidos.first().copied().unwrap_or_default();
        let mut deslocamento: isize = 0;
        for (range, removido) in multi.ranges.iter_mut().zip(removidos) {
            let antes = range.1 - range.0;
            let start = (range.0 as isize + deslocamento).max(0) as usize;
            *range = (start, start + antes - removido);
            deslocamento -= removido as isize;
        }
        multi.caret = caret.saturating_sub(removido);
        self.cursor = multi
            .ranges
            .last()
            .map_or(self.cursor, |(start, _)| start + multi.caret);
        self.selection = None;
        true
    }

    /// Move o ponto de edição dentro das marcas, sem sair delas.
    ///
    /// As setas durante a marcação movem **todas** as ocorrências juntas: é a
    /// mesma alteração acontecendo em vários lugares, e um ponto por marca faria
    /// as posições divergirem. Sair da marca encerraria a promessa, então o
    /// movimento para nas bordas.
    fn move_occurrence_caret(&mut self, buffer: &TextBuffer, forward: bool) -> bool {
        let Some(multi) = self.multi.as_mut() else {
            return false;
        };
        let Some((start, end)) = multi.ranges.first().copied() else {
            return false;
        };
        let text = buffer.text();
        let ponto = (start + multi.caret).min(end);
        let alvo = if forward {
            next_boundary(text, ponto).min(end)
        } else {
            previous_boundary(text, ponto).max(start)
        };
        multi.caret = alvo - start;
        self.cursor = multi
            .ranges
            .last()
            .map_or(self.cursor, |(marca, _)| marca + multi.caret);
        true
    }

    /// Texto digitado entra no cursor, substituindo o que estiver marcado.
    pub fn insert(&mut self, buffer: &mut TextBuffer, text: &str) -> bool {
        self.remember(buffer);
        if self.multi.is_some() {
            return self.replace_occurrences(buffer, text);
        }
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
        if control && key.eq_ignore_ascii_case("z") {
            self.undo(buffer);
            return EditorAction::None;
        }
        if control && key.eq_ignore_ascii_case("d") {
            self.select_next_occurrence(buffer);
            return EditorAction::None;
        }
        // Com ocorrências marcadas, `Enter` confirma e `Esc` desiste — nenhum dos
        // dois escreve. Enquanto a edição múltipla está aberta é ela que dá
        // sentido a essas teclas.
        if self.multi.is_some() {
            match key.to_ascii_lowercase().as_str() {
                "enter" => {
                    self.confirm_occurrences();
                    return EditorAction::None;
                }
                "escape" => {
                    self.cancel_occurrences(buffer);
                    return EditorAction::None;
                }
                "backspace" => {
                    self.remember(buffer);
                    self.backspace_occurrences(buffer);
                    return EditorAction::None;
                }
                "arrowleft" => {
                    self.move_occurrence_caret(buffer, false);
                    return EditorAction::None;
                }
                "arrowright" => {
                    self.move_occurrence_caret(buffer, true);
                    return EditorAction::None;
                }
                _ => {}
            }
        }
        match key.to_ascii_lowercase().as_str() {
            "backspace" => {
                self.remember(buffer);
                if !self.delete_selection(buffer) {
                    let previous = previous_boundary(buffer.text(), self.cursor);
                    if previous < self.cursor && buffer.replace(previous..self.cursor, "").is_ok() {
                        self.cursor = previous;
                    }
                }
            }
            // A linha nova herda a indentação da que ficou para trás; a regra é
            // do editor da biblioteca, que também a aplica quando é ele quem
            // recebe a tecla.
            "enter" => {
                let text = buffer.text();
                let from = self
                    .selection_range()
                    .map_or(self.cursor, |range| range.start);
                let indentation = CodeEditor::line_indentation(text, chars_before(text, from));
                self.insert(buffer, &format!("\n{indentation}"));
            }
            // Com um trecho marcado, Tab desloca o bloco inteiro.
            "tab" if self.selection_range().is_some() => {
                self.remember(buffer);
                self.shift_lines(buffer, !shift);
            }
            "tab" if shift => {
                self.remember(buffer);
                self.unindent(buffer);
            }
            "tab" => {
                self.remember(buffer);
                self.indent(buffer);
            }
            // Com `Ctrl`, o salto é de palavra em palavra. Onde a palavra
            // começa e acaba é regra do editor da biblioteca, que é a mesma do
            // duplo clique — decidir isso aqui daria dois recortes do texto.
            "arrowleft" if control => {
                let target = self.word_boundary(buffer, false);
                self.move_cursor(target, shift);
            }
            "arrowright" if control => {
                let target = self.word_boundary(buffer, true);
                self.move_cursor(target, shift);
            }
            "arrowleft" => {
                let target = previous_boundary(buffer.text(), self.cursor);
                self.move_cursor(Self::cursor_stop(buffer.text(), target, false), shift);
            }
            "arrowright" => {
                let target = next_boundary(buffer.text(), self.cursor);
                self.move_cursor(Self::cursor_stop(buffer.text(), target, true), shift);
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

    /// Onde o cursor pode parar, dado onde ele foi mandado.
    ///
    /// Num arquivo CRLF o retorno de carro pertence ao fim de linha, e entre ele
    /// e a quebra não há posição: o que fosse digitado ali entraria **depois** do
    /// fim da linha, o fragmento desenhado passaria a ter um retorno no meio, e o
    /// shaper abre outra linha de layout com o que vem depois — o texto recém
    /// digitado aparecendo repetido sobre a linha de baixo.
    fn cursor_stop(text: &str, target: usize, forward: bool) -> usize {
        let bytes = text.as_bytes();
        if target > 0 && bytes.get(target) == Some(&b'\n') && bytes.get(target - 1) == Some(&b'\r')
        {
            // O fim de linha é um lugar só: quem vinha andando o atravessa
            // inteiro, e quem chega por clique para no fim do que se vê.
            return if forward { target + 1 } else { target - 1 };
        }
        target
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
        let target =
            Self::cursor_stop(&text, offset_for_line_column(&text, target_line, column), false);
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
        if buffer
            .replace(line_start..line_start + removable, "")
            .is_ok()
        {
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
        syntax: Option<SyntaxView<'_>>,
        decorations: Vec<LineDecoration>,
        focused: bool,
    ) {
        let text = buffer.text();
        let revision = buffer.revision();
        let origem = self.source.unwrap_or_default();
        let stale = !matches!(
            &self.view,
            Some((fonte, seen, _)) if *fonte == origem && *seen == revision
        );
        if stale {
            self.view = Some((origem, revision, CodeEditor::new(EDITOR_VIEW_ID, text)));
            self.syntax_key = None;
        }
        // A IDE conta bytes e o editor conta caracteres: sem converter, o cursor
        // sairia do lugar no primeiro acento do arquivo.
        let cursor = chars_before(text, self.cursor);
        let selection = self.selection_range().map(|range| {
            EditorRange::new(
                chars_before(text, range.start),
                chars_before(text, range.end),
            )
        });
        // As demais ocorrências marcadas também precisam ser vistas: são elas
        // que dizem onde a próxima tecla vai bater.
        let extra: Vec<EditorRange> = self
            .multi
            .iter()
            .flat_map(|multi| multi.ranges.iter())
            .map(|(start, end)| {
                EditorRange::new(chars_before(text, *start), chars_before(text, *end))
            })
            .collect();
        let bounds = self.bounds;
        let scroll_offset = self.scroll_offset;
        let scroll_x = self.scroll_x;
        // Rolar com a barra não move o cursor; digitar e andar com as setas
        // movem. É essa diferença que decide se a vista deve segui-lo.
        let revelar_cursor = self.revealed_cursor != Some(self.cursor);
        self.revealed_cursor = Some(self.cursor);
        let reveal = self.pending_reveal.take();
        let Some((_, _, editor)) = self.view.as_mut() else {
            return;
        };
        editor.layout(context, bounds);
        match syntax {
            Some(SyntaxView {
                version,
                spans: syntax,
            }) if self.syntax_key != Some((origem, version)) => {
                editor.set_syntax(
                    syntax
                        .iter()
                        .map(|(start, end, kind)| SyntaxSpan {
                            range: EditorRange::new(*start, *end),
                            token_kind: *kind,
                        })
                        .collect(),
                );
                self.syntax_key = Some((origem, version));
            }
            None if self.syntax_key.is_some() => {
                editor.set_syntax(Vec::new());
                self.syntax_key = None;
            }
            _ => {}
        }
        editor.set_decorations(decorations);
        editor.set_focused(focused);
        editor.set_cursor(cursor);
        // Depois do cursor: `set_cursor` significa "cursor movido, sem seleção".
        editor.set_selection(selection);
        editor.set_extra_selections(extra);
        editor.set_scroll_offset(scroll_offset);
        editor.set_scroll_x(scroll_x);
        if let Some(line) = reveal {
            editor.reveal_line(line);
        }
        // Só quando o cursor se moveu. Revelar a cada quadro desfaria qualquer
        // rolagem feita à mão: a barra levaria a vista para um lado e o cursor a
        // traria de volta no quadro seguinte, como se o arrasto não funcionasse.
        if revelar_cursor {
            editor.reveal_cursor_column();
        }
        self.scroll_offset = editor.scroll_offset();
        self.scroll_x = editor.scroll_x();
    }

    /// Desenha o painel. Requer um [`EditorPane::sync`] no mesmo quadro.
    pub fn paint(&self, context: &mut PaintContext) {
        if let Some((_, _, editor)) = self.view.as_ref() {
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
/// Colunas percorridas a cada passo do arrasto além da borda lateral.
const AUTOSCROLL_COLUMNS: f32 = 3.0;
/// Quantos passos, no máximo, um único tique do arrasto pode dar.
const AUTOSCROLL_MAX_STEPS: f32 = 4.0;

/// Quantos passos dar, dada a distância do ponteiro até a borda.
///
/// Sempre ao menos um, para que encostar já role, e no máximo
/// [`AUTOSCROLL_MAX_STEPS`], para que afastar o ponteiro acelere sem virar
/// salto.
fn ritmo(distancia: f32, unidade: f32) -> f32 {
    (1.0 + distancia / unidade.max(1.0)).clamp(1.0, AUTOSCROLL_MAX_STEPS)
}
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

    /// Num arquivo CRLF o cursor não para entre o retorno e a quebra.
    ///
    /// Era o que duplicava o texto: com um espaço no fim da linha, o cursor
    /// chegava depois do retorno, o caractere digitado entrava ali e o fragmento
    /// desenhado ficava com um retorno no meio — o shaper abre outra linha de
    /// layout com o que vem depois, e o que foi digitado aparecia repetido sobre
    /// a linha de baixo.
    #[test]
    fn the_cursor_never_stops_between_the_carriage_return_and_the_break() {
        let (mut pane, _) = pane(EditorCapabilities::plain());
        let mut buffer = TextBuffer::new("int x = 1; \r\nint y = 2;\r\n");

        // Fim do que se vê na primeira linha: logo antes do retorno.
        pane.set_cursor(11);
        pane.key(&mut buffer, "ArrowRight", false, false);
        assert_eq!(
            pane.cursor(),
            13,
            "a seta pula o fim de linha inteiro, e não para dentro dele"
        );
        pane.key(&mut buffer, "ArrowLeft", false, false);
        assert_eq!(pane.cursor(), 11, "e volta para o fim do que se vê");

        // Digitar aí escreve **antes** do fim de linha, que fica intacto.
        pane.insert(&mut buffer, "z");
        assert_eq!(buffer.text(), "int x = 1; z\r\nint y = 2;\r\n");
        for linha in buffer.text().split('\n') {
            assert!(
                !linha.trim_end_matches('\r').contains('\r'),
                "nenhum retorno no meio de uma linha: {linha:?}"
            );
        }

        // Descer e subir preservando a coluna também não para no fim de linha.
        pane.set_cursor(12);
        pane.key(&mut buffer, "ArrowDown", false, false);
        pane.key(&mut buffer, "ArrowUp", false, false);
        assert!(pane.cursor() <= 12);
    }

    /// O clique consulta a view, que mede a largura na fonte de verdade.
    ///
    /// O painel refazia a conta com a largura estimada, e o erro se acumula por
    /// caractere: no meio de uma linha o cursor caía vários caracteres longe do
    /// clique. Com a rolagem lateral, ele também precisava contar o quanto a
    /// linha já andou.
    #[test]
    fn the_click_asks_the_view_instead_of_estimating() {
        let (mut pane, _) = pane(EditorCapabilities::plain());
        let buffer = TextBuffer::new("a".repeat(200));
        pane.sync(&LayoutContext::default(), &buffer, None, Vec::new(), true);

        let coluna = |indice: usize| {
            Point::new(
                CodeEditor::gutter_width() + indice as f32 * CodeEditor::default_char_width(),
                4.0,
            )
        };
        assert_eq!(pane.offset_at_point(&buffer, coluna(30)), 30);

        // Com a linha rolada, o mesmo ponto da tela é outro caractere — e o
        // painel precisa saber disso, não só o editor.
        pane.set_scroll_x(CodeEditor::default_char_width() * 20.0);
        pane.sync(&LayoutContext::default(), &buffer, None, Vec::new(), true);
        assert_eq!(
            pane.offset_at_point(&buffer, coluna(30)),
            50,
            "o clique conta a rolagem lateral"
        );
    }

    /// `Shift` com as quatro setas marca a partir do cursor.
    ///
    /// A âncora é onde o cursor estava quando a primeira seta foi pressionada, e
    /// continua a mesma enquanto `Shift` segurar — é o que faz a seleção crescer
    /// em vez de recomeçar a cada tecla.
    #[test]
    fn shift_with_the_four_arrows_selects_from_the_cursor() {
        let (mut pane, _) = pane(EditorCapabilities::plain());
        let mut buffer = TextBuffer::new("um\ndois\ntres");

        // Direita: marca crescendo a partir do cursor.
        pane.set_cursor(0);
        pane.key(&mut buffer, "ArrowRight", true, false);
        pane.key(&mut buffer, "ArrowRight", true, false);
        assert_eq!(pane.selected_text(&buffer), Some("um"));

        // Baixo: alarga a partir da **mesma** âncora, atravessando a linha.
        pane.key(&mut buffer, "ArrowDown", true, false);
        assert_eq!(pane.selected_text(&buffer), Some("um\ndo"));

        // Cima: encolhe de volta, sem trocar a âncora.
        pane.key(&mut buffer, "ArrowUp", true, false);
        assert_eq!(pane.selected_text(&buffer), Some("um"));

        // Esquerda: continua da mesma origem.
        pane.key(&mut buffer, "ArrowLeft", true, false);
        assert_eq!(pane.selected_text(&buffer), Some("u"));

        // Sem `Shift`, mover desfaz a seleção.
        pane.key(&mut buffer, "ArrowRight", false, false);
        assert_eq!(pane.selection_range(), None);
    }

    /// `Shift+clique` marca do cursor até o ponto clicado.
    ///
    /// Fixar uma âncora nova apagaria a seleção que o usuário quer alargar — é
    /// justamente o gesto de estender que `Shift` pede.
    #[test]
    fn shift_click_extends_from_the_cursor_to_the_clicked_point() {
        let (mut pane, buffer) = pane(EditorCapabilities::plain());
        let coluna = |index: usize| {
            Point::new(
                CodeEditor::gutter_width() + index as f32 * CodeEditor::default_char_width(),
                4.0,
            )
        };
        // Cursor no começo, e `Shift+clique` na coluna 2 da primeira linha.
        pane.set_cursor(0);
        pane.pointer_down(&buffer, coluna(2), false, true);
        assert_eq!(pane.selection_range(), Some(0..2));
        assert_eq!(pane.selected_text(&buffer), Some("um"));

        // Outro `Shift+clique` alarga a partir da **mesma** âncora.
        pane.pointer_down(&buffer, coluna(1), false, true);
        assert_eq!(pane.selection_range(), Some(0..1));

        // Sem `Shift`, o clique recomeça.
        pane.pointer_down(&buffer, coluna(1), false, false);
        assert_eq!(pane.selection_range(), None);
        assert_eq!(pane.cursor(), 1);
    }

    /// Texto largo e comprido o bastante para rolar nas duas direções.
    fn buffer_grande() -> TextBuffer {
        TextBuffer::new(
            (0..200)
                .map(|linha| format!("linha {linha} {}", "x".repeat(200)))
                .collect::<Vec<_>>()
                .join("\n"),
        )
    }

    /// Arrastar além da borda leva a vista junto, nas quatro direções.
    ///
    /// Sem isso a seleção pararia no que está à mostra, e marcar além da tela
    /// exigiria soltar, rolar e recomeçar.
    #[test]
    fn dragging_past_the_edge_scrolls_towards_the_selection() {
        let (mut pane, _) = pane(EditorCapabilities::plain());
        let buffer = buffer_grande();
        let bounds = pane.bounds();

        // Começa uma seleção e arrasta para baixo, além da borda.
        pane.pointer_down(&buffer, Point::new(bounds.origin.x + 60.0, 4.0), false, false);
        pane.pointer_move(
            &buffer,
            Point::new(bounds.origin.x + 60.0, bounds.origin.y + bounds.size.height + 20.0),
        );
        assert!(pane.drag_autoscroll(&buffer));
        assert!(pane.scroll_offset() > 0.0, "arrastar para baixo desce a vista");

        // Para a direita, além da borda lateral.
        pane.pointer_move(
            &buffer,
            Point::new(bounds.origin.x + bounds.size.width + 20.0, 10.0),
        );
        assert!(pane.drag_autoscroll(&buffer));
        assert!(pane.scroll_x() > 0.0, "arrastar à direita anda de lado");

        // De volta para cima e para a esquerda, sem passar do começo.
        pane.pointer_move(&buffer, Point::new(bounds.origin.x - 20.0, bounds.origin.y - 20.0));
        for _ in 0..20 {
            pane.drag_autoscroll(&buffer);
        }
        assert_eq!(pane.scroll_offset(), 0.0, "não passa do topo");
        assert_eq!(pane.scroll_x(), 0.0, "nem da margem esquerda");

        // Sem arrasto em curso, mover o ponteiro não rola nada.
        pane.pointer_up();
        pane.pointer_move(
            &buffer,
            Point::new(bounds.origin.x + 60.0, bounds.origin.y + bounds.size.height + 20.0),
        );
        assert!(!pane.drag_autoscroll(&buffer));
        assert_eq!(pane.scroll_offset(), 0.0);
    }

    /// Com o ponteiro parado fora da borda, o relógio continua rolando.
    ///
    /// É o caso de quem segura o botão junto ao rodapé esperando a vista
    /// descer: sem tique, nada acontece até o mouse mexer de novo.
    #[test]
    fn holding_the_pointer_outside_keeps_scrolling_and_selecting() {
        let (mut pane, _) = pane(EditorCapabilities::plain());
        let buffer = buffer_grande();
        let bounds = pane.bounds();
        let fora = Point::new(
            bounds.origin.x + 60.0,
            bounds.origin.y + bounds.size.height + 20.0,
        );

        pane.pointer_down(&buffer, Point::new(bounds.origin.x + 60.0, 4.0), false, false);
        pane.pointer_move(&buffer, fora);
        // Um único movimento e, daí em diante, só o relógio.
        assert!(pane.drag_autoscroll(&buffer));
        let primeiro = pane.scroll_offset();
        let Some(inicial) = pane.selection_range() else {
            panic!("o arrasto marcou um trecho");
        };

        assert!(pane.drag_autoscroll(&buffer), "o tique seguinte também rola");
        assert!(
            pane.scroll_offset() > primeiro,
            "sem mexer o mouse, a vista continua descendo"
        );
        let Some(agora) = pane.selection_range() else {
            panic!("a seleção acompanha o tique");
        };
        assert_eq!(
            agora.start, inicial.start,
            "a âncora fica onde o gesto começou"
        );
        assert!(agora.end > inicial.end, "e o trecho marcado cresce");

        // Soltar encerra: o relógio segue batendo, mas não mexe mais na vista.
        pane.pointer_up();
        let parado = pane.scroll_offset();
        assert!(!pane.drag_autoscroll(&buffer));
        assert_eq!(pane.scroll_offset(), parado);
    }

    /// O passo cresce com a distância, mas não vira salto.
    #[test]
    fn the_step_grows_with_the_distance_up_to_a_ceiling() {
        let unidade = CodeEditor::line_height();
        assert_eq!(ritmo(0.0, unidade), 1.0, "encostar na borda já anda uma vez");
        assert!(ritmo(unidade, unidade) > 1.0, "mais longe, mais rápido");
        assert_eq!(
            ritmo(unidade * 1_000.0, unidade),
            AUTOSCROLL_MAX_STEPS,
            "o ponteiro no canto da tela não varre o arquivo"
        );
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
        pane.pointer_down(&buffer, column(0), false, false);
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
        assert_eq!(plain.pointer_down(&buffer, point, true, false), EditorAction::None);

        let (mut full, buffer) = pane(EditorCapabilities::full());
        assert!(matches!(
            full.pointer_down(&buffer, point, true, false),
            EditorAction::Navigate(_)
        ));
    }

    /// A calha só responde onde ela existe.
    #[test]
    fn the_gutter_only_answers_when_the_capability_is_on() {
        let point = Point::new(4.0, CodeEditor::line_height() + 4.0);
        let (mut plain, buffer) = pane(EditorCapabilities::plain());
        assert_eq!(
            plain.pointer_down(&buffer, point, false, false),
            EditorAction::None
        );

        let (mut full, buffer) = pane(EditorCapabilities::full());
        assert_eq!(
            full.pointer_down(&buffer, point, false, false),
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

    /// `Ctrl+Z` volta uma ação por vez, até o teto de dez.
    #[test]
    fn undo_walks_back_one_action_at_a_time_up_to_ten() {
        let (mut pane, _) = pane(EditorCapabilities::plain());
        let mut buffer = TextBuffer::new("");
        for letra in ["a", "b", "c"] {
            pane.insert(&mut buffer, letra);
        }
        assert_eq!(buffer.text(), "abc");

        pane.key(&mut buffer, "z", false, true);
        assert_eq!(buffer.text(), "ab");
        pane.key(&mut buffer, "z", false, true);
        assert_eq!(buffer.text(), "a");
        assert_eq!(pane.cursor(), 1, "o cursor volta junto com o texto");

        // Além do histórico, desfazer não faz nada — e não estraga o texto.
        pane.key(&mut buffer, "z", false, true);
        pane.key(&mut buffer, "z", false, true);
        assert_eq!(buffer.text(), "");

        // O teto vale: quinze alterações deixam dez passos.
        let mut longo = TextBuffer::new("");
        let mut outro = EditorPane::new(EditorCapabilities::plain());
        for _ in 0..15 {
            outro.insert(&mut longo, "x");
        }
        assert_eq!(outro.undo_depth(), 10);
    }

    /// `Ctrl+D` vai somando ocorrências do trecho marcado.
    #[test]
    fn control_d_adds_the_next_occurrence_of_the_selection() {
        let (mut pane, _) = pane(EditorCapabilities::plain());
        let buffer = TextBuffer::new("nome = nome + nome");
        pane.set_selection(Some((0, 4)));
        assert!(pane.select_next_occurrence(&buffer));
        assert_eq!(pane.occurrences(), vec![(0, 4), (7, 11)]);
        assert!(pane.select_next_occurrence(&buffer));
        assert_eq!(pane.occurrences(), vec![(0, 4), (7, 11), (14, 18)]);
        // Esgotadas as ocorrências, repetir não inventa marca nova.
        assert!(!pane.select_next_occurrence(&buffer));
        assert_eq!(pane.occurrences().len(), 3);

        // Sem seleção não há o que procurar.
        let mut solto = EditorPane::new(EditorCapabilities::plain());
        assert!(!solto.select_next_occurrence(&buffer));
    }

    /// Trocar de arquivo troca o que é desenhado.
    ///
    /// Todo buffer nasce na revisão zero. Com a cópia de desenho guardada só pela
    /// revisão, o segundo arquivo aberto reaproveitava a do primeiro: a aba
    /// trocava de nome e o conteúdo não.
    #[test]
    fn opening_another_document_replaces_what_is_drawn() {
        let (mut pane, _) = pane(EditorCapabilities::plain());
        let primeiro = TextBuffer::new("conteúdo do primeiro");
        let segundo = TextBuffer::new("conteúdo do segundo");
        assert_eq!(
            primeiro.revision(),
            segundo.revision(),
            "os dois nascem na mesma revisão — é o que expunha o defeito"
        );

        pane.set_source(1);
        pane.sync(&LayoutContext::default(), &primeiro, None, Vec::new(), true);
        let texto = |pane: &EditorPane| {
            pane.view
                .as_ref()
                .map(|(_, _, view)| view.buffer().to_text())
                .unwrap_or_default()
        };
        assert_eq!(texto(&pane), "conteúdo do primeiro");

        pane.set_source(2);
        pane.sync(&LayoutContext::default(), &segundo, None, Vec::new(), true);
        assert_eq!(texto(&pane), "conteúdo do segundo");
    }

    /// O desfazer não atravessa a troca de arquivo.
    ///
    /// Um `Ctrl+Z` que sobrevivesse à troca escreveria o texto de um arquivo
    /// dentro de outro — perda de dados, não incômodo.
    #[test]
    fn the_history_does_not_cross_from_one_document_to_another() {
        let (mut pane, _) = pane(EditorCapabilities::plain());
        let mut primeiro = TextBuffer::new("primeiro");
        pane.set_source(1);
        pane.insert(&mut primeiro, "!");
        assert_eq!(pane.undo_depth(), 1);

        pane.set_source(2);
        assert_eq!(
            pane.undo_depth(),
            0,
            "o histórico do outro arquivo não vale"
        );
        let mut segundo = TextBuffer::new("segundo");
        assert!(!pane.undo(&mut segundo));
        assert_eq!(segundo.text(), "segundo", "nada do outro arquivo vazou");
    }

    /// As ocorrências marcadas chegam ao editor que desenha.
    ///
    /// Sem isto, só a última ficaria visível, e o usuário não veria onde a
    /// próxima tecla vai bater.
    #[test]
    fn every_marked_occurrence_reaches_the_view() {
        let (mut pane, _) = pane(EditorCapabilities::plain());
        let buffer = TextBuffer::new("nome = nome + nome");
        pane.set_selection(Some((0, 4)));
        pane.select_next_occurrence(&buffer);
        pane.select_next_occurrence(&buffer);

        pane.sync(&LayoutContext::default(), &buffer, None, Vec::new(), true);
        let Some((_, _, view)) = pane.view.as_ref() else {
            panic!("a view precisa existir depois do sync");
        };
        let marcadas: Vec<(usize, usize)> = view
            .extra_selections()
            .iter()
            .map(|range| (range.start, range.end))
            .collect();
        assert_eq!(
            marcadas,
            vec![(0, 4), (7, 11), (14, 18)],
            "as três ocorrências precisam ser desenhadas"
        );
    }

    /// Cada marca é um cursor: o texto fica, e a alteração é replicada.
    ///
    /// Substituir o trecho na primeira tecla apagaria de uma vez o que o usuário
    /// quer apenas alterar — mudar uma letra não é trocar a palavra inteira.
    #[test]
    fn typing_replays_the_change_at_every_marked_occurrence() {
        let (mut pane, _) = pane(EditorCapabilities::plain());
        let mut buffer = TextBuffer::new("nome = nome + nome");
        // Cursor no fim do trecho, que é onde ele fica ao selecionar arrastando.
        pane.set_cursor(4);
        pane.set_selection(Some((0, 4)));
        pane.select_next_occurrence(&buffer);
        pane.select_next_occurrence(&buffer);

        pane.insert(&mut buffer, "s");
        assert_eq!(
            buffer.text(),
            "nomes = nomes + nomes",
            "o que estava lá fica"
        );
        assert_eq!(pane.occurrences(), vec![(0, 5), (8, 13), (16, 21)]);

        pane.insert(&mut buffer, "!");
        assert_eq!(buffer.text(), "nomes! = nomes! + nomes!");

        // O trecho marcado continua realçado, para se ver onde se está.
        pane.sync(&LayoutContext::default(), &buffer, None, Vec::new(), true);
        let Some((_, _, view)) = pane.view.as_ref() else {
            panic!("a view precisa existir depois do sync");
        };
        assert_eq!(view.extra_selections().len(), 3);

        // Apagar tira uma letra de cada, e não o trecho inteiro.
        pane.key(&mut buffer, "Backspace", false, false);
        assert_eq!(buffer.text(), "nomes = nomes + nomes");
        pane.key(&mut buffer, "Backspace", false, false);
        assert_eq!(buffer.text(), "nome = nome + nome");
    }

    /// A alteração acontece onde o cursor está, e não no fim da marca.
    ///
    /// Marcar não move o ponto de digitação: quem pôs o cursor no meio da palavra
    /// quer alterar ali, nas várias ocorrências.
    #[test]
    fn the_change_happens_where_the_cursor_is_inside_each_mark() {
        let (mut pane, _) = pane(EditorCapabilities::plain());
        let mut buffer = TextBuffer::new("nome = nome");
        // Cursor entre `no` e `me`, com a palavra marcada. O cursor vem antes
        // porque `set_cursor` significa "movido, sem seleção".
        pane.set_cursor(2);
        pane.set_selection(Some((0, 4)));
        pane.select_next_occurrence(&buffer);

        pane.insert(&mut buffer, "X");
        assert_eq!(
            buffer.text(),
            "noXme = noXme",
            "a letra entra onde o cursor estava, não no fim"
        );

        // As setas movem o ponto em todas as ocorrências juntas.
        pane.key(&mut buffer, "ArrowRight", false, false);
        pane.insert(&mut buffer, "Y");
        assert_eq!(buffer.text(), "noXmYe = noXmYe");

        pane.key(&mut buffer, "ArrowLeft", false, false);
        pane.key(&mut buffer, "Backspace", false, false);
        assert_eq!(buffer.text(), "noXYe = noXYe", "apagar segue o mesmo ponto");

        // O movimento para na borda da marca, sem escapar dela.
        for _ in 0..10 {
            pane.key(&mut buffer, "ArrowLeft", false, false);
        }
        pane.insert(&mut buffer, "Z");
        assert_eq!(buffer.text(), "ZnoXYe = ZnoXYe");
    }

    /// `Ctrl` com as setas laterais salta de palavra em palavra.
    ///
    /// Em bytes, que é como o painel guarda o cursor: com acento no caminho, um
    /// salto contado em caracteres pararia no meio de uma letra.
    #[test]
    fn control_with_the_side_arrows_jumps_between_words() {
        let (mut pane, _) = pane(EditorCapabilities::plain());
        let mut buffer = TextBuffer::new("int número = valor;");
        pane.set_cursor(0);

        pane.key(&mut buffer, "ArrowRight", false, true);
        assert_eq!(pane.cursor(), 3, "fim de `int`");
        pane.key(&mut buffer, "ArrowRight", false, true);
        assert_eq!(pane.cursor(), 11, "`número` tem sete bytes, e não seis");

        pane.key(&mut buffer, "ArrowLeft", false, true);
        assert_eq!(pane.cursor(), 4, "começo de `número`");

        // Com `Shift`, o salto marca em vez de só andar.
        pane.key(&mut buffer, "ArrowRight", true, true);
        assert_eq!(pane.selection_range(), Some(4..11));

        // Sem `Ctrl`, continua andando de caractere em caractere.
        pane.set_cursor(0);
        pane.key(&mut buffer, "ArrowRight", false, false);
        assert_eq!(pane.cursor(), 1);
    }

    /// As marcas não são seleção, e nenhum caminho de seleção as alcança.
    ///
    /// Deixá-las se passarem por uma fazia copiar, colar, `Tab` e `Backspace`
    /// mirarem a última ocorrência sem saber que havia outras.
    #[test]
    fn the_marks_are_not_a_selection() {
        let (mut pane, _) = pane(EditorCapabilities::plain());
        let mut buffer = TextBuffer::new("nome = nome");
        pane.set_cursor(4);
        pane.set_selection(Some((0, 4)));
        pane.select_next_occurrence(&buffer);
        assert_eq!(pane.occurrences(), vec![(0, 4), (7, 11)]);
        assert_eq!(
            pane.selection_range(),
            None,
            "marcar não deixa seleção para trás"
        );
        assert_eq!(pane.selected_text(&buffer), None);

        // Digitar continua alcançando todas, que é o que a marcação promete.
        pane.insert(&mut buffer, "i");
        assert_eq!(buffer.text(), "nomei = nomei");
        assert_eq!(pane.selection_range(), None);
    }

    /// Acento não pode ser cortado pela metade ao apagar.
    #[test]
    fn backspace_removes_a_whole_character_from_each_occurrence() {
        let (mut pane, _) = pane(EditorCapabilities::plain());
        let mut buffer = TextBuffer::new("x = x");
        pane.set_cursor(1);
        pane.set_selection(Some((0, 1)));
        pane.select_next_occurrence(&buffer);
        pane.insert(&mut buffer, "ç");
        pane.insert(&mut buffer, "ã");
        assert_eq!(buffer.text(), "xçã = xçã");
        pane.key(&mut buffer, "Backspace", false, false);
        assert_eq!(buffer.text(), "xç = xç", "sai um caractere, não um byte");
    }

    /// `Enter` confirma o que foi escrito; `Esc` devolve o texto ao começo.
    #[test]
    fn enter_confirms_the_multi_edit_and_escape_undoes_it() {
        let (mut pane, _) = pane(EditorCapabilities::plain());
        let mut buffer = TextBuffer::new("nome = nome");
        pane.set_cursor(4);
        pane.set_selection(Some((0, 4)));
        pane.select_next_occurrence(&buffer);
        pane.insert(&mut buffer, "id");
        pane.key(&mut buffer, "Enter", false, false);
        assert_eq!(buffer.text(), "nomeid = nomeid");
        assert!(pane.occurrences().is_empty(), "confirmar solta as marcas");
        // Confirmado, `Enter` volta a quebrar linha.
        pane.key(&mut buffer, "Enter", false, false);
        assert!(buffer.text().contains('\n'));

        let mut outro = EditorPane::new(EditorCapabilities::plain());
        let mut voltando = TextBuffer::new("nome = nome");
        outro.set_cursor(4);
        outro.set_selection(Some((0, 4)));
        outro.select_next_occurrence(&voltando);
        outro.insert(&mut voltando, "id");
        assert_eq!(voltando.text(), "nomeid = nomeid");
        outro.key(&mut voltando, "Escape", false, false);
        assert_eq!(
            voltando.text(),
            "nome = nome",
            "desistir devolve o texto ao que era antes das marcas"
        );
        assert!(outro.occurrences().is_empty());
    }

    /// `Enter` abre a linha já indentada como a anterior.
    ///
    /// A regra é do editor da biblioteca; o painel converte os deslocamentos,
    /// porque a IDE conta bytes e o editor conta caracteres.
    #[test]
    fn enter_keeps_the_indentation_of_the_previous_line() {
        let (mut pane, _) = pane(EditorCapabilities::plain());
        let mut buffer = TextBuffer::new("class A {\n    int total;");
        pane.set_cursor(buffer.text().len());
        pane.key(&mut buffer, "Enter", false, false);
        assert_eq!(buffer.text(), "class A {\n    int total;\n    ");
        assert_eq!(pane.cursor(), buffer.text().len());

        // Digitar em seguida continua de onde a indentação parou.
        pane.insert(&mut buffer, "x");
        assert_eq!(buffer.text(), "class A {\n    int total;\n    x");

        // Na margem, nada é herdado.
        let mut raso = TextBuffer::new("abc");
        pane.set_cursor(3);
        pane.key(&mut raso, "Enter", false, false);
        assert_eq!(raso.text(), "abc\n");
    }

    /// Com acento antes do cursor, a conversão entre bytes e caracteres não pode
    /// deslocar a leitura da indentação.
    #[test]
    fn the_indentation_survives_multibyte_text() {
        let (mut pane, _) = pane(EditorCapabilities::plain());
        let mut buffer = TextBuffer::new("    ação;");
        pane.set_cursor(buffer.text().len());
        pane.key(&mut buffer, "Enter", false, false);
        assert_eq!(buffer.text(), "    ação;\n    ");
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
