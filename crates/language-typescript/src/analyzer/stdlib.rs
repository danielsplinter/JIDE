//! Os tipos que o próprio TypeScript traz — `Array`, `Date`, `HTMLElement`.
//!
//! É a fase 7 da `25`. Sem isto o índice não conhece **nenhum** tipo da
//! linguagem, e todo ponto sobre algo que o projeto não declara desce para o
//! analisador externo: `let w: String[];` custava o prazo de cinco segundos dele.
//!
//! # O que é daqui, e o que não é
//!
//! Daqui é o **texto**: dado o conteúdo de cada `lib.*.d.ts`, quais tipos ele
//! declara e quais membros cada um tem. Achar a pasta onde eles moram, ler a
//! versão instalada e descobrir o que o `tsconfig` alcança é conhecimento de
//! projeto, e mora em `crate::project::stdlib` — este módulo não lê
//! `package.json`, como diz a fronteira registrada em `analyzer`.
//!
//! # A fusão de interfaces, que é a parte nova
//!
//! No código de um projeto um tipo é declarado uma vez. Aqui não: `interface
//! Array` é reaberta em **12 arquivos**, e `forEach` mora no `lib.es5` enquanto
//! `at` mora no `lib.es2022.array`. Quem toma a primeira declaração e para
//! devolve uma lista que parece certa e está pela metade.
//!
//! **E a fusão vale só aqui.** Dois módulos de um projeto que declaram
//! `LoginService` são dois tipos, e juntá-los seria a armadilha que a fase 3
//! evitou nas referências. O que se funde é o que a linguagem manda fundir:
//! `interface` de mesmo nome no escopo global.
//!
//! # Guarda-se tudo; o alvo decide só o que se oferece
//!
//! Cada declaração guarda de qual `lib` veio, e é na **resposta** que o alcance
//! do projeto filtra. Assim trocar de alvo — ou o mesmo projeto ter
//! `tsconfig.app.json` e `tsconfig.spec.json` com alvos diferentes — não custa
//! releitura nenhuma.

use std::collections::{HashMap, HashSet};

use ide_domain::CompletionItem;

use super::{members, parser::TypeScriptParser};

/// A primeira linha do cache, que diz que ele é nosso e de qual formato.
///
/// **O número sobe sempre que o conteúdo gravado mudar**, e não só quando o
/// formato mudar. A chave do arquivo é a versão do TypeScript, que não muda
/// quando **nós** passamos a guardar mais coisa — foi o que aconteceu quando
/// método passou a guardar o tipo de retorno. Sem esta linha, quem já tivesse o
/// arquivo antigo continuaria lendo o conteúdo velho para sempre, e nada
/// acusaria: um cache incompleto tem a mesma cara de um cache certo.
const ASSINATURA: &str = "ERTSLIB2";

/// Uma declaração vinda de um arquivo da biblioteca.
struct Parte {
    /// O nome do `lib` de onde ela veio — `es5`, `dom`, `es2022.array`.
    ///
    /// Guardado por declaração, e não por tipo: é o que permite `Array` ter
    /// `forEach` sempre e `at` só a partir de ES2022.
    lib: String,
    itens: Vec<CompletionItem>,
    herda: Vec<String>,
}

/// Os tipos da biblioteca, prontos para consulta.
#[derive(Default)]
pub(crate) struct Biblioteca {
    tipos: HashMap<String, Vec<Parte>>,
}

/// Quais `lib` valem para um projeto.
///
/// Vazio quer dizer **sem filtro**, e não "nada vale": é o que responde por um
/// projeto cujo `tsconfig` não diz nada, onde recusar tudo seria pior do que a
/// completação que existia antes desta fase.
#[derive(Clone, Debug, Default)]
pub(crate) struct Alcance {
    libs: HashSet<String>,
}

impl Alcance {
    pub(crate) fn de(libs: HashSet<String>) -> Self {
        Self { libs }
    }

    fn contem(&self, lib: &str) -> bool {
        self.libs.is_empty() || self.libs.contains(lib)
    }
}

