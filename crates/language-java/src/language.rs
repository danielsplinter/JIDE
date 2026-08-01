use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Condvar, Mutex, RwLock, RwLockReadGuard},
    thread,
    time::Duration,
};

use crate::completion::{finish_member_list, member_name};
use crate::documents::{Documents, ParsedDocument};
use crate::index::{ExternalClass, WorkspaceIndex};
use crate::navigation::{member_access, token_at_position, within};
use crate::semantics::receiver_type;
use crate::symbols::simple_class_name;
use async_trait::async_trait;
use ide_domain::{
    AccessorCandidate, AccessorKind, AccessorPlan, CompletionItem, CompletionKind,
    CompletionRequest, DefinitionRequest, Diagnostic, DiagnosticSeverity, DocumentChange,
    DocumentId, DocumentSnapshot, ImportItem, LanguageId, Location, OutlineItem, OutlineKind,
    ProviderId, ReferencesRequest, SemanticScope, SemanticSnapshot, SemanticSymbol, SymbolKind,
    SyntaxHighlight, SyntaxHighlightKind, SyntaxSnapshot, TextPosition, TextRange, TypeDescriptor,
};
#[cfg(test)]
use ide_language_api::LanguageToolchainConfig;
use ide_language_api::{
    ActiveLanguage, LANGUAGE_API_VERSION, LanguageActivationContext, LanguageCapabilities,
    LanguageError, LanguageMetadata, LanguageProvider, MemberAccess,
};
use tree_sitter::{Node, Point, Tree};

pub const JAVA_LANGUAGE_ID: &str = "java";
pub const JAVA_PROVIDER_ID: &str = "native-java";

#[derive(Default)]
pub struct JavaLanguageProvider;

impl JavaLanguageProvider {
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl LanguageProvider for JavaLanguageProvider {
    fn metadata(&self) -> LanguageMetadata {
        LanguageMetadata {
            language_id: LanguageId(JAVA_LANGUAGE_ID.to_owned()),
            provider_id: ProviderId(JAVA_PROVIDER_ID.to_owned()),
            display_name: "Java nativo".to_owned(),
            extensions: vec!["java".to_owned()],
            api_version: LANGUAGE_API_VERSION,
            // O ponto pede os membros do que está à esquerda dele.
            trigger_characters: vec!['.'],
        }
    }

    fn capabilities(&self) -> LanguageCapabilities {
        LanguageCapabilities::SYNTAX
            | LanguageCapabilities::DIAGNOSTICS
            | LanguageCapabilities::SEMANTICS
            | LanguageCapabilities::COMPLETION
            | LanguageCapabilities::DEFINITION
            | LanguageCapabilities::REFERENCES
    }

    async fn activate(
        &self,
        context: LanguageActivationContext,
    ) -> Result<Box<dyn ActiveLanguage>, LanguageError> {
        let language_id = LanguageId(JAVA_LANGUAGE_ID.to_owned());
        let toolchain_root = context
            .toolchain(&language_id)
            .map(|toolchain| toolchain.installation_root.as_path());
        Ok(Box::new(JavaLanguage::new(
            &context.workspace_root,
            &context.source_roots,
            toolchain_root,
        )?))
    }
}

struct JavaLanguage {
    language_id: LanguageId,
    documents: Documents,
    /// Vazio até a varredura em segundo plano terminar.
    workspace_index: Arc<RwLock<WorkspaceIndex>>,
    /// Marca e aviso de que o índice chegou, para quem precisa esperá-lo.
    index_ready: Arc<(Mutex<bool>, Condvar)>,
}

impl JavaLanguage {
    /// Membros públicos do tipo à esquerda do ponto.
    ///
    /// Duas origens, nesta ordem: o arquivo aberto, que responde pelo tipo que
    /// ainda não foi compilado, e as classes compiladas do workspace, que
    /// respondem pelo JDK, pelas dependências e pelo próprio projeto depois de
    /// um build. A cadeia de superclasses é percorrida porque `getClass` e
    /// `toString` são tão membros do objeto quanto os declarados nele.
    fn member_completion(
        &self,
        access: &MemberAccess,
        document: &ParsedDocument,
    ) -> Vec<CompletionItem> {
        let type_name = receiver_type(&access.receiver, &document.semantic.symbols);
        let mut items = members_in_document(&type_name, &document.semantic);
        // O que a lista oferece é o que se alcança pelo ponto, e `private` não se
        // alcança de fora — no arquivo aberto isso vem da árvore, porque o fonte
        // não passou por compilador nenhum.
        let private = private_members(&document.tree, &document.snapshot.text);
        items.retain(|item| !private.contains(member_name(&item.label)));
        items.extend(self.members_of_type(&type_name));
        finish_member_list(items, &access.prefix)
    }

    /// Membros de um tipo, de onde quer que ele venha no projeto.
    ///
    /// O arquivo aberto fica de fora porque só ele depende de um documento; tudo
    /// o mais — os demais fontes, o JDK, as dependências — é o mesmo índice, e
    /// por isso a mesma busca serve à completação de dentro de um arquivo e à do
    /// editor do depurador, que não tem arquivo nenhum.
    fn members_of_type(&self, type_name: &str) -> Vec<CompletionItem> {
        let mut items = self.members_in_project(type_name);
        let mut current = Some(type_name.to_owned());
        let mut depth = 0;
        while let Some(name) = current.take()
            && depth < MAX_SUPER_DEPTH
        {
            depth += 1;
            let Some(descriptor) = self
                .index()
                .external_classes
                .iter()
                .find(|class| class.simple == name || class.binary == name)
                .and_then(ExternalClass::descriptor)
            else {
                break;
            };
            items.extend(
                descriptor
                    .fields
                    .iter()
                    .filter(|member| is_visible_member(member))
                    .map(|member| completion_for_class_member(member, false)),
            );
            items.extend(
                descriptor
                    .methods
                    .iter()
                    .filter(|member| is_visible_member(member))
                    .map(|member| completion_for_class_member(member, true)),
            );
            current = descriptor.super_name;
        }
        items
    }

    /// Membros de um tipo declarado noutro arquivo do projeto.
    ///
    /// O arquivo é lido e analisado na hora, e não guardado analisado: manter a
    /// análise de todos os fontes do projeto custaria memória proporcional ao
    /// projeto para responder sobre um tipo de cada vez — o mesmo motivo pelo
    /// qual os membros das classes compiladas são lidos sob demanda.
    ///
    /// A regra de o que é membro é a mesma do arquivo aberto: só a origem do
    /// texto muda.
    fn members_in_project(&self, type_name: &str) -> Vec<CompletionItem> {
        let indice = self.index();
        let Some(path) = indice.declarations.get(type_name) else {
            return Vec::new();
        };
        let Ok(text) = fs::read_to_string(path) else {
            return Vec::new();
        };
        let Ok(tree) = self.documents.parse_tree(&text) else {
            return Vec::new();
        };
        let snapshot = DocumentSnapshot {
            id: DocumentId(0),
            path: path.clone(),
            version: 0,
            text,
        };
        let (semantic, _) = analyze_semantics(&snapshot, &tree);
        let mut items = members_in_document(type_name, &semantic);
        let private = private_members(&tree, &snapshot.text);
        items.retain(|item| !private.contains(member_name(&item.label)));
        items
    }

    /// Ativa sem esperar o índice.
    ///
    /// A varredura do projeto e do JDK leva segundos, e esperá-la aqui é o que
    /// segurava a primeira consulta à linguagem. O índice nasce **vazio** e é
    /// montado numa linha de execução à parte; até chegar, o que depende dele
    /// responde nada, e o que depende só do arquivo aberto responde igual.
    fn new(
        workspace_root: &Path,
        source_roots: &[PathBuf],
        toolchain_root: Option<&Path>,
    ) -> Result<Self, LanguageError> {
        let documents = Documents::new()?;
        let workspace_index = Arc::new(RwLock::new(WorkspaceIndex::default()));
        let pronto = Arc::new((Mutex::new(false), Condvar::new()));

        let destino = Arc::clone(&workspace_index);
        let aviso = Arc::clone(&pronto);
        let raiz = workspace_root.to_path_buf();
        let fontes = source_roots.to_vec();
        let toolchain = toolchain_root.map(Path::to_path_buf);
        thread::spawn(move || {
            let indice = Documents::new()
                .and_then(|documentos| {
                    documentos.with_parser_mut(|parser| {
                        WorkspaceIndex::scan(&raiz, &fontes, toolchain.as_deref(), parser)
                    })
                })
                .unwrap_or_default();
            if let Ok(mut guarda) = destino.write() {
                *guarda = indice;
            }
            let (marca, condicao) = &*aviso;
            if let Ok(mut pronto) = marca.lock() {
                *pronto = true;
            }
            condicao.notify_all();
        });

        Ok(Self {
            language_id: LanguageId(JAVA_LANGUAGE_ID.to_owned()),
            documents,
            workspace_index,
            index_ready: pronto,
        })
    }

    /// Espera o índice terminar. Existe para quem **precisa** dele agora.
    ///
    /// Com limite: um índice que falhe não pode pendurar quem esperou. Quem não
    /// chama isto trabalha com o que já existe, que é o caminho normal.
    pub(super) fn wait_for_index(&self, limite: Duration) -> bool {
        let (marca, condicao) = &*self.index_ready;
        let Ok(pronto) = marca.lock() else {
            return false;
        };
        let Ok((pronto, _)) = condicao.wait_timeout_while(pronto, limite, |pronto| !*pronto) else {
            return false;
        };
        *pronto
    }

    /// O índice, para leitura.
    fn index(&self) -> RwLockReadGuard<'_, WorkspaceIndex> {
        self.workspace_index
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

#[async_trait]
impl ActiveLanguage for JavaLanguage {
    async fn file_changed(&self, path: &Path) -> Result<(), LanguageError> {
        // Só o arquivo que mudou: o resto do índice fica como está.
        let mut indice = self
            .workspace_index
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        self.documents
            .with_parser_mut(|parser| indice.reindex_file(path, parser))
    }

    async fn wait_until_indexed(&self, timeout: Duration) -> bool {
        self.wait_for_index(timeout)
    }

    fn language_id(&self) -> &LanguageId {
        &self.language_id
    }

    async fn member_access(
        &self,
        text: &str,
        offset: usize,
    ) -> Result<Option<MemberAccess>, LanguageError> {
        Ok(member_access(text, offset))
    }

    async fn open_document(&self, document: DocumentSnapshot) -> Result<(), LanguageError> {
        self.documents.open(document)
    }

    async fn change_document(&self, change: DocumentChange) -> Result<(), LanguageError> {
        self.documents.change(change)
    }

    async fn close_document(&self, document_id: DocumentId) -> Result<(), LanguageError> {
        self.documents.close(document_id)
    }

    async fn diagnostics(&self, document_id: DocumentId) -> Result<Vec<Diagnostic>, LanguageError> {
        Ok(self.syntax(document_id).await?.diagnostics)
    }

    async fn syntax(&self, document_id: DocumentId) -> Result<SyntaxSnapshot, LanguageError> {
        self.documents.syntax(document_id)
    }

    async fn semantic(&self, document_id: DocumentId) -> Result<SemanticSnapshot, LanguageError> {
        self.documents.semantic(document_id)
    }

    async fn accessor_plan(
        &self,
        document_id: DocumentId,
        position: TextPosition,
        kind: AccessorKind,
    ) -> Result<AccessorPlan, LanguageError> {
        let documents = self
            .documents
            .lock()
            .map_err(|_| LanguageError::Provider("Java document lock poisoned".to_owned()))?;
        let document = documents
            .get(&document_id)
            .ok_or_else(|| LanguageError::Provider("Java document is not open".to_owned()))?;
        accessor_plan_for(&document.snapshot.text, &document.tree, position, kind)
            .ok_or_else(|| LanguageError::Unsupported("nenhum tipo nesta posição".to_owned()))
    }

    async fn references_to_name(&self, name: &str) -> Result<Vec<Location>, LanguageError> {
        let mut documents = self.documents.lock()?;
        Documents::ensure_semantics(&mut documents);
        // O índice é montado na ativação e não acompanha edição: para um arquivo
        // **aberto** ele fala do texto de antes. Quem responde por esses é o
        // documento; do índice vem só o que não está aberto. Sem esse corte, a
        // mesma ocorrência aparecia duas vezes, uma delas em posição vencida.
        let abertos: HashSet<&Path> = documents
            .values()
            .map(|document| document.snapshot.path.as_path())
            .collect();
        let do_indice = |location: &Location| !abertos.contains(location.path.as_path());
        let mut locations = documents
            .values()
            .flat_map(|document| document.references.get(name).into_iter().flatten())
            .cloned()
            .chain(
                self.index()
                    .references
                    .get(name)
                    .into_iter()
                    .flatten()
                    .filter(|location| do_indice(location))
                    .cloned(),
            )
            .collect::<Vec<_>>();
        locations.extend(
            documents
                .values()
                .flat_map(|document| document.semantic.symbols.iter())
                .filter(|symbol| symbol.name == name)
                .map(|symbol| symbol.location.clone()),
        );
        locations.extend(
            self.index()
                .symbols
                .iter()
                .filter(|symbol| symbol.name == name)
                .map(|symbol| symbol.location.clone())
                .filter(do_indice),
        );
        locations.sort_by(|esquerda, direita| {
            esquerda.path.cmp(&direita.path).then(
                (esquerda.range.start.line, esquerda.range.start.column)
                    .cmp(&(direita.range.start.line, direita.range.start.column)),
            )
        });
        locations.dedup();
        Ok(locations)
    }

