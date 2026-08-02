//! O provider `typescript.service`.
//!
//! Ele sobe o `tsserver` do projeto e traduz o contrato da IDE para o protocolo
//! dele. O que ele acrescenta ao nativo é **tipo**: completação com os membros
//! certos, diagnóstico de verdade, definição que atravessa módulo.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};

use async_trait::async_trait;
use ide_domain::{
    CompletionItem, CompletionKind, CompletionRequest, DefinitionRequest, Diagnostic,
    DiagnosticSeverity, DocumentChange, DocumentId, DocumentSnapshot, LanguageId, Location,
    ProviderId, TextPosition, TextRange,
};
use ide_language_api::{
    ActiveLanguage, LANGUAGE_API_VERSION, LanguageActivationContext, LanguageCapabilities,
    LanguageError, LanguageMetadata, LanguageProvider,
};
use ide_process::{ProcessRequest, ProcessSupervisor};

use super::locate::{Missing, locate};
use super::protocol::Session;
use crate::analyzer::TYPESCRIPT_LANGUAGE_ID;

pub const TYPESCRIPT_SERVICE_PROVIDER_ID: &str = "typescript.service";

/// Quanto se espera por uma resposta.
///
/// Um analisador que não respondeu em cinco segundos está travado ou indexando
/// um projeto enorme; nos dois casos, esperar mais só adia a degradação.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);

pub struct TypeScriptServiceProvider {
    processes: Arc<dyn ProcessSupervisor>,
}

impl TypeScriptServiceProvider {
    #[must_use]
    pub fn new(processes: Arc<dyn ProcessSupervisor>) -> Self {
        Self { processes }
    }
}

#[async_trait]
impl LanguageProvider for TypeScriptServiceProvider {
    fn metadata(&self) -> LanguageMetadata {
        LanguageMetadata {
            language_id: LanguageId(TYPESCRIPT_LANGUAGE_ID.to_owned()),
            provider_id: ProviderId(TYPESCRIPT_SERVICE_PROVIDER_ID.to_owned()),
            display_name: "TypeScript".to_owned(),
            extensions: vec!["ts".to_owned()],
            api_version: LANGUAGE_API_VERSION,
            // Com tipos, o ponto volta a valer: agora há o que oferecer depois
            // dele, e o que se oferece é o que existe.
            trigger_characters: vec!['.'],
        }
    }

    fn capabilities(&self) -> LanguageCapabilities {
        LanguageCapabilities::DIAGNOSTICS
            | LanguageCapabilities::COMPLETION
            | LanguageCapabilities::DEFINITION
    }

    async fn activate(
        &self,
        context: LanguageActivationContext,
    ) -> Result<Box<dyn ActiveLanguage>, LanguageError> {
        let node_home = context
            .toolchain(&LanguageId(TYPESCRIPT_LANGUAGE_ID.to_owned()))
            .map(|toolchain| toolchain.installation_root.clone());
        // Faltar Node ou o pacote do projeto **não é falha genérica de
        // ativação**: é o motivo pelo qual este provider não serve agora, e o
        // texto diz o que fazer. `Unavailable` é o que faz o host cair para o
        // provider nativo, em vez de deixar o `.ts` sem ninguém.
        let localizacao = locate(&context.workspace_root, node_home.as_deref())
            .map_err(|falta: Missing| LanguageError::Unavailable(falta.to_string()))?;

        let conversa = self
            .processes
            .converse(ProcessRequest {
                program: localizacao.node,
                args: vec![
                    localizacao.tsserver.to_string_lossy().into_owned(),
                    // Sem isso o analisador manda diagnóstico sozinho a cada
                    // mudança, e nós já perguntamos quando queremos. É ruído no
                    // canal, e custa CPU do outro lado.
                    "--suppressDiagnosticEvents".to_owned(),
                ],
                working_directory: Some(context.workspace_root.clone()),
                timeout: None,
                environment: Vec::new(),
            })
            .await
            .map_err(|erro| LanguageError::Unavailable(erro.to_string()))?;

        Ok(Box::new(ActiveTypeScriptService {
            language_id: LanguageId(TYPESCRIPT_LANGUAGE_ID.to_owned()),
            session: Session::new(Arc::from(conversa)),
            documentos: Mutex::new(HashMap::new()),
        }))
    }
}

struct ActiveTypeScriptService {
    language_id: LanguageId,
    session: Session,
    /// Caminho e texto de cada documento aberto.
    ///
    /// O texto fica aqui porque a mudança é aplicada reabrindo o arquivo
    /// inteiro, e para reabrir é preciso ter o que reabrir.
    documentos: Mutex<HashMap<DocumentId, (PathBuf, String)>>,
}