impl Biblioteca {
    /// Monta a biblioteca a partir do conteúdo de cada arquivo.
    ///
    /// Recebe o texto já lido, e não caminhos: quem lê disco é quem sabe onde os
    /// arquivos moram, e isso é conhecimento de projeto.
    pub(crate) fn nova<'a>(arquivos: impl Iterator<Item = (String, &'a str)>) -> Self {
        let Ok(parser) = TypeScriptParser::new() else {
            return Self::default();
        };
        let mut tipos: HashMap<String, Vec<Parte>> = HashMap::new();
        for (lib, texto) in arquivos {
            for (nome, membros) in members::todos_os_tipos(&parser, texto) {
                tipos.entry(nome).or_default().push(Parte {
                    lib: lib.clone(),
                    itens: membros.itens,
                    herda: membros.herda,
                });
            }
        }
        Self { tipos }
    }

    /// Quantos nomes distintos a biblioteca conhece.
    pub(crate) fn nomes(&self) -> usize {
        self.tipos.len()
    }

    /// Os membros de um tipo, fundidos e filtrados pelo alcance do projeto.
    ///
    /// `None` quando o nome não existe na biblioteca — e é diferente de uma
    /// lista vazia, que quer dizer "existe, e o alcance deste projeto não o
    /// alcança". Quem chama precisa dos dois para não afirmar o que não sabe.
    pub(crate) fn membros(&self, nome: &str, alcance: &Alcance) -> Option<members::Membros> {
        let partes = self.tipos.get(nome)?;
        let mut fundido = members::Membros::default();
        let mut vistos = HashSet::new();
        for parte in partes.iter().filter(|parte| alcance.contem(&parte.lib)) {
            for item in &parte.itens {
                if vistos.insert(item.label.clone()) {
                    fundido.itens.push(item.clone());
                }
            }
            for herdado in &parte.herda {
                if !fundido.herda.contains(herdado) {
                    fundido.herda.push(herdado.clone());
                }
            }
        }
        Some(fundido)
    }

    /// A biblioteca escrita como texto, para o cache em disco.
    ///
    /// # Por que um formato próprio, e não JSON
    ///
    /// `CompletionItem` mora em `ide-domain`, que é neutro e não conhece
    /// serialização. Pô-la lá para ganhar um `derive` é acrescentar dependência
    /// a uma crate que a IDE inteira usa, por causa de uma linguagem — e a `12`
    /// já decidiu que não.
    ///
    /// O formato é de linhas porque **cache que se lê a olho nu se conserta**:
    /// um índice binário corrompido vira um relatório de defeito sem pista, e
    /// este abre num editor de texto.
    pub(crate) fn escrever(&self) -> String {
        let mut saida = String::from(ASSINATURA);
        saida.push('\n');
        // Ordenado: um arquivo que muda de ordem a cada gravação é impossível de
        // comparar entre duas execuções.
        let mut nomes: Vec<&String> = self.tipos.keys().collect();
        nomes.sort();
        for nome in nomes {
            let Some(partes) = self.tipos.get(nome) else {
                continue;
            };
            for parte in partes {
                saida.push_str("T\t");
                saida.push_str(&escapar(nome));
                saida.push('\t');
                saida.push_str(&escapar(&parte.lib));
                saida.push('\n');
                for item in &parte.itens {
                    saida.push_str("M\t");
                    saida.push_str(numero_da_especie(item.kind));
                    saida.push('\t');
                    saida.push_str(&escapar(&item.label));
                    saida.push('\t');
                    saida.push_str(&escapar(item.detail.as_deref().unwrap_or_default()));
                    saida.push('\n');
                }
                for herdado in &parte.herda {
                    saida.push_str("H\t");
                    saida.push_str(&escapar(herdado));
                    saida.push('\n');
                }
            }
        }
        saida
    }

    /// A biblioteca lida de volta do cache.
    ///
    /// Uma linha que não se entende é **ignorada**, e não derruba a leitura: um
    /// cache é reconstruível por definição, e recusá-lo inteiro por causa de uma
    /// linha estranha custaria os 3,3 MB de novo.
    pub(crate) fn reler(texto: &str) -> Self {
        // **Um arquivo de outra versão é descartado, e não convertido.** É a
        // mesma regra do índice em disco, e ela tem endereço: a correção que fez
        // método guardar o tipo de retorno mudou o **conteúdo** sem mudar a
        // chave, que é a versão do TypeScript. Sem esta linha, quem já tivesse
        // rodado a versão anterior leria para sempre um cache sem tipo de
        // retorno — e nada acusaria, porque o arquivo continua bem formado.
        let Some(resto) = texto.strip_prefix(ASSINATURA) else {
            return Self::default();
        };
        let texto = resto;
        let mut tipos: HashMap<String, Vec<Parte>> = HashMap::new();
        let mut atual: Option<(String, Parte)> = None;
        for linha in texto.lines() {
            let mut campos = linha.split('\t');
            match campos.next() {
                Some("T") => {
                    if let Some((nome, parte)) = atual.take() {
                        tipos.entry(nome).or_default().push(parte);
                    }
                    let (Some(nome), Some(lib)) = (campos.next(), campos.next()) else {
                        continue;
                    };
                    atual = Some((
                        desescapar(nome),
                        Parte {
                            lib: desescapar(lib),
                            itens: Vec::new(),
                            herda: Vec::new(),
                        },
                    ));
                }
                Some("M") => {
                    let (Some(especie), Some(label), Some(detail)) =
                        (campos.next(), campos.next(), campos.next())
                    else {
                        continue;
                    };
                    if let Some((_, parte)) = atual.as_mut() {
                        let detail = desescapar(detail);
                        parte.itens.push(CompletionItem {
                            label: desescapar(label),
                            detail: (!detail.is_empty()).then_some(detail),
                            kind: especie_do_numero(especie),
                        });
                    }
                }
                Some("H") => {
                    if let (Some((_, parte)), Some(herdado)) = (atual.as_mut(), campos.next()) {
                        parte.herda.push(desescapar(herdado));
                    }
                }
                _ => {}
            }
        }
        if let Some((nome, parte)) = atual.take() {
            tipos.entry(nome).or_default().push(parte);
        }
        Self { tipos }
    }
}

