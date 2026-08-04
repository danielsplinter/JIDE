//! Quem **usa** um nome, e não quem escreve as mesmas letras.
//!
//! É a metade que faltava da fase 3 da `25`. O teste que importa é o mesmo que
//! decidiu a definição: **dois módulos declaram o mesmo nome**. Uma busca por
//! texto listaria os usos dos dois juntos, e acertaria por sorte metade das
//! vezes.

use std::path::{Path, PathBuf};

use ide_domain::{DocumentId, DocumentSnapshot, ReferencesRequest, TextPosition};
use ide_language_api::{ActiveLanguage, LanguageActivationContext, LanguageProvider};
use language_typescript::TypeScriptLanguageProvider;

fn projeto(nome: &str) -> PathBuf {
    let raiz = std::env::temp_dir().join(format!("er-ts-ref-{nome}-{}", std::process::id()));
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
        Err(erro) => panic!("o provider precisa ativar: {erro}"),
    }
}

/// Abre o arquivo e pergunta quem usa o nome sob a posição.
fn usos(
    ativo: &dyn ActiveLanguage,
    caminho: &Path,
    linha: u32,
    coluna: u32,
    com_declaracao: bool,
) -> Vec<(String, u32)> {
    let Ok(texto) = std::fs::read_to_string(caminho) else {
        panic!("o arquivo do teste precisa existir: {caminho:?}");
    };
    assert!(
        pollster::block_on(ativo.open_document(DocumentSnapshot {
            id: DocumentId(1),
            path: caminho.to_path_buf(),
            version: 1,
            text: texto,
        }))
        .is_ok()
    );
    let achadas = match pollster::block_on(ativo.references(ReferencesRequest {
        document_id: DocumentId(1),
        position: TextPosition {
            line: linha,
            column: coluna,
        },
        include_declaration: com_declaracao,
    })) {
        Ok(achadas) => achadas,
        Err(erro) => panic!("as referências precisam responder: {erro}"),
    };
    let mut resumo: Vec<(String, u32)> = achadas
        .into_iter()
        .map(|local| {
            (
                local
                    .path
                    .file_name()
                    .and_then(|nome| nome.to_str())
                    .unwrap_or_default()
                    .to_owned(),
                local.range.start.line,
            )
        })
        .collect();
    resumo.sort();
    resumo
}

/// **O teste que importa.** Dois módulos declaram `LoginService`.
///
/// Uma busca por texto acharia os usos dos dois. Quem decide é o `import` de
/// cada arquivo que usa.
#[test]
fn the_same_name_in_two_modules_does_not_mix_the_uses() {
    let raiz = projeto("homonimos");
    escrever(
        &raiz.join("src/auth/login.service.ts"),
        "export class LoginService {}\n",
    );
    escrever(
        &raiz.join("src/legado/login.service.ts"),
        "export class LoginService {}\n",
    );
    // Usa o de `auth`.
    let usa_auth = raiz.join("src/app/painel.component.ts");
    escrever(
        &usa_auth,
        "import { LoginService } from '../auth/login.service';\n\
         export class Painel {\n\
         \x20 constructor(private servico: LoginService) {}\n\
         }\n",
    );
    // Usa o do legado, e menciona o nome tantas vezes quanto o outro.
    escrever(
        &raiz.join("src/velho/tela.component.ts"),
        "import { LoginService } from '../legado/login.service';\n\
         export class Tela {\n\
         \x20 constructor(private servico: LoginService) {}\n\
         }\n",
    );

    let ativo = ativado(&raiz);
    // No `LoginService` importado pelo painel.
    let achadas = usos(ativo.as_ref(), &usa_auth, 0, 10, true);
    let arquivos: Vec<&str> = achadas.iter().map(|(nome, _)| nome.as_str()).collect();
    assert!(
        arquivos.contains(&"painel.component.ts"),
        "quem usa precisa entrar: {achadas:?}"
    );
    assert!(
        !arquivos.contains(&"tela.component.ts"),
        "o homônimo do outro módulo não pode entrar: {achadas:?}"
    );
}

/// A declaração entra ou não, conforme quem pergunta pediu.
#[test]
fn the_declaration_is_included_only_when_asked() {
    let raiz = projeto("com-e-sem");
    let servico = raiz.join("src/pedido.service.ts");
    escrever(&servico, "export class PedidoService {}\n");
    let usa = raiz.join("src/painel.ts");
    escrever(
        &usa,
        "import { PedidoService } from './pedido.service';\n\
         export const x: PedidoService | null = null;\n",
    );

    let ativo = ativado(&raiz);
    let com = usos(ativo.as_ref(), &usa, 0, 10, true);
    let sem = usos(ativo.as_ref(), &usa, 0, 10, false);
    assert!(
        com.len() > sem.len(),
        "pedir a declaração precisa trazer mais: com={com:?} sem={sem:?}"
    );
    assert!(
        com.contains(&("pedido.service.ts".to_owned(), 0)),
        "a declaração está na primeira linha do serviço: {com:?}"
    );
    assert!(
        !sem.contains(&("pedido.service.ts".to_owned(), 0)),
        "sem pedir, a declaração fica de fora: {sem:?}"
    );
}

/// **Menção não é uso.**
///
/// O nome dentro de um comentário ou de uma string escreve as mesmas letras e
/// não usa nada. É o que separa isto da busca por texto que a IDE já tinha.
#[test]
fn a_mention_in_a_comment_or_string_is_not_a_use() {
    let raiz = projeto("mencao");
    let servico = raiz.join("src/pedido.service.ts");
    escrever(&servico, "export class PedidoService {}\n");
    let usa = raiz.join("src/painel.ts");
    escrever(
        &usa,
        "import { PedidoService } from './pedido.service';\n\
         // PedidoService faz o pedido\n\
         const nota = 'PedidoService';\n\
         export const x: PedidoService | null = null;\n",
    );

    let ativo = ativado(&raiz);
    let achadas = usos(ativo.as_ref(), &usa, 0, 10, false);
    let linhas: Vec<u32> = achadas
        .iter()
        .filter(|(arquivo, _)| arquivo == "painel.ts")
        .map(|(_, linha)| *linha)
        .collect();
    assert!(
        !linhas.contains(&1),
        "o comentário não é uso: {achadas:?}"
    );
    assert!(
        !linhas.contains(&2),
        "a string não é uso: {achadas:?}"
    );
    assert!(linhas.contains(&3), "a anotação de tipo é uso: {achadas:?}");
}

/// Um arquivo que nem menciona o nome não é lido duas vezes nem entra na lista.
#[test]
fn a_file_that_never_mentions_the_name_stays_out() {
    let raiz = projeto("alheio");
    let servico = raiz.join("src/pedido.service.ts");
    escrever(&servico, "export class PedidoService {}\n");
    escrever(&raiz.join("src/outro.ts"), "export const nada = 1;\n");
    let usa = raiz.join("src/painel.ts");
    escrever(
        &usa,
        "import { PedidoService } from './pedido.service';\n\
         export const x: PedidoService | null = null;\n",
    );

    let ativo = ativado(&raiz);
    let achadas = usos(ativo.as_ref(), &usa, 0, 10, true);
    assert!(
        !achadas.iter().any(|(arquivo, _)| arquivo == "outro.ts"),
        "{achadas:?}"
    );
}