impl ActiveTypeScriptService {
    /// Traduz a falha do analisador para o erro certo do contrato.
    ///
    /// Morto vira `Unavailable`, que faz o host reencaminhar o documento ao
    /// provider nativo. Vivo e recusando vira `Provider`, que é falha deste
    /// pedido e não muda a rota. Ver a fase 3b da `23`.
    fn failure(&self, detalhe: String) -> LanguageError {
        if self.session.is_alive() {
            LanguageError::Provider(detalhe)
        } else {
            LanguageError::Unavailable(detalhe)
        }
    }

    fn documento(&self, document_id: DocumentId) -> Result<(PathBuf, String), LanguageError> {
        self.documentos
            .lock()
            .map_err(|_| LanguageError::Provider("registro de documentos travado".to_owned()))?
            .get(&document_id)
            .cloned()
            .ok_or_else(|| LanguageError::Provider("documento não está aberto".to_owned()))
    }

    async fn abrir(&self, path: &Path, texto: &str) -> Result<(), LanguageError> {
        // `open` não tem resposta no protocolo, e esperar por uma travaria toda
        // abertura de arquivo.
        self.session
            .notify(
                "open",
                serde_json::json!({
                    "file": path_argument(path),
                    "fileContent": texto,
                    "scriptKindName": "TS",
                }),
            )
            .await
            .map_err(|detalhe| self.failure(detalhe))
    }
}

#[async_trait]
impl ActiveLanguage for ActiveTypeScriptService {
    fn language_id(&self) -> &LanguageId {
        &self.language_id
    }

    async fn open_document(&self, document: DocumentSnapshot) -> Result<(), LanguageError> {
        self.abrir(&document.path, &document.text).await?;
        self.documentos
            .lock()
            .map_err(|_| LanguageError::Provider("registro de documentos travado".to_owned()))?
            .insert(document.id, (document.path, document.text));
        Ok(())
    }

    async fn change_document(&self, change: DocumentChange) -> Result<(), LanguageError> {
        let (path, atual) = self.documento(change.document_id)?;
        let novo = apply(&atual, change.range, &change.text);
        // O protocolo tem mudança por intervalo, e ela é o caminho rápido.
        // Reabrir com o texto inteiro é lento e é **certo**: a conversão de
        // linha e coluna do domínio para a do analisador erra por um, e errar
        // aqui reescreve no lugar errado sem erro nenhum a apontar. Trocar por
        // incremental é trabalho com medição, e não palpite.
        self.abrir(&path, &novo).await?;
        self.documentos
            .lock()
            .map_err(|_| LanguageError::Provider("registro de documentos travado".to_owned()))?
            .insert(change.document_id, (path, novo));
        Ok(())
    }

    async fn close_document(&self, document_id: DocumentId) -> Result<(), LanguageError> {
        let Ok((path, _)) = self.documento(document_id) else {
            return Ok(());
        };
        self.documentos
            .lock()
            .map_err(|_| LanguageError::Provider("registro de documentos travado".to_owned()))?
            .remove(&document_id);
        self.session
            .notify(
                "close",
                serde_json::json!({ "file": path_argument(&path) }),
            )
            .await
            .map_err(|detalhe| self.failure(detalhe))
    }

    async fn diagnostics(&self, document_id: DocumentId) -> Result<Vec<Diagnostic>, LanguageError> {
        let (path, _) = self.documento(document_id)?;
        let resposta = self
            .session
            .request(
                "semanticDiagnosticsSync",
                serde_json::json!({ "file": path_argument(&path), "includeLinePosition": true }),
                REQUEST_TIMEOUT,
            )
            .await
            .map_err(|detalhe| self.failure(detalhe))?;
        Ok(resposta
            .as_array()
            .map(|itens| itens.iter().filter_map(diagnostic_from).collect())
            .unwrap_or_default())
    }

    async fn completion(
        &self,
        request: CompletionRequest,
    ) -> Result<Vec<CompletionItem>, LanguageError> {
        let (path, _) = self.documento(request.document_id)?;
        let (line, offset) = to_service(request.position);
        let resposta = self
            .session
            .request(
                "completionInfo",
                serde_json::json!({
                    "file": path_argument(&path),
                    "line": line,
                    "offset": offset,
                }),
                REQUEST_TIMEOUT,
            )
            .await
            .map_err(|detalhe| self.failure(detalhe))?;
        let entradas = resposta
            .get("entries")
            .and_then(serde_json::Value::as_array)
            .cloned()
            .unwrap_or_default();
        Ok(entradas
            .iter()
            .filter(|entrada| {
                request.prefix.is_empty()
                    || entrada
                        .get("name")
                        .and_then(serde_json::Value::as_str)
                        .is_some_and(|nome| nome.starts_with(&request.prefix))
            })
            .filter_map(completion_from)
            .collect())
    }

