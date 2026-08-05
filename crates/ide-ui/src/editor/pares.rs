//! Os pares que se fecham sozinhos, e as decisões que isso exige.
//!
//! Este módulo não toca no texto: ele só **decide**. Quem escreve é o painel,
//! que é o dono do cursor e do buffer — e separar as duas coisas é o que faz
//! cada regra caber num teste que não precisa de janela nem de documento aberto.
//!
//! Aspas são **simétricas** — o mesmo caractere abre e fecha —, e por isso vivem
//! numa lista à parte e decidem numa ordem própria: primeiro se passam por cima,
//! só depois se abrem. Um par de chaves nunca precisa dessa dúvida.

use std::cmp::Reverse;
use std::collections::HashMap;
use std::ops::Range;

/// Os pares que a IDE fecha.
const PARES: [(char, char); 3] = [('(', ')'), ('{', '}'), ('[', ']')];

/// As aspas, que são o mesmo caractere dos dois lados.
const ASPAS: [char; 3] = ['\'', '"', '`'];

/// O fechamento de um abridor.
///
/// **Só os pares de verdade**, e não as aspas. Quem pergunta isto é também o
/// `Enter`, e uma aspa não abre bloco nenhum: `"` seguido de `Enter` partiria a
/// string no meio.
pub(super) fn fechamento_de(caractere: char) -> Option<char> {
    PARES
        .iter()
        .find(|(abre, _)| *abre == caractere)
        .map(|(_, fecha)| *fecha)
}

/// Se o caractere fecha algum par.
fn e_fechamento(caractere: char) -> bool {
    PARES.iter().any(|(_, fecha)| *fecha == caractere)
}

/// Se o caractere que vem antes do cursor faz parte de uma palavra.
///
/// É o que separa `don't` de uma string que começa. Sem isto, escrever um
/// apóstrofo no meio de uma palavra — num comentário, numa mensagem — devolveria
/// `don''t`, e é o defeito que faz alguém desligar a conveniência inteira.
fn palavra_antes(texto: &str, cursor: usize) -> bool {
    texto
        .get(..cursor)
        .and_then(|antes| antes.chars().next_back())
        .is_some_and(|caractere| caractere.is_alphanumeric() || caractere == '_')
}

/// O que fazer com o caractere que acabou de ser digitado.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Digitacao {
    /// Escrever os dois, e deixar o cursor entre eles.
    Fecha(char, char),
    /// O fechamento já é o que está sob o cursor: passar por cima, sem escrever.
    Pula(char),
    /// Envolver o trecho selecionado, em vez de apagá-lo.
    Envolve(char, char),
    /// Nada de par: escrever o que veio.
    Normal,
}

/// A decisão diante de uma tecla.
///
/// A segunda regra — `Pula` — é a que decide se isto ajuda ou atrapalha. Quem
/// tem o hábito de escrever o `)` vai escrevê-lo, e sem ela receberia `())`.
pub(super) fn ao_digitar(
    texto: &str,
    cursor: usize,
    selecao: Option<Range<usize>>,
    digitado: &str,
) -> Digitacao {
    let mut caracteres = digitado.chars();
    // Um caractere, e só um. Colar um trecho ou receber um texto composto não é
    // digitar um par.
    let (Some(caractere), None) = (caracteres.next(), caracteres.next()) else {
        return Digitacao::Normal;
    };
    let sob_o_cursor = texto.get(cursor..).and_then(|resto| resto.chars().next());
    // As aspas decidem primeiro, e numa ordem própria: sendo o mesmo caractere
    // dos dois lados, "passar por cima" tem de ser perguntado **antes** de
    // "abrir" — a mesma tecla é as duas coisas, e abrir sempre encheria o
    // arquivo de aspas nunca fechadas.
    if ASPAS.contains(&caractere) {
        if selecao.is_some() {
            return Digitacao::Envolve(caractere, caractere);
        }
        if sob_o_cursor == Some(caractere) {
            return Digitacao::Pula(caractere);
        }
        if palavra_antes(texto, cursor) {
            return Digitacao::Normal;
        }
        return Digitacao::Fecha(caractere, caractere);
    }
    if let Some(fecha) = fechamento_de(caractere) {
        if selecao.is_some() {
            return Digitacao::Envolve(caractere, fecha);
        }
        return Digitacao::Fecha(caractere, fecha);
    }
    // Com trecho marcado, digitar um fechamento é substituir o trecho por ele —
    // que é o que qualquer editor faz, e não é assunto deste módulo.
    if selecao.is_some() || !e_fechamento(caractere) {
        return Digitacao::Normal;
    }
    if sob_o_cursor == Some(caractere) {
        return Digitacao::Pula(caractere);
    }
    Digitacao::Normal
}

