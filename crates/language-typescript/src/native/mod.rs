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
    CompletionItem, CompletionRequest, DefinitionRequest, Diagnostic, DocumentChange, DocumentId,
    DocumentSnapshot, LanguageId, Location, ProviderId, SemanticSymbol, SyntaxSnapshot, TextRange,
};
use ide_language_api::{
    ReadinessSignal,
    ActiveLanguage, LANGUAGE_API_VERSION, LanguageActivationContext, LanguageCapabilities,
    LanguageError, LanguageMetadata, LanguageProvider,
};

use tree_sitter::{InputEdit, Point, Tree};

use crate::analyzer::index::WorkspaceIndex;
use crate::analyzer::members;
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
            // O ponto voltou a valer na fase 4 da `25`: há o que oferecer
            // quando o receptor tem tipo **declarado**, e quando não tem, a
            // resposta é dizer que não se sabe — e não uma lista adivinhada.
            trigger_characters: vec!['.'],
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
            | LanguageCapabilities::COMPLETION
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
    /// A árvore desta versão do texto.
    ///
    /// # Por que guardá-la, e não só o realce
    ///
    /// O tree-sitter existe para **reanalisar só o que mudou**, e para isso ele
    /// precisa da árvore anterior. Sem ela, cada tecla reconstrói o arquivo
    /// inteiro: medido num arquivo de 3 144 linhas, **45 ms por tecla** — 22
    /// quadros por segundo gastos só em recolorir o que não mudou.
    tree: Tree,
    /// Quando a lista de símbolos deste documento foi montada.
    ///
    /// Ela acompanha o texto **em repouso**, e não a cada tecla: enquanto as
    /// mudanças chegam depressa, a lista anterior continua valendo. Ver
    /// [`INTERVALO_DA_ESTRUTURA`].
    estrutura_em: std::time::Instant,
    snapshot: SyntaxSnapshot,
}

