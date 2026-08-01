//! Varredura limitada das entradas que alimentam o índice do workspace.

use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
};

use ide_domain::{
    DocumentId, DocumentSnapshot, Location, SemanticSymbol, SymbolKind, TextRange, TypeDescriptor,
};
use tree_sitter::Parser;

use crate::{language::analyze_semantics, symbols::simple_class_name};

mod file;

pub(crate) fn collect_workspace_paths(root: &Path, output: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let ignored = path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| matches!(name, ".git" | "target" | "node_modules" | ".gradle"));
            if !ignored {
                collect_workspace_paths(&path, output);
            }
        } else {
            output.push(path);
        }
    }
}

/// Tipo compilado que o workspace alcança: `.class` do próprio projeto ou
/// classe dentro de um jar de dependência.
#[derive(Clone, Debug)]
pub(super) struct ExternalClass {
    /// Nome simples, como aparece escrito no código.
    pub(super) simple: String,
    /// Nome binário, que localiza a classe dentro do jar.
    pub(super) binary: String,
    /// Arquivo `.class` ou o jar que a contém.
    pub(super) origin: PathBuf,
}

/// Uma ocorrência de um nome, com o arquivo guardado por número.
///
/// Um monorepo tem milhões de ocorrências e dezenas de milhares de arquivos.
/// Repetir o caminho em cada uma delas era, medido, a maior parte da memória do
/// índice: 2,7 milhões de cópias de um `PathBuf` longo.
#[derive(Clone, Copy)]
pub(super) struct Occurrence {
    file: u32,
    range: TextRange,
}

/// Uma declaração do projeto, com o arquivo guardado por número.
///
/// É um `SemanticSymbol` sem o `Location`: muda só onde o caminho mora.
/// Trezentas mil declarações carregando cada uma a cópia do seu caminho são
/// dezenas de megabytes gastos em repetir trinta mil nomes de arquivo.
pub(super) struct IndexedSymbol {
    pub(super) name: String,
    pub(super) kind: SymbolKind,
    pub(super) range: TextRange,
    pub(super) type_descriptor: Option<TypeDescriptor>,
    pub(super) scope_depth: u32,
    file: u32,
}

/// O que uma varredura produz, e o que uma reindexação altera.
///
/// Continua sendo a forma de trabalho: construir exige escrever, e escrever
/// exige estrutura. O que mudou na fase 2 é que ela deixa de ser a **única**
/// forma de responder — um índice vindo do disco responde dos bytes.
#[derive(Default)]
pub(super) struct Dados {
    symbols: Vec<IndexedSymbol>,
    references: HashMap<String, Vec<Occurrence>>,
    /// Os arquivos citados pelas ocorrências e pelas declarações, uma vez cada.
    files: Vec<PathBuf>,
    file_ids: HashMap<PathBuf, u32>,
    pub(super) external_classes: Vec<ExternalClass>,
    /// Arquivo que declara cada tipo do projeto, pelo nome simples.
    ///
    /// Guardar o caminho, e não os membros, é o que permite responder pela
    /// classe **como ela está agora**: o fonte é a verdade, e o `.class` do
    /// último build pode ser mais velho que o arquivo aberto ao lado.
    pub(super) declarations: HashMap<String, PathBuf>,
}

/// O índice do projeto, venha ele de onde vier.
///
/// Duas origens convivem: o arquivo carregado, que responde a quase tudo, e os
/// dados em memória, que guardam o que foi construído agora ou reindexado desde
/// o carregamento. Um arquivo refeito em memória some do carregado — é isso que
/// `substituidos` marca, e é o que impede a IDE de responder pelo texto de antes.
#[derive(Default)]
pub(super) struct WorkspaceIndex {
    carregado: Option<file::Carregado>,
    memoria: Dados,
    /// Números de arquivo do carregado que a memória refez.
    substituidos: HashSet<u32>,
}

/// Uma declaração, venha da memória ou do arquivo.
#[derive(Clone, Copy)]
pub(super) enum Simbolo<'a> {
    Memoria(&'a IndexedSymbol),
    Disco(file::SimboloNoDisco<'a>),
}

impl Simbolo<'_> {
    pub(super) fn name(&self) -> &str {
        match self {
            Self::Memoria(simbolo) => &simbolo.name,
            Self::Disco(simbolo) => simbolo.name(),
        }
    }

    pub(super) fn kind(&self) -> SymbolKind {
        match self {
            Self::Memoria(simbolo) => simbolo.kind,
            Self::Disco(simbolo) => simbolo.kind(),
        }
    }

    /// Só o nome do tipo. A completação passa por todas as declarações a cada
    /// tecla, e montar o descritor inteiro ali seria pagar por nada.
    pub(super) fn type_name(&self) -> Option<&str> {
        match self {
            Self::Memoria(simbolo) => simbolo
                .type_descriptor
                .as_ref()
                .map(|descritor| descritor.name.as_str()),
            Self::Disco(simbolo) => simbolo.type_name(),
        }
    }

    fn type_descriptor(&self) -> Option<TypeDescriptor> {
        match self {
            Self::Memoria(simbolo) => simbolo.type_descriptor.clone(),
            Self::Disco(simbolo) => simbolo.type_descriptor(),
        }
    }

    fn range(&self) -> TextRange {
        match self {
            Self::Memoria(simbolo) => simbolo.range,
            Self::Disco(simbolo) => simbolo.range(),
        }
    }

    fn scope_depth(&self) -> u32 {
        match self {
            Self::Memoria(simbolo) => simbolo.scope_depth,
            Self::Disco(simbolo) => simbolo.scope_depth(),
        }
    }
}

