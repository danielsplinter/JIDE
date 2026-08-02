//! O provider nativo de TypeScript.
//!
//! Ele responde por **texto**, e não por tipos: realce, estrutura e erro de
//! sintaxe. Tipo é o que o analisador externo traz, na fase 3 da `23` — e este
//! provider não sai quando ele chegar. É o chão: sem Node instalado, sem o
//! pacote `typescript` no projeto, ou com o processo morto, é ele que responde.
//! Ver a ADR-025.

use std::{collections::HashMap, sync::Mutex};

use async_trait::async_trait;
use ide_domain::{
    Diagnostic, DocumentChange, DocumentId, DocumentSnapshot, LanguageId, ProviderId,
    SyntaxSnapshot,
};
use ide_language_api::{
    ActiveLanguage, LANGUAGE_API_VERSION, LanguageActivationContext, LanguageCapabilities,
    LanguageError, LanguageMetadata, LanguageProvider,
};

use crate::parser::TypeScriptParser;
use crate::syntax;

pub const TYPESCRIPT_LANGUAGE_ID: &str = "typescript";
pub const TYPESCRIPT_PROVIDER_ID: &str = "typescript.syntax";

pub struct TypeScriptLanguageProvider;

impl TypeScriptLanguageProvider {
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl Default for TypeScriptLanguageProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl LanguageProvider for TypeScriptLanguageProvider {
    fn metadata(&self) -> LanguageMetadata {
        LanguageMetadata {
            language_id: LanguageId(TYPESCRIPT_LANGUAGE_ID.to_owned()),
            provider_id: ProviderId(TYPESCRIPT_PROVIDER_ID.to_owned()),
            display_name: "TypeScript nativo".to_owned(),
            extensions: vec!["ts".to_owned()],
            api_version: LANGUAGE_API_VERSION,
            // Sem tipos não há o que oferecer depois do ponto, e oferecer o que
            // se adivinha seria pior do que não oferecer. O gatilho volta com o
            // analisador externo.
            trigger_characters: Vec::new(),
        }
    }

    fn capabilities(&self) -> LanguageCapabilities {
        LanguageCapabilities::SYNTAX | LanguageCapabilities::DIAGNOSTICS
    }

    async fn activate(
        &self,
        _context: LanguageActivationContext,
    ) -> Result<Box<dyn ActiveLanguage>, LanguageError> {
        Ok(Box::new(ActiveTypeScript::new()?))
    }
}

/// Documento aberto, com a última análise já pronta.
///
/// O realce é calculado na abertura e a cada mudança, e guardado. Quem pede o
/// `SyntaxSnapshot` lê o que está aqui — recalcular a pedido faria a mesma
/// travessia duas vezes por tecla.
struct ParsedDocument {
    text: String,
    snapshot: SyntaxSnapshot,
}

struct ActiveTypeScript {
    language_id: LanguageId,
    parser: TypeScriptParser,
    documents: Mutex<HashMap<DocumentId, ParsedDocument>>,
}

impl ActiveTypeScript {
    fn new() -> Result<Self, LanguageError> {
        Ok(Self {
            language_id: LanguageId(TYPESCRIPT_LANGUAGE_ID.to_owned()),
            parser: TypeScriptParser::new()?,
            documents: Mutex::new(HashMap::new()),
        })
    }

    fn analyze(
        &self,
        document_id: DocumentId,
        version: u64,
        text: &str,
    ) -> Result<ParsedDocument, LanguageError> {
        let tree = self.parser.parse(text, None)?;
        let pass = syntax::analyze(&tree, text);
        Ok(ParsedDocument {
            text: text.to_owned(),
            snapshot: SyntaxSnapshot {
                document_id,
                version,
                outline: pass.outline,
                highlights: pass.highlights,
                // TypeScript não tem o conceito de import do domínio, que foi
                // desenhado sobre `import a.b.C` de Java. Um `import { X } from
                // "y"` não cabe nele sem mentir, e mentir aqui apareceria como
                // navegação errada. Fica vazio até haver contrato que o expresse.
                imports: Vec::new(),
                diagnostics: pass.diagnostics,
            },
        })
    }

