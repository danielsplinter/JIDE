#![doc = "Provider Java nativo: gramática, parsing incremental e análise sintática."]

use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    sync::Mutex,
};

use async_trait::async_trait;
use ide_domain::{
    CompletionItem, CompletionKind, CompletionRequest, DefinitionRequest, Diagnostic,
    DiagnosticSeverity, DocumentChange, DocumentId, DocumentSnapshot, ImportItem, LanguageId,
    Location, OutlineItem, OutlineKind, ProviderId, ReferencesRequest, SemanticScope,
    SemanticSnapshot, SemanticSymbol, SymbolKind, SyntaxHighlight, SyntaxHighlightKind, SyntaxNode,
    SyntaxSnapshot, TextPosition, TextRange, TypeDescriptor,
};
use ide_language_api::{
    ActiveLanguage, LANGUAGE_API_VERSION, LanguageActivationContext, LanguageCapabilities,
    LanguageError, LanguageMetadata, LanguageProvider,
};
use tree_sitter::{InputEdit, Node, Parser, Point, Tree};

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
        Ok(Box::new(JavaLanguage::new(&context.workspace_root)?))
    }
}

struct ParsedDocument {
    snapshot: DocumentSnapshot,
    tree: Tree,
    analysis: SyntaxSnapshot,
    semantic: SemanticSnapshot,
    references: HashMap<String, Vec<Location>>,
}

struct JavaLanguage {
    language_id: LanguageId,
    parser: Mutex<Parser>,
    documents: Mutex<HashMap<DocumentId, ParsedDocument>>,
    workspace_index: WorkspaceIndex,
}

impl JavaLanguage {
    fn new(workspace_root: &Path) -> Result<Self, LanguageError> {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_java::LANGUAGE.into())
            .map_err(|error| LanguageError::Provider(error.to_string()))?;
        let workspace_index = WorkspaceIndex::scan(workspace_root, &mut parser);
        Ok(Self {
            language_id: LanguageId(JAVA_LANGUAGE_ID.to_owned()),
            parser: Mutex::new(parser),
            documents: Mutex::new(HashMap::new()),
            workspace_index,
        })
    }

    fn parse(
        &self,
        snapshot: DocumentSnapshot,
        previous: Option<&Tree>,
    ) -> Result<ParsedDocument, LanguageError> {
        let tree = self
            .parser
            .lock()
            .map_err(|_| LanguageError::Provider("Java parser lock poisoned".to_owned()))?
            .parse(&snapshot.text, previous)
            .ok_or_else(|| LanguageError::Provider("Java parsing was cancelled".to_owned()))?;
        let analysis = analyze(&snapshot, &tree);
        let (semantic, references) = analyze_semantics(&snapshot, &tree);
        Ok(ParsedDocument {
            snapshot,
            tree,
            analysis,
            semantic,
            references,
        })
    }
}

#[async_trait]
impl ActiveLanguage for JavaLanguage {
    fn language_id(&self) -> &LanguageId {
        &self.language_id
    }

    async fn open_document(&self, document: DocumentSnapshot) -> Result<(), LanguageError> {
        let id = document.id;
        let parsed = self.parse(document, None)?;
        self.documents
            .lock()
            .map_err(|_| LanguageError::Provider("Java document lock poisoned".to_owned()))?
            .insert(id, parsed);
        Ok(())
    }

