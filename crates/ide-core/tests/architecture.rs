use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    fs,
    path::{Path, PathBuf},
};

struct WorkspaceCrate {
    name: String,
    manifest: toml::Value,
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn manifest(path: &Path) -> toml::Value {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) => panic!("não foi possível ler {}: {error}", path.display()),
    };
    match toml::from_str(&text) {
        Ok(manifest) => manifest,
        Err(error) => panic!("manifest inválido em {}: {error}", path.display()),
    }
}

fn workspace_crates(root_manifest: &toml::Value) -> Vec<WorkspaceCrate> {
    let root = workspace_root();
    let Some(members) = root_manifest
        .get("workspace")
        .and_then(|workspace| workspace.get("members"))
        .and_then(toml::Value::as_array)
    else {
        panic!("workspace.members não foi declarado");
    };
    members
        .iter()
        .map(|member| {
            let Some(relative) = member.as_str() else {
                panic!("workspace member não é texto: {member}");
            };
            let crate_manifest = manifest(&root.join(relative).join("Cargo.toml"));
            let Some(name) = crate_manifest
                .get("package")
                .and_then(|package| package.get("name"))
                .and_then(toml::Value::as_str)
            else {
                panic!("crate sem package.name: {relative}");
            };
            WorkspaceCrate {
                name: name.to_owned(),
                manifest: crate_manifest,
            }
        })
        .collect()
}

fn dependency_names(manifest: &toml::Value, include_dev: bool) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    let sections = if include_dev {
        ["dependencies", "build-dependencies", "dev-dependencies"].as_slice()
    } else {
        ["dependencies", "build-dependencies"].as_slice()
    };
    for section in sections {
        if let Some(table) = manifest.get(*section).and_then(toml::Value::as_table) {
            names.extend(table.keys().cloned());
        }
    }
    if let Some(targets) = manifest.get("target").and_then(toml::Value::as_table) {
        for target in targets.values() {
            for section in sections {
                if let Some(table) = target.get(*section).and_then(toml::Value::as_table) {
                    names.extend(table.keys().cloned());
                }
            }
        }
    }
    names
}

fn rust_sources(directory: &Path) -> Vec<PathBuf> {
    let entries = fs::read_dir(directory)
        .unwrap_or_else(|error| panic!("não foi possível listar {}: {error}", directory.display()));
    let mut sources = Vec::new();
    for entry in entries {
        let entry =
            entry.unwrap_or_else(|error| panic!("não foi possível ler item do diretório: {error}"));
        let path = entry.path();
        if path.is_dir() {
            sources.extend(rust_sources(&path));
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            sources.push(path);
        }
    }
    sources
}

#[test]
fn ui_dependencies_are_relative_centralized_and_versioned() {
    let root = workspace_root();
    let root_manifest = manifest(&root.join("Cargo.toml"));
    let Some(dependencies) = root_manifest
        .get("workspace")
        .and_then(|workspace| workspace.get("dependencies"))
        .and_then(toml::Value::as_table)
    else {
        panic!("workspace.dependencies não foi declarado");
    };
    let ui_dependencies = dependencies
        .iter()
        .filter(|(name, _)| name.starts_with("ui-"))
        .collect::<Vec<_>>();
    assert!(
        !ui_dependencies.is_empty(),
        "dependências da ERLibUi ausentes"
    );
    for (name, dependency) in ui_dependencies {
        let Some(table) = dependency.as_table() else {
            panic!("{name} precisa declarar path e version");
        };
        let Some(path) = table.get("path").and_then(toml::Value::as_str) else {
            panic!("{name} não declara path");
        };
        assert!(
            Path::new(path).is_relative(),
            "{name} usa caminho absoluto: {path}"
        );
        assert!(
            table
                .get("version")
                .and_then(toml::Value::as_str)
                .is_some_and(|version| !version.trim().is_empty()),
            "{name} não possui versão explícita"
        );
    }

    for workspace_crate in workspace_crates(&root_manifest) {
        let mut dependency_tables = Vec::new();
        for section in ["dependencies", "build-dependencies", "dev-dependencies"] {
            if let Some(table) = workspace_crate
                .manifest
                .get(section)
                .and_then(toml::Value::as_table)
            {
                dependency_tables.push(table);
            }
        }
        if let Some(targets) = workspace_crate
            .manifest
            .get("target")
            .and_then(toml::Value::as_table)
        {
            for target in targets.values() {
                for section in ["dependencies", "build-dependencies", "dev-dependencies"] {
                    if let Some(table) = target.get(section).and_then(toml::Value::as_table) {
                        dependency_tables.push(table);
                    }
                }
            }
        }
        for table in dependency_tables {
            for (name, dependency) in table {
                if let Some(path) = dependency
                    .as_table()
                    .and_then(|dependency| dependency.get("path"))
                    .and_then(toml::Value::as_str)
                {
                    assert!(
                        Path::new(path).is_relative(),
                        "{} -> {name} usa caminho absoluto: {path}",
                        workspace_crate.name
                    );
                }
                if name.starts_with("ui-") {
                    assert!(
                        dependency
                            .as_table()
                            .and_then(|dependency| dependency.get("workspace"))
                            .and_then(toml::Value::as_bool)
                            == Some(true),
                        "{} precisa herdar {name} de workspace.dependencies",
                        workspace_crate.name
                    );
                }
            }
        }
    }
}

