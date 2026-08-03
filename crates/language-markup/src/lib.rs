#![doc = "Marcação: realce e estrutura de um `.html`, e nada além."]
#![doc = ""]
#![doc = "# O que este provider é, e o que ele deliberadamente não é"]
#![doc = ""]
#![doc = "Ele é **HTML puro**. Não sabe o que é Angular, nem Vue, nem template de"]
#![doc = "coisa nenhuma: `@if`, `@for` e `{{ }}` são texto comum para ele — não"]
#![doc = "destacados, e **não marcados como erro**."]
#![doc = ""]
#![doc = "É a mesma regra que a `24` fixa e que a `23` já aplicou a SCSS: **o que"]
#![doc = "não se entende cala, e não acusa**. Acusar erro num template correto"]
#![doc = "seria pior do que calar, e uma gramática de template nossa envelheceria"]
#![doc = "sozinha a cada versão maior do framework."]
#![doc = ""]
#![doc = "Quem responde por tipo dentro do template é o analisador, pelo plugin da"]
#![doc = "ADR-029. Este provider e aquele **atendem o mesmo arquivo ao mesmo"]
#![doc = "tempo**, cada um com as capacidades que tem; é a composição da `04`."]

use std::{collections::HashMap, sync::Mutex};

use async_trait::async_trait;
use ide_domain::{
    Diagnostic, DocumentChange, DocumentId, DocumentSnapshot, LanguageId, OutlineItem, OutlineKind, ProviderId,
    SyntaxHighlight, SyntaxHighlightKind, SyntaxSnapshot, TextPosition, TextRange,
};
use ide_language_api::{
    ActiveLanguage, LANGUAGE_API_VERSION, LanguageActivationContext, LanguageCapabilities,
    LanguageError, LanguageMetadata, LanguageProvider,
};
use tree_sitter::{Node, Parser, Point, Tree};

pub const MARKUP_LANGUAGE_ID: &str = "markup";
pub const MARKUP_PROVIDER_ID: &str = "markup.basic";

pub struct MarkupLanguageProvider;

impl MarkupLanguageProvider {
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl Default for MarkupLanguageProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl LanguageProvider for MarkupLanguageProvider {
    fn metadata(&self) -> LanguageMetadata {
        LanguageMetadata {
            language_id: LanguageId(MARKUP_LANGUAGE_ID.to_owned()),
            provider_id: ProviderId(MARKUP_PROVIDER_ID.to_owned()),
            display_name: "Marcação".to_owned(),
            extensions: vec!["html".to_owned(), "htm".to_owned()],
            api_version: LANGUAGE_API_VERSION,
            trigger_characters: Vec::new(),
        }
    }

    /// Realce e estrutura, e **nenhum diagnóstico**.
    ///
    /// A gramática é a de HTML, e um template de framework não é HTML válido
    /// inteiro. Anunciar `DIAGNOSTICS` faria a IDE sublinhar de vermelho um
    /// arquivo correto — que é a resposta errada com cara de certa.
    fn capabilities(&self) -> LanguageCapabilities {
        LanguageCapabilities::SYNTAX
    }

    async fn activate(
        &self,
        _context: LanguageActivationContext,
    ) -> Result<Box<dyn ActiveLanguage>, LanguageError> {
        Ok(Box::new(ActiveMarkup::new()?))
    }
}

struct Documento {
    texto: String,
    snapshot: SyntaxSnapshot,
}

struct ActiveMarkup {
    language_id: LanguageId,
    parser: Mutex<Parser>,
    documentos: Mutex<HashMap<DocumentId, Documento>>,
}

impl ActiveMarkup {
    fn new() -> Result<Self, LanguageError> {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_html::LANGUAGE.into())
            .map_err(|erro| LanguageError::Provider(erro.to_string()))?;
        Ok(Self {
            language_id: LanguageId(MARKUP_LANGUAGE_ID.to_owned()),
            parser: Mutex::new(parser),
            documentos: Mutex::new(HashMap::new()),
        })
    }

