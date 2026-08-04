//! Realce, estrutura e erros de sintaxe, numa passada só sobre a árvore.
//!
//! É a mesma regra que a `16` fixou para Java: uma travessia produz tudo o que
//! o `SyntaxSnapshot` carrega. Percorrer de novo por consumidor custaria o dobro
//! a cada tecla digitada.

use ide_domain::{
    Diagnostic, DiagnosticSeverity, OutlineItem, OutlineKind, SyntaxHighlight, SyntaxHighlightKind,
};
use tree_sitter::{Node, Tree};

use super::lines::{LineIndex, node_range};

/// O que uma passada pela árvore produz.
pub(crate) struct SyntaxPass {
    pub(crate) highlights: Vec<SyntaxHighlight>,
    pub(crate) outline: Vec<OutlineItem>,
    pub(crate) diagnostics: Vec<Diagnostic>,
}

/// # `com_estrutura` existe porque a estrutura tem outro ritmo
///
/// O realce muda a cada tecla; a lista de símbolos, não. Medido num arquivo de
/// 3 144 linhas: montá-la é **35%** do custo de cada tecla, para produzir catorze
/// itens que quase nunca mudam.
///
/// Quem chama com `false` reaproveita a lista anterior. Ver `analyze` no provider
/// nativo, que decide quando isso vale.
pub(crate) fn analyze(tree: &Tree, source: &str, com_estrutura: bool) -> SyntaxPass {
    let lines = LineIndex::new(source);
    let mut pass = SyntaxPass {
        highlights: Vec::new(),
        outline: Vec::new(),
        diagnostics: Vec::new(),
    };
    let root = tree.root_node();
    walk(root, None, &lines, &mut pass);
    if com_estrutura {
        pass.outline = outline_of(root, &lines);
    }
    pass
}

fn walk(node: Node<'_>, pai: Option<Node<'_>>, lines: &LineIndex<'_>, pass: &mut SyntaxPass) {
    if node.is_error() || node.is_missing() {
        pass.diagnostics.push(Diagnostic {
            range: node_range(node, lines),
            severity: DiagnosticSeverity::Error,
            message: if node.is_missing() {
                format!("Falta {}", node.kind())
            } else {
                "Trecho que não é TypeScript válido".to_owned()
            },
            source: Some("typescript".to_owned()),
        });
    }
    if let Some(kind) = highlight_kind(node, pai) {
        pass.highlights.push(SyntaxHighlight {
            range: node_range(node, lines),
            kind,
        });
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk(child, Some(node), lines, pass);
    }
}

/// Só as declarações de primeiro nível e os membros de tipo entram.
///
/// A estrutura serve para navegar o arquivo, e uma lista com toda expressão
/// aninhada não seria navegação — seria a árvore outra vez.
fn outline_of(node: Node<'_>, lines: &LineIndex<'_>) -> Vec<OutlineItem> {
    let mut items = Vec::new();
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        // `export class X` embrulha a declaração; o que interessa é o que está
        // dentro, senão a lista teria um item sem nome por exportação.
        if matches!(child.kind(), "export_statement") {
            items.extend(outline_of(child, lines));
            continue;
        }
        let Some(kind) = outline_kind(child.kind()) else {
            items.extend(outline_of(child, lines));
            continue;
        };
        let Some(name_node) = child.child_by_field_name("name") else {
            continue;
        };
        let Ok(name) = name_node.utf8_text(lines.source().as_bytes()) else {
            continue;
        };
        let body = child
            .child_by_field_name("body")
            .map(|body| outline_of(body, lines))
            .unwrap_or_default();
        items.push(OutlineItem {
            name: name.to_owned(),
            kind,
            range: node_range(child, lines),
            name_range: node_range(name_node, lines),
            children: body,
        });
    }
    items
}

/// O vocabulário de estrutura do domínio é o de Java, e não cobre TypeScript.
///
/// `type` e `function` solta não têm correspondente: o mais próximo honesto é
/// `Class` para o que declara um tipo nomeado e `Method` para o que declara
/// código chamável. Traduzir mal aqui é melhor do que alargar o contrato por uma
/// linguagem — mas fica anotado como dívida do dia em que a terceira chegar.
fn outline_kind(kind: &str) -> Option<OutlineKind> {
    match kind {
        "class_declaration" | "abstract_class_declaration" => Some(OutlineKind::Class),
        "interface_declaration" => Some(OutlineKind::Interface),
        "enum_declaration" => Some(OutlineKind::Enum),
        "type_alias_declaration" => Some(OutlineKind::Class),
        "function_declaration" | "method_definition" | "method_signature" => {
            Some(OutlineKind::Method)
        }
        "public_field_definition" | "property_signature" => Some(OutlineKind::Field),
        _ => None,
    }
}

/// Como um nó é classificado, decidido **uma vez por espécie**.
#[derive(Clone, Copy, Eq, PartialEq)]
enum Classe {
    /// A espécie basta para saber a cor.
    Fixa(SyntaxHighlightKind),
    /// Um identificador: a cor depende de onde ele está.
    PeloPai,
    Nenhuma,
}

