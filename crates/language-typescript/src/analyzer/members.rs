//! Os membros de um tipo, e o tipo de um receptor.
//!
//! É a fase 4 da `25`: o que aparece depois do ponto quando o receptor tem tipo
//! **declarado**. Tudo aqui sai do texto — que nome tem cada membro, e que tipo
//! está escrito ao lado de cada nome. Para onde um nome de tipo aponta é
//! conhecimento de projeto, e mora em `modules`.
//!
//! # O que este módulo não faz, e é metade do assunto
//!
//! Ele não infere. `store.select(sel).pipe(map(x => x.` exige instanciar
//! genéricos, escolher entre sobrecargas e fazer o tipo voltar da assinatura
//! para dentro da lambda — o verificador de tipos, que a ADR-025 recusou
//! escrever.
//!
//! Por isso a resposta aqui tem **três** formas, e não duas: os membros, "este
//! tipo não tem membros", e **"não sei o tipo"**. As duas últimas se pareceriam
//! numa lista vazia, e confundi-las é a família de defeito que esta IDE já
//! encontrou várias vezes.

use ide_domain::{CompletionItem, CompletionKind};

use super::parser::TypeScriptParser;

/// O que se descobriu sobre o receptor de um ponto.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Receptor {
    /// O receptor tem este tipo declarado.
    Tipo(String),
    /// **O segundo elo de uma cadeia:** `this.svc.` ou `this.buscar().`.
    ///
    /// Quem está antes do ponto é um **membro** de um tipo que se conhece, e o
    /// tipo dele está escrito ao lado da declaração desse membro. Achar onde
    /// esse membro é declarado exige atravessar módulos e herança, e isso é
    /// conhecimento de projeto: este módulo diz **de quem** e **qual membro**, e
    /// quem resolve responde.
    ///
    /// É a fase 8 da `25`, e ela existe porque um quarto de tudo o que a IDE não
    /// respondia estava aqui — num componente Angular, quase tudo se acessa por
    /// `this.`.
    Membro {
        /// O tipo de quem está antes do primeiro ponto.
        tipo: String,
        /// O nome do membro sob o ponto que se está resolvendo.
        membro: String,
    },
    /// Há um ponto, e **não se sabe** o tipo de quem está antes dele.
    Desconhecido,
    /// Não há ponto nenhum aqui: a pergunta não é de acesso a membro.
    Nenhum,
}

/// O tipo do receptor do ponto imediatamente antes da posição.
///
/// # As formas que ele reconhece
///
/// | forma | de onde sai o tipo |
/// | --- | --- |
/// | `this.` | a classe que envolve a posição |
/// | `svc.`, injetado no construtor | a anotação do parâmetro |
/// | `svc.`, campo da classe | a anotação do campo |
/// | `p.`, com `const p: Pedido` | a anotação da declaração |
/// | `p.`, com `const p = new Pedido()` | o construtor chamado |
/// | `this.svc.` | `Membro`: o tipo de `this` e o nome `svc` |
/// | `this.buscar().` | `Membro`: o tipo de `this` e o nome `buscar` |
///
/// As duas últimas são a fase 8, e não trazem o tipo pronto: o tipo de um membro
/// está escrito onde ele é declarado, e achar isso atravessa módulos e herança —
/// conhecimento de projeto, que não é deste módulo.
///
/// Qualquer outra coisa — elemento de vetor, terceiro elo de uma cadeia, receptor
/// vindo de genérico — devolve `Desconhecido`, e **não** `Nenhum`.
pub(crate) fn receptor_em(
    parser: &TypeScriptParser,
    texto: &str,
    linha: u32,
    coluna: u32,
) -> Receptor {
    let Some(recorte) = ate_a_posicao(texto, linha, coluna) else {
        return Receptor::Nenhum;
    };
    let antes = recorte.trim_end();
    let Some(sem_ponto) = antes.strip_suffix('.') else {
        // Pode haver um prefixo já digitado: `this.tot|`.
        let Some(inicio) = antes.rfind(['.', ' ', '(', ',', ';', '\n', '\t']) else {
            return Receptor::Nenhum;
        };
        if antes.as_bytes().get(inicio) != Some(&b'.') {
            return Receptor::Nenhum;
        }
        return receptor_do_texto(parser, texto, &antes[..inicio]);
    };
    receptor_do_texto(parser, texto, sem_ponto)
}

/// O texto do arquivo até a posição, em bytes.
fn ate_a_posicao(texto: &str, linha: u32, coluna: u32) -> Option<&str> {
    let mut inicio = 0usize;
    for (numero, conteudo) in texto.split_inclusive('\n').enumerate() {
        if numero == linha as usize {
            let sem_quebra = conteudo.strip_suffix('\n').unwrap_or(conteudo);
            let sem_quebra = sem_quebra.strip_suffix('\r').unwrap_or(sem_quebra);
            let dentro = sem_quebra
                .char_indices()
                .nth(coluna as usize)
                .map_or(sem_quebra.len(), |(byte, _)| byte);
            return texto.get(..inicio + dentro);
        }
        inicio += conteudo.len();
    }
    None
}