/// O abridor logo antes do cursor e o fechamento que lhe corresponde — **e só
/// enquanto os dois estiverem na mesma linha**.
///
/// Enquanto o par está numa linha só, o fechamento ainda é o eco da tecla que o
/// criou, e apagá-lo junto desfaz um gesto. Assim que o `Enter` o empurrou para
/// outra linha, ele deixou de ser eco e virou o fim de um bloco: apagá-lo
/// levaria embora o fechamento de um corpo que já tem conteúdo dentro, e quem
/// apertou `Backspace` uma vez não pediu isso.
pub(super) fn a_apagar(texto: &str, cursor: usize) -> Option<(Range<usize>, Range<usize>)> {
    let abre = texto.get(..cursor)?.chars().next_back()?;
    let inicio = cursor - abre.len_utf8();
    // Aspas não têm profundidade que se conte: `'a'` e `'b'` na mesma linha são
    // dois pares, e nada no texto diz qual aspa pertence a qual. Por isso aqui
    // só o par **encostado** — o que a tecla anterior acabou de criar.
    if ASPAS.contains(&abre) {
        if texto.get(cursor..).and_then(|resto| resto.chars().next()) != Some(abre) {
            return None;
        }
        return Some((inicio..cursor, cursor..cursor + abre.len_utf8()));
    }
    let fecha = fechamento_de(abre)?;
    // Profundidade porque o primeiro `)` da linha pode ser de um par de dentro:
    // em `f(g(|))`, apagar o `(` de `g` não pode levar o `)` de `f`.
    let mut profundidade = 0usize;
    for (deslocamento, caractere) in texto.get(cursor..)?.char_indices() {
        if caractere == '\n' {
            return None;
        }
        if caractere == abre {
            profundidade += 1;
        } else if caractere == fecha {
            if profundidade == 0 {
                let posicao = cursor + deslocamento;
                return Some((inicio..cursor, posicao..posicao + fecha.len_utf8()));
            }
            profundidade -= 1;
        }
    }
    None
}

/// A linha que o `Enter` abre logo depois de um abridor.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct LinhaNova {
    /// Brancos que abrem a linha onde o abridor está.
    pub abertura: String,
    /// Se o fechamento está encostado no cursor, e vai para a linha dele.
    pub fechamento_junto: bool,
}

/// O que o `Enter` encontra quando o cursor está logo depois de um abridor.
///
/// A linha em branco fica um nível mais fundo que a linha da abertura, e o
/// fechamento volta a alinhar com ela — nunca mais fundo, que é o que qualquer
/// formatador reescreveria no primeiro `diff`.
pub(super) fn ao_abrir_linha(texto: &str, cursor: usize) -> Option<LinhaNova> {
    let antes = texto.get(..cursor)?;
    let abre = antes.chars().next_back()?;
    let fecha = fechamento_de(abre)?;
    let inicio = antes.rfind('\n').map_or(0, |quebra| quebra + 1);
    let abertura = texto
        .get(inicio..)?
        .chars()
        .take_while(|caractere| *caractere == ' ' || *caractere == '\t')
        .collect();
    let fechamento_junto =
        texto.get(cursor..).and_then(|resto| resto.chars().next()) == Some(fecha);
    Some(LinhaNova {
        abertura,
        fechamento_junto,
    })
}

