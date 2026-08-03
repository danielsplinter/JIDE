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
    ProviderId, SemanticSymbol, SymbolKind, TextPosition, TextRange,
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

/// Quanto se espera enquanto o analisador ainda monta o projeto.
///
/// **Ele não responde nada até terminar.** Os pedidos que chegam durante a carga
/// ficam enfileirados e são respondidos todos juntos no fim; medido contra um
/// projeto Angular de 8 958 arquivos, isso levou 30 s. Com o prazo curto, toda
/// busca feita nesse intervalo virava "não respondeu a tempo" — que numa busca é
/// indistinguível de "não achei nada". Era o que acontecia com a primeira busca
/// depois de abrir a IDE.
///
/// Só a **busca pelo projeto** ganha este prazo, e por três razões: quem a pediu
/// está esperando por ela, ela roda fora da thread da interface, e ela é
/// cancelável — o giro na tela é o que torna a espera honesta em vez de um
/// travamento. Completação continua com o prazo curto: ali quem digita segue
/// digitando, e uma lista que aparecesse trinta segundos depois seria pior do
/// que nenhuma.
const LOADING_TIMEOUT: Duration = Duration::from_secs(120);

/// Teto de memória do processo do analisador, em megabytes.
///
/// **É orçamento imposto, e não sofrido.** A `08` trata memória como requisito
/// arquitetural com orçamento explícito, e este é o primeiro lugar da IDE onde
/// existe um número a impor: o analisador carrega a árvore de todo arquivo do
/// programa mais os `.d.ts` alcançados, e num monorepo isso chega aos
/// gigabytes — é por isso que editores expõem teto configurável para ele.
///
/// Ultrapassar o teto derruba o processo, e derrubar o processo é a queda para o
/// nativo que a fase 3b construiu: a IDE continua respondendo, com menos.
/// Deixar sem teto trocaria isso por a máquina inteira paginar.
///
/// 2 GB é folgado para projeto de aplicação e aperta em monorepo grande, que é
/// justamente onde se quer que aperte. O número não está ligado a um
/// `MemoryBudget` — ele ainda não existe em código, e a `08` registra a
/// pendência.
const MAX_HEAP_MB: u32 = 2048;

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
        Ok(Box::new(self.start(context).await?))
    }
}

impl TypeScriptServiceProvider {
    /// Sobe o analisador e devolve o tipo concreto.
    ///
    /// `activate` embrulha isto. Existe separado porque há o que perguntar ao
    /// serviço que **não** está no contrato — a lista de arquivos do projeto,
    /// que serve para conferir o nosso leitor de `tsconfig.json` e não para a
    /// IDE consumir.
    pub(crate) async fn start(
        &self,
        context: LanguageActivationContext,
    ) -> Result<ActiveTypeScriptService, LanguageError> {
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
                    format!("--max-old-space-size={MAX_HEAP_MB}"),
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

        Ok(ActiveTypeScriptService {
            language_id: LanguageId(TYPESCRIPT_LANGUAGE_ID.to_owned()),
            session: Session::new(Arc::from(conversa)),
            documentos: Mutex::new(HashMap::new()),
        })
    }
}

pub(crate) struct ActiveTypeScriptService {
    language_id: LanguageId,
    session: Session,
    /// Caminho e texto de cada documento aberto.
    ///
    /// O texto fica aqui porque a mudança vai por intervalo: é preciso ter o
    /// texto anterior para saber se o intervalo cabe nele, e para reabrir com o
    /// texto inteiro quando não cabe.
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

