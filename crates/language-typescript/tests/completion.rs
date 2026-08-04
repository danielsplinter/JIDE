//! O ponto, quando o receptor tem tipo declarado.
//!
//! É o critério da fase 4 da `25`: num componente Angular, `this.` e um
//! parâmetro de construtor injetado completam com os membros certos, **sem o
//! analisador de pé**. E `.pipe(map(x => x.` responde que não soube.
//!
//! # A terceira resposta é o que se testa aqui
//!
//! Lista vazia é uma **afirmação**: "este tipo não tem membros". Dizê-la quando
//! na verdade não se sabe o tipo é a família de defeito que esta IDE encontrou
//! cinco vezes — a resposta errada com a mesma cara da certa. Por isso metade
//! destes testes cobra o erro, e não o acerto.

use std::path::{Path, PathBuf};

use ide_domain::{CompletionRequest, DocumentId, DocumentSnapshot, TextPosition};
use ide_language_api::{
    ActiveLanguage, LanguageActivationContext, LanguageError, LanguageProvider,
};
use language_typescript::TypeScriptLanguageProvider;

fn projeto(nome: &str) -> PathBuf {
    let raiz = std::env::temp_dir().join(format!("er-ts-comp-{nome}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&raiz);
    assert!(std::fs::create_dir_all(&raiz).is_ok());
    raiz
}

fn escrever(caminho: &Path, conteudo: &str) {
    if let Some(pasta) = caminho.parent() {
        assert!(std::fs::create_dir_all(pasta).is_ok());
    }
    assert!(std::fs::write(caminho, conteudo).is_ok());
}

fn ativado(raiz: &Path) -> Box<dyn ActiveLanguage> {
    match pollster::block_on(TypeScriptLanguageProvider::new().activate(
        LanguageActivationContext {
            workspace_root: raiz.to_path_buf(),
            source_roots: Vec::new(),
            toolchains: Vec::new(),
        },
    )) {
        Ok(ativo) => ativo,
        Err(erro) => panic!("o provider nativo precisa ativar: {erro}"),
    }
}

/// A posição logo depois de um trecho, achada no próprio texto.
///
/// Contar coluna à mão erra, e o erro se disfarça de defeito do código: a
/// primeira versão destes testes apontava uma coluna antes do ponto, e a
/// resposta "não há pergunta aqui" parecia "não soube responder".
fn depois_de(texto: &str, trecho: &str) -> TextPosition {
    for (numero, linha) in texto.lines().enumerate() {
        if let Some(byte) = linha.find(trecho) {
            return TextPosition {
                line: numero as u32,
                column: (linha[..byte + trecho.len()].chars().count()) as u32,
            };
        }
    }
    panic!("o trecho {trecho:?} precisa estar no texto");
}

/// Abre o arquivo e pede a completação logo depois de um trecho.
fn completar(
    ativo: &dyn ActiveLanguage,
    caminho: &Path,
    trecho: &str,
) -> Result<Vec<String>, LanguageError> {
    let Ok(texto) = std::fs::read_to_string(caminho) else {
        panic!("o arquivo do teste precisa existir: {caminho:?}");
    };
    let position = depois_de(&texto, trecho);
    assert!(
        pollster::block_on(ativo.open_document(DocumentSnapshot {
            id: DocumentId(1),
            path: caminho.to_path_buf(),
            version: 1,
            text: texto,
        }))
        .is_ok()
    );
    pollster::block_on(ativo.completion(CompletionRequest {
        document_id: DocumentId(1),
        position,
        prefix: String::new(),
    }))
    .map(|itens| itens.into_iter().map(|item| item.label).collect())
}

/// **O critério.** Num componente, `this.` e o serviço injetado completam.
#[test]
fn in_a_component_this_and_the_injected_service_complete() {
    let raiz = projeto("componente");
    escrever(
        &raiz.join("src/login.service.ts"),
        "export class LoginService {\n  entrar(usuario: string) {}\n  sair() {}\n}\n",
    );
    escrever(
        &raiz.join("src/pagina.component.ts"),
        "import { LoginService } from './login.service';\n\
         export class PaginaComponent {\n\
        \x20 titulo = 'oi';\n\
        \x20 constructor(private login: LoginService) {}\n\
        \x20 abrir() {\n\
        \x20   this.\n\
        \x20   login.\n\
        \x20 }\n\
         }\n",
    );
    let ativo = ativado(&raiz);
    let arquivo = raiz.join("src/pagina.component.ts");

    let proprios = match completar(ativo.as_ref(), &arquivo, "this.") {
        Ok(itens) => itens,
        Err(erro) => panic!("`this.` precisa completar: {erro}"),
    };
    assert!(
        proprios.contains(&"titulo".to_owned()) && proprios.contains(&"abrir".to_owned()),
        "os membros da própria classe: {proprios:?}"
    );

    let injetado = match completar(ativo.as_ref(), &arquivo, "login.") {
        Ok(itens) => itens,
        Err(erro) => panic!("o serviço injetado precisa completar: {erro}"),
    };
    assert!(
        injetado.contains(&"entrar".to_owned()) && injetado.contains(&"sair".to_owned()),
        "os membros do serviço, que está noutro arquivo: {injetado:?}"
    );
    let _ = std::fs::remove_dir_all(&raiz);
}

/// **O critério da fase 8.** `this.campo.` completa com os membros do campo.
///
/// É onde estava um quarto de tudo o que a IDE não respondia: em `this.svc.`, o
/// `this.svc` era alcançável e o resto não. Num componente Angular quase tudo se
/// acessa por `this.`, e o segundo ponto é o gesto mais comum que existe.
#[test]
fn the_second_link_of_a_chain_completes() {
    let raiz = projeto("segundo-elo");
    escrever(
        &raiz.join("src/login.service.ts"),
        "export class LoginService {\n  entrar(usuario: string) {}\n  sair() {}\n}\n",
    );
    escrever(
        &raiz.join("src/pagina.component.ts"),
        "import { LoginService } from './login.service';\n\
         export class PaginaComponent {\n\
        \x20 private login: LoginService;\n\
        \x20 abrir() {\n\
        \x20   this.login.\n\
        \x20 }\n\
         }\n",
    );
    let ativo = ativado(&raiz);
    let itens = match completar(
        ativo.as_ref(),
        &raiz.join("src/pagina.component.ts"),
        "this.login.",
    ) {
        Ok(itens) => itens,
        Err(erro) => panic!("`this.login.` precisa completar: {erro}"),
    };
    assert!(
        itens.contains(&"entrar".to_owned()) && itens.contains(&"sair".to_owned()),
        "os membros do serviço, alcançados pelo segundo elo: {itens:?}"
    );
    let _ = std::fs::remove_dir_all(&raiz);
}

/// **E a chamada também**, que é a outra metade dos elos.
///
/// `this.buscar().` só tem resposta porque método passou a guardar o tipo de
/// retorno — a correção que precedeu esta fase. Sem ela, isto responderia "não
/// sei" enquanto `this.campo.` respondia, e nada acusaria a metade que faltava.
#[test]
fn a_call_is_a_link_too() {
    let raiz = projeto("elo-chamada");
    escrever(
        &raiz.join("src/pedido.ts"),
        "export class Pedido {\n  total = 0;\n  somar() {}\n}\n",
    );
    escrever(
        &raiz.join("src/pagina.ts"),
        "import { Pedido } from './pedido';\n\
         export class Pagina {\n\
        \x20 buscar(): Pedido { return null!; }\n\
        \x20 abrir() {\n\
        \x20   this.buscar().\n\
        \x20 }\n\
         }\n",
    );
    let ativo = ativado(&raiz);
    let itens = match completar(ativo.as_ref(), &raiz.join("src/pagina.ts"), "this.buscar().") {
        Ok(itens) => itens,
        Err(erro) => panic!("`this.buscar().` precisa completar: {erro}"),
    };
    assert!(
        itens.contains(&"total".to_owned()) && itens.contains(&"somar".to_owned()),
        "os membros do que o método devolve: {itens:?}"
    );
    let _ = std::fs::remove_dir_all(&raiz);
}

/// **Um campo de tipo da linguagem também**, que é o que faz a fase 7 valer.
///
/// `this.nome.` com `nome: string` só vira resposta se as duas coisas
/// existirem: o elo da cadeia e a `interface String` no índice. Sozinha, nenhuma
/// das duas responde isto.
#[test]
fn a_field_of_a_language_type_completes_through_the_chain() {
    let raiz = projeto("elo-biblioteca");
    let lib = raiz.join("node_modules/typescript/lib");
    escrever(
        &raiz.join("node_modules/typescript/package.json"),
        "{\"version\": \"0.0.0-teste\"}",
    );
    escrever(
        &lib.join("lib.es5.d.ts"),
        "interface String {\n  charAt(i: number): string;\n  trim(): string;\n}\n",
    );
    escrever(
        &lib.join("lib.es2022.full.d.ts"),
        "/// <reference no-default-lib=\"true\"/>\n/// <reference lib=\"es5\" />\n",
    );
    escrever(
        &raiz.join("tsconfig.json"),
        "{ \"compilerOptions\": { \"target\": \"ES2022\" } }",
    );
    escrever(
        &raiz.join("src/pagina.ts"),
        "export class Pagina {\n\
        \x20 nome: string;\n\
        \x20 abrir() {\n\
        \x20   this.nome.\n\
        \x20 }\n\
         }\n",
    );
    let ativo = ativado(&raiz);
    assert!(
        pollster::block_on(ativo.wait_until_indexed(std::time::Duration::from_secs(60))),
        "a preparação do projeto precisa terminar"
    );
    let itens = match completar(ativo.as_ref(), &raiz.join("src/pagina.ts"), "this.nome.") {
        Ok(itens) => itens,
        Err(erro) => panic!("`this.nome.` precisa completar: {erro}"),
    };
    assert!(
        itens.contains(&"charAt".to_owned()) && itens.contains(&"trim".to_owned()),
        "o elo levou até a `interface String`: {itens:?}"
    );
    let _ = std::fs::remove_dir_all(&raiz);
}

/// **A outra metade do critério.** O que ele não sabe, ele diz que não sabe.
///
/// `.pipe(map(x => x.` exige instanciar genéricos e fazer o tipo voltar da
/// assinatura para dentro da lambda. Responder lista vazia seria afirmar que o
/// tipo não tem membros — falso, e indistinguível do certo na tela.
#[test]
fn what_it_does_not_know_it_says_it_does_not_know() {
    let raiz = projeto("desconhecido");
    escrever(
        &raiz.join("src/pagina.ts"),
        "export class Pagina {\n\
        \x20 abrir() {\n\
        \x20   this.buscar().pipe(map(x => x.\n\
        \x20 }\n\
        \x20 buscar() { return null!; }\n\
         }\n",
    );
    let ativo = ativado(&raiz);

    let resposta = completar(ativo.as_ref(), &raiz.join("src/pagina.ts"), "=> x.");
    match resposta {
        Err(LanguageError::Unresolved(motivo)) => {
            assert!(
                motivo.contains("não sei"),
                "a recusa precisa dizer o que não se sabe: {motivo}"
            );
        }
        Err(outro) => panic!("não saber não é falha do provider: {outro:?}"),
        Ok(itens) => panic!(
            "lista vazia afirma que o tipo não tem membros, e isso é falso: {itens:?}"
        ),
    }
    let _ = std::fs::remove_dir_all(&raiz);
}

/// Um tipo de dependência instalada também é "não sei".
#[test]
fn a_type_from_a_dependency_is_also_unknown() {
    let raiz = projeto("dependencia");
    escrever(
        &raiz.join("src/pagina.ts"),
        "import { HttpClient } from '@angular/common/http';\n\
         export class Pagina {\n\
        \x20 constructor(private http: HttpClient) {}\n\
        \x20 abrir() {\n\
        \x20   http.\n\
        \x20 }\n\
         }\n",
    );
    let ativo = ativado(&raiz);

    let resposta = completar(ativo.as_ref(), &raiz.join("src/pagina.ts"), "http.");
    assert!(
        matches!(resposta, Err(LanguageError::Unresolved(_))),
        "o índice não alcança dependência, e diz isso: {resposta:?}"
    );
    let _ = std::fs::remove_dir_all(&raiz);
}

/// Os membros herdados aparecem junto com os próprios.
///
/// Num componente Angular, metade do que vem depois de `this.` está na classe de
/// que ele herda. Uma lista sem isso parece certa e está incompleta.
#[test]
fn inherited_members_show_up_too() {
    let raiz = projeto("heranca");
    escrever(
        &raiz.join("src/base.ts"),
        "export class Base {\n  daBase() {}\n  campoDaBase = 1;\n}\n",
    );
    escrever(
        &raiz.join("src/filha.ts"),
        "import { Base } from './base';\n\
         export class Filha extends Base {\n\
        \x20 propria() {}\n\
        \x20 usar() {\n\
        \x20   this.\n\
        \x20 }\n\
         }\n",
    );
    let ativo = ativado(&raiz);

    let itens = match completar(ativo.as_ref(), &raiz.join("src/filha.ts"), "this.") {
        Ok(itens) => itens,
        Err(erro) => panic!("`this.` precisa completar: {erro}"),
    };
    assert!(
        itens.contains(&"propria".to_owned())
            && itens.contains(&"daBase".to_owned())
            && itens.contains(&"campoDaBase".to_owned()),
        "os herdados vêm junto: {itens:?}"
    );
    let _ = std::fs::remove_dir_all(&raiz);
}

/// O prefixo já digitado filtra a lista.
#[test]
fn the_typed_prefix_filters_the_list() {
    let raiz = projeto("prefixo");
    escrever(
        &raiz.join("src/p.ts"),
        "export class P {\n  abrir() {}\n  abolir() {}\n  fechar() {}\n  usar() {\n    this.ab\n  }\n}\n",
    );
    let ativo = ativado(&raiz);
    let arquivo = raiz.join("src/p.ts");
    let Ok(texto_para_prefixo) = std::fs::read_to_string(&arquivo) else {
        panic!("o arquivo precisa existir");
    };
    let texto = texto_para_prefixo.clone();
    assert!(
        pollster::block_on(ativo.open_document(DocumentSnapshot {
            id: DocumentId(1),
            path: arquivo,
            version: 1,
            text: texto,
        }))
        .is_ok()
    );
    let itens = pollster::block_on(ativo.completion(CompletionRequest {
        document_id: DocumentId(1),
        position: depois_de(&texto_para_prefixo, "this.ab"),
        prefix: "ab".to_owned(),
    }));
    let Ok(itens) = itens else {
        panic!("a completação com prefixo precisa responder: {itens:?}");
    };
    let nomes: Vec<_> = itens.into_iter().map(|item| item.label).collect();
    assert!(nomes.contains(&"abrir".to_owned()) && nomes.contains(&"abolir".to_owned()));
    assert!(!nomes.contains(&"fechar".to_owned()), "o prefixo filtra: {nomes:?}");
    let _ = std::fs::remove_dir_all(&raiz);
}