    async fn definition(
        &self,
        request: DefinitionRequest,
    ) -> Result<Vec<Location>, LanguageError> {
        let (path, _) = self.documento(request.document_id)?;
        let (line, offset) = to_service(request.position);
        let resposta = self
            .session
            .request(
                "definition",
                serde_json::json!({
                    "file": path_argument(&path),
                    "line": line,
                    "offset": offset,
                }),
                REQUEST_TIMEOUT,
            )
            .await
            .map_err(|detalhe| self.failure(detalhe))?;
        Ok(resposta
            .as_array()
            .map(|itens| itens.iter().filter_map(location_from).collect())
            .unwrap_or_default())
    }

    async fn shutdown(&self) -> Result<(), LanguageError> {
        self.session.shutdown().await;
        Ok(())
    }
}

/// O caminho como o analisador espera recebê-lo.
///
/// Ele usa barra normal mesmo no Windows, e um caminho com contrabarra volta
/// como "arquivo não encontrado" — sem erro, só sem resposta.
fn path_argument(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

/// O domínio conta linha e coluna a partir de zero; o analisador, a partir de um.
///
/// Um deslocamento de uma linha aponta para o lugar errado sem erro nenhum a
/// apontar — a completação viria do que está acima do cursor.
const fn to_service(position: TextPosition) -> (u32, u32) {
    (position.line + 1, position.column + 1)
}

const fn from_service(line: u32, offset: u32) -> TextPosition {
    TextPosition {
        line: line.saturating_sub(1),
        column: offset.saturating_sub(1),
    }
}

fn position_from(valor: &serde_json::Value) -> Option<TextPosition> {
    let line = u32::try_from(valor.get("line")?.as_u64()?).ok()?;
    let offset = u32::try_from(valor.get("offset")?.as_u64()?).ok()?;
    Some(from_service(line, offset))
}

fn diagnostic_from(valor: &serde_json::Value) -> Option<Diagnostic> {
    let inicio = position_from(valor.get("start")?)?;
    let fim = position_from(valor.get("end")?).unwrap_or(inicio);
    Some(Diagnostic {
        range: TextRange {
            start: inicio,
            end: fim,
        },
        severity: match valor.get("category").and_then(serde_json::Value::as_str) {
            Some("warning") => DiagnosticSeverity::Warning,
            Some("suggestion") => DiagnosticSeverity::Hint,
            Some("message") => DiagnosticSeverity::Information,
            _ => DiagnosticSeverity::Error,
        },
        message: valor
            .get("text")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        source: Some("typescript".to_owned()),
    })
}

fn completion_from(valor: &serde_json::Value) -> Option<CompletionItem> {
    let label = valor.get("name")?.as_str()?.to_owned();
    let kind = match valor.get("kind").and_then(serde_json::Value::as_str) {
        Some("method" | "function" | "local function") => CompletionKind::Method,
        Some("property" | "getter" | "setter") => CompletionKind::Field,
        Some("class") => CompletionKind::Class,
        Some("interface") => CompletionKind::Interface,
        Some("enum") => CompletionKind::Enum,
        Some("constructor") => CompletionKind::Constructor,
        Some("keyword") => CompletionKind::Keyword,
        _ => CompletionKind::Variable,
    };
    Some(CompletionItem {
        label,
        detail: valor
            .get("kindModifiers")
            .and_then(serde_json::Value::as_str)
            .filter(|modificadores| !modificadores.is_empty())
            .map(str::to_owned),
        kind,
    })
}

fn location_from(valor: &serde_json::Value) -> Option<Location> {
    let inicio = position_from(valor.get("start")?)?;
    let fim = position_from(valor.get("end")?).unwrap_or(inicio);
    Some(Location {
        path: PathBuf::from(valor.get("file")?.as_str()?),
        range: TextRange {
            start: inicio,
            end: fim,
        },
    })
}

/// Aplica a mudança ao texto guardado.
///
/// Intervalo ausente é substituição do documento inteiro, que é como o host
/// sincroniza depois de contrapressão (ADR-017).
fn apply(current: &str, range: Option<TextRange>, text: &str) -> String {
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