#[test]
fn protected_crates_only_depend_on_allowed_internal_boundaries() {
    let root_manifest = manifest(&workspace_root().join("Cargo.toml"));
    let crates = workspace_crates(&root_manifest);
    let internal = crates
        .iter()
        .map(|workspace_crate| workspace_crate.name.clone())
        .collect::<BTreeSet<_>>();
    let allowed = BTreeMap::from([
        (
            "ide-app",
            BTreeSet::from([
                "ide-application",
                "ide-core",
                "ide-debug-api",
                "ide-domain",
                "ide-language-api",
                "ide-language-host",
                "ide-process",
                "ide-project",
                "ide-terminal",
                "ide-toolchain-api",
                "ide-ui",
                "ide-workspace",
                "language-java",
                "language-style",
                "language-typescript",
            ]),
        ),
        ("ide-domain", BTreeSet::new()),
        (
            "ide-application",
            BTreeSet::from([
                "ide-debug-api",
                "ide-domain",
                "ide-language-api",
                "ide-toolchain-api",
            ]),
        ),
        ("ide-core", BTreeSet::from(["ide-domain"])),
        ("ide-language-api", BTreeSet::from(["ide-domain"])),
        (
            "ide-language-host",
            BTreeSet::from(["ide-domain", "ide-language-api"]),
        ),
        ("ide-process", BTreeSet::from(["ide-domain"])),
        ("ide-project", BTreeSet::new()),
        ("ide-terminal", BTreeSet::new()),
        ("ide-toolchain-api", BTreeSet::from(["ide-domain"])),
        ("ide-debug-api", BTreeSet::from(["ide-domain"])),
        (
            "ide-workspace",
            BTreeSet::from(["ide-application", "ide-domain"]),
        ),
        (
            "ide-ui",
            BTreeSet::from([
                "ide-application",
                "ide-domain",
                "ide-terminal",
                "ide-workspace",
            ]),
        ),
        // Uma crate por linguagem, desde a fase 8 da `12`: análise, toolchain,
        // build e depuração de Java moram juntas, e por isso a lista é a união
        // do que cada uma das cinco crates absorvidas exigia.
        //
        // O que o compilador garantia e agora não garante — o analisador não
        // alcançar `ide-process` nem `ide-project` — passou a ser a guarda
        // `the_java_analyzer_cannot_reach_process_or_project`.
        // A segunda linguagem custou **uma** crate, que é o que a fase 8 da `12`
        // veio comprar. No formato antigo teria custado até seis.
        //
        // `ide-process`, `ide-project` e `ide-toolchain-api` entraram na fase 2
        // da `23`, com o sistema de build de npm e a instalação de Node — e é
        // por isso que o analisador dela passou a precisar da mesma cerca que o
        // de Java tem.
        // Folhas de estilo não compilam nem executam: só gramática e contrato.
        (
            "language-style",
            BTreeSet::from(["ide-domain", "ide-language-api"]),
        ),
        (
            "language-typescript",
            BTreeSet::from([
                "ide-domain",
                "ide-language-api",
                "ide-process",
                "ide-project",
                "ide-toolchain-api",
            ]),
        ),
        (
            "language-java",
            BTreeSet::from([
                "ide-debug-api",
                "ide-domain",
                "ide-language-api",
                "ide-process",
                "ide-project",
                "ide-toolchain-api",
            ]),
        ),
    ]);
    assert_eq!(
        allowed.keys().copied().collect::<BTreeSet<_>>(),
        internal.iter().map(String::as_str).collect::<BTreeSet<_>>(),
        "toda crate do workspace precisa participar do mapa arquitetural"
    );
    for workspace_crate in &crates {
        let allowed_dependencies = &allowed[workspace_crate.name.as_str()];
        let actual = dependency_names(&workspace_crate.manifest, false)
            .into_iter()
            .filter(|name| internal.contains(name))
            .collect::<BTreeSet<_>>();
        let expected = allowed_dependencies
            .iter()
            .map(|name| (*name).to_owned())
            .collect::<BTreeSet<_>>();
        assert_eq!(
            actual, expected,
            "{} atravessou uma fronteira arquitetural",
            workspace_crate.name
        );
    }
}

