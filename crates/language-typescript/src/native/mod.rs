//! O provider nativo de TypeScript.
//!
//! # Por que ele não mora em `analyzer`
//!
//! Ele **compõe** duas coisas de naturezas diferentes: a análise, que responde
//! sobre texto, e a resolução de módulos, que depende do `tsconfig.json`. O
//! `analyzer` promete não alcançar projeto, e a promessa vale — quem alcança é
//! quem compõe, que é aqui. Ver a fase 3 da `25`.
//!
//! Ele responde por **texto**, e não por tipos: realce, estrutura e erro de
//! sintaxe. Tipo é o que o analisador externo traz, na fase 3 da `23` — e este
//! provider não sai quando ele chegar. É o chão: sem Node instalado, sem o
//! pacote `typescript` no projeto, ou com o processo morto, é ele que responde.
//! Ver a ADR-025.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use async_trait::async_trait;
use ide_domain::{
    DefinitionRequest, Diagnostic, DocumentChange, DocumentId, DocumentSnapshot, LanguageId,
    Location, ProviderId, SemanticSymbol, SyntaxSnapshot,
};
use ide_language_api::{
    ReadinessSignal,
    ActiveLanguage, LANGUAGE_API_VERSION, LanguageActivationContext, LanguageCapabilities,
    LanguageError, LanguageMetadata, LanguageProvider,
};

use crate::analyzer::index::WorkspaceIndex;
use crate::analyzer::references;
use crate::modules::{ModuleResolver, Reexportacao, declarante};
use crate::analyzer::parser::TypeScriptParser;
use crate::analyzer::syntax;
use crate::analyzer::TYPESCRIPT_LANGUAGE_ID;

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
            | LanguageCapabilities::DEFINITION
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
    /// De que arquivo ele veio.
    ///
    /// A navegação parte daqui: o `import` é relativo ao arquivo que o escreve,
    /// e sem saber qual é não há de onde resolver.
    path: PathBuf,
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
    /// Para onde cada `import` do projeto aponta.
    ///
    /// Ele é do **projeto** — depende do `tsconfig.json` —, e é por isto que
    /// este provider não mora no `analyzer`. Ver a fase 3 da `25`.
    resolver: ModuleResolver,
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

        // O `tsconfig.json` da raiz é a origem do que `paths` e `baseUrl` valem
        // (ADR-027). Sem ele, sobram as importações relativas — que continuam
        // funcionando, e é a degradação certa: menos resposta, nunca resposta
        // errada.
        let resolver = ModuleResolver::new(
            &crate::project::tsconfig::load(&context.workspace_root.join("tsconfig.json"))
                .unwrap_or_default(),
        );

        Ok(Self {
            language_id: LanguageId(TYPESCRIPT_LANGUAGE_ID.to_owned()),
            parser: TypeScriptParser::new()?,
            documents: Mutex::new(HashMap::new()),
            index,
            readiness,
            resolver,
        })
    }

    fn analyze(
        &self,
        document_id: DocumentId,
        path: &Path,
        version: u64,
        text: &str,
    ) -> Result<ParsedDocument, LanguageError> {
        let tree = self.parser.parse(text, None)?;
        let pass = syntax::analyze(&tree, text);
        Ok(ParsedDocument {
            path: path.to_path_buf(),
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
        let parsed = self.analyze(document.id, &document.path, document.version, &document.text)?;
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
            (current.path.clone(), apply(&current.text, change.range, &change.text))
        };
        let (caminho, text) = text;
        let parsed = self.analyze(change.document_id, &caminho, change.version, &text)?;
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

    /// Onde o nome sob o cursor é declarado.
    ///
    /// # A pergunta não é "quem se chama assim"
    ///
    /// Em Java, pacote e classpath tornam um nome globalmente resolvível.
    /// Em TypeScript quem decide é o **`import`**: dois módulos podem declarar
    /// `LoginService`, e abrir o primeiro que o índice achasse seria abrir o
    /// errado com a mesma cara de certo.
    ///
    /// Por isso o caminho é: o nome sai do texto, o `import` diz de que módulo
    /// ele vem, o resolvedor diz que arquivo é esse, e os barris são
    /// atravessados até quem declara.
    ///
    /// # Não achar é uma resposta
    ///
    /// Nome que vem de dependência instalada devolve lista vazia, e não erro:
    /// o índice responde pelo projeto. É o que o host precisa para encaminhar a
    /// pergunta a quem alcança mais — o analisador externo, quando houver.
    async fn definition(&self, request: DefinitionRequest) -> Result<Vec<Location>, LanguageError> {
        let (caminho, texto) = {
            let documentos = self
                .documents
                .lock()
                .map_err(|_| LanguageError::Provider("registro de documentos travado".to_owned()))?;
            let Some(parsed) = documentos.get(&request.document_id) else {
                return Ok(Vec::new());
            };
            (parsed.path.clone(), parsed.text.clone())
        };
        let Some(nome) = references::identificador_em(
            &self.parser,
            &texto,
            request.position.line,
            request.position.column,
        ) else {
            return Ok(Vec::new());
        };

        // O arquivo aberto responde pelo **texto do editor**, e não pelo do
        // disco: quem acabou de escrever a classe espera achá-la.
        let aqui = references::do_texto(&self.parser, &texto);
        if let Some(range) = aqui.declaracao(&nome) {
            return Ok(vec![Location {
                path: caminho,
                range,
            }]);
        }

        // O cursor **dentro de um `import`** decide sozinho: ali o módulo está
        // escrito na mesma linha, e procurar o nome pela lista acharia outro
        // homônimo importado de outro lugar no mesmo arquivo.
        let de_dentro = references::importacao_em(
            &self.parser,
            &texto,
            request.position.line,
            request.position.column,
        );
        // Fora dele, o nome que o destino conhece pode não ser o que este
        // arquivo usa: `import { Pedido as PedidoAntigo }` põe dois em jogo.
        let Some((la, especificador)) = de_dentro
            .as_ref()
            .map(|(nome, de)| (nome.clone(), de.as_str()))
            .or_else(|| aqui.origem(&nome))
        else {
            return Ok(Vec::new());
        };
        let Some(modulo) = self.resolver.resolve(&caminho, especificador) else {
            return Ok(Vec::new());
        };
        let parser = &self.parser;
        let exportacoes = |arquivo: &Path| {
            references::de_arquivo(parser, arquivo)
                .reexportados
                .into_iter()
                .map(|trazido| Reexportacao {
                    nome: trazido.usado,
                    origem: trazido.origem,
                    de: trazido.de,
                })
                .collect()
        };
        let declara = |arquivo: &Path, nome: &str| {
            references::de_arquivo(parser, arquivo)
                .declaracao(nome)
                .is_some()
        };
        let Some(destino) = declarante(&self.resolver, &modulo, &la, &exportacoes, &declara)
        else {
            return Ok(Vec::new());
        };
        let Some(range) = references::de_arquivo(parser, &destino).declaracao(&la) else {
            return Ok(Vec::new());
        };
        Ok(vec![Location {
            path: destino,
            range,
        }])
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
