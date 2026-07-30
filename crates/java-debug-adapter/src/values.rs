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

/// Argumento já ajustado ao tipo que o parâmetro declara.
///
/// Só `Primitive` pode ir direto para o alvo; as demais formas precisam de uma
/// ida ao processo depurado para existirem como objeto lá dentro.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Argument {
    Primitive(Value),
    /// Embrulhar o primitivo chamando `valueOf` da classe indicada.
    Boxed {
        class: &'static str,
        value_of: &'static str,
        value: Value,
    },
    /// Criar uma `String` no alvo.
    Text(String),
    Null,
}

/// Quanto o literal digitado combina com o parâmetro declarado, do mais direto
/// ao que exige mais conversão.
const EXACT: u32 = 4;
/// O parâmetro nomeia a classe de embrulho do próprio literal.
const BOXED: u32 = 3;
/// O parâmetro aceita qualquer objeto, e o embrulho é escolhido pelo literal.
const OPEN_FIT: u32 = 2;
/// O número precisou mudar de largura ou de forma para caber.
const WIDENED: u32 = 1;

/// Classes de embrulho: assinatura, etiqueta do primitivo e o `valueOf` que a
/// produz.
const BOXES: &[(&str, u8, &str)] = &[
    ("Ljava/lang/Boolean;", b'Z', "(Z)Ljava/lang/Boolean;"),
    ("Ljava/lang/Byte;", b'B', "(B)Ljava/lang/Byte;"),
    ("Ljava/lang/Character;", b'C', "(C)Ljava/lang/Character;"),
    ("Ljava/lang/Short;", b'S', "(S)Ljava/lang/Short;"),
    ("Ljava/lang/Integer;", b'I', "(I)Ljava/lang/Integer;"),
    ("Ljava/lang/Long;", b'J', "(J)Ljava/lang/Long;"),
    ("Ljava/lang/Float;", b'F', "(F)Ljava/lang/Float;"),
    ("Ljava/lang/Double;", b'D', "(D)Ljava/lang/Double;"),
];

/// Parâmetros que aceitam qualquer objeto, e por isso recebem o embrulho
/// natural do literal.
const OPEN: &[&str] = &[
    "Ljava/lang/Object;",
    "Ljava/io/Serializable;",
    "Ljava/lang/Comparable;",
];

/// Ajusta o literal ao parâmetro, ou recusa.
///
/// Recusar é o ponto: o alvo não confere o tipo do argumento antes de repassá-lo
/// à chamada, então um `long` no lugar de um `java.lang.Long` vira um endereço
/// inválido e mata o processo depurado. Entre errar o tipo e não chamar, não
/// chamar é o único desfecho aceitável.
///
/// O número devolvido diz o quanto o literal serve, e é o que separa
/// `setId(long)` de `setId(Long)` quando as duas sobrecargas existem.
#[must_use]
pub(crate) fn coerce(literal: &Literal, parameter: &str) -> Option<(Argument, u32)> {
    let reference = parameter.starts_with('L') || parameter.starts_with('[');
    match literal {
        Literal::Null => reference.then_some((Argument::Null, EXACT)),
        Literal::Text(text) => {
            if parameter == "Ljava/lang/String;" {
                Some((Argument::Text(text.clone()), EXACT))
            } else if OPEN.contains(&parameter) || parameter == "Ljava/lang/CharSequence;" {
                Some((Argument::Text(text.clone()), WIDENED))
            } else {
                None
            }
        }
        _ if reference => {
            let (declared, (class, tag, value_of)) = box_for(literal, parameter)?;
            let value = as_primitive(literal, tag)?;
            let fit = match (natural_tag(literal) == Some(tag), declared) {
                (false, _) => WIDENED,
                (true, true) => BOXED,
                (true, false) => OPEN_FIT,
            };
            Some((
                Argument::Boxed {
                    class,
                    value_of,
                    value,
                },
                fit,
            ))
        }
        _ => {
            let tag = *parameter.as_bytes().first()?;
            let value = as_primitive(literal, tag)?;
            let fit = if natural_tag(literal) == Some(tag) {
                EXACT
            } else {
                WIDENED
            };
            Some((Argument::Primitive(value), fit))
        }
    }
}

/// Assinaturas dos parâmetros de uma assinatura JNI de método.
#[must_use]
pub(crate) fn parameter_signatures(signature: &str) -> Vec<String> {
    let Some(inside) = signature
        .strip_prefix('(')
        .and_then(|rest| rest.split(')').next())
    else {
        return Vec::new();
    };
    let mut parameters = Vec::new();
    let mut rest = inside;
    while !rest.is_empty() {
        let arrays = rest.len() - rest.trim_start_matches('[').len();
        let body = &rest[arrays..];
        let consumed = if body.starts_with('L') {
            body.find(';').map_or(body.len(), |index| index + 1)
        } else {
            1
        };
        let end = (arrays + consumed).min(rest.len());
        parameters.push(rest[..end].to_owned());
        rest = &rest[end..];
    }
    parameters
}

/// A classe de embrulho vem do parâmetro quando ele já é uma; quando o
/// parâmetro aceita qualquer objeto, vem do literal.
/// O sinalizador diz se a classe veio declarada no parâmetro; escolhida pelo
/// literal, ela vale menos na comparação entre sobrecargas.
fn box_for(literal: &Literal, parameter: &str) -> Option<(bool, (&'static str, u8, &'static str))> {
    if let Some(box_type) = BOXES.iter().find(|(class, ..)| *class == parameter) {
        return Some((true, *box_type));
    }
    let open = OPEN.contains(&parameter) || parameter == "Ljava/lang/Number;";
    if !open {
        return None;
    }
    let tag = natural_tag(literal)?;
    if parameter == "Ljava/lang/Number;" && tag == b'Z' {
        return None;
    }
    BOXES
        .iter()
        .find(|(_, box_tag, _)| *box_tag == tag)
        .map(|box_type| (false, *box_type))
}

