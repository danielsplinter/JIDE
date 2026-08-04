//! O que um arquivo importa, reexporta e declara.
//!
//! É texto, e por isso mora aqui. Para onde cada especificador aponta é
//! conhecimento de projeto, e mora em `modules` — ver a fase 3 da `25`.
//!
//! # Por que isto não vai para o índice em disco
//!
//! `definition` precisa das importações de **um** arquivo, o que está sob o
//! cursor, e das reexportações dos poucos barris no caminho até a declaração.
//! Gravar isso para os 8 956 arquivos do projeto seria pagar a escrita inteira
//! para ler meia dúzia de registros.
//!
//! Ler e analisar um arquivo custa cerca de meio milissegundo — medido: 4,1 s
//! para 8 956 arquivos. Uma cadeia de barris toca uma dezena deles.

use std::path::Path;

use ide_domain::{SymbolKind, TextRange};

use super::lines::{LineIndex, node_range};
use super::parser::TypeScriptParser;

/// O que um arquivo diz sobre nomes que vêm de fora e vão para fora.
#[derive(Debug, Default)]
pub(crate) struct Referencias {
    /// O que este arquivo traz de fora.
    pub(crate) importados: Vec<Trazido>,
    /// O que este arquivo repassa para fora.
    pub(crate) reexportados: Vec<Trazido>,
    /// Declarações de tipo deste arquivo, com onde estão.
    pub(crate) declarados: Vec<(String, SymbolKind, TextRange)>,
}

/// Um nome que atravessa a fronteira de um módulo.
///
/// # Os dois nomes não são o mesmo, e é isso que se perde sem esta estrutura
///
/// `import { Pedido as PedidoAntigo }` põe **dois** nomes em jogo: o que o
/// arquivo usa e o que o módulo de origem declara. Procurar o nome usado no
/// arquivo de destino não acha nada — ele nunca ouviu falar dele —, e a
/// navegação pareceria um limite do índice quando é só troca de nome.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Trazido {
    /// O nome como este arquivo o usa. `None` em `export * from`, que não nomeia
    /// ninguém e vale para qualquer nome.
    pub(crate) usado: Option<String>,
    /// O nome como o módulo de origem o declara.
    pub(crate) origem: Option<String>,
    /// De que módulo ele vem.
    pub(crate) de: String,
}

impl Referencias {
    /// De onde vem um nome, se ele for importado: o nome lá e o módulo.
    pub(crate) fn origem(&self, nome: &str) -> Option<(String, &str)> {
        self.importados
            .iter()
            .find(|trazido| trazido.usado.as_deref() == Some(nome))
            .map(|trazido| {
                (
                    trazido.origem.clone().unwrap_or_else(|| nome.to_owned()),
                    trazido.de.as_str(),
                )
            })
    }

    /// Onde este arquivo declara um nome, se declarar.
    pub(crate) fn declaracao(&self, nome: &str) -> Option<TextRange> {
        self.declarados
            .iter()
            .find(|(declarado, _, _)| declarado == nome)
            .map(|(_, _, range)| *range)
    }
}

/// Lê um arquivo e extrai o que ele importa, reexporta e declara.
pub(crate) fn de_arquivo(parser: &TypeScriptParser, caminho: &Path) -> Referencias {
    let Ok(texto) = std::fs::read_to_string(caminho) else {
        return Referencias::default();
    };
    do_texto(parser, &texto)
}

/// O mesmo, para o texto que o editor tem — que pode não ser o do disco.
pub(crate) fn do_texto(parser: &TypeScriptParser, texto: &str) -> Referencias {
    let Ok(arvore) = parser.parse(texto, None) else {
        return Referencias::default();
    };
    na_arvore(&arvore, texto)
}

