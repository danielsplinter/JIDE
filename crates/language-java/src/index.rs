//! Varredura limitada das entradas que alimentam o índice do workspace.

use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};

use ide_domain::{
    DocumentId, DocumentSnapshot, Location, SemanticSymbol, SymbolKind, TextRange, TypeDescriptor,
};
use tree_sitter::Parser;

use crate::{language::analyze_semantics, symbols::simple_class_name};

pub(super) fn collect_workspace_paths(root: &Path, output: &mut Vec<PathBuf>) {
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

#[derive(Default)]
pub(super) struct WorkspaceIndex {
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

    /// Onde uma declaração está. É o que custa, então é pedido à parte.
    pub(super) fn location_of(&self, symbol: &IndexedSymbol) -> Option<Location> {
        Some(Location {
            path: self.files.get(symbol.file as usize)?.clone(),
            range: symbol.range,
        })
    }

    /// A declaração na forma que o resto da IDE fala.
    pub(super) fn materialize(&self, symbol: &IndexedSymbol) -> Option<SemanticSymbol> {
        Some(SemanticSymbol {
            name: symbol.name.clone(),
            kind: symbol.kind,
            location: self.location_of(symbol)?,
            type_descriptor: symbol.type_descriptor.clone(),
            scope_depth: symbol.scope_depth,
        })
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

impl WorkspaceIndex {
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

