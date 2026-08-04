#![doc = "Folhas de estilo: realce e estrutura, e nada além."]
#![doc = ""]
#![doc = "Completação de propriedade e resolução de seletor são um projeto"]
#![doc = "próprio, e a `23` os deixou de fora de propósito. O que existe aqui é o"]
#![doc = "mínimo que faz um arquivo de estilo deixar de ser texto cru."]

use std::{collections::HashMap, sync::Mutex};

use async_trait::async_trait;
use ide_domain::{
    CompletionItem, CompletionKind, CompletionRequest, Diagnostic, DiagnosticSeverity,
    DocumentChange, DocumentId, DocumentSnapshot, LanguageId, OutlineItem, OutlineKind, ProviderId,
    SyntaxHighlight, SyntaxHighlightKind, SyntaxSnapshot, TextPosition, TextRange,
};
use ide_language_api::{
    ActiveLanguage, LANGUAGE_API_VERSION, LanguageActivationContext, LanguageCapabilities,
    LanguageError, LanguageMetadata, LanguageProvider,
};
use tree_sitter::{Node, Parser, Point, Tree};

pub const STYLE_LANGUAGE_ID: &str = "style";
pub const STYLE_PROVIDER_ID: &str = "style.basic";

/// A extensão que a gramática julga, e a que ela só realça.
///
/// A gramática é a de CSS. Ela **realça** SCSS quase todo — medido, quatro nós
/// ruins de sessenta e dois numa amostra pequena —, mas não sabe julgá-lo: `$cor`
/// e `@mixin` viram erro num arquivo válido.
///
/// Por isso o diagnóstico sai só para `.css`. Acusar erro num SCSS correto seria
/// pior do que calar, e é a mesma regra que a `24` fixa para o template: **o que
/// não se entende cala, e não acusa**.
const JULGADA: &str = "css";

pub struct StyleLanguageProvider;

impl StyleLanguageProvider {
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl Default for StyleLanguageProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl LanguageProvider for StyleLanguageProvider {
    fn metadata(&self) -> LanguageMetadata {
        LanguageMetadata {
            language_id: LanguageId(STYLE_LANGUAGE_ID.to_owned()),
            provider_id: ProviderId(STYLE_PROVIDER_ID.to_owned()),
            display_name: "Folhas de estilo".to_owned(),
            // `scss` entra porque é o que um projeto Angular usa de verdade: o
            // `angular.json` declara `"style": "scss"`, e um provider só de
            // `.css` não atenderia arquivo nenhum.
            extensions: vec!["css".to_owned(), "scss".to_owned()],
            api_version: LANGUAGE_API_VERSION,
            // Cada um destes começa **um nome que o arquivo inventou**, e é
            // isso que os separa de uma propriedade de CSS: o que vem depois
            // deles só pode ser sabido lendo este arquivo.
            trigger_characters: vec!['$', '%'],
        }
    }

    fn capabilities(&self) -> LanguageCapabilities {
        LanguageCapabilities::SYNTAX
            | LanguageCapabilities::DIAGNOSTICS
            | LanguageCapabilities::COMPLETION
    }

    async fn activate(
        &self,
        _context: LanguageActivationContext,
    ) -> Result<Box<dyn ActiveLanguage>, LanguageError> {
        Ok(Box::new(ActiveStyle::new()?))
    }
}

struct Documento {
    texto: String,
    /// Se este arquivo pode ser julgado por esta gramática.
    ///
    /// Decidido na abertura, pela extensão, e guardado: a mudança não traz o
    /// caminho, e reanalisar precisa da mesma decisão.
    julgar: bool,
    snapshot: SyntaxSnapshot,
}

struct ActiveStyle {
    language_id: LanguageId,
    parser: Mutex<Parser>,
    documentos: Mutex<HashMap<DocumentId, Documento>>,
}

impl ActiveStyle {
    fn new() -> Result<Self, LanguageError> {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_css::LANGUAGE.into())
            .map_err(|erro| LanguageError::Provider(erro.to_string()))?;
        Ok(Self {
            language_id: LanguageId(STYLE_LANGUAGE_ID.to_owned()),
            parser: Mutex::new(parser),
            documentos: Mutex::new(HashMap::new()),
        })
    }

    fn analisar(
        &self,
        document_id: DocumentId,
        version: u64,
        texto: &str,
        julgar: bool,
    ) -> Result<Documento, LanguageError> {
        let arvore: Tree = self
            .parser
            .lock()
            .map_err(|_| LanguageError::Provider("parser de estilo travado".to_owned()))?
            .parse(texto, None)
            .ok_or_else(|| LanguageError::Provider("análise cancelada".to_owned()))?;
        let linhas = LineIndex::new(texto);
        let mut highlights = Vec::new();
        let mut diagnostics = Vec::new();
        percorrer(
            arvore.root_node(),
            &linhas,
            julgar,
            &mut highlights,
            &mut diagnostics,
        );
        Ok(Documento {
            texto: texto.to_owned(),
            julgar,
            snapshot: SyntaxSnapshot {
                document_id,
                version,
                outline: estrutura(arvore.root_node(), &linhas),
                highlights,
                imports: Vec::new(),
                diagnostics,
            },
        })
    }
}

