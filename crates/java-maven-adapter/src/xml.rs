//! Leitor XML mínimo, suficiente para descritores de build.
//!
//! O POM é um documento de estrutura simples: elementos aninhados com texto,
//! sem conteúdo misto. Este leitor cobre declaração, comentários, `DOCTYPE`,
//! `CDATA`, tags vazias e as entidades predefinidas; atributos são lidos e
//! descartados, pois o modelo efetivo do Maven não depende deles.

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct XmlElement {
    pub(crate) name: String,
    pub(crate) text: String,
    pub(crate) children: Vec<XmlElement>,
}

impl XmlElement {
    pub(crate) fn child(&self, name: &str) -> Option<&Self> {
        self.children.iter().find(|child| child.name == name)
    }

    pub(crate) fn children_named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Self> {
        self.children.iter().filter(move |child| child.name == name)
    }

    /// Texto direto de um filho, já sem espaços nas bordas.
    pub(crate) fn child_text(&self, name: &str) -> Option<&str> {
        self.child(name)
            .map(|child| child.text.trim())
            .filter(|text| !text.is_empty())
    }
}

pub(crate) fn parse(input: &str) -> Result<XmlElement, String> {
    let bytes = input.as_bytes();
    let mut index = 0;
    let mut stack: Vec<XmlElement> = Vec::new();
    let mut root: Option<XmlElement> = None;

    while index < bytes.len() {
        if bytes[index] == b'<' {
            if input[index..].starts_with("<!--") {
                index = skip_until(input, index + 4, "-->")?;
            } else if input[index..].starts_with("<![CDATA[") {
                let end = find(input, index + 9, "]]>")?;
                if let Some(current) = stack.last_mut() {
                    current.text.push_str(&input[index + 9..end]);
                }
                index = end + 3;
            } else if input[index..].starts_with("<?") {
                index = skip_until(input, index + 2, "?>")?;
            } else if input[index..].starts_with("<!") {
                index = skip_until(input, index + 2, ">")?;
            } else if input[index..].starts_with("</") {
                let end = find(input, index + 2, ">")?;
                let name = local_name(input[index + 2..end].trim());
                let Some(finished) = stack.pop() else {
                    return Err(format!("unexpected closing tag </{name}>"));
                };
                if finished.name != name {
                    return Err(format!(
                        "closing tag </{name}> does not match <{}>",
                        finished.name
                    ));
                }
                match stack.last_mut() {
                    Some(parent) => parent.children.push(finished),
                    None => root = Some(finished),
                }
                index = end + 1;
            } else {
                let end = find(input, index + 1, ">")?;
                let raw = input[index + 1..end].trim();
                let self_closing = raw.ends_with('/');
                let raw = raw.trim_end_matches('/').trim();
                let name = local_name(raw.split_whitespace().next().unwrap_or_default());
                if name.is_empty() {
                    return Err("empty element name".to_owned());
                }
                let element = XmlElement {
                    name,
                    ..XmlElement::default()
                };
                if self_closing {
                    match stack.last_mut() {
                        Some(parent) => parent.children.push(element),
                        None => root = Some(element),
                    }
                } else {
                    stack.push(element);
                }
                index = end + 1;
            }
            continue;
        }
        let next = input[index..]
            .find('<')
            .map_or(input.len(), |offset| index + offset);
        if let Some(current) = stack.last_mut() {
            current.text.push_str(&decode_entities(&input[index..next]));
        }
        index = next;
    }

    if !stack.is_empty() {
        return Err("document has unclosed elements".to_owned());
    }
    root.ok_or_else(|| "document has no root element".to_owned())
}

fn find(input: &str, from: usize, pattern: &str) -> Result<usize, String> {
    input[from..]
        .find(pattern)
        .map(|offset| from + offset)
        .ok_or_else(|| format!("unterminated `{pattern}` in document"))
}

fn skip_until(input: &str, from: usize, pattern: &str) -> Result<usize, String> {
    find(input, from, pattern).map(|end| end + pattern.len())
}

/// Descarta o prefixo de namespace, mantendo apenas o nome local.
fn local_name(raw: &str) -> String {
    raw.rsplit(':').next().unwrap_or(raw).to_owned()
}

fn decode_entities(text: &str) -> String {
    if !text.contains('&') {
        return text.to_owned();
    }
    let mut decoded = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find('&') {
        decoded.push_str(&rest[..start]);
        let tail = &rest[start..];
        let Some(end) = tail.find(';').filter(|end| *end <= 10) else {
            decoded.push('&');
            rest = &tail[1..];
            continue;
        };
        let entity = &tail[1..end];
        match entity {
            "lt" => decoded.push('<'),
            "gt" => decoded.push('>'),
            "amp" => decoded.push('&'),
            "quot" => decoded.push('"'),
            "apos" => decoded.push('\''),
            _ => {
                if let Some(code) = entity
                    .strip_prefix("#x")
                    .and_then(|value| u32::from_str_radix(value, 16).ok())
                    .or_else(|| {
                        entity
                            .strip_prefix('#')
                            .and_then(|value| value.parse().ok())
                    })
                    && let Some(character) = char::from_u32(code)
                {
                    decoded.push(character);
                } else {
                    decoded.push_str(&tail[..=end]);
                }
            }
        }
        rest = &tail[end + 1..];
    }
    decoded.push_str(rest);
    decoded
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_nested_elements_comments_cdata_and_entities() {
        let document = r#"<?xml version="1.0" encoding="UTF-8"?>
<!-- comentário <com> marcação -->
<project xmlns="http://maven.apache.org/POM/4.0.0">
  <modelVersion>4.0.0</modelVersion>
  <name>Demo &amp; Co</name>
  <description><![CDATA[usa <tags> livremente]]></description>
  <properties>
    <java.version>8</java.version>
  </properties>
  <modules>
    <module>app</module>
    <module>lib</module>
  </modules>
  <build/>
</project>"#;

        let root = match parse(document) {
            Ok(root) => root,
            Err(error) => panic!("parsing failed: {error}"),
        };
        assert_eq!(root.name, "project");
        assert_eq!(root.child_text("name"), Some("Demo & Co"));
        assert_eq!(
            root.child_text("description"),
            Some("usa <tags> livremente")
        );
        assert_eq!(
            root.child("properties")
                .and_then(|properties| properties.child_text("java.version")),
            Some("8")
        );
        let modules = match root.child("modules") {
            Some(modules) => modules,
            None => panic!("modules element is missing"),
        };
        let names: Vec<&str> = modules
            .children_named("module")
            .map(|module| module.text.trim())
            .collect();
        assert_eq!(names, vec!["app", "lib"]);
        assert!(
            root.child("build")
                .is_some_and(|build| build.children.is_empty())
        );
    }

    #[test]
    fn rejects_unbalanced_documents() {
        assert!(parse("<project><name>demo</project>").is_err());
        assert!(parse("<project>").is_err());
        assert!(parse("   ").is_err());
    }
}