/// Uma classe externa, venha da memória ou do arquivo.
#[derive(Clone, Copy)]
pub(super) enum Externa<'a> {
    Memoria(&'a ExternalClass),
    Disco(file::ExternaNoDisco<'a>),
}

impl Externa<'_> {
    pub(super) fn simple(&self) -> &str {
        match self {
            Self::Memoria(classe) => &classe.simple,
            Self::Disco(classe) => classe.simple(),
        }
    }

    pub(super) fn binary(&self) -> &str {
        match self {
            Self::Memoria(classe) => &classe.binary,
            Self::Disco(classe) => classe.binary(),
        }
    }

    /// Os metadados da classe, lidos do jar na hora.
    pub(super) fn descriptor(&self) -> Option<java_classfile::ClassDescriptor> {
        match self {
            Self::Memoria(classe) => classe.descriptor(),
            Self::Disco(classe) => ExternalClass {
                simple: String::new(),
                binary: classe.binary().to_owned(),
                origin: PathBuf::from(classe.origin()),
            }
            .descriptor(),
        }
    }
}

/// Se o símbolo pode ser nomeado de fora do arquivo que o declara.
///
/// É o que separa o índice do projeto da semântica do documento aberto: quem
/// pergunta pelo arquivo em que está sempre recebe os símbolos dele, com locais
/// e parâmetros incluídos. O índice responde pelo **resto** do projeto, e ali
/// isso não existe.
fn declaravel(symbol: &SemanticSymbol) -> bool {
    !matches!(
        symbol.kind,
        SymbolKind::Parameter | SymbolKind::LocalVariable
    )
}

impl WorkspaceIndex {
    /// Um índice recém-construído, que ainda só existe em memória.
    pub(super) fn construido(memoria: Dados) -> Self {
        Self {
            carregado: None,
            memoria,
            substituidos: HashSet::new(),
        }
    }

    /// Um índice vindo do arquivo deste projeto, se houver um utilizável.
    ///
    /// Não decide se ele **ainda vale** — quem carrega é quem confere, e a
    /// conferência é da fase 4. Aqui só se responde se o arquivo existe e está
    /// íntegro.
    pub(super) fn carregar(root: &Path, toolchain: Option<&Path>) -> Option<Self> {
        let carregado = file::Carregado::open(&file::caminho_do_indice(root, toolchain)?)?;
        Some(Self {
            carregado: Some(carregado),
            memoria: Dados::default(),
            substituidos: HashSet::new(),
        })
    }

    /// Carrega de um caminho escolhido. Ver `save_to`.
    #[cfg(test)]
    pub(super) fn carregar_de(path: &Path) -> Option<Self> {
        Some(Self {
            carregado: Some(file::Carregado::open(path)?),
            memoria: Dados::default(),
            substituidos: HashSet::new(),
        })
    }

    /// Grava o índice deste projeto no disco.
    ///
    /// Só faz sentido para um índice construído: um que veio do arquivo já está
    /// gravado, e regravá-lo do delta perderia tudo o que não foi reindexado.
    pub(super) fn save(&self, root: &Path, toolchain: Option<&Path>) -> bool {
        self.carregado.is_none()
            && file::caminho_do_indice(root, toolchain)
                .is_some_and(|caminho| self.save_to(&caminho))
    }

    /// Se este índice veio do arquivo.
    #[cfg(test)]
    pub(super) const fn veio_do_arquivo(&self) -> bool {
        self.carregado.is_some()
    }

    /// Grava num caminho escolhido. Serve à medição, que não usa o cache.
    pub(super) fn save_to(&self, path: &Path) -> bool {
        file::write(&self.memoria, path).is_ok()
    }

    /// Os fontes que mudaram desde a gravação do índice.
    ///
    /// Novo, alterado ou apagado — os três entram na lista, e a lista vazia quer
    /// dizer que o arquivo ainda descreve o projeto. É a fase 4 da `20`: antes
    /// qualquer diferença reprovava o índice inteiro, e agora ela custa
    /// exatamente os arquivos que ela toca.
    ///
    /// Não cobre os jars: uma dependência trocada sem mexer em fonte nenhum
    /// passa despercebida. Limite conhecido, herdado da fase 2.
    pub(super) fn diferenca(&self, root: &Path, source_roots: &[PathBuf]) -> Vec<PathBuf> {
        let Some(carregado) = &self.carregado else {
            return Vec::new();
        };
        let mut gravados: HashMap<PathBuf, (u64, u64)> = (0..carregado.arquivos())
            .filter_map(|indice| carregado.arquivo_gravado(indice))
            .map(|(caminho, quando, tamanho)| (PathBuf::from(caminho), (quando, tamanho)))
            .collect();
        let mut mudaram = Vec::new();
        let mut caminhos = Vec::new();
        collect_workspace_paths(root, &mut caminhos);
        for caminho in caminhos {
            let fonte_java = caminho
                .extension()
                .and_then(|extensao| extensao.to_str())
                .is_some_and(|extensao| extensao.eq_ignore_ascii_case("java"))
                && (source_roots.is_empty()
                    || source_roots.iter().any(|raiz| caminho.starts_with(raiz)));
            if !fonte_java {
                continue;
            }
            let Some((quando, tamanho)) = gravados.remove(&caminho) else {
                // O índice não conhece este fonte: ele nasceu depois.
                mudaram.push(caminho);
                continue;
            };
            let Ok(dados) = fs::metadata(&caminho) else {
                mudaram.push(caminho);
                continue;
            };
            let agora = dados
                .modified()
                .ok()
                .and_then(|instante| instante.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0, |desde| desde.as_secs());
            if dados.len() != tamanho || agora != quando {
                mudaram.push(caminho);
            }
        }
        // O que sobrou foi apagado desde a gravação.
        mudaram.extend(gravados.into_keys());
        mudaram
    }

    /// Relê os fontes que mudaram, e só eles.
    ///
    /// Cada um passa por `reindex_file`, que já sabe tirar do carregado o que
    /// aquele arquivo dizia e pôr no lugar o que ele diz agora — ou só tirar, se
    /// ele sumiu do disco.
    pub(super) fn reconciliar(&mut self, mudaram: &[PathBuf], parser: &mut Parser) {
        for caminho in mudaram {
            self.reindex_file(caminho, parser);
        }
    }

