//! Duas linguagens vivas ao mesmo tempo, sem uma responder pela outra.
//!
//! É o critério da fase 1 da `23`, e o que ela vem provar de fato: a IDE dizia
//! ser multilíngue desde a `00` e nunca tivera duas linguagens. Uma afirmação
//! com um exemplo só não foi verificada — foi escrita.

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

fn host_with_both() -> LanguageHost {
    let host = LanguageHost::new("/w");
    let java: Arc<dyn LanguageProvider> = Arc::new(language_java::JavaLanguageProvider::new());
    let typescript: Arc<dyn LanguageProvider> =
        Arc::new(language_typescript::TypeScriptLanguageProvider::new());
    success(host.register(java));
    success(host.register(typescript));
    host
}

/// Cada extensão vai ao seu provider, e nenhuma cai na do vizinho.
#[test]
fn each_extension_reaches_its_own_provider() {
    let host = host_with_both();

    let para_java = success(host.provider_for_extension("java", LanguageCapabilities::SYNTAX));
    let para_ts = success(host.provider_for_extension("ts", LanguageCapabilities::SYNTAX));
    assert_ne!(
        para_java, para_ts,
        "duas extensões não podem cair no mesmo provider"
    );
    assert_eq!(para_java.0, language_java::JAVA_PROVIDER_ID);
    assert_eq!(para_ts.0, language_typescript::TYPESCRIPT_PROVIDER_ID);
}

/// Abrir um `.java` e um `.ts` dá realce nos dois, sem interferência.
#[test]
fn both_files_are_highlighted_side_by_side() {
    let host = host_with_both();

    let java = DocumentSnapshot {
        id: DocumentId(1),
        path: PathBuf::from("/w/Pedido.java"),
        version: 1,
        text: "class Pedido { int total; }".to_owned(),
    };
    let typescript = DocumentSnapshot {
        id: DocumentId(2),
        path: PathBuf::from("/w/pedido.ts"),
        version: 1,
        text: "export class Pedido { total: number = 0; }".to_owned(),
    };
    success(pollster::block_on(host.open_document(context(1), java)));
    success(pollster::block_on(
        host.open_document(context(2), typescript),
    ));

    let realce_java = success(pollster::block_on(host.syntax(context(3), DocumentId(1))));
    let realce_ts = success(pollster::block_on(host.syntax(context(4), DocumentId(2))));

    for (nome, snapshot) in [("java", &realce_java), ("typescript", &realce_ts)] {
        assert!(
            snapshot
                .highlights
                .iter()
                .any(|span| span.kind == SyntaxHighlightKind::Keyword),
            "o realce de {nome} precisa reconhecer palavra reservada"
        );
        assert!(
            snapshot.outline.iter().any(|item| item.name == "Pedido"),
            "a estrutura de {nome} precisa listar o tipo declarado"
        );
    }

    // O documento fica preso ao provider que o aceitou: fechar um não pode
    // apagar o outro, e trocar de aba não confunde a rota.
    success(pollster::block_on(
        host.close_document(context(5), DocumentId(2)),
    ));
    assert!(
        pollster::block_on(host.syntax(context(6), DocumentId(1))).is_ok(),
        "fechar um `.ts` não pode calar o `.java` aberto"
    );
}

/// Uma extensão sem provider não é atendida por quem estiver por perto.
#[test]
fn an_unknown_extension_reaches_nobody() {
    let host = host_with_both();
    assert!(
        host.provider_for_extension("md", LanguageCapabilities::SYNTAX)
            .is_err(),
        "um `.md` não pode cair no provider de outra linguagem"
    );
}

/// Com os dois providers de TypeScript registrados, o realce ainda responde?
///
/// O nativo tem SYNTAX; o externo tem COMPLETION, DEFINITION e DIAGNOSTICS. Se o
/// documento fica preso a um provider só, o realce depende de qual deles pegou o
/// arquivo — e a ordem declarada põe o externo na frente.
#[test]
fn syntax_still_answers_with_both_typescript_providers() {
    let host = LanguageHost::new("/w");
    let nativo: Arc<dyn LanguageProvider> =
        Arc::new(language_typescript::TypeScriptLanguageProvider::new());
    let externo: Arc<dyn LanguageProvider> =
        Arc::new(language_typescript::TypeScriptServiceProvider::new(Arc::new(
            ide_process::NativeProcessSupervisor::default(),
        )));
    success(host.register(nativo));
    success(host.register(externo));
    success(host.configure_selection(
        ide_domain::LanguageId("typescript".to_owned()),
        ide_language_host::ProviderSelection {
            primary: ide_domain::ProviderId(
                language_typescript::TYPESCRIPT_SERVICE_PROVIDER_ID.to_owned(),
            ),
            fallbacks: vec![ide_domain::ProviderId(
                language_typescript::TYPESCRIPT_PROVIDER_ID.to_owned(),
            )],
        },
    ));

    let documento = DocumentSnapshot {
        id: DocumentId(9),
        path: PathBuf::from("/w/pedido.ts"),
        version: 1,
        text: "export class Pedido {}".to_owned(),
    };
    success(pollster::block_on(
        host.open_document(context(9), documento),
    ));
    let realce = pollster::block_on(host.syntax(context(10), DocumentId(9)));
    assert!(
        realce.is_ok(),
        "com os dois registrados, o realce precisa continuar respondendo: {realce:?}"
    );
}