/// Toda linguagem concreta é ligada só na raiz de composição.
///
/// A guarda fala de `language-*`, e não de Java, porque o critério é o da
/// linguagem desde a fase 8 da `12` — é o único que não precisa ser reescrito
/// quando a terceira e a quarta entrarem.
#[test]
fn concrete_language_crates_stay_behind_the_composition_root() {
    let root_manifest = manifest(&workspace_root().join("Cargo.toml"));
    let crates = workspace_crates(&root_manifest);
    let language_crates = crates
        .iter()
        .map(|workspace_crate| workspace_crate.name.as_str())
        .filter(|name| name.starts_with("language-"))
        .collect::<BTreeSet<_>>();
    assert!(
        !language_crates.is_empty(),
        "o workspace precisa ter ao menos uma crate de linguagem"
    );

    for language in language_crates {
        let consumers = crates
            .iter()
            .filter(|workspace_crate| {
                dependency_names(&workspace_crate.manifest, false).contains(language)
            })
            .map(|workspace_crate| workspace_crate.name.as_str())
            .collect::<BTreeSet<_>>();
        assert_eq!(
            consumers,
            BTreeSet::from(["ide-app"]),
            "{language} só pode ser consumida pela raiz de composição"
        );
    }
}

/// O analisador não dispara processo e não lê o modelo de projeto.
///
/// Até a fase 8 da `12` quem garantia isto era o compilador: `language-java`
/// era uma crate que não dependia de `ide-process` nem de `ide-project`. Numa
/// crate por linguagem a dependência existe — o `build` precisa dela —, e
/// `pub(crate)` não ajuda, porque ele protege o lado de fora do lado de dentro e
/// **não particiona o lado de dentro**.
///
/// Esta guarda é o que sobrou, e é mais fraca de propósito: texto, não tipo. A
/// troca — vinte crates evitadas por uma guarda de texto — está registrada na
/// `12`.
#[test]
fn the_analyzer_of_a_language_cannot_reach_process_or_project() {
    let root = workspace_root();
    let forbidden = ["ide_process", "ide_project"];
    let mut debt = BTreeSet::new();

    for language in ["language-java", "language-typescript"] {
        let analyzer = root.join("crates").join(language).join("src/analyzer");
        assert!(
            analyzer.is_dir(),
            "{language} precisa manter a análise em src/analyzer"
        );
        for path in rust_sources(&analyzer) {
            let relative = path.strip_prefix(&root).unwrap_or(&path);
            let source = fs::read_to_string(&path).unwrap_or_else(|error| {
                panic!("não foi possível ler {}: {error}", relative.display())
            });
            for term in forbidden {
                if source.contains(term) {
                    debt.insert(format!("{}:{term}", relative.display()));
                }
            }
        }
    }

    assert!(
        debt.is_empty(),
        "o analisador não pode alcançar processo nem modelo de projeto: {debt:?}"
    );
}

#[test]
fn neutral_crates_expose_no_language_specific_public_api() {
    let root = workspace_root();
    // `ide-core` entrou na fase 0 da `23`, e faltava desde sempre: é onde a
    // configuração persistida vive, e ela conhecia JDK e Maven pelo nome. A
    // guarda tinha um furo exatamente onde estava o vazamento.
    let sources = [
        "crates/ide-application/src",
        "crates/ide-core/src",
        "crates/ide-ui/src",
        "crates/ide-workspace/src",
    ]
    .into_iter()
    .flat_map(|relative| rust_sources(&root.join(relative)))
    .collect::<Vec<_>>();
    // `node_` e não `node`: o segundo casaria com `FileNode`, que é vocabulário
    // de árvore e não de linguagem. O que se quer barrar é `node_home` e
    // `node_modules`.
    let language_terms = [
        "java",
        "jdk",
        "jvm",
        "maven",
        "gradle",
        "typescript",
        "node_",
        "npm",
        "tsconfig",
        "angular",
        // Não é nome de linguagem, é de plataforma — e vazou do mesmo jeito,
        // como `classpath_entries` num contrato de tarefa que não pode saber o
        // que é uma JVM.
        "classpath",
    ];
    let mut actual_debt = BTreeSet::new();

    for path in sources {
        let relative = path.strip_prefix(&root).unwrap_or(&path);
        let source = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("não foi possível ler {}: {error}", relative.display()));
        for line in source.lines().map(str::trim) {
            let public_symbol = line
                .strip_prefix("pub fn ")
                .or_else(|| line.strip_prefix("pub struct "))
                .or_else(|| line.strip_prefix("pub enum "))
                .or_else(|| line.strip_prefix("pub trait "))
                .or_else(|| line.strip_prefix("pub type "))
                .or_else(|| line.strip_prefix("pub const "))
                .or_else(|| line.strip_prefix("pub static "))
                .or_else(|| line.strip_prefix("pub mod "))
                .or_else(|| line.strip_prefix("pub use "))
                // Campo de struct também é API pública, e não era examinado:
                // `pub jdk_home: Option<PathBuf>` não começa por nenhum dos
                // prefixos acima e passava mesmo com a crate na lista.
                .or_else(|| {
                    line.strip_prefix("pub ")
                        .filter(|rest| rest.contains(':') && !rest.starts_with("fn "))
                })
                .and_then(|rest| {
                    rest.split(|character: char| {
                        !(character.is_ascii_alphanumeric() || character == '_')
                    })
                    .next()
                });
            if let Some(symbol) = public_symbol {
                let declaration = line.to_ascii_lowercase();
                if language_terms.iter().any(|term| declaration.contains(term)) {
                    actual_debt.insert(format!("{}:{symbol}", relative.display()));
                }
            }
        }
    }

    assert!(
        actual_debt.is_empty(),
        "APIs públicas das crates neutras não podem expor conceitos de linguagem: {actual_debt:?}"
    );
}

