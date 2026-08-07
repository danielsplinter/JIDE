//! Buffers puros, documentos abertos e abas do editor.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use ide_domain::{DocumentId, TextRange};
use thiserror::Error;

#[derive(Clone, Debug)]
pub struct TextBuffer {
    text: String,
    revision: u64,
    dirty: bool,
}

impl TextBuffer {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            revision: 0,
            dirty: false,
        }
    }

    pub fn text(&self) -> &str {
        &self.text
    }
    pub const fn revision(&self) -> u64 {
        self.revision
    }
    pub const fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub fn replace(
        &mut self,
        range: std::ops::Range<usize>,
        replacement: &str,
    ) -> Result<(), BufferError> {
        if range.start > range.end
            || range.end > self.text.len()
            || !self.text.is_char_boundary(range.start)
            || !self.text.is_char_boundary(range.end)
        {
            return Err(BufferError::InvalidRange);
        }
        self.text.replace_range(range, replacement);
        self.revision += 1;
        self.dirty = true;
        Ok(())
    }

    pub fn mark_saved(&mut self) {
        self.dirty = false;
    }
}

/// Troca o conteúdo de uma linha, respeitando o fim de linha do arquivo.
///
/// **O fim de linha é o do arquivo, e não o da máquina.** Reescrever com `\n` um
/// arquivo que usa `\r\n` marcaria *todas* as linhas como alteradas: devolver
/// uma linha viraria um diff inteiro, e quem pediu a devolução de uma linha veria
/// o arquivo inteiro ficar verde.
///
/// Uma linha além do fim é acrescentada: é o caso de devolver a última linha de
/// um arquivo que encolheu.
///
/// Vive aqui, e não na aplicação, porque é manipulação de texto e tem o mesmo
/// dono que [`rewrite_occurrences`] — e porque assim se testa sem gravar nada.
#[must_use]
pub fn rewrite_line(text: &str, line: usize, replacement: String) -> String {
    let quebra = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let terminava_com_quebra = text.ends_with('\n');
    let mut linhas: Vec<String> = text.lines().map(ToOwned::to_owned).collect();
    if line < linhas.len() {
        linhas[line] = replacement;
    } else {
        linhas.push(replacement);
    }
    let mut novo = linhas.join(quebra);
    if terminava_com_quebra {
        novo.push_str(quebra);
    }
    novo
}

/// Acrescenta uma linha numa posição, respeitando o fim de linha do arquivo.
///
/// **Irmã de [`rewrite_line`], e diferente dela no que importa.** Devolver ao
/// arquivo uma linha que *saiu* dele não é trocar a linha que está naquela
/// posição — essa é outra linha, que ninguém mandou tocar. É acrescentar, e
/// empurrar o resto para baixo.
///
/// Posição além do fim entra no fim, que é onde ela cabe.
#[must_use]
pub fn insert_line(text: &str, at: usize, novo: String) -> String {
    let quebra = if text.contains("\r\n") { "\r\n" } else { "\n" };
    // Arquivo vazio não tem quebra para copiar, e a linha acrescentada precisa
    // terminar: sem isto, a seguinte grudaria nela.
    let terminava_com_quebra = text.is_empty() || text.ends_with('\n');
    let mut linhas: Vec<String> = text.lines().map(ToOwned::to_owned).collect();
    linhas.insert(at.min(linhas.len()), novo);
    let mut resultado = linhas.join(quebra);
    if terminava_com_quebra {
        resultado.push_str(quebra);
    }
    resultado
}

/// Troca um nome nas posições dadas, do fim para o começo do texto.
///
/// Do fim para o começo porque trocar no começo moveria as posições seguintes:
/// cada uma seria escrita alguns caracteres fora do lugar. Uma posição que já
/// não contém o nome antigo é ignorada — ela veio de uma análise vencida, e
/// escrever ali sobrescreveria outra coisa.
///
/// Vive aqui, e não na tela nem na aplicação, porque as duas precisam dela: uma
/// para os arquivos abertos, outra para os fechados.
#[must_use]
pub fn rewrite_occurrences(
    text: &str,
    ranges: &[TextRange],
    old_name: &str,
    new_name: &str,
) -> String {
    let mut ordenadas: Vec<&TextRange> = ranges.iter().collect();
    ordenadas.sort_by(|esquerda, direita| {
        (direita.start.line, direita.start.column)
            .cmp(&(esquerda.start.line, esquerda.start.column))
    });
    let mut resultado = text.to_owned();
    for range in ordenadas {
        let inicio = offset_at(
            &resultado,
            range.start.line as usize,
            range.start.column as usize,
        );
        let fim = offset_at(
            &resultado,
            range.end.line as usize,
            range.end.column as usize,
        );
        if fim > inicio && resultado.get(inicio..fim) == Some(old_name) {
            resultado.replace_range(inicio..fim, new_name);
        }
    }
    resultado
}

