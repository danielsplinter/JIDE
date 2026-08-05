#![doc = "Tipos de domínio independentes de linguagem e infraestrutura."]

use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use serde::{Deserialize, Serialize};

macro_rules! numeric_id {
    ($name:ident) => {
        #[derive(
            Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize,
        )]
        pub struct $name(pub u64);
    };
}

numeric_id!(DocumentId);
numeric_id!(WorkspaceId);
numeric_id!(ProjectId);
numeric_id!(SymbolId);
numeric_id!(ProcessId);
numeric_id!(RequestId);

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct LanguageId(pub String);

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct ProviderId(pub String);

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct TextPosition {
    pub line: u32,
    pub column: u32,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct TextRange {
    pub start: TextPosition,
    pub end: TextPosition,
}

/// Uma troca de texto num intervalo do documento.
///
/// # Por que ela existe, e por que é neutra
///
/// Escolher `HttpClient` numa lista de completação não é só escrever o nome: o
/// arquivo precisa do `import` correspondente, ou o que se sugeriu não compila.
/// Quem sabe qual `import` escrever, onde pô-lo e se já existe um é a
/// linguagem; o que a IDE recebe é **onde trocar e por quê**.
///
/// Nada aqui é de TypeScript. Java tem a mesma necessidade com os seus
/// `import`, e qualquer linguagem que ofereça um nome fora do alcance vai
/// precisar dizer o que mais muda junto.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TextEdit {
    pub range: TextRange,
    pub text: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DocumentSnapshot {
    pub id: DocumentId,
    pub path: PathBuf,
    pub version: u64,
    pub text: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DocumentChange {
    pub document_id: DocumentId,
    pub version: u64,
    pub range: Option<TextRange>,
    pub text: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Location {
    pub path: PathBuf,
    pub range: TextRange,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DiagnosticSeverity {
    Error,
    Warning,
    Information,
    Hint,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Diagnostic {
    pub range: TextRange,
    pub severity: DiagnosticSeverity,
    pub message: String,
    pub source: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SyntaxHighlightKind {
    Keyword,
    Type,
    Function,
    Field,
    Variable,
    String,
    Number,
    Comment,
    Annotation,
    Operator,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SyntaxHighlight {
    pub range: TextRange,
    pub kind: SyntaxHighlightKind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OutlineKind {
    Class,
    Interface,
    Enum,
    Annotation,
    Constructor,
    Method,
    Field,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OutlineItem {
    pub name: String,
    pub kind: OutlineKind,
    /// Toda a declaração, do primeiro modificador ao fecho.
    pub range: TextRange,
    /// Só o nome, dentro da declaração.
    ///
    /// `range` começa onde a declaração começa, que **não** é onde o nome está:
    /// uma anotação ou um comentário acima empurram o começo para cima. Quem
    /// precisa saber se o usuário clicou no nome — renomear, por exemplo —
    /// pergunta por aqui, senão erraria por uma linha justamente nos arquivos
    /// anotados.
    pub name_range: TextRange,
    pub children: Vec<OutlineItem>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImportItem {
    pub path: String,
    pub is_static: bool,
    pub wildcard: bool,
    pub range: TextRange,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SyntaxSnapshot {
    pub document_id: DocumentId,
    pub version: u64,
    pub outline: Vec<OutlineItem>,
    pub highlights: Vec<SyntaxHighlight>,
    pub imports: Vec<ImportItem>,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SymbolKind {
    Package,
    Class,
    /// Registro — uma classe, mas declarada por `record`.
    Record,
    Interface,
    Enum,
    /// Constante de enumeração, como um item declarado dentro de um `enum`.
    EnumConstant,
    Annotation,
    Constructor,
    Method,
    Field,
    Parameter,
    LocalVariable,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TypeDescriptor {
    pub name: String,
    pub array_dimensions: u8,
    pub generic_arguments: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SemanticSymbol {
    pub name: String,
    pub kind: SymbolKind,
    pub location: Location,
    pub type_descriptor: Option<TypeDescriptor>,
    pub scope_depth: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SemanticScope {
    pub range: TextRange,
    pub depth: u32,
    pub symbols: Vec<usize>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SemanticSnapshot {
    pub document_id: DocumentId,
    pub version: u64,
    pub symbols: Vec<SemanticSymbol>,
    pub scopes: Vec<SemanticScope>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompletionKind {
    Keyword,
    Class,
    Interface,
    Enum,
    Constructor,
    Method,
    Field,
    Variable,
}

/// Um acessor que a linguagem sabe escrever para um campo.
///
/// O texto vem pronto da linguagem, e a tela só escolhe quais entram. É o que
/// mantém a IDE sem saber Java: ela mostra `nome` numa lista e insere o trecho
/// que recebeu, sem opinar sobre `get`, `is` ou tipo de retorno.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AccessorCandidate {
    /// Nome do campo, que é o que a tela mostra.
    pub field: String,
    /// Corpo do acessor, pronto para ser inserido. `None` quando já existe.
    pub source: Option<String>,
}

/// O que a linguagem propõe gerar, e onde.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AccessorPlan {
    pub candidates: Vec<AccessorCandidate>,
    /// Linha onde os trechos entram, antes do fecho do tipo.
    pub insert_at: TextPosition,
}

/// Qual acessor gerar.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AccessorKind {
    Getter,
    Setter,
    /// Um construtor com os campos escolhidos como parâmetros.
    ///
    /// Diferente dos outros: eles geram um trecho **por campo**, e este gera um
    /// trecho só a partir do conjunto marcado — nenhum campo marcado é um
    /// construtor sem parâmetros, que é uma resposta legítima e não a ausência
    /// de resposta. Por isso o texto não vem no plano; é pedido à linguagem
    /// depois da escolha, por `constructor_source`.
    Constructor,
    /// Os dois de uma vez.
    ///
    /// Um campo entra na lista quando falta **algum** dos dois, e o texto
    /// gerado traz só o que falta: repetir o que a classe já tem seria erro
    /// de compilação, não conveniência.
    Both,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompletionItem {
    pub label: String,
    pub detail: Option<String>,
    pub kind: CompletionKind,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompletionRequest {
    pub document_id: DocumentId,
    pub position: TextPosition,
    pub prefix: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DefinitionRequest {
    pub document_id: DocumentId,
    pub position: TextPosition,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReferencesRequest {
    pub document_id: DocumentId,
    pub position: TextPosition,
    pub include_declaration: bool,
}

/// Qual das escolhas de uma seção de configurações.
///
/// A contribuição declara os rótulos em `SettingsSection`; ninguém acima sabe o
/// que a ferramenta é. Em Java a principal é o JDK e a segunda o Maven; noutra
/// linguagem serão outras, ou não haverá segunda.
///
/// Mora aqui porque três camadas precisavam do mesmo conceito — a configuração
/// que grava, o comando que diz o que o usuário clicou e a janela que desenha —
/// e três definições da mesma coisa discordam um dia. Ver a fase 0 da `23`.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub enum ToolRole {
    Primary,
    Secondary,
}

impl ToolRole {
    /// Chave estável para gravar em arquivo.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Primary => "primary",
            Self::Secondary => "secondary",
        }
    }
}

/// Aviso de que ninguém mais quer o resultado desta operação.
///
/// Mora aqui, e não no contrato de linguagens onde nasceu, porque cancelar não
/// é assunto de linguagem: quem lê o disco, fala com o Git ou consulta um
/// processo externo cancela pelo mesmo motivo. Um segundo tipo com o mesmo nome
/// noutra crate discordaria um dia, e a discordância apareceria como
/// comportamento estranho e não como erro de compilação. Ver a `22`.
///
/// Quem observa o token decide onde olhar. Ele não interrompe trabalho em curso;
/// ele diz que o trabalho deixou de valer.
#[derive(Clone, Debug, Default)]
pub struct CancellationToken {
    cancelled: Arc<AtomicBool>,
}

impl CancellationToken {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }

    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn range_preserves_zero_based_positions() {
        let range = TextRange {
            start: TextPosition { line: 2, column: 4 },
            end: TextPosition { line: 2, column: 9 },
        };
        assert_eq!(range.start.line, 2);
        assert_eq!(range.end.column, 9);
    }

    #[test]
    fn domain_manifest_has_no_infrastructure_dependency() {
        let manifest = include_str!("../Cargo.toml");
        for forbidden in ["wgpu", "winit", "tree-sitter", "tokio", "tracing"] {
            assert!(
                !manifest.contains(forbidden),
                "{forbidden} leaked into ide-domain"
            );
        }
    }
}