    async fn change_document(&self, change: DocumentChange) -> Result<(), LanguageError> {
        let mut documents = self
            .documents
            .lock()
            .map_err(|_| LanguageError::Provider("Java document lock poisoned".to_owned()))?;
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

    async fn close_document(&self, document_id: DocumentId) -> Result<(), LanguageError> {
        self.documents
            .lock()
            .map_err(|_| LanguageError::Provider("Java document lock poisoned".to_owned()))?
            .remove(&document_id);
        Ok(())
    }

    async fn diagnostics(&self, document_id: DocumentId) -> Result<Vec<Diagnostic>, LanguageError> {
        Ok(self.syntax(document_id).await?.diagnostics)
    }

    async fn syntax(&self, document_id: DocumentId) -> Result<SyntaxSnapshot, LanguageError> {
        self.documents
            .lock()
            .map_err(|_| LanguageError::Provider("Java document lock poisoned".to_owned()))?
            .get(&document_id)
            .map(|document| document.analysis.clone())
            .ok_or_else(|| LanguageError::Provider("Java document is not open".to_owned()))
    }

    async fn semantic(&self, document_id: DocumentId) -> Result<SemanticSnapshot, LanguageError> {
        self.documents
            .lock()
            .map_err(|_| LanguageError::Provider("Java document lock poisoned".to_owned()))?
            .get(&document_id)
            .map(|document| document.semantic.clone())
            .ok_or_else(|| LanguageError::Provider("Java document is not open".to_owned()))
    }

    async fn completion(
        &self,
        request: CompletionRequest,
    ) -> Result<Vec<CompletionItem>, LanguageError> {
        let documents = self
            .documents
            .lock()
            .map_err(|_| LanguageError::Provider("Java document lock poisoned".to_owned()))?;
        let document = documents
            .get(&request.document_id)
            .ok_or_else(|| LanguageError::Provider("Java document is not open".to_owned()))?;
        let mut items = Vec::new();
        let mut seen = HashSet::new();
        for symbol in document
            .semantic
            .symbols
            .iter()
            .chain(self.workspace_index.symbols.iter())
        {
            if symbol.name.starts_with(&request.prefix) && seen.insert(symbol.name.clone()) {
                items.push(completion_for_symbol(symbol));
            }
        }
        for (name, path) in &self.workspace_index.external_classes {
            if name.starts_with(&request.prefix) && seen.insert(name.clone()) {
                items.push(CompletionItem {
                    label: name.clone(),
                    detail: Some(format!("class file em {}", path.display())),
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
        let documents = self
            .documents
            .lock()
            .map_err(|_| LanguageError::Provider("Java document lock poisoned".to_owned()))?;
        let document = documents
            .get(&request.document_id)
            .ok_or_else(|| LanguageError::Provider("Java document is not open".to_owned()))?;
        let name = token_at_position(&document.snapshot.text, request.position);
        let mut symbols = documents
            .values()
            .flat_map(|document| document.semantic.symbols.iter())
            .chain(self.workspace_index.symbols.iter())
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
        let documents = self
            .documents
            .lock()
            .map_err(|_| LanguageError::Provider("Java document lock poisoned".to_owned()))?;
        let document = documents
            .get(&request.document_id)
            .ok_or_else(|| LanguageError::Provider("Java document is not open".to_owned()))?;
        let name = token_at_position(&document.snapshot.text, request.position);
        let mut locations = documents
            .values()
            .flat_map(|document| document.references.get(&name).into_iter().flatten())
            .chain(
                self.workspace_index
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
                    .chain(self.workspace_index.symbols.iter())
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
        self.documents
            .lock()
            .map_err(|_| LanguageError::Provider("Java document lock poisoned".to_owned()))?
            .clear();
        Ok(())
    }
}

#[derive(Default)]
struct WorkspaceIndex {
    symbols: Vec<SemanticSymbol>,
    references: HashMap<String, Vec<Location>>,
    external_classes: Vec<(String, PathBuf)>,
}

impl WorkspaceIndex {
    fn scan(root: &Path, parser: &mut Parser) -> Self {
        let mut paths = Vec::new();
        collect_workspace_paths(root, &mut paths, 600);
        let mut index = Self::default();
        let mut java_count = 0;
        let mut archive_count = 0;
        for path in paths {
            match path.extension().and_then(|extension| extension.to_str()) {
                Some(extension) if extension.eq_ignore_ascii_case("java") && java_count < 500 => {
                    if let Ok(text) = fs::read_to_string(&path)
                        && let Some(tree) = parser.parse(&text, None)
                    {
                        let snapshot = DocumentSnapshot {
                            id: DocumentId(0),
                            path: path.clone(),
                            version: 0,
                            text,
                        };
                        let (semantic, references) = analyze_semantics(&snapshot, &tree);
                        index.symbols.extend(semantic.symbols);
                        merge_references(&mut index.references, references);
                        java_count += 1;
                    }
                }
                Some(extension) if extension.eq_ignore_ascii_case("class") => {
                    if let Ok(bytes) = fs::read(&path)
                        && let Ok(class) = java_classfile::read_class(&bytes)
                    {
                        index
                            .external_classes
                            .push((simple_class_name(&class.binary_name), path));
                    }
                }
                Some(extension) if extension.eq_ignore_ascii_case("jar") && archive_count < 64 => {
                    if let Ok(classes) = java_classfile::index_jar(&path, 20_000) {
                        index.external_classes.extend(
                            classes
                                .into_iter()
                                .map(|class| (simple_class_name(&class.binary_name), path.clone())),
                        );
                    }
                    archive_count += 1;
                }
                _ => {}
            }
        }
        index
    }
}

fn collect_workspace_paths(root: &Path, output: &mut Vec<PathBuf>, limit: usize) {
    if output.len() >= limit {
        return;
    }
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        if output.len() >= limit {
            break;
        }
        let path = entry.path();
        if path.is_dir() {
            let ignored = path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| matches!(name, ".git" | "target" | "node_modules" | ".gradle"));
            if !ignored {
                collect_workspace_paths(&path, output, limit);
            }
        } else {
            output.push(path);
        }
    }
}

fn merge_references(
    target: &mut HashMap<String, Vec<Location>>,
    source: HashMap<String, Vec<Location>>,
) {
    for (name, locations) in source {
        target.entry(name).or_default().extend(locations);
    }
}

fn simple_class_name(binary_name: &str) -> String {
    binary_name
        .rsplit('.')
        .next()
        .unwrap_or(binary_name)
        .split('$')
        .next()
        .unwrap_or(binary_name)
        .to_owned()
}

fn analyze_semantics(
    document: &DocumentSnapshot,
    tree: &Tree,
) -> (SemanticSnapshot, HashMap<String, Vec<Location>>) {
    let root = tree.root_node();
    let mut symbols = Vec::new();
    let mut scopes = vec![SemanticScope {
        range: node_range(root, &document.text),
        depth: 0,
        symbols: Vec::new(),
    }];
    let mut references = HashMap::new();
    visit_semantics(
        root,
        document,
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
    parent_scope: usize,
    symbols: &mut Vec<SemanticSymbol>,
    scopes: &mut Vec<SemanticScope>,
    references: &mut HashMap<String, Vec<Location>>,
) {
    let scope_index = if node.kind() != "program" && is_scope(node.kind()) {
        let index = scopes.len();
        scopes.push(SemanticScope {
            range: node_range(node, &document.text),
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
                    range: node_range(name_node, &document.text),
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
                range: node_range(node, &document.text),
            });
    }

    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        visit_semantics(child, document, scope_index, symbols, scopes, references);
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

fn token_at_position(text: &str, position: TextPosition) -> String {
    let offset = match offset_for_position(text, position) {
        Ok(offset) => offset,
        Err(_) => return String::new(),
    };
    let mut start = offset;
    while start > 0 {
        let previous = text[..start]
            .char_indices()
            .last()
            .map_or(0, |(index, _)| index);
        let character = text[previous..start].chars().next();
        if !character.is_some_and(|character| character == '_' || character.is_alphanumeric()) {
            break;
        }
        start = previous;
    }
    let mut end = offset;
    while end < text.len() {
        let Some(character) = text[end..].chars().next() else {
            break;
        };
        if character != '_' && !character.is_alphanumeric() {
            break;
        }
        end += character.len_utf8();
    }
    text[start..end].to_owned()
}

fn analyze(document: &DocumentSnapshot, tree: &Tree) -> SyntaxSnapshot {
    let root = tree.root_node();
    let mut highlights = Vec::new();
    let mut diagnostics = Vec::new();
    collect_highlights(root, &document.text, &mut highlights);
    collect_diagnostics(root, &document.text, &mut diagnostics);
    highlights.sort_by_key(|highlight| {
        (
            highlight.range.start.line,
            highlight.range.start.column,
            highlight.range.end.line,
            highlight.range.end.column,
        )
    });
    SyntaxSnapshot {
        document_id: document.id,
        version: document.version,
        tree: syntax_node(root, &document.text),
        outline: collect_outline(root, &document.text),
        highlights,
        imports: collect_imports(root, &document.text),
        diagnostics,
    }
}

fn syntax_node(node: Node<'_>, source: &str) -> SyntaxNode {
    let mut cursor = node.walk();
    let children = node
        .named_children(&mut cursor)
        .map(|child| syntax_node(child, source))
        .collect();
    SyntaxNode {
        kind: node.kind().to_owned(),
        range: node_range(node, source),
        has_error: node.has_error(),
        children,
    }
}

fn collect_outline(node: Node<'_>, source: &str) -> Vec<OutlineItem> {
    let mut result = Vec::new();
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if let Some(kind) = outline_kind(child.kind()) {
            let name = child
                .child_by_field_name("name")
                .and_then(|name| name.utf8_text(source.as_bytes()).ok())
                .unwrap_or("<anonymous>")
                .to_owned();
            result.push(OutlineItem {
                name,
                kind,
                range: node_range(child, source),
                children: collect_outline(child, source),
            });
        } else {
            result.extend(collect_outline(child, source));
        }
    }
    result
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

fn collect_imports(root: Node<'_>, source: &str) -> Vec<ImportItem> {
    let mut imports = Vec::new();
    let mut cursor = root.walk();
    for node in root.named_children(&mut cursor) {
        if node.kind() != "import_declaration" {
            continue;
        }
        let text = node
            .utf8_text(source.as_bytes())
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
            range: node_range(node, source),
        });
    }
    imports
}

fn collect_highlights(node: Node<'_>, source: &str, output: &mut Vec<SyntaxHighlight>) {
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
        output.push(SyntaxHighlight {
            range: node_range(node, source),
            kind,
        });
        if whole_node {
            return;
        }
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_highlights(child, source, output);
    }
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

fn collect_diagnostics(node: Node<'_>, source: &str, output: &mut Vec<Diagnostic>) {
    if node.is_error() || node.is_missing() {
        let message = if node.is_missing() {
            format!("Esperado `{}`", node.kind())
        } else {
            "Sintaxe Java inválida".to_owned()
        };
        output.push(Diagnostic {
            range: node_range(node, source),
            severity: DiagnosticSeverity::Error,
            message,
            source: Some(JAVA_PROVIDER_ID.to_owned()),
        });
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_diagnostics(child, source, output);
    }
}

fn node_range(node: Node<'_>, source: &str) -> TextRange {
    TextRange {
        start: text_position(source, node.start_position()),
        end: text_position(source, node.end_position()),
    }
}

fn text_position(source: &str, point: Point) -> TextPosition {
    let line = source.lines().nth(point.row).unwrap_or_default();
    let byte_column = point.column.min(line.len());
    let column = line
        .get(..byte_column)
        .map_or(0, |prefix| prefix.chars().count());
    TextPosition {
        line: point.row as u32,
        column: column as u32,
    }
}

fn offset_for_position(text: &str, position: TextPosition) -> Result<usize, LanguageError> {
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

fn point_for_offset(text: &str, offset: usize) -> Point {
    let prefix = &text[..offset.min(text.len())];
    let row = prefix.bytes().filter(|byte| *byte == b'\n').count();
    let column = prefix
        .rsplit_once('\n')
        .map_or(prefix.len(), |(_, line)| line.len());
    Point { row, column }
}

fn point_after_text(start: Point, inserted: &str) -> Point {
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

    fn active() -> Box<dyn ActiveLanguage> {
        match pollster::block_on(
            JavaLanguageProvider::new().activate(LanguageActivationContext {
                workspace_root: ".".into(),
            }),
        ) {
            Ok(active) => active,
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
            let base = texto.find(ancora).unwrap_or_else(|| panic!("âncora {ancora}"));
            let offset = base + ancora.find(alvo).unwrap_or(0);
            let antes = &texto[..offset];
            let line = antes.matches(char::from(10)).count() as u32;
            let column = antes.rsplit(char::from(10)).next().unwrap_or("").chars().count() as u32;
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
        assert!(!syntax.tree.children.is_empty());
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
            column: text_position(text, point).column,
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
        })) {
            Ok(active) => active,
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