/// O mesmo, sobre uma árvore que já existe.
///
/// # Por que existe separado de [`do_texto`]
///
/// Quem procura referências precisa das duas coisas do mesmo arquivo: de onde o
/// nome vem, e onde ele aparece. Cada uma analisava o texto por conta, e o
/// arquivo era percorrido **duas vezes**.
///
/// *Medido: 7,0 s para 6,8 s no monorepo de referência — **3%**, e não a metade
/// que a segunda análise parecia valer. O que sobra está em outro lugar, e o
/// registro fica aqui para a próxima previsão não repetir o erro.*
pub(crate) fn na_arvore(arvore: &tree_sitter::Tree, texto: &str) -> Referencias {
    let bytes = texto.as_bytes();
    let mut referencias = Referencias::default();
    let mut cursor = arvore.walk();
    let mut pilha = vec![arvore.root_node()];
    while let Some(no) = pilha.pop() {
        match no.kind() {
            "import_statement" => colher_import(no, bytes, &mut referencias),
            // Um `export … from` é reexportação; um `export` sem `from` é
            // declaração, e cai na travessia normal abaixo.
            "export_statement" if no.child_by_field_name("source").is_some() => {
                colher_reexport(no, bytes, &mut referencias);
            }
            _ => colher_declaracao(no, bytes, &mut referencias),
        }
        pilha.extend(no.children(&mut cursor));
    }
    referencias
}

fn especificador(no: tree_sitter::Node, bytes: &[u8]) -> Option<String> {
    let fonte = no.child_by_field_name("source")?;
    let cru = fonte.utf8_text(bytes).ok()?;
    // O nó da string inclui as aspas; o especificador é o miolo.
    Some(cru.trim_matches(['\'', '"', '`']).to_owned())
}

fn colher_import(no: tree_sitter::Node, bytes: &[u8], para: &mut Referencias) {
    let Some(de) = especificador(no, bytes) else {
        return;
    };
    let mut cursor = no.walk();
    let mut pilha = vec![no];
    while let Some(atual) = pilha.pop() {
        if atual.kind() == "import_specifier" {
            // `import { A as B }` traz os dois: o que vale aqui é o **alias**,
            // porque é o nome que o arquivo usa. Sem `alias`, é o próprio nome.
            let usado = atual
                .child_by_field_name("alias")
                .or_else(|| atual.child_by_field_name("name"));
            if let Some(usado) = usado
                && let Ok(nome) = usado.utf8_text(bytes)
            {
                let origem = atual
                    .child_by_field_name("name")
                    .and_then(|no| no.utf8_text(bytes).ok())
                    .map(str::to_owned);
                para.importados.push(Trazido {
                    usado: Some(nome.to_owned()),
                    origem,
                    de: de.clone(),
                });
            }
        }
        // `import Padrao from './x'` e `import * as N from './x'`.
        if matches!(atual.kind(), "identifier" | "namespace_import")
            && atual.parent().is_some_and(|pai| pai.kind() == "import_clause")
            && let Ok(nome) = atual.utf8_text(bytes)
        {
            let nome = nome.trim_start_matches("* as ").trim().to_owned();
            para.importados.push(Trazido {
                usado: Some(nome.clone()),
                origem: Some(nome),
                de: de.clone(),
            });
        }
        pilha.extend(atual.children(&mut cursor));
    }
}

fn colher_reexport(no: tree_sitter::Node, bytes: &[u8], para: &mut Referencias) {
    let Some(de) = especificador(no, bytes) else {
        return;
    };
    let mut cursor = no.walk();
    let mut pilha = vec![no];
    let mut nominal = false;
    while let Some(atual) = pilha.pop() {
        if atual.kind() == "export_specifier" {
            nominal = true;
            let exposto = atual
                .child_by_field_name("alias")
                .or_else(|| atual.child_by_field_name("name"));
            if let Some(exposto) = exposto
                && let Ok(nome) = exposto.utf8_text(bytes)
            {
                let origem = atual
                    .child_by_field_name("name")
                    .and_then(|no| no.utf8_text(bytes).ok())
                    .map(str::to_owned);
                para.reexportados.push(Trazido {
                    usado: Some(nome.to_owned()),
                    origem,
                    de: de.clone(),
                });
            }
        }
        pilha.extend(atual.children(&mut cursor));
    }
    // `export * from './x'` não nomeia ninguém: ele expõe tudo o que vier de lá.
    if !nominal {
        para.reexportados.push(Trazido {
            usado: None,
            origem: None,
            de,
        });
    }
}