    /// Caminho e texto de tudo o que está aberto, para converter posições.
    ///
    /// Enquanto se digita, o buffer não é o que está no disco; ler o arquivo para
    /// converter a coluna de uma posição no próprio arquivo aberto daria a linha
    /// errada. Registro travado devolve lista vazia: a conversão cai para a
    /// aproximação, e não para o erro.
    fn abertos(&self) -> Vec<(PathBuf, String)> {
        self.documentos
            .lock()
            .map(|registro| registro.values().cloned().collect())
            .unwrap_or_default()
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

    /// Reescreve um intervalo do arquivo aberto no analisador.
    ///
    /// `change` também não tem resposta no protocolo. Isso é o que a torna
    /// barata e é o que a torna perigosa: um intervalo errado não volta como
    /// erro, volta como respostas erradas daí em diante.
    async fn mudar(
        &self,
        path: &Path,
        atual: &str,
        range: TextRange,
        texto: &str,
    ) -> Result<(), LanguageError> {
        self.session
            .notify("change", change_arguments(path, atual, range, texto))
            .await
            .map_err(|detalhe| self.failure(detalhe))
    }
}

#[async_trait]
impl ActiveLanguage for ActiveTypeScriptService {
    fn language_id(&self) -> &LanguageId {
        &self.language_id
    }

    /// Este analisador tem, sim, o que preparar: ele monta o projeto inteiro
    /// antes de responder qualquer coisa. Ver `LOADING_TIMEOUT`.
    fn readiness(&self) -> Option<ide_language_api::ReadinessSignal> {
        Some(self.session.readiness())
    }

    async fn open_document(&self, document: DocumentSnapshot) -> Result<(), LanguageError> {
        self.abrir(&document.path, &document.text).await?;
        self.documentos
            .lock()
            .map_err(|_| LanguageError::Provider("registro de documentos travado".to_owned()))?
            .insert(document.id, (document.path, document.text));
        Ok(())
    }

    /// Manda só o que mudou, e reabre o arquivo quando não pode confiar no
    /// intervalo.
    ///
    /// Reabrir a cada tecla mandava o arquivo inteiro pelo cano — num arquivo de
    /// 3 000 linhas, isso a cada caractere digitado. O intervalo manda os bytes
    /// que mudaram.
    ///
    /// **A troca só é segura porque a conversão de posição já estava provada.**
    /// `to_service` é a mesma que carrega completação e definição contra o
    /// analisador de verdade desde a fase 3c: se ela errasse por um, aqueles
    /// testes já teriam falhado. Errar aqui é pior do que errar lá — uma
    /// completação errada aparece na tela, um intervalo errado **reescreve o
    /// buffer no lugar errado** e envenena tudo o que vier depois, calado.
    ///
    /// Por isso a válvula: intervalo que não cabe no texto que temos significa
    /// que o nosso espelho e o editor discordam, e aí reabrir é o que
    /// ressincroniza os dois. Ver `cabe_no_texto`.
    async fn change_document(&self, change: DocumentChange) -> Result<(), LanguageError> {
        let (path, atual) = self.documento(change.document_id)?;
        let novo = apply(&atual, change.range, &change.text);
        match change.range.filter(|range| cabe_no_texto(&atual, *range)) {
            Some(range) => self.mudar(&path, &atual, range, &change.text).await?,
            // Sem intervalo é substituição do documento inteiro (ADR-017), e
            // intervalo que não cabe é desconfiança: os dois pedem o texto todo.
            None => self.abrir(&path, &novo).await?,
        }
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
        let (path, texto) = self.documento(document_id)?;
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
            .map(|itens| {
                itens
                    .iter()
                    .filter_map(|item| diagnostic_from(item, &texto))
                    .collect()
            })
            .unwrap_or_default())
    }