    fn analisar(
        &self,
        document_id: DocumentId,
        version: u64,
        texto: &str,
    ) -> Result<Documento, LanguageError> {
        let arvore: Tree = self
            .parser
            .lock()
            .map_err(|_| LanguageError::Provider("parser de marcação travado".to_owned()))?
            .parse(texto, None)
            .ok_or_else(|| LanguageError::Provider("análise cancelada".to_owned()))?;
        let linhas = LineIndex::new(texto);
        let mut highlights = Vec::new();
        percorrer(arvore.root_node(), &linhas, &mut highlights);
        Ok(Documento {
            texto: texto.to_owned(),
            snapshot: SyntaxSnapshot {
                document_id,
                version,
                outline: estrutura(arvore.root_node(), &linhas),
                highlights,
                imports: Vec::new(),
                diagnostics: Vec::new(),
            },
        })
    }
}

#[async_trait]
impl ActiveLanguage for ActiveMarkup {
    fn language_id(&self) -> &LanguageId {
        &self.language_id
    }

    async fn open_document(&self, document: DocumentSnapshot) -> Result<(), LanguageError> {
        let analisado = self.analisar(document.id, document.version, &document.text)?;
        self.documentos
            .lock()
            .map_err(|_| LanguageError::Provider("documentos de marcação travados".to_owned()))?
            .insert(document.id, analisado);
        Ok(())
    }

    async fn change_document(&self, change: DocumentChange) -> Result<(), LanguageError> {
        let texto = {
            let documentos = self.documentos.lock().map_err(|_| {
                LanguageError::Provider("documentos de marcação travados".to_owned())
            })?;
            let Some(atual) = documentos.get(&change.document_id) else {
                return Ok(());
            };
            aplicar(&atual.texto, change.range, &change.text)
        };
        let analisado = self.analisar(change.document_id, change.version, &texto)?;
        self.documentos
            .lock()
            .map_err(|_| LanguageError::Provider("documentos de marcação travados".to_owned()))?
            .insert(change.document_id, analisado);
        Ok(())
    }

    async fn close_document(&self, document_id: DocumentId) -> Result<(), LanguageError> {
        self.documentos
            .lock()
            .map_err(|_| LanguageError::Provider("documentos de marcação travados".to_owned()))?
            .remove(&document_id);
        Ok(())
    }

    /// Sempre vazio, e é o desenho.
    ///
    /// Este provider não anuncia `DIAGNOSTICS`, mas o contrato exige o método.
    /// Devolver a lista vazia diz o que ele tem a dizer sobre um template:
    /// nada, porque ele não sabe julgá-lo.
    async fn diagnostics(&self, _document_id: DocumentId) -> Result<Vec<Diagnostic>, LanguageError> {
        Ok(Vec::new())
    }

    async fn syntax(&self, document_id: DocumentId) -> Result<SyntaxSnapshot, LanguageError> {
        self.documentos
            .lock()
            .map_err(|_| LanguageError::Provider("documentos de marcação travados".to_owned()))?
            .get(&document_id)
            .map(|documento| documento.snapshot.clone())
            .ok_or_else(|| LanguageError::Provider("documento não está aberto".to_owned()))
    }

    async fn shutdown(&self) -> Result<(), LanguageError> {
        self.documentos
            .lock()
            .map_err(|_| LanguageError::Provider("documentos de marcação travados".to_owned()))?
            .clear();
        Ok(())
    }
}

/// Nenhum nó de erro vira diagnóstico, de propósito.
///
/// Um template de framework tem sintaxe que a gramática de HTML não conhece, e
/// ela produz nós de erro em arquivo correto. O realce sobrevive a isso — o que
/// ela entendeu continua colorido —, e o que ela não entendeu fica sem cor, e
/// não vermelho.
fn percorrer(node: Node<'_>, linhas: &LineIndex<'_>, highlights: &mut Vec<SyntaxHighlight>) {
    if let Some(kind) = classificar(node) {
        highlights.push(SyntaxHighlight {
            range: intervalo(node, linhas),
            kind,
        });
    }
    let mut cursor = node.walk();
    for filho in node.children(&mut cursor) {
        percorrer(filho, linhas, highlights);
    }
}