    async fn constructor_source(
        &self,
        document_id: DocumentId,
        position: TextPosition,
        fields: Vec<String>,
    ) -> Result<Option<String>, LanguageError> {
        let documents = self
            .documents
            .lock()
            .map_err(|_| LanguageError::Provider("Java document lock poisoned".to_owned()))?;
        let document = documents
            .get(&document_id)
            .ok_or_else(|| LanguageError::Provider("Java document is not open".to_owned()))?;
        Ok(constructor_source_for(
            &document.snapshot.text,
            &document.tree,
            position,
            &fields,
        ))
    }

    async fn workspace_types(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<SemanticSymbol>, LanguageError> {
        let query = query.trim().to_ascii_lowercase();
        let indice = self.index();
        let mut found: Vec<SemanticSymbol> = indice
            .symbols
            .iter()
            .filter(|symbol| {
                matches!(
                    symbol.kind,
                    SymbolKind::Class
                        | SymbolKind::Interface
                        | SymbolKind::Record
                        | SymbolKind::Enum
                )
            })
            .filter(|symbol| query.is_empty() || symbol.name.to_ascii_lowercase().contains(&query))
            .cloned()
            .collect();
        // Quem começa com o que foi digitado vem antes de quem só contém: é o
        // que se procura ao escrever as primeiras letras de um nome.
        found.sort_by(|left, right| {
            let peso = |symbol: &SemanticSymbol| {
                usize::from(!symbol.name.to_ascii_lowercase().starts_with(&query))
            };
            peso(left)
                .cmp(&peso(right))
                .then_with(|| left.name.cmp(&right.name))
        });
        found.dedup_by(|left, right| left.name == right.name && left.location == right.location);
        found.truncate(limit);
        Ok(found)
    }

    async fn type_members(
        &self,
        type_name: &str,
        prefix: &str,
    ) -> Result<Vec<CompletionItem>, LanguageError> {
        Ok(finish_member_list(self.members_of_type(type_name), prefix))
    }

    async fn completion(
        &self,
        request: CompletionRequest,
    ) -> Result<Vec<CompletionItem>, LanguageError> {
        let mut documents = self.documents.lock()?;
        // A semântica é calculada sob demanda, e a demanda é esta.
        Documents::ensure_semantics(&mut documents);
        let document = documents
            .get(&request.document_id)
            .ok_or_else(|| LanguageError::Provider("Java document is not open".to_owned()))?;
        // Depois de um ponto, o que interessa são os membros do objeto à
        // esquerda — não todo símbolo do projeto que começa com o mesmo prefixo.
        if let Ok(offset) = offset_for_position(&document.snapshot.text, request.position)
            && let Some(access) = member_access(&document.snapshot.text, offset)
        {
            return Ok(self.member_completion(&access, document));
        }
        let mut items = Vec::new();
        let mut seen = HashSet::new();
        for symbol in document
            .semantic
            .symbols
            .iter()
            .chain(self.index().symbols.iter())
        {
            if symbol.name.starts_with(&request.prefix) && seen.insert(symbol.name.clone()) {
                items.push(completion_for_symbol(symbol));
            }
        }
        for class in &self.index().external_classes {
            if class.simple.starts_with(&request.prefix) && seen.insert(class.simple.clone()) {
                items.push(CompletionItem {
                    label: class.simple.clone(),
                    detail: Some(format!("class file em {}", class.origin.display())),
                    kind: CompletionKind::Class,
                });
            }
        }
        for keyword in JAVA_KEYWORDS {
            if keyword.starts_with(&request.prefix) && seen.insert((*keyword).to_owned()) {
                items.push(CompletionItem {
                    label: (*keyword).to_owned(),
                    detail: Some("Java keyword".to_owned()),
                    kind: CompletionKind::Keyword,
                });
            }
        }
        items.sort_by(|left, right| left.label.cmp(&right.label));
        items.truncate(100);
        Ok(items)
    }

    async fn definition(&self, request: DefinitionRequest) -> Result<Vec<Location>, LanguageError> {
        let mut documents = self.documents.lock()?;
        Documents::ensure_semantics(&mut documents);
        let document = documents
            .get(&request.document_id)
            .ok_or_else(|| LanguageError::Provider("Java document is not open".to_owned()))?;
        let name = token_at_position(&document.snapshot.text, request.position);
        let indice = self.index();
        let mut symbols = documents
            .values()
            .flat_map(|document| document.semantic.symbols.iter())
            .chain(indice.symbols.iter())
            .filter(|symbol| symbol.name == name)
            .collect::<Vec<_>>();
        symbols.sort_by(|left, right| {
            let left_same_file = left.location.path == document.snapshot.path;
            let right_same_file = right.location.path == document.snapshot.path;
            right_same_file
                .cmp(&left_same_file)
                .then(right.scope_depth.cmp(&left.scope_depth))
                .then(
                    left.location
                        .range
                        .start
                        .line
                        .abs_diff(request.position.line)
                        .cmp(
                            &right
                                .location
                                .range
                                .start
                                .line
                                .abs_diff(request.position.line),
                        ),
                )
        });
        let mut locations = symbols
            .into_iter()
            .map(|symbol| symbol.location.clone())
            .collect::<Vec<_>>();
        locations.sort_by(|left, right| {
            left.path
                .cmp(&right.path)
                .then(left.range.start.line.cmp(&right.range.start.line))
        });
        locations.dedup();
        Ok(locations)
    }

    async fn references(&self, request: ReferencesRequest) -> Result<Vec<Location>, LanguageError> {
        let mut documents = self.documents.lock()?;
        Documents::ensure_semantics(&mut documents);
        let document = documents
            .get(&request.document_id)
            .ok_or_else(|| LanguageError::Provider("Java document is not open".to_owned()))?;
        let name = token_at_position(&document.snapshot.text, request.position);
        let mut locations = documents
            .values()
            .flat_map(|document| document.references.get(&name).into_iter().flatten())
            .chain(
                self.index()
                    .references
                    .get(&name)
                    .into_iter()
                    .flatten(),
            )
            .cloned()
            .collect::<Vec<_>>();
        if request.include_declaration {
            locations.extend(
                documents
                    .values()
                    .flat_map(|document| document.semantic.symbols.iter())
                    .chain(self.index().symbols.iter())
                    .filter(|symbol| symbol.name == name)
                    .map(|symbol| symbol.location.clone()),
            );
        }
        locations.sort_by(|left, right| {
            left.path
                .cmp(&right.path)
                .then(left.range.start.line.cmp(&right.range.start.line))
                .then(left.range.start.column.cmp(&right.range.start.column))
        });
        locations.dedup();
        Ok(locations)
    }

    async fn shutdown(&self) -> Result<(), LanguageError> {
        self.documents.clear()
    }
}

/// Nome simples, como se escreve no código.
///
/// O último segmento, e não o primeiro: `java.lang.System$Logger` se escreve
/// `System.Logger`, e o tipo se chama `Logger`. Tomando o primeiro, a classe
/// aninhada — e a anônima `System$1` — atenderiam por `System`, e uma delas
/// responderia no lugar da verdadeira.
pub(super) fn analyze_semantics(
    document: &DocumentSnapshot,
    tree: &Tree,
) -> (SemanticSnapshot, HashMap<String, Vec<Location>>) {
    let root = tree.root_node();
    // O índice de linhas é feito uma vez por análise: é ele que evita procurar a
    // linha de cada nó varrendo o arquivo desde o começo.
    let lines = LineIndex::new(&document.text);
    let mut symbols = Vec::new();
    let mut scopes = vec![SemanticScope {
        range: node_range(root, &lines),
        depth: 0,
        symbols: Vec::new(),
    }];
    let mut references = HashMap::new();
    visit_semantics(
        root,
        document,
        &lines,
        0,
        &mut symbols,
        &mut scopes,
        &mut references,
    );
    (
        SemanticSnapshot {
            document_id: document.id,
            version: document.version,
            symbols,
            scopes,
        },
        references,
    )
}

fn visit_semantics(
    node: Node<'_>,
    document: &DocumentSnapshot,
    lines: &LineIndex<'_>,
    parent_scope: usize,
    symbols: &mut Vec<SemanticSymbol>,
    scopes: &mut Vec<SemanticScope>,
    references: &mut HashMap<String, Vec<Location>>,
) {
    let scope_index = if node.kind() != "program" && is_scope(node.kind()) {
        let index = scopes.len();
        scopes.push(SemanticScope {
            range: node_range(node, lines),
            depth: scopes[parent_scope].depth + 1,
            symbols: Vec::new(),
        });
        index
    } else {
        parent_scope
    };

    if let Some((kind, name_node, type_descriptor)) = declaration(node, &document.text) {
        let name = name_node
            .utf8_text(document.text.as_bytes())
            .unwrap_or_default()
            .to_owned();
        if !name.is_empty() {
            let symbol_index = symbols.len();
            symbols.push(SemanticSymbol {
                name,
                kind,
                location: Location {
                    path: document.path.clone(),
                    range: node_range(name_node, lines),
                },
                type_descriptor,
                scope_depth: scopes[scope_index].depth,
            });
            scopes[scope_index].symbols.push(symbol_index);
        }
    }

    if node.child_count() == 0
        && matches!(node.kind(), "identifier" | "type_identifier")
        && !is_declaration_name(node)
        && let Ok(name) = node.utf8_text(document.text.as_bytes())
    {
        references
            .entry(name.to_owned())
            .or_default()
            .push(Location {
                path: document.path.clone(),
                range: node_range(node, lines),
            });
    }

    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        visit_semantics(
            child,
            document,
            lines,
            scope_index,
            symbols,
            scopes,
            references,
        );
    }
}

fn is_scope(kind: &str) -> bool {
    matches!(
        kind,
        "class_body"
            | "interface_body"
            | "enum_body"
            | "annotation_type_body"
            | "constructor_body"
            | "block"
            | "lambda_expression"
            | "method_declaration"
            | "constructor_declaration"
            | "for_statement"
            | "enhanced_for_statement"
            | "catch_clause"
    )
}

fn declaration<'tree>(
    node: Node<'tree>,
    source: &str,
) -> Option<(SymbolKind, Node<'tree>, Option<TypeDescriptor>)> {
    let kind = match node.kind() {
        "class_declaration" => SymbolKind::Class,
        // Um `record` declara um tipo como qualquer outro; sem ele no índice,
        // navegar até um DTO não encontrava nada.
        "record_declaration" => SymbolKind::Record,
        "interface_declaration" => SymbolKind::Interface,
        "enum_declaration" => SymbolKind::Enum,
        "enum_constant" => SymbolKind::EnumConstant,
        "annotation_type_declaration" => SymbolKind::Annotation,
        "annotation_type_element_declaration" => SymbolKind::Method,
        "compact_constructor_declaration" => SymbolKind::Constructor,
        "constructor_declaration" => SymbolKind::Constructor,
        "method_declaration" => SymbolKind::Method,
        "field_declaration" => SymbolKind::Field,
        "formal_parameter" | "spread_parameter" => SymbolKind::Parameter,
        "local_variable_declaration" => SymbolKind::LocalVariable,
        _ => return None,
    };
    let name = node.child_by_field_name("name").or_else(|| {
        let mut cursor = node.walk();
        node.named_children(&mut cursor)
            .find(|child| child.kind() == "variable_declarator")
            .and_then(|declarator| declarator.child_by_field_name("name"))
    })?;
    let type_descriptor = node
        .child_by_field_name("type")
        .and_then(|kind| kind.utf8_text(source.as_bytes()).ok())
        .map(parse_type);
    Some((kind, name, type_descriptor))
}

fn is_declaration_name(node: Node<'_>) -> bool {
    node.parent().is_some_and(|parent| {
        parent
            .child_by_field_name("name")
            .is_some_and(|name| name.id() == node.id())
            || parent.kind() == "variable_declarator"
    })
}

/// Profundidade máxima da cadeia de superclasses percorrida.
///
/// Herança em Java termina em `Object`, e parar cedo evita que um índice
/// inconsistente vire laço infinito.
const MAX_SUPER_DEPTH: usize = 12;

/// Marca de membro público num arquivo `.class`, pela especificação da JVM.
const ACC_PUBLIC: u16 = 0x0001;
/// Sintético e ponte: gerados pelo compilador, não escritos por ninguém.
const ACC_SYNTHETIC: u16 = 0x1000;
const ACC_BRIDGE: u16 = 0x0040;

/// A posição está dentro do intervalo.
/// Nome do tipo que o receptor tem.
///
/// Variável, parâmetro ou campo declarado no arquivo entrega o tipo pela
/// declaração. Não achando nenhum, o próprio receptor é tomado como nome de
/// tipo — é o caso do acesso estático, `Integer.` ou `Math.`.
/// Traduz um descritor da JVM para a forma que se lê no código.
///
/// `Ljava/lang/String;` vira `String`, `[I` vira `int[]`. O nome simples basta:
/// o menu mostra o tipo para diferenciar membros, não para qualificar imports.
fn readable_descriptor(descriptor: &str) -> String {
    let mut characters = descriptor.chars().peekable();
    let mut arrays = 0;
    while characters.peek() == Some(&'[') {
        characters.next();
        arrays += 1;
    }
    let mut name = match characters.next() {
        Some('B') => "byte".to_owned(),
        Some('C') => "char".to_owned(),
        Some('D') => "double".to_owned(),
        Some('F') => "float".to_owned(),
        Some('I') => "int".to_owned(),
        Some('J') => "long".to_owned(),
        Some('S') => "short".to_owned(),
        Some('Z') => "boolean".to_owned(),
        Some('V') => "void".to_owned(),
        Some('L') => {
            // No descritor o nome vem com barras — `java/lang/String` — e o
            // índice do workspace trabalha com pontos.
            let binary: String = characters
                .by_ref()
                .take_while(|value| *value != ';')
                .collect();
            simple_class_name(&binary.replace('/', "."))
        }
        _ => descriptor.to_owned(),
    };
    for _ in 0..arrays {
        name.push_str("[]");
    }
    name
}

/// Parâmetros e retorno de um descritor de método.
fn method_signature(descriptor: &str) -> (Vec<String>, String) {
    let Some((parameters, result)) = descriptor
        .strip_prefix('(')
        .and_then(|rest| rest.split_once(')'))
    else {
        return (Vec::new(), readable_descriptor(descriptor));
    };
    let mut types = Vec::new();
    let mut rest = parameters;
    while !rest.is_empty() {
        let arrays = rest.len() - rest.trim_start_matches('[').len();
        let body = &rest[arrays..];
        let consumed = if body.starts_with('L') {
            body.find(';').map_or(body.len(), |index| index + 1)
        } else {
            1
        };
        types.push(readable_descriptor(&rest[..arrays + consumed]));
        rest = &rest[arrays + consumed..];
    }
    (types, readable_descriptor(result))
}

/// Membro público de uma classe compilada vira item do menu.
fn completion_for_class_member(
    member: &java_classfile::ClassMember,
    method: bool,
) -> CompletionItem {
    if method {
        let (parameters, result) = method_signature(&member.descriptor);
        CompletionItem {
            label: format!("{}({})", member.name, parameters.join(", ")),
            detail: Some(result),
            kind: CompletionKind::Method,
        }
    } else {
        CompletionItem {
            label: member.name.clone(),
            detail: Some(readable_descriptor(&member.descriptor)),
            kind: CompletionKind::Field,
        }
    }
}

/// Membros declarados dentro da classe, no próprio arquivo aberto.
///
/// Serve o tipo que ainda não foi compilado — a classe que está sendo escrita
/// agora, ou a que vive no mesmo arquivo. O corpo da classe é um escopo, e o
/// escopo já sabe quais símbolos nasceram dentro dele; o que falta é ligar o
/// nome da classe ao corpo dela, que é o primeiro escopo aberto depois do nome.
fn members_in_document(type_name: &str, semantic: &SemanticSnapshot) -> Vec<CompletionItem> {
    let Some(class) = semantic.symbols.iter().find(|symbol| {
        symbol.name == type_name
            && matches!(
                symbol.kind,
                SymbolKind::Class | SymbolKind::Interface | SymbolKind::Record | SymbolKind::Enum
            )
    }) else {
        return Vec::new();
    };
    let Some(body) = semantic
        .scopes
        .iter()
        .filter(|scope| {
            scope.depth == class.scope_depth + 1
                && (scope.range.start.line, scope.range.start.column)
                    >= (
                        class.location.range.end.line,
                        class.location.range.end.column,
                    )
        })
        .min_by_key(|scope| (scope.range.start.line, scope.range.start.column))
    else {
        return Vec::new();
    };
    // Um campo é registrado no corpo da classe, mas um método abre escopo
    // próprio e é registrado dentro dele — por isso a busca é por posição, e
    // não pela lista de símbolos do corpo. A profundidade separa o que é membro
    // desta classe do que pertence a uma classe aninhada mais abaixo.
    semantic
        .symbols
        .iter()
        .filter(|symbol| matches!(symbol.kind, SymbolKind::Method | SymbolKind::Field))
        .filter(|symbol| {
            (symbol.scope_depth == body.depth || symbol.scope_depth == body.depth + 1)
                && within(&body.range, symbol.location.range.start)
        })
        .map(|symbol| CompletionItem {
            label: if symbol.kind == SymbolKind::Method {
                format!("{}()", symbol.name)
            } else {
                symbol.name.clone()
            },
            detail: symbol
                .type_descriptor
                .as_ref()
                .map(|descriptor| descriptor.name.clone()),
            kind: if symbol.kind == SymbolKind::Method {
                CompletionKind::Method
            } else {
                CompletionKind::Field
            },
        })
        .collect()
}

/// Só o que o usuário pode escrever com o ponto.
/// Filtra pelo prefixo, tira repetidos e ordena. É o fim de toda lista de
/// membros, venha ela de onde vier.
/// Nomes dos membros declarados `private` no arquivo.
///
/// As classes compiladas trazem os modificadores nos próprios bytes; o fonte
/// não, e os símbolos da análise semântica guardam nome, tipo e escopo, não
/// visibilidade. Ler isto da árvore é o que faz a lista depois do ponto ser de
/// membros públicos também para o código que ainda não foi compilado.
fn private_members(tree: &Tree, text: &str) -> HashSet<String> {
    let mut names = HashSet::new();
    let mut cursor = tree.walk();
    let mut pending = vec![tree.root_node()];
    while let Some(node) = pending.pop() {
        pending.extend(node.children(&mut cursor));
        if !matches!(node.kind(), "field_declaration" | "method_declaration") {
            continue;
        }
        let private = node
            .children(&mut node.walk())
            .find(|child| child.kind() == "modifiers")
            .and_then(|modifiers| modifiers.utf8_text(text.as_bytes()).ok())
            .is_some_and(|modifiers| modifiers.split_whitespace().any(|word| word == "private"));
        if !private {
            continue;
        }
        if let Some(name) = node
            .child_by_field_name("name")
            .and_then(|name| name.utf8_text(text.as_bytes()).ok())
        {
            names.insert(name.to_owned());
        }
        // Um campo declara os nomes dentro dos declaradores, e podem ser vários
        // na mesma linha: `private int a, b;`.
        for declarator in node
            .children(&mut node.walk())
            .filter(|child| child.kind() == "variable_declarator")
        {
            if let Some(name) = declarator
                .child_by_field_name("name")
                .and_then(|name| name.utf8_text(text.as_bytes()).ok())
            {
                names.insert(name.to_owned());
            }
        }
    }
    names
}

fn is_visible_member(member: &java_classfile::ClassMember) -> bool {
    member.access_flags & ACC_PUBLIC != 0
        && member.access_flags & (ACC_SYNTHETIC | ACC_BRIDGE) == 0
        // Construtor e inicializador estático não são alcançáveis por ponto.
        && !member.name.starts_with('<')
}

fn parse_type(value: &str) -> TypeDescriptor {
    let mut name = value.trim().to_owned();
    let mut array_dimensions = 0;
    while name.ends_with("[]") {
        name.truncate(name.len().saturating_sub(2));
        array_dimensions += 1;
    }
    let generic_arguments = name
        .split_once('<')
        .map(|(_, arguments)| {
            arguments
                .trim_end_matches('>')
                .split(',')
                .map(|argument| argument.trim().to_owned())
                .collect()
        })
        .unwrap_or_default();
    if let Some((base, _)) = name.split_once('<') {
        name = base.to_owned();
    }
    TypeDescriptor {
        name,
        array_dimensions,
        generic_arguments,
    }
}

fn completion_for_symbol(symbol: &SemanticSymbol) -> CompletionItem {
    CompletionItem {
        label: symbol.name.clone(),
        detail: symbol
            .type_descriptor
            .as_ref()
            .map(|kind| kind.name.clone()),
        kind: match symbol.kind {
            // Um registro se completa como o tipo que ele é.
            SymbolKind::Class | SymbolKind::Record | SymbolKind::Annotation => {
                CompletionKind::Class
            }
            SymbolKind::Interface => CompletionKind::Interface,
            SymbolKind::Enum => CompletionKind::Enum,
            SymbolKind::EnumConstant => CompletionKind::Field,
            SymbolKind::Constructor => CompletionKind::Constructor,
            SymbolKind::Method => CompletionKind::Method,
            SymbolKind::Field => CompletionKind::Field,
            SymbolKind::Package | SymbolKind::Parameter | SymbolKind::LocalVariable => {
                CompletionKind::Variable
            }
        },
    }
}

pub(super) fn analyze(document: &DocumentSnapshot, tree: &Tree) -> SyntaxSnapshot {
    let root = tree.root_node();
    let lines = LineIndex::new(&document.text);
    let (highlights, diagnostics, outline) = collect_analysis(root, &lines);
    SyntaxSnapshot {
        document_id: document.id,
        version: document.version,
        outline,
        highlights,
        imports: collect_imports(root, &lines),
        diagnostics,
    }
}

/// Percorre a árvore com **um** cursor, chamando `visit` em cada nó.
///
/// `Node::walk` cria um cursor novo, e a análise fazia isso uma vez por nó: só
/// andar por uma árvore de 81 mil nós custava 35 ms, e a análise andava três
/// vezes — realces, diagnósticos e outline. Um cursor e uma passada.
///
/// `visit` recebe a profundidade, que é o que permite fechar o que estava aberto
/// ao subir de novo.
fn walk_tree(root: Node<'_>, mut visit: impl FnMut(Node<'_>, usize)) {
    let mut cursor = root.walk();
    let mut depth = 0usize;
    loop {
        visit(cursor.node(), depth);
        if cursor.goto_first_child() {
            depth += 1;
            continue;
        }
        loop {
            if cursor.goto_next_sibling() {
                break;
            }
            // Voltar ao ponto de partida encerra: subir além dele sairia da
            // subárvore que foi pedida.
            if depth == 0 || !cursor.goto_parent() {
                return;
            }
            depth -= 1;
        }
    }
}