/// O receptor, dado o texto que vem antes do ponto.
fn receptor_do_texto(parser: &TypeScriptParser, texto: &str, antes: &str) -> Receptor {
    // `this.buscar().` — o que vem antes do ponto é uma chamada, e o que
    // interessa é o **nome chamado**: o tipo dele é o que o método devolve, e
    // isso já está guardado desde a correção que precedeu a fase 8.
    let antes = sem_a_chamada_final(antes);
    let Some(nome) = ultimo_identificador(antes) else {
        // Antes do ponto há alguma coisa que não é um nome simples — `]`, uma
        // cadeia de chamadas. **Não sabemos**, e dizer isso é a resposta.
        return if antes.trim_end().is_empty() {
            Receptor::Nenhum
        } else {
            Receptor::Desconhecido
        };
    };
    // Em `a.b.`, o `b` é membro de `a`. O tipo de `a` sai daqui; o de `b` está
    // escrito onde `b` é declarado, e achar isso é de quem resolve módulos.
    let base = antes
        .trim_end()
        .strip_suffix(&nome)
        .map(str::trim_end)
        .and_then(|resto| resto.strip_suffix('.'));
    if let Some(base) = base {
        return match receptor_do_texto(parser, texto, base) {
            Receptor::Tipo(tipo) => Receptor::Membro { tipo, membro: nome },
            // **Um passo, e não a cadeia inteira.** Em `a.b.c.`, resolver `c`
            // exigiria o tipo de `b`, que já é resposta desta mesma pergunta —
            // e cada elo a mais multiplica o custo por uma frequência que a
            // medição da fase 7 mostrou ser pequena.
            _ => Receptor::Desconhecido,
        };
    }
    if nome == "this" {
        return match classe_que_envolve(parser, texto, antes.len()) {
            Some(classe) => Receptor::Tipo(classe),
            None => Receptor::Desconhecido,
        };
    }
    match tipo_declarado(parser, texto, &nome) {
        Some(tipo) => Receptor::Tipo(tipo),
        None => Receptor::Desconhecido,
    }
}

/// O texto sem a lista de argumentos que o encerra, se houver uma.
///
/// `this.buscar(a, f(b))` vira `this.buscar`. Os parênteses são contados, e não
/// procurados de trás para frente pelo primeiro que aparecer: um argumento que
/// seja outra chamada tem parênteses dentro, e cortar no primeiro deixaria
/// `this.buscar(a, f` — um nome que não existe.
fn sem_a_chamada_final(texto: &str) -> &str {
    let cortado = texto.trim_end();
    if !cortado.ends_with(')') {
        return texto;
    }
    let mut profundidade = 0i32;
    for (byte, caractere) in cortado.char_indices().rev() {
        match caractere {
            ')' => profundidade += 1,
            '(' => {
                profundidade -= 1;
                if profundidade == 0 {
                    return &cortado[..byte];
                }
            }
            _ => {}
        }
    }
    // Parênteses desequilibrados: o texto está sendo digitado, e não há o que
    // concluir dele.
    texto
}

/// O nome do tipo escrito num texto — o que está ao lado de um membro.
///
/// # Por que passa pela gramática em vez de olhar o texto
///
/// `Observable<Pedido>` é `Observable`, `Pedido[]` é `Array`, `string` é
/// `String`. As três regras já existem e já são testadas em [`nome_do_tipo`];
/// reescrevê-las sobre texto seria uma segunda implementação da mesma coisa,
/// que envelheceria em silêncio quando uma delas mudasse.
///
/// # União só passa quando sobra um tipo só
///
/// `Pedido | null` é um `Pedido` para quem vai digitar um ponto. `string |
/// number` **não é** nenhum dos dois, e responder com os membros do primeiro
/// seria a resposta errada com a cara da certa — o defeito que esta
/// especificação já caçou várias vezes. Nesse caso, não se sabe.
pub(crate) fn nome_do_tipo_escrito(parser: &TypeScriptParser, escrito: &str) -> Option<String> {
    let uteis: Vec<&str> = escrito
        .split('|')
        .map(str::trim)
        .filter(|parte| !parte.is_empty() && *parte != "null" && *parte != "undefined")
        .collect();
    let [unico] = uteis.as_slice() else {
        return None;
    };
    let fonte = format!("let __er: {unico};");
    let arvore = parser.parse(&fonte, None).ok()?;
    let bytes = fonte.as_bytes();
    let mut cursor = arvore.walk();
    let mut pilha = vec![arvore.root_node()];
    while let Some(no) = pilha.pop() {
        if no.kind() == "type_annotation" {
            return nome_do_tipo(no, bytes);
        }
        let filhos: Vec<_> = no.children(&mut cursor).collect();
        pilha.extend(filhos.into_iter().rev());
    }
    None
}

fn ultimo_identificador(texto: &str) -> Option<String> {
    let fim = texto.trim_end();
    let inicio = fim
        .char_indices()
        .rev()
        .take_while(|(_, c)| c.is_alphanumeric() || *c == '_' || *c == '$')
        .last()
        .map(|(byte, _)| byte)?;
    let nome = fim.get(inicio..)?;
    (!nome.is_empty() && !nome.starts_with(|c: char| c.is_numeric())).then(|| nome.to_owned())
}

/// O nome da classe que envolve um deslocamento.
fn classe_que_envolve(parser: &TypeScriptParser, texto: &str, byte: usize) -> Option<String> {
    let arvore = parser.parse(texto, None).ok()?;
    let mut no = arvore
        .root_node()
        .descendant_for_byte_range(byte.saturating_sub(1), byte)?;
    loop {
        if matches!(no.kind(), "class_declaration" | "abstract_class_declaration")
            && let Some(nome) = no.child_by_field_name("name")
            && let Ok(texto_do_nome) = nome.utf8_text(texto.as_bytes())
        {
            return Some(texto_do_nome.to_owned());
        }
        no = no.parent()?;
    }
}