/// De quanto em quanto tempo a lista de símbolos é refeita, no máximo.
///
/// # Por que tempo, e não "quando a estrutura mudou"
///
/// Saber se uma edição mexeu na estrutura custa quase o mesmo que refazê-la: é
/// preciso percorrer a árvore para descobrir. Adivinhar pelo texto editado —
/// "tem chave? tem `class`?" — seria heurística, e erraria calada.
///
/// O relógio não erra: ela pode ficar até este intervalo atrás do texto, e nunca
/// mais do que isso. Quem a lê é o painel de símbolos e uma contagem na barra de
/// estado; nenhum dos dois precisa dela na mesma tecla.
///
/// 150 ms é abaixo do que se percebe numa lista lateral, e acima do intervalo
/// entre teclas de quem digita depressa — que é onde a economia está.
const INTERVALO_DA_ESTRUTURA: std::time::Duration = std::time::Duration::from_millis(150);

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

    /// Os membros de um tipo, seguindo herança e atravessando módulos.
    ///
    /// **A herança não é enfeite**: num componente Angular, metade do que
    /// aparece depois de `this.` vem da classe de que ele herda. Uma lista sem
    /// isso parece certa e está incompleta, que é pior do que parecer curta.
    ///
    /// Devolve `None` quando não se acha onde o tipo é declarado — vindo de
    /// dependência instalada, por exemplo. Quem chama transforma isso na
    /// terceira resposta.
    fn membros_do_tipo(&self, caminho: &Path, texto: &str, tipo: &str) -> Option<Vec<CompletionItem>> {
        let mut itens: Vec<CompletionItem> = Vec::new();
        let mut vistos: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut fila = vec![(caminho.to_path_buf(), texto.to_owned(), tipo.to_owned(), 0usize)];
        let mut achou = false;
        // Herança em círculo não compila, mas um arquivo em edição pode tê-la; o
        // teto é a rede que impede a IDE de girar num texto que ninguém compilou.
        while let Some((arquivo, conteudo, nome, profundidade)) = fila.pop() {
            if profundidade > 16 || !vistos.insert(format!("{}#{nome}", arquivo.display())) {
                continue;
            }
            let Some(membros) = members::membros_de(&self.parser, &conteudo, &nome) else {
                // O tipo não está neste arquivo: ele vem de outro módulo, e o
                // caminho até lá é o mesmo da definição.
                if let Some(destino) = self.arquivo_que_declara(&arquivo, &conteudo, &nome)
                    && let Ok(outro) = std::fs::read_to_string(&destino)
                {
                    fila.push((destino, outro, nome, profundidade + 1));
                }
                continue;
            };
            achou = true;
            for item in membros.itens {
                if vistos.insert(format!("membro:{}", item.label)) {
                    itens.push(item);
                }
            }
            for herdado in membros.herda {
                fila.push((arquivo.clone(), conteudo.clone(), herdado, profundidade + 1));
            }
        }
        achou.then_some(itens)
    }

    /// Que arquivo declara um nome, visto de outro. É o caminho da fase 3.
    fn arquivo_que_declara(&self, de: &Path, texto: &str, nome: &str) -> Option<PathBuf> {
        let referencias = references::do_texto(&self.parser, texto);
        let (la, especificador) = referencias.origem(nome)?;
        let modulo = self.resolver.resolve(de, especificador)?;
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
        declarante(&self.resolver, &modulo, &la, &exportacoes, &declara)
    }

    /// O arquivo que um caminho entre aspas aponta, se ele existir.
    ///
    /// # Por que isto não é conhecimento de Angular
    ///
    /// O caso que trouxe isto foi o `templateUrl` de um componente, mas a regra
    /// não o menciona: **um texto literal que nomeia um arquivo vizinho leva a
    /// ele**. Vale para `styleUrls`, para um caminho escrito à mão, e para
    /// qualquer outro framework que use a mesma convenção.
    ///
    /// Resolver aqui, e não no analisador, é o que faz o clique responder na
    /// hora: o `tsserver` também sabe responder — verificado —, mas ele custa
    /// dezenas de segundos para subir, e este arquivo ou existe ou não existe.
    ///
    /// Só caminho **relativo**: um `'@algum/pacote'` é módulo, e módulo já tem
    /// dono no caminho de cima. Sem esta cerca, um `import` cairia aqui e
    /// deixaria de passar pela resolução que sabe atravessar barril.
    fn arquivo_vizinho(&self, de: &Path, literal: &str) -> Option<PathBuf> {
        if !literal.starts_with("./") && !literal.starts_with("../") {
            return None;
        }
        let candidato = de
            .parent()?
            .join(literal.replace('/', std::path::MAIN_SEPARATOR_STR));
        candidato.is_file().then_some(candidato)
    }

    fn analyze(
        &self,
        document_id: DocumentId,
        path: &Path,
        version: u64,
        text: &str,
        anterior: Option<&Tree>,
        estrutura_anterior: Option<(&[ide_domain::OutlineItem], std::time::Instant)>,
    ) -> Result<ParsedDocument, LanguageError> {
        let tree = self.parser.parse(text, anterior)?;
        // A lista anterior vale enquanto for recente. Sem ela — na abertura —
        // não há o que reaproveitar, e a primeira sempre é montada.
        let reaproveitar = estrutura_anterior
            .filter(|(_, em)| em.elapsed() < INTERVALO_DA_ESTRUTURA);
        let mut pass = syntax::analyze(&tree, text, reaproveitar.is_none());
        let estrutura_em = match reaproveitar {
            Some((anterior, em)) => {
                pass.outline = anterior.to_vec();
                em
            }
            None => std::time::Instant::now(),
        };
        Ok(ParsedDocument {
            path: path.to_path_buf(),
            text: text.to_owned(),
            tree: tree.clone(),
            estrutura_em,
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
        // Abrir não tem árvore anterior: é a primeira análise deste documento.
        let parsed = self.analyze(
            document.id,
            &document.path,
            document.version,
            &document.text,
            None,
            None,
        )?;
        self.store(document.id, parsed)
    }

    async fn change_document(&self, change: DocumentChange) -> Result<(), LanguageError> {
        let (caminho, anterior, estrutura, texto) = {
            let documents = self.documents.lock().map_err(|_| {
                LanguageError::Provider("TypeScript documents lock poisoned".to_owned())
            })?;
            let Some(current) = documents.get(&change.document_id) else {
                // Mudança de documento que não foi aberto aqui: ignorar é a
                // resposta certa, e não um erro — o host roteia por extensão e
                // um fechamento em corrida chega assim.
                return Ok(());
            };
            // **A árvore precisa ser avisada da edição antes de servir.** Ela
            // descreve o texto de antes; entregá-la sem o `InputEdit` faria o
            // tree-sitter reaproveitar nós em posições que mudaram, e o
            // resultado seria uma árvore errada **em silêncio** — realce e
            // navegação apontando para o lugar errado, sem erro nenhum.
            let editada = change.range.map(|range| {
                let mut copia = current.tree.clone();
                copia.edit(&edicao(&current.text, range, &change.text));
                copia
            });
            (
                current.path.clone(),
                editada,
                Some((current.snapshot.outline.clone(), current.estrutura_em)),
                apply(&current.text, change.range, &change.text),
            )
        };
        // Sem intervalo é substituição do documento inteiro (ADR-017): não há o
        // que reaproveitar, e reanalisar do zero é o certo.
        let parsed = self.analyze(
            change.document_id,
            &caminho,
            change.version,
            &texto,
            anterior.as_ref(),
            estrutura
                .as_ref()
                .map(|(itens, em)| (itens.as_slice(), *em)),
        )?;
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
            // Sem identificador, ainda pode haver **um texto literal** — o
            // `templateUrl` de um componente é o caso que trouxe isto.
            let Some(literal) = references::texto_literal_em(
                &self.parser,
                &texto,
                request.position.line,
                request.position.column,
            ) else {
                // Nem nome nem texto: o cursor está sobre pontuação ou espaço, e
                // ali **não há** o que abrir. Lista vazia é a verdade.
                //
                // Dizer "não sei" aqui faria cada clique perdido descer para o
                // analisador externo — que é a espera que a fase 5 da `25`
                // existe para evitar.
                return Ok(Vec::new());
            };
            if let Some(arquivo) = self.arquivo_vizinho(&caminho, &literal) {
                return Ok(vec![Location {
                    path: arquivo,
                    range: TextRange::default(),
                }]);
            }
            // Texto que não é arquivo vizinho pode ser especificador de módulo,
            // caminho de recurso ou coisa que só quem tem tipos sabe. **Não
            // sei** deixa a pergunta descer; lista vazia afirmaria que ali não
            // há destino nenhum.
            return Err(LanguageError::Unresolved(format!(
                "`{literal}` não é um arquivo vizinho"
            )));
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
            // O nome não é declarado aqui nem importado: pode ser global, de uma
            // dependência, ou de um `namespace`. **Não alcanço** é a resposta, e
            // ela faz a pergunta passar ao analisador — lista vazia afirmaria
            // que o nome não tem declaração nenhuma.
            return Err(LanguageError::Unresolved(format!(
                "não sei de onde vem `{nome}`"
            )));
        };
        let Some(modulo) = self.resolver.resolve(&caminho, especificador) else {
            return Err(LanguageError::Unresolved(format!(
                "não alcanço o módulo `{especificador}`"
            )));
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
            return Err(LanguageError::Unresolved(format!(
                "não achei onde `{la}` é declarado"
            )));
        };
        let Some(range) = references::de_arquivo(parser, &destino).declaracao(&la) else {
            return Err(LanguageError::Unresolved(format!(
                "não achei a declaração de `{la}`"
            )));
        };
        Ok(vec![Location {
            path: destino,
            range,
        }])
    }

    /// O que aparece depois do ponto, quando o receptor tem tipo declarado.
    ///
    /// # Três respostas, e não duas
    ///
    /// | situação | resposta |
    /// | --- | --- |
    /// | o tipo é conhecido | os membros dele |
    /// | o tipo é conhecido e não tem membros | lista vazia, que **afirma** isso |
    /// | o tipo não é conhecido | `Unavailable`, dizendo que não se sabe |
    ///
    /// A terceira é o ponto inteiro. `store.select(s).pipe(map(x => x.` exige
    /// instanciar genéricos e fazer o tipo voltar da assinatura para dentro da
    /// lambda — o verificador que a ADR-025 recusou. Responder lista vazia ali
    /// seria dizer "este tipo não tem membros", que é uma afirmação falsa, e é a
    /// família de defeito que esta IDE já encontrou várias vezes.
    ///
    /// Dizendo `Unavailable`, o host encaminha a pergunta a quem alcança mais —
    /// o analisador externo, quando houver.
    async fn completion(
        &self,
        request: CompletionRequest,
    ) -> Result<Vec<CompletionItem>, LanguageError> {
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
        let receptor = members::receptor_em(
            &self.parser,
            &texto,
            request.position.line,
            request.position.column,
        );
        let tipo = match receptor {
            members::Receptor::Tipo(tipo) => tipo,
            members::Receptor::Desconhecido => {
                // `Unresolved`, e não `Unavailable`: não saber o tipo de uma
                // expressão é um limite deste provider, e não a morte dele.
                // Dizendo `Unavailable`, ele se demitiria por admitir o que
                // sempre foi verdade — e o arquivo perderia o realce junto.
                return Err(LanguageError::Unresolved(
                    "não sei o tipo desta expressão".to_owned(),
                ));
            }
            // Sem ponto, a completação é por nome — e ela não é desta fase.
            members::Receptor::Nenhum => return Ok(Vec::new()),
        };

        let Some(itens) = self.membros_do_tipo(&caminho, &texto, &tipo) else {
            return Err(LanguageError::Unresolved(format!(
                "não sei onde `{tipo}` é declarado"
            )));
        };
        let prefixo = request.prefix.to_lowercase();
        Ok(itens
            .into_iter()
            .filter(|item| prefixo.is_empty() || item.label.to_lowercase().starts_with(&prefixo))
            .collect())
    }

    async fn syntax(
        &self,
        document_id: DocumentId,
        _visible: Option<ide_domain::TextRange>,
    ) -> Result<SyntaxSnapshot, LanguageError> {
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

/// A edição, como o tree-sitter a entende.
///
/// # As duas unidades, de novo
///
/// O `InputEdit` quer deslocamento em **bytes** e ponto com coluna em **bytes**;
/// o domínio conta colunas em **caracteres**. É a mesma armadilha que a `lines`
/// resolve na volta, e errar aqui não aparece como erro — aparece como árvore
/// certa para um texto que não é este.
fn edicao(atual: &str, range: ide_domain::TextRange, texto: &str) -> InputEdit {
    let inicio = offset_of(atual, range.start.line as usize, range.start.column as usize);
    let fim = offset_of(atual, range.end.line as usize, range.end.column as usize);
    let coluna_inicial = inicio - inicio_da_linha(atual, range.start.line as usize);
    let coluna_final = fim - inicio_da_linha(atual, range.end.line as usize);

    // Onde o texto inserido termina, contado a partir de onde ele começou.
    let quebras = texto.bytes().filter(|byte| *byte == b'\n').count();
    let novo_fim = match texto.rsplit_once('\n') {
        Some((_, ultima)) => Point {
            row: range.start.line as usize + quebras,
            column: ultima.len(),
        },
        None => Point {
            row: range.start.line as usize,
            column: coluna_inicial + texto.len(),
        },
    };

    InputEdit {
        start_byte: inicio,
        old_end_byte: fim,
        new_end_byte: inicio + texto.len(),
        start_position: Point {
            row: range.start.line as usize,
            column: coluna_inicial,
        },
        old_end_position: Point {
            row: range.end.line as usize,
            column: coluna_final,
        },
        new_end_position: novo_fim,
    }
}

/// O byte em que uma linha começa.
fn inicio_da_linha(fonte: &str, linha: usize) -> usize {
    let mut offset = 0;
    for (indice, atual) in fonte.split_inclusive('\n').enumerate() {
        if indice == linha {
            return offset;
        }
        offset += atual.len();
    }
    fonte.len()
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