/// Realces, diagnósticos e outline de uma só passada pela árvore.
///
/// Os três precisam do mesmo percurso e cada um custava o seu; juntos, custam um.
fn collect_analysis(
    root: Node<'_>,
    lines: &LineIndex<'_>,
) -> (Vec<SyntaxHighlight>, Vec<Diagnostic>, Vec<OutlineItem>) {
    let mut highlights = Vec::new();
    let mut diagnostics = Vec::new();
    // Itens de outline abertos, do mais raso ao mais profundo. Um item fecha
    // quando o percurso volta ao nível dele, e aí entra como filho de quem o
    // contém.
    let mut abertos: Vec<(usize, OutlineItem)> = Vec::new();
    let mut raiz: Vec<OutlineItem> = Vec::new();
    // Profundidade do nó que já respondeu pelo realce de tudo abaixo dele — é
    // como um comentário ou uma string ficam de uma cor só, sem que os filhos
    // pintem por cima. Não vale para diagnósticos, que continuam descendo.
    let mut coberto: Option<usize> = None;

    walk_tree(root, |node, depth| {
        if coberto.is_some_and(|limite| depth <= limite) {
            coberto = None;
        }
        fechar_outline(&mut abertos, &mut raiz, depth);

        if node.is_error() || node.is_missing() {
            let message = if node.is_missing() {
                format!("Esperado `{}`", node.kind())
            } else {
                "Sintaxe Java inválida".to_owned()
            };
            diagnostics.push(Diagnostic {
                range: node_range(node, lines),
                severity: DiagnosticSeverity::Error,
                message,
                source: Some(JAVA_PROVIDER_ID.to_owned()),
            });
        }

        if let Some(kind) = outline_kind(node.kind()) {
            // Um campo não tem `name` próprio: o nome mora no declarador. Sem
            // isto todo campo saía como `<anonymous>` no outline.
            let name = node
                .child_by_field_name("name")
                .or_else(|| declarator_name(node))
                .and_then(|name| name.utf8_text(lines.source().as_bytes()).ok())
                .unwrap_or("<anonymous>")
                .to_owned();
            let nome_node = node
                .child_by_field_name("name")
                .or_else(|| declarator_name(node));
            abertos.push((
                depth,
                OutlineItem {
                    name,
                    kind,
                    range: node_range(node, lines),
                    // Sem nó de nome — declaração incompleta enquanto se digita
                    // — vale a declaração, que é o mais próximo que existe.
                    name_range: nome_node
                        .map_or_else(|| node_range(node, lines), |nome| node_range(nome, lines)),
                    children: Vec::new(),
                },
            ));
        }

        if coberto.is_some() {
            return;
        }
        let whole_node = node.kind().contains("comment")
            || matches!(
                node.kind(),
                "string_literal"
                    | "character_literal"
                    | "text_block"
                    | "marker_annotation"
                    | "annotation"
            );
        if (whole_node || node.child_count() == 0)
            && let Some(kind) = highlight_kind(node)
            && node.end_byte() > node.start_byte()
        {
            highlights.push(SyntaxHighlight {
                range: node_range(node, lines),
                kind,
            });
            if whole_node {
                coberto = Some(depth);
            }
        }
    });
    fechar_outline(&mut abertos, &mut raiz, 0);

    // O realce sai em ordem de nó, que não é ordem de texto: quem consome busca
    // por linha, e busca binária exige ordenação.
    highlights.sort_by_key(|highlight| {
        (
            highlight.range.start.line,
            highlight.range.start.column,
            highlight.range.end.line,
            highlight.range.end.column,
        )
    });
    (highlights, diagnostics, raiz)
}