#[test]
fn native_ide_has_no_language_specific_fields_or_constructors() {
    let root = workspace_root();
    let native = fs::read_to_string(root.join("crates/ide-app/src/native_ide.rs"))
        .unwrap_or_else(|error| panic!("não foi possível ler native_ide.rs: {error}"));
    let struct_start = native
        .find("struct NativeIde {")
        .unwrap_or_else(|| panic!("NativeIde não foi encontrado"));
    let struct_body = &native[struct_start..];
    let struct_end = struct_body
        .find("\n}")
        .unwrap_or_else(|| panic!("fim de NativeIde não foi encontrado"));
    let fields = &struct_body[..struct_end];
    assert!(
        !fields.to_ascii_lowercase().contains("java"),
        "NativeIde não pode possuir campos específicos de linguagem"
    );

    for constructor in [
        "JavaLanguageProvider::new",
        "JavaToolchainAdapter::new",
        "JavaDebugAdapter::new",
        "MavenAdapter::new",
        "GradleAdapter::new",
        "TypeScriptLanguageProvider::new",
        "NodeToolchainProvider::new",
        "NpmAdapter::new",
    ] {
        assert!(
            !native.contains(constructor),
            "{constructor} precisa permanecer no módulo de composição da linguagem"
        );
    }

    // O caminho da ferramenta **principal** é neutro: o que a tela mostra é o
    // rótulo que a contribuição declarou — "JDK" em Java, "Node" em TypeScript.
    // Estes textos existiam escritos à mão, e com uma linguagem só ninguém
    // percebia; com duas, a janela de uma pediria a ferramenta da outra.
    //
    // A guarda é uma lista, e não uma varredura por nome: o caminho da segunda
    // ferramenta **ainda** nomeia o Maven, de propósito, porque generalizá-lo
    // depende de haver uma segunda linguagem com segunda ferramenta. Ver a
    // fase 0 da `23`.
    for texto in [
        "Selecionar pasta do JDK",
        "JDK a salvar",
        "No JDK selected",
        "Selected JDK",
        "selecione um JDK",
    ] {
        assert!(
            !native.contains(texto),
            "o caminho da ferramenta principal não pode nomear a de uma linguagem: {texto}"
        );
    }

    let composition = fs::read_to_string(root.join("crates/ide-app/src/java_contribution.rs"))
        .unwrap_or_else(|error| panic!("não foi possível ler java_contribution.rs: {error}"));
    assert!(
        composition.contains("JavaLanguageProvider::new")
            && composition.contains("JavaToolchainAdapter::new")
            && composition.contains("JavaDebugAdapter::new"),
        "a composição Java precisa montar provider e adapters"
    );
}

#[test]
fn tasks_and_toolchains_are_dispatched_without_java_branches_in_native_ide() {
    let root = workspace_root();
    let native = fs::read_to_string(root.join("crates/ide-app/src/native_ide.rs"))
        .unwrap_or_else(|error| panic!("não foi possível ler native_ide.rs: {error}"));
    let controllers = fs::read_to_string(root.join("crates/ide-app/src/controllers.rs"))
        .unwrap_or_else(|error| panic!("não foi possível ler controllers.rs: {error}"));
    for forbidden in [
        "enum JavaTask",
        "fn start_java_task",
        "fn detect_java_",
        "JavaToolchainProvider::",
        "CompilationRequest {",
        "ExecutionRequest {",
        "TestRequest {",
    ] {
        assert!(
            !native.contains(forbidden),
            "{forbidden} precisa pertencer à contribuição, não a NativeIde"
        );
    }
    assert!(
        native.contains("fn start_task(&mut self, task_id: TaskId)")
            && controllers.contains("controller.execute(&task_id, context)"),
        "NativeIde precisa despachar todas as tarefas pelo TaskController"
    );
}

