//! Dono isolado do parser incremental tree-sitter.

use std::sync::Mutex;

use ide_language_api::LanguageError;
use tree_sitter::{Parser, Tree};

pub(crate) struct TypeScriptParser {
    inner: Mutex<Parser>,
}

impl TypeScriptParser {
    pub(crate) fn new() -> Result<Self, LanguageError> {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into())
            .map_err(|error| LanguageError::Provider(error.to_string()))?;
        Ok(Self {
            inner: Mutex::new(parser),
        })
    }

    pub(crate) fn parse(
        &self,
        source: &str,
        previous: Option<&Tree>,
    ) -> Result<Tree, LanguageError> {
        self.inner
            .lock()
            .map_err(|_| LanguageError::Provider("TypeScript parser lock poisoned".to_owned()))?
            .parse(source, previous)
            .ok_or_else(|| LanguageError::Provider("TypeScript parsing was cancelled".to_owned()))
    }
}
