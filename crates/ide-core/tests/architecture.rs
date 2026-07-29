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
                "java-debug-adapter",
                "java-gradle-adapter",
                "java-maven-adapter",
                "java-toolchain",
                "language-java",
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
        ("java-classfile", BTreeSet::new()),
        (
            "java-debug-adapter",
            BTreeSet::from(["ide-debug-api", "ide-domain"]),
        ),
        (
            "java-gradle-adapter",
            BTreeSet::from(["ide-process", "ide-project"]),
        ),
        (
            "java-maven-adapter",
            BTreeSet::from(["ide-process", "ide-project"]),
        ),
        (
            "java-toolchain",
            BTreeSet::from(["ide-domain", "ide-process", "ide-toolchain-api"]),
        ),
        (
            "language-java",
            BTreeSet::from(["ide-domain", "ide-language-api", "java-classfile"]),
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

#[test]
fn concrete_java_crates_stay_behind_the_composition_root() {
    let root_manifest = manifest(&workspace_root().join("Cargo.toml"));
    let crates = workspace_crates(&root_manifest);
    let concrete_java_crates = BTreeSet::from([
        "java-classfile",
        "java-debug-adapter",
        "java-gradle-adapter",
        "java-maven-adapter",
        "java-toolchain",
        "language-java",
    ]);
    let expected_consumers = BTreeMap::from([
        ("java-classfile", BTreeSet::from(["language-java"])),
        ("java-debug-adapter", BTreeSet::from(["ide-app"])),
        ("java-gradle-adapter", BTreeSet::from(["ide-app"])),
        ("java-maven-adapter", BTreeSet::from(["ide-app"])),
        ("java-toolchain", BTreeSet::from(["ide-app"])),
        ("language-java", BTreeSet::from(["ide-app"])),
    ]);

    let mut actual_consumers = concrete_java_crates
        .iter()
        .map(|name| (*name, BTreeSet::new()))
        .collect::<BTreeMap<_, _>>();
    for workspace_crate in &crates {
        for dependency in dependency_names(&workspace_crate.manifest, false) {
            if let Some(consumers) = actual_consumers.get_mut(dependency.as_str()) {
                consumers.insert(workspace_crate.name.as_str());
            }
        }
    }

    assert_eq!(
        actual_consumers, expected_consumers,
        "implementações Java concretas só podem ser ligadas no composition root; \
         java-classfile é detalhe interno do provider Java"
    );
}

#[test]
fn neutral_crates_expose_no_language_specific_public_api() {
    let root = workspace_root();
    let sources = [
        "crates/ide-application/src/commands.rs",
        "crates/ide-ui/src/lib.rs",
        "crates/ide-workspace/src/lib.rs",
    ];
    let language_terms = ["java", "jdk", "jvm", "maven", "gradle"];
    let mut actual_debt = BTreeSet::new();

    for relative in sources {
        let source = fs::read_to_string(root.join(relative))
            .unwrap_or_else(|error| panic!("não foi possível ler {relative}: {error}"));
        for line in source.lines().map(str::trim) {
            let public_symbol = line
                .strip_prefix("pub fn ")
                .or_else(|| line.strip_prefix("pub struct "))
                .or_else(|| line.strip_prefix("pub enum "))
                .or_else(|| line.strip_prefix("pub trait "))
                .or_else(|| line.strip_prefix("pub type "))
                .and_then(|rest| {
                    rest.split(|character: char| {
                        !(character.is_ascii_alphanumeric() || character == '_')
                    })
                    .next()
                });
            if let Some(symbol) = public_symbol {
                let declaration = line.to_ascii_lowercase();
                if language_terms.iter().any(|term| declaration.contains(term)) {
                    actual_debt.insert(format!("{relative}:{symbol}"));
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
    let main = fs::read_to_string(root.join("crates/ide-app/src/main.rs"))
        .unwrap_or_else(|error| panic!("não foi possível ler ide-app/src/main.rs: {error}"));
    let struct_start = main
        .find("struct NativeIde {")
        .unwrap_or_else(|| panic!("NativeIde não foi encontrado"));
    let struct_body = &main[struct_start..];
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
    ] {
        assert!(
            !main.contains(constructor),
            "{constructor} precisa permanecer no módulo de composição Java"
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
    let main = fs::read_to_string(root.join("crates/ide-app/src/main.rs"))
        .unwrap_or_else(|error| panic!("não foi possível ler ide-app/src/main.rs: {error}"));
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
            !main.contains(forbidden),
            "{forbidden} precisa pertencer à contribuição, não a NativeIde"
        );
    }
    assert!(
        main.contains("fn start_task(&mut self, task_id: TaskId)")
            && main.contains("controller.execute(&task_id, context)"),
        "NativeIde precisa despachar todas as tarefas pelo TaskController"
    );
}

#[test]
fn phase_four_keeps_ui_and_workspace_driven_by_neutral_models() {
    let root = workspace_root();
    let ui = fs::read_to_string(root.join("crates/ide-ui/src/lib.rs"))
        .unwrap_or_else(|error| panic!("não foi possível ler ide-ui/src/lib.rs: {error}"));
    let shell = fs::read_to_string(root.join("crates/ide-ui/src/ide_shell.rs"))
        .unwrap_or_else(|error| panic!("não foi possível ler ide_shell.rs: {error}"));
    let production_shell = shell
        .split("#[cfg(test)]\nmod tests")
        .next()
        .unwrap_or(&shell);
    let production_ui = format!("{ui}\n{production_shell}");
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
        struct_field_count(&shell, "IdeShell") <= 15,
        "IdeShell deve possuir no máximo 15 campos de coordenação"
    );

    let features = [
        ("explorer.rs", "ExplorerState"),
        ("editor.rs", "EditorAreaState"),
        ("terminal.rs", "TerminalPanelState"),
        ("search.rs", "SearchState"),
        ("settings.rs", "SettingsState"),
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