/// O tipo declarado de um nome, procurado pelo arquivo.
///
/// Procura anotação de parâmetro de construtor, de campo e de declaração, e
/// `new X()`. **Não** infere nada além disso.
fn tipo_declarado(parser: &TypeScriptParser, texto: &str, nome: &str) -> Option<String> {
    let arvore = parser.parse(texto, None).ok()?;
    let bytes = texto.as_bytes();
    let mut cursor = arvore.walk();
    let mut pilha = vec![arvore.root_node()];
    while let Some(no) = pilha.pop() {
        let candidato = match no.kind() {
            // `constructor(private svc: LoginService)` e `metodo(p: Pedido)`.
            "required_parameter" | "optional_parameter" => no,
            // `svc: LoginService;` dentro da classe.
            "public_field_definition" | "property_signature" => no,
            // `const p: Pedido = …` e `const p = new Pedido()`.
            "variable_declarator" => no,
            _ => {
                pilha.extend(no.children(&mut cursor));
                continue;
            }
        };
        let mesmo = candidato
            .child_by_field_name("pattern")
            .or_else(|| candidato.child_by_field_name("name"))
            .and_then(|no| no.utf8_text(bytes).ok())
            .is_some_and(|escrito| escrito == nome);
        if mesmo {
            if let Some(tipo) = candidato
                .child_by_field_name("type")
                .and_then(|no| nome_do_tipo(no, bytes))
            {
                return Some(tipo);
            }
            if let Some(construido) = candidato
                .child_by_field_name("value")
                .and_then(|valor| construtor_chamado(valor, bytes))
            {
                return Some(construido);
            }
        }
        pilha.extend(candidato.children(&mut cursor));
    }
    None
}

/// O nome escrito numa anotação de tipo, sem genéricos.
///
/// `Observable<Pedido>` devolve `Observable`, e é o bastante para dizer que o
/// **receptor** é um `Observable`; o que os métodos dele devolvem é genérico, e
/// isso o índice não sabe.
///
/// # `Pedido[]` é um `Array`, e não um `Pedido`
///
/// Descer até o primeiro nome escrito devolvia o tipo do **elemento**, jogando
/// fora o `[]`: `let w: String[]` dizia `String`. Hoje isso passa despercebido
/// porque `String` não está no índice e a pergunta desce para o analisador — mas
/// num projeto que declare o próprio `Pedido`, `pedidos.` ofereceria os membros
/// de um pedido no lugar de `map`, `filter` e `length`. Resposta errada, que é
/// pior do que "não sei".
///
/// # E a ordem da visita não era a da escrita
///
/// A pilha empilhava os filhos na ordem do texto e os retirava do fim, então a
/// busca chegava ao **último** nome antes do primeiro. Para `Observable<Pedido>`
/// isso devolvia `Pedido`, o oposto do que esta documentação promete — e num
/// código Angular, cheio de `Observable<T>` e `Signal<T>`, era o ponto oferecendo
/// os membros do conteúdo no lugar dos do recipiente. Empilhar ao contrário faz
/// a retirada seguir a ordem do texto.
fn nome_do_tipo(no: tree_sitter::Node, bytes: &[u8]) -> Option<String> {
    let mut cursor = no.walk();
    let mut pilha = vec![no];
    while let Some(atual) = pilha.pop() {
        if let Some(nome) = nome_de_vetor(atual) {
            return Some(nome);
        }
        if atual.kind() == "predefined_type"
            && let Ok(texto) = atual.utf8_text(bytes)
            && let Some(interface) = interface_do_primitivo(texto)
        {
            return Some(interface.to_owned());
        }
        if matches!(atual.kind(), "type_identifier" | "identifier")
            && let Ok(texto) = atual.utf8_text(bytes)
        {
            return Some(texto.to_owned());
        }
        let filhos: Vec<_> = atual.children(&mut cursor).collect();
        pilha.extend(filhos.into_iter().rev());
    }
    None
}

/// A interface que declara os membros de um tipo primitivo.
///
/// `const nome: string` é a anotação mais comum que existe, e o `string`
/// minúsculo **não é** um nome de tipo declarado em lugar nenhum: quem declara
/// `charAt`, `trim` e `split` é a `interface String` do `lib.es5.d.ts`. Sem esta
/// tradução, o ponto sobre a metade das variáveis de um projeto continuaria
/// dizendo "não sei".
///
/// `any`, `unknown`, `void`, `never` e `object` ficam de fora **de propósito**:
/// não há interface por trás deles, e inventar uma seria oferecer membros que
/// ninguém declarou.
fn interface_do_primitivo(escrito: &str) -> Option<&'static str> {
    Some(match escrito {
        "string" => "String",
        "number" => "Number",
        "boolean" => "Boolean",
        "symbol" => "Symbol",
        "bigint" => "BigInt",
        _ => return None,
    })
}

/// `Array` quando o nó é uma das duas formas de vetor, e nada quando não é.
///
/// `T[]` e `ReadonlyArray<T>` já chegam como nome próprio; as duas que não
/// chegam são a anotação com colchetes e a tupla. Uma tupla não é um `Array` no
/// verificador de tipos, mas **tem os membros de um**, que é o que esta pergunta
/// quer saber.
fn nome_de_vetor(no: tree_sitter::Node) -> Option<String> {
    matches!(no.kind(), "array_type" | "tuple_type").then(|| "Array".to_owned())
}

fn construtor_chamado(no: tree_sitter::Node, bytes: &[u8]) -> Option<String> {
    if no.kind() != "new_expression" {
        return None;
    }
    no.child_by_field_name("constructor")?
        .utf8_text(bytes)
        .ok()
        .map(str::to_owned)
}

/// Os membros que um tipo declara, e de que tipo ele herda.
#[derive(Debug, Default)]
pub(crate) struct Membros {
    pub(crate) itens: Vec<CompletionItem>,
    /// Os tipos de que este herda, para a cadeia ser seguida por quem resolve.
    pub(crate) herda: Vec<String>,
}