#[async_trait]
impl ActiveLanguage for ActiveStyle {
    fn language_id(&self) -> &LanguageId {
        &self.language_id
    }

    async fn open_document(&self, document: DocumentSnapshot) -> Result<(), LanguageError> {
        let julgar = julga(&document.path);
        let analisado = self.analisar(document.id, document.version, &document.text, julgar)?;
        self.documentos
            .lock()
            .map_err(|_| LanguageError::Provider("documentos de estilo travados".to_owned()))?
            .insert(document.id, analisado);
        Ok(())
    }

    async fn change_document(&self, change: DocumentChange) -> Result<(), LanguageError> {
        let (texto, julgar) = {
            let documentos = self
                .documentos
                .lock()
                .map_err(|_| LanguageError::Provider("documentos de estilo travados".to_owned()))?;
            let Some(atual) = documentos.get(&change.document_id) else {
                return Ok(());
            };
            (
                aplicar(&atual.texto, change.range, &change.text),
                atual.julgar,
            )
        };
        let analisado = self.analisar(change.document_id, change.version, &texto, julgar)?;
        self.documentos
            .lock()
            .map_err(|_| LanguageError::Provider("documentos de estilo travados".to_owned()))?
            .insert(change.document_id, analisado);
        Ok(())
    }

    async fn close_document(&self, document_id: DocumentId) -> Result<(), LanguageError> {
        self.documentos
            .lock()
            .map_err(|_| LanguageError::Provider("documentos de estilo travados".to_owned()))?
            .remove(&document_id);
        Ok(())
    }

    /// O que **este arquivo** declara, e nada mais.
    ///
    /// É o nível 1 da fase 5 da `23`: sem lista de propriedades, sem dado
    /// embarcado, sem tabela de versão. Num projeto com tema, é a completação
    /// que mais se usa — quem digita `$` quer as cores daquele projeto.
    ///
    /// # Por que o rótulo não traz o sigilo
    ///
    /// A interface substitui o **trecho de identificador** antes do cursor pelo
    /// rótulo escolhido, e `$` não é caractere de identificador para ela. Com o
    /// rótulo `$cor`, aceitar depois de digitar `$` escreveria `$$cor`.
    ///
    /// O sigilo vai no `detail`, que é o que a lista mostra ao lado — o nome
    /// continua legível, e a inserção cai certa.
    async fn completion(
        &self,
        request: CompletionRequest,
    ) -> Result<Vec<CompletionItem>, LanguageError> {
        let texto = self
            .documentos
            .lock()
            .map_err(|_| LanguageError::Provider("documentos de estilo travados".to_owned()))?
            .get(&request.document_id)
            .map(|documento| documento.texto.clone())
            .ok_or_else(|| LanguageError::Provider("documento não está aberto".to_owned()))?;

        let cursor = deslocamento(
            &texto,
            request.position.line as usize,
            request.position.column as usize,
        );
        let Some(sigilo) = sigilo_antes(&texto, cursor, &request.prefix) else {
            // Fora de um nome que este arquivo inventa, não há o que oferecer
            // **ainda**: nomes de propriedade são o nível 2, e uma lista vazia
            // diz isso melhor do que um erro.
            return Ok(Vec::new());
        };
        Ok(declaracoes(&texto, sigilo)
            .into_iter()
            .filter(|nome| nome.starts_with(&request.prefix))
            .map(|nome| CompletionItem {
                detail: Some(format!("{sigilo}{nome}")),
                label: nome,
                kind: CompletionKind::Variable,
            })
            .collect())
    }

    async fn diagnostics(&self, document_id: DocumentId) -> Result<Vec<Diagnostic>, LanguageError> {
        Ok(self
            .documentos
            .lock()
            .map_err(|_| LanguageError::Provider("documentos de estilo travados".to_owned()))?
            .get(&document_id)
            .map(|documento| documento.snapshot.diagnostics.clone())
            .unwrap_or_default())
    }

    async fn syntax(&self, document_id: DocumentId) -> Result<SyntaxSnapshot, LanguageError> {
        self.documentos
            .lock()
            .map_err(|_| LanguageError::Provider("documentos de estilo travados".to_owned()))?
            .get(&document_id)
            .map(|documento| documento.snapshot.clone())
            .ok_or_else(|| LanguageError::Provider("documento não está aberto".to_owned()))
    }

    async fn shutdown(&self) -> Result<(), LanguageError> {
        self.documentos
            .lock()
            .map_err(|_| LanguageError::Provider("documentos de estilo travados".to_owned()))?
            .clear();
        Ok(())
    }
}

fn julga(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|valor| valor.to_str())
        .is_some_and(|extensao| extensao.eq_ignore_ascii_case(JULGADA))
}