/// Registra o que este arquivo declara, por nome.
///
/// # Tipo não basta, e a comparação com o analisador foi quem disse
///
/// O índice em disco guarda **tipos**, porque a pergunta dele é "ir para o
/// tipo". Mas `Ctrl+clique` cai sobre qualquer nome: `appConfig`, `routes`, uma
/// função utilitária. Conferido contra o analisador num projeto real, 79 de 94
/// definições divergiam — e todas pelo mesmo motivo: nós devíamos `None` para
/// nome que não fosse tipo.
///
/// Isto não custa nada no arquivo do índice: as declarações são extraídas sob
/// demanda, do arquivo que a pergunta alcança.
fn colher_declaracao(no: tree_sitter::Node, bytes: &[u8], para: &mut Referencias) {
    let kind = match no.kind() {
        "class_declaration" | "abstract_class_declaration" => SymbolKind::Class,
        "interface_declaration" => SymbolKind::Interface,
        "enum_declaration" => SymbolKind::Enum,
        "type_alias_declaration" => SymbolKind::Class,
        "function_declaration" | "generator_function_declaration" => SymbolKind::Method,
        // **Método de classe conta.** `Ctrl+clique` em `this.buscar()` cai sobre
        // um nome que está declarado logo acima, no mesmo arquivo — e sem
        // registrá-lo o índice dizia que não alcançava, e a pergunta ia acordar
        // um analisador de 1,9 GB para responder o que estava na tela.
        "method_definition" | "method_signature" => SymbolKind::Method,
        // `const x = …` e `let x = …` no nível do módulo: são o que um
        // `export const` expõe, e Angular os usa para configuração e rotas.
        "variable_declarator" if no_nivel_do_modulo(no) => SymbolKind::Field,
        _ => return,
    };
    let Some(nome) = no.child_by_field_name("name") else {
        return;
    };
    // Só identificador simples: um `const { a, b } = …` desestrutura, e o nome
    // que se vê ali não é o que o módulo exporta.
    if kind == SymbolKind::Field && nome.kind() != "identifier" {
        return;
    }
    let Ok(texto) = nome.utf8_text(bytes) else {
        return;
    };
    let inicio = nome.start_position();
    let fim = nome.end_position();
    para.declarados.push((
        texto.to_owned(),
        kind,
        TextRange {
            start: ide_domain::TextPosition {
                line: inicio.row as u32,
                column: inicio.column as u32,
            },
            end: ide_domain::TextPosition {
                line: fim.row as u32,
                column: fim.column as u32,
            },
        },
    ));
}

/// Se um `const`/`let` está no nível do módulo, e não dentro de uma função.
///
/// Variável local não é destino de navegação entre arquivos, e registrá-la faria
/// a busca por nome achar a primeira homônima de qualquer bloco.
fn no_nivel_do_modulo(no: tree_sitter::Node) -> bool {
    let mut atual = no.parent();
    while let Some(pai) = atual {
        match pai.kind() {
            "program" => return true,
            "statement_block" | "function_declaration" | "arrow_function" | "method_definition"
            | "class_body" => return false,
            _ => atual = pai.parent(),
        }
    }
    false
}

/// A importação sob uma posição: o nome no destino e de que módulo ele vem.
///
/// # Por que procurar por texto não basta
///
/// Em `import { login as fetchingToken } from './utils/login'`, o cursor sobre
/// `login` está sobre o nome de **origem** — e `login` não é um nome que este
/// arquivo use. Procurar "de onde vem `login`" pela lista de importados acharia
/// **outro** `login`, importado de outro módulo na mesma tela, e abriria o
/// arquivo errado com a mesma cara de certo.
///
/// Foi assim que a comparação com o analisador achou a última divergência num
/// projeto real: 93 definições iguais e uma diferente, exatamente esta.
pub(crate) fn importacao_em(
    parser: &TypeScriptParser,
    texto: &str,
    linha: u32,
    coluna: u32,
) -> Option<(String, String)> {
    let arvore = parser.parse(texto, None).ok()?;
    let ponto = tree_sitter::Point {
        row: linha as usize,
        column: coluna as usize,
    };
    let mut no = arvore
        .root_node()
        .named_descendant_for_point_range(ponto, ponto)?;
    // Sobe até o especificador, e dele até a declaração que traz o módulo.
    while no.kind() != "import_specifier" {
        no = no.parent()?;
        if no.kind() == "program" {
            return None;
        }
    }
    let nome = no.child_by_field_name("name")?.utf8_text(texto.as_bytes()).ok()?;
    let mut declaracao = no.parent()?;
    while declaracao.kind() != "import_statement" {
        declaracao = declaracao.parent()?;
    }
    let de = especificador(declaracao, texto.as_bytes())?;
    Some((nome.to_owned(), de))
}

