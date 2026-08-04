#![doc = "Folhas de estilo: realce e estrutura, e nada além."]
#![doc = ""]
#![doc = "Completação de propriedade e resolução de seletor são um projeto"]
#![doc = "próprio, e a `23` os deixou de fora de propósito. O que existe aqui é o"]
#![doc = "mínimo que faz um arquivo de estilo deixar de ser texto cru."]

mod resolucao;

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Mutex,
};

use async_trait::async_trait;
use ide_domain::{
    CompletionItem, CompletionKind, CompletionRequest, Diagnostic, DiagnosticSeverity,
    DocumentChange, DocumentId, DocumentSnapshot, LanguageId, OutlineItem, OutlineKind, ProviderId,
    SyntaxHighlight, SyntaxHighlightKind, SyntaxSnapshot, TextPosition, TextRange,
};
use ide_language_api::{
    ActiveLanguage, LANGUAGE_API_VERSION, LanguageActivationContext, LanguageCapabilities,
    LanguageError, LanguageMetadata, LanguageProvider, ReadinessSignal,
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
        context: LanguageActivationContext,
    ) -> Result<Box<dyn ActiveLanguage>, LanguageError> {
        Ok(Box::new(ActiveStyle::new(context.workspace_root)?))
    }
}

struct Documento {
    texto: String,
    /// Se este arquivo pode ser julgado por esta gramática.
    ///
    /// Decidido na abertura, pela extensão, e guardado: a mudança não traz o
    /// caminho, e reanalisar precisa da mesma decisão.
    julgar: bool,
    /// Onde o arquivo está.
    ///
    /// Guardado porque `@import '../styles-config'` só quer dizer alguma coisa
    /// a partir da pasta de quem importa. A mudança não traz o caminho, e sem
    /// isto ele se perderia na primeira tecla.
    caminho: PathBuf,
    snapshot: SyntaxSnapshot,
}

struct ActiveStyle {
    language_id: LanguageId,
    /// A raiz do projeto, para o `node_modules` de um especificador nu.
    raiz: PathBuf,
    /// Quem importa quem, montado **em segundo plano** ao abrir o projeto.
    ///
    /// # Por que não na primeira pergunta, e por que não aqui na thread
    ///
    /// Montar sob demanda punia justamente quem estava digitando: medido no
    /// monorepo de referência, a primeira completação num `.scss` esperava
    /// **588 ms** pela varredura.
    ///
    /// Montar dentro de `activate` seria pior. A ativação acontece na abertura
    /// de um documento, e essa acontece **na thread da interface** — a mesma
    /// família de travamento que a `25` caçou cinco vezes.
    ///
    /// Então sobe uma linha de execução à parte, e quem pergunta antes de ela
    /// terminar recebe o que dá para responder sem o grafo. Degradar, e não
    /// esperar.
    grafo: std::sync::Arc<Mutex<Option<std::sync::Arc<resolucao::Grafo>>>>,
    /// Se o grafo já está de pé.
    ///
    /// É o mesmo `ReadinessSignal` que o analisador de TypeScript usa, e pelo
    /// mesmo motivo: quem espera precisa saber se ainda vale esperar.
    prontidao: ReadinessSignal,
    /// Se uma varredura já está correndo.
    ///
    /// **Por instância, e não global.** Como estático, dois projetos abertos em
    /// sequência disputariam a mesma marca, e o segundo poderia desistir de
    /// montar por causa do primeiro — ficando sem grafo para sempre.
    montando: std::sync::Arc<std::sync::atomic::AtomicBool>,
    /// O que cada arquivo põe em escopo, já calculado.
    ///
    /// # Por que este cache não é otimização prematura
    ///
    /// Sem ele, cada completação relia os arquivos: para cada ancestral, o
    /// próprio mais tudo o que ele alcança. Medido no projeto de referência,
    /// **464 ms** por lista — tarde demais para algo que aparece enquanto se
    /// digita. Com ele, o mesmo trabalho é feito uma vez por arquivo.
    ///
    /// A chave leva o sigilo porque `$` e `%` percorrem os mesmos arquivos e
    /// colhem coisas diferentes.
    escopos: Mutex<HashMap<(PathBuf, char), std::sync::Arc<Vec<String>>>>,
    parser: Mutex<Parser>,
    documentos: Mutex<HashMap<DocumentId, Documento>>,
}

