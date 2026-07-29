//! Dono isolado do parser incremental tree-sitter.

use std::sync::Mutex;

use ide_language_api::LanguageError;
use tree_sitter::{Parser, Tree};

pub(super) struct JavaParser {
    inner: Mutex<Parser>,
}

impl JavaParser {
    pub(super) fn new() -> Result<Self, LanguageError> {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_java::LANGUAGE.into())
            .map_err(|error| LanguageError::Provider(error.to_string()))?;
        Ok(Self {
            inner: Mutex::new(parser),
        })
    }

    pub(super) fn parse(
        &self,
        source: &str,
        previous: Option<&Tree>,
    ) -> Result<Tree, LanguageError> {
        self.inner
            .lock()
            .map_err(|_| LanguageError::Provider("Java parser lock poisoned".to_owned()))?
            .parse(source, previous)
            .ok_or_else(|| LanguageError::Provider("Java parsing was cancelled".to_owned()))
    }

    pub(super) fn with_mut<T>(
        &self,
        operation: impl FnOnce(&mut Parser) -> T,
    ) -> Result<T, LanguageError> {
        let mut parser = self
            .inner
            .lock()
            .map_err(|_| LanguageError::Provider("Java parser lock poisoned".to_owned()))?;
        Ok(operation(&mut parser))
    }
}