/// Números das espécies, escritos à mão.
///
/// Derivar da ordem do `enum` faria reordenar uma variante mudar o significado
/// de todo cache já gravado, em silêncio. É a mesma regra do índice em disco.
const fn numero_da_especie(kind: ide_domain::CompletionKind) -> &'static str {
    match kind {
        ide_domain::CompletionKind::Method => "m",
        ide_domain::CompletionKind::Field => "f",
        _ => "o",
    }
}

fn especie_do_numero(numero: &str) -> ide_domain::CompletionKind {
    match numero {
        "m" => ide_domain::CompletionKind::Method,
        // Campo é o padrão: uma espécie desconhecida vira o que menos promete.
        _ => ide_domain::CompletionKind::Field,
    }
}

/// O separador é tabulação e o terminador é quebra de linha, então nenhum dos
/// dois pode aparecer dentro de um campo.
///
/// **E eles aparecem**: `detail` sai da anotação de tipo escrita no `.d.ts`, e
/// uma assinatura de função lá ocupa várias linhas. Sem escapar, um único membro
/// de `HTMLCanvasElement` quebraria o resto do arquivo em silêncio.
fn escapar(texto: &str) -> String {
    texto
        .replace('\\', "\\\\")
        .replace('\t', "\\t")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
}

fn desescapar(texto: &str) -> String {
    let mut saida = String::with_capacity(texto.len());
    let mut caracteres = texto.chars();
    while let Some(caractere) = caracteres.next() {
        if caractere != '\\' {
            saida.push(caractere);
            continue;
        }
        match caracteres.next() {
            Some('t') => saida.push('\t'),
            Some('n') => saida.push('\n'),
            Some('r') => saida.push('\r'),
            Some('\\') => saida.push('\\'),
            Some(outro) => saida.push(outro),
            None => break,
        }
    }
    saida
}

/// Os `lib` citados por `/// <reference lib="…" />` num arquivo.
///
/// Lido do texto, e não da árvore: são comentários, e a gramática não os
/// interpreta. O compilador também os lê assim.
///
/// # O atributo é `lib`, e não qualquer coisa terminada em `lib`
///
/// **Todo** arquivo da biblioteca começa com `no-default-lib="true"`, e procurar
/// `lib="` solto colhe o `true` dele como se fosse um `lib` chamado assim.
/// Nenhum arquivo `lib.true.d.ts` existe, então o engano não quebrava nada — ele
/// só punha um nome inventado dentro do alcance, calado, esperando o dia em que
/// alguém contasse quantos `lib` valem.
fn referencias_de_linha(linha: &str) -> Option<String> {
    let cortado = linha.trim();
    if !cortado.starts_with("///") {
        return None;
    }
    let inicio = cortado.find("lib=\"")?;
    // O que vem antes precisa separar palavras: em `no-default-lib=`, vem `-`.
    let anterior = cortado[..inicio].chars().next_back();
    if anterior.is_some_and(|caractere| !caractere.is_whitespace()) {
        return None;
    }
    let depois = cortado.get(inicio + "lib=\"".len()..)?;
    let fim = depois.find('"')?;
    Some(depois[..fim].to_lowercase())
}