impl ActiveStyle {
    fn new(raiz: PathBuf) -> Result<Self, LanguageError> {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_css::LANGUAGE.into())
            .map_err(|erro| LanguageError::Provider(erro.to_string()))?;
        let grafo = std::sync::Arc::new(Mutex::new(None));
        let prontidao = ReadinessSignal::new();
        let montando = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        montar_ao_fundo(&raiz, &grafo, &prontidao, &montando);
        Ok(Self {
            language_id: LanguageId(STYLE_LANGUAGE_ID.to_owned()),
            raiz,
            grafo,
            prontidao,
            montando,
            escopos: Mutex::new(HashMap::new()),
            parser: Mutex::new(parser),
            documentos: Mutex::new(HashMap::new()),
        })
    }

    /// Os nomes que um arquivo põe em escopo: os dele, mais os do que ele
    /// importa sem qualificar.
    ///
    /// Calculado uma vez por arquivo e por sigilo. O documento aberto **não**
    /// passa por aqui — o texto dele é o do editor, e não o do disco.
    fn escopo_de(&self, caminho: &Path, sigilo: char) -> std::sync::Arc<Vec<String>> {
        let chave = (caminho.to_path_buf(), sigilo);
        if let Ok(cache) = self.escopos.lock()
            && let Some(pronto) = cache.get(&chave)
        {
            return pronto.clone();
        }
        let mut nomes = std::collections::BTreeSet::new();
        if let Ok(conteudo) = std::fs::read_to_string(caminho) {
            nomes.extend(declaracoes(&conteudo, sigilo));
            for alcancado in resolucao::alcancados(caminho, &conteudo, &self.raiz) {
                if alcancado.espaco.is_some() {
                    continue;
                }
                if let Ok(vizinho) = std::fs::read_to_string(&alcancado.caminho) {
                    nomes.extend(declaracoes(&vizinho, sigilo));
                }
            }
        }
        let pronto = std::sync::Arc::new(nomes.into_iter().collect::<Vec<_>>());
        if let Ok(mut cache) = self.escopos.lock() {
            cache.insert(chave, pronto.clone());
        }
        pronto
    }

    /// O grafo, se ele já estiver de pé.
    ///
    /// `None` enquanto a varredura corre. Quem pergunta nesse intervalo recebe
    /// o que o próprio arquivo e o que ele importa oferecem — menos, e não
    /// errado.
    fn grafo(&self) -> Option<std::sync::Arc<resolucao::Grafo>> {
        self.grafo.lock().ok()?.clone()
    }

    fn analisar(
        &self,
        document_id: DocumentId,
        version: u64,
        texto: &str,
        julgar: bool,
        caminho: PathBuf,
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
            caminho,
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

    fn readiness(&self) -> Option<ReadinessSignal> {
        Some(self.prontidao.clone())
    }

    /// Um `.scss` gravado pode ter mudado o que importa, e o grafo envelhece
    /// com isso.
    ///
    /// Remontar inteiro é o caminho simples e correto: a alternativa seria
    /// comparar as importações de antes com as de agora, e o "antes" não existe
    /// aqui. Corre ao fundo, como na abertura, e o cache de escopos vai junto —
    /// ele foi calculado sobre o grafo velho.
    async fn file_changed(&self, path: &Path) -> Result<(), LanguageError> {
        if path.extension().and_then(|valor| valor.to_str()) != Some("scss") {
            return Ok(());
        }
        if let Ok(mut escopos) = self.escopos.lock() {
            escopos.clear();
        }
        montar_ao_fundo(&self.raiz, &self.grafo, &self.prontidao, &self.montando);
        Ok(())
    }

    async fn open_document(&self, document: DocumentSnapshot) -> Result<(), LanguageError> {
        let julgar = julga(&document.path);
        let analisado = self.analisar(
            document.id,
            document.version,
            &document.text,
            julgar,
            document.path.clone(),
        )?;
        self.documentos
            .lock()
            .map_err(|_| LanguageError::Provider("documentos de estilo travados".to_owned()))?
            .insert(document.id, analisado);
        Ok(())
    }

    async fn change_document(&self, change: DocumentChange) -> Result<(), LanguageError> {
        let (texto, julgar, caminho) = {
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
                atual.caminho.clone(),
            )
        };
        let analisado =
            self.analisar(change.document_id, change.version, &texto, julgar, caminho)?;
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
        let (texto, caminho) = self
            .documentos
            .lock()
            .map_err(|_| LanguageError::Provider("documentos de estilo travados".to_owned()))?
            .get(&request.document_id)
            .map(|documento| (documento.texto.clone(), documento.caminho.clone()))
            .ok_or_else(|| LanguageError::Provider("documento não está aberto".to_owned()))?;

        let cursor = deslocamento(
            &texto,
            request.position.line as usize,
            request.position.column as usize,
        );
        let inicio = cursor.saturating_sub(request.prefix.len());
        let Some(sigilo) = sigilo_antes(&texto, inicio) else {
            // Sem sigilo, o que cabe é o nome de uma propriedade — e só onde uma
            // propriedade cabe. Ver [`cabe_uma_propriedade`].
            return Ok(if cabe_uma_propriedade(&texto, inicio) {
                propriedades()
                    .filter(|nome| nome.starts_with(&request.prefix))
                    .map(|nome| CompletionItem {
                        label: nome.to_owned(),
                        detail: None,
                        kind: CompletionKind::Field,
                    })
                    .collect()
            } else {
                Vec::new()
            });
        };
        // `v.$cor` pergunta pelo módulo `v`; `$cor` pergunta pelo que entrou sem
        // qualificação. Oferecer um no lugar do outro daria nomes que o arquivo
        // não enxerga.
        let espaco = espaco_antes(&texto, inicio.saturating_sub(1));

        // Conjunto ordenado, e não vetor. Medido: com dedução por varredura
        // linear, juntar o escopo de até 128 ancestrais — mil e poucos nomes
        // cada — custava **502 ms** por lista. O trabalho é o mesmo; o que
        // mudou foi a estrutura em que ele é acumulado.
        let mut nomes = std::collections::BTreeSet::new();
        if espaco.is_none() {
            nomes.extend(declaracoes(&texto, sigilo));
        }
        for alcancado in resolucao::alcancados(&caminho, &texto, &self.raiz) {
            if alcancado.espaco != espaco {
                continue;
            }
            if let Ok(conteudo) = std::fs::read_to_string(&alcancado.caminho) {
                nomes.extend(declaracoes(&conteudo, sigilo));
            }
        }
        // O escopo que vem **de cima**: um parcial usa o que quem o importou
        // trouxe, e ele próprio não importa nada. Só sem qualificação — é o
        // `@import` que tem escopo global, e um `@use` com apelido não vaza
        // para quem foi importado.
        // O escopo de cada ancestral vem **emprestado**, e não copiado. Medido:
        // clonar os nomes de cada um para juntá-los custava 206 ms por lista num
        // monorepo com mil e poucas variáveis. Aqui só sobrevive o que o prefixo
        // deixa passar, e só esses viram texto novo.
        let mut de_cima = Vec::new();
        if espaco.is_none()
            && let Some(grafo) = self.grafo()
        {
            for ancestral in grafo.ancestrais(&caminho) {
                de_cima.push(self.escopo_de(&ancestral, sigilo));
            }
        }
        let mut todos = std::collections::BTreeSet::new();
        todos.extend(nomes.iter().map(String::as_str));
        for escopo in &de_cima {
            todos.extend(escopo.iter().map(String::as_str));
        }
        Ok(todos
            .into_iter()
            .filter(|nome| nome.starts_with(&request.prefix))
            .map(|nome| CompletionItem {
                detail: Some(format!("{sigilo}{nome}")),
                label: nome.to_owned(),
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

    async fn syntax(
        &self,
        document_id: DocumentId,
        _visible: Option<ide_domain::TextRange>,
    ) -> Result<SyntaxSnapshot, LanguageError> {
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
fn sigilo_antes(texto: &str, inicio: usize) -> Option<char> {
    texto
        .get(..inicio)?
        .chars()
        .next_back()
        .filter(|caractere| matches!(caractere, '$' | '%'))
}

/// Sobe a varredura numa linha de execução à parte.
///
/// Uma por vez: uma sequência de gravações não pode empilhar varreduras do
/// projeto inteiro. Quem chegar enquanto uma corre não faz nada, e a que está
/// correndo já vai ler o disco como ele está agora.
fn montar_ao_fundo(
    raiz: &Path,
    destino: &std::sync::Arc<Mutex<Option<std::sync::Arc<resolucao::Grafo>>>>,
    prontidao: &ReadinessSignal,
    montando: &std::sync::Arc<std::sync::atomic::AtomicBool>,
) {
    use std::sync::atomic::Ordering;
    if montando.swap(true, Ordering::AcqRel) {
        return;
    }
    let raiz = raiz.to_path_buf();
    let destino = destino.clone();
    let prontidao = prontidao.clone();
    let marca = montando.clone();
    let devolver = montando.clone();
    // Se a linha não pode subir, o grafo fica sem montar: a completação
    // responde menos, e a IDE não deixa de responder.
    let subiu = std::thread::Builder::new()
        .name("estilo-grafo".to_owned())
        .spawn(move || {
            let montado = std::sync::Arc::new(resolucao::Grafo::construir(&raiz));
            if let Ok(mut guardado) = destino.lock() {
                *guardado = Some(montado);
            }
            prontidao.mark_ready();
            marca.store(false, Ordering::Release);
        });
    if subiu.is_err() {
        // Sem a linha, não há varredura e não haverá prontidão: a marca precisa
        // voltar, senão nenhuma tentativa futura passa deste ponto.
        devolver.store(false, Ordering::Release);
    }
}

/// As propriedades que a completação conhece.
///
/// A lista vem do `mdn-data`, sob CC0-1.0, e viaja dentro do executável — são
/// 8,1 KB. A procedência e o critério de corte estão em
/// `dados/PROVENIENCIA.md`.
///
/// **É dado, e não lógica.** Propriedades novas entram por ano; atualizar é
/// trocar um arquivo, e não mexer em código — que é a diferença entre isto e
/// escrever uma gramática de template, que a `24` recusou.
fn propriedades() -> impl Iterator<Item = &'static str> {
    include_str!("../dados/propriedades-css.txt")
        .lines()
        .filter(|linha| !linha.is_empty())
}

/// Uma propriedade cabe aqui?
///
/// # As duas condições, e por que as duas
///
/// **Dentro de um bloco.** No topo de um arquivo só cabem seletor e regra `@`;
/// oferecer `display` ali seria oferecer o que não compila. A conta é de chaves
/// abertas menos fechadas, e o aninhamento do SCSS entra de graça nela.
///
/// **No começo de uma declaração.** O caractere anterior, fora espaços, precisa
/// ser `{`, `;` ou `}`. É o que separa `color: |` — onde cabe um **valor**, e
/// valor é o nível 3 — de `  |`, onde cabe o nome.
///
/// A conta ignora chave dentro de texto e de comentário. É aproximação
/// deliberada: o preço de errar é uma lista oferecida onde não devia, e não uma
/// resposta errada sobre o código.
fn cabe_uma_propriedade(texto: &str, inicio: usize) -> bool {
    let Some(ate) = texto.get(..inicio) else {
        return false;
    };
    let profundidade = ate.chars().fold(0i32, |conta, caractere| match caractere {
        '{' => conta + 1,
        '}' => conta - 1,
        _ => conta,
    });
    if profundidade <= 0 {
        return false;
    }
    ate.chars()
        .rev()
        .find(|caractere| !caractere.is_whitespace())
        .is_some_and(|anterior| matches!(anterior, '{' | ';' | '}'))
}

/// O módulo que qualifica o nome, quando ele vem escrito `v.$cor`.
///
/// Devolve `None` para `$cor` escrito direto — que é o que entra por `@import`
/// e por `@use ... as *`.
fn espaco_antes(texto: &str, antes_do_sigilo: usize) -> Option<String> {
    let ate = texto.get(..antes_do_sigilo)?;
    let sem_ponto = ate.strip_suffix('.')?;
    let nome = sem_ponto
        .chars()
        .rev()
        .take_while(|caractere| caractere.is_alphanumeric() || *caractere == '-' || *caractere == '_')
        .collect::<String>()
        .chars()
        .rev()
        .collect::<String>();
    (!nome.is_empty()).then_some(nome)
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