    /// O índice inteiro na forma de trabalho, para poder ser regravado.
    ///
    /// Junta o que sobrou do carregado com o que a memória refez. Custa montar
    /// as estruturas de novo — é o preço de devolver ao disco um índice que
    /// nasceu de duas origens, e por isso só acontece quando houve diferença.
    fn juntar(&self) -> Dados {
        let mut dados = Dados::default();
        for simbolo in self.symbols() {
            let Some(local) = self.location_of(simbolo) else {
                continue;
            };
            let file = dados.file_id(&local.path);
            dados.symbols.push(IndexedSymbol {
                name: simbolo.name().to_owned(),
                kind: simbolo.kind(),
                range: local.range,
                type_descriptor: simbolo.type_descriptor(),
                scope_depth: simbolo.scope_depth(),
                file,
            });
        }
        if let Some(carregado) = &self.carregado {
            // Uma passada pelos nomes, e não uma por arquivo: percorrer os
            // nomes dentro do laço dos arquivos seria 26 mil vezes 78 mil.
            for nome in carregado.nomes_gravados() {
                let convertidas: Vec<(PathBuf, TextRange)> = carregado
                    .ocorrencias_de(nome)
                    .filter(|ocorrencia| !self.substituidos.contains(&ocorrencia.file))
                    .filter_map(|ocorrencia| {
                        let caminho = PathBuf::from(carregado.arquivo(ocorrencia.file)?);
                        Some((caminho, ocorrencia.range))
                    })
                    .collect();
                if convertidas.is_empty() {
                    continue;
                }
                let entrada = dados.references.entry(nome.to_owned()).or_default();
                for (caminho, range) in convertidas {
                    let file = match dados.file_ids.get(&caminho) {
                        Some(id) => *id,
                        None => {
                            let id = u32::try_from(dados.files.len()).unwrap_or(u32::MAX);
                            dados.files.push(caminho.clone());
                            dados.file_ids.insert(caminho, id);
                            id
                        }
                    };
                    entrada.push(Occurrence { file, range });
                }
            }
            for classe in carregado.externas() {
                dados.external_classes.push(ExternalClass {
                    simple: classe.simple().to_owned(),
                    binary: classe.binary().to_owned(),
                    origin: PathBuf::from(classe.origin()),
                });
            }
        }
        for (nome, ocorrencias) in &self.memoria.references {
            let convertidas: Vec<Occurrence> = ocorrencias
                .iter()
                .filter_map(|ocorrencia| {
                    let caminho = self.memoria.file_path(ocorrencia.file)?.to_path_buf();
                    Some((caminho, ocorrencia.range))
                })
                .map(|(caminho, range)| Occurrence {
                    file: dados.file_id(&caminho),
                    range,
                })
                .collect();
            dados
                .references
                .entry(nome.clone())
                .or_default()
                .extend(convertidas);
        }
        dados
            .external_classes
            .extend(self.memoria.external_classes.iter().cloned());
        for simbolo in self.symbols() {
            if matches!(
                simbolo.kind(),
                SymbolKind::Class | SymbolKind::Interface | SymbolKind::Record | SymbolKind::Enum
            ) && let Some(local) = self.location_of(simbolo)
            {
                dados
                    .declarations
                    .entry(simbolo.name().to_owned())
                    .or_insert(local.path);
            }
        }
        dados
    }

    /// Devolve ao disco o índice reconciliado.
    ///
    /// Sem isto a próxima abertura recalcularia a mesma diferença, e ela só
    /// cresceria: o arquivo continuaria descrevendo o projeto de duas semanas
    /// atrás.
    pub(super) fn regravar(&self, root: &Path, toolchain: Option<&Path>) -> bool {
        file::caminho_do_indice(root, toolchain)
            .is_some_and(|caminho| file::write(&self.juntar(), &caminho).is_ok())
    }

    /// Regrava num caminho escolhido, juntando as duas origens. Serve à medição.
    #[cfg(test)]
    pub(super) fn save_juntando(&self, path: &Path) -> bool {
        file::write(&self.juntar(), path).is_ok()
    }

    /// Quantas declarações vieram da memória — serve aos testes da fase 4.
    #[cfg(test)]
    pub(super) fn memory_symbol_count(&self) -> usize {
        self.memoria.symbol_count()
    }

