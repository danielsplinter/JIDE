//! Leitura do modelo efetivo básico de um `pom.xml`.
//!
//! A interpretação é nativa e cobre o que a IDE precisa para indexar e compilar:
//! coordenadas herdadas do `<parent>`, propriedades, `<modules>`,
//! `<dependencyManagement>`, dependências e os diretórios declarados em
//! `<build>`. Casos que exigem o modelo efetivo completo — perfis ativos,
//! heranças fora do workspace, plugins que alteram o build — continuam
//! disponíveis executando o Maven externo.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use crate::xml::{self, XmlElement};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct RawDependency {
    pub(crate) group: String,
    pub(crate) artifact: String,
    pub(crate) version: Option<String>,
    pub(crate) scope: Option<String>,
    pub(crate) optional: bool,
    pub(crate) system_path: Option<String>,
}

impl RawDependency {
    pub(crate) fn key(&self) -> String {
        format!("{}:{}", self.group, self.artifact)
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct EffectivePom {
    pub(crate) path: PathBuf,
    pub(crate) group: String,
    pub(crate) artifact: String,
    pub(crate) version: String,
    pub(crate) packaging: String,
    pub(crate) name: Option<String>,
    pub(crate) properties: BTreeMap<String, String>,
    pub(crate) managed_versions: BTreeMap<String, String>,
    pub(crate) modules: Vec<String>,
    pub(crate) dependencies: Vec<RawDependency>,
    pub(crate) plugins: Vec<String>,
    pub(crate) source_directory: Option<String>,
    pub(crate) test_source_directory: Option<String>,
    pub(crate) build_directory: Option<String>,
    pub(crate) output_directory: Option<String>,
    pub(crate) test_output_directory: Option<String>,
}

pub(crate) fn read(path: &Path, parent: Option<&EffectivePom>) -> Result<EffectivePom, String> {
    let content = std::fs::read_to_string(path).map_err(|error| error.to_string())?;
    parse(path, &content, parent)
}

pub(crate) fn parse(
    path: &Path,
    content: &str,
    parent: Option<&EffectivePom>,
) -> Result<EffectivePom, String> {
    let root = xml::parse(content)?;
    if root.name != "project" {
        return Err(format!("`{}` is not a Maven project", path.display()));
    }
    let declared_parent = root.child("parent");
    let inherited_group = declared_parent
        .and_then(|element| element.child_text("groupId"))
        .map(str::to_owned)
        .or_else(|| parent.map(|parent| parent.group.clone()));
    let inherited_version = declared_parent
        .and_then(|element| element.child_text("version"))
        .map(str::to_owned)
        .or_else(|| parent.map(|parent| parent.version.clone()));

    let group = root
        .child_text("groupId")
        .map(str::to_owned)
        .or(inherited_group)
        .unwrap_or_default();
    let artifact = root
        .child_text("artifactId")
        .map(str::to_owned)
        .ok_or_else(|| format!("`{}` does not declare an artifactId", path.display()))?;
    let version = root
        .child_text("version")
        .map(str::to_owned)
        .or(inherited_version)
        .unwrap_or_default();

    let mut properties: BTreeMap<String, String> = parent
        .map(|parent| parent.properties.clone())
        .unwrap_or_default();
    if let Some(declared) = root.child("properties") {
        for property in &declared.children {
            properties.insert(property.name.clone(), property.text.trim().to_owned());
        }
    }
    properties.insert("project.groupId".to_owned(), group.clone());
    properties.insert("project.artifactId".to_owned(), artifact.clone());
    properties.insert("project.version".to_owned(), version.clone());
    properties.insert("pom.groupId".to_owned(), group.clone());
    properties.insert("pom.artifactId".to_owned(), artifact.clone());
    properties.insert("pom.version".to_owned(), version.clone());
    resolve_properties(&mut properties);

    let mut managed_versions: BTreeMap<String, String> = parent
        .map(|parent| parent.managed_versions.clone())
        .unwrap_or_default();
    if let Some(management) = root
        .child("dependencyManagement")
        .and_then(|element| element.child("dependencies"))
    {
        for dependency in read_dependencies(management, &properties) {
            if let Some(version) = dependency.version.clone() {
                managed_versions.insert(dependency.key(), version);
            }
        }
    }

    let dependencies = root
        .child("dependencies")
        .map(|element| read_dependencies(element, &properties))
        .unwrap_or_default();

    let modules = root
        .child("modules")
        .map(|element| {
            element
                .children_named("module")
                .map(|module| interpolate(module.text.trim(), &properties))
                .filter(|module| !module.is_empty())
                .collect()
        })
        .unwrap_or_default();

    let build = root.child("build");
    let plugins = build
        .and_then(|build| build.child("plugins"))
        .map(|plugins| {
            plugins
                .children_named("plugin")
                .filter_map(|plugin| plugin.child_text("artifactId").map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    let build_text = |name: &str| {
        build
            .and_then(|build| build.child_text(name))
            .map(|value| interpolate(value, &properties))
    };
    let source_directory = build_text("sourceDirectory");
    let test_source_directory = build_text("testSourceDirectory");
    let build_directory = build_text("directory");
    let output_directory = build_text("outputDirectory");
    let test_output_directory = build_text("testOutputDirectory");

    Ok(EffectivePom {
        path: path.to_path_buf(),
        packaging: root
            .child_text("packaging")
            .map_or_else(|| "jar".to_owned(), |value| value.to_owned()),
        name: root.child_text("name").map(str::to_owned),
        group,
        artifact,
        version,
        properties,
        managed_versions,
        modules,
        dependencies,
        plugins,
        source_directory,
        test_source_directory,
        build_directory,
        output_directory,
        test_output_directory,
    })
}

fn read_dependencies(
    container: &XmlElement,
    properties: &BTreeMap<String, String>,
) -> Vec<RawDependency> {
    container
        .children_named("dependency")
        .filter_map(|dependency| {
            let group = interpolate(dependency.child_text("groupId")?, properties);
            let artifact = interpolate(dependency.child_text("artifactId")?, properties);
            Some(RawDependency {
                group,
                artifact,
                version: dependency
                    .child_text("version")
                    .map(|value| interpolate(value, properties)),
                scope: dependency.child_text("scope").map(str::to_owned),
                optional: dependency
                    .child_text("optional")
                    .is_some_and(|value| value.eq_ignore_ascii_case("true")),
                system_path: dependency
                    .child_text("systemPath")
                    .map(|value| interpolate(value, properties)),
            })
        })
        .collect()
}

/// Expande `${...}` até estabilizar, com limite para referências cíclicas.
fn resolve_properties(properties: &mut BTreeMap<String, String>) {
    for _ in 0..4 {
        let snapshot = properties.clone();
        let mut changed = false;
        for value in properties.values_mut() {
            let resolved = interpolate(value, &snapshot);
            if &resolved != value {
                *value = resolved;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
}

pub(crate) fn interpolate(value: &str, properties: &BTreeMap<String, String>) -> String {
    if !value.contains("${") {
        return value.trim().to_owned();
    }
    let mut resolved = String::with_capacity(value.len());
    let mut rest = value;
    while let Some(start) = rest.find("${") {
        resolved.push_str(&rest[..start]);
        let tail = &rest[start..];
        let Some(end) = tail.find('}') else {
            resolved.push_str(tail);
            return resolved.trim().to_owned();
        };
        let name = &tail[2..end];
        match properties.get(name) {
            Some(replacement) if !replacement.contains("${") => resolved.push_str(replacement),
            _ => resolved.push_str(&tail[..=end]),
        }
        rest = &tail[end + 1..];
    }
    resolved.push_str(rest);
    resolved.trim().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    const PARENT: &str = r#"<project>
  <groupId>com.example</groupId>
  <artifactId>demo-parent</artifactId>
  <version>1.2.0</version>
  <packaging>pom</packaging>
  <properties>
    <junit.version>4.13.2</junit.version>
    <lib.version>${junit.version}</lib.version>
  </properties>
  <dependencyManagement>
    <dependencies>
      <dependency>
        <groupId>org.slf4j</groupId>
        <artifactId>slf4j-api</artifactId>
        <version>1.7.36</version>
      </dependency>
    </dependencies>
  </dependencyManagement>
  <modules>
    <module>app</module>
  </modules>
</project>"#;

    const CHILD: &str = r#"<project>
  <parent>
    <groupId>com.example</groupId>
    <artifactId>demo-parent</artifactId>
    <version>1.2.0</version>
  </parent>
  <artifactId>demo-app</artifactId>
  <dependencies>
    <dependency>
      <groupId>org.slf4j</groupId>
      <artifactId>slf4j-api</artifactId>
    </dependency>
    <dependency>
      <groupId>junit</groupId>
      <artifactId>junit</artifactId>
      <version>${junit.version}</version>
      <scope>test</scope>
    </dependency>
  </dependencies>
  <build>
    <sourceDirectory>src/java</sourceDirectory>
  </build>
</project>"#;

    fn parent_pom() -> EffectivePom {
        match parse(Path::new("/p/pom.xml"), PARENT, None) {
            Ok(pom) => pom,
            Err(error) => panic!("parent parsing failed: {error}"),
        }
    }

    #[test]
    fn inherits_group_version_properties_and_managed_versions() {
        let parent = parent_pom();
        assert_eq!(parent.modules, vec!["app".to_owned()]);
        assert_eq!(
            parent.properties.get("lib.version").map(String::as_str),
            Some("4.13.2")
        );

        let child = match parse(Path::new("/p/app/pom.xml"), CHILD, Some(&parent)) {
            Ok(child) => child,
            Err(error) => panic!("child parsing failed: {error}"),
        };
        assert_eq!(child.group, "com.example");
        assert_eq!(child.version, "1.2.0");
        assert_eq!(child.artifact, "demo-app");
        assert_eq!(child.source_directory.as_deref(), Some("src/java"));
        assert_eq!(
            child
                .managed_versions
                .get("org.slf4j:slf4j-api")
                .map(String::as_str),
            Some("1.7.36")
        );
        let junit = match child
            .dependencies
            .iter()
            .find(|dependency| dependency.artifact == "junit")
        {
            Some(dependency) => dependency,
            None => panic!("junit dependency is missing"),
        };
        assert_eq!(junit.version.as_deref(), Some("4.13.2"));
        assert_eq!(junit.scope.as_deref(), Some("test"));
    }

    #[test]
    fn unknown_placeholders_are_preserved_and_missing_artifact_fails() {
        let mut properties = BTreeMap::new();
        properties.insert("known".to_owned(), "value".to_owned());
        assert_eq!(
            interpolate("${known}-${other}", &properties),
            "value-${other}"
        );
        assert!(parse(Path::new("/p/pom.xml"), "<project></project>", None).is_err());
        assert!(parse(Path::new("/p/pom.xml"), "<other/>", None).is_err());
    }
}
