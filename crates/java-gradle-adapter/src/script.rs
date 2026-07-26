//! Extração textual dos scripts do Gradle.
//!
//! Gradle é tratado como ferramenta externa: a IDE não interpreta Groovy nem
//! Kotlin. Aqui só são extraídas as declarações convencionais que qualquer
//! script expõe em forma literal — `include` no settings, `rootProject.name` e
//! as dependências com coordenada em string. Qualquer coisa calculada em tempo
//! de execução é ignorada e continua acessível executando o Gradle.

use ide_project_model::{DependencyScope, ProjectCoordinates};

const CONFIGURATIONS: &[&str] = &[
    "implementation",
    "api",
    "compileOnly",
    "compileOnlyApi",
    "runtimeOnly",
    "annotationProcessor",
    "testImplementation",
    "testCompileOnly",
    "testRuntimeOnly",
    "testAnnotationProcessor",
    "providedCompile",
    "providedRuntime",
];

/// Caminhos relativos dos módulos declarados com `include`.
#[must_use]
pub(crate) fn included_modules(settings: &str) -> Vec<String> {
    let mut modules = Vec::new();
    for line in meaningful_lines(settings) {
        let Some(rest) = line
            .strip_prefix("include ")
            .or_else(|| line.strip_prefix("include("))
        else {
            continue;
        };
        for literal in string_literals(rest) {
            let path = literal.trim_start_matches(':').replace(':', "/");
            if !path.is_empty() && !modules.contains(&path) {
                modules.push(path);
            }
        }
    }
    modules
}

#[must_use]
pub(crate) fn root_project_name(settings: &str) -> Option<String> {
    meaningful_lines(settings)
        .filter(|line| line.starts_with("rootProject.name"))
        .find_map(|line| string_literals(line).into_iter().next())
}

/// Dependências declaradas com coordenada literal, com o escopo da configuração.
#[must_use]
pub(crate) fn declared_dependencies(script: &str) -> Vec<(ProjectCoordinates, DependencyScope)> {
    let mut dependencies = Vec::new();
    let mut depth = 0_usize;
    let mut inside = false;
    for line in meaningful_lines(script) {
        if !inside && line.starts_with("dependencies") && line.contains('{') {
            inside = true;
            depth = 0;
        }
        if !inside {
            continue;
        }
        depth += line.matches('{').count();
        depth = depth.saturating_sub(line.matches('}').count());
        if depth == 0 {
            inside = false;
            continue;
        }
        let Some(configuration) = CONFIGURATIONS
            .iter()
            .find(|configuration| starts_with_configuration(line, configuration))
        else {
            continue;
        };
        let scope = DependencyScope::parse(configuration);
        if let Some(coordinates) = string_literals(line)
            .into_iter()
            .find_map(|literal| coordinates(&literal))
        {
            dependencies.push((coordinates, scope));
            continue;
        }
        if let Some(coordinates) = map_coordinates(line) {
            dependencies.push((coordinates, scope));
        }
    }
    dependencies
}

fn starts_with_configuration(line: &str, configuration: &str) -> bool {
    line.strip_prefix(configuration)
        .is_some_and(|rest| rest.starts_with([' ', '(']))
}

fn meaningful_lines(script: &str) -> impl Iterator<Item = &str> {
    script
        .lines()
        .map(|line| line.split("//").next().unwrap_or(line).trim())
        .filter(|line| !line.is_empty() && !line.starts_with('*') && !line.starts_with("/*"))
}

fn string_literals(line: &str) -> Vec<String> {
    let mut literals = Vec::new();
    let mut rest = line;
    while let Some(start) = rest.find(['\'', '"']) {
        let Some(quote) = rest[start..].chars().next() else {
            break;
        };
        let tail = &rest[start + quote.len_utf8()..];
        let Some(end) = tail.find(quote) else {
            break;
        };
        literals.push(tail[..end].to_owned());
        rest = &tail[end + quote.len_utf8()..];
    }
    literals
}

fn coordinates(literal: &str) -> Option<ProjectCoordinates> {
    let parts: Vec<&str> = literal.split(':').collect();
    if !matches!(parts.len(), 2 | 3) || parts.iter().any(|part| part.trim().is_empty()) {
        return None;
    }
    if literal.contains("${") {
        return None;
    }
    Some(ProjectCoordinates {
        group: parts[0].to_owned(),
        artifact: parts[1].to_owned(),
        version: parts.get(2).copied().unwrap_or_default().to_owned(),
    })
}

/// Forma `group: 'x', name: 'y', version: 'z'`, usada em scripts Groovy.
fn map_coordinates(line: &str) -> Option<ProjectCoordinates> {
    let entry = |key: &str| -> Option<String> {
        let start = line.find(&format!("{key}:"))?;
        string_literals(&line[start..]).into_iter().next()
    };
    Some(ProjectCoordinates {
        group: entry("group")?,
        artifact: entry("name")?,
        version: entry("version").unwrap_or_default(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_included_modules_and_root_name_from_both_dialects() {
        let groovy =
            "rootProject.name = 'demo'\ninclude ':app', ':core:api'\n// include ':ignored'";
        assert_eq!(root_project_name(groovy).as_deref(), Some("demo"));
        assert_eq!(
            included_modules(groovy),
            vec!["app".to_owned(), "core/api".to_owned()]
        );

        let kotlin = "rootProject.name = \"demo\"\ninclude(\":app\")";
        assert_eq!(root_project_name(kotlin).as_deref(), Some("demo"));
        assert_eq!(included_modules(kotlin), vec!["app".to_owned()]);
    }

    #[test]
    fn reads_literal_dependencies_and_skips_computed_ones() {
        let script = r#"
plugins { id 'java' }

dependencies {
    implementation 'org.slf4j:slf4j-api:1.7.36'
    testImplementation("junit:junit:4.13.2")
    compileOnly group: 'javax.servlet', name: 'javax.servlet-api', version: '3.1.0'
    implementation "com.example:dynamic:${versions.lib}"
    implementation project(':core')
    // implementation 'commented:out:1.0'
}

task hello { doLast { println 'implementation not here' } }
"#;
        let dependencies = declared_dependencies(script);
        let labels: Vec<String> = dependencies
            .iter()
            .map(|(coordinates, scope)| format!("{}|{}", coordinates.label(), scope.as_str()))
            .collect();
        assert_eq!(
            labels,
            vec![
                "org.slf4j:slf4j-api:1.7.36|compile".to_owned(),
                "junit:junit:4.13.2|test".to_owned(),
                "javax.servlet:javax.servlet-api:3.1.0|provided".to_owned(),
            ]
        );
    }
}
