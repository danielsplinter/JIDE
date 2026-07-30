//! Propriedade dos documentos Java e do parsing incremental.

use std::{
    collections::HashMap,
    sync::{Mutex, MutexGuard},
};

use ide_domain::{
    DocumentChange, DocumentId, DocumentSnapshot, Location, SemanticSnapshot, SyntaxSnapshot,
};
use ide_language_api::LanguageError;
use tree_sitter::{InputEdit, Parser, Tree};

use crate::{
    language::{
        analyze, analyze_semantics, offset_for_position, point_after_text, point_for_offset,
    },
    parser::JavaParser,
};

pub(super) struct ParsedDocument {
    pub(super) snapshot: DocumentSnapshot,
    pub(super) tree: Tree,
    pub(super) analysis: SyntaxSnapshot,
    pub(super) semantic: SemanticSnapshot,
    pub(super) references: HashMap<String, Vec<Location>>,
    /// Se a semântica acima vale para o texto atual.
    ///
    /// Símbolos, escopos e referências custam quase tanto quanto o realce e não
    /// servem para desenhar: só completação, navegação e busca de usos os
    /// consultam. Calculá-los a cada tecla era pagar por resposta que ninguém
    /// tinha pedido — agora a tecla apenas invalida, e quem pergunta paga.
    semantics_ready: bool,
}

impl ParsedDocument {
    /// Calcula a semântica se ela ainda não valer para o texto atual.
    fn ensure_semantics(&mut self) {
        if self.semantics_ready {
            return;
        }
        let (semantic, references) = analyze_semantics(&self.snapshot, &self.tree);
        self.semantic = semantic;
        self.references = references;
        self.semantics_ready = true;
    }
}

pub(super) struct Documents {
    parser: JavaParser,
    entries: Mutex<HashMap<DocumentId, ParsedDocument>>,
}

impl Documents {
    pub(super) fn new() -> Result<Self, LanguageError> {
        Ok(Self {
            parser: JavaParser::new()?,
            entries: Mutex::new(HashMap::new()),
        })
    }

    pub(super) fn with_parser_mut<T>(
        &self,
        operation: impl FnOnce(&mut Parser) -> T,
    ) -> Result<T, LanguageError> {
        self.parser.with_mut(operation)
    }

    pub(super) fn parse_tree(&self, source: &str) -> Result<Tree, LanguageError> {
        self.parser.parse(source, None)
    }

    fn parse(
        &self,
        snapshot: DocumentSnapshot,
        previous: Option<&Tree>,
    ) -> Result<ParsedDocument, LanguageError> {
        let tree = self.parser.parse(&snapshot.text, previous)?;
        let analysis = analyze(&snapshot, &tree);
        // A semântica fica para quem a pedir; ver `ParsedDocument`.
        let semantic = SemanticSnapshot {
            document_id: snapshot.id,
            version: snapshot.version,
            symbols: Vec::new(),
            scopes: Vec::new(),
        };
        Ok(ParsedDocument {
            snapshot,
            tree,
            analysis,
            semantic,
            references: HashMap::new(),
            semantics_ready: false,
        })
    }

    pub(super) fn lock(
        &self,
    ) -> Result<MutexGuard<'_, HashMap<DocumentId, ParsedDocument>>, LanguageError> {
        self.entries
            .lock()
            .map_err(|_| LanguageError::Provider("Java document lock poisoned".to_owned()))
    }

    pub(super) fn open(&self, document: DocumentSnapshot) -> Result<(), LanguageError> {
        let id = document.id;
        let parsed = self.parse(document, None)?;
        self.lock()?.insert(id, parsed);
        Ok(())
    }

    pub(super) fn change(&self, change: DocumentChange) -> Result<(), LanguageError> {
        let mut documents = self.lock()?;
        let document = documents
            .get_mut(&change.document_id)
            .ok_or_else(|| LanguageError::Provider("Java document is not open".to_owned()))?;
        let (start_byte, old_end_byte) = match change.range {
            Some(range) => (
                offset_for_position(&document.snapshot.text, range.start)?,
                offset_for_position(&document.snapshot.text, range.end)?,
            ),
            None => (0, document.snapshot.text.len()),
        };
        if start_byte > old_end_byte {
            return Err(LanguageError::Provider(
                "Java document change has an invalid range".to_owned(),
            ));
        }

        let start_position = point_for_offset(&document.snapshot.text, start_byte);
        let old_end_position = point_for_offset(&document.snapshot.text, old_end_byte);
        let new_end_position = point_after_text(start_position, &change.text);
        document.tree.edit(&InputEdit {
            start_byte,
            old_end_byte,
            new_end_byte: start_byte + change.text.len(),
            start_position,
            old_end_position,
            new_end_position,
        });
        document
            .snapshot
            .text
            .replace_range(start_byte..old_end_byte, &change.text);
        document.snapshot.version = change.version;

        let updated = self.parse(document.snapshot.clone(), Some(&document.tree))?;
        *document = updated;
        Ok(())
    }

    pub(super) fn close(&self, document_id: DocumentId) -> Result<(), LanguageError> {
        self.lock()?.remove(&document_id);
        Ok(())
    }

    pub(super) fn syntax(&self, document_id: DocumentId) -> Result<SyntaxSnapshot, LanguageError> {
        self.lock()?
            .get(&document_id)
            .map(|document| document.analysis.clone())
            .ok_or_else(|| LanguageError::Provider("Java document is not open".to_owned()))
    }

    pub(super) fn semantic(
        &self,
        document_id: DocumentId,
    ) -> Result<SemanticSnapshot, LanguageError> {
        self.lock()?
            .get_mut(&document_id)
            .map(|document| {
                document.ensure_semantics();
                document.semantic.clone()
            })
            .ok_or_else(|| LanguageError::Provider("Java document is not open".to_owned()))
    }

    /// Prepara a semântica de todos os documentos abertos.
    ///
    /// Quem responde por completação, navegação e usos olha o conjunto dos
    /// arquivos abertos, e não só o da vez; é aqui que a conta adiada é paga.
    pub(super) fn ensure_semantics(documents: &mut HashMap<DocumentId, ParsedDocument>) {
        for document in documents.values_mut() {
            document.ensure_semantics();
        }
    }

    pub(super) fn clear(&self) -> Result<(), LanguageError> {
        self.lock()?.clear();
        Ok(())
    }
}
