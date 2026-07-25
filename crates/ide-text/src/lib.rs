#![doc = "Buffers, documentos abertos e abas do editor."]

use std::{collections::HashMap, fs, path::{Path, PathBuf}};

use ide_domain::DocumentId;
use thiserror::Error;

#[derive(Clone, Debug)]
pub struct TextBuffer {
    text: String,
    revision: u64,
    dirty: bool,
}

impl TextBuffer {
    pub fn new(text: impl Into<String>) -> Self {
        Self { text: text.into(), revision: 0, dirty: false }
    }

    pub fn text(&self) -> &str { &self.text }
    pub const fn revision(&self) -> u64 { self.revision }
    pub const fn is_dirty(&self) -> bool { self.dirty }

    pub fn replace(&mut self, range: std::ops::Range<usize>, replacement: &str) -> Result<(), BufferError> {
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

    pub fn mark_saved(&mut self) { self.dirty = false; }
}

#[derive(Clone, Debug)]
pub struct OpenDocument {
    pub id: DocumentId,
    pub path: PathBuf,
    pub buffer: TextBuffer,
}

#[derive(Default)]
pub struct EditorSession {
    documents: HashMap<DocumentId, OpenDocument>,
    tabs: Vec<DocumentId>,
    active: Option<DocumentId>,
    next_id: u64,
}

impl EditorSession {
    pub fn open(&mut self, path: &Path) -> Result<DocumentId, BufferError> {
        if let Some(document) = self.documents.values().find(|document| document.path == path) {
            self.active = Some(document.id);
            return Ok(document.id);
        }
        let text = fs::read_to_string(path)?;
        self.next_id += 1;
        let id = DocumentId(self.next_id);
        self.documents.insert(id, OpenDocument {
            id,
            path: path.to_path_buf(),
            buffer: TextBuffer::new(text),
        });
        self.tabs.push(id);
        self.active = Some(id);
        Ok(id)
    }

    pub fn open_memory(&mut self, name: impl Into<PathBuf>, text: impl Into<String>) -> DocumentId {
        self.next_id += 1;
        let id = DocumentId(self.next_id);
        self.documents.insert(id, OpenDocument {
            id,
            path: name.into(),
            buffer: TextBuffer::new(text),
        });
        self.tabs.push(id);
        self.active = Some(id);
        id
    }

    pub fn activate(&mut self, id: DocumentId) -> Result<(), BufferError> {
        if !self.documents.contains_key(&id) { return Err(BufferError::UnknownDocument(id)); }
        self.active = Some(id);
        Ok(())
    }

    pub fn close(&mut self, id: DocumentId) -> Result<OpenDocument, BufferError> {
        let document = self.documents.remove(&id).ok_or(BufferError::UnknownDocument(id))?;
        self.tabs.retain(|tab| *tab != id);
        if self.active == Some(id) { self.active = self.tabs.last().copied(); }
        Ok(document)
    }

    pub const fn active_id(&self) -> Option<DocumentId> { self.active }
    pub fn active(&self) -> Option<&OpenDocument> { self.active.and_then(|id| self.documents.get(&id)) }
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
    #[error("document I/O failed: {0}")]
    Io(#[from] std::io::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unicode_edits_require_valid_boundaries() {
        let mut buffer = TextBuffer::new("ação");
        assert!(buffer.replace(0..2, "A").is_err());
        assert!(buffer.replace(0..3, "A").is_ok());
        assert_eq!(buffer.text(), "Aão");
        assert!(buffer.is_dirty());
    }

    #[test]
    fn closing_active_tab_selects_previous_tab() {
        let mut session = EditorSession::default();
        let first = session.open_memory("one.rs", "one");
        let second = session.open_memory("two.rs", "two");
        assert!(session.close(second).is_ok());
        assert_eq!(session.active_id(), Some(first));
    }
}