/// Quem está perguntando, do ponto de vista de quem responde.
///
/// # Por que a lista depende disso
///
/// `private` existe para não ser visto de fora. Oferecê-lo a quem está fora é
/// sugerir código que **não compila** — e é a família de defeito que esta
/// especificação mais persegue, porque a lista errada tem a mesma cara da certa.
///
/// Medido no projeto de referência: em **45 de 96** pontos que o índice
/// respondia, ele oferecia a mais — e era sempre isto ou um `static`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Alcance {
    /// O receptor é a própria classe que envolve o cursor: `this.`.
    Dentro,
    /// O receptor é outro tipo. Só o que é público.
    DeFora,
}

/// Os membros de um tipo declarado num texto.
pub(crate) fn membros_de(
    parser: &TypeScriptParser,
    texto: &str,
    tipo: &str,
    alcance: Alcance,
) -> Option<Membros> {
    let arvore = parser.parse(texto, None).ok()?;
    membros_na_arvore(&arvore, texto, tipo, alcance)
}

/// O mesmo, sobre uma árvore que já existe.
pub(crate) fn membros_na_arvore(
    arvore: &tree_sitter::Tree,
    texto: &str,
    tipo: &str,
    alcance: Alcance,
) -> Option<Membros> {
    let bytes = texto.as_bytes();
    let mut cursor = arvore.walk();
    let mut pilha = vec![arvore.root_node()];
    while let Some(no) = pilha.pop() {
        let corresponde = matches!(
            no.kind(),
            "class_declaration"
                | "abstract_class_declaration"
                | "interface_declaration"
                | "enum_declaration"
        ) && no
            .child_by_field_name("name")
            .and_then(|nome| nome.utf8_text(bytes).ok())
            .is_some_and(|nome| nome == tipo);
        if corresponde {
            return Some(colher_membros(no, bytes, alcance));
        }
        pilha.extend(no.children(&mut cursor));
    }
    None
}

/// O nome da classe que envolve uma posição, se houver uma.
///
/// É o que separa `this.` de `outro.`: quem pergunta de dentro vê o que é
/// privado, e quem pergunta de fora não.
pub(crate) fn classe_em(
    parser: &TypeScriptParser,
    texto: &str,
    linha: u32,
    coluna: u32,
) -> Option<String> {
    let recorte = ate_a_posicao(texto, linha, coluna)?;
    classe_que_envolve(parser, texto, recorte.len())
}

/// Todos os tipos declarados num texto, com os membros de cada um.
///
/// # Por que não é `membros_de` num laço
///
/// `membros_de` procura **um** nome e para na primeira declaração dele, que é o
/// certo para o código de um projeto: ali um tipo é declarado uma vez, e duas
/// declarações do mesmo nome são dois tipos.
///
/// Nos `lib.*.d.ts` do TypeScript a regra é outra — `interface Array` é reaberta
/// em doze arquivos, e cada abertura acrescenta. Esta função entrega o que cada
/// arquivo declara, **sem fundir**: fundir é decisão de quem sabe que está
/// olhando para a biblioteca, e não para código de projeto. Ver a fase 7 da `25`.
pub(crate) fn todos_os_tipos(parser: &TypeScriptParser, texto: &str) -> Vec<(String, Membros)> {
    let Ok(arvore) = parser.parse(texto, None) else {
        return Vec::new();
    };
    let bytes = texto.as_bytes();
    let mut achados = Vec::new();
    let mut cursor = arvore.walk();
    let mut pilha = vec![arvore.root_node()];
    while let Some(no) = pilha.pop() {
        let declara = matches!(
            no.kind(),
            "class_declaration"
                | "abstract_class_declaration"
                | "interface_declaration"
                | "enum_declaration"
        );
        if declara
            && let Some(nome) = no
                .child_by_field_name("name")
                .and_then(|nome| nome.utf8_text(bytes).ok())
        {
            // A biblioteca é sempre olhada de fora: ninguém edita dentro da
            // `interface String`.
            achados.push((nome.to_owned(), colher_membros(no, bytes, Alcance::DeFora)));
        }
        pilha.extend(no.children(&mut cursor));
    }
    achados
}

/// O tipo escrito ao lado de um membro.
///
/// # Propriedade guarda em `type`; método guarda em `return_type`
///
/// A gramática separa os dois porque são conceitos diferentes — uma propriedade
/// **tem** um tipo, um método **devolve** um. Lendo só `type`, todo método saía
/// sem tipo nenhum: `buscar(): Pedido` guardava `buscar` e perdia `Pedido`.
///
/// **O estrago aparecia pequeno e não era.** Na lista de completação o método
/// só aparecia sem dizer o que devolve, que é cosmético. Mas é deste campo que
/// sai o receptor do elo seguinte de uma cadeia: `this.buscar().` só tem
/// resposta se `buscar` tiver guardado `Pedido`. Sem isto, metade dos elos de
/// uma cadeia em código Angular — as chamadas — nasceria cega, e **pareceria
/// funcionar**, porque a outra metade responderia.
fn tipo_do_membro(membro: tree_sitter::Node, bytes: &[u8]) -> Option<String> {
    membro
        .child_by_field_name("type")
        .or_else(|| membro.child_by_field_name("return_type"))
        .and_then(|no| no.utf8_text(bytes).ok())
        .map(|texto| texto.trim_start_matches(':').trim().to_owned())
}

