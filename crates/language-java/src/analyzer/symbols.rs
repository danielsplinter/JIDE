//! Normalização de nomes de símbolos Java.

pub(super) fn simple_class_name(binary_name: &str) -> String {
    binary_name
        .rsplit(['$', '.'])
        .next()
        .unwrap_or(binary_name)
        .to_owned()
}