/// Fecha os itens de outline que o percurso deixou para trás.
fn fechar_outline(
    abertos: &mut Vec<(usize, OutlineItem)>,
    raiz: &mut Vec<OutlineItem>,
    depth: usize,
) {
    while abertos.last().is_some_and(|(nivel, _)| *nivel >= depth) {
        let Some((_, item)) = abertos.pop() else {
            return;
        };
        match abertos.last_mut() {
            Some((_, pai)) => pai.children.push(item),
            None => raiz.push(item),
        }
    }
}

/// O tipo que contém a posição, com seu corpo e seus membros.
///
/// O plano de acessores e o construtor precisam exatamente disto, e procurar o
/// tipo duas vezes por caminhos diferentes acabaria em duas respostas.
fn enclosing_type<'a>(
    source: &str,
    tree: &'a Tree,
    position: TextPosition,
) -> Option<(Node<'a>, Node<'a>)> {
    let ponto = Point {
        row: position.line as usize,
        column: byte_column_of(source, position),
    };
    let mut tipo = tree.root_node().descendant_for_point_range(ponto, ponto)?;
    while !matches!(
        tipo.kind(),
        "class_declaration" | "interface_declaration" | "enum_declaration"
    ) {
        tipo = tipo.parent()?;
    }
    let corpo = tipo.child_by_field_name("body")?;
    Some((tipo, corpo))
}

/// Campos declarados no corpo do tipo, em ordem, como `(nome, tipo)`.
fn declared_fields<'a>(corpo: Node<'a>, bytes: &'a [u8]) -> Vec<(&'a str, &'a str)> {
    let mut cursor = corpo.walk();
    let mut campos = Vec::new();
    for campo in corpo
        .named_children(&mut cursor)
        .filter(|node| node.kind() == "field_declaration")
    {
        let Some(tipo_texto) = campo
            .child_by_field_name("type")
            .and_then(|node| node.utf8_text(bytes).ok())
        else {
            continue;
        };
        let mut interno = campo.walk();
        for declarador in campo
            .named_children(&mut interno)
            .filter(|node| node.kind() == "variable_declarator")
        {
            if let Some(nome) = declarador
                .child_by_field_name("name")
                .and_then(|node| node.utf8_text(bytes).ok())
            {
                campos.push((nome, tipo_texto));
            }
        }
    }
    campos
}

/// Construtor do tipo que contém a posição, com os campos escolhidos.
///
/// Lista vazia é um construtor **sem parâmetros** — uma resposta legítima, e não
/// a ausência de resposta. Devolve `None` quando o tipo já tem um construtor com
/// a mesma lista de tipos: repetir a assinatura não compila, e escrever isso
/// seria entregar um arquivo quebrado.
///
/// Toda a convenção é daqui: a ordem dos parâmetros segue a **declaração dos
/// campos**, e não a ordem em que foram marcados, porque é assim que se lê a
/// classe depois.
fn constructor_source_for(
    source: &str,
    tree: &Tree,
    position: TextPosition,
    fields: &[String],
) -> Option<String> {
    let bytes = source.as_bytes();
    let (tipo, corpo) = enclosing_type(source, tree, position)?;
    let nome_tipo = tipo
        .child_by_field_name("name")
        .and_then(|node| node.utf8_text(bytes).ok())?;
    let escolhidos: Vec<(&str, &str)> = declared_fields(corpo, bytes)
        .into_iter()
        .filter(|(nome, _)| fields.iter().any(|escolhido| escolhido == nome))
        .collect();

    let assinatura: Vec<String> = escolhidos
        .iter()
        .map(|(_, tipo_texto)| (*tipo_texto).to_owned())
        .collect();
    let mut cursor = corpo.walk();
    let repetido = corpo
        .named_children(&mut cursor)
        .filter(|node| node.kind() == "constructor_declaration")
        .any(|node| constructor_parameter_types(node, bytes) == assinatura);
    if repetido {
        return None;
    }

    let parametros = escolhidos
        .iter()
        .map(|(nome, tipo_texto)| format!("{tipo_texto} {nome}"))
        .collect::<Vec<_>>()
        .join(", ");
    let atribuicoes: String = escolhidos
        .iter()
        .map(|(nome, _)| format!("        this.{nome} = {nome};\n"))
        .collect();
    Some(format!(
        "\n    public {nome_tipo}({parametros}) {{\n{atribuicoes}    }}\n"
    ))
}

/// Tipos dos parâmetros de um construtor, na ordem declarada.
fn constructor_parameter_types(node: Node<'_>, bytes: &[u8]) -> Vec<String> {
    let Some(lista) = node.child_by_field_name("parameters") else {
        return Vec::new();
    };
    let mut cursor = lista.walk();
    lista
        .named_children(&mut cursor)
        .filter(|parametro| parametro.kind() == "formal_parameter")
        .filter_map(|parametro| {
            parametro
                .child_by_field_name("type")
                .and_then(|tipo| tipo.utf8_text(bytes).ok())
                .map(str::to_owned)
        })
        .collect()
}

/// Acessores que faltam ao tipo que contém a posição.
///
/// Tudo o que é Java mora aqui: a convenção de nome — `getNome`, mas `isAtivo`
/// para `boolean` —, o tipo de retorno e a indentação do corpo. Quem chama
/// recebe texto pronto e um lugar para pô-lo.
fn accessor_plan_for(
    source: &str,
    tree: &Tree,
    position: TextPosition,
    kind: AccessorKind,
) -> Option<AccessorPlan> {
    let bytes = source.as_bytes();
    let ponto = Point {
        row: position.line as usize,
        column: byte_column_of(source, position),
    };
    let mut tipo = tree.root_node().descendant_for_point_range(ponto, ponto)?;
    while !matches!(
        tipo.kind(),
        "class_declaration" | "interface_declaration" | "enum_declaration"
    ) {
        tipo = tipo.parent()?;
    }
    let corpo = tipo.child_by_field_name("body")?;
    let mut cursor = corpo.walk();
    let membros: Vec<Node<'_>> = corpo.named_children(&mut cursor).collect();
    let existentes: Vec<String> = membros
        .iter()
        .filter(|node| node.kind() == "method_declaration")
        .filter_map(|node| node.child_by_field_name("name"))
        .filter_map(|name| name.utf8_text(bytes).ok())
        .map(str::to_owned)
        .collect();

    let mut candidates = Vec::new();
    for campo in membros
        .iter()
        .filter(|node| node.kind() == "field_declaration")
    {
        let Some(declarado) = campo.child_by_field_name("type") else {
            continue;
        };
        let Ok(tipo_texto) = declarado.utf8_text(bytes) else {
            continue;
        };
        let mut interno = campo.walk();
        for declarador in campo
            .named_children(&mut interno)
            .filter(|node| node.kind() == "variable_declarator")
        {
            let Some(nome) = declarador
                .child_by_field_name("name")
                .and_then(|node| node.utf8_text(bytes).ok())
            else {
                continue;
            };
            // Com os dois, o campo entra se faltar **algum**, e o texto traz só
            // o que falta: repetir o que a classe já tem seria erro de
            // compilação, não conveniência.
            let partes: &[AccessorKind] = match kind {
                AccessorKind::Both => &[AccessorKind::Getter, AccessorKind::Setter],
                AccessorKind::Getter => &[AccessorKind::Getter],
                AccessorKind::Setter => &[AccessorKind::Setter],
                // O construtor não gera nada por campo: o texto sai do conjunto
                // escolhido, depois, em `constructor_source`. Aqui o campo entra
                // na lista apenas para poder ser escolhido.
                AccessorKind::Constructor => &[],
            };
            let fonte: String = partes
                .iter()
                .filter_map(|parte| {
                    let metodo = accessor_name(nome, tipo_texto, *parte);
                    (!existentes.contains(&metodo))
                        .then(|| accessor_source(nome, tipo_texto, &metodo, *parte))
                })
                .collect();
            candidates.push(AccessorCandidate {
                field: nome.to_owned(),
                source: (!fonte.is_empty()).then_some(fonte),
            });
        }
    }
    // Onde o cursor está, e no começo da linha: inserir no meio dela partiria
    // um token. A linha em que o cursor estava desce, que é o que se espera de
    // "gerar aqui".
    //
    // Preso ao corpo do tipo: com o cursor na linha da declaração, ou depois do
    // fecho, o método sairia fora da classe e nem compilaria.
    let abertura = corpo.start_position().row as u32;
    let fecho = corpo.end_position().row as u32;
    let linha = position.line.clamp(abertura.saturating_add(1), fecho);
    Some(AccessorPlan {
        candidates,
        insert_at: TextPosition {
            line: linha,
            column: 0,
        },
    })
}

/// Nome do acessor pela convenção da linguagem.
fn accessor_name(field: &str, type_name: &str, kind: AccessorKind) -> String {
    let mut chars = field.chars();
    let capitalizado = chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().collect::<String>() + chars.as_str()
    });
    match kind {
        // `boolean` usa `is`, e é a única exceção que a convenção tem.
        AccessorKind::Getter if type_name == "boolean" => format!("is{capitalizado}"),
        AccessorKind::Getter => format!("get{capitalizado}"),
        AccessorKind::Setter => format!("set{capitalizado}"),
        // `Both` é desdobrado nos dois antes de chegar aqui, e o construtor não
        // passa por aqui: ele não tem nome de acessor.
        AccessorKind::Both | AccessorKind::Constructor => format!("get{capitalizado}"),
    }
}

fn accessor_source(field: &str, type_name: &str, method: &str, kind: AccessorKind) -> String {
    match kind {
        AccessorKind::Getter => {
            format!("\n    public {type_name} {method}() {{\n        return {field};\n    }}\n")
        }
        AccessorKind::Setter => format!(
            "\n    public void {method}({type_name} {field}) {{\n        this.{field} = {field};\n    }}\n"
        ),
        AccessorKind::Both | AccessorKind::Constructor => String::new(),
    }
}

/// Coluna em bytes, que é como o tree-sitter conta.
fn byte_column_of(source: &str, position: TextPosition) -> usize {
    source
        .lines()
        .nth(position.line as usize)
        .map_or(0, |line| {
            line.char_indices()
                .nth(position.column as usize)
                .map_or(line.len(), |(index, _)| index)
        })
}

/// Nome do primeiro declarador de um campo.
///
/// `private int a, b;` declara dois no mesmo nó; o outline mostra o primeiro,
/// que é o que dá nome à linha.
fn declarator_name<'a>(node: Node<'a>) -> Option<Node<'a>> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .find(|child| child.kind() == "variable_declarator")
        .and_then(|declarator| declarator.child_by_field_name("name"))
}

fn outline_kind(kind: &str) -> Option<OutlineKind> {
    match kind {
        "class_declaration" => Some(OutlineKind::Class),
        "interface_declaration" => Some(OutlineKind::Interface),
        "enum_declaration" => Some(OutlineKind::Enum),
        "annotation_type_declaration" => Some(OutlineKind::Annotation),
        "constructor_declaration" => Some(OutlineKind::Constructor),
        "method_declaration" => Some(OutlineKind::Method),
        "field_declaration" => Some(OutlineKind::Field),
        _ => None,
    }
}

fn collect_imports(root: Node<'_>, lines: &LineIndex<'_>) -> Vec<ImportItem> {
    let mut imports = Vec::new();
    let mut cursor = root.walk();
    for node in root.named_children(&mut cursor) {
        if node.kind() != "import_declaration" {
            continue;
        }
        let text = node
            .utf8_text(lines.source().as_bytes())
            .unwrap_or_default()
            .trim()
            .trim_end_matches(';')
            .trim();
        let body = text.strip_prefix("import").unwrap_or(text).trim();
        let (is_static, path) = body
            .strip_prefix("static")
            .map_or((false, body), |path| (true, path.trim()));
        imports.push(ImportItem {
            path: path.trim_end_matches(".*").to_owned(),
            is_static,
            wildcard: path.ends_with(".*"),
            range: node_range(node, lines),
        });
    }
    imports
}

fn highlight_kind(node: Node<'_>) -> Option<SyntaxHighlightKind> {
    let kind = node.kind();
    if kind.contains("comment") {
        return Some(SyntaxHighlightKind::Comment);
    }
    if matches!(kind, "string_literal" | "character_literal" | "text_block") {
        return Some(SyntaxHighlightKind::String);
    }
    if kind.ends_with("_literal")
        && matches!(
            kind,
            "decimal_integer_literal"
                | "hex_integer_literal"
                | "octal_integer_literal"
                | "binary_integer_literal"
                | "decimal_floating_point_literal"
                | "hex_floating_point_literal"
        )
    {
        return Some(SyntaxHighlightKind::Number);
    }
    if kind == "type_identifier" {
        return Some(SyntaxHighlightKind::Type);
    }
    if kind == "identifier" {
        return node.parent().and_then(|parent| match parent.kind() {
            "method_declaration" | "method_invocation" | "constructor_declaration" => {
                Some(SyntaxHighlightKind::Function)
            }
            "field_access" | "field_declaration" => Some(SyntaxHighlightKind::Field),
            "variable_declarator" | "formal_parameter" => Some(SyntaxHighlightKind::Variable),
            // Fragmento de um nome qualificado — o `org` e o `springframework` de
            // um import. Não nomeia nada que se possa abrir.
            "scoped_identifier" | "package_declaration" => None,
            // Qualquer outro identificador é uma **referência** a algo declarado
            // em outro lugar: a constante usada numa comparação, a variável
            // passada como argumento, o contador de um laço. Classificá-los só
            // na declaração deixava o uso sem realce, e sem realce a interface
            // não tinha como saber que dali dá para navegar.
            _ => Some(SyntaxHighlightKind::Variable),
        });
    }
    if matches!(kind, "marker_annotation" | "annotation") {
        return Some(SyntaxHighlightKind::Annotation);
    }
    if JAVA_KEYWORDS.contains(&kind) {
        return Some(SyntaxHighlightKind::Keyword);
    }
    if JAVA_OPERATORS.contains(&kind) {
        return Some(SyntaxHighlightKind::Operator);
    }
    None
}

