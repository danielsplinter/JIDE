//! Trocar de projeto sem fechar a IDE.
//!
//! Dois defeitos relatados por quem usa: a janela congelava, e quando não
//! congelava o texto abria sem cor. Os dois nasciam do mesmo lugar — a troca
//! chamava `shutdown()`, que **desabilita** cada provider com worker de pé e
//! apaga as rotas dele, e depois reabilitava **um**, pelo nome, escrito no
//! código da aplicação.
//!
//! O resultado era o inverso do esperado: as linguagens que estavam em uso eram
//! exatamente as que morriam.

use std::{path::PathBuf, sync::Arc};

use ide_domain::{DocumentId, DocumentSnapshot, RequestId, SyntaxHighlightKind};
use ide_language_api::{
    CancellationToken, LanguageCapabilities, LanguageProvider, LanguageRequestContext,
};
use ide_language_host::LanguageHost;

fn success<T, E: std::fmt::Debug>(result: Result<T, E>) -> T {
    match result {
        Ok(value) => value,
        Err(error) => panic!("operação deveria funcionar: {error:?}"),
    }
}

fn context(id: u64) -> LanguageRequestContext {
    LanguageRequestContext {
        request_id: RequestId(id),
        cancellation: CancellationToken::new(),
    }
}

fn host_com_duas(raiz: &str) -> LanguageHost {
    let host = LanguageHost::new(raiz);
    let java: Arc<dyn LanguageProvider> = Arc::new(language_java::JavaLanguageProvider::new());
    let typescript: Arc<dyn LanguageProvider> =
        Arc::new(language_typescript::TypeScriptLanguageProvider::new());
    success(host.register(java));
    success(host.register(typescript));
    host
}

fn documento(id: u64, caminho: &str, texto: &str) -> DocumentSnapshot {
    DocumentSnapshot {
        id: DocumentId(id),
        path: PathBuf::from(caminho),
        version: 1,
        text: texto.to_owned(),
    }
}

fn tem_palavra_reservada(host: &LanguageHost, id: u64, documento: DocumentId) -> bool {
    match pollster::block_on(host.syntax(context(id), documento)) {
        Ok(snapshot) => snapshot
            .highlights
            .iter()
            .any(|span| span.kind == SyntaxHighlightKind::Keyword),
        Err(_) => false,
    }
}

/// Depois de trocar a raiz, **as duas linguagens continuam respondendo**.
///
/// É o defeito da cor, na forma em que ele acontecia: o `.ts` estava em uso — e
/// por isso tinha worker de pé —, e era justamente ele que a troca derrubava.
#[test]
fn trocar_de_raiz_nao_desliga_as_linguagens_em_uso() {
    let host = host_com_duas("/antigo");

    // Os dois em uso, com worker de pé: é essa a condição que fazia o defeito
    // aparecer, e um provider que nunca subiu não a reproduz.
    success(pollster::block_on(host.open_document(
        context(1),
        documento(1, "/antigo/Pedido.java", "class Pedido { int total; }"),
    )));
    success(pollster::block_on(host.open_document(
        context(2),
        documento(2, "/antigo/pedido.ts", "export class Pedido {}"),
    )));
    assert!(tem_palavra_reservada(&host, 3, DocumentId(1)));
    assert!(tem_palavra_reservada(&host, 4, DocumentId(2)));

    // A troca de projeto, como a aplicação a faz.
    success(host.set_workspace_root("/novo"));
    success(host.set_source_roots(Vec::new()));
    let soltos = success(host.detach_workers());
    assert!(
        !soltos.is_empty(),
        "os workers em uso precisam ter sido soltos, senão o teste não prova nada"
    );
    success(pollster::block_on(soltos.shutdown()));

    // Os documentos do projeto novo, nas duas linguagens.
    success(pollster::block_on(host.open_document(
        context(5),
        documento(10, "/novo/Cliente.java", "class Cliente { int id; }"),
    )));
    success(pollster::block_on(host.open_document(
        context(6),
        documento(11, "/novo/cliente.ts", "export class Cliente {}"),
    )));

    assert!(
        tem_palavra_reservada(&host, 7, DocumentId(10)),
        "o `.java` do projeto novo precisa vir com realce"
    );
    assert!(
        tem_palavra_reservada(&host, 8, DocumentId(11)),
        "o `.ts` do projeto novo precisa vir com realce — era esta a linha que \
         falhava, porque a troca desligava quem estava em uso e reabilitava só \
         uma linguagem, pelo nome"
    );
}