    /// Onde um nome aparece, no projeto inteiro.
    ///
    /// No carregado a busca é binária sobre a tabela ordenada de nomes: Ctrl+
    /// clique não toca o resto do arquivo.
    pub(super) fn references_to<'a>(&'a self, name: &'a str) -> impl Iterator<Item = Location> {
        let do_disco = self.carregado.iter().flat_map(move |carregado| {
            carregado
                .ocorrencias_de(name)
                .filter(|ocorrencia| !self.substituidos.contains(&ocorrencia.file))
                .filter_map(|ocorrencia| {
                    Some(Location {
                        path: PathBuf::from(carregado.arquivo(ocorrencia.file)?),
                        range: ocorrencia.range,
                    })
                })
        });
        do_disco.chain(self.memoria.references_to(name))
    }

    /// As declarações do projeto, das duas origens.
    pub(super) fn symbols(&self) -> impl Iterator<Item = Simbolo<'_>> {
        let do_disco = self.carregado.iter().flat_map(|carregado| {
            carregado
                .simbolos()
                .filter(|simbolo| !self.substituidos.contains(&simbolo.file()))
                .map(Simbolo::Disco)
        });
        do_disco.chain(self.memoria.symbols().map(Simbolo::Memoria))
    }

    /// As declarações cujo nome começa com o prefixo, sem distinguir maiúsculas.
    ///
    /// No arquivo isso é uma faixa contígua, achada por busca binária: é a fase
    /// 3 da `20`, e é o que faz digitar uma letra não percorrer o índice
    /// inteiro. Na memória — o delta reindexado desde o carregamento — a
    /// varredura continua, e continua barata porque o delta é pequeno.
    ///
    /// Quem chama filtra o que ainda precisa: a completação quer prefixo
    /// **com** distinção de maiúsculas, que é um subconjunto desta faixa.
    pub(super) fn symbols_with_prefix<'a>(
        &'a self,
        prefix: &'a str,
    ) -> impl Iterator<Item = Simbolo<'a>> {
        let minusculo = prefix.to_ascii_lowercase();
        let do_disco = self.carregado.iter().flat_map(move |carregado| {
            carregado
                .simbolos_com_prefixo(prefix)
                .filter(|simbolo| !self.substituidos.contains(&simbolo.file()))
                .map(Simbolo::Disco)
        });
        do_disco.chain(
            self.memoria
                .symbols()
                .filter(move |simbolo| {
                    simbolo
                        .name
                        .to_ascii_lowercase()
                        .starts_with(minusculo.as_str())
                })
                .map(Simbolo::Memoria),
        )
    }

    /// Onde uma declaração está. É o que custa, então é pedido à parte.
    pub(super) fn location_of(&self, symbol: Simbolo<'_>) -> Option<Location> {
        let path = match &symbol {
            Simbolo::Memoria(simbolo) => self.memoria.file_path(simbolo.file)?.to_path_buf(),
            Simbolo::Disco(simbolo) => {
                PathBuf::from(self.carregado.as_ref()?.arquivo(simbolo.file())?)
            }
        };
        Some(Location {
            path,
            range: symbol.range(),
        })
    }

    /// A declaração na forma que o resto da IDE fala.
    pub(super) fn materialize(&self, symbol: Simbolo<'_>) -> Option<SemanticSymbol> {
        Some(SemanticSymbol {
            name: symbol.name().to_owned(),
            kind: symbol.kind(),
            location: self.location_of(symbol)?,
            type_descriptor: symbol.type_descriptor(),
            scope_depth: symbol.scope_depth(),
        })
    }

    /// O arquivo que declara um tipo, pelo nome simples.
    ///
    /// A memória vem primeiro: um arquivo reindexado agora sabe mais que o
    /// arquivo gravado antes dele.
    pub(super) fn declaring_file(&self, type_name: &str) -> Option<PathBuf> {
        if let Some(caminho) = self.memoria.declarations.get(type_name) {
            return Some(caminho.clone());
        }
        Some(PathBuf::from(
            self.carregado.as_ref()?.declaracao(type_name)?,
        ))
    }

    /// As classes do JDK e dos jars, das duas origens.
    pub(super) fn external_classes(&self) -> impl Iterator<Item = Externa<'_>> {
        self.carregado
            .iter()
            .flat_map(|carregado| carregado.externas().map(Externa::Disco))
            .chain(self.memoria.external_classes.iter().map(Externa::Memoria))
    }

    /// Reindexa **um** arquivo, tirando antes o que ele havia declarado.
    ///
    /// O que veio do disco para aquele arquivo passa a ser ignorado: quem
    /// responde por ele é a memória, que acabou de lê-lo.
    pub(super) fn reindex_file(&mut self, path: &Path, parser: &mut Parser) {
        if let Some(carregado) = &self.carregado {
            let alvo = path.to_string_lossy();
            for indice in 0..carregado.arquivos() {
                if carregado.arquivo(u32::try_from(indice).unwrap_or(u32::MAX))
                    == Some(alvo.as_ref())
                {
                    self.substituidos
                        .insert(u32::try_from(indice).unwrap_or(u32::MAX));
                }
            }
        }
        self.memoria.reindex_file(path, parser);
    }

    /// Quantas declarações — serve à medição.
    #[cfg(test)]
    pub(super) fn symbol_count(&self) -> usize {
        self.carregado
            .as_ref()
            .map_or(0, file::Carregado::simbolos_conta)
            + self.memoria.symbol_count()
    }

    /// Quantos nomes e quantas ocorrências — serve à medição.
    #[cfg(test)]
    pub(super) fn reference_counts(&self) -> (usize, usize) {
        self.memoria.reference_counts()
    }

    /// Quantos tipos declarados — serve à medição.
    #[cfg(test)]
    pub(super) fn declaration_count(&self) -> usize {
        self.carregado
            .as_ref()
            .map_or(0, file::Carregado::declaracoes_conta)
            + self.memoria.declarations.len()
    }
}