const JAVA_KEYWORDS: &[&str] = &[
    "abstract",
    "assert",
    "boolean",
    "break",
    "byte",
    "case",
    "catch",
    "char",
    "class",
    "const",
    "continue",
    "default",
    "do",
    "double",
    "else",
    "enum",
    "extends",
    "final",
    "finally",
    "float",
    "for",
    "goto",
    "if",
    "implements",
    "import",
    "instanceof",
    "int",
    "interface",
    "long",
    "native",
    "new",
    "package",
    "private",
    "protected",
    "public",
    "return",
    "short",
    "static",
    "strictfp",
    "super",
    "switch",
    "synchronized",
    "this",
    "throw",
    "throws",
    "transient",
    "try",
    "void",
    "volatile",
    "while",
    "true",
    "false",
    "null",
];

const JAVA_OPERATORS: &[&str] = &[
    "+", "-", "*", "/", "%", "=", "==", "!=", "<", ">", "<=", ">=", "&&", "||", "!", "&", "|", "^",
    "~", "<<", ">>", ">>>", "++", "--", "+=", "-=", "*=", "/=", "%=", "->", "::",
];

/// Onde cada linha começa, para converter posições sem varrer o arquivo.
///
/// O tree-sitter dá a posição de um nó em linha e **coluna de bytes**; a IDE fala
/// em coluna de caracteres. A conversão precisa da linha onde o nó está, e
/// procurá-la com `lines().nth(row)` varre o texto desde o começo a cada nó —
/// com a árvore inteira sendo convertida por análise, o custo crescia com o
/// **quadrado** do tamanho do arquivo: medimos 46 ms para 200 linhas, 1,5 s para
/// 1000 e 12,5 s para 3000, a cada tecla digitada. Guardando o início de cada
/// linha uma vez, cada conversão passa a ser uma indexação.
pub(super) struct LineIndex<'a> {
    source: &'a str,
    starts: Vec<usize>,
}

impl<'a> LineIndex<'a> {
    pub(super) fn new(source: &'a str) -> Self {
        let mut starts = vec![0];
        starts.extend(
            source
                .bytes()
                .enumerate()
                .filter(|(_, byte)| *byte == b'\n')
                .map(|(offset, _)| offset + 1),
        );
        Self { source, starts }
    }

    pub(super) const fn source(&self) -> &'a str {
        self.source
    }

    /// Converte o ponto do tree-sitter em posição de caracteres.
    fn position(&self, point: Point) -> TextPosition {
        let line = self.line(point.row);
        let byte_column = point.column.min(line.len());
        let column = line
            .get(..byte_column)
            .map_or(0, |prefix| prefix.chars().count());
        TextPosition {
            line: point.row as u32,
            column: column as u32,
        }
    }

    /// A linha, sem a quebra — como `str::lines` a entregaria.
    fn line(&self, row: usize) -> &'a str {
        let Some(start) = self.starts.get(row).copied() else {
            return "";
        };
        let end = self
            .starts
            .get(row + 1)
            .map_or(self.source.len(), |next| next.saturating_sub(1));
        let line = self.source.get(start..end).unwrap_or_default();
        // O `\r` do CRLF não conta como coluna, igual ao que `lines` faz.
        line.strip_suffix('\r').unwrap_or(line)
    }
}

fn node_range(node: Node<'_>, lines: &LineIndex<'_>) -> TextRange {
    TextRange {
        start: lines.position(node.start_position()),
        end: lines.position(node.end_position()),
    }
}

pub(super) fn offset_for_position(
    text: &str,
    position: TextPosition,
) -> Result<usize, LanguageError> {
    let mut offset = 0;
    let target_line = position.line as usize;
    for (line_index, line) in text.split('\n').enumerate() {
        if line_index == target_line {
            let column = position.column as usize;
            let byte = line
                .char_indices()
                .map(|(index, _)| index)
                .chain(std::iter::once(line.len()))
                .nth(column)
                .ok_or_else(|| {
                    LanguageError::Provider("Java change column is out of bounds".to_owned())
                })?;
            return Ok(offset + byte);
        }
        offset += line.len() + 1;
    }
    Err(LanguageError::Provider(
        "Java change line is out of bounds".to_owned(),
    ))
}

pub(super) fn point_for_offset(text: &str, offset: usize) -> Point {
    let prefix = &text[..offset.min(text.len())];
    let row = prefix.bytes().filter(|byte| *byte == b'\n').count();
    let column = prefix
        .rsplit_once('\n')
        .map_or(prefix.len(), |(_, line)| line.len());
    Point { row, column }
}