/// Todo o código de produção do crate de interface, num texto só.
///
/// Os testes ficam de fora — eles falam de Java de propósito, porque é a
/// linguagem que os cenários usam.
fn neutral_ui_sources(root: &Path) -> String {
    fn coletar(diretorio: &Path, destino: &mut String) {
        let Ok(entradas) = fs::read_dir(diretorio) else {
            return;
        };
        for entrada in entradas.flatten() {
            let caminho = entrada.path();
            if caminho.is_dir() {
                coletar(&caminho, destino);
                continue;
            }
            if caminho.extension().is_none_or(|extensao| extensao != "rs")
                || caminho.file_name().is_some_and(|nome| nome == "tests.rs")
            {
                continue;
            }
            let Ok(fonte) = fs::read_to_string(&caminho) else {
                continue;
            };
            destino.push_str(
                fonte
                    .split("#[cfg(test)]\nmod tests")
                    .next()
                    .unwrap_or(&fonte),
            );
            destino.push('\n');
        }
    }

    let mut fontes = String::new();
    coletar(&root.join("crates/ide-ui/src"), &mut fontes);
    fontes
}

#[test]
fn phase_four_keeps_ui_and_workspace_driven_by_neutral_models() {
    let root = workspace_root();
    // A UI neutra é o crate inteiro, e não um arquivo dele: depois da fase 5 o
    // shell é uma casca com as áreas em módulos, e amarrar o guarda a um nome de
    // arquivo faria a regra mudar de endereço junto com o código.
    // Ver `14-ide-shell-decomposition`.
    let production_ui = neutral_ui_sources(&root);
    for forbidden in [
        "java.package",
        "java.class",
        "java.interface",
        "Buscar conteúdo em Java",
        "SettingsPage::Compiler",
        "ApplicationCommand::CompileProject",
        "ApplicationCommand::RunActiveFile",
        "ApplicationCommand::TestProject",
    ] {
        assert!(
            !production_ui.contains(forbidden),
            "a UI neutra não pode conter o fluxo concreto {forbidden}"
        );
    }
    assert!(
        production_ui.contains("UiContributionCatalog")
            && production_ui.contains("ApplicationCommand::ExecuteTask"),
        "templates, seções e tarefas precisam chegar à UI pelo catálogo"
    );

    let workspace_search = fs::read_to_string(root.join("crates/ide-workspace/src/search.rs"))
        .unwrap_or_else(|error| panic!("não foi possível ler a busca: {error}"));
    assert!(
        workspace_search.contains("SearchScope") && !workspace_search.contains("\"java\""),
        "a busca de conteúdo deve obedecer SearchScope sem inferir Java"
    );
}

fn visit(
    node: &str,
    graph: &HashMap<String, BTreeSet<String>>,
    state: &mut HashMap<String, u8>,
    stack: &mut Vec<String>,
) -> Result<(), String> {
    match state.get(node).copied().unwrap_or_default() {
        2 => return Ok(()),
        1 => {
            stack.push(node.to_owned());
            return Err(stack.join(" -> "));
        }
        _ => {}
    }
    state.insert(node.to_owned(), 1);
    stack.push(node.to_owned());
    if let Some(dependencies) = graph.get(node) {
        for dependency in dependencies {
            visit(dependency, graph, state, stack)?;
        }
    }
    stack.pop();
    state.insert(node.to_owned(), 2);
    Ok(())
}

#[test]
fn internal_crate_graph_has_no_cycles() {
    let root_manifest = manifest(&workspace_root().join("Cargo.toml"));
    let crates = workspace_crates(&root_manifest);
    let internal = crates
        .iter()
        .map(|workspace_crate| workspace_crate.name.clone())
        .collect::<BTreeSet<_>>();
    let graph = crates
        .iter()
        .map(|workspace_crate| {
            let dependencies = dependency_names(&workspace_crate.manifest, true)
                .into_iter()
                .filter(|name| internal.contains(name))
                .collect();
            (workspace_crate.name.clone(), dependencies)
        })
        .collect::<HashMap<_, _>>();
    let mut state = HashMap::new();
    for node in graph.keys() {
        if let Err(cycle) = visit(node, &graph, &mut state, &mut Vec::new()) {
            panic!("ciclo entre crates: {cycle}");
        }
    }
}

fn struct_field_count(source: &str, name: &str) -> usize {
    let marker = format!("struct {name} {{");
    let body = source
        .split_once(&marker)
        .unwrap_or_else(|| panic!("estrutura {name} não encontrada"))
        .1
        .split_once('}')
        .unwrap_or_else(|| panic!("estrutura {name} sem fechamento"))
        .0;
    body.lines()
        .map(str::trim)
        .filter(|line| {
            !line.is_empty() && !line.starts_with("//") && line.ends_with(',') && line.contains(':')
        })
        .count()
}