/// Deslocamento em bytes de uma posição em linha e coluna de caracteres.
fn offset_at(text: &str, line: usize, column: usize) -> usize {
    let mut offset = 0;
    for (indice, conteudo) in text.split('\n').enumerate() {
        if indice == line {
            return offset
                + conteudo
                    .char_indices()
                    .nth(column)
                    .map_or(conteudo.len(), |(byte, _)| byte);
        }
        offset += conteudo.len() + 1;
    }
    text.len()
}

#[derive(Clone, Debug)]
pub struct OpenDocument {
    pub id: DocumentId,
    pub path: PathBuf,
    pub buffer: TextBuffer,
    persistent: bool,
}

impl OpenDocument {
    #[must_use]
    pub const fn is_persistent(&self) -> bool {
        self.persistent
    }
}

#[derive(Default)]
pub struct EditorSession {
    documents: HashMap<DocumentId, OpenDocument>,
    tabs: Vec<DocumentId>,
    active: Option<DocumentId>,
    next_id: u64,
}

impl EditorSession {
    pub fn open(&mut self, path: &Path, text: impl Into<String>) -> DocumentId {
        if let Some(document) = self
            .documents
            .values()
            .find(|document| document.path == path)
        {
            self.active = Some(document.id);
            return document.id;
        }
        self.next_id += 1;
        let id = DocumentId(self.next_id);
        self.documents.insert(
            id,
            OpenDocument {
                id,
                path: path.to_path_buf(),
                buffer: TextBuffer::new(text),
                persistent: true,
            },
        );
        self.tabs.push(id);
        self.active = Some(id);
        id
    }

    pub fn open_memory(&mut self, name: impl Into<PathBuf>, text: impl Into<String>) -> DocumentId {
        self.next_id += 1;
        let id = DocumentId(self.next_id);
        self.documents.insert(
            id,
            OpenDocument {
                id,
                path: name.into(),
                buffer: TextBuffer::new(text),
                persistent: false,
            },
        );
        self.tabs.push(id);
        self.active = Some(id);
        id
    }

    pub fn activate(&mut self, id: DocumentId) -> Result<(), BufferError> {
        if !self.documents.contains_key(&id) {
            return Err(BufferError::UnknownDocument(id));
        }
        self.active = Some(id);
        Ok(())
    }

    pub fn close(&mut self, id: DocumentId) -> Result<OpenDocument, BufferError> {
        let document = self
            .documents
            .remove(&id)
            .ok_or(BufferError::UnknownDocument(id))?;
        self.tabs.retain(|tab| *tab != id);
        if self.active == Some(id) {
            self.active = self.tabs.last().copied();
        }
        Ok(document)
    }

    pub const fn active_id(&self) -> Option<DocumentId> {
        self.active
    }
    pub fn active(&self) -> Option<&OpenDocument> {
        self.active.and_then(|id| self.documents.get(&id))
    }
    pub fn active_mut(&mut self) -> Option<&mut OpenDocument> {
        self.active.and_then(|id| self.documents.get_mut(&id))
    }
    /// Troca o caminho de um documento aberto, para seguir um arquivo renomeado.
    pub fn set_path(&mut self, id: DocumentId, path: PathBuf) {
        if let Some(document) = self.documents.get_mut(&id) {
            document.path = path;
        }
    }

    /// Documento aberto, para quem vai reescrevê-lo.
    pub fn document_mut(&mut self, id: DocumentId) -> Option<&mut OpenDocument> {
        self.documents.get_mut(&id)
    }

    pub fn document(&self, id: DocumentId) -> Option<&OpenDocument> {
        self.documents.get(&id)
    }

