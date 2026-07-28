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

/// Chamada de método reconhecida numa expressão.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct MethodCall {
    /// Caminho até o objeto que recebe a chamada; vazio quer dizer `this`.
    pub(crate) receiver: Vec<String>,
    pub(crate) method: String,
    pub(crate) arguments: Vec<Literal>,
}

/// Argumento aceito numa chamada.
///
/// Só literais: aceitar expressões exigiria avaliá-las antes, e cada avaliação é
/// outra ida ao alvo. Literais cobrem o que se digita numa inspeção — trocar um
/// id, ligar um sinalizador, passar um nome.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Literal {
    Null,
    Bool(bool),
    Int(i32),
    Long(i64),
    Double(f64),
    Text(String),
}

/// Reconhece `caminho.metodo(arg, arg)` numa expressão.
///
/// Devolve `None` quando não é chamada — aí a expressão é tratada como caminho de
/// leitura, que continua sendo o caso comum.
pub(crate) fn parse_call(expression: &str) -> Option<MethodCall> {
    let trimmed = expression.trim().trim_end_matches(';').trim();
    let open = trimmed.find('(')?;
    if !trimmed.ends_with(')') {
        return None;
    }
    let target = trimmed[..open].trim();
    let inside = trimmed[open + 1..trimmed.len() - 1].trim();
    let mut segments: Vec<String> = target
        .split('.')
        .map(|part| part.trim().to_owned())
        .collect();
    let method = segments.pop()?;
    if !is_identifier(&method) || segments.iter().any(|segment| !is_identifier(segment)) {
        return None;
    }
    let arguments = if inside.is_empty() {
        Vec::new()
    } else {
        inside
            .split(',')
            .map(|argument| parse_literal(argument.trim()))
            .collect::<Option<Vec<_>>>()?
    };
    Some(MethodCall {
        receiver: segments,
        method,
        arguments,
    })
}

fn parse_literal(value: &str) -> Option<Literal> {
    if value == "null" {
        return Some(Literal::Null);
    }
    if value == "true" || value == "false" {
        return Some(Literal::Bool(value == "true"));
    }
    if let Some(text) = value
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
    {
        return Some(Literal::Text(text.to_owned()));
    }
    // O sufixo é o que distingue `4` de `4L` em Java, e o alvo recusa a chamada
    // quando o tipo do argumento não bate com o do parâmetro.
    if let Some(number) = value.strip_suffix(['L', 'l']) {
        return number.parse().ok().map(Literal::Long);
    }
    if let Some(number) = value.strip_suffix(['D', 'd', 'F', 'f']) {
        return number.parse().ok().map(Literal::Double);
    }
    if value.contains('.') {
        return value.parse().ok().map(Literal::Double);
    }
    value.parse().ok().map(Literal::Int)
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

    /// Uma chamada é reconhecida com receptor, nome e argumentos.
    #[test]
    fn a_call_is_told_apart_from_a_path() {
        let Some(call) = parse_call("m.setId(4L);") else {
            panic!("deveria reconhecer a chamada");
        };
        assert_eq!(call.receiver, vec!["m".to_owned()]);
        assert_eq!(call.method, "setId");
        assert_eq!(call.arguments, vec![Literal::Long(4)]);

        // Sem receptor, a chamada é sobre `this`.
        let Some(proprio) = parse_call("executar()") else {
            panic!("deveria reconhecer a chamada sem receptor");
        };
        assert!(proprio.receiver.is_empty());
        assert!(proprio.arguments.is_empty());

        // Caminho não é chamada, e continua sendo leitura.
        assert_eq!(parse_call("pedido.cliente.nome"), None);
    }

    /// O sufixo do literal decide o tipo, como em Java.
    #[test]
    fn literal_suffixes_choose_the_argument_type() {
        let Some(call) = parse_call("a.b(1, 2L, 3.5, true, null)") else {
            panic!("deveria reconhecer a chamada");
        };
        assert_eq!(
            call.arguments,
            vec![
                Literal::Int(1),
                Literal::Long(2),
                Literal::Double(3.5),
                Literal::Bool(true),
                Literal::Null,
            ]
        );
    }

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