#[test]
fn phase_five_keeps_ui_state_split_by_feature() {
    let root = workspace_root();
    let ui_root = root.join("crates/ide-ui/src");
    let facade = fs::read_to_string(ui_root.join("lib.rs"))
        .unwrap_or_else(|error| panic!("não foi possível ler ide-ui/src/lib.rs: {error}"));
    assert!(
        facade.lines().count() <= 1_500,
        "ide-ui/src/lib.rs deve permanecer uma fachada com no máximo 1.500 linhas"
    );

    let shell = fs::read_to_string(ui_root.join("ide_shell.rs"))
        .unwrap_or_else(|error| panic!("não foi possível ler ide_shell.rs: {error}"));
    assert!(
        struct_field_count(&shell, "IdeShell") <= 16,
        "IdeShell deve possuir no máximo 16 campos de coordenação"
    );

    let features = [
        ("explorer.rs", "ExplorerState"),
        ("editor.rs", "EditorAreaState"),
        ("terminal.rs", "TerminalPanelState"),
        // A busca virou superfície na fase 3: o estado dela mudou de arquivo,
        // não de dono. Ver `14-ide-shell-decomposition`.
        ("ide_shell/type_search.rs", "TypeSearchSurface"),
        ("ide_shell/settings.rs", "SettingsSurface"),
        ("debugging.rs", "DebugPanelState"),
        ("menus.rs", "MenuState"),
    ];
    let state_names = features.iter().map(|(_, state)| *state).collect::<Vec<_>>();
    for (file, own_state) in features {
        let source = fs::read_to_string(ui_root.join(file))
            .unwrap_or_else(|error| panic!("não foi possível ler {file}: {error}"));
        assert!(
            struct_field_count(&source, own_state) <= 20,
            "{own_state} ultrapassou o teto de 20 campos"
        );
        assert!(
            !source.contains("IdeShell"),
            "{file} não pode receber ou acessar o IdeShell inteiro"
        );
        for foreign_state in state_names
            .iter()
            .copied()
            .filter(|state| *state != own_state)
        {
            assert!(
                !source.contains(foreign_state),
                "{file} não pode acessar diretamente o estado {foreign_state}"
            );
        }
    }
}

#[test]
fn phase_six_keeps_native_application_split_into_controllers() {
    let root = workspace_root();
    let app_root = root.join("crates/ide-app/src");
    let main = fs::read_to_string(app_root.join("main.rs"))
        .unwrap_or_else(|error| panic!("não foi possível ler ide-app/src/main.rs: {error}"));
    assert!(
        main.lines().count() <= 800,
        "ide-app/src/main.rs deve permanecer com no máximo 800 linhas"
    );

    let native = fs::read_to_string(app_root.join("native_ide.rs"))
        .unwrap_or_else(|error| panic!("não foi possível ler native_ide.rs: {error}"));
    assert!(
        struct_field_count(&native, "NativeIde") <= 12,
        "NativeIde deve possuir no máximo 12 campos de coordenação"
    );
    for field_type in [
        "NativeWindowState",
        "WorkspaceController",
        "DocumentController",
        "LanguageController",
        "ProjectController",
        "AppTaskController",
        "AppDebugController",
        "UiBridge",
    ] {
        assert!(
            native.contains(field_type),
            "NativeIde precisa coordenar {field_type}"
        );
    }

    let controllers = fs::read_to_string(app_root.join("controllers.rs"))
        .unwrap_or_else(|error| panic!("não foi possível ler controllers.rs: {error}"));
    assert!(
        !controllers.contains("NativeIde"),
        "controllers não podem receber ou conhecer NativeIde"
    );
    assert!(
        controllers.contains("fn synchronize_application")
            && controllers.contains("fn synchronize_documents")
            && controllers.contains("fn reset_import"),
        "documentos, linguagem e projeto precisam possuir seus casos de uso"
    );

    let bridge = fs::read_to_string(app_root.join("ui_bridge.rs"))
        .unwrap_or_else(|error| panic!("não foi possível ler ui_bridge.rs: {error}"));
    assert!(
        bridge.contains("impl From<ApplicationCommand> for UiAction")
            && bridge.contains("fn actions("),
        "UiBridge precisa traduzir ApplicationCommand antes do despacho"
    );
    assert!(
        !native.contains("ApplicationCommand::OpenDocument")
            && !native.contains("ApplicationCommand::SaveDocument"),
        "NativeIde não pode voltar a traduzir comandos de UI diretamente"
    );
}