pub(super) fn point_after_text(start: Point, inserted: &str) -> Point {
    let new_lines = inserted.bytes().filter(|byte| *byte == b'\n').count();
    if new_lines == 0 {
        Point {
            row: start.row,
            column: start.column + inserted.len(),
        }
    } else {
        Point {
            row: start.row + new_lines,
            column: inserted
                .rsplit_once('\n')
                .map_or(inserted.len(), |(_, line)| line.len()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A conversão de posição não varre o arquivo, e continua correta.
    ///
    /// O índice existe por causa de desempenho, mas o que ele substitui era
    /// `lines().nth(row)` — se ele errar a linha, o realce sai deslocado.
    #[test]
    fn the_line_index_converts_byte_columns_into_character_columns() {
        let texto = "int número = 1;
String s = \"olá\";
int x;";
        let lines = LineIndex::new(texto);
        assert_eq!(lines.source(), texto);

        // `número` tem um caractere de dois bytes: a coluna de bytes 10 é a
        // coluna de caracteres 9.
        let posicao = lines.position(Point { row: 0, column: 10 });
        assert_eq!((posicao.line, posicao.column), (0, 9));

        // A segunda linha termina em CRLF, e o retorno não conta como coluna.
        let fim = lines.position(Point { row: 1, column: 18 });
        assert_eq!(fim.line, 1);
        assert_eq!(
            fim.column, 17,
            "a linha tem 18 bytes e 17 caracteres, porque o acento ocupa dois"
        );

        let ultima = lines.position(Point { row: 2, column: 4 });
        assert_eq!((ultima.line, ultima.column), (2, 4));

        // Linha inexistente não estoura: o texto pode ter mudado sob a árvore.
        let fora = lines.position(Point { row: 99, column: 3 });
        assert_eq!((fora.line, fora.column), (99, 0));
    }

    /// A análise de um arquivo grande não pode custar o quadrado do tamanho.
    ///
    /// Foi o defeito que travava a digitação: cada nó da árvore procurava a sua
    /// linha varrendo o arquivo desde o começo, e uma tecla num arquivo de 3000
    /// linhas custava mais de 12 segundos. O limite é folgado de propósito —
    /// máquina lenta ou build de depuração não podem reprovar o teste —, mas
    /// voltar ao comportamento quadrático o estoura com sobra.
    #[test]
    fn analyzing_a_large_file_stays_far_from_quadratic() {
        let corpo: String = (0..2000)
            .map(|indice| {
                format!(
                    "    int metodo{indice}() {{ return {indice}; }}
"
                )
            })
            .collect();
        let snapshot = snapshot(&format!(
            "public class Grande {{
{corpo}}}
"
        ));
        let (Ok(parser), snapshot) = (crate::parser::JavaParser::new(), snapshot) else {
            panic!("parser Java indisponível");
        };
        let Ok(tree) = parser.parse(&snapshot.text, None) else {
            panic!("o fonte de teste não parseou");
        };

        let inicio = std::time::Instant::now();
        let analise = analyze(&snapshot, &tree);
        let gasto = inicio.elapsed();

        assert!(!analise.highlights.is_empty());
        assert!(
            gasto < std::time::Duration::from_secs(3),
            "análise de 2000 linhas levou {gasto:?}; o custo voltou a crescer com o quadrado do arquivo"
        );
    }

    /// O outline da passada única aninha como a recursão aninhava.
    #[test]
    fn the_single_pass_outline_keeps_members_inside_their_type() {
        let fonte = "public class Pedido {
    private int id;
    public int getId() { return id; }
    class Interna { void f() {} }
}
";
        let snapshot = snapshot(fonte);
        let (Ok(parser), snapshot) = (crate::parser::JavaParser::new(), snapshot) else {
            panic!("parser Java indisponível");
        };
        let Ok(tree) = parser.parse(&snapshot.text, None) else {
            panic!("o fonte de teste não parseou");
        };
        let outline = analyze(&snapshot, &tree).outline;

        assert_eq!(outline.len(), 1, "só a classe fica na raiz");
        let classe = &outline[0];
        assert_eq!(classe.name, "Pedido");
        let nomes: Vec<&str> = classe
            .children
            .iter()
            .map(|item| item.name.as_str())
            .collect();
        assert_eq!(nomes, vec!["id", "getId", "Interna"]);
        let Some(interna) = classe.children.iter().find(|item| item.name == "Interna") else {
            panic!("classe interna ausente do outline");
        };
        assert_eq!(
            interna
                .children
                .iter()
                .map(|item| item.name.as_str())
                .collect::<Vec<_>>(),
            vec!["f"],
            "o membro da classe interna fica dentro dela"
        );
    }

    /// Uma classe criada agora entra na completação, sem reiniciar nada.
    ///
    /// É o critério da fase 4 da `19`. Antes, o índice era montado na ativação e
    /// só ali: um arquivo novo aparecia na busca por tipo na **próxima** vez que
    /// a linguagem fosse ativada.
    #[test]
    fn a_file_saved_now_joins_the_index() {
        let root = std::env::temp_dir().join(format!("er-ide-incremental-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        assert!(fs::create_dir_all(&root).is_ok());
        assert!(fs::write(root.join("Antigo.java"), "public class Antigo {}\n").is_ok());

        let active = match pollster::block_on(JavaLanguageProvider::new().activate(
            LanguageActivationContext {
                workspace_root: root.clone(),
                source_roots: vec![root.clone()],
                toolchains: Vec::new(),
            },
        )) {
            Ok(active) => {
                assert!(pollster::block_on(
                    active.wait_until_indexed(Duration::from_secs(60))
                ));
                active
            }
            Err(error) => panic!("falha ao ativar o provider: {error}"),
        };

        let nomes = |active: &dyn ActiveLanguage| -> Vec<String> {
            match pollster::block_on(active.workspace_types("", 50)) {
                Ok(found) => found.into_iter().map(|symbol| symbol.name).collect(),
                Err(error) => panic!("falha na busca: {error}"),
            }
        };
        assert!(!nomes(active.as_ref()).iter().any(|nome| nome == "Novo"));

        // A classe nasce agora, e o aviso é o que a faz entrar.
        assert!(fs::write(root.join("Novo.java"), "public class Novo {}\n").is_ok());
        assert!(pollster::block_on(active.file_changed(&root.join("Novo.java"))).is_ok());
        assert!(
            nomes(active.as_ref()).iter().any(|nome| nome == "Novo"),
            "a classe criada precisa participar da completação: {:?}",
            nomes(active.as_ref())
        );

        // E apagar tira: reindexar um arquivo remove o que ele declarava.
        assert!(fs::remove_file(root.join("Novo.java")).is_ok());
        assert!(pollster::block_on(active.file_changed(&root.join("Novo.java"))).is_ok());
        assert!(!nomes(active.as_ref()).iter().any(|nome| nome == "Novo"));
        assert!(
            nomes(active.as_ref()).iter().any(|nome| nome == "Antigo"),
            "reindexar um arquivo não pode levar os outros junto"
        );
        let _ = fs::remove_dir_all(&root);
    }

    /// Ativar devolve sem esperar o índice.
    ///
    /// É o critério da fase 2 da `19`: a varredura do projeto e do JDK leva
    /// segundos, e a primeira consulta à linguagem esperava por ela. Agora o
    /// ambiente volta na hora, e quem precisa da resposta completa pede para
    /// esperar.
    #[test]
    fn activation_returns_before_the_index_is_ready() {
        let inicio = std::time::Instant::now();
        let active = match pollster::block_on(JavaLanguageProvider::new().activate(
            LanguageActivationContext {
                workspace_root: ".".into(),
                source_roots: Vec::new(),
                toolchains: Vec::new(),
            },
        )) {
            // Este teste mede a ativação: esperar aqui mediria a espera.
            Ok(active) => active,
            Err(error) => panic!("ativação falhou: {error}"),
        };
        let ativacao = inicio.elapsed();
        assert!(
            ativacao < Duration::from_millis(250),
            "ativar não pode esperar a varredura: levou {ativacao:?}"
        );
        // E o índice chega depois, para quem quiser esperá-lo.
        assert!(pollster::block_on(
            active.wait_until_indexed(Duration::from_secs(60))
        ));
    }

    /// A semântica é calculada sob demanda e refeita depois de cada mudança.
    ///
    /// Ela não serve para desenhar, então sair do caminho da tecla é o ponto;
    /// mas quem pergunta tem de receber a resposta do texto **atual**, senão a
    /// navegação apontaria para onde o símbolo estava.
    #[test]
    fn semantics_are_computed_on_demand_and_refreshed_after_a_change() {
        let active = active();
        let fonte = "public class Alvo { int primeiro; }";
        assert!(pollster::block_on(active.open_document(snapshot(fonte))).is_ok());

        let antes = match pollster::block_on(active.semantic(DocumentId(1))) {
            Ok(semantic) => semantic,
            Err(error) => panic!("semântica indisponível: {error}"),
        };
        assert!(
            antes.symbols.iter().any(|symbol| symbol.name == "primeiro"),
            "a primeira pergunta calcula"
        );

        // Reescreve o arquivo inteiro trocando o nome do campo.
        assert!(
            pollster::block_on(active.change_document(DocumentChange {
                document_id: DocumentId(1),
                version: 2,
                range: None,
                text: "public class Alvo { int segundo; }".to_owned(),
            }))
            .is_ok()
        );
        let depois = match pollster::block_on(active.semantic(DocumentId(1))) {
            Ok(semantic) => semantic,
            Err(error) => panic!("semântica indisponível: {error}"),
        };
        assert!(
            depois.symbols.iter().any(|symbol| symbol.name == "segundo"),
            "a mudança invalida o que estava guardado"
        );
        assert!(
            !depois
                .symbols
                .iter()
                .any(|symbol| symbol.name == "primeiro"),
            "e não devolve o símbolo que deixou de existir"
        );
    }

    fn active() -> Box<dyn ActiveLanguage> {
        active_with_jdk(None)
    }

    fn active_with_jdk(jdk_home: Option<PathBuf>) -> Box<dyn ActiveLanguage> {
        let toolchains = jdk_home
            .map(|installation_root| LanguageToolchainConfig {
                language_id: LanguageId(JAVA_LANGUAGE_ID.to_owned()),
                installation_root,
                properties: Default::default(),
            })
            .into_iter()
            .collect();
        match pollster::block_on(
            JavaLanguageProvider::new().activate(LanguageActivationContext {
                workspace_root: ".".into(),
                source_roots: Vec::new(),
                toolchains,
            }),
        ) {
            Ok(active) => {
                // Os testes afirmam a resposta **completa**, então esperam o
                // índice. A aplicação não espera: é essa diferença que a fase 2
                // introduz.
                assert!(
                    pollster::block_on(active.wait_until_indexed(Duration::from_secs(60))),
                    "o índice do projeto não ficou pronto a tempo"
                );
                active
            }
            Err(error) => panic!("failed to activate Java provider: {error}"),
        }
    }

    /// O **uso** de uma constante, variável ou contador também é realçado.
    ///
    /// Só as declarações eram classificadas. Sem realce no uso, a interface não
    /// tinha como saber que dali dá para navegar, e o cursor não virava mão
    /// justamente onde o usuário quer clicar.
    #[test]
    fn identifier_references_are_highlighted_not_only_declarations() {
        let texto = concat!(
            "import org.exemplo.Coisa;
",
            "class A {
",
            "  static final int TOTAL = 10;
",
            "  void f() {
",
            "    for (int i = 0; i < TOTAL; i++) { usa(i); }
",
            "  }
",
            "}
"
        );
        let provider = active();
        assert!(pollster::block_on(provider.open_document(snapshot(texto))).is_ok());
        let resultado = match pollster::block_on(provider.syntax(DocumentId(1))) {
            Ok(resultado) => resultado,
            Err(error) => panic!("análise falhou: {error}"),
        };

        let kind_em = |ancora: &str, alvo: &str| {
            let base = texto
                .find(ancora)
                .unwrap_or_else(|| panic!("âncora {ancora}"));
            let offset = base + ancora.find(alvo).unwrap_or(0);
            let antes = &texto[..offset];
            let line = antes.matches(char::from(10)).count() as u32;
            let column = antes
                .rsplit(char::from(10))
                .next()
                .unwrap_or("")
                .chars()
                .count() as u32;
            resultado
                .highlights
                .iter()
                .find(|highlight| {
                    (highlight.range.start.line, highlight.range.start.column) <= (line, column)
                        && (line, column) < (highlight.range.end.line, highlight.range.end.column)
                })
                .map(|highlight| highlight.kind)
        };

        assert_eq!(
            kind_em("i < TOTAL", "TOTAL"),
            Some(SyntaxHighlightKind::Variable),
            "constante usada numa comparação"
        );
        assert_eq!(
            kind_em("usa(i)", "i"),
            Some(SyntaxHighlightKind::Variable),
            "variável passada como argumento"
        );
        assert_eq!(
            kind_em("i++", "i"),
            Some(SyntaxHighlightKind::Variable),
            "contador de laço"
        );
        // Fragmento de nome qualificado não nomeia nada que se possa abrir.
        assert_eq!(kind_em("org.exemplo", "org"), None);
    }

    fn snapshot(text: &str) -> DocumentSnapshot {
        DocumentSnapshot {
            id: DocumentId(1),
            path: "Example.java".into(),
            version: 1,
            text: text.to_owned(),
        }
    }

    /// O que está antes do ponto é o receptor; o que está depois é o filtro.
    #[test]
    fn a_dot_marks_a_member_access() {
        assert_eq!(
            member_access("pedido.", 7),
            Some(MemberAccess {
                receiver: "pedido".to_owned(),
                prefix: String::new(),
            })
        );
        assert_eq!(
            member_access("    pedido.getVal", 17),
            Some(MemberAccess {
                receiver: "pedido".to_owned(),
                prefix: "getVal".to_owned(),
            })
        );
    }

    /// Sem ponto não há acesso a membro, e um decimal não é um objeto.
    #[test]
    fn a_decimal_literal_is_not_a_member_access() {
        assert_eq!(member_access("total", 5), None);
        assert_eq!(member_access("valor = 3.14", 12), None);
    }

    /// O tipo vem da declaração da variável; sem declaração, o próprio nome é
    /// tomado como tipo, que é o caso do acesso estático.
    #[test]
    fn the_receiver_type_comes_from_the_declaration() {
        let symbols = vec![SemanticSymbol {
            name: "pedido".to_owned(),
            kind: SymbolKind::LocalVariable,
            location: Location {
                path: "Example.java".into(),
                range: TextRange::default(),
            },
            type_descriptor: Some(parse_type("Pedido")),
            scope_depth: 1,
        }];
        assert_eq!(receiver_type("pedido", &symbols), "Pedido");
        assert_eq!(receiver_type("Integer", &symbols), "Integer");
    }

    /// Descritores da JVM viram o que se lê no código.
    #[test]
    fn jvm_descriptors_are_shown_as_written_types() {
        assert_eq!(readable_descriptor("Ljava/lang/String;"), "String");
        assert_eq!(readable_descriptor("[I"), "int[]");
        assert_eq!(readable_descriptor("V"), "void");
        let (parameters, result) = method_signature("(Ljava/lang/String;I[J)Z");
        assert_eq!(parameters, vec!["String", "int", "long[]"]);
        assert_eq!(result, "boolean");
    }

    /// Membros de uma classe do projeto declarada em **outro arquivo**.
    ///
    /// Uma classe por arquivo é a regra em Java, então este é o caso comum — e
    /// era o que não funcionava: só o JDK e as dependências respondiam, porque a
    /// busca olhava o arquivo aberto e as classes compiladas, nunca os outros
    /// fontes do projeto.
    #[test]
    fn members_of_a_project_class_in_another_file_are_offered_after_the_dot() {
        let root = std::env::temp_dir().join(format!("er-ide-java-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        assert!(fs::create_dir_all(&root).is_ok());
        assert!(
            fs::write(
                root.join("Matricula.java"),
                concat!(
                    "public class Matricula {\n",
                    "    public Long id;\n",
                    "    public String nome;\n",
                    "    public void setId(Long id) {}\n",
                    "    private void interno() {}\n",
                    "}\n",
                ),
            )
            .is_ok()
        );

        let active = match pollster::block_on(JavaLanguageProvider::new().activate(
            LanguageActivationContext {
                workspace_root: root.clone(),
                source_roots: vec![root.clone()],
                toolchains: Vec::new(),
            },
        )) {
            Ok(active) => {
                assert!(
                    pollster::block_on(active.wait_until_indexed(Duration::from_secs(60))),
                    "o índice do projeto não ficou pronto a tempo"
                );
                active
            }
            Err(error) => panic!("falha ao ativar o provider: {error}"),
        };

        // O arquivo aberto não declara `Matricula`; ele apenas a usa.
        let source = concat!(
            "class Uso {\n",
            "    void executar() {\n",
            "        Matricula m = new Matricula();\n",
            "        m.\n",
            "    }\n",
            "}\n",
        );
        if let Err(error) = pollster::block_on(active.open_document(snapshot(source))) {
            panic!("falha ao abrir documento: {error}");
        }
        let items = match pollster::block_on(active.completion(CompletionRequest {
            document_id: DocumentId(1),
            position: TextPosition {
                line: 3,
                column: 10,
            },
            prefix: String::new(),
        })) {
            Ok(items) => items,
            Err(error) => panic!("falha na completação: {error}"),
        };
        let labels: Vec<&str> = items.iter().map(|item| item.label.as_str()).collect();
        assert!(labels.contains(&"id"), "campo público: {labels:?}");
        assert!(labels.contains(&"nome"), "campo público: {labels:?}");
        assert!(labels.contains(&"setId()"), "método público: {labels:?}");
        assert!(
            !labels.contains(&"interno()"),
            "membro privado não é oferecido: {labels:?}"
        );
        let _ = fs::remove_dir_all(root);
    }

    /// O outline diz onde está o **nome**, e não só onde a declaração começa.
    ///
    /// Uma anotação acima da classe empurra o começo da declaração para a linha
    /// dela. Quem precisa saber se o clique caiu no nome — o menu de renomear —
    /// errava por uma linha justamente nos arquivos anotados, que num projeto
    /// com Spring ou JPA são quase todos.
    #[test]
    fn the_outline_points_at_the_name_and_not_only_at_the_declaration() {
        let fonte = concat!(
            "@Entity\n",
            "public class Pedido {\n",
            "    private Long id;\n",
            "}\n",
        );
        let snapshot = snapshot(fonte);
        let (Ok(parser), snapshot) = (crate::parser::JavaParser::new(), snapshot) else {
            panic!("parser Java indisponível");
        };
        let Ok(tree) = parser.parse(&snapshot.text, None) else {
            panic!("o fonte de teste não parseou");
        };
        let outline = analyze(&snapshot, &tree).outline;
        let Some(classe) = outline.first() else {
            panic!("a classe precisa estar no outline");
        };

        assert_eq!(
            classe.range.start.line, 0,
            "a declaração começa na anotação"
        );
        assert_eq!(
            classe.name_range.start.line, 1,
            "mas o nome está abaixo dela"
        );
        assert_eq!(classe.name_range.start.column, 13);
        assert_eq!(classe.name_range.end.column, 19);
    }

    /// Para um arquivo aberto, quem vale é o documento — não o índice.
    ///
    /// O índice é montado na ativação e não acompanha edição nem renomeação.
    /// Somando os dois, a mesma ocorrência vinha duas vezes, uma delas em
    /// posição vencida — e depois de renomear vinha um arquivo que já não
    /// existe, com o nome antigo, na lista da janela.
    #[test]
    fn an_open_file_answers_for_itself_instead_of_the_stale_index() {
        let active = active();
        let source = "public class Pedido {
    Pedido copiar() { return new Pedido(); }
}
";
        assert!(pollster::block_on(active.open_document(snapshot(source))).is_ok());

        let Ok(encontrados) = pollster::block_on(active.references_to_name("Pedido")) else {
            panic!("referências indisponíveis");
        };
        let caminho = snapshot(source).path;
        let deste_arquivo: Vec<_> = encontrados
            .iter()
            .filter(|local| local.path == caminho)
            .collect();
        let mut posicoes: Vec<_> = deste_arquivo
            .iter()
            .map(|local| (local.range.start.line, local.range.start.column))
            .collect();
        let antes = posicoes.len();
        posicoes.sort_unstable();
        posicoes.dedup();
        assert_eq!(
            posicoes.len(),
            antes,
            "nenhuma ocorrência pode aparecer duas vezes: {posicoes:?}"
        );
    }

    /// As referências por nome cobrem o projeto, não só o arquivo aberto.
    ///
    /// Renomear um arquivo fala de um nome que pode não estar aberto em lugar
    /// nenhum, e por isso a pergunta é pelo **nome**, e não por uma posição.
    #[test]
    fn references_to_a_name_cover_declaration_and_uses() {
        let active = active();
        let source = concat!(
            "public class Pedido {\n",
            "    private Pedido anterior;\n",
            "    Pedido copiar() { return new Pedido(); }\n",
            "}\n",
        );
        assert!(pollster::block_on(active.open_document(snapshot(source))).is_ok());

        let encontrados = match pollster::block_on(active.references_to_name("Pedido")) {
            Ok(encontrados) => encontrados,
            Err(error) => panic!("referências indisponíveis: {error}"),
        };
        assert!(
            encontrados.len() >= 4,
            "declaração, campo, retorno e construção: {}",
            encontrados.len()
        );
        assert!(
            encontrados.iter().any(|local| local.range.start.line == 0),
            "a declaração entra, porque ela também será trocada"
        );

        // Um nome que não existe não devolve nada.
        let vazio = match pollster::block_on(active.references_to_name("Inexistente")) {
            Ok(vazio) => vazio,
            Err(error) => panic!("referências indisponíveis: {error}"),
        };
        assert!(vazio.is_empty());
    }

    /// O construtor sai do conjunto escolhido, e a lista vazia é um construtor
    /// sem parâmetros — resposta legítima, e não ausência de resposta.
    #[test]
    fn the_constructor_is_built_from_the_chosen_fields() {
        let active = active();
        let source = concat!(
            "public class Pedido {\n",
            "    private Long id;\n",
            "    private String nome;\n",
            "    private boolean ativo;\n",
            "}\n",
        );
        assert!(pollster::block_on(active.open_document(snapshot(source))).is_ok());
        let dentro = TextPosition { line: 2, column: 4 };
        let gerar = |campos: Vec<String>| match pollster::block_on(active.constructor_source(
            DocumentId(1),
            dentro,
            campos,
        )) {
            Ok(fonte) => fonte,
            Err(error) => panic!("construtor indisponível: {error}"),
        };

        // Dois campos marcados: parâmetros e atribuições na ordem em que os
        // campos foram **declarados**, e não na ordem em que foram marcados.
        assert_eq!(
            gerar(vec!["nome".to_owned(), "id".to_owned()]).as_deref(),
            Some(concat!(
                "\n",
                "    public Pedido(Long id, String nome) {\n",
                "        this.id = id;\n",
                "        this.nome = nome;\n",
                "    }\n",
            ))
        );

        // Nenhum campo: construtor sem parâmetros.
        assert_eq!(
            gerar(Vec::new()).as_deref(),
            Some("\n    public Pedido() {\n    }\n")
        );

        // Todos os campos.
        let Some(todos) = gerar(vec!["id".to_owned(), "nome".to_owned(), "ativo".to_owned()])
        else {
            panic!("o construtor com todos os campos precisa existir");
        };
        assert!(todos.contains("public Pedido(Long id, String nome, boolean ativo)"));
        assert!(todos.contains("this.ativo = ativo;"));
    }

    /// Um construtor de mesma assinatura não é gerado duas vezes.
    ///
    /// Repetir a assinatura não compila: entregar isso seria quebrar o arquivo
    /// em nome da conveniência.
    #[test]
    fn a_constructor_with_the_same_signature_is_not_generated_again() {
        let active = active();
        let source = concat!(
            "public class Pedido {\n",
            "    private Long id;\n",
            "    private String nome;\n",
            "\n",
            "    public Pedido(Long id) { this.id = id; }\n",
            "}\n",
        );
        assert!(pollster::block_on(active.open_document(snapshot(source))).is_ok());
        let dentro = TextPosition { line: 2, column: 4 };
        let gerar = |campos: Vec<String>| match pollster::block_on(active.constructor_source(
            DocumentId(1),
            dentro,
            campos,
        )) {
            Ok(fonte) => fonte,
            Err(error) => panic!("construtor indisponível: {error}"),
        };

        assert_eq!(
            gerar(vec!["id".to_owned()]),
            None,
            "já existe um construtor que recebe só `Long`"
        );
        assert!(
            gerar(vec!["id".to_owned(), "nome".to_owned()]).is_some(),
            "outra assinatura continua sendo gerada"
        );
        assert!(
            gerar(Vec::new()).is_some(),
            "o sem parâmetros também é outra assinatura"
        );
    }

    /// O plano de acessores conhece a convenção e o que já existe.
    ///
    /// É aqui que mora tudo o que é Java: `get` para quase tudo e `is` para
    /// `boolean`, o tipo de retorno, e não repetir o que a classe já tem.
    #[test]
    fn the_accessor_plan_knows_the_convention_and_what_already_exists() {
        let active = active();
        let source = concat!(
            "public class Matricula {\n",
            "    private Long id;\n",
            "    private String nome;\n",
            "    private boolean ativo;\n",
            "\n",
            "    public String getNome() { return nome; }\n",
            "}\n",
        );
        assert!(pollster::block_on(active.open_document(snapshot(source))).is_ok());
        let plano = match pollster::block_on(active.accessor_plan(
            DocumentId(1),
            TextPosition { line: 1, column: 4 },
            AccessorKind::Getter,
        )) {
            Ok(plano) => plano,
            Err(error) => panic!("plano indisponível: {error}"),
        };

        let campos: Vec<&str> = plano
            .candidates
            .iter()
            .map(|item| item.field.as_str())
            .collect();
        assert_eq!(campos, vec!["id", "nome", "ativo"]);

        // `nome` já tem getter: entra na lista, mas sem texto para gerar.
        let por_campo = |nome: &str| {
            plano
                .candidates
                .iter()
                .find(|item| item.field == nome)
                .and_then(|item| item.source.clone())
        };
        assert!(por_campo("nome").is_none(), "o que já existe não é gerado");
        let id = por_campo("id").unwrap_or_default();
        assert!(id.contains("public Long getId()"), "{id}");
        assert!(id.contains("return id;"), "{id}");
        // `boolean` usa `is`, que é a única exceção da convenção.
        let ativo = por_campo("ativo").unwrap_or_default();
        assert!(ativo.contains("public boolean isAtivo()"), "{ativo}");

        // O ponto de inserção é a linha do cursor, e no começo dela.
        assert_eq!(plano.insert_at.line, 1);
        assert_eq!(plano.insert_at.column, 0);

        // Preso ao corpo do tipo: na linha da declaração, desce para dentro.
        let na_declaracao = pollster::block_on(active.accessor_plan(
            DocumentId(1),
            TextPosition { line: 0, column: 2 },
            AccessorKind::Getter,
        ));
        assert_eq!(
            na_declaracao.map(|plano| plano.insert_at.line).ok(),
            Some(1),
            "com o cursor na linha da classe, o método não pode sair dela"
        );

        // Depois do fecho, também: o limite é a linha da chave que fecha.
        let apos_fecho = pollster::block_on(active.accessor_plan(
            DocumentId(1),
            TextPosition {
                line: 5,
                column: 10,
            },
            AccessorKind::Getter,
        ));
        assert_eq!(apos_fecho.map(|plano| plano.insert_at.line).ok(), Some(5));

        // O setter usa a mesma máquina, com outra convenção.
        let setters = match pollster::block_on(active.accessor_plan(
            DocumentId(1),
            TextPosition { line: 1, column: 4 },
            AccessorKind::Setter,
        )) {
            Ok(plano) => plano,
            Err(error) => panic!("plano indisponível: {error}"),
        };
        let nome = setters
            .candidates
            .iter()
            .find(|item| item.field == "nome")
            .and_then(|item| item.source.clone())
            .unwrap_or_default();
        assert!(nome.contains("public void setNome(String nome)"), "{nome}");
        assert!(nome.contains("this.nome = nome;"), "{nome}");

        // Os dois de uma vez: o campo entra se faltar algum, e sai só o que
        // falta — repetir o que existe seria erro de compilação.
        let ambos = match pollster::block_on(active.accessor_plan(
            DocumentId(1),
            TextPosition { line: 1, column: 4 },
            AccessorKind::Both,
        )) {
            Ok(plano) => plano,
            Err(error) => panic!("plano indisponível: {error}"),
        };
        let fonte = |campo: &str| {
            ambos
                .candidates
                .iter()
                .find(|item| item.field == campo)
                .and_then(|item| item.source.clone())
                .unwrap_or_default()
        };
        let id = fonte("id");
        assert!(id.contains("public Long getId()"), "{id}");
        assert!(id.contains("public void setId(Long id)"), "{id}");
        // `nome` já tem o getter: só o setter é gerado.
        let nome = fonte("nome");
        assert!(
            !nome.contains("getNome"),
            "o getter existente não se repete: {nome}"
        );
        assert!(nome.contains("public void setNome(String nome)"), "{nome}");
    }

    /// A busca por nome encontra os tipos do projeto e diz onde eles estão.
    ///
    /// Só tipos, e só com arquivo: o resultado existe para ser aberto.
    #[test]
    fn workspace_types_are_found_by_name_with_where_they_are() {
        let root = std::env::temp_dir().join(format!("er-ide-busca-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        assert!(fs::create_dir_all(&root).is_ok());
        assert!(
            fs::write(
                root.join("Pedido.java"),
                "public class Pedido { void calcular() {} }\n",
            )
            .is_ok()
        );
        assert!(
            fs::write(
                root.join("PedidoRepository.java"),
                "public interface PedidoRepository {}\n",
            )
            .is_ok()
        );
        assert!(
            fs::write(
                root.join("MeuPedido.java"),
                "public record MeuPedido(long id) {}\n",
            )
            .is_ok()
        );
        let active = match pollster::block_on(JavaLanguageProvider::new().activate(
            LanguageActivationContext {
                workspace_root: root.clone(),
                source_roots: vec![root.clone()],
                toolchains: Vec::new(),
            },
        )) {
            Ok(active) => {
                assert!(
                    pollster::block_on(active.wait_until_indexed(Duration::from_secs(60))),
                    "o índice do projeto não ficou pronto a tempo"
                );
                active
            }
            Err(error) => panic!("falha ao ativar o provider: {error}"),
        };

        let found = match pollster::block_on(active.workspace_types("pedido", 50)) {
            Ok(found) => found,
            Err(error) => panic!("falha na busca: {error}"),
        };
        let nomes: Vec<&str> = found.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(
            nomes,
            vec!["Pedido", "PedidoRepository", "MeuPedido"],
            "quem começa com o digitado vem antes: {nomes:?}"
        );
        assert!(
            found[0].location.path.ends_with("Pedido.java"),
            "o resultado precisa dizer onde abrir"
        );
        assert!(!nomes.contains(&"calcular"), "método não é tipo: {nomes:?}");

        // Consulta vazia devolve tudo, para a janela nascer com conteúdo.
        assert_eq!(
            pollster::block_on(active.workspace_types("", 50))
                .map(|found| found.len())
                .unwrap_or_default(),
            3
        );
        // O teto é respeitado.
        assert_eq!(
            pollster::block_on(active.workspace_types("", 1))
                .map(|found| found.len())
                .unwrap_or_default(),
            1
        );
        let _ = fs::remove_dir_all(root);
    }

    /// Consultar por nome de tipo alcança o projeto inteiro, sem arquivo aberto.
    ///
    /// É o que o editor do depurador usa: lá não há documento, e uma classe que
    /// não participa do que está sendo depurado precisa ser tão conhecida quanto
    /// qualquer outra.
    #[test]
    fn members_can_be_asked_by_type_name_without_any_document() {
        let root = std::env::temp_dir().join(format!("er-ide-tipo-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        assert!(fs::create_dir_all(&root).is_ok());
        assert!(
            fs::write(
                root.join("Relatorio.java"),
                concat!(
                    "public class Relatorio {\n",
                    "    public String titulo;\n",
                    "    public void emitir() {}\n",
                    "    private void rascunho() {}\n",
                    "}\n",
                ),
            )
            .is_ok()
        );
        let active = match pollster::block_on(JavaLanguageProvider::new().activate(
            LanguageActivationContext {
                workspace_root: root.clone(),
                source_roots: vec![root.clone()],
                toolchains: Vec::new(),
            },
        )) {
            Ok(active) => {
                assert!(
                    pollster::block_on(active.wait_until_indexed(Duration::from_secs(60))),
                    "o índice do projeto não ficou pronto a tempo"
                );
                active
            }
            Err(error) => panic!("falha ao ativar o provider: {error}"),
        };

        let items = match pollster::block_on(active.type_members("Relatorio", "")) {
            Ok(items) => items,
            Err(error) => panic!("falha ao consultar o tipo: {error}"),
        };
        let labels: Vec<&str> = items.iter().map(|item| item.label.as_str()).collect();
        assert!(labels.contains(&"titulo"), "campo público: {labels:?}");
        assert!(labels.contains(&"emitir()"), "método público: {labels:?}");
        assert!(
            !labels.contains(&"rascunho()"),
            "membro privado não é oferecido: {labels:?}"
        );

        // O prefixo filtra, como na lista de dentro de um arquivo.
        let filtrados = match pollster::block_on(active.type_members("Relatorio", "tit")) {
            Ok(items) => items,
            Err(error) => panic!("falha ao filtrar: {error}"),
        };
        assert_eq!(
            filtrados
                .iter()
                .map(|item| item.label.as_str())
                .collect::<Vec<_>>(),
            vec!["titulo"]
        );

        // Um tipo desconhecido responde vazio, e não com erro.
        assert_eq!(
            pollster::block_on(active.type_members("NaoExiste", ""))
                .map(|items| items.len())
                .unwrap_or(usize::MAX),
            0
        );
        let _ = fs::remove_dir_all(root);
    }

    /// Membros do tipo declarado no próprio arquivo, que ainda não foi
    /// compilado — o caso da classe que está sendo escrita agora.
    #[test]
    fn members_of_a_class_in_the_open_file_are_offered_after_the_dot() {
        let active = active();
        let source = concat!(
            "class Pedido {\n",
            "    int total;\n",
            "    String descricao() { return \"\"; }\n",
            "    private void interno() {}\n",
            "}\n",
            "class Uso {\n",
            "    void executar() {\n",
            "        Pedido pedido = new Pedido();\n",
            "        pedido.\n",
            "    }\n",
            "}\n",
        );
        if let Err(error) = pollster::block_on(active.open_document(snapshot(source))) {
            panic!("falha ao abrir documento: {error}");
        }
        let line = source[..source.find("pedido.\n").unwrap_or(0)]
            .matches('\n')
            .count() as u32;
        let items = match pollster::block_on(active.completion(CompletionRequest {
            document_id: DocumentId(1),
            position: TextPosition { line, column: 15 },
            prefix: String::new(),
        })) {
            Ok(items) => items,
            Err(error) => panic!("falha na completação: {error}"),
        };
        let labels: Vec<&str> = items.iter().map(|item| item.label.as_str()).collect();
        assert!(labels.contains(&"total"), "campo do tipo: {labels:?}");
        assert!(
            labels.contains(&"descricao()"),
            "método do tipo: {labels:?}"
        );
        assert!(
            !labels.contains(&"interno()"),
            "membro privado não se alcança pelo ponto: {labels:?}"
        );
        // O menu depois do ponto fala do objeto, não do arquivo: nada de
        // palavra-chave nem de classe solta do índice.
        assert!(
            !labels.contains(&"class"),
            "palavra-chave vazou: {labels:?}"
        );
        assert!(!labels.contains(&"Uso"), "classe vizinha vazou: {labels:?}");
    }

    /// O prefixo já digitado filtra os membros.
    #[test]
    fn what_is_typed_after_the_dot_filters_the_members() {
        let active = active();
        let source = concat!(
            "class Pedido {\n",
            "    int total;\n",
            "    String descricao() { return \"\"; }\n",
            "}\n",
            "class Uso {\n",
            "    void executar() {\n",
            "        Pedido pedido = new Pedido();\n",
            "        pedido.des\n",
            "    }\n",
            "}\n",
        );
        if let Err(error) = pollster::block_on(active.open_document(snapshot(source))) {
            panic!("falha ao abrir documento: {error}");
        }
        let line = source[..source.find("pedido.des").unwrap_or(0)]
            .matches('\n')
            .count() as u32;
        let items = match pollster::block_on(active.completion(CompletionRequest {
            document_id: DocumentId(1),
            position: TextPosition { line, column: 18 },
            prefix: "des".to_owned(),
        })) {
            Ok(items) => items,
            Err(error) => panic!("falha na completação: {error}"),
        };
        let labels: Vec<&str> = items.iter().map(|item| item.label.as_str()).collect();
        assert_eq!(labels, vec!["descricao()"]);
    }

    /// Um tipo da biblioteca padrão responde pelos próprios membros.
    ///
    /// `System` não está no workspace nem no arquivo aberto: vem do JDK, que até
    /// então nunca era indexado — era por isso que digitar `System.` não
    /// mostrava nada.
    #[test]
    fn a_jdk_type_offers_its_members() {
        if std::env::var_os("JAVA_HOME").is_none() {
            // Sem JDK apontado não há o que indexar, e inventar um aqui testaria
            // outra coisa que não esta ligação.
            return;
        }
        let active = active();
        let source = "class Uso {\n    void executar() {\n        System.\n    }\n}\n";
        if let Err(error) = pollster::block_on(active.open_document(snapshot(source))) {
            panic!("falha ao abrir documento: {error}");
        }
        let items = match pollster::block_on(active.completion(CompletionRequest {
            document_id: DocumentId(1),
            position: TextPosition {
                line: 2,
                column: 15,
            },
            prefix: String::new(),
        })) {
            Ok(items) => items,
            Err(error) => panic!("falha na completação: {error}"),
        };
        let labels: Vec<&str> = items.iter().map(|item| item.label.as_str()).collect();
        assert!(labels.contains(&"out"), "campo estático: {labels:?}");
        assert!(
            labels
                .iter()
                .any(|label| label.starts_with("currentTimeMillis")),
            "método estático: {labels:?}"
        );
        // Herdado de Object, e tão membro do objeto quanto os declarados.
        assert!(
            labels.iter().any(|label| label.starts_with("toString")),
            "membro herdado: {labels:?}"
        );
    }

    /// O JDK indexado é o que a IDE aponta, não o do ambiente.
    ///
    /// Apontando para uma pasta que não é JDK, a biblioteca padrão não entra —
    /// mesmo com `JAVA_HOME` válido na máquina. É o que garante que trocar de
    /// JDK pelo menu troque as classes que a completação conhece, em vez de
    /// ficar preso num caminho de ambiente.
    #[test]
    fn the_indexed_jdk_is_the_one_the_ide_points_at() {
        let active = active_with_jdk(Some(PathBuf::from("jdk-que-nao-existe")));
        let source = "class Uso {\n    void executar() {\n        System.\n    }\n}\n";
        if let Err(error) = pollster::block_on(active.open_document(snapshot(source))) {
            panic!("falha ao abrir documento: {error}");
        }
        let items = match pollster::block_on(active.completion(CompletionRequest {
            document_id: DocumentId(1),
            position: TextPosition {
                line: 2,
                column: 15,
            },
            prefix: String::new(),
        })) {
            Ok(items) => items,
            Err(error) => panic!("falha na completação: {error}"),
        };
        assert!(
            items.is_empty(),
            "o JDK do ambiente não devia responder: {:?}",
            items.iter().map(|item| &item.label).collect::<Vec<_>>()
        );
    }

    /// Sem ponto, a completação continua sendo a do arquivo inteiro.
    #[test]
    fn without_a_dot_the_completion_still_covers_the_file() {
        let active = active();
        let source = "class Pedido {\n    int total;\n}\n";
        if let Err(error) = pollster::block_on(active.open_document(snapshot(source))) {
            panic!("falha ao abrir documento: {error}");
        }
        let items = match pollster::block_on(active.completion(CompletionRequest {
            document_id: DocumentId(1),
            position: TextPosition { line: 2, column: 1 },
            prefix: "Ped".to_owned(),
        })) {
            Ok(items) => items,
            Err(error) => panic!("falha na completação: {error}"),
        };
        let labels: Vec<&str> = items.iter().map(|item| item.label.as_str()).collect();
        assert!(labels.contains(&"Pedido"), "{labels:?}");
    }

    #[test]
    fn java_8_grammar_accepts_required_language_constructs() {
        let active = active();
        let source = r#"
            import java.util.stream.*;
            @Deprecated
            interface Mapper<T> {
                default T map(T value) { return value; }
                static <T> Mapper<T> identity() { return value -> value; }
            }
            class Example {
                void run() throws Exception {
                    try (AutoCloseable resource = open()) {
                        Mapper<String> mapper = Example::convert;
                    }
                }
            }
        "#;
        assert!(pollster::block_on(active.open_document(snapshot(source))).is_ok());
        let syntax = match pollster::block_on(active.syntax(DocumentId(1))) {
            Ok(syntax) => syntax,
            Err(error) => panic!("syntax unavailable: {error}"),
        };
        assert!(syntax.diagnostics.is_empty(), "{:?}", syntax.diagnostics);
        assert!(
            !syntax.outline.is_empty(),
            "o outline sai da árvore analisada"
        );
    }

    #[test]
    fn extracts_outline_highlights_and_imports() {
        let active = active();
        let source = r#"
            import static java.util.Collections.*;
            public class Example {
                private String name = "ER";
                public void run() {}
            }
        "#;
        assert!(pollster::block_on(active.open_document(snapshot(source))).is_ok());
        let syntax = match pollster::block_on(active.syntax(DocumentId(1))) {
            Ok(syntax) => syntax,
            Err(error) => panic!("syntax unavailable: {error}"),
        };
        assert_eq!(syntax.imports.len(), 1);
        assert!(syntax.imports[0].is_static);
        assert!(syntax.imports[0].wildcard);
        assert!(syntax.outline.iter().any(|item| item.name == "Example"));
        // O campo precisa aparecer pelo nome: `field_declaration` não tem `name`
        // próprio, e sem procurar no declarador todo campo saía `<anonymous>`.
        let membros: Vec<(&str, OutlineKind)> = syntax
            .outline
            .iter()
            .flat_map(|item| item.children.iter())
            .map(|item| (item.name.as_str(), item.kind))
            .collect();
        assert!(
            membros.contains(&("name", OutlineKind::Field)),
            "o campo precisa ser nomeado: {membros:?}"
        );
        assert!(membros.contains(&("run", OutlineKind::Method)));
        assert!(
            syntax
                .highlights
                .iter()
                .any(|span| span.kind == SyntaxHighlightKind::Keyword)
        );
        assert!(
            syntax
                .highlights
                .iter()
                .any(|span| span.kind == SyntaxHighlightKind::String)
        );
    }

    #[test]
    fn reparses_incrementally_and_reports_syntax_errors() {
        let active = active();
        assert!(pollster::block_on(active.open_document(snapshot("class Example { }"))).is_ok());
        let change = DocumentChange {
            document_id: DocumentId(1),
            version: 2,
            range: Some(TextRange {
                start: TextPosition {
                    line: 0,
                    column: 16,
                },
                end: TextPosition {
                    line: 0,
                    column: 17,
                },
            }),
            text: String::new(),
        };
        assert!(pollster::block_on(active.change_document(change)).is_ok());
        let syntax = match pollster::block_on(active.syntax(DocumentId(1))) {
            Ok(syntax) => syntax,
            Err(error) => panic!("syntax unavailable: {error}"),
        };
        assert_eq!(syntax.version, 2);
        assert!(!syntax.diagnostics.is_empty());
    }

    #[test]
    fn provider_runs_through_language_host_contract() {
        let host = ide_language_host::LanguageHost::new(".");
        assert!(
            host.register(std::sync::Arc::new(JavaLanguageProvider::new()))
                .is_ok()
        );
        let selected = pollster::block_on(
            host.open_document(host.request_context(), snapshot("class App {}")),
        );
        assert_eq!(selected.ok(), Some(ProviderId(JAVA_PROVIDER_ID.to_owned())));
        let syntax = pollster::block_on(host.syntax(host.request_context(), DocumentId(1)));
        assert!(syntax.is_ok());
        assert!(pollster::block_on(host.shutdown()).is_ok());
    }

    fn position_of_nth(text: &str, token: &str, occurrence: usize) -> TextPosition {
        let offset = text
            .match_indices(token)
            .nth(occurrence)
            .map_or(0, |(offset, _)| offset);
        let point = point_for_offset(text, offset);
        TextPosition {
            line: point.row as u32,
            column: LineIndex::new(text).position(point).column,
        }
    }

    #[test]
    fn builds_symbols_scopes_types_definitions_references_and_completion() {
        let active = active();
        let source = "class Example {\n  String name;\n  void run(String input) {\n    String local = input;\n    System.out.println(local);\n  }\n}";
        assert!(pollster::block_on(active.open_document(snapshot(source))).is_ok());
        let semantic = match pollster::block_on(active.semantic(DocumentId(1))) {
            Ok(semantic) => semantic,
            Err(error) => panic!("semantic snapshot unavailable: {error}"),
        };
        assert!(semantic.scopes.len() >= 3);
        assert!(semantic.symbols.iter().any(|symbol| {
            symbol.name == "local"
                && symbol
                    .type_descriptor
                    .as_ref()
                    .is_some_and(|kind| kind.name == "String")
        }));

        let reference_position = position_of_nth(source, "local", 1);
        let definitions = pollster::block_on(active.definition(DefinitionRequest {
            document_id: DocumentId(1),
            position: reference_position,
        }));
        assert_eq!(
            definitions
                .ok()
                .and_then(|locations| locations.first().cloned())
                .map(|location| location.range.start),
            Some(position_of_nth(source, "local", 0))
        );
        let references = pollster::block_on(active.references(ReferencesRequest {
            document_id: DocumentId(1),
            position: reference_position,
            include_declaration: true,
        }));
        assert!(references.is_ok_and(|locations| locations.len() >= 2));

        let completions = pollster::block_on(active.completion(CompletionRequest {
            document_id: DocumentId(1),
            position: reference_position,
            prefix: "lo".to_owned(),
        }));
        assert!(completions.is_ok_and(|items| items.iter().any(|item| item.label == "local")));
    }

    #[test]
    fn resolves_definition_from_another_workspace_source() {
        let root =
            std::env::temp_dir().join(format!("er-ide-java-semantic-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        assert!(fs::create_dir_all(&root).is_ok());
        let target = root.join("Target.java");
        assert!(fs::write(&target, "class Target {}").is_ok());
        let provider = JavaLanguageProvider::new();
        let active = match pollster::block_on(provider.activate(LanguageActivationContext {
            workspace_root: root.clone(),
            source_roots: vec![root.clone()],
            toolchains: Vec::new(),
        })) {
            Ok(active) => {
                assert!(
                    pollster::block_on(active.wait_until_indexed(Duration::from_secs(60))),
                    "o índice do projeto não ficou pronto a tempo"
                );
                active
            }
            Err(error) => panic!("provider activation failed: {error}"),
        };
        let source = "class Main { Target value; }";
        let mut main = snapshot(source);
        main.path = root.join("Main.java");
        assert!(pollster::block_on(active.open_document(main)).is_ok());
        let definitions = pollster::block_on(active.definition(DefinitionRequest {
            document_id: DocumentId(1),
            position: position_of_nth(source, "Target", 0),
        }));
        assert!(definitions.is_ok_and(|locations| {
            locations
                .first()
                .is_some_and(|location| location.path == target)
        }));
        let _ = fs::remove_dir_all(root);
    }
}