/// A estrutura de um documento de marcação é o aninhamento dos elementos.
///
/// O nome é a etiqueta, com o `id` ou a primeira classe quando houver: numa
/// página com trinta `div`, a lista sem isso não ajudaria ninguém.
fn estrutura(node: Node<'_>, linhas: &LineIndex<'_>) -> Vec<OutlineItem> {
    let mut itens = Vec::new();
    let mut cursor = node.walk();
    for filho in node.named_children(&mut cursor) {
        if filho.kind() != "element" {
            itens.extend(estrutura(filho, linhas));
            continue;
        }
        let Some(abertura) = filho.named_child(0) else {
            continue;
        };
        let mut interno = abertura.walk();
        let Some(etiqueta) = abertura
            .named_children(&mut interno)
            .find(|no| no.kind() == "tag_name")
        else {
            continue;
        };
        let Ok(nome) = etiqueta.utf8_text(linhas.source().as_bytes()) else {
            continue;
        };
        itens.push(OutlineItem {
            name: match qualificador(abertura, linhas) {
                Some(extra) => format!("{nome}{extra}"),
                None => nome.to_owned(),
            },
            kind: OutlineKind::Class,
            range: intervalo(filho, linhas),
            name_range: intervalo(etiqueta, linhas),
            children: estrutura(filho, linhas),
        });
    }
    itens
}

/// `#id` ou `.classe` da etiqueta de abertura, se houver.
fn qualificador(abertura: Node<'_>, linhas: &LineIndex<'_>) -> Option<String> {
    let fonte = linhas.source().as_bytes();
    let mut cursor = abertura.walk();
    for atributo in abertura.named_children(&mut cursor) {
        if atributo.kind() != "attribute" {
            continue;
        }
        let mut interno = atributo.walk();
        let filhos = atributo.named_children(&mut interno).collect::<Vec<_>>();
        let Some(nome) = filhos.first().and_then(|no| no.utf8_text(fonte).ok()) else {
            continue;
        };
        let Some(valor) = filhos.get(1).and_then(|no| no.utf8_text(fonte).ok()) else {
            continue;
        };
        let valor = valor.trim_matches(['"', '\'']);
        if valor.is_empty() {
            continue;
        }
        if nome.eq_ignore_ascii_case("id") {
            return Some(format!("#{valor}"));
        }
        if nome.eq_ignore_ascii_case("class") {
            return valor.split_whitespace().next().map(|um| format!(".{um}"));
        }
    }
    None
}

fn classificar(node: Node<'_>) -> Option<SyntaxHighlightKind> {
    match node.kind() {
        "comment" | "doctype" => Some(SyntaxHighlightKind::Comment),
        "tag_name" => Some(SyntaxHighlightKind::Keyword),
        "attribute_name" => Some(SyntaxHighlightKind::Field),
        "attribute_value" | "quoted_attribute_value" => Some(SyntaxHighlightKind::String),
        "entity" => Some(SyntaxHighlightKind::Number),
        _ => None,
    }
}

struct LineIndex<'a> {
    source: &'a str,
    starts: Vec<usize>,
}

impl<'a> LineIndex<'a> {
    fn new(source: &'a str) -> Self {
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

    const fn source(&self) -> &'a str {
        self.source
    }

    /// O tree-sitter conta colunas em bytes; o domínio conta em caracteres.
    fn posicao(&self, point: Point) -> TextPosition {
        let linha = self.linha(point.row);
        let byte = point.column.min(linha.len());
        TextPosition {
            line: u32::try_from(point.row).unwrap_or(u32::MAX),
            column: u32::try_from(linha.get(..byte).map_or(0, |antes| antes.chars().count()))
                .unwrap_or(u32::MAX),
        }
    }

    fn linha(&self, row: usize) -> &'a str {
        let Some(inicio) = self.starts.get(row).copied() else {
            return "";
        };
        let fim = self
            .starts
            .get(row + 1)
            .map_or(self.source.len(), |proximo| proximo.saturating_sub(1));
        let linha = self.source.get(inicio..fim).unwrap_or_default();
        linha.strip_suffix('\r').unwrap_or(linha)
    }
}

fn intervalo(node: Node<'_>, linhas: &LineIndex<'_>) -> TextRange {
    TextRange {
        start: linhas.posicao(node.start_position()),
        end: linhas.posicao(node.end_position()),
    }
}

fn aplicar(atual: &str, range: Option<TextRange>, texto: &str) -> String {
    let Some(range) = range else {
        return texto.to_owned();
    };
    let inicio = deslocamento(atual, range.start.line as usize, range.start.column as usize);
    let fim = deslocamento(atual, range.end.line as usize, range.end.column as usize);
    let mut novo = String::with_capacity(atual.len() + texto.len());
    novo.push_str(atual.get(..inicio).unwrap_or_default());
    novo.push_str(texto);
    novo.push_str(atual.get(fim..).unwrap_or_default());
    novo
}