#[test]
fn phase_seven_keeps_language_state_in_its_owning_modules() {
    let root = workspace_root();
    let java_root = root.join("crates/language-java/src");
    let java_facade = fs::read_to_string(java_root.join("lib.rs"))
        .unwrap_or_else(|error| panic!("não foi possível ler language-java/lib.rs: {error}"));
    assert!(
        java_facade.lines().count() <= 30,
        "language-java/lib.rs deve permanecer uma fachada de até 30 linhas"
    );
    for exported in [
        "JavaLanguageProvider",
        "JAVA_LANGUAGE_ID",
        "JAVA_PROVIDER_ID",
    ] {
        assert!(
            java_facade.contains(exported),
            "a fachada Java precisa preservar {exported}"
        );
    }

    // Os módulos da análise passaram para `analyzer/` na fase 8 da `12`, quando
    // toolchain, build e depuração entraram na mesma crate e a análise precisou
    // de um lugar próprio para a guarda poder falar dela.
    let analyzer_root = java_root.join("analyzer");
    let language = fs::read_to_string(analyzer_root.join("language.rs"))
        .unwrap_or_else(|error| panic!("não foi possível ler language.rs: {error}"));
    let documents = fs::read_to_string(analyzer_root.join("documents.rs"))
        .unwrap_or_else(|error| panic!("não foi possível ler documents.rs: {error}"));
    // O índice virou diretório quando ganhou o formato em disco (fase 1 da 20):
    // `mod.rs` continua sendo quem possui a construção e a consulta.
    let index = fs::read_to_string(analyzer_root.join("index/mod.rs"))
        .unwrap_or_else(|error| panic!("não foi possível ler index/mod.rs: {error}"));
    assert!(
        documents.contains("struct Documents")
            && documents.contains("parser: JavaParser")
            && documents.contains("fn change(")
            && !language.contains("Mutex<HashMap<DocumentId, ParsedDocument>>"),
        "documents deve possuir documentos analisados e parsing incremental"
    );
    assert!(
        index.contains("struct WorkspaceIndex")
            && index.contains("fn scan(")
            && index.contains("external_classes")
            && !language.contains("struct WorkspaceIndex"),
        "index deve possuir construção e consulta do índice Java"
    );

    let host_root = root.join("crates/ide-language-host/src");
    let host_facade = fs::read_to_string(host_root.join("lib.rs"))
        .unwrap_or_else(|error| panic!("não foi possível ler ide-language-host/lib.rs: {error}"));
    assert!(
        host_facade.lines().count() <= 30,
        "ide-language-host/lib.rs deve permanecer uma fachada de até 30 linhas"
    );
    for exported in [
        "LanguageHost",
        "LanguageHostConfig",
        "LanguageHostError",
        "ProviderSnapshot",
        "ProviderSelection",
        "LanguageToolchainConfig",
    ] {
        assert!(
            host_facade.contains(exported),
            "a fachada do host precisa preservar {exported}"
        );
    }

    let host = fs::read_to_string(host_root.join("host.rs"))
        .unwrap_or_else(|error| panic!("não foi possível ler host.rs: {error}"));
    let worker = fs::read_to_string(host_root.join("worker.rs"))
        .unwrap_or_else(|error| panic!("não foi possível ler worker.rs: {error}"));
    let registry = fs::read_to_string(host_root.join("registry.rs"))
        .unwrap_or_else(|error| panic!("não foi possível ler registry.rs: {error}"));
    let routing = fs::read_to_string(host_root.join("routing.rs"))
        .unwrap_or_else(|error| panic!("não foi possível ler routing.rs: {error}"));
    assert!(
        worker.contains("struct ProviderWorker")
            && worker.contains("fn run_worker(")
            && worker.contains("WorkerRequest::Shutdown")
            && !host.contains("struct ProviderWorker"),
        "worker deve possuir ativação, fila, despacho e shutdown"
    );
    assert!(
        registry.contains("fn register(")
            && registry.contains("fn configure_selection(")
            && registry.contains("fn candidate_ids(")
            && registry.contains("fn route(")
            && routing.contains("struct ProviderSelection"),
        "registry e routing devem possuir registro, seleção e rotas"
    );
}