/// A cor de cada espécie de nó, indexada pelo número que o tree-sitter dá a ela.
///
/// # Por que esta tabela existe
///
/// A classificação era feita **por texto**, a cada nó: uma busca de substring
/// mais duas varreduras lineares de 61 entradas cada. Medido num arquivo de
/// 3 144 linhas, com 24 514 nós: percorrer a árvore custava 6,5 ms, e
/// classificá-la **20,9 ms** — cerca de três milhões de comparações de texto por
/// tecla.
///
/// O `kind_id` é um número, e a gramática tem algumas centenas de espécies. A
/// tabela é montada uma vez, com as mesmas regras escritas em texto — a
/// legibilidade fica onde estava, e a comparação vira um índice.
fn tabela() -> &'static [Classe] {
    static TABELA: std::sync::OnceLock<Vec<Classe>> = std::sync::OnceLock::new();
    TABELA.get_or_init(|| {
        let linguagem: tree_sitter::Language = tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into();
        (0..linguagem.node_kind_count())
            .map(|id| {
                let Ok(id) = u16::try_from(id) else {
                    return Classe::Nenhuma;
                };
                linguagem
                    .node_kind_for_id(id)
                    .map_or(Classe::Nenhuma, classe_da_especie)
            })
            .collect()
    })
}

/// A regra, escrita como sempre esteve — e lida uma vez por espécie.
fn classe_da_especie(kind: &str) -> Classe {
    if kind.contains("comment") {
        return Classe::Fixa(SyntaxHighlightKind::Comment);
    }
    if matches!(
        kind,
        "string" | "string_fragment" | "template_string" | "regex"
    ) {
        return Classe::Fixa(SyntaxHighlightKind::String);
    }
    if kind == "number" {
        return Classe::Fixa(SyntaxHighlightKind::Number);
    }
    if matches!(kind, "type_identifier" | "predefined_type") {
        return Classe::Fixa(SyntaxHighlightKind::Type);
    }
    if kind == "decorator" {
        return Classe::Fixa(SyntaxHighlightKind::Annotation);
    }
    if matches!(kind, "identifier" | "property_identifier") {
        return Classe::PeloPai;
    }
    if TYPESCRIPT_KEYWORDS.contains(&kind) {
        return Classe::Fixa(SyntaxHighlightKind::Keyword);
    }
    if TYPESCRIPT_OPERATORS.contains(&kind) {
        return Classe::Fixa(SyntaxHighlightKind::Operator);
    }
    Classe::Nenhuma
}

/// # Por que o pai vem de fora
///
/// `Node::parent()` **não é barato**: o tree-sitter o reconstrói descendo a
/// árvore, e chamá-lo por identificador custou 14 ms num arquivo de 3 144
/// linhas — mais do que a travessia inteira, que é 6 ms.
///
/// Quem percorre já sabe quem é o pai, porque veio dele. Passá-lo adiante
/// troca uma reconstrução por um argumento.
fn highlight_kind(node: Node<'_>, pai: Option<Node<'_>>) -> Option<SyntaxHighlightKind> {
    match tabela()
        .get(node.kind_id() as usize)
        .copied()
        .unwrap_or(Classe::Nenhuma)
    {
        Classe::Fixa(kind) => Some(kind),
        // Só os identificadores olham em volta, e são a minoria dos nós.
        Classe::PeloPai => pai.map(|parent| match parent.kind() {
            "function_declaration" | "method_definition" | "method_signature"
            | "call_expression" => SyntaxHighlightKind::Function,
            "public_field_definition" | "property_signature" | "member_expression" => {
                SyntaxHighlightKind::Field
            }
            // Como em Java: qualquer outro identificador é uma **referência** a
            // algo declarado noutro lugar, e classificá-los só na declaração
            // deixaria o uso sem realce — e sem realce a interface não sabe que
            // dali dá para navegar.
            _ => SyntaxHighlightKind::Variable,
        }),
        Classe::Nenhuma => None,
    }
}

const TYPESCRIPT_KEYWORDS: &[&str] = &[
    "abstract",
    "any",
    "as",
    "async",
    "await",
    "break",
    "case",
    "catch",
    "class",
    "const",
    "continue",
    "debugger",
    "declare",
    "default",
    "delete",
    "do",
    "else",
    "enum",
    "export",
    "extends",
    "false",
    "finally",
    "for",
    "from",
    "function",
    "get",
    "if",
    "implements",
    "import",
    "in",
    "instanceof",
    "interface",
    "is",
    "keyof",
    "let",
    "namespace",
    "new",
    "null",
    "of",
    "override",
    "private",
    "protected",
    "public",
    "readonly",
    "return",
    "satisfies",
    "set",
    "static",
    "super",
    "switch",
    "this",
    "throw",
    "true",
    "try",
    "type",
    "typeof",
    "undefined",
    "var",
    "void",
    "while",
    "yield",
];

const TYPESCRIPT_OPERATORS: &[&str] = &[
    "!", "!=", "!==", "%", "&&", "*", "**", "+", "++", "-", "--", "->", "/", "<", "<=", "=", "==",
    "===", "=>", ">", ">=", "?", "??", "?.", "|", "||",
];