    async fn completion(
        &self,
        request: CompletionRequest,
    ) -> Result<Vec<CompletionItem>, LanguageError> {
        let (path, texto) = self.documento(request.document_id)?;
        let (line, offset) = to_service(&texto, request.position);
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
        let (path, texto) = self.documento(request.document_id)?;
        let (line, offset) = to_service(&texto, request.position);
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
            .map(|itens| {
                let mut textos = Textos::com_abertos(self.abertos());
                itens
                    .iter()
                    .filter_map(|item| location_from(item, &mut textos))
                    .collect()
            })
            .unwrap_or_default())
    }

    /// Tipos do projeto cujo nome casa com o que foi digitado.
    ///
    /// É o `navto` do analisador — a mesma pergunta que a busca por nome faz, e
    /// que o provider nativo não sabe responder: sem índice, ele só conhece o
    /// arquivo aberto.
    ///
    /// Só entram símbolos com arquivo, porque o resultado existe para ser
    /// aberto, e um tipo declarado dentro de um `.d.ts` de dependência não tem
    /// onde ser aberto de forma útil.
    async fn workspace_types(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<SemanticSymbol>, LanguageError> {
        // Consulta vazia devolve tudo o que couber, para a janela ter o que
        // mostrar antes da primeira letra — é o que o contrato da `03` pede.
        let identificador = search_value(query);
        let busca = if identificador.is_empty() {
            "*"
        } else {
            identificador.as_str()
        };
        let prazo = if self.session.is_ready() {
            REQUEST_TIMEOUT
        } else {
            LOADING_TIMEOUT
        };
        let resposta = self
            .session
            .request(
                "navto",
                serde_json::json!({
                    "searchValue": busca,
                    "maxResultCount": limit,
                }),
                prazo,
            )
            .await
            .map_err(|detalhe| self.failure(detalhe))?;
        Ok(resposta
            .as_array()
            .map(|itens| {
                let mut textos = Textos::com_abertos(self.abertos());
                itens
                    .iter()
                    .filter_map(|item| symbol_from(item, &mut textos))
                    .collect()
            })
            .unwrap_or_default())
    }

    async fn shutdown(&self) -> Result<(), LanguageError> {
        self.session.shutdown().await;
        Ok(())
    }
}

fn symbol_from(valor: &serde_json::Value, textos: &mut Textos) -> Option<SemanticSymbol> {
    let name = valor.get("name")?.as_str()?.to_owned();
    let kind = match valor.get("kind").and_then(serde_json::Value::as_str)? {
        "class" | "local class" => SymbolKind::Class,
        "interface" => SymbolKind::Interface,
        "enum" => SymbolKind::Enum,
        "type" | "alias" => SymbolKind::Class,
        // Só tipo entra na busca por tipo: função e variável soltas encheriam a
        // lista com o que a pergunta não é.
        _ => return None,
    };
    let path = PathBuf::from(valor.get("file")?.as_str()?);
    let texto = textos.de(&path).map(str::to_owned);
    let inicio = position_from(valor.get("start")?, texto.as_deref())?;
    let fim = position_from(valor.get("end")?, texto.as_deref()).unwrap_or(inicio);
    Some(SemanticSymbol {
        name,
        kind,
        location: Location {
            path,
            range: TextRange {
                start: inicio,
                end: fim,
            },
        },
        type_descriptor: None,
        scope_depth: 0,
    })
}

impl ActiveTypeScriptService {
    /// Quais arquivos o **analisador** considera parte do projeto.
    ///
    /// Serve para conferir contra o que o nosso leitor de `tsconfig.json`
    /// concluiu. A ADR-027 diz que a origem é o arquivo, e não um processo — os
    /// dois leem o mesmo `tsconfig.json`, e o nosso é aproximado.
    ///
    /// Errar contra a mesma fonte é defeito com forma conhecida e testável;
    /// duas definições diferentes seriam desacordo por desenho, que nenhum teste
    /// apanha porque os dois lados estão certos. Isto é o que torna a diferença
    /// visível.
    ///
    /// **Só existe no teste**, e é coerente: a IDE não consome esta lista — ela
    /// usa a nossa, lida do `tsconfig.json`. Perguntar ao processo seria a
    /// dependência que a ADR-027 recusa.
    #[cfg(test)]
    pub(crate) async fn project_files(
        &self,
        document_id: DocumentId,
    ) -> Result<Vec<PathBuf>, LanguageError> {
        let (path, _) = self.documento(document_id)?;
        let resposta = self
            .session
            .request(
                "projectInfo",
                serde_json::json!({
                    "file": path_argument(&path),
                    "needFileNameList": true,
                }),
                REQUEST_TIMEOUT,
            )
            .await
            .map_err(|detalhe| self.failure(detalhe))?;
        Ok(resposta
            .get("fileNames")
            .and_then(serde_json::Value::as_array)
            .map(|nomes| {
                nomes
                    .iter()
                    .filter_map(serde_json::Value::as_str)
                    .map(PathBuf::from)
                    .collect()
            })
            .unwrap_or_default())
    }
}