/// O identificador que está sob uma posição, se houver.
///
/// É o que transforma "onde o cursor está" em "que nome se quer". Sem isto,
/// `definition` teria de receber o nome de quem chama — e quem chama é a tela,
/// que não sabe o que é identificador em TypeScript.
pub(crate) fn identificador_em(
    parser: &TypeScriptParser,
    texto: &str,
    linha: u32,
    coluna: u32,
) -> Option<String> {
    let arvore = parser.parse(texto, None).ok()?;
    let ponto = tree_sitter::Point {
        row: linha as usize,
        column: coluna as usize,
    };
    let no = arvore
        .root_node()
        .named_descendant_for_point_range(ponto, ponto)?;
    if !matches!(
        no.kind(),
        "identifier" | "type_identifier" | "property_identifier" | "shorthand_property_identifier"
    ) {
        return None;
    }
    no.utf8_text(texto.as_bytes()).ok().map(str::to_owned)
}

/// O conteúdo do texto literal sob o cursor, se houver um.
///
/// # Por que isto é separado do identificador
///
/// Dentro de aspas não há identificador, e o nó é `string` ou `string_fragment`.
/// Quem procurava só identificador achava nada — e "nada" era respondido como
/// **lista vazia**, que afirma que a posição não tem destino. É a mesma família
/// de erro que a `25` nomeou: dizer "não existe" quando o certo era "não sei".
///
/// Serve a qualquer literal de caminho, e não só ao `templateUrl`: um
/// `styleUrls`, um `import('./x')` ou um caminho escrito à mão respondem pela
/// mesma regra.
pub(crate) fn texto_literal_em(
    parser: &TypeScriptParser,
    texto: &str,
    linha: u32,
    coluna: u32,
) -> Option<String> {
    let arvore = parser.parse(texto, None).ok()?;
    let ponto = tree_sitter::Point {
        row: linha as usize,
        column: coluna as usize,
    };
    let mut no = arvore
        .root_node()
        .named_descendant_for_point_range(ponto, ponto)?;
    // O cursor pode cair no fragmento ou na string inteira, dependendo de onde
    // ele está entre as aspas.
    if no.kind() == "string" && let Some(dentro) = no.named_child(0) {
        no = dentro;
    }
    if !matches!(no.kind(), "string_fragment") {
        return None;
    }
    no.utf8_text(texto.as_bytes()).ok().map(str::to_owned)
}

/// Onde um identificador aparece numa árvore já analisada.
///
/// **Só identificador**, e não texto solto: `Pedido` dentro de um comentário ou
/// de uma string não é uso, é menção. É o que separa isto de uma busca por
/// texto — e a busca por texto é o que a IDE já tinha.
///
/// Havia uma irmã desta que analisava o texto antes de contar, e ela ficou sem
/// chamador quando a busca por usos passou a compartilhar a árvore que o realce
/// já tem. Foi removida: uma função morta que reanalisa é um convite a desfazer
/// a economia que acabou de ser feita.
pub(crate) fn ocorrencias_na_arvore(
    arvore: &tree_sitter::Tree,
    texto: &str,
    nome: &str,
) -> Vec<TextRange> {
    let linhas = LineIndex::new(texto);
    let mut achadas = Vec::new();
    let mut pilha = vec![arvore.root_node()];
    while let Some(no) = pilha.pop() {
        if matches!(
            no.kind(),
            "identifier" | "type_identifier" | "property_identifier" | "shorthand_property_identifier"
        ) && no.utf8_text(texto.as_bytes()).is_ok_and(|texto| texto == nome)
        {
            achadas.push(node_range(no, &linhas));
        }
        let mut cursor = no.walk();
        pilha.extend(no.children(&mut cursor));
    }
    achadas.sort_by_key(|range| (range.start.line, range.start.column));
    achadas
}

#[cfg(test)]
mod tests {
    use super::*;

