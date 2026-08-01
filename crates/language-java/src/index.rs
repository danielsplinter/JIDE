//! Varredura limitada das entradas que alimentam o índice do workspace.

use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};

use ide_domain::{DocumentId, DocumentSnapshot, Location, SemanticSymbol, SymbolKind};
use tree_sitter::Parser;

use crate::{language::analyze_semantics, symbols::simple_class_name};

pub(super) fn collect_workspace_paths(root: &Path, output: &mut Vec<PathBuf>, limit: usize) {
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

#[derive(Default)]
pub(super) struct WorkspaceIndex {
    pub(super) symbols: Vec<SemanticSymbol>,
    pub(super) references: HashMap<String, Vec<Location>>,
    pub(super) external_classes: Vec<ExternalClass>,
    /// Arquivo que declara cada tipo do projeto, pelo nome simples.
    ///
    /// Guardar o caminho, e não os membros, é o que permite responder pela
    /// classe **como ela está agora**: o fonte é a verdade, e o `.class` do
    /// último build pode ser mais velho que o arquivo aberto ao lado.
    pub(super) declarations: HashMap<String, PathBuf>,
}

/// Teto de classes indexadas da biblioteca padrão.
///
/// `java.base` tem pouco mais de seis mil; o JDK inteiro passa de trinta mil.
/// O índice guarda só os nomes, mas guardar todos os módulos para responder
/// sobre `java.lang` custaria memória sem melhorar resposta nenhuma.
const MAX_JDK_CLASSES: usize = 24_000;

impl WorkspaceIndex {
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
        self.symbols.extend(semantic.symbols);
        merge_references(&mut self.references, references);
    }

    /// Reindexa **um** arquivo, tirando antes o que ele havia declarado.
    ///
    /// Salvar deixa de esperar a próxima ativação. Com 26 mil fontes, refazer o
    /// índice inteiro a cada gravação seria trabalho de minutos.
    pub(super) fn reindex_file(&mut self, path: &Path, parser: &mut Parser) {
        self.symbols.retain(|symbol| symbol.location.path != path);
        for locais in self.references.values_mut() {
            locais.retain(|local| local.path != path);
        }
        self.references.retain(|_, locais| !locais.is_empty());
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
            // `java.base` primeiro: se o teto cortar alguma coisa, que não seja
            // `java.lang`.
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
        let mut remaining = MAX_JDK_CLASSES;
        for archive in archives {
            if remaining == 0 {
                break;
            }
            let Ok(names) = java_classfile::list_classes(&archive, remaining) else {
                continue;
            };
            remaining = remaining.saturating_sub(names.len());
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
        collect_workspace_paths(root, &mut paths, 600);
        let mut index = Self::default();
        // A biblioteca padrão vem antes do workspace: `String` e `System` são
        // do JDK, e um tipo do projeto com o mesmo nome simples é a exceção,
        // não a regra.
        index.scan_jdk(toolchain_root);
        let mut java_count = 0;
        let mut archive_count = 0;
        for path in paths {
            match path.extension().and_then(|extension| extension.to_str()) {
                Some(extension)
                    if extension.eq_ignore_ascii_case("java")
                        && java_count < 500
                        && (source_roots.is_empty()
                            || source_roots.iter().any(|root| path.starts_with(root))) =>
                {
                    // A extração de `index_source` é da fase 4 e fica; o teto
                    // volta com a reversão da fase 3, e a contagem com ele.
                    index.index_source(&path, parser);
                    java_count += 1;
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
                Some(extension) if extension.eq_ignore_ascii_case("jar") && archive_count < 64 => {
                    if let Ok(classes) = java_classfile::index_jar(&path, 20_000) {
                        index
                            .external_classes
                            .extend(classes.into_iter().map(|class| ExternalClass {
                                simple: simple_class_name(&class.binary_name),
                                binary: class.binary_name,
                                origin: path.clone(),
                            }));
                    }
                    archive_count += 1;
                }
                _ => {}
            }
        }
        index
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
