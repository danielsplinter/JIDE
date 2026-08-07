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

/// Troca uma faixa de linhas por outras, respeitando o fim de linha do arquivo.
///
/// **Uma operação só, porque na tela é um gesto só.** Devolver uma linha ao
/// estado do último commit pode ser trocar, acrescentar ou apagar, e as três são
/// a mesma coisa vista de fora: *a faixa de então entra no lugar da faixa de
/// agora*. Separá-las em três funções fazia quem chama decidir qual usar, e
/// decidir errado apaga código.
///
/// A faixa é meio-aberta. Vazia — `start == end` — quer dizer **onde**, e não *o
/// quê*: nada sai daquela posição, e as linhas novas entram ali.
///
/// # O fim de linha é o do arquivo
///
/// Reescrever com `\n` um arquivo que usa `\r\n` marcaria *todas* as linhas como
/// alteradas: devolver uma linha viraria um diff inteiro, e quem pediu uma linha
/// veria o arquivo inteiro ficar verde.
#[must_use]
pub fn replace_lines(text: &str, range: std::ops::Range<usize>, novas: &[String]) -> String {
    let quebra = if text.contains("\r\n") { "\r\n" } else { "\n" };
    // Arquivo vazio não tem quebra para copiar, e o que entra nele precisa
    // terminar: sem isso, a linha seguinte grudaria na última.
    let terminava_com_quebra = text.is_empty() || text.ends_with('\n');
    let mut linhas: Vec<String> = text.lines().map(ToOwned::to_owned).collect();
    let start = range.start.min(linhas.len());
    let end = range.end.clamp(start, linhas.len());
    linhas.splice(start..end, novas.iter().cloned());
    if linhas.is_empty() {
        return String::new();
    }
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

    /// Trocar, acrescentar e apagar são a mesma faixa, e o arquivo continua seu.
    ///
    /// O fim de linha é o que separa "uma linha mudou" de "o arquivo inteiro
    /// mudou": num arquivo com CRLF, escrever LF marca todas as linhas.
    #[test]
    fn trocar_uma_faixa_cobre_os_tres_gestos_e_guarda_o_fim_de_linha() {
        let linha = |texto: &str| vec![texto.to_owned()];

        // Trocar: a faixa de agora sai, a de então entra.
        assert_eq!(
            replace_lines("um\ndois\ntres\n", 1..2, &linha("DOIS")),
            "um\nDOIS\ntres\n"
        );
        assert_eq!(
            replace_lines("um\r\ndois\r\ntres\r\n", 1..2, &linha("DOIS")),
            "um\r\nDOIS\r\ntres\r\n",
            "o arquivo usava CRLF, e continua usando"
        );

        // Acrescentar: faixa vazia diz **onde**, e o resto desce.
        assert_eq!(
            replace_lines("a\nc\n", 1..1, &linha("b")),
            "a\nb\nc\n",
            "o `c` continua no arquivo"
        );

        // Apagar: nada entra, e é assim que se desfaz uma linha acrescentada.
        assert_eq!(
            replace_lines("a\nnova\nb\n", 1..2, &[]),
            "a\nb\n",
            "a linha some, e só ela"
        );
        assert_eq!(
            replace_lines("a\numa\noutra\nb\n", 1..3, &[]),
            "a\nb\n",
            "um bloco inteiro de uma vez"
        );

        // Casos de borda: sem quebra no fim continua sem, arquivo vazio ganha
        // a linha terminada, e faixa além do fim entra no fim.
        assert_eq!(replace_lines("um\ndois", 0..1, &linha("UM")), "UM\ndois");
        assert_eq!(replace_lines("", 0..0, &linha("primeira")), "primeira\n");
        assert_eq!(replace_lines("a\n", 9..9, &linha("fim")), "a\nfim\n");
        assert_eq!(
            replace_lines("so\n", 0..1, &[]),
            "",
            "apagar tudo devolve o vazio, e não uma quebra solta"
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