impl Dados {
    /// Onde um nome aparece, no projeto inteiro.
    ///
    /// Devolve `Location` porque é o que o resto da IDE fala; o formato
    /// compacto existe só aqui dentro.
    pub(super) fn references_to(&self, name: &str) -> impl Iterator<Item = Location> + '_ {
        self.references
            .get(name)
            .into_iter()
            .flatten()
            .filter_map(|ocorrencia| {
                Some(Location {
                    path: self.files.get(ocorrencia.file as usize)?.clone(),
                    range: ocorrencia.range,
                })
            })
    }

    /// As declarações do projeto, na forma compacta.
    ///
    /// Quem só precisa do nome, da espécie ou do tipo — a completação, por
    /// exemplo, que passa por todas a cada tecla — trabalha aqui e não paga
    /// caminho nenhum.
    pub(super) fn symbols(&self) -> impl Iterator<Item = &IndexedSymbol> {
        self.symbols.iter()
    }

    /// Quantas declarações — serve à medição da fase 3.
    #[cfg(test)]
    pub(super) fn symbol_count(&self) -> usize {
        self.symbols.len()
    }

    /// Quantos nomes e quantas ocorrências — serve à medição da fase 3.
    #[cfg(test)]
    pub(super) fn reference_counts(&self) -> (usize, usize) {
        (
            self.references.len(),
            self.references.values().map(Vec::len).sum(),
        )
    }

    /// O caminho de um arquivo, pelo número.
    fn file_path(&self, id: u32) -> Option<&Path> {
        self.files.get(id as usize).map(PathBuf::as_path)
    }

    /// O número deste arquivo, criando-o se for a primeira vez.
    fn file_id(&mut self, path: &Path) -> u32 {
        if let Some(id) = self.file_ids.get(path) {
            return *id;
        }
        let id = u32::try_from(self.files.len()).unwrap_or(u32::MAX);
        self.files.push(path.to_path_buf());
        self.file_ids.insert(path.to_path_buf(), id);
        id
    }

    /// Lê um fonte e acrescenta o que ele declara.
    fn index_source(&mut self, path: &Path, parser: &mut Parser) {
        let Ok(text) = fs::read_to_string(path) else {
            return;
        };
        let Some(tree) = parser.parse(&text, None) else {
            return;
        };
        let snapshot = DocumentSnapshot {
            id: DocumentId(0),
            path: path.to_path_buf(),
            version: 0,
            text,
        };
        let (semantic, references) = analyze_semantics(&snapshot, &tree);
        for symbol in &semantic.symbols {
            if matches!(
                symbol.kind,
                SymbolKind::Class | SymbolKind::Interface | SymbolKind::Record | SymbolKind::Enum
            ) {
                self.declarations
                    .entry(symbol.name.clone())
                    .or_insert_with(|| path.to_path_buf());
            }
        }
        // Tudo o que sai deste passo é deste arquivo: o número sai uma vez, e
        // não uma por declaração ou ocorrência.
        let file = self.file_id(path);
        // Só o que outro arquivo pode nomear. Um parâmetro ou uma variável
        // local de outro fonte não é destino de navegação nem sugestão de
        // completação — e guardá-los, com o teto fora, era memória paga por
        // resposta errada.
        self.symbols.extend(
            semantic
                .symbols
                .into_iter()
                .filter(declaravel)
                .map(|symbol| IndexedSymbol {
                    name: symbol.name,
                    kind: symbol.kind,
                    range: symbol.location.range,
                    type_descriptor: symbol.type_descriptor,
                    scope_depth: symbol.scope_depth,
                    file,
                }),
        );
        for (name, locations) in references {
            self.references
                .entry(name)
                .or_default()
                .extend(locations.into_iter().map(|local| Occurrence {
                    file,
                    range: local.range,
                }));
        }
    }

    /// Reindexa **um** arquivo, tirando antes o que ele havia declarado.
    ///
    /// Salvar deixa de esperar a próxima ativação. Com 26 mil fontes, refazer o
    /// índice inteiro a cada gravação seria trabalho de minutos.
    pub(super) fn reindex_file(&mut self, path: &Path, parser: &mut Parser) {
        // O número do arquivo continua válido: ele é reusado ao reindexar, e
        // some só quando o projeto inteiro é relido.
        if let Some(file) = self.file_ids.get(path).copied() {
            self.symbols.retain(|symbol| symbol.file != file);
            for ocorrencias in self.references.values_mut() {
                ocorrencias.retain(|ocorrencia| ocorrencia.file != file);
            }
            self.references.retain(|_, ocorrencias| !ocorrencias.is_empty());
        }
        self.declarations.retain(|_, declarado| declarado != path);
        if path.exists() {
            self.index_source(path, parser);
        }
    }

    /// Indexa a biblioteca padrão do JDK apontado por `JAVA_HOME`.
    ///
    /// A partir do Java 9 as classes vivem em `jmods/*.jmod`, que são zips com
    /// tudo sob `classes/`; até o 8, em `jre/lib/rt.jar`. Nos dois casos basta
    /// ler o diretório do arquivo: o nome da classe está no caminho da entrada,
    /// e os membros são lidos depois, um tipo por vez.
    ///
    /// O JDK é o escolhido na IDE. `JAVA_HOME` só entra quando nenhuma
    /// instalação foi escolhida ainda — um caminho fixo de ambiente não pode
    /// decidir no lugar do usuário, mas serve enquanto ele não decidiu.
    fn scan_jdk(&mut self, jdk_home: Option<&Path>) {
        let Some(home) = jdk_home
            .map(Path::to_path_buf)
            .or_else(|| std::env::var_os("JAVA_HOME").map(PathBuf::from))
        else {
            return;
        };
        let mut archives = Vec::new();
        if let Ok(entries) = fs::read_dir(home.join("jmods")) {
            archives.extend(entries.flatten().map(|entry| entry.path()).filter(|path| {
                path.extension()
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("jmod"))
            }));
            // `java.base` primeiro: nada corta mais, mas quem resolve um nome
            // simples repetido fica com o primeiro, e `java.lang` é a aposta
            // certa.
            archives.sort_by_key(|path| {
                let base = path
                    .file_name()
                    .is_some_and(|name| name == "java.base.jmod");
                (!base, path.clone())
            });
        } else {
            archives.extend(
                [home.join("jre/lib/rt.jar"), home.join("lib/rt.jar")]
                    .into_iter()
                    .filter(|path| path.is_file()),
            );
        }
        for archive in archives {
            let Ok(names) = java_classfile::list_classes(&archive, usize::MAX) else {
                continue;
            };
            self.external_classes
                .extend(names.into_iter().map(|binary| ExternalClass {
                    simple: simple_class_name(&binary),
                    binary,
                    origin: archive.clone(),
                }));
        }
    }
}

impl ExternalClass {
    /// Lê os metadados da classe, de onde quer que ela esteja.
    ///
    /// A leitura é sob demanda: guardar os membros de todas as classes de todos
    /// os jars indexados custaria memória proporcional ao classpath inteiro,
    /// para responder sobre um tipo de cada vez.
    pub(super) fn descriptor(&self) -> Option<java_classfile::ClassDescriptor> {
        if self.origin.extension().is_some_and(|extension| {
            extension.eq_ignore_ascii_case("jar")
                || extension.eq_ignore_ascii_case("zip")
                || extension.eq_ignore_ascii_case("jmod")
        }) {
            java_classfile::read_class_in_archive(&self.origin, &self.binary).ok()
        } else {
            fs::read(&self.origin)
                .ok()
                .and_then(|bytes| java_classfile::read_class(&bytes).ok())
        }
    }
}