    fn store(&self, document_id: DocumentId, parsed: ParsedDocument) -> Result<(), LanguageError> {
        self.documents
            .lock()
            .map_err(|_| LanguageError::Provider("TypeScript documents lock poisoned".to_owned()))?
            .insert(document_id, parsed);
        Ok(())
    }
}

#[async_trait]
impl ActiveLanguage for ActiveTypeScript {
    fn language_id(&self) -> &LanguageId {
        &self.language_id
    }

    async fn open_document(&self, document: DocumentSnapshot) -> Result<(), LanguageError> {
        let parsed = self.analyze(document.id, document.version, &document.text)?;
        self.store(document.id, parsed)
    }

    async fn change_document(&self, change: DocumentChange) -> Result<(), LanguageError> {
        let text = {
            let documents = self.documents.lock().map_err(|_| {
                LanguageError::Provider("TypeScript documents lock poisoned".to_owned())
            })?;
            let Some(current) = documents.get(&change.document_id) else {
                // Mudança de documento que não foi aberto aqui: ignorar é a
                // resposta certa, e não um erro — o host roteia por extensão e
                // um fechamento em corrida chega assim.
                return Ok(());
            };
            apply(&current.text, change.range, &change.text)
        };
        let parsed = self.analyze(change.document_id, change.version, &text)?;
        self.store(change.document_id, parsed)
    }

    async fn close_document(&self, document_id: DocumentId) -> Result<(), LanguageError> {
        self.documents
            .lock()
            .map_err(|_| LanguageError::Provider("TypeScript documents lock poisoned".to_owned()))?
            .remove(&document_id);
        Ok(())
    }

    async fn diagnostics(&self, document_id: DocumentId) -> Result<Vec<Diagnostic>, LanguageError> {
        Ok(self
            .documents
            .lock()
            .map_err(|_| LanguageError::Provider("TypeScript documents lock poisoned".to_owned()))?
            .get(&document_id)
            .map(|parsed| parsed.snapshot.diagnostics.clone())
            .unwrap_or_default())
    }

    async fn syntax(&self, document_id: DocumentId) -> Result<SyntaxSnapshot, LanguageError> {
        self.documents
            .lock()
            .map_err(|_| LanguageError::Provider("TypeScript documents lock poisoned".to_owned()))?
            .get(&document_id)
            .map(|parsed| parsed.snapshot.clone())
            .ok_or_else(|| LanguageError::Provider("documento não está aberto".to_owned()))
    }

    async fn shutdown(&self) -> Result<(), LanguageError> {
        self.documents
            .lock()
            .map_err(|_| LanguageError::Provider("TypeScript documents lock poisoned".to_owned()))?
            .clear();
        Ok(())
    }
}

/// Aplica a mudança ao texto guardado.
///
/// Intervalo ausente é substituição do documento inteiro, que é como o host
/// sincroniza depois de contrapressão (ADR-017).
fn apply(current: &str, range: Option<ide_domain::TextRange>, text: &str) -> String {
    let Some(range) = range else {
        return text.to_owned();
    };
    let start = offset_of(current, range.start.line as usize, range.start.column as usize);
    let end = offset_of(current, range.end.line as usize, range.end.column as usize);
    let mut updated = String::with_capacity(current.len() + text.len());
    updated.push_str(current.get(..start).unwrap_or_default());
    updated.push_str(text);
    updated.push_str(current.get(end..).unwrap_or_default());
    updated
}

/// Byte onde a linha e a coluna caem, contando colunas em caracteres.
fn offset_of(source: &str, line: usize, column: usize) -> usize {
    let mut offset = 0;
    for (index, current) in source.split_inclusive('\n').enumerate() {
        if index == line {
            let trimmed = current.strip_suffix('\n').unwrap_or(current);
            let trimmed = trimmed.strip_suffix('\r').unwrap_or(trimmed);
            let inner = trimmed
                .char_indices()
                .nth(column)
                .map_or(trimmed.len(), |(byte, _)| byte);
            return offset + inner;
        }
        offset += current.len();
    }
    source.len()
}
