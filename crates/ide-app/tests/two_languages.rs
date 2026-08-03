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

/// A IDE sabe que uma linguagem prepara o projeto sem saber o que ela prepara.
///
/// # O que este teste guarda
///
/// O analisador de TypeScript não responde **nada** enquanto monta o projeto:
/// medido contra 8 958 arquivos, 30 s de silêncio. Sem um sinal, a IDE não tinha
/// como distinguir "ainda não" de "não tem" — e quem usa concluía que a IDE
/// estava quebrada.
///
/// O sinal é lido **sem passar pela fila do worker**, e é isso que o torna útil:
/// a fila atende um pedido por vez, e a pergunta ficaria atrás justamente do
/// trabalho sobre o qual se pergunta.
///
/// # E é neutro
///
/// Uma linguagem que não tem o que preparar — todo provider nativo — nunca
/// aparece como preparando. A IDE não pergunta "o tsserver terminou?"; ela
/// pergunta se **alguma** linguagem ainda prepara o projeto.
#[test]
fn the_ide_knows_a_language_is_preparing_without_knowing_what() {
    let host = LanguageHost::new("/w");
    let sinal = ide_language_api::ReadinessSignal::new();
    let provider: Arc<dyn LanguageProvider> =
        Arc::new(PreparingProvider::new("preparando", sinal.clone()));
    success(host.register(provider));

    // Antes de ativar não há worker, e portanto nada a preparar.
    assert!(!host.preparing(), "sem linguagem ativa, nada prepara nada");

    // Ativar é o que entrega o sinal. Abrir um documento ativa.
    success(pollster::block_on(host.open_document(
        context(1),
        DocumentSnapshot {
            id: DocumentId(1),
            path: PathBuf::from("/w/a.preparando"),
            version: 1,
            text: String::new(),
        },
    )));
    assert!(
        host.preparing(),
        "com o sinal ainda não marcado, a linguagem está preparando"
    );

    sinal.mark_ready();
    assert!(
        !host.preparing(),
        "marcado o sinal, não há mais o que esperar"
    );
}

/// Uma linguagem sem nada a preparar nunca aparece como preparando.
#[test]
fn a_language_with_nothing_to_prepare_never_reports_preparing() {
    let host = host_with_both();
    success(pollster::block_on(host.open_document(
        context(1),
        DocumentSnapshot {
            id: DocumentId(1),
            path: PathBuf::from("/w/Pedido.java"),
            version: 1,
            text: "class Pedido {}".to_owned(),
        },
    )));
    assert!(
        !host.preparing(),
        "o provider nativo responde por completo desde a ativação"
    );
}

/// Um provider que entrega um sinal de prontidão, para o teste controlar.
struct PreparingProvider {
    extension: String,
    signal: ide_language_api::ReadinessSignal,
}

impl PreparingProvider {
    fn new(extension: &str, signal: ide_language_api::ReadinessSignal) -> Self {
        Self {
            extension: extension.to_owned(),
            signal,
        }
    }
}

#[async_trait::async_trait]
impl LanguageProvider for PreparingProvider {
    fn metadata(&self) -> ide_language_api::LanguageMetadata {
        ide_language_api::LanguageMetadata {
            language_id: ide_domain::LanguageId(self.extension.clone()),
            provider_id: ide_domain::ProviderId(self.extension.clone()),
            display_name: "Preparando".to_owned(),
            extensions: vec![self.extension.clone()],
            api_version: ide_language_api::LANGUAGE_API_VERSION,
            trigger_characters: Vec::new(),
        }
    }

    fn capabilities(&self) -> LanguageCapabilities {
        LanguageCapabilities::SYNTAX
    }

    async fn activate(
        &self,
        _: ide_language_api::LanguageActivationContext,
    ) -> Result<Box<dyn ide_language_api::ActiveLanguage>, ide_language_api::LanguageError> {
        Ok(Box::new(PreparingLanguage {
            language_id: ide_domain::LanguageId(self.extension.clone()),
            signal: self.signal.clone(),
        }))
    }
}

struct PreparingLanguage {
    language_id: ide_domain::LanguageId,
    signal: ide_language_api::ReadinessSignal,
}

#[async_trait::async_trait]
impl ide_language_api::ActiveLanguage for PreparingLanguage {
    fn language_id(&self) -> &ide_domain::LanguageId {
        &self.language_id
    }

    fn readiness(&self) -> Option<ide_language_api::ReadinessSignal> {
        Some(self.signal.clone())
    }

    async fn open_document(
        &self,
        _: DocumentSnapshot,
    ) -> Result<(), ide_language_api::LanguageError> {
        Ok(())
    }

    async fn change_document(
        &self,
        _: ide_domain::DocumentChange,
    ) -> Result<(), ide_language_api::LanguageError> {
        Ok(())
    }

    async fn close_document(
        &self,
        _: DocumentId,
    ) -> Result<(), ide_language_api::LanguageError> {
        Ok(())
    }

    async fn diagnostics(
        &self,
        _: DocumentId,
    ) -> Result<Vec<ide_domain::Diagnostic>, ide_language_api::LanguageError> {
        Ok(Vec::new())
    }

    async fn shutdown(&self) -> Result<(), ide_language_api::LanguageError> {
        Ok(())
    }
}
