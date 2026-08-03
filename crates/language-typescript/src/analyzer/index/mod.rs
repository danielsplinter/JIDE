//! O índice do projeto: quais tipos existem e onde.
//!
//! É a fase 1 da `25`. Responde **uma** pergunta — "quais tipos do projeto se
//! chamam assim" —, e é a única capacidade que não depende de `import` nenhum:
//! um nome ou casa ou não casa, sem precisar saber o que está ao alcance de
//! quem. Definição, referências e o ponto são as fases seguintes, e todas pedem
//! resolução de módulos.
//!
//! # O texto e a árvore não sobrevivem ao arquivo
//!
//! Cada fonte é lido, analisado, tem as declarações extraídas — e **o texto e a
//! árvore morrem ali**. É o que o índice de Java faz, e é a diferença entre
//! guardar um índice e guardar o programa: medido, o analisador externo mantém
//! 11 287 árvores residentes e custa 1,9 GB.

mod file;

use std::path::{Path, PathBuf};

use ide_domain::{Location, SemanticSymbol, SymbolKind, TextPosition, TextRange};

use super::parser::TypeScriptParser;

/// O índice de um projeto, aberto para consulta.
pub(crate) struct WorkspaceIndex {
    aberto: file::Aberto,
}

impl WorkspaceIndex {
    /// Constrói o índice do projeto e o abre.
    ///
    /// Reconstrói sempre, por enquanto: saber o que mudou desde a última vez é a
    /// fase 4 da `20` para Java, e trazê-la para cá antes de haver o que
    /// aproveitar seria escrever invalidação sem ter uso.
    pub(crate) fn build(root: &Path, source_roots: &[PathBuf]) -> Option<Self> {
        let caminho = file::caminho_do_indice(root)?;
        let parser = TypeScriptParser::new().ok()?;
        let mut declaracoes = Vec::new();
        for fonte in fontes(root, source_roots) {
            declaracoes.extend(declaracoes_de(&parser, &fonte));
        }
        file::write(declaracoes, &caminho).ok()?;
        Self::open(root)
    }

    /// Abre um índice já gravado, sem reconstruir.
    pub(crate) fn open(root: &Path) -> Option<Self> {
        let caminho = file::caminho_do_indice(root)?;
        Some(Self {
            aberto: file::Aberto::open(&caminho)?,
        })
    }

    /// Quantos nomes distintos o índice conhece.
    #[cfg(test)]
    pub(super) fn nomes(&self) -> usize {
        self.aberto.nomes()
    }

    /// Os tipos cujo nome casa com a consulta.
    ///
    /// **A comparação é por pedaço, e não por prefixo.** Quem procura escreve
    /// `federated-login-context` ou `FederatedLoginContext` pensando no mesmo
    /// tipo, e os dois viram os mesmos três pedaços; exigir **todos** é o que
    /// impede a lista de encher com o que casa só com `context`. É a mesma regra
    /// que a `23` teve de aplicar ao analisador externo, e pela mesma razão.
    ///
    /// A varredura percorre a tabela de nomes, que está em memória e é pequena.
    /// Os registros de símbolo só saem do disco para os nomes que casaram.
    pub(crate) fn tipos(&self, query: &str, limit: usize) -> Vec<SemanticSymbol> {
        let segmentos = segmentos(query);
        let mut casaram: Vec<(u8, usize, &str, u32, u32)> = Vec::new();
        for (nome, primeiro, quantos) in self.aberto.cada_nome() {
            let normalizado = normalizado(nome);
            if !segmentos
                .iter()
                .all(|pedaco| normalizado.contains(pedaco.as_str()))
            {
                continue;
            }
            let inteira: String = segmentos.concat();
            let posicao = if normalizado == inteira {
                0
            } else if normalizado.starts_with(&inteira) {
                1
            } else if normalizado.contains(&inteira) {
                2
            } else {
                3
            };
            casaram.push((posicao, normalizado.len(), nome, primeiro, quantos));
        }
        casaram.sort_by(|esquerda, direita| {
            (esquerda.0, esquerda.1, esquerda.2).cmp(&(direita.0, direita.1, direita.2))
        });

        let mut encontrados = Vec::new();
        for (_, _, nome, primeiro, quantos) in casaram {
            for gravado in self.aberto.simbolos(primeiro, quantos) {
                encontrados.push(SemanticSymbol {
                    name: nome.to_owned(),
                    kind: gravado.kind,
                    location: Location {
                        path: gravado.arquivo,
                        range: TextRange {
                            start: TextPosition {
                                line: gravado.inicio.0,
                                column: gravado.inicio.1,
                            },
                            end: TextPosition {
                                line: gravado.fim.0,
                                column: gravado.fim.1,
                            },
                        },
                    },
                    scope_depth: 0,
                    type_descriptor: None,
                });
                if encontrados.len() >= limit {
                    return encontrados;
                }
            }
        }
        encontrados
    }
}

