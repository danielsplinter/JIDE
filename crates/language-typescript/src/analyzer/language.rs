//! O provider nativo de TypeScript.
//!
//! Ele responde por **texto**, e não por tipos: realce, estrutura e erro de
//! sintaxe. Tipo é o que o analisador externo traz, na fase 3 da `23` — e este
//! provider não sai quando ele chegar. É o chão: sem Node instalado, sem o
//! pacote `typescript` no projeto, ou com o processo morto, é ele que responde.
//! Ver a ADR-025.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use async_trait::async_trait;
use ide_domain::{
    Diagnostic, DocumentChange, DocumentId, DocumentSnapshot, LanguageId, ProviderId,
    SemanticSymbol, SyntaxSnapshot,
};
use ide_language_api::{
    ReadinessSignal,
    ActiveLanguage, LANGUAGE_API_VERSION, LanguageActivationContext, LanguageCapabilities,
    LanguageError, LanguageMetadata, LanguageProvider,
};

use super::index::WorkspaceIndex;
use super::parser::TypeScriptParser;
use super::syntax;

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
        // `WORKSPACE_SYMBOLS` e não `COMPLETION`: este provider sabe **quais
        // tipos existem** e não sabe o tipo de uma expressão. Declarar as duas
        // juntas seria prometer o ponto, que é a fase 4 da `25`.
        LanguageCapabilities::SYNTAX
            | LanguageCapabilities::DIAGNOSTICS
            | LanguageCapabilities::WORKSPACE_SYMBOLS
    }

    async fn activate(
        &self,
        context: LanguageActivationContext,
    ) -> Result<Box<dyn ActiveLanguage>, LanguageError> {
        Ok(Box::new(ActiveTypeScript::new(context)?))
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
    /// O índice do projeto, quando ficar pronto.
    ///
    /// **Aberto**, e não carregado: o que fica em memória é a tabela de nomes, e
    /// os registros saem do disco quando um nome casa.
    index: Arc<Mutex<Option<WorkspaceIndex>>>,
    /// O sinal que diz quando a varredura terminou.
    readiness: ReadinessSignal,
}

impl ActiveTypeScript {
    fn new(context: LanguageActivationContext) -> Result<Self, LanguageError> {
        let index: Arc<Mutex<Option<WorkspaceIndex>>> = Arc::new(Mutex::new(None));
        let readiness = ReadinessSignal::new();

        // **A varredura não pode segurar a ativação.** Medida contra um monorepo
        // de 8 958 arquivos: 7,2 s com o cache do sistema quente, e muito mais
        // frio — que é o estado ao abrir o projeto pela primeira vez. Ativar é o
        // que dá realce ao arquivo que se acabou de abrir, e fazer o realce
        // esperar pela varredura do projeto inteiro seria trocar um problema
        // conhecido por outro.
        //
        // O sinal de prontidão é o mesmo que o analisador externo usa, e a IDE
        // já sabe mostrá-lo: gira no meio da tela enquanto dura, e some no fim.
        // Nada aqui é de TypeScript — é "esta linguagem ainda está preparando o
        // projeto".
        let raiz = context.workspace_root.clone();
        let raizes = context.source_roots.clone();
        let destino = Arc::clone(&index);
        let avisar = readiness.clone();
        std::thread::Builder::new()
            .name("typescript-index".to_owned())
            .spawn(move || {
                let construido = WorkspaceIndex::build(&raiz, &raizes);
                if let Ok(mut lugar) = destino.lock() {
                    *lugar = construido;
                }
                // Marcado **depois** de guardar, e marcado mesmo se a construção
                // falhar: um sinal que nunca fica pronto deixaria a IDE dizendo
                // para sempre que está carregando.
                avisar.mark_ready();
            })
            .map_err(|erro| LanguageError::Provider(erro.to_string()))?;

        Ok(Self {
            language_id: LanguageId(TYPESCRIPT_LANGUAGE_ID.to_owned()),
            parser: TypeScriptParser::new()?,
            documents: Mutex::new(HashMap::new()),
            index,
            readiness,
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

    /// Este provider tem o que preparar: a varredura do projeto.
    fn readiness(&self) -> Option<ReadinessSignal> {
        Some(self.readiness.clone())
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

    /// Os tipos do projeto cujo nome casa com o que foi digitado.
    ///
    /// **É a única pergunta de projeto que não depende de `import`**: um nome ou
    /// casa ou não casa. Definição e referências precisam saber **qual** dos
    /// `LoginService` é o certo, e isso é a fase 2 da `25`.
    ///
    /// Sem índice a resposta é um erro, e não uma lista vazia: "não indexei" e
    /// "não existe tipo com esse nome" são coisas diferentes, e confundi-las é a
    /// família de defeito que esta IDE já encontrou várias vezes.
    async fn workspace_types(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<SemanticSymbol>, LanguageError> {
        // Enquanto a varredura roda, a resposta é **"ainda não"**, e não uma
        // lista vazia. As duas se pareceriam na tela, e confundi-las é a família
        // de defeito que esta IDE já encontrou várias vezes.
        if !self.readiness.is_ready() {
            return Err(LanguageError::Unavailable(
                "o projeto ainda está sendo indexado".to_owned(),
            ));
        }
        let registro = self
            .index
            .lock()
            .map_err(|_| LanguageError::Provider("índice indisponível".to_owned()))?;
        let Some(index) = registro.as_ref() else {
            return Err(LanguageError::Unavailable(
                "não foi possível indexar o projeto".to_owned(),
            ));
        };
        Ok(index.tipos(query, limit))
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