/// Os parâmetros de construtor que **também são campos**.
///
/// `constructor(private readonly svc: LoginService)` declara um parâmetro e um
/// campo de uma vez, e é o idioma de injeção de dependência de Angular: no
/// projeto de referência, **toda** página injeta assim.
///
/// # O que separa um parâmetro-campo de um parâmetro comum
///
/// O modificador. `constructor(x: Foo)` recebe `x` e o esquece quando o
/// construtor termina; `constructor(private x: Foo)` guarda. Incluir os dois
/// faria `this.` oferecer nomes que não existem depois da construção — resposta
/// errada, e não uma a menos.
///
/// **Faltava por inteiro, e a medição da fase 8 é que apontou.** O tipo do
/// receptor já saía daqui — `svc.` sozinho funcionava, porque quem o resolve
/// olha os parâmetros do construtor. O que não funcionava era `this.svc.`, que
/// pergunta pelos **membros da classe** — e nesta lista o `svc` não estava.
fn propriedades_do_construtor(
    construtor: tree_sitter::Node,
    bytes: &[u8],
    alcance: Alcance,
) -> Vec<CompletionItem> {
    let Some(parametros) = construtor.child_by_field_name("parameters") else {
        return Vec::new();
    };
    let mut cursor = parametros.walk();
    let mut achados = Vec::new();
    for parametro in parametros.named_children(&mut cursor) {
        if !matches!(parametro.kind(), "required_parameter" | "optional_parameter") {
            continue;
        }
        let mut interno = parametro.walk();
        let guarda = parametro.children(&mut interno).any(|filho| {
            matches!(filho.kind(), "accessibility_modifier" | "override_modifier")
                || filho.utf8_text(bytes).is_ok_and(|texto| texto == "readonly")
        });
        if !guarda {
            continue;
        }
        // Um `private svc` guardado pelo construtor é tão privado quanto um
        // campo escrito à mão: quem olha de fora não o vê.
        if alcance == Alcance::DeFora && e_escondido(parametro, bytes) {
            continue;
        }
        let Some(nome) = parametro
            .child_by_field_name("pattern")
            .and_then(|no| no.utf8_text(bytes).ok())
        else {
            continue;
        };
        achados.push(CompletionItem {
            label: nome.to_owned(),
            detail: tipo_do_membro(parametro, bytes),
            kind: CompletionKind::Field,
        });
    }
    achados
}

/// Se um membro é `static`.
///
/// **Estático não aparece depois de um ponto numa instância**, e é o que fazia a
/// IDE oferecer `ɵfac` e `ɵprov` — os internos que o compilador do Angular
/// declara em toda classe gerada. Eles existem no `.d.ts`, e não existem no
/// objeto que se tem na mão.
///
/// Nada aqui é de Angular: a regra é da linguagem, e vale para qualquer
/// `static`. O que o projeto de referência deu foi o **caso** que a revelou.
fn e_estatico(membro: tree_sitter::Node, bytes: &[u8]) -> bool {
    let mut cursor = membro.walk();
    membro
        .children(&mut cursor)
        .any(|filho| filho.utf8_text(bytes).is_ok_and(|texto| texto == "static"))
}

/// Se um membro é escondido de quem olha de fora.
fn e_escondido(membro: tree_sitter::Node, bytes: &[u8]) -> bool {
    let mut cursor = membro.walk();
    membro.children(&mut cursor).any(|filho| {
        filho.kind() == "accessibility_modifier"
            && filho
                .utf8_text(bytes)
                .is_ok_and(|texto| matches!(texto, "private" | "protected"))
    })
}