/// Os pedaços de uma consulta, como quem digitou os pensou.
fn segmentos(query: &str) -> Vec<String> {
    let mut pedacos = Vec::new();
    let mut atual = String::new();
    let mut anterior_minuscula = false;
    for caractere in query.chars() {
        if !caractere.is_alphanumeric() {
            if !atual.is_empty() {
                pedacos.push(std::mem::take(&mut atual));
            }
            anterior_minuscula = false;
            continue;
        }
        if caractere.is_uppercase() && anterior_minuscula && !atual.is_empty() {
            pedacos.push(std::mem::take(&mut atual));
        }
        anterior_minuscula = caractere.is_lowercase() || caractere.is_numeric();
        atual.extend(caractere.to_lowercase());
    }
    if !atual.is_empty() {
        pedacos.push(atual);
    }
    pedacos
}

fn normalizado(nome: &str) -> String {
    nome.chars()
        .filter(|caractere| caractere.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// Os `.ts` do projeto, sem descer no que não é fonte.
///
/// `node_modules` fica de fora **de propósito**: o índice responde pelos tipos
/// **do projeto**, e os das dependências são justamente o que o analisador
/// externo traz. Misturá-los aqui encheria a busca com o que não se escreve.
fn fontes(root: &Path, source_roots: &[PathBuf]) -> Vec<PathBuf> {
    let mut pilha = if source_roots.is_empty() {
        vec![root.to_path_buf()]
    } else {
        source_roots.to_vec()
    };
    let mut achados = Vec::new();
    while let Some(pasta) = pilha.pop() {
        let Ok(entradas) = std::fs::read_dir(&pasta) else {
            continue;
        };
        for entrada in entradas.flatten() {
            let caminho = entrada.path();
            let Some(nome) = caminho.file_name().and_then(|nome| nome.to_str()) else {
                continue;
            };
            if caminho.is_dir() {
                if nome != "node_modules" && nome != "dist" && !nome.starts_with('.') {
                    pilha.push(caminho);
                }
            } else if nome.ends_with(".ts") && !nome.ends_with(".d.ts") {
                achados.push(caminho);
            }
        }
    }
    achados
}

/// As declarações de um arquivo. O texto e a árvore não saem daqui.
fn declaracoes_de(parser: &TypeScriptParser, caminho: &Path) -> Vec<file::Declaracao> {
    let Ok(texto) = std::fs::read_to_string(caminho) else {
        return Vec::new();
    };
    let Ok(arvore) = parser.parse(&texto, None) else {
        return Vec::new();
    };
    let mut achadas = Vec::new();
    let mut cursor = arvore.walk();
    let mut pilha = vec![arvore.root_node()];
    while let Some(no) = pilha.pop() {
        let kind = match no.kind() {
            "class_declaration" | "abstract_class_declaration" => Some(SymbolKind::Class),
            "interface_declaration" => Some(SymbolKind::Interface),
            "enum_declaration" => Some(SymbolKind::Enum),
            // `type X = …` é um tipo como qualquer outro para quem procura um
            // nome, e classificá-lo como classe é o que a `23` já faz na
            // travessia do analisador externo.
            "type_alias_declaration" => Some(SymbolKind::Class),
            _ => None,
        };
        if let Some(kind) = kind
            && let Some(nome) = no.child_by_field_name("name")
            && let Ok(texto_do_nome) = nome.utf8_text(texto.as_bytes())
        {
            let inicio = no.start_position();
            let fim = no.end_position();
            achadas.push(file::Declaracao {
                nome: texto_do_nome.to_owned(),
                arquivo: caminho.to_path_buf(),
                kind,
                inicio: (inicio.row as u32, inicio.column as u32),
                fim: (fim.row as u32, fim.column as u32),
            });
        }
        pilha.extend(no.children(&mut cursor));
    }
    achadas
}

#[cfg(test)]
mod tests {
    use super::*;

    fn projeto(nome: &str) -> PathBuf {
        let raiz = std::env::temp_dir().join(format!("er-ts-index-{nome}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&raiz);
        assert!(std::fs::create_dir_all(raiz.join("src")).is_ok());
        raiz
    }

    /// A consulta com hífen acha o tipo escrito em `CamelCase`.
    ///
    /// É o defeito que o analisador externo teve na `23`, e o índice não pode
    /// repeti-lo: quem procura um arquivo escreve o nome dele.
    #[test]
    fn a_hyphenated_query_finds_the_camel_case_type() {
        let raiz = projeto("hifen");
        assert!(
            std::fs::write(
                raiz.join("src/a.ts"),
                "export class FederatedLoginContext {}\nexport class LoginService {}\n",
            )
            .is_ok()
        );
        let Some(indice) = WorkspaceIndex::build(&raiz, &[raiz.join("src")]) else {
            panic!("o índice precisa ser construído");
        };
        let achados = indice.tipos("federated-login-context", 20);
        let nomes: Vec<_> = achados.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(
            nomes,
            vec!["FederatedLoginContext"],
            "só o que tem os três pedaços: {nomes:?}"
        );
        let _ = std::fs::remove_dir_all(&raiz);
    }

    /// Classe, interface, enum e apelido de tipo entram todos.
    #[test]
    fn every_kind_of_type_declaration_is_indexed() {
        let raiz = projeto("especies");
        assert!(
            std::fs::write(
                raiz.join("src/a.ts"),
                "export class PedidoClasse {}\n\
                 export interface PedidoInterface {}\n\
                 export enum PedidoEnum { A }\n\
                 export type PedidoApelido = string;\n",
            )
            .is_ok()
        );
        let Some(indice) = WorkspaceIndex::build(&raiz, &[raiz.join("src")]) else {
            panic!("o índice precisa ser construído");
        };
        assert_eq!(indice.nomes(), 4, "as quatro formas de declarar um tipo");
        assert_eq!(indice.tipos("pedido", 20).len(), 4);
        let _ = std::fs::remove_dir_all(&raiz);
    }

    /// `node_modules` fica de fora.
    ///
    /// O índice responde pelos tipos **do projeto**; os das dependências são o
    /// que o analisador externo traz, e misturá-los encheria a busca com o que
    /// ninguém escreveu.
    #[test]
    fn dependencies_are_left_out() {
        let raiz = projeto("dependencias");
        assert!(std::fs::create_dir_all(raiz.join("node_modules/coisa")).is_ok());
        assert!(
            std::fs::write(
                raiz.join("node_modules/coisa/index.ts"),
                "export class TipoDeDependencia {}\n",
            )
            .is_ok()
        );
        assert!(std::fs::write(raiz.join("src/a.ts"), "export class TipoDoProjeto {}\n").is_ok());
        let Some(indice) = WorkspaceIndex::build(&raiz, &[]) else {
            panic!("o índice precisa ser construído");
        };
        let nomes: Vec<_> = indice
            .tipos("tipo", 20)
            .into_iter()
            .map(|simbolo| simbolo.name)
            .collect();
        assert_eq!(nomes, vec!["TipoDoProjeto"], "veio: {nomes:?}");
        let _ = std::fs::remove_dir_all(&raiz);
    }

    /// Um índice já gravado abre sem reconstruir.
    #[test]
    fn an_existing_index_opens_without_rebuilding() {
        let raiz = projeto("reabrir");
        assert!(std::fs::write(raiz.join("src/a.ts"), "export class Pedido {}\n").is_ok());
        assert!(WorkspaceIndex::build(&raiz, &[raiz.join("src")]).is_some());

        let Some(indice) = WorkspaceIndex::open(&raiz) else {
            panic!("o índice gravado precisa abrir sozinho");
        };
        assert_eq!(indice.tipos("Pedido", 20).len(), 1);
        let _ = std::fs::remove_dir_all(&raiz);
    }

    /// Projeto sem índice gravado não abre, e não inventa um vazio.
    #[test]
    fn a_project_without_an_index_does_not_pretend_to_have_one() {
        let raiz = projeto("sem-indice");
        assert!(WorkspaceIndex::open(&raiz).is_none());
        let _ = std::fs::remove_dir_all(&raiz);
    }
}
