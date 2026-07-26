//! Apresentação de valores e leitura de expressões de inspeção.

use crate::wire::Value;

/// Nome legível de um tipo a partir da assinatura JNI.
#[must_use]
pub(crate) fn type_name(signature: &str) -> String {
    let mut dimensions = 0;
    let mut rest = signature;
    while let Some(inner) = rest.strip_prefix('[') {
        dimensions += 1;
        rest = inner;
    }
    let base = match rest.chars().next() {
        Some('Z') => "boolean".to_owned(),
        Some('B') => "byte".to_owned(),
        Some('C') => "char".to_owned(),
        Some('S') => "short".to_owned(),
        Some('I') => "int".to_owned(),
        Some('J') => "long".to_owned(),
        Some('F') => "float".to_owned(),
        Some('D') => "double".to_owned(),
        Some('V') => "void".to_owned(),
        Some('L') => rest
            .trim_start_matches('L')
            .trim_end_matches(';')
            .rsplit('/')
            .next()
            .unwrap_or(rest)
            .to_owned(),
        _ => rest.to_owned(),
    };
    format!("{base}{}", "[]".repeat(dimensions))
}

/// Texto de um valor primitivo. Objetos precisam de consulta ao alvo.
#[must_use]
pub(crate) fn format_primitive(value: &Value) -> Option<String> {
    Some(match value {
        Value::Bool(value) => value.to_string(),
        Value::Byte(value) => value.to_string(),
        Value::Char(value) => char::from_u32(u32::from(*value))
            .map_or_else(|| format!("\\u{value:04x}"), |value| format!("'{value}'")),
        Value::Short(value) => value.to_string(),
        Value::Int(value) => value.to_string(),
        Value::Long(value) => value.to_string(),
        Value::Float(value) => value.to_string(),
        Value::Double(value) => value.to_string(),
        Value::Void => "void".to_owned(),
        Value::Object { .. } => return None,
    })
}

#[must_use]
pub(crate) fn is_null(value: &Value) -> bool {
    matches!(value, Value::Object { id: 0, .. })
}

#[must_use]
pub(crate) fn is_string(value: &Value) -> bool {
    matches!(value, Value::Object { tag: b's', id } if *id != 0)
}

#[must_use]
pub(crate) fn is_array(value: &Value) -> bool {
    matches!(value, Value::Object { tag: b'[', id } if *id != 0)
}

/// Caminho de inspeção aceito: `this`, um nome local, ou uma cadeia de campos.
///
/// Invocar métodos e avaliar operadores fica de fora de propósito: executar
/// código no alvo muda o estado do programa depurado, e isso precisa ser uma
/// decisão explícita, não um efeito colateral de olhar uma variável.
#[must_use]
pub(crate) fn parse_path(expression: &str) -> Option<Vec<String>> {
    let trimmed = expression.trim();
    if trimmed.is_empty() {
        return None;
    }
    let segments: Vec<String> = trimmed
        .split('.')
        .map(|part| part.trim().to_owned())
        .collect();
    if segments.iter().any(|segment| !is_identifier(segment)) {
        return None;
    }
    Some(segments)
}

fn is_identifier(value: &str) -> bool {
    let mut characters = value.chars();
    characters
        .next()
        .is_some_and(|first| first.is_alphabetic() || first == '_' || first == '$')
        && characters
            .all(|character| character.is_alphanumeric() || character == '_' || character == '$')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signatures_become_readable_type_names() {
        assert_eq!(type_name("Ljava/lang/String;"), "String");
        assert_eq!(type_name("I"), "int");
        assert_eq!(type_name("[[I"), "int[][]");
        assert_eq!(type_name("[Ljava/util/List;"), "List[]");
    }

    #[test]
    fn primitives_are_formatted_and_objects_are_left_to_the_target() {
        assert_eq!(format_primitive(&Value::Int(7)).as_deref(), Some("7"));
        assert_eq!(
            format_primitive(&Value::Bool(false)).as_deref(),
            Some("false")
        );
        assert_eq!(format_primitive(&Value::Char(65)).as_deref(), Some("'A'"));
        assert!(format_primitive(&Value::Object { tag: b'L', id: 3 }).is_none());
        assert!(is_null(&Value::Object { tag: b'L', id: 0 }));
        assert!(is_string(&Value::Object { tag: b's', id: 3 }));
        assert!(is_array(&Value::Object { tag: b'[', id: 3 }));
    }

    #[test]
    fn inspection_paths_accept_fields_and_reject_code() {
        assert_eq!(parse_path("total"), Some(vec!["total".to_owned()]));
        assert_eq!(
            parse_path("this.order.id"),
            Some(vec!["this".to_owned(), "order".to_owned(), "id".to_owned()])
        );
        assert!(parse_path("list.size()").is_none());
        assert!(parse_path("a + b").is_none());
        assert!(parse_path("items[0]").is_none());
        assert!(parse_path("  ").is_none());
        assert!(parse_path("2fast").is_none());
    }
}