    /// Confirma a gravação somente se o buffer ainda estiver na revisão enviada
    /// ao adapter. Assim uma resposta atrasada nunca limpa a marca de uma edição
    /// mais nova.
    pub fn mark_saved(&mut self, id: DocumentId, revision: u64) -> Result<(), BufferError> {
        let document = self
            .documents
            .get_mut(&id)
            .ok_or(BufferError::UnknownDocument(id))?;
        if document.buffer.revision() == revision {
            document.buffer.mark_saved();
        }
        Ok(())
    }
    pub fn tabs(&self) -> impl Iterator<Item = &OpenDocument> {
        self.tabs.iter().filter_map(|id| self.documents.get(id))
    }
}

#[derive(Debug, Error)]
pub enum BufferError {
    #[error("invalid text range")]
    InvalidRange,
    #[error("unknown document {0:?}")]
    UnknownDocument(DocumentId),
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A troca de uma linha guarda o fim de linha que o arquivo já usava.
    ///
    /// É o que separa "uma linha mudou" de "o arquivo inteiro mudou": num
    /// arquivo com CRLF, escrever LF marca todas as linhas como alteradas.
    #[test]
    fn trocar_uma_linha_guarda_o_fim_de_linha_do_arquivo() {
        let unix = "um\ndois\ntres\n";
        assert_eq!(
            rewrite_line(unix, 1, "DOIS".to_owned()),
            "um\nDOIS\ntres\n"
        );

        let windows = "um\r\ndois\r\ntres\r\n";
        assert_eq!(
            rewrite_line(windows, 1, "DOIS".to_owned()),
            "um\r\nDOIS\r\ntres\r\n",
            "o arquivo usava CRLF, e continua usando"
        );

        // Sem quebra no fim, continua sem: acrescentar uma seria uma alteração
        // que ninguém pediu, na última linha.
        assert_eq!(rewrite_line("um\ndois", 0, "UM".to_owned()), "UM\ndois");

        // Linha além do fim: o arquivo encolheu, e a linha volta no fim dele.
        assert_eq!(
            rewrite_line("um\n", 5, "novo".to_owned()),
            "um\nnovo\n",
            "devolver uma linha que já não existe a acrescenta"
        );
    }

    #[test]
    fn unicode_edits_require_valid_boundaries() {
        let mut buffer = TextBuffer::new("ação");
        assert!(buffer.replace(0..2, "A").is_err());
        assert!(buffer.replace(0..3, "A").is_ok());
        assert_eq!(buffer.text(), "Aão");
        assert!(buffer.is_dirty());
    }

    /// Acrescentar não é trocar: a linha de baixo continua onde estava.
    ///
    /// É o caso da linha que foi *removida* e volta pela seta da comparação.
    /// Trocá-la pela que está naquela posição apagaria conteúdo que ninguém
    /// mandou tocar.
    #[test]
    fn acrescentar_uma_linha_empurra_o_resto_para_baixo() {
        assert_eq!(
            insert_line("a\nc\n", 1, "b".to_owned()),
            "a\nb\nc\n",
            "o `c` continua no arquivo"
        );
        assert_eq!(
            insert_line("a\r\nc\r\n", 1, "b".to_owned()),
            "a\r\nb\r\nc\r\n",
            "e o fim de linha do arquivo é respeitado"
        );
        assert_eq!(
            insert_line("", 0, "primeira".to_owned()),
            "primeira\n",
            "arquivo vazio ganha a linha, terminada"
        );
        assert_eq!(
            insert_line("a\n", 9, "fim".to_owned()),
            "a\nfim\n",
            "posição além do fim entra no fim"
        );
    }

    #[test]
    fn closing_active_tab_selects_previous_tab() {
        let mut session = EditorSession::default();
        let first = session.open_memory("one.rs", "one");
        let second = session.open_memory("two.rs", "two");
        assert!(session.close(second).is_ok());
        assert_eq!(session.active_id(), Some(first));
    }

    #[test]
    fn saving_an_old_revision_does_not_clear_a_new_edit() {
        let mut session = EditorSession::default();
        let id = session.open_memory("one.rs", "one");
        let Some(document) = session.active_mut() else {
            panic!("documento em memória deveria estar ativo");
        };
        assert!(document.buffer.replace(0..3, "two").is_ok());
        let saved_revision = document.buffer.revision();
        assert!(document.buffer.replace(0..3, "three").is_ok());

        assert!(session.mark_saved(id, saved_revision).is_ok());
        assert!(
            session
                .active()
                .is_some_and(|document| document.buffer.is_dirty())
        );
    }
}