impl Dados {
    pub(super) fn scan(
        root: &Path,
        source_roots: &[PathBuf],
        toolchain_root: Option<&Path>,
        parser: &mut Parser,
    ) -> Self {
        let mut paths = Vec::new();
        collect_workspace_paths(root, &mut paths);
        let mut index = Self::default();
        // A biblioteca padrão vem antes do workspace: `String` e `System` são
        // do JDK, e um tipo do projeto com o mesmo nome simples é a exceção,
        // não a regra.
        index.scan_jdk(toolchain_root);
        for path in paths {
            match path.extension().and_then(|extension| extension.to_str()) {
                Some(extension)
                    if extension.eq_ignore_ascii_case("java")
                        && (source_roots.is_empty()
                            || source_roots.iter().any(|root| path.starts_with(root))) =>
                {
                    index.index_source(&path, parser);
                    // Ceder entre arquivos é o que separa "demora" de "trava":
                    // o indexador é uma linha de execução só, e quem ela não
                    // pode atrapalhar é a que desenha.
                    std::thread::yield_now();
                }
                Some(extension) if extension.eq_ignore_ascii_case("class") => {
                    if let Ok(bytes) = fs::read(&path)
                        && let Ok(class) = java_classfile::read_class(&bytes)
                    {
                        index.external_classes.push(ExternalClass {
                            simple: simple_class_name(&class.binary_name),
                            binary: class.binary_name.clone(),
                            origin: path,
                        });
                    }
                }
                Some(extension) if extension.eq_ignore_ascii_case("jar") => {
                    if let Ok(classes) = java_classfile::index_jar(&path, usize::MAX) {
                        index
                            .external_classes
                            .extend(classes.into_iter().map(|class| ExternalClass {
                                simple: simple_class_name(&class.binary_name),
                                binary: class.binary_name,
                                origin: path.clone(),
                            }));
                    }
                }
                _ => {}
            }
        }
        index
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::documents::Documents;
    use file::{CABECALHO_PARA_TESTE, VERSION_PARA_TESTE};

    /// Um projeto pequeno, com o bastante para exercitar cada área do arquivo.
    fn projeto(nome: &str) -> PathBuf {
        let raiz = std::env::temp_dir().join(format!("er-ide-{nome}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&raiz);
        assert!(fs::create_dir_all(&raiz).is_ok());
        assert!(
            fs::write(
                raiz.join("Pedido.java"),
                "public class Pedido {\n  private java.util.List<String> itens;\n  \
                 public String nome() { return \"x\"; }\n}\n",
            )
            .is_ok()
        );
        assert!(
            fs::write(
                raiz.join("Servico.java"),
                "public interface Servico {\n  Pedido criar(String nome);\n}\n",
            )
            .is_ok()
        );
        assert!(
            fs::write(
                raiz.join("Estado.java"),
                "public enum Estado { ABERTO, FECHADO }\n",
            )
            .is_ok()
        );
        raiz
    }

    fn varrer(raiz: &Path) -> WorkspaceIndex {
        let documentos = match Documents::new() {
            Ok(documentos) => documentos,
            Err(error) => panic!("analisador indisponível: {error}"),
        };
        let dados = match documentos.with_parser_mut(|parser| {
            Dados::scan(
                raiz,
                std::slice::from_ref(&raiz.to_path_buf()),
                None,
                parser,
            )
        }) {
            Ok(dados) => dados,
            Err(error) => panic!("varredura falhou: {error}"),
        };
        WorkspaceIndex::construido(dados)
    }

    /// As declarações de um nome, em ordem estável.
    fn declaracoes(indice: &WorkspaceIndex, nome: &str) -> Vec<Location> {
        let mut achados: Vec<Location> = indice
            .symbols()
            .filter(|simbolo| simbolo.name() == nome)
            .filter_map(|simbolo| indice.location_of(simbolo))
            .collect();
        achados.sort_by(|esquerda, direita| {
            esquerda
                .path
                .cmp(&direita.path)
                .then(esquerda.range.start.line.cmp(&direita.range.start.line))
        });
        achados
    }

    /// O que a completação monta para um prefixo: nome, espécie e tipo.
    fn por_prefixo(indice: &WorkspaceIndex, prefixo: &str) -> Vec<String> {
        let mut achados: Vec<String> = indice
            .symbols()
            .filter(|simbolo| simbolo.name().starts_with(prefixo))
            .map(|simbolo| {
                format!(
                    "{}|{:?}|{:?}",
                    simbolo.name(),
                    simbolo.kind(),
                    simbolo.type_name()
                )
            })
            .collect();
        achados.sort();
        achados
    }

    fn ocorrencias(indice: &WorkspaceIndex, nome: &str) -> Vec<Location> {
        let mut achados: Vec<Location> = indice.references_to(nome).collect();
        achados.sort_by_key(|local| (local.path.clone(), local.range.start.line));
        achados
    }

    /// As quatro consultas respondem igual, venha o índice de onde vier.
    ///
    /// É o critério da fase 1 da `20` e metade do da fase 2: um índice
    /// construído e um índice **carregado do arquivo** têm de ser
    /// indistinguíveis para quem pergunta. Comparar as estruturas diria menos —
    /// o que precisa sobreviver ao disco é o que a IDE pergunta.
    #[test]
    fn what_goes_to_disk_answers_the_same_when_it_comes_back() {
        let raiz = projeto("indice-ciclo");
        let original = varrer(&raiz);
        let arquivo = raiz.join("indice.bin");
        assert!(original.save_to(&arquivo));
        let Some(lido) = WorkspaceIndex::carregar_de(&arquivo) else {
            panic!("o índice gravado não pôde ser carregado");
        };

        // 1. Ctrl+clique e navegação.
        for nome in ["Pedido", "Servico", "Estado", "nome", "criar", "itens"] {
            assert_eq!(
                declaracoes(&lido, nome),
                declaracoes(&original, nome),
                "declarações de {nome} mudaram ao passar pelo disco"
            );
        }

        // 2. Referências e renomear.
        for nome in ["Pedido", "String", "nome"] {
            assert_eq!(
                ocorrencias(&lido, nome),
                ocorrencias(&original, nome),
                "ocorrências de {nome} mudaram"
            );
        }

        // 3. Completação.
        for prefixo in ["", "P", "no", "cri"] {
            assert_eq!(
                por_prefixo(&lido, prefixo),
                por_prefixo(&original, prefixo),
                "a completação por {prefixo:?} mudou"
            );
        }

        // 4. Busca por tipo: o arquivo que declara cada tipo.
        for tipo in ["Pedido", "Servico", "Estado"] {
            assert_eq!(
                lido.declaring_file(tipo),
                original.declaring_file(tipo),
                "o arquivo que declara {tipo} mudou"
            );
            assert!(
                lido.declaring_file(tipo).is_some(),
                "o índice carregado precisa conhecer {tipo}"
            );
        }
        assert_eq!(lido.declaring_file("NaoExiste"), None);
        assert_eq!(lido.symbol_count(), original.symbol_count());
        let _ = fs::remove_dir_all(&raiz);
    }

    /// Reindexar depois de carregar não deixa a resposta antiga aparecer.
    ///
    /// É o ponto onde as duas origens poderiam discordar: o arquivo diz uma
    /// coisa, a memória diz outra, e a IDE responderia as duas. Quem já foi
    /// refeito na memória some do carregado.
    #[test]
    fn what_the_memory_redid_hides_what_the_file_says() {
        let raiz = projeto("indice-delta");
        let arquivo = raiz.join("indice.bin");
        assert!(varrer(&raiz).save_to(&arquivo));
        let Some(mut indice) = WorkspaceIndex::carregar_de(&arquivo) else {
            panic!("não carregou");
        };
        assert!(indice.symbols().any(|simbolo| simbolo.name() == "Estado"));

        // O arquivo muda no disco e é reindexado: o tipo antigo sai, o novo entra.
        let fonte = raiz.join("Estado.java");
        assert!(fs::write(&fonte, "public enum Situacao { NOVO }\n").is_ok());
        let documentos = match Documents::new() {
            Ok(documentos) => documentos,
            Err(error) => panic!("analisador indisponível: {error}"),
        };
        assert!(
            documentos
                .with_parser_mut(|parser| indice.reindex_file(&fonte, parser))
                .is_ok()
        );

        assert!(
            !indice.symbols().any(|simbolo| simbolo.name() == "Estado"),
            "o que o arquivo dizia sobre este fonte não pode mais aparecer"
        );
        assert!(
            indice.symbols().any(|simbolo| simbolo.name() == "Situacao"),
            "e o que a memória leu tem de aparecer"
        );
        // Os outros arquivos continuam vindo do carregado, intactos.
        assert!(indice.symbols().any(|simbolo| simbolo.name() == "Pedido"));
        assert_eq!(
            indice.declaring_file("Situacao"),
            Some(fonte.clone()),
            "e o tipo novo é encontrado no arquivo certo"
        );
        assert!(
            ocorrencias(&indice, "Estado").is_empty(),
            "as ocorrências do fonte refeito também saem do carregado"
        );
        let _ = fs::remove_dir_all(&raiz);
    }

    /// A diferença enxerga o que mudou, e só o que mudou.
    #[test]
    fn the_difference_sees_what_changed_and_only_that() {
        let raiz = projeto("indice-diferenca");
        let arquivo = raiz.join("indice.bin");
        assert!(varrer(&raiz).save_to(&arquivo));
        let fontes = vec![raiz.clone()];
        let recarregar = || match WorkspaceIndex::carregar_de(&arquivo) {
            Some(indice) => indice,
            None => panic!("nao carregou"),
        };
        assert!(
            recarregar().diferenca(&raiz, &fontes).is_empty(),
            "sem nada mudar, nada difere"
        );

        // Alterado.
        let fonte = raiz.join("Pedido.java");
        assert!(fs::write(&fonte, "public class Pedido { int a; }\n").is_ok());
        assert_eq!(
            recarregar().diferenca(&raiz, &fontes),
            vec![fonte.clone()],
            "so o fonte alterado difere"
        );

        // Novo.
        assert!(varrer(&raiz).save_to(&arquivo));
        let novo = raiz.join("Novo.java");
        assert!(fs::write(&novo, "public class Novo {}\n").is_ok());
        assert_eq!(
            recarregar().diferenca(&raiz, &fontes),
            vec![novo.clone()],
            "so o fonte novo difere"
        );

        // Apagado.
        assert!(varrer(&raiz).save_to(&arquivo));
        assert!(fs::remove_file(&novo).is_ok());
        assert_eq!(
            recarregar().diferenca(&raiz, &fontes),
            vec![novo],
            "so o fonte apagado difere"
        );
        let _ = fs::remove_dir_all(&raiz);
    }

    /// Abrir relê o que mudou, e mais nada — e o disco fica em dia.
    ///
    /// É o critério da fase 4 da `20`, nas três metades: projeto inalterado não
    /// relê nada; um fonte alterado fora da IDE relê **aquele** fonte; e depois
    /// da reconciliação o arquivo volta a descrever o projeto, senão a próxima
    /// abertura recalcularia a mesma diferença.
    #[test]
    fn opening_rereads_what_changed_and_nothing_else() {
        let raiz = projeto("indice-reconcilia");
        let arquivo = raiz.join("indice.bin");
        assert!(varrer(&raiz).save_to(&arquivo));
        let fontes = vec![raiz.clone()];
        let documentos = match Documents::new() {
            Ok(documentos) => documentos,
            Err(error) => panic!("analisador indisponível: {error}"),
        };

        // 1. Projeto inalterado: nada é relido.
        let Some(intacto) = WorkspaceIndex::carregar_de(&arquivo) else {
            panic!("nao carregou");
        };
        assert!(intacto.diferenca(&raiz, &fontes).is_empty());
        assert_eq!(
            intacto.memory_symbol_count(),
            0,
            "projeto inalterado nao pode reler fonte nenhum"
        );

        // 2. Um fonte muda fora da IDE: só ele é relido.
        let fonte = raiz.join("Estado.java");
        assert!(fs::write(&fonte, "public enum Situacao { NOVO }\n").is_ok());
        let Some(mut indice) = WorkspaceIndex::carregar_de(&arquivo) else {
            panic!("nao carregou");
        };
        let mudaram = indice.diferenca(&raiz, &fontes);
        assert_eq!(mudaram, vec![fonte.clone()]);
        assert!(
            documentos
                .with_parser_mut(|parser| indice.reconciliar(&mudaram, parser))
                .is_ok()
        );
        // O tipo velho saiu, o novo entrou, e os outros arquivos nao foram
        // relidos: a memoria so tem o que veio deste fonte.
        assert!(!indice.symbols().any(|simbolo| simbolo.name() == "Estado"));
        assert!(indice.symbols().any(|simbolo| simbolo.name() == "Situacao"));
        assert!(indice.symbols().any(|simbolo| simbolo.name() == "Pedido"));
        assert!(
            indice.memory_symbol_count() < 5,
            "so um fonte foi relido, e ele declara pouca coisa: {}",
            indice.memory_symbol_count()
        );

        // 3. O disco volta a descrever o projeto.
        let Some(caminho_real) = file::caminho_do_indice(&raiz, None) else {
            panic!("sem diretorio de cache");
        };
        let _ = fs::remove_file(&caminho_real);
        assert!(indice.regravar(&raiz, None), "regravar precisa dizer que gravou");
        let Some(depois) = WorkspaceIndex::carregar_de(&caminho_real) else {
            panic!("o indice regravado nao pode ser carregado");
        };
        assert!(
            depois.diferenca(&raiz, &fontes).is_empty(),
            "depois de regravar, nada mais difere"
        );
        assert!(depois.veio_do_arquivo());
        // E o que ele responde é o que a reconciliação produziu.
        assert!(depois.symbols().any(|simbolo| simbolo.name() == "Situacao"));
        assert!(!depois.symbols().any(|simbolo| simbolo.name() == "Estado"));
        assert!(depois.symbols().any(|simbolo| simbolo.name() == "Pedido"));
        assert_eq!(
            depois.declaring_file("Situacao"),
            Some(fonte),
            "e o tipo novo continua no arquivo certo depois da volta ao disco"
        );
        assert!(
            !ocorrencias(&depois, "Pedido").is_empty(),
            "as ocorrencias dos fontes intactos sobrevivem a regravacao"
        );
        let _ = fs::remove_file(&caminho_real);
        let _ = fs::remove_dir_all(&raiz);
    }

    /// Gravar põe o arquivo onde ele deve estar.
    #[test]
    fn saving_puts_the_file_where_it_belongs() {
        let raiz = projeto("indice-save");
        let indice = varrer(&raiz);
        let Some(caminho) = file::caminho_do_indice(&raiz, None) else {
            panic!("sem diretório de cache no ambiente");
        };
        let _ = fs::remove_file(&caminho);
        assert!(indice.save(&raiz, None), "gravar precisa dizer que gravou");
        assert!(caminho.is_file(), "o arquivo tem de existir em {caminho:?}");
        assert!(
            WorkspaceIndex::carregar(&raiz, None).is_some(),
            "e tem de ser carregável"
        );
        // Um índice que veio do disco não se regrava: o delta não é o todo.
        let Some(carregado) = WorkspaceIndex::carregar(&raiz, None) else {
            panic!("não carregou");
        };
        assert!(
            !carregado.save(&raiz, None),
            "regravar um índice carregado perderia o que não foi reindexado"
        );
        let _ = fs::remove_file(&caminho);
        let _ = fs::remove_dir_all(&raiz);
    }

    /// Um arquivo que não serve é descartado, nunca lido pela metade.
    #[test]
    fn a_file_that_does_not_serve_is_discarded() {
        let raiz = projeto("indice-invalido");
        let arquivo = raiz.join("indice.bin");
        assert!(varrer(&raiz).save_to(&arquivo));
        let bom = match fs::read(&arquivo) {
            Ok(bytes) => bytes,
            Err(error) => panic!("não foi possível reler o arquivo: {error}"),
        };
        assert!(WorkspaceIndex::carregar_de(&arquivo).is_some());

        // Assinatura de outro programa.
        let mut outro = bom.clone();
        outro[0] = b'X';
        assert!(fs::write(&arquivo, &outro).is_ok());
        assert!(
            WorkspaceIndex::carregar_de(&arquivo).is_none(),
            "assinatura errada tem de ser recusada"
        );

        // Versão futura: o formato mudou, e converter é código que ninguém testa.
        let mut futuro = bom.clone();
        futuro[8..12].copy_from_slice(&(VERSION_PARA_TESTE + 1).to_le_bytes());
        assert!(fs::write(&arquivo, &futuro).is_ok());
        assert!(
            WorkspaceIndex::carregar_de(&arquivo).is_none(),
            "outra versão tem de ser recusada"
        );

        // Truncado: um desligamento no meio da escrita.
        for corte in [0, 8, CABECALHO_PARA_TESTE, bom.len() / 2, bom.len() - 1] {
            assert!(fs::write(&arquivo, &bom[..corte.min(bom.len())]).is_ok());
            assert!(
                WorkspaceIndex::carregar_de(&arquivo).is_none(),
                "arquivo cortado em {corte} tem de ser recusado"
            );
        }

        // E o bom continua bom: o teste não passaria por recusar tudo.
        assert!(fs::write(&arquivo, &bom).is_ok());
        assert!(WorkspaceIndex::carregar_de(&arquivo).is_some());
        let _ = fs::remove_dir_all(&raiz);
    }
}