/// O passo de indentação **do arquivo**, lido das linhas que ele já tem.
///
/// Do arquivo, e não de uma configuração: um projeto que indenta com dois
/// espaços e uma IDE que indenta com quatro produzem um `diff` que quem revisa
/// vê antes do que foi escrito.
///
/// A conta é sobre **degraus**, e não sobre a menor indentação que aparece. A
/// menor seria quase sempre `1`, por causa do espaço que abre a linha de dentro
/// de um comentário de bloco — `* nota` —, e o arquivo inteiro passaria a
/// indentar de um em um.
pub(super) fn passo_de(texto: &str) -> Option<String> {
    let mut tabulacoes = 0usize;
    let mut espacos = 0usize;
    let mut larguras: Vec<usize> = Vec::new();
    for linha in texto.lines() {
        let brancos: String = linha
            .chars()
            .take_while(|caractere| *caractere == ' ' || *caractere == '\t')
            .collect();
        // Linha vazia ou só de brancos não diz nada sobre o passo.
        if brancos.len() == linha.len() {
            continue;
        }
        if brancos.starts_with('\t') {
            tabulacoes += 1;
        } else {
            if !brancos.is_empty() {
                espacos += 1;
            }
            // A largura entra mesmo quando é zero: são as linhas de fora que
            // dão o degrau das de dentro.
            larguras.push(brancos.len());
        }
    }
    // A comparação é entre linhas **indentadas**: contar as de margem zero do
    // lado dos espaços faria uma classe inteira tabulada perder para as duas
    // linhas que abrem e fecham o arquivo.
    if tabulacoes > espacos {
        return Some("\t".to_owned());
    }
    let mut degraus: HashMap<usize, usize> = HashMap::new();
    let mut anterior = 0usize;
    for largura in larguras {
        if largura > anterior {
            *degraus.entry(largura - anterior).or_default() += 1;
        }
        anterior = largura;
    }
    // O degrau mais comum; havendo empate, o menor deles.
    degraus
        .into_iter()
        .max_by_key(|(degrau, ocorrencias)| (*ocorrencias, Reverse(*degrau)))
        .map(|(degrau, _)| " ".repeat(degrau))
}

#[cfg(test)]
mod tests {
    use super::{Digitacao, LinhaNova, a_apagar, ao_abrir_linha, ao_digitar, passo_de};

    /// Abrir um par pede o fechamento; qualquer outra letra não pede nada.
    #[test]
    fn abrir_um_par_pede_o_fechamento() {
        assert_eq!(ao_digitar("", 0, None, "("), Digitacao::Fecha('(', ')'));
        assert_eq!(ao_digitar("", 0, None, "{"), Digitacao::Fecha('{', '}'));
        assert_eq!(ao_digitar("", 0, None, "["), Digitacao::Fecha('[', ']'));
        assert_eq!(ao_digitar("", 0, None, "a"), Digitacao::Normal);
    }

    /// Aspas fecham sozinhas, sendo o mesmo caractere dos dois lados.
    #[test]
    fn as_aspas_fecham_sozinhas() {
        assert_eq!(ao_digitar("", 0, None, "'"), Digitacao::Fecha('\'', '\''));
        assert_eq!(ao_digitar("", 0, None, "\""), Digitacao::Fecha('"', '"'));
        assert_eq!(ao_digitar("", 0, None, "`"), Digitacao::Fecha('`', '`'));
    }

    /// Digitar a aspa que já está sob o cursor passa por cima dela.
    ///
    /// Esta pergunta vem **antes** da de abrir: a mesma tecla é as duas coisas.
    #[test]
    fn a_aspa_sob_o_cursor_e_passagem_e_nao_abertura() {
        assert_eq!(ao_digitar("''", 1, None, "'"), Digitacao::Pula('\''));
        assert_eq!(ao_digitar("\"a\"", 2, None, "\""), Digitacao::Pula('"'));
    }

    /// O apóstrofo de `don't` não abre string nenhuma.
    #[test]
    fn o_apostrofo_no_meio_da_palavra_nao_abre_string() {
        assert_eq!(ao_digitar("// don", 6, None, "'"), Digitacao::Normal);
        assert_eq!(ao_digitar("valor1", 6, None, "'"), Digitacao::Normal);
        assert_eq!(ao_digitar("nome_", 5, None, "'"), Digitacao::Normal);
        // Depois de um espaço ou de um sinal, é string começando.
        assert_eq!(
            ao_digitar("const a = ", 10, None, "'"),
            Digitacao::Fecha('\'', '\'')
        );
    }

    /// Aspas também envolvem o trecho marcado.
    #[test]
    fn as_aspas_envolvem_o_trecho_marcado() {
        assert_eq!(
            ao_digitar("abc", 3, Some(0..3), "\""),
            Digitacao::Envolve('"', '"')
        );
    }

    /// Apagar a aspa leva a de trás **só quando ela está encostada**.
    #[test]
    fn apagar_a_aspa_leva_so_a_encostada() {
        assert_eq!(a_apagar("''", 1), Some((0..1, 1..2)));
        // `'a'` e `'b'` na mesma linha: nada diz qual aspa é de qual par.
        assert_eq!(a_apagar("'a' 'b'", 1), None);
    }