/// O caminho antigo, para provar que o defeito era este.
///
/// `shutdown()` **desabilita** cada provider com worker de pé e apaga as rotas
/// dele. Reabilitar um pelo nome — como a troca de projeto fazia — deixa todos
/// os outros mudos, e o `.ts` do projeto novo abria sem cor.
///
/// Este teste não guarda comportamento desejado: ele guarda a **razão** de
/// `detach_workers` existir. Sem ele, alguém simplifica de volta para
/// `shutdown()` e o defeito volta calado.
#[test]
fn desligar_e_reabilitar_um_pelo_nome_cala_os_outros() {
    let host = host_com_duas("/antigo");
    success(pollster::block_on(host.open_document(
        context(1),
        documento(1, "/antigo/pedido.ts", "export class Pedido {}"),
    )));
    assert!(tem_palavra_reservada(&host, 2, DocumentId(1)));

    // Exatamente o que a troca de projeto fazia antes.
    success(pollster::block_on(host.shutdown()));
    success(host.set_workspace_root("/novo"));
    success(host.enable(&ide_domain::ProviderId(
        language_java::JAVA_PROVIDER_ID.to_owned(),
    )));

    assert!(
        host.provider_for_extension("ts", LanguageCapabilities::SYNTAX)
            .is_err(),
        "é este o defeito: a extensão que estava em uso deixa de ter provider"
    );
    assert!(
        host.provider_for_extension("java", LanguageCapabilities::SYNTAX)
            .is_ok(),
        "e só a linguagem citada pelo nome sobrevive"
    );
}

/// As rotas por extensão sobrevivem à troca.
///
/// `shutdown()` apagava as rotas do provider desabilitado, e sem rota a
/// pergunta não chega a ninguém — nem para dizer que não sabe.
#[test]
fn as_rotas_por_extensao_sobrevivem_a_troca() {
    let host = host_com_duas("/antigo");
    success(pollster::block_on(host.open_document(
        context(1),
        documento(1, "/antigo/pedido.ts", "export class Pedido {}"),
    )));
    let antes = success(host.provider_for_extension("ts", LanguageCapabilities::SYNTAX));

    success(host.set_workspace_root("/novo"));
    success(pollster::block_on(
        success(host.detach_workers()).shutdown(),
    ));

    let depois = success(host.provider_for_extension("ts", LanguageCapabilities::SYNTAX));
    assert_eq!(
        antes, depois,
        "a extensão precisa continuar chegando ao mesmo provider"
    );
}

/// Soltar os workers é imediato; esperar por eles é que pode demorar.
///
/// A separação existe por causa do congelamento: a fila de um worker atende um
/// pedido por vez, e o encerramento fica atrás de uma preparação que pode levar
/// dois minutos. Este teste garante que a troca de estado **não** está do lado
/// que espera — se `detach_workers` voltar a esperar, ele denuncia.
#[test]
fn soltar_os_workers_nao_espera_ninguem() {
    let host = host_com_duas("/antigo");
    success(pollster::block_on(host.open_document(
        context(1),
        documento(1, "/antigo/pedido.ts", "export class Pedido {}"),
    )));

    let marca = std::time::Instant::now();
    let soltos = success(host.detach_workers());
    let soltar = marca.elapsed();

    assert!(!soltos.is_empty());
    assert!(
        soltar < std::time::Duration::from_millis(100),
        "soltar é uma troca de estado sob o lock e precisa ser imediata; \
         levou {soltar:?}"
    );

    // E o provider já está pronto para subir de novo, antes de o antigo morrer:
    // é isso que faz a janela continuar respondendo enquanto a limpeza acontece.
    success(pollster::block_on(host.open_document(
        context(2),
        documento(2, "/novo/cliente.ts", "export class Cliente {}"),
    )));
    assert!(tem_palavra_reservada(&host, 3, DocumentId(2)));
    success(pollster::block_on(soltos.shutdown()));
}