/// O tipo que o literal tem por si só, antes de olhar o parâmetro.
fn natural_tag(literal: &Literal) -> Option<u8> {
    Some(match literal {
        Literal::Bool(_) => b'Z',
        Literal::Int(_) => b'I',
        Literal::Long(_) => b'J',
        Literal::Double(_) => b'D',
        Literal::Null | Literal::Text(_) => return None,
    })
}

/// Converte o literal no primitivo pedido, quando isso não perde informação.
fn as_primitive(literal: &Literal, tag: u8) -> Option<Value> {
    let number = match literal {
        Literal::Bool(value) => return (tag == b'Z').then_some(Value::Bool(*value)),
        Literal::Int(value) => i64::from(*value),
        Literal::Long(value) => *value,
        Literal::Double(value) => {
            return match tag {
                b'D' => Some(Value::Double(*value)),
                b'F' => Some(Value::Float(*value as f32)),
                _ => None,
            };
        }
        Literal::Null | Literal::Text(_) => return None,
    };
    Some(match tag {
        b'B' => Value::Byte(i8::try_from(number).ok()?),
        b'C' => Value::Char(u16::try_from(number).ok()?),
        b'S' => Value::Short(i16::try_from(number).ok()?),
        b'I' => Value::Int(i32::try_from(number).ok()?),
        b'J' => Value::Long(number),
        b'F' => Value::Float(number as f32),
        b'D' => Value::Double(number as f64),
        _ => return None,
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

    /// O caso que derrubava a JVM: `setId(4L)` num campo `java.lang.Long`.
    ///
    /// Enviar o `long` cru com a etiqueta `J` fazia o alvo ler o número como
    /// endereço de objeto e matar o processo depurado. O literal precisa virar
    /// um embrulho antes de sair.
    #[test]
    fn a_number_going_into_a_boxed_parameter_is_wrapped_instead_of_sent_raw() {
        let Some((argument, _)) = coerce(&Literal::Long(4), "Ljava/lang/Long;") else {
            panic!("deveria aceitar o literal");
        };
        assert_eq!(
            argument,
            Argument::Boxed {
                class: "Ljava/lang/Long;",
                value_of: "(J)Ljava/lang/Long;",
                value: Value::Long(4),
            }
        );

        // O mesmo literal num parâmetro primitivo vai direto.
        assert_eq!(
            coerce(&Literal::Long(4), "J").map(|(argument, _)| argument),
            Some(Argument::Primitive(Value::Long(4)))
        );
    }

    /// Entre `setId(long)` e `setId(Long)`, `4L` escolhe a primitiva.
    #[test]
    fn the_closest_overload_scores_higher() {
        let Some((_, primitiva)) = coerce(&Literal::Long(4), "J") else {
            panic!("deveria aceitar o primitivo");
        };
        let Some((_, embrulhada)) = coerce(&Literal::Long(4), "Ljava/lang/Long;") else {
            panic!("deveria aceitar o embrulho");
        };
        let Some((_, aberta)) = coerce(&Literal::Long(4), "Ljava/lang/Object;") else {
            panic!("deveria aceitar o parâmetro aberto");
        };
        assert!(
            primitiva > embrulhada && embrulhada > aberta,
            "quanto menos conversão, melhor: {primitiva} > {embrulhada} > {aberta}"
        );
    }

    /// O que não cabe é recusado, e não enviado com o tipo errado.
    #[test]
    fn a_literal_that_does_not_fit_the_parameter_is_refused() {
        assert!(coerce(&Literal::Bool(true), "Ljava/lang/Long;").is_none());
        assert!(coerce(&Literal::Long(4), "Ljava/util/List;").is_none());
        assert!(coerce(&Literal::Text("a".to_owned()), "I").is_none());
        assert!(
            coerce(&Literal::Null, "I").is_none(),
            "null não é primitivo"
        );
        // Estreitar um número só passa quando ele cabe.
        assert!(coerce(&Literal::Int(300), "B").is_none());
        assert_eq!(
            coerce(&Literal::Int(7), "B").map(|(argument, _)| argument),
            Some(Argument::Primitive(Value::Byte(7)))
        );
    }

    #[test]
    fn text_becomes_a_string_created_in_the_target() {
        assert_eq!(
            coerce(&Literal::Text("oi".to_owned()), "Ljava/lang/String;")
                .map(|(argument, _)| argument),
            Some(Argument::Text("oi".to_owned()))
        );
        assert_eq!(
            coerce(&Literal::Null, "Ljava/lang/String;").map(|(argument, _)| argument),
            Some(Argument::Null)
        );
    }

    #[test]
    fn parameters_are_read_one_by_one_from_the_signature() {
        assert_eq!(
            parameter_signatures("(JLjava/lang/String;[IZ)V"),
            vec!["J", "Ljava/lang/String;", "[I", "Z"]
        );
        assert!(parameter_signatures("()V").is_empty());
        assert_eq!(
            parameter_signatures("([[Ljava/lang/Object;)I"),
            vec!["[[Ljava/lang/Object;"]
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