#[test]
fn phase_eight_preserves_the_final_architecture_metrics() {
    let root = workspace_root();
    let root_manifest = manifest(&root.join("Cargo.toml"));
    let crates = workspace_crates(&root_manifest);
    let internal = crates
        .iter()
        .map(|workspace_crate| workspace_crate.name.clone())
        .collect::<BTreeSet<_>>();
    let graph = crates
        .iter()
        .map(|workspace_crate| {
            let dependencies = dependency_names(&workspace_crate.manifest, true)
                .into_iter()
                .filter(|name| internal.contains(name))
                .collect::<BTreeSet<_>>();
            (workspace_crate.name.clone(), dependencies)
        })
        .collect::<BTreeMap<_, _>>();
    let edge_count = graph.values().map(BTreeSet::len).sum::<usize>();
    let app_fan_out = graph.get("ide-app").map_or(0, BTreeSet::len);
    let domain_fan_in = graph
        .values()
        .filter(|dependencies| dependencies.contains("ide-domain"))
        .count();

    // 16: 14 depois da fase 8 da `12`, mais TypeScript e mais as folhas de
    // estilo. A conta que a consolidação prometeu — uma linguagem, uma crate —
    // segue valendo na terceira.
    assert_eq!(crates.len(), 16, "a refatoração não deve pulverizar crates");
    assert!(
        edge_count <= 46,
        "o grafo interno ultrapassou a linha final de 46 arestas: {edge_count}"
    );
    assert!(
        app_fan_out <= 15,
        "ide-app ultrapassou o fan-out final de 15: {app_fan_out}"
    );
    // Era `>= 13`, absoluto, e a fase 8 mostrou que a forma estava errada: o
    // número caiu para 11 sozinho quando cinco crates viraram módulos, sem que
    // nada tivesse deixado de convergir. Limite inferior absoluto não sobrevive
    // a consolidação — o que se quer afirmar é a **proporção**.
    assert!(
        domain_fan_in * 2 > crates.len(),
        "contratos deixaram de convergir para ide-domain: {domain_fan_in} de {}",
        crates.len()
    );

    let line_limits = [
        // 17: cada linguagem é mais uma linha de `mod` na raiz de composição, e
        // é exatamente o que ela deve custar.
        ("crates/ide-app/src/main.rs", 17),
        // 31 desde a fase 2 da decomposição do shell: o módulo `text` reúne
        // funções que viviam duplicadas no shell e no editor. O teto existe para
        // a raiz continuar um manifesto, e uma linha de `mod` é o que ela é.
        ("crates/ide-ui/src/lib.rs", 31),
        // 18 desde a fase 8 da `12`: a fachada passou a declarar `analyzer`,
        // `build`, `debug` e `toolchain`, e a reexportar o que a raiz de
        // composição consome de cada um. Continua sendo só `mod` e `pub use` —
        // o teto existe para que nenhuma lógica se instale aqui.
        ("crates/language-java/src/lib.rs", 18),
        ("crates/ide-language-host/src/lib.rs", 10),
    ];
    for (relative, limit) in line_limits {
        let source = fs::read_to_string(root.join(relative))
            .unwrap_or_else(|error| panic!("não foi possível ler {relative}: {error}"));
        assert!(
            source.lines().count() <= limit,
            "{relative} ultrapassou a linha final de {limit} linhas"
        );
    }

    let native = fs::read_to_string(root.join("crates/ide-app/src/native_ide.rs"))
        .unwrap_or_else(|error| panic!("não foi possível ler NativeIde: {error}"));
    let shell = fs::read_to_string(root.join("crates/ide-ui/src/ide_shell.rs"))
        .unwrap_or_else(|error| panic!("não foi possível ler IdeShell: {error}"));
    assert_eq!(
        struct_field_count(&native, "NativeIde"),
        9,
        "NativeIde divergiu da linha final"
    );
    // Cresce durante a fase 3 da decomposição — cada janela que sai do arquivo
    // vira um campo aqui —, e volta a encolher na fase 4, quando as superfícies
    // passam a viver numa lista só. Ver `14-ide-shell-decomposition`.
    //
    // O 15º é o `host`: um anfitrião só para a tela inteira, que é o que tira das
    // mãos do shell a pergunta de quem recebe o gesto. Ver `16-single-host`.
    //
    // O 16º é a troca de abas por `Ctrl+Tab`, que é a **sétima** janela e segue o
    // padrão das outras seis. Cada janela nova custa um campo aqui, e é isso que
    // a fase 4 da `14` resolve: com as superfícies numa lista só, este número
    // volta a encolher em vez de acompanhar o número de janelas.
    assert_eq!(
        struct_field_count(&shell, "IdeShell"),
        16,
        "IdeShell divergiu da linha final"
    );
}

/// A IDE não desenha. Ponto.
///
/// Retângulo, contorno e texto crus não passam pelo tema, pela medição de fonte
/// nem pela árvore de acessibilidade. Quem desenha é o componente da ERLibUi; o
/// que falta nela é pedido a ela, e não contornado aqui.
///
/// Este teste já foi um **teto** que só podia encolher — 48 primitivas quando a
/// dívida foi medida, depois 2. Agora é zero, e zero é diferente de um número
/// pequeno: não há mais o que negociar quadro a quadro. As duas últimas caíram
/// quando a `Panel` ganhou borda por lado e a página de depuração passou a
/// entregar o foco ao próprio campo.
///
/// Os atalhos `raw_fill` e `raw_stroke` foram apagados junto: enquanto
/// existirem, desenhar à mão fica a uma linha de distância.
#[test]
fn the_ide_does_not_draw() {
    let fontes = neutral_ui_sources(&workspace_root());
    let cruas = fontes.match_indices("raw_fill(").count()
        + fontes.match_indices("raw_stroke(").count()
        + fontes.match_indices("raw_label(").count();
    assert_eq!(
        cruas, 0,
        "a IDE voltou a desenhar primitiva crua: desenhar é da biblioteca, e o          que falta nela deve ser pedido a ela"
    );
}
