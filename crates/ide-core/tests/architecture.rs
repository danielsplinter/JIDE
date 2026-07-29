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
        ("ide-domain", BTreeSet::new()),
        ("ide-application", BTreeSet::from(["ide-domain"])),
        ("ide-language-api", BTreeSet::from(["ide-domain"])),
        (
            "ide-language-host",
            BTreeSet::from(["ide-domain", "ide-language-api"]),
        ),
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
    ]);
    for workspace_crate in &crates {
        let Some(allowed_dependencies) = allowed.get(workspace_crate.name.as_str()) else {
            continue;
        };
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