    /// Digitar o fechamento que já está sob o cursor passa por cima dele.
    #[test]
    fn digitar_o_fechamento_que_ja_esta_ali_passa_por_cima() {
        assert_eq!(ao_digitar("()", 1, None, ")"), Digitacao::Pula(')'));
        // Sob o cursor está outra coisa: é um fechamento de verdade a escrever.
        assert_eq!(ao_digitar("(a", 2, None, ")"), Digitacao::Normal);
        assert_eq!(ao_digitar("(]", 1, None, ")"), Digitacao::Normal);
    }

    /// Com trecho marcado, o abridor envolve em vez de apagar.
    #[test]
    fn com_trecho_marcado_o_abridor_envolve() {
        assert_eq!(
            ao_digitar("abc", 3, Some(0..3), "("),
            Digitacao::Envolve('(', ')')
        );
        // O fechamento com seleção continua sendo substituição, e não é daqui.
        assert_eq!(ao_digitar("abc", 3, Some(0..3), ")"), Digitacao::Normal);
    }

    /// Um texto composto — colado, por exemplo — não abre par nenhum.
    #[test]
    fn texto_composto_nao_e_par() {
        assert_eq!(ao_digitar("", 0, None, "(a)"), Digitacao::Normal);
        assert_eq!(ao_digitar("", 0, None, ""), Digitacao::Normal);
    }

    /// Apagar o abridor leva o fechamento junto, enquanto os dois estão na linha.
    #[test]
    fn apagar_o_abridor_leva_o_fechamento_da_mesma_linha() {
        assert_eq!(a_apagar("()", 1), Some((0..1, 1..2)));
        assert_eq!(a_apagar("f(a, b)", 2), Some((1..2, 6..7)));
        // Ninguém se apaga sozinho a partir de um caractere comum.
        assert_eq!(a_apagar("ab", 2), None);
        // Aberto e nunca fechado: o abridor vai sozinho.
        assert_eq!(a_apagar("f(a", 2), None);
    }

    /// Depois que o `Enter` separou os dois, o fechamento fica onde está.
    #[test]
    fn fechamento_em_outra_linha_fica_onde_esta() {
        assert_eq!(a_apagar("{\n  corpo\n}", 1), None);
    }

    /// O fechamento que se apaga é o do par, e não o primeiro que aparece.
    #[test]
    fn o_fechamento_e_o_do_par_e_nao_o_primeiro() {
        // `f(g(|))` — apagar o `(` de `g` leva o `)` de `g`, não o de `f`.
        assert_eq!(a_apagar("f(g())", 4), Some((3..4, 4..5)));
    }

    /// O `Enter` só abre bloco quando o cursor está logo depois de um abridor.
    #[test]
    fn o_enter_abre_bloco_depois_do_abridor() {
        assert_eq!(
            ao_abrir_linha("  metodo() {}", 12),
            Some(LinhaNova {
                abertura: "  ".to_owned(),
                fechamento_junto: true,
            })
        );
        // Sem o fechamento encostado, só a linha em branco.
        assert_eq!(
            ao_abrir_linha("  metodo() {", 12),
            Some(LinhaNova {
                abertura: "  ".to_owned(),
                fechamento_junto: false,
            })
        );
        // No meio do texto não há bloco que abrir.
        assert_eq!(ao_abrir_linha("  metodo()", 10), None);
    }

    /// O passo vem do arquivo: dois espaços, quatro, ou tabulação.
    #[test]
    fn o_passo_vem_do_arquivo() {
        let dois = "class A {\n  metodo() {\n    faz();\n  }\n}";
        assert_eq!(passo_de(dois).as_deref(), Some("  "));

        let quatro = "class A {\n    metodo() {\n        faz();\n    }\n}";
        assert_eq!(passo_de(quatro).as_deref(), Some("    "));

        let tabulado = "class A {\n\tmetodo() {\n\t\tfaz();\n\t}\n}";
        assert_eq!(passo_de(tabulado).as_deref(), Some("\t"));

        // Arquivo sem nenhuma linha indentada não tem o que dizer.
        assert_eq!(passo_de("const a = 1;\n"), None);
    }

    /// O espaço que abre a linha de um comentário de bloco não vira o passo.
    #[test]
    fn o_comentario_de_bloco_nao_define_o_passo() {
        let com_comentario = concat!(
            "/**\n",
            " * Nota.\n",
            " */\n",
            "class A {\n",
            "  metodo() {\n",
            "    faz();\n",
            "  }\n",
            "}\n",
        );
        assert_eq!(
            passo_de(com_comentario).as_deref(),
            Some("  "),
            "o ` *` do comentário indenta de um, e não pode arrastar o arquivo"
        );
    }
}