    fn analisar(texto: &str) -> Referencias {
        let Ok(parser) = TypeScriptParser::new() else {
            panic!("a gramática precisa carregar");
        };
        do_texto(&parser, texto)
    }

    /// Um `import` nomeado diz de onde cada nome vem.
    #[test]
    fn a_named_import_says_where_each_name_comes_from() {
        let referencias = analisar("import { Pedido, Resumo } from './modelo';\n");
        assert_eq!(
            referencias.origem("Pedido"),
            Some(("Pedido".to_owned(), "./modelo"))
        );
        assert_eq!(
            referencias.origem("Resumo"),
            Some(("Resumo".to_owned(), "./modelo"))
        );
        assert_eq!(referencias.origem("Outro"), None);
    }

    /// `import { A as B }` registra o nome que o arquivo **usa**.
    ///
    /// Quem está sob o cursor é `B`; procurar por `A` no arquivo de destino é o
    /// passo seguinte, e confundir os dois abriria a declaração errada.
    #[test]
    fn a_renamed_import_registers_the_name_in_use() {
        let referencias = analisar("import { Pedido as PedidoAntigo } from './modelo';\n");
        assert_eq!(
            referencias.origem("PedidoAntigo"),
            Some(("Pedido".to_owned(), "./modelo")),
            "o arquivo usa `PedidoAntigo`, e o módulo de origem declara `Pedido`"
        );
    }

    /// `export * from` não nomeia ninguém, e vale para qualquer nome.
    #[test]
    fn a_star_re_export_names_nobody() {
        let referencias = analisar("export * from './modelo';\n");
        assert_eq!(
            referencias.reexportados,
            vec![Trazido {
                usado: None,
                origem: None,
                de: "./modelo".to_owned()
            }]
        );
    }

    /// `export { A } from` vale só para `A`.
    #[test]
    fn a_named_re_export_is_only_for_its_name() {
        let referencias = analisar("export { Pedido } from './modelo';\n");
        assert_eq!(
            referencias.reexportados,
            vec![Trazido {
                usado: Some("Pedido".to_owned()),
                origem: Some("Pedido".to_owned()),
                de: "./modelo".to_owned()
            }]
        );
    }

    /// `export class` é declaração, e não reexportação.
    ///
    /// Os dois começam com `export`, e tratá-los igual faria a busca sair do
    /// arquivo que já tem a resposta.
    #[test]
    fn an_exported_declaration_is_not_a_re_export() {
        let referencias = analisar("export class Pedido {}\n");
        assert!(referencias.reexportados.is_empty());
        assert!(referencias.declaracao("Pedido").is_some());
    }

    /// A declaração aponta para o **nome**, e não para a classe inteira.
    ///
    /// Abrir o arquivo com o cursor no `class` da linha e não no nome faria a
    /// navegação parecer imprecisa sem estar errada.
    #[test]
    fn the_declaration_points_at_the_name() {
        let referencias = analisar("export class Pedido {}\n");
        let Some(range) = referencias.declaracao("Pedido") else {
            panic!("a declaração precisa ser achada");
        };
        assert_eq!(range.start.line, 0);
        assert_eq!(range.start.column, "export class ".len() as u32);
    }

    /// Um método de classe é declaração como qualquer outra.
    #[test]
    fn a_class_method_is_a_declaration() {
        let referencias = analisar("export class Pagina {
  buscar() {}
}
");
        let nomes: Vec<_> = referencias
            .declarados
            .iter()
            .map(|(nome, _, _)| nome.as_str())
            .collect();
        assert!(nomes.contains(&"buscar"), "veio: {nomes:?}");
        assert!(referencias.declaracao("buscar").is_some());
    }

    /// O identificador sob o cursor é o que se procura.
    #[test]
    fn the_identifier_under_the_cursor_is_what_is_wanted() {
        let Ok(parser) = TypeScriptParser::new() else {
            panic!("a gramática precisa carregar");
        };
        let texto = "import { Pedido } from './modelo';\n";
        assert_eq!(
            identificador_em(&parser, texto, 0, 10).as_deref(),
            Some("Pedido")
        );
        // Sobre a pontuação não há identificador nenhum, e inventar um seria
        // responder a uma pergunta que ninguém fez.
        assert_eq!(identificador_em(&parser, texto, 0, 7), None);
    }
}