fn percorrer(
    node: Node<'_>,
    linhas: &LineIndex<'_>,
    julgar: bool,
    highlights: &mut Vec<SyntaxHighlight>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if julgar && (node.is_error() || node.is_missing()) {
        diagnostics.push(Diagnostic {
            range: intervalo(node, linhas),
            severity: DiagnosticSeverity::Error,
            message: "Trecho que não é CSS válido".to_owned(),
            source: Some("style".to_owned()),
        });
    }
    if let Some(kind) = classificar(node) {
        highlights.push(SyntaxHighlight {
            range: intervalo(node, linhas),
            kind,
        });
    }
    let mut cursor = node.walk();
    for filho in node.children(&mut cursor) {
        percorrer(filho, linhas, julgar, highlights, diagnostics);
    }
}

/// A estrutura de uma folha de estilo é a lista de regras.
///
/// O seletor é o nome, porque é por ele que se procura — ninguém navega uma
/// folha de estilo por declaração.
fn estrutura(node: Node<'_>, linhas: &LineIndex<'_>) -> Vec<OutlineItem> {
    let mut itens = Vec::new();
    let mut cursor = node.walk();
    for filho in node.named_children(&mut cursor) {
        if filho.kind() != "rule_set" {
            itens.extend(estrutura(filho, linhas));
            continue;
        }
        let Some(seletores) = filho.child(0) else {
            continue;
        };
        let Ok(nome) = seletores.utf8_text(linhas.source().as_bytes()) else {
            continue;
        };
        itens.push(OutlineItem {
            name: nome.split_whitespace().collect::<Vec<_>>().join(" "),
            kind: OutlineKind::Class,
            range: intervalo(filho, linhas),
            name_range: intervalo(seletores, linhas),
            children: filho
                .child_by_field_name("body")
                .map(|corpo| estrutura(corpo, linhas))
                .unwrap_or_default(),
        });
    }
    itens
}

fn classificar(node: Node<'_>) -> Option<SyntaxHighlightKind> {
    match node.kind() {
        "comment" => Some(SyntaxHighlightKind::Comment),
        "string_value" => Some(SyntaxHighlightKind::String),
        "integer_value" | "float_value" | "color_value" => Some(SyntaxHighlightKind::Number),
        "tag_name" | "class_name" | "id_name" | "property_name" => {
            Some(SyntaxHighlightKind::Field)
        }
        "at_keyword" | "important" | "from" | "to" => Some(SyntaxHighlightKind::Keyword),
        "plain_value" => Some(SyntaxHighlightKind::Variable),
        "function_name" => Some(SyntaxHighlightKind::Function),
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
            line: point.row as u32,
            column: linha.get(..byte).map_or(0, |antes| antes.chars().count()) as u32,
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

/// O sigilo que abre o nome sendo digitado, se houver um.
///
/// Olha o caractere imediatamente antes do trecho já digitado: com o cursor em
/// `$cor|`, o prefixo é `cor` e o que vem antes é `$`. Com o cursor logo depois
/// do `$`, o prefixo é vazio e o caractere anterior é o próprio `$`.
fn sigilo_antes(texto: &str, cursor: usize, prefixo: &str) -> Option<char> {
    let inicio = cursor.checked_sub(prefixo.len())?;
    texto
        .get(..inicio)?
        .chars()
        .next_back()
        .filter(|caractere| matches!(caractere, '$' | '%'))
}

/// Os nomes que este arquivo declara com um dado sigilo.
///
/// # Por que isto lê texto, e não a árvore
///
/// A gramática é a de CSS, e **`$cor-primaria` não é CSS**. Ela estilhaça a
/// declaração em nós de erro — verificado: `$cor-primaria: #333` vira
/// `ERROR "$c"`, `or`, `-`, `ERROR "primaria"`. Não há nó de onde tirar o nome.
///
/// Ler a linha resolve, não envelhece, e é honesto sobre o que está fazendo. É a
/// mesma razão pela qual o diagnóstico de SCSS já era silenciado: a árvore não
/// sabe deste arquivo, então quem sabe tem de ser outro.
fn declaracoes(texto: &str, sigilo: char) -> Vec<String> {
    let mut nomes = Vec::new();
    for linha in texto.lines() {
        let linha = linha.trim_start();
        let Some(resto) = linha.strip_prefix(sigilo) else {
            continue;
        };
        let nome = resto
            .chars()
            .take_while(|caractere| caractere.is_alphanumeric() || *caractere == '-' || *caractere == '_')
            .collect::<String>();
        if nome.is_empty() {
            continue;
        }
        // Só declaração: `$cor: #333` e `%base {`. Um **uso** — `color: $cor` —
        // não começa a linha, e por isso não chega aqui; oferecer usos como se
        // fossem declarações encheria a lista de repetição.
        let depois = resto.get(nome.len()..).unwrap_or_default().trim_start();
        let declara = match sigilo {
            '$' => depois.starts_with(':'),
            _ => depois.starts_with('{') || depois.starts_with(','),
        };
        if declara && !nomes.contains(&nome) {
            nomes.push(nome);
        }
    }
    nomes.sort();
    nomes
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