/// A consulta como o `navto` quer recebê-la: um identificador, sem separadores.
///
/// # Isto foi medido, e o número é gritante
///
/// Contra um projeto Angular de verdade, procurando `federated-login-context`:
///
/// | consulta enviada | devolvidos | corretos |
/// | --- | --- | --- |
/// | `federated-login-context` | 100 | **0** |
/// | `federated login context` | 100 | **0** |
/// | `federatedlogincontext` | 6 | **6** |
///
/// **O `navto` trata o separador como quebra e responde com o que casa com
/// qualquer pedaço.** Num projeto grande isso enche o limite inteiro com toda
/// variável chamada `context` antes de chegar perto do que se procurou — e o que
/// se via na tela eram arquivos com `login` no nome, que era o segundo pedaço.
///
/// Juntar os pedaços num identificador devolve a busca por subsequência em
/// `CamelCase`, que é a que serve. `Pedido` continua `Pedido`: consulta de uma
/// palavra só passa por aqui sem mudar.
///
/// Fica **aqui**, e não na aplicação, porque é quirk deste analisador. Impor a
/// outras linguagens o formato que o `navto` prefere seria vazar TypeScript para
/// dentro da IDE, que é o que a `23` existe para impedir.
fn search_value(query: &str) -> String {
    query.chars().filter(|c| c.is_alphanumeric()).collect()
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
///
/// # E a coluna não conta a mesma coisa nos dois lados
///
/// **Nós contamos caractere; o analisador conta unidade UTF-16.** Um emoji vale
/// um caractere e duas unidades, e tudo o que vier depois dele na linha tem
/// coluna diferente nas duas contagens. Por isso o texto entra aqui: a conversão
/// não é aritmética, é uma releitura da linha.
///
/// Isto foi **medido**, e não deduzido: uma sondagem trocou a primeira letra de
/// um membro numa linha com emoji, e a troca caiu no espaço anterior — o membro
/// virou `Xdesconto` em vez de `Xesconto`. Ver a sondagem em `tests/service.rs`.
fn to_service(texto: &str, position: TextPosition) -> (u32, u32) {
    let unidades = texto
        .split_inclusive('\n')
        .nth(position.line as usize)
        .map_or(position.column, |linha| {
            let linha = linha.strip_suffix('\n').unwrap_or(linha);
            let linha = linha.strip_suffix('\r').unwrap_or(linha);
            let dentro: u32 = linha
                .chars()
                .take(position.column as usize)
                .map(|caractere| caractere.len_utf16() as u32)
                .sum();
            // O que passa do fim da linha atravessa sem tradução, como faz o
            // caminho de volta. Truncar aqui e preservar lá tornaria as duas
            // conversões não inversas, e é o que o teste de ida e volta pega.
            dentro + position.column.saturating_sub(linha.chars().count() as u32)
        });
    (position.line + 1, unidades + 1)
}

/// O caminho de volta, com o mesmo desencontro de contagem.
///
/// O analisador devolve a coluna em **unidade UTF-16**; o domínio a quer em
/// **caractere**. Sem o texto, só dá para desfazer a base um — o que está certo
/// para toda linha sem caractere astral e erra para adiante nas outras, deixando
/// o realce de um diagnóstico deslocado.
///
/// Por isso o texto é opcional aqui, e não obrigatório como na ida: numa
/// definição que leva a um arquivo ilegível, ficar sem posição nenhuma seria pior
/// do que ficar com uma coluna adiante.
fn from_service(texto: Option<&str>, line: u32, offset: u32) -> TextPosition {
    let unidades = offset.saturating_sub(1);
    let linha = line.saturating_sub(1);
    let coluna = texto
        .and_then(|texto| texto.split_inclusive('\n').nth(linha as usize))
        .map_or(unidades, |conteudo| {
            let conteudo = conteudo.strip_suffix('\n').unwrap_or(conteudo);
            let conteudo = conteudo.strip_suffix('\r').unwrap_or(conteudo);
            coluna_de_unidades(conteudo, unidades)
        });
    TextPosition {
        line: linha,
        column: coluna,
    }
}

/// Quantos caracteres cabem antes de tantas unidades UTF-16.
///
/// O que sobra é somado de volta: uma coluna além do fim da linha — que o
/// analisador usa para marcar o fim de um intervalo — tem de continuar depois de
/// todas as outras, e não grudar no último caractere.
fn coluna_de_unidades(linha: &str, unidades: u32) -> u32 {
    let mut restantes = unidades;
    let mut coluna = 0;
    for caractere in linha.chars() {
        let tamanho = caractere.len_utf16() as u32;
        if restantes < tamanho {
            break;
        }
        restantes -= tamanho;
        coluna += 1;
    }
    coluna + restantes
}

fn position_from(valor: &serde_json::Value, texto: Option<&str>) -> Option<TextPosition> {
    let line = u32::try_from(valor.get("line")?.as_u64()?).ok()?;
    let offset = u32::try_from(valor.get("offset")?.as_u64()?).ok()?;
    Some(from_service(texto, line, offset))
}

/// Os textos dos arquivos que aparecem numa resposta.
///
/// Converter a coluna pede **a linha**, e uma definição leva a qualquer arquivo do
/// projeto. O que está aberto vem do espelho — e tem de vir de lá, porque o buffer
/// sendo editado não é o que está no disco. O resto é lido, uma vez só por
/// resposta: uma definição costuma trazer um resultado, e a leitura que falha
/// devolve a coluna sem conversão em vez de descartar a posição.
struct Textos {
    conhecidos: HashMap<PathBuf, Option<String>>,
}

impl Textos {
    fn com_abertos(abertos: Vec<(PathBuf, String)>) -> Self {
        Self {
            conhecidos: abertos
                .into_iter()
                .map(|(caminho, texto)| (caminho, Some(texto)))
                .collect(),
        }
    }

    fn de(&mut self, path: &Path) -> Option<&str> {
        if !self.conhecidos.contains_key(path) {
            self.conhecidos
                .insert(path.to_path_buf(), std::fs::read_to_string(path).ok());
        }
        self.conhecidos.get(path)?.as_deref()
    }
}

fn diagnostic_from(valor: &serde_json::Value, texto: &str) -> Option<Diagnostic> {
    let inicio = position_from(valor.get("start")?, Some(texto))?;
    let fim = position_from(valor.get("end")?, Some(texto)).unwrap_or(inicio);
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

fn location_from(valor: &serde_json::Value, textos: &mut Textos) -> Option<Location> {
    let path = PathBuf::from(valor.get("file")?.as_str()?);
    let texto = textos.de(&path).map(str::to_owned);
    let inicio = position_from(valor.get("start")?, texto.as_deref())?;
    let fim = position_from(valor.get("end")?, texto.as_deref()).unwrap_or(inicio);
    Some(Location {
        path,
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

/// Os argumentos de uma mudança por intervalo.
///
/// Isto é função pura de propósito. Mandar intervalo e reabrir o arquivo inteiro
/// deixam o analisador **no mesmo estado**, e por isso nenhum teste de ponta a
/// ponta consegue dizer qual dos dois rodou — um teste que passasse "porque a
/// completação funcionou" passaria igual com o caminho caro. O que dá para
/// provar em separado é a **decisão** (`cabe_no_texto`) e o **payload** (aqui);
/// o de ponta a ponta prova que o conjunto funciona contra o analisador de
/// verdade.
fn change_arguments(
    path: &Path,
    atual: &str,
    range: TextRange,
    texto: &str,
) -> serde_json::Value {
    let (line, offset) = to_service(atual, range.start);
    let (end_line, end_offset) = to_service(atual, range.end);
    serde_json::json!({
        "file": path_argument(path),
        "line": line,
        "offset": offset,
        "endLine": end_line,
        "endOffset": end_offset,
        "insertString": texto,
    })
}

/// Se o intervalo existe mesmo no texto que temos.
///
/// É a válvula da mudança incremental. `apply` **acomoda** intervalo inválido
/// grudando o texto no fim da linha, o que sempre produziu alguma coisa; mandar
/// esse intervalo ao analisador produziria um buffer diferente do nosso, e nada
/// avisaria. Quando não cabe, o caminho é reabrir com o texto inteiro, que
/// ressincroniza os dois lados.
fn cabe_no_texto(texto: &str, range: TextRange) -> bool {
    let cabe = |posicao: ide_domain::TextPosition| {
        texto
            .split_inclusive('\n')
            .nth(posicao.line as usize)
            .is_some_and(|linha| {
                let linha = linha.strip_suffix('\n').unwrap_or(linha);
                let linha = linha.strip_suffix('\r').unwrap_or(linha);
                // Igual ao comprimento é o fim da linha, que é posição válida.
                posicao.column as usize <= linha.chars().count()
            })
    };
    (range.start.line, range.start.column) <= (range.end.line, range.end.column)
        && cabe(range.start)
        && cabe(range.end)
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

#[cfg(test)]
mod tests {
    use super::*;
    use ide_process::NativeProcessSupervisor;

    fn intervalo(inicio: (u32, u32), fim: (u32, u32)) -> TextRange {
        TextRange {
            start: TextPosition { line: inicio.0, column: inicio.1 },
            end: TextPosition { line: fim.0, column: fim.1 },
        }
    }

    /// Um intervalo dentro do texto pode ir por mudança incremental.
    #[test]
    fn a_range_inside_the_text_fits() {
        let texto = "class Pedido {\n  total = 0;\n}\n";
        assert!(cabe_no_texto(texto, intervalo((1, 2), (1, 7))));
        // O fim da linha é posição válida: é onde se digita.
        assert!(cabe_no_texto(texto, intervalo((1, 12), (1, 12))));
    }

    /// Linha que não existe não vai por intervalo.
    ///
    /// Mandar isto ao analisador deixaria o buffer dele diferente do nosso, e
    /// **nada avisaria** — as respostas seguintes viriam erradas, calado. Por
    /// isso o caminho passa a ser reabrir o arquivo inteiro.
    #[test]
    fn a_range_past_the_end_does_not_fit() {
        let texto = "class Pedido {\n}\n";
        assert!(!cabe_no_texto(texto, intervalo((9, 0), (9, 0))));
        assert!(!cabe_no_texto(texto, intervalo((0, 0), (0, 99))));
    }

    /// Intervalo invertido não vai por intervalo.
    #[test]
    fn a_backwards_range_does_not_fit() {
        let texto = "class Pedido {\n}\n";
        assert!(!cabe_no_texto(texto, intervalo((1, 0), (0, 0))));
    }

    /// O pedido de mudança leva as duas pontas do intervalo, contadas a partir de
    /// um.
    ///
    /// `change` não tem resposta no protocolo: um argumento errado não volta como
    /// erro, volta como buffer envenenado e respostas erradas depois. Por isso o
    /// payload é conferido aqui, e não só pelo efeito.
    #[test]
    fn the_change_request_carries_both_ends_of_the_range() {
        let argumentos = change_arguments(
            Path::new("/projeto/pedido.ts"),
            "class Pedido { total = 0; }\n  const x = 1;\n",
            intervalo((0, 25), (1, 3)),
            " desconto = 0;",
        );
        assert_eq!(argumentos["file"], "/projeto/pedido.ts");
        assert_eq!(argumentos["line"], 1);
        assert_eq!(argumentos["offset"], 26);
        assert_eq!(argumentos["endLine"], 2);
        assert_eq!(argumentos["endOffset"], 4);
        assert_eq!(argumentos["insertString"], " desconto = 0;");
    }

    /// A consulta vira identificador antes de ir ao analisador.
    ///
    /// Medido: com o separador, 100 resultados e nenhum certo; sem ele, 6 e todos
    /// certos. Ver `search_value`.
    #[test]
    fn the_query_becomes_an_identifier_before_it_is_sent() {
        assert_eq!(search_value("federated-login-context"), "federatedlogincontext");
        assert_eq!(search_value("federated login context"), "federatedlogincontext");
        assert_eq!(search_value("FederatedLoginContext"), "FederatedLoginContext");
        // Uma palavra só atravessa sem mudar, que é o caso mais comum.
        assert_eq!(search_value("Pedido"), "Pedido");
        assert_eq!(search_value("---"), "");
    }

    /// A conversão de posição é a mesma que a completação usa.
    ///
    /// É o argumento inteiro de a mudança incremental ser segura: se `to_service`
    /// errasse por um, os testes de completação contra o analisador de verdade já
    /// teriam falhado desde a fase 3c.
    #[test]
    fn the_position_conversion_is_one_based() {
        let texto = "linha zero\nlinha um\n";
        assert_eq!(to_service(texto, TextPosition { line: 0, column: 0 }), (1, 1));
        assert_eq!(to_service(texto, TextPosition { line: 1, column: 5 }), (2, 6));
    }

    /// O caminho de volta desfaz exatamente a ida.
    ///
    /// Ida e volta usam o mesmo texto, e uma posição que sai e entra tem de
    /// voltar a ser ela mesma. É o que impede as duas conversões de divergirem
    /// quando uma for mexida — o desencontro entre elas foi o defeito inteiro.
    #[test]
    fn the_return_path_undoes_the_outgoing_one() {
        let texto = "const e = \"\u{1F642}\"; class Pedido { desconto = 0; }\nsegunda linha\n";
        for linha in 0..2u32 {
            for coluna in 0..20u32 {
                let posicao = TextPosition { line: linha, column: coluna };
                let (l, o) = to_service(texto, posicao);
                assert_eq!(
                    from_service(Some(texto), l, o),
                    posicao,
                    "ida e volta precisam fechar em {posicao:?}"
                );
            }
        }
    }

    /// Sem o texto, a volta só desfaz a base um.
    ///
    /// É a aproximação de sempre, e continua certa para toda linha sem caractere
    /// astral. Vale mais do que descartar a posição de uma definição num arquivo
    /// que não deu para ler.
    #[test]
    fn without_the_text_the_return_path_only_undoes_the_one_base() {
        assert_eq!(
            from_service(None, 3, 8),
            TextPosition { line: 2, column: 7 }
        );
    }

    /// Uma coluna além do fim da linha continua além.
    ///
    /// O analisador marca o fim de um intervalo assim, e grudá-la no último
    /// caractere encolheria o realce de um diagnóstico em um.
    #[test]
    fn a_column_past_the_end_of_the_line_stays_past_it() {
        assert_eq!(coluna_de_unidades("abc", 3), 3);
        assert_eq!(coluna_de_unidades("abc", 9), 9);
        // O emoji vale duas unidades e um caractere.
        assert_eq!(coluna_de_unidades("\u{1F642}!", 3), 2);
    }

    /// Um emoji antes da coluna a desloca, porque o analisador conta UTF-16.
    ///
    /// Isto foi medido contra o analisador de verdade: sem a conversão, a troca
    /// de um caractere caía no anterior, e o membro trocado continuava existindo.
    #[test]
    fn an_astral_character_shifts_the_column() {
        // Nada de astral antes: caractere e unidade contam igual.
        let simples = "const e = \"X\";\n";
        assert_eq!(to_service(simples, TextPosition { line: 0, column: 7 }), (1, 8));

        // Doze caracteres até ali, e treze unidades: o emoji conta duas vezes.
        let com_emoji = "const e = \"\u{1F642}\";\n";
        assert_eq!(
            to_service(com_emoji, TextPosition { line: 0, column: 12 }),
            (1, 14)
        );
    }

    /// A nossa lista de arquivos do projeto bate com a do analisador.
    ///
    /// É o fecho da ADR-027, e o teste que ela pediu pelo nome. A origem é o
    /// **arquivo**: nós lemos o `tsconfig.json` e o analisador lê o mesmo
    /// `tsconfig.json`. O nosso leitor é aproximado e o dele é exato, e é por
    /// isso que a divergência é **defeito nosso** — mas ela só é defeito porque
    /// existe um lugar que a mostra, e é este.
    ///
    /// Sem este teste, "as duas listas batem" seria uma frase na especificação.
    #[test]
    #[ignore = "exige Node instalado e `npm install typescript` no projeto de teste"]
    fn our_file_list_matches_the_analyzer() {
        let root = std::env::temp_dir().join(format!("er-adr027-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let fonte = root.join("fonte");
        assert!(std::fs::create_dir_all(&fonte).is_ok());
        assert!(std::fs::write(root.join("package.json"), r#"{"name":"t"}"#).is_ok());
        assert!(
            std::fs::write(
                root.join("tsconfig.json"),
                r#"{ "include": ["fonte/**/*"], "compilerOptions": { "outDir": "saida" } }"#,
            )
            .is_ok()
        );
        let arquivo = fonte.join("pedido.ts");
        assert!(std::fs::write(&arquivo, "export class Pedido {}\n").is_ok());

        // No Windows o npm é um `.cmd`, e não um executável.
        #[cfg(windows)]
        const NPM: &str = "npm.cmd";
        #[cfg(not(windows))]
        const NPM: &str = "npm";
        let instalado = std::process::Command::new(NPM)
            .args(["install", "typescript@5", "--no-audit", "--no-fund"])
            .current_dir(&root)
            .status();
        assert!(instalado.is_ok_and(|status| status.success()));

        let runtime = match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(runtime) => runtime,
            Err(erro) => panic!("runtime de teste: {erro}"),
        };
        let provider = TypeScriptServiceProvider::new(Arc::new(NativeProcessSupervisor::default()));
        let servico = match runtime.block_on(provider.start(LanguageActivationContext {
            workspace_root: root.clone(),
            source_roots: Vec::new(),
            toolchains: Vec::new(),
        })) {
            Ok(servico) => servico,
            Err(erro) => panic!("o analisador precisa subir: {erro}"),
        };

        let documento = DocumentSnapshot {
            id: DocumentId(1),
            path: arquivo,
            version: 1,
            text: "export class Pedido {}\n".to_owned(),
        };
        assert!(runtime.block_on(servico.open_document(documento)).is_ok());
        // O analisador precisa de um instante para montar o projeto.
        std::thread::sleep(Duration::from_secs(2));

        let dele = match runtime.block_on(servico.project_files(DocumentId(1))) {
            Ok(arquivos) => arquivos,
            Err(erro) => panic!("o analisador precisa listar o projeto: {erro}"),
        };
        // Os `.d.ts` da biblioteca padrão entram na lista dele e não na nossa:
        // eles não são fonte do projeto. O que se compara é o que está **sob as
        // nossas raízes**, que é onde as duas leituras têm de concordar.
        let nossas = crate::tsconfig::load(&root.join("tsconfig.json"))
            .map(|config| config.source_roots())
            .unwrap_or_default();
        assert_eq!(nossas, vec![fonte.clone()], "a nossa raiz sai do tsconfig");

        let sob_a_raiz: Vec<_> = dele.iter().filter(|caminho| dentro(caminho, &nossas)).collect();
        assert!(
            !sob_a_raiz.is_empty(),
            "o arquivo sob a nossa raiz precisa estar na lista do analisador; ele viu: {dele:?}"
        );

        runtime.block_on(servico.session.shutdown());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Compara caminhos como os dois lados os escrevem.
    ///
    /// O analisador usa barra normal e nós usamos a do sistema; e o Windows não
    /// distingue maiúsculas. Comparar cru diria que dois caminhos iguais são
    /// diferentes.
    fn dentro(caminho: &std::path::Path, raizes: &[PathBuf]) -> bool {
        let normalizar = |valor: &std::path::Path| {
            valor.to_string_lossy().to_lowercase().replace('\\', "/")
        };
        let alvo = normalizar(caminho);
        raizes.iter().any(|raiz| alvo.starts_with(&normalizar(raiz)))
    }
}