fn colher_membros(tipo: tree_sitter::Node, bytes: &[u8], alcance: Alcance) -> Membros {
    let mut membros = Membros::default();
    if let Some(corpo) = tipo.child_by_field_name("body") {
        let mut cursor = corpo.walk();
        for filho in corpo.named_children(&mut cursor) {
            let kind = match filho.kind() {
                "method_definition" | "method_signature" => CompletionKind::Method,
                "public_field_definition" | "property_signature" | "enum_assignment" => {
                    CompletionKind::Field
                }
                // Um item de enum sem valor é um identificador solto no corpo.
                "property_identifier" => CompletionKind::Field,
                _ => continue,
            };
            let nome = filho
                .child_by_field_name("name")
                .or(Some(filho))
                .and_then(|no| no.utf8_text(bytes).ok());
            let Some(nome) = nome else { continue };
            // O construtor não é membro que se acesse por ponto — mas os
            // parâmetros dele podem ser membros, e num código Angular quase
            // sempre são.
            if nome == "constructor" {
                membros
                    .itens
                    .extend(propriedades_do_construtor(filho, bytes, alcance));
                continue;
            }
            if e_estatico(filho, bytes) {
                continue;
            }
            if alcance == Alcance::DeFora && e_escondido(filho, bytes) {
                continue;
            }
            membros.itens.push(CompletionItem {
                label: nome.to_owned(),
                detail: tipo_do_membro(filho, bytes),
                kind,
            });
        }
    }
    // `extends` e `implements`: os membros herdados também aparecem depois do
    // ponto, e não tê-los faria a lista parecer certa e incompleta.
    let mut cursor = tipo.walk();
    for filho in tipo.named_children(&mut cursor) {
        if !matches!(filho.kind(), "class_heritage" | "extends_type_clause") {
            continue;
        }
        let mut interno = filho.walk();
        let mut pilha = vec![filho];
        while let Some(atual) = pilha.pop() {
            if matches!(atual.kind(), "type_identifier" | "identifier")
                && let Ok(nome) = atual.utf8_text(bytes)
            {
                membros.herda.push(nome.to_owned());
            }
            pilha.extend(atual.children(&mut interno));
        }
    }
    membros
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parser() -> TypeScriptParser {
        match TypeScriptParser::new() {
            Ok(parser) => parser,
            Err(_) => panic!("a gramática precisa carregar"),
        }
    }

    /// `this.` dentro da classe sabe de que classe se trata.
    #[test]
    fn this_knows_its_class() {
        let texto = "export class Pedido {\n  total = 0;\n  somar() {\n    this.\n  }\n}\n";
        assert_eq!(
            receptor_em(&parser(), texto, 3, 9),
            Receptor::Tipo("Pedido".to_owned())
        );
    }

    /// Um parâmetro de construtor injetado tem o tipo escrito ao lado.
    ///
    /// É o padrão de injeção de dependência, e o receptor mais comum num código
    /// Angular.
    #[test]
    fn an_injected_constructor_parameter_has_its_type_written_next_to_it() {
        let texto = "class Pagina {\n  constructor(private svc: LoginService) {}\n  ir() {\n    svc.\n  }\n}\n";
        assert_eq!(
            receptor_em(&parser(), texto, 3, 8),
            Receptor::Tipo("LoginService".to_owned())
        );
    }

    /// `const p: Pedido` e `const p = new Pedido()` dizem o tipo.
    #[test]
    fn a_declared_local_says_its_type() {
        let anotado = "const p: Pedido = null!;\np.\n";
        assert_eq!(
            receptor_em(&parser(), anotado, 1, 2),
            Receptor::Tipo("Pedido".to_owned())
        );
        let construido = "const p = new Pedido();\np.\n";
        assert_eq!(
            receptor_em(&parser(), construido, 1, 2),
            Receptor::Tipo("Pedido".to_owned())
        );
    }

    /// **`Observable<Pedido>` é um `Observable`, e não um `Pedido`.**
    ///
    /// A documentação de `nome_do_tipo` sempre prometeu isto, e o código fazia o
    /// contrário: a pilha visitava os filhos ao contrário, então o primeiro nome
    /// encontrado era o **argumento** do genérico. Num código Angular, onde
    /// `Observable<T>` e `Signal<T>` estão em toda parte, o ponto oferecia os
    /// membros do conteúdo no lugar dos do recipiente.
    #[test]
    fn a_generic_is_its_own_name_and_not_its_argument() {
        let texto = "const fluxo: Observable<Pedido> = null!;\nfluxo.\n";
        assert_eq!(
            receptor_em(&parser(), texto, 1, 6),
            Receptor::Tipo("Observable".to_owned())
        );
    }

    /// **`string` minúsculo é a `interface String`.**
    ///
    /// É a anotação mais comum que existe, e o `string` da linguagem não é um
    /// nome declarado em lugar nenhum: quem declara `charAt` e `trim` é a
    /// `interface String` do `lib.es5.d.ts`. Sem a tradução, o ponto sobre
    /// metade das variáveis de um projeto continuaria dizendo "não sei".
    #[test]
    fn a_primitive_annotation_is_its_interface() {
        for (escrito, interface) in [
            ("string", "String"),
            ("number", "Number"),
            ("boolean", "Boolean"),
        ] {
            let texto = format!("const v: {escrito} = null!;\nv.\n");
            assert_eq!(
                receptor_em(&parser(), &texto, 1, 2),
                Receptor::Tipo(interface.to_owned()),
                "`{escrito}` precisa virar `{interface}`"
            );
        }
        // `any` não tem interface por trás, e inventar uma seria oferecer
        // membros que ninguém declarou.
        let qualquer = "const v: any = null!;\nv.\n";
        assert_eq!(
            receptor_em(&parser(), qualquer, 1, 2),
            Receptor::Desconhecido
        );
    }

    /// **`Pedido[]` é um `Array`, e não um `Pedido`.**
    ///
    /// Antes desta correção o `[]` era jogado fora e o receptor virava o tipo do
    /// elemento. Num projeto que declara `Pedido`, `pedidos.` ofereceria `total`
    /// e `cliente` no lugar de `map` e `length` — resposta errada, e não uma
    /// ausência de resposta.
    #[test]
    fn an_array_annotation_is_an_array_and_not_its_element() {
        let anotado = "const pedidos: Pedido[] = [];\npedidos.\n";
        assert_eq!(
            receptor_em(&parser(), anotado, 1, 8),
            Receptor::Tipo("Array".to_owned())
        );
        let generico = "const pedidos: Array<Pedido> = [];\npedidos.\n";
        assert_eq!(
            receptor_em(&parser(), generico, 1, 8),
            Receptor::Tipo("Array".to_owned())
        );
        // Uma tupla não é um `Array` para o verificador de tipos, mas tem os
        // membros de um — e é isso que esta pergunta quer saber.
        let tupla = "const par: [number, string] = [1, ''];\npar.\n";
        assert_eq!(
            receptor_em(&parser(), tupla, 1, 4),
            Receptor::Tipo("Array".to_owned())
        );
    }

    /// **O resultado de uma chamada é desconhecido, e não vazio.**
    ///
    /// É a terceira resposta que a `25` exige. Devolver lista vazia diria "este
    /// tipo não tem membros", que é uma afirmação — e uma falsa.
    #[test]
    fn the_result_of_a_call_is_unknown_and_not_empty() {
        let texto = "const p = buscar();\nbuscar().\n";
        assert_eq!(receptor_em(&parser(), texto, 1, 9), Receptor::Desconhecido);
    }

    /// **O segundo elo de uma cadeia diz de quem e qual membro.**
    ///
    /// Em `this.svc.`, o tipo de `svc` está escrito onde `svc` é declarado, e
    /// achar isso atravessa módulos e herança. O que este módulo entrega é o
    /// endereço da pergunta: o tipo de `this`, e o nome `svc`.
    #[test]
    fn the_second_link_of_a_chain_says_whose_and_which() {
        let texto = "class P {\n  m() {\n    this.svc.\n  }\n}\n";
        assert_eq!(
            receptor_em(&parser(), texto, 2, 13),
            Receptor::Membro {
                tipo: "P".to_owned(),
                membro: "svc".to_owned()
            }
        );
    }

    /// **E a chamada também**, porque método agora guarda o que devolve.
    ///
    /// `this.buscar().` é o outro metade dos elos de cadeia num código Angular.
    /// Os parênteses são contados, e não procurados de trás para frente: um
    /// argumento que seja outra chamada tem parênteses dentro.
    #[test]
    fn a_call_is_a_link_too() {
        let simples = "class P {\n  m() {\n    this.buscar().\n  }\n}\n";
        assert_eq!(
            receptor_em(&parser(), simples, 2, 18),
            Receptor::Membro {
                tipo: "P".to_owned(),
                membro: "buscar".to_owned()
            }
        );
        let aninhada = "class P {\n  m() {\n    this.buscar(a, f(b)).\n  }\n}\n";
        assert_eq!(
            receptor_em(&parser(), aninhada, 2, 25),
            Receptor::Membro {
                tipo: "P".to_owned(),
                membro: "buscar".to_owned()
            }
        );
    }

    /// **Um passo, e não a cadeia inteira.**
    ///
    /// `a.b.c.` exigiria o tipo de `b`, que é resposta desta mesma pergunta um
    /// nível acima. `a.b.c.d.` é raro; `this.campo.` é o dia inteiro.
    #[test]
    fn the_third_link_is_still_unknown() {
        let texto = "class P {\n  m() {\n    this.svc.cliente.\n  }\n}\n";
        assert_eq!(receptor_em(&parser(), texto, 2, 21), Receptor::Desconhecido);
    }

    /// Um elo cujo receptor não se conhece continua desconhecido.
    ///
    /// Sem isto, `qualquer.coisa.` viraria um endereço de pergunta que ninguém
    /// pode responder — e a resposta viria vazia, que **afirma** que o tipo não
    /// tem membros.
    #[test]
    fn a_link_on_an_unknown_receiver_stays_unknown() {
        let texto = "function f() {\n  qualquer.coisa.\n}\n";
        assert_eq!(receptor_em(&parser(), texto, 1, 17), Receptor::Desconhecido);
    }

    /// O tipo escrito ao lado de um membro vira o nome de um tipo.
    ///
    /// É o que transforma o `detail` de um membro em receptor do elo seguinte.
    #[test]
    fn the_written_type_of_a_member_becomes_a_type_name() {
        let parser = parser();
        for (escrito, esperado) in [
            ("Pedido", Some("Pedido")),
            ("Observable<Pedido>", Some("Observable")),
            ("Pedido[]", Some("Array")),
            ("string", Some("String")),
            // Uma união com `null` continua sendo o tipo que sobra: é o que
            // quem digita o ponto quer.
            ("Pedido | null", Some("Pedido")),
            ("Pedido | undefined", Some("Pedido")),
            // Duas coisas de verdade não são nenhuma delas, e responder com a
            // primeira seria a resposta errada com a cara da certa.
            ("string | number", None),
            ("", None),
        ] {
            assert_eq!(
                nome_do_tipo_escrito(&parser, escrito).as_deref(),
                esperado,
                "`{escrito}`"
            );
        }
    }

    /// Sem ponto nenhum, a pergunta não é de acesso a membro.
    #[test]
    fn without_a_dot_there_is_no_question() {
        let texto = "const p = 1;\n";
        assert_eq!(receptor_em(&parser(), texto, 0, 8), Receptor::Nenhum);
    }

    /// Os membros de uma classe são os campos e os métodos.
    #[test]
    fn the_members_of_a_class_are_its_fields_and_methods() {
        let texto = "export class Pedido {\n  total = 0;\n  cliente: string;\n  somar(v: number) {}\n  constructor() {}\n}\n";
        let Some(membros) = membros_de(&parser(), texto, "Pedido", Alcance::Dentro) else {
            panic!("a classe precisa ser achada");
        };
        let nomes: Vec<_> = membros.itens.iter().map(|item| item.label.as_str()).collect();
        assert!(nomes.contains(&"total") && nomes.contains(&"cliente") && nomes.contains(&"somar"));
        assert!(
            !nomes.contains(&"constructor"),
            "o construtor não se acessa por ponto"
        );
    }

    /// **Método guarda o que devolve, e não só o nome.**
    ///
    /// A gramática põe o tipo de uma propriedade em `type` e o de um método em
    /// `return_type`, e a extração lia só o primeiro: `buscar(): Pedido` guardava
    /// `buscar` e perdia `Pedido`.
    ///
    /// Parecia cosmético — o método aparecia na lista sem dizer o que devolve —,
    /// mas é deste campo que sai o receptor do elo seguinte de uma cadeia. Sem
    /// ele, `this.buscar().` nunca teria resposta.
    #[test]
    fn a_method_records_what_it_returns() {
        let texto = "export class Servico {\n  \
                     nome: string;\n  \
                     buscar(): Pedido {}\n  \
                     salvar(p: Pedido): void {}\n}\n";
        let Some(membros) = membros_de(&parser(), texto, "Servico", Alcance::Dentro) else {
            panic!("a classe precisa ser achada");
        };
        let tipo_de = |procurado: &str| -> Option<String> {
            membros
                .itens
                .iter()
                .find(|item| item.label == procurado)
                .and_then(|item| item.detail.clone())
        };
        assert_eq!(tipo_de("nome"), Some("string".to_owned()));
        assert_eq!(tipo_de("buscar"), Some("Pedido".to_owned()));
        assert_eq!(tipo_de("salvar"), Some("void".to_owned()));
    }

    /// O mesmo numa interface, onde a espécie do nó é outra.
    ///
    /// `method_signature` e `method_definition` são nós diferentes na gramática,
    /// e um conserto que só alcance um dos dois deixa metade do caso de pé.
    #[test]
    fn an_interface_method_records_it_too() {
        let texto = "export interface Servico {\n  buscar(): Pedido;\n  nome: string;\n}\n";
        let Some(membros) = membros_de(&parser(), texto, "Servico", Alcance::Dentro) else {
            panic!("a interface precisa ser achada");
        };
        let buscar = membros.itens.iter().find(|item| item.label == "buscar");
        assert_eq!(
            buscar.and_then(|item| item.detail.clone()),
            Some("Pedido".to_owned())
        );
    }

    /// **Parâmetro de construtor com modificador é membro da classe.**
    ///
    /// É o idioma de injeção de Angular, e ele faltava por inteiro: `svc.`
    /// sozinho funcionava — quem resolve o receptor olha os parâmetros do
    /// construtor —, mas `this.svc.` não, porque esta lista não o tinha.
    ///
    /// Sem modificador não é membro: o parâmetro morre quando o construtor
    /// termina, e oferecê-lo em `this.` seria um nome que não existe.
    #[test]
    fn a_constructor_parameter_with_a_modifier_is_a_field() {
        let texto = "export class Pagina {\n  \
                     constructor(private readonly svc: LoginService, publico: Outro) {}\n}\n";
        let Some(membros) = membros_de(&parser(), texto, "Pagina", Alcance::Dentro) else {
            panic!("a classe precisa ser achada");
        };
        let nomes: Vec<_> = membros.itens.iter().map(|item| item.label.as_str()).collect();
        assert!(nomes.contains(&"svc"), "o injetado é membro: {nomes:?}");
        assert!(
            !nomes.contains(&"publico"),
            "sem modificador não é membro: {nomes:?}"
        );
        let tipo = membros
            .itens
            .iter()
            .find(|item| item.label == "svc")
            .and_then(|item| item.detail.clone());
        assert_eq!(
            tipo,
            Some("LoginService".to_owned()),
            "o tipo precisa vir junto, senão o elo seguinte não anda"
        );
    }

    /// **`static` não aparece depois de um ponto, e `private` só de dentro.**
    ///
    /// Medido no projeto de referência: em **45 de 96** pontos que o índice
    /// respondia, ele oferecia a mais do que o `tsserver` — e era sempre uma
    /// destas duas coisas. Sugerir `ɵfac`, que o Angular declara `static` em
    /// toda classe gerada, é sugerir código que não compila.
    ///
    /// Nada aqui é de Angular: as duas regras são da linguagem. O que o projeto
    /// real deu foi o **caso** que as revelou.
    #[test]
    fn static_never_shows_and_private_only_from_inside() {
        let texto = "export class Servico {\n  \
                     publico = 1;\n  \
                     private escondido = 2;\n  \
                     protected herdavel = 3;\n  \
                     static ɵfac: unknown;\n  \
                     static criar(): Servico { return null!; }\n}\n";
        let nomes = |alcance: Alcance| -> Vec<String> {
            membros_de(&parser(), texto, "Servico", alcance)
                .map(|membros| membros.itens.into_iter().map(|item| item.label).collect())
                .unwrap_or_default()
        };

        let de_fora = nomes(Alcance::DeFora);
        assert!(de_fora.contains(&"publico".to_owned()), "veio: {de_fora:?}");
        assert!(
            !de_fora.contains(&"escondido".to_owned())
                && !de_fora.contains(&"herdavel".to_owned()),
            "o que é privado não se vê de fora: {de_fora:?}"
        );

        let de_dentro = nomes(Alcance::Dentro);
        assert!(
            de_dentro.contains(&"escondido".to_owned())
                && de_dentro.contains(&"herdavel".to_owned()),
            "de dentro, o privado é legítimo: {de_dentro:?}"
        );

        // Estático não aparece de lugar nenhum: ele não existe na instância.
        for lista in [&de_fora, &de_dentro] {
            assert!(
                !lista.contains(&"ɵfac".to_owned()) && !lista.contains(&"criar".to_owned()),
                "estático não está numa instância: {lista:?}"
            );
        }
    }

    /// O parâmetro de construtor privado segue a mesma regra do campo privado.
    ///
    /// Era o `useNonNullable` do `FormBuilder` do Angular aparecendo em
    /// `this.formBuilder.` — privado numa classe que não é a de quem pergunta.
    #[test]
    fn a_private_constructor_property_follows_the_same_rule() {
        let texto = "export class Pagina {\n  \
                     constructor(private oculto: Svc, public visivel: Svc) {}\n}\n";
        let nomes = |alcance: Alcance| -> Vec<String> {
            membros_de(&parser(), texto, "Pagina", alcance)
                .map(|membros| membros.itens.into_iter().map(|item| item.label).collect())
                .unwrap_or_default()
        };
        let de_fora = nomes(Alcance::DeFora);
        assert!(de_fora.contains(&"visivel".to_owned()), "veio: {de_fora:?}");
        assert!(!de_fora.contains(&"oculto".to_owned()), "veio: {de_fora:?}");
        assert!(nomes(Alcance::Dentro).contains(&"oculto".to_owned()));
    }

    /// O que a classe herda é dito, para quem resolve seguir a cadeia.
    #[test]
    fn what_the_class_inherits_is_reported() {
        let texto = "class Especial extends Base implements Coisa {\n  proprio = 1;\n}\n";
        let Some(membros) = membros_de(&parser(), texto, "Especial", Alcance::Dentro) else {
            panic!("a classe precisa ser achada");
        };
        assert!(membros.herda.contains(&"Base".to_owned()));
        assert!(membros.herda.contains(&"Coisa".to_owned()));
    }

    /// Interface também tem membros.
    #[test]
    fn an_interface_has_members_too() {
        let texto = "export interface Resumo {\n  total: number;\n  descrever(): string;\n}\n";
        let Some(membros) = membros_de(&parser(), texto, "Resumo", Alcance::Dentro) else {
            panic!("a interface precisa ser achada");
        };
        let nomes: Vec<_> = membros.itens.iter().map(|item| item.label.as_str()).collect();
        assert!(nomes.contains(&"total") && nomes.contains(&"descrever"));
    }
}