fn deslocamento(fonte: &str, linha: usize, coluna: usize) -> usize {
    let mut offset = 0;
    for (indice, atual) in fonte.split_inclusive('\n').enumerate() {
        if indice == linha {
            let limpa = atual.strip_suffix('\n').unwrap_or(atual);
            let limpa = limpa.strip_suffix('\r').unwrap_or(limpa);
            return offset
                + limpa
                    .char_indices()
                    .nth(coluna)
                    .map_or(limpa.len(), |(byte, _)| byte);
        }
        offset += atual.len();
    }
    fonte.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn abrir(texto: &str) -> SyntaxSnapshot {
        let ativo = match ActiveMarkup::new() {
            Ok(ativo) => ativo,
            Err(erro) => panic!("o parser de marcação precisa subir: {erro}"),
        };
        let documento = DocumentSnapshot {
            id: DocumentId(1),
            path: std::path::PathBuf::from("pagina.html"),
            version: 1,
            text: texto.to_owned(),
        };
        assert!(pollster::block_on(ativo.open_document(documento)).is_ok());
        match pollster::block_on(ativo.syntax(DocumentId(1))) {
            Ok(snapshot) => snapshot,
            Err(erro) => panic!("o realce precisa existir: {erro}"),
        }
    }

    #[test]
    fn uma_etiqueta_ganha_cor() {
        let snapshot = abrir("<p class=\"nota\">oi</p>\n");
        assert!(
            snapshot
                .highlights
                .iter()
                .any(|realce| realce.kind == SyntaxHighlightKind::Keyword),
            "a etiqueta precisa ser destacada: {:?}",
            snapshot.highlights
        );
        assert!(
            snapshot
                .highlights
                .iter()
                .any(|realce| realce.kind == SyntaxHighlightKind::Field),
            "o nome do atributo precisa ser destacado"
        );
    }

    /// **O critério que separa este provider de uma gramática de template.**
    ///
    /// Um template de Angular tem `@if`, `@for` e `{{ }}`. Nada disso é HTML, e
    /// nada disso pode virar erro — o arquivo está correto.
    #[test]
    fn sintaxe_de_framework_nao_vira_erro() {
        let snapshot = abrir(
            "@if (jogador(); as atual) {\n  <p>{{ atual.nome }}</p>\n} @else {\n  <span>-</span>\n}\n",
        );
        assert!(
            snapshot.diagnostics.is_empty(),
            "o que não se entende cala, e não acusa: {:?}",
            snapshot.diagnostics
        );
    }

    /// E o realce sobrevive ao que a gramática não entendeu: o `<p>` no meio de
    /// um bloco `@if` continua colorido.
    #[test]
    fn o_realce_sobrevive_ao_que_a_gramatica_nao_entende() {
        let snapshot = abrir("@if (x) {\n  <p class=\"a\">oi</p>\n}\n");
        assert!(
            snapshot
                .highlights
                .iter()
                .any(|realce| realce.kind == SyntaxHighlightKind::Keyword),
            "a etiqueta dentro do bloco precisa continuar destacada: {:?}",
            snapshot.highlights
        );
    }

    #[test]
    fn a_estrutura_qualifica_pelo_id_ou_pela_classe() {
        let snapshot =
            abrir("<div id=\"topo\">\n  <span class=\"aviso destaque\">oi</span>\n</div>\n");
        let Some(raiz) = snapshot.outline.first() else {
            panic!("a estrutura precisa ter a raiz: {:?}", snapshot.outline);
        };
        assert_eq!(raiz.name, "div#topo");
        let Some(filho) = raiz.children.first() else {
            panic!("o filho precisa aparecer: {:?}", raiz.children);
        };
        assert_eq!(filho.name, "span.aviso");
    }

    /// Uma linha com acento desloca a coluna: o tree-sitter conta bytes, e o
    /// domínio conta caracteres.
    #[test]
    fn a_coluna_e_de_caracteres_e_nao_de_bytes() {
        let snapshot = abrir("<p>ção</p>\n");
        let fim = snapshot
            .highlights
            .iter()
            .filter(|realce| realce.kind == SyntaxHighlightKind::Keyword)
            .map(|realce| realce.range.end.column)
            .max();
        assert_eq!(
            fim,
            Some(9),
            "colunas em bytes dariam 11: {:?}",
            snapshot.highlights
        );
    }
}