pub(crate) fn referencias_de(texto: &str) -> Vec<String> {
    texto.lines().filter_map(referencias_de_linha).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const ES5: &str = "interface Array<T> {\n  length: number;\n  forEach(f: any): void;\n}\n\
                       interface String {\n  charAt(i: number): string;\n}\n";
    const ES2022_ARRAY: &str = "interface Array<T> {\n  at(i: number): T;\n}\n";
    const ES2024_ARRAY: &str = "interface Array<T> {\n  daFrente(): T;\n}\n";

    fn biblioteca() -> Biblioteca {
        Biblioteca::nova(
            [
                ("es5".to_owned(), ES5),
                ("es2022.array".to_owned(), ES2022_ARRAY),
                ("es2024.array".to_owned(), ES2024_ARRAY),
            ]
            .into_iter(),
        )
    }

    fn alcance(libs: &[&str]) -> Alcance {
        Alcance::de(libs.iter().map(|lib| (*lib).to_owned()).collect())
    }

    /// **`Array` é a soma das suas declarações.**
    ///
    /// `forEach` vem do `es5` e `at` do `es2022.array`. Tomar a primeira
    /// declaração e parar — que é o que se faz com o código do projeto —
    /// devolveria uma lista pela metade.
    #[test]
    fn a_reopened_interface_is_the_sum_of_its_declarations() {
        let Some(membros) = biblioteca().membros("Array", &alcance(&["es5", "es2022.array"])) else {
            panic!("`Array` precisa existir");
        };
        let nomes: Vec<_> = membros.itens.iter().map(|item| item.label.as_str()).collect();
        assert!(
            nomes.contains(&"forEach") && nomes.contains(&"at") && nomes.contains(&"length"),
            "a fusão precisa somar as declarações: {nomes:?}"
        );
    }

    /// **O alvo do projeto corta o que ele não alcança.**
    ///
    /// Oferecer um método de ES2024 num projeto ES2022 é sugerir código que o
    /// build recusa — pior do que não sugerir nada, porque o erro só aparece na
    /// compilação.
    #[test]
    fn the_target_hides_what_the_project_cannot_use() {
        let biblioteca = biblioteca();
        let Some(membros) = biblioteca.membros("Array", &alcance(&["es5", "es2022.array"])) else {
            panic!("`Array` precisa existir");
        };
        let nomes: Vec<_> = membros.itens.iter().map(|item| item.label.as_str()).collect();
        assert!(
            !nomes.contains(&"daFrente"),
            "ES2024 não vale num projeto ES2022: {nomes:?}"
        );
        // E a declaração continua guardada: o que muda é a resposta, e não a
        // leitura. Sem filtro, ela aparece.
        let Some(tudo) = biblioteca.membros("Array", &Alcance::default()) else {
            panic!("`Array` precisa existir");
        };
        let nomes: Vec<_> = tudo.itens.iter().map(|item| item.label.as_str()).collect();
        assert!(nomes.contains(&"daFrente"), "guardada, mas não oferecida");
    }

    /// Um nome que a biblioteca não tem é `None`, e não uma lista vazia.
    ///
    /// São respostas diferentes: uma diz "não conheço este tipo" e a outra diz
    /// "conheço, e ele não tem nada ao seu alcance". Confundi-las é a família de
    /// defeito que a `25` já encontrou várias vezes.
    #[test]
    fn an_unknown_name_is_not_an_empty_list() {
        let biblioteca = biblioteca();
        assert!(biblioteca.membros("Pedido", &Alcance::default()).is_none());
        assert!(biblioteca.membros("Array", &Alcance::default()).is_some());
        assert_eq!(biblioteca.nomes(), 2, "`Array` e `String`");
    }

    /// **O cache devolve o que guardou, com alvo e tudo.**
    ///
    /// O que se afirma não é "escreveu e leu": é que o **alcance continua
    /// funcionando depois da ida ao disco**. Gravar as declarações sem de qual
    /// `lib` cada uma veio pareceria certo — o `Array` reapareceria completo — e
    /// o projeto ES2022 passaria a receber os métodos de ES2024, calado.
    #[test]
    fn the_cache_gives_back_what_it_stored() {
        let original = biblioteca();
        let relida = Biblioteca::reler(&original.escrever());
        assert_eq!(relida.nomes(), original.nomes());

        let limitado = alcance(&["es5", "es2022.array"]);
        let (Some(antes), Some(depois)) = (
            original.membros("Array", &limitado),
            relida.membros("Array", &limitado),
        ) else {
            panic!("`Array` precisa existir dos dois lados");
        };
        let rotulos = |membros: &members::Membros| -> Vec<String> {
            membros.itens.iter().map(|item| item.label.clone()).collect()
        };
        assert_eq!(rotulos(&antes), rotulos(&depois));
        assert!(
            !rotulos(&depois).contains(&"daFrente".to_owned()),
            "o `lib` de cada declaração precisa sobreviver ao disco"
        );
    }

    /// **Um tipo escrito em várias linhas não quebra o arquivo.**
    ///
    /// O separador é tabulação e o terminador é quebra de linha, e as duas
    /// aparecem dentro de um `detail`: nos `.d.ts` de verdade uma união de tipos
    /// ocupa várias linhas, indentadas. Sem escapar, um membro estraga tudo o
    /// que vem depois dele — e o estrago é calado, porque o arquivo continua
    /// legível e só perde declarações.
    #[test]
    fn a_multiline_type_survives_the_cache() {
        let texto = "interface Tela {\n  \
                     modo:\n    | 'claro'\n    | 'escuro';\n  \
                     largura: number;\n}\n";
        let original = Biblioteca::nova([("dom".to_owned(), texto)].into_iter());
        let bruto = original.escrever();
        let relida = Biblioteca::reler(&bruto);
        let Some(membros) = relida.membros("Tela", &Alcance::default()) else {
            panic!("`Tela` precisa sobreviver");
        };
        let rotulos: Vec<_> = membros.itens.iter().map(|item| item.label.as_str()).collect();
        assert_eq!(
            rotulos,
            vec!["modo", "largura"],
            "o membro seguinte ao multilinha some quando não se escapa: {rotulos:?}"
        );
        let Some(detalhe) = membros.itens.first().and_then(|item| item.detail.as_deref()) else {
            panic!("o tipo do membro precisa sobreviver");
        };
        assert!(
            detalhe.contains("claro") && detalhe.contains("escuro"),
            "o detalhe voltou truncado: {detalhe:?}"
        );
    }

    /// **Um cache de outro formato é descartado, e não lido pela metade.**
    ///
    /// A chave do arquivo é a versão do TypeScript, e ela não muda quando *nós*
    /// passamos a guardar mais coisa. Foi o que aconteceu quando método passou a
    /// guardar o tipo de retorno: o arquivo antigo continuava bem formado e
    /// continuava sendo aceito, com metade da informação. Nada acusaria.
    #[test]
    fn a_cache_of_another_format_is_discarded() {
        let velho = "T\tArray\tes5\nM\tm\tforEach\t\n";
        assert_eq!(
            Biblioteca::reler(velho).nomes(),
            0,
            "sem a assinatura, o arquivo não é nosso"
        );
        let atual = biblioteca().escrever();
        assert!(atual.starts_with("ERTSLIB"), "o cache precisa se identificar");
        assert!(Biblioteca::reler(&atual).nomes() > 0);
    }

    /// A corrente de `reference lib` é lida do comentário.
    ///
    /// **E o `no-default-lib="true"` não entra.** Ele abre todo arquivo da
    /// biblioteca, e procurar `lib="` solto colhe o `true` dele como se fosse um
    /// `lib` chamado assim — um nome inventado dentro do alcance, calado.
    #[test]
    fn the_reference_chain_is_read_from_the_comments() {
        let texto = "/// <reference no-default-lib=\"true\"/>\n\
                     /// <reference lib=\"es2021\" />\n\
                     /// <reference lib=\"ES2022.Array\" />\n\
                     interface Coisa {}\n";
        assert_eq!(referencias_de(texto), vec!["es2021", "es2022.array"]);
    }
}
