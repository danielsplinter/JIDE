//! O analisador externo sobe na pergunta, e não na abertura.
//!
//! É o critério da fase 5 da `25`. O que se cobra é comportamental e medível:
//! abrir um `.ts` **não** põe um processo de pé, e a primeira pergunta que o
//! índice não alcança põe.
//!
//! ```text
//! set IDE_PROJETO_GRANDE=C:\caminho\de\um\projeto\com\node_modules
//! cargo test --release -p ide-app --test analyzer_on_demand -- --ignored --nocapture
//! ```

use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use ide_domain::{CompletionRequest, DocumentId, DocumentSnapshot, TextPosition};
use ide_language_api::LanguageProvider;
use ide_language_host::LanguageHost;
use ide_process::NativeProcessSupervisor;

fn projeto() -> PathBuf {
    let Some(caminho) = std::env::var_os("IDE_PROJETO_GRANDE").map(PathBuf::from) else {
        panic!("aponte IDE_PROJETO_GRANDE para um projeto TypeScript com node_modules");
    };
    assert!(caminho.is_dir(), "o projeto precisa existir: {caminho:?}");
    caminho
}

/// Um `.ts` que sirva aos dois lados do critério.
///
/// Precisa de uma classe declarada — para o índice ter o que saber e responder
/// sem o analisador — **e** de um ponto sobre resultado de chamada, que é o que
/// ele não tipa. Escolher pelo primeiro que aparece deixaria o teste depender da
/// ordem das pastas, que já enganou uma medição desta especificação.
fn algum_ts(root: &Path) -> Option<PathBuf> {
    let mut pilha = vec![root.to_path_buf()];
    while let Some(pasta) = pilha.pop() {
        let entradas = std::fs::read_dir(&pasta).ok()?;
        for entrada in entradas.flatten() {
            let caminho = entrada.path();
            let nome = caminho.file_name()?.to_str()?.to_owned();
            if caminho.is_dir() {
                if nome != "node_modules" && nome != "dist" && !nome.starts_with('.') {
                    pilha.push(caminho);
                }
                continue;
            }
            if nome.ends_with(".ts")
                && !nome.ends_with(".d.ts")
                && std::fs::read_to_string(&caminho).is_ok_and(|texto| {
                    texto.contains("export class ")
                        && posicao_de_um_ponto_dificil(&texto).is_some()
                })
            {
                return Some(caminho);
            }
        }
    }
    None
}

/// **O critério.** Abrir não sobe o analisador; a pergunta que ninguém soube, sim.
#[test]
#[ignore = "exige IDE_PROJETO_GRANDE com node_modules instalado"]
fn opening_does_not_start_the_analyzer_but_an_unanswerable_question_does() {
    let root = projeto();
    let Some(arquivo) = algum_ts(&root) else {
        panic!("o projeto precisa ter um `.ts` com classe declarada");
    };

    let processos = Arc::new(NativeProcessSupervisor::default());
    let host = LanguageHost::new(root.clone());
    let _ = &root;
    let nativo: Arc<dyn LanguageProvider> =
        Arc::new(language_typescript::TypeScriptLanguageProvider::new());
    let externo: Arc<dyn LanguageProvider> = Arc::new(
        language_typescript::TypeScriptServiceProvider::new(
            Arc::clone(&processos) as Arc<dyn ide_process::ProcessSupervisor>,
        ),
    );
    assert!(host.register(nativo).is_ok());
    assert!(host.register(externo).is_ok());
    // A mesma ordem que a IDE compõe: o índice na frente, o analisador atrás.
    assert!(
        host.configure_selection(
            ide_domain::LanguageId(language_typescript::TYPESCRIPT_LANGUAGE_ID.to_owned()),
            ide_language_host::ProviderSelection {
                primary: ide_domain::ProviderId(
                    language_typescript::TYPESCRIPT_PROVIDER_ID.to_owned()
                ),
                fallbacks: vec![ide_domain::ProviderId(
                    language_typescript::TYPESCRIPT_SERVICE_PROVIDER_ID.to_owned()
                )],
            },
        )
        .is_ok()
    );

    let Ok(texto) = std::fs::read_to_string(&arquivo) else {
        panic!("o arquivo precisa ser legível");
    };
    assert!(
        pollster::block_on(host.open_document(
            host.request_context(),
            DocumentSnapshot {
                id: DocumentId(1),
                path: arquivo.clone(),
                version: 1,
                text: texto.clone(),
            },
        ))
        .is_ok()
    );

    assert_eq!(
        processos.live_conversations().len(),
        0,
        "abrir um `.ts` não pode subir processo nenhum: é 1,9 GB e trinta \
         segundos pagos por quem talvez não pergunte nada que exija tipos"
    );

    // Uma pergunta que o índice não alcança: o ponto sobre o resultado de uma
    // chamada, que exige tipo de retorno.
    let Some((numero_da_linha, coluna)) = posicao_de_um_ponto_dificil(&texto) else {
        panic!("o arquivo precisa ter um `.` sobre expressão que o índice não tipa");
    };
    let _ = pollster::block_on(host.completion(
        host.request_context(),
        CompletionRequest {
            document_id: DocumentId(1),
            position: TextPosition {
                line: numero_da_linha,
                column: coluna,
            },
            prefix: String::new(),
        },
    ));

    // A pergunta **anota** quem acordar; quem ativa é o laço de quadros, fora da
    // thread da interface. Aqui o teste faz o papel dele.
    assert_eq!(
        processos.live_conversations().len(),
        0,
        "perguntar não pode subir processo na thread de quem perguntou"
    );
    for provider_id in host.take_pending_activation() {
        assert!(host.activate_provider(&provider_id).is_ok());
    }

    // O processo leva um instante para aparecer na tabela do sistema.
    for _ in 0..40 {
        if !processos.live_conversations().is_empty() {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(
        processos.live_conversations().len(),
        1,
        "a pergunta que ninguém soube é o que faz o analisador valer o que custa"
    );
}

/// O analisador que subiu recebe os documentos, e para de dizer que carrega.
///
/// # O defeito que este teste guarda
///
/// A fase 5 acordava o analisador na pergunta e **não lhe entregava nada**. O
/// `tsserver` não carrega projeto sem um arquivo aberto, então ele ficava de pé
/// sem nada para montar: `projectLoadingFinish` nunca vinha, o sinal de
/// prontidão nunca ficava pronto, e a animação de carregamento girava no meio da
/// tela para sempre.
///
/// Quem tem o texto é a aplicação, e é ela que reoferece — mas o host precisa
/// **dizer** o que falta, e é isso que se cobra aqui.
#[test]
#[ignore = "exige IDE_PROJETO_GRANDE com node_modules instalado"]
fn the_analyzer_that_woke_up_gets_the_documents_it_missed() {
    let root = projeto();
    let Some(arquivo) = algum_ts(&root) else {
        panic!("o projeto precisa ter um `.ts` que sirva aos dois lados");
    };
    let processos = Arc::new(NativeProcessSupervisor::default());
    let host = LanguageHost::new(root.clone());
    let nativo: Arc<dyn LanguageProvider> =
        Arc::new(language_typescript::TypeScriptLanguageProvider::new());
    let externo: Arc<dyn LanguageProvider> = Arc::new(
        language_typescript::TypeScriptServiceProvider::new(
            Arc::clone(&processos) as Arc<dyn ide_process::ProcessSupervisor>,
        ),
    );
    assert!(host.register(nativo).is_ok());
    assert!(host.register(externo).is_ok());
    assert!(
        host.configure_selection(
            ide_domain::LanguageId(language_typescript::TYPESCRIPT_LANGUAGE_ID.to_owned()),
            ide_language_host::ProviderSelection {
                primary: ide_domain::ProviderId(
                    language_typescript::TYPESCRIPT_PROVIDER_ID.to_owned()
                ),
                fallbacks: vec![ide_domain::ProviderId(
                    language_typescript::TYPESCRIPT_SERVICE_PROVIDER_ID.to_owned()
                )],
            },
        )
        .is_ok()
    );

    let Ok(texto) = std::fs::read_to_string(&arquivo) else {
        panic!("o arquivo precisa ser legível");
    };
    let instantaneo = DocumentSnapshot {
        id: DocumentId(1),
        path: arquivo.clone(),
        version: 1,
        text: texto.clone(),
    };
    assert!(
        pollster::block_on(host.open_document(host.request_context(), instantaneo.clone())).is_ok()
    );
    assert!(
        host.documents_missing_providers().is_empty(),
        "com um provider só de pé, não falta documento a ninguém"
    );

    let Some((linha, coluna)) = posicao_de_um_ponto_dificil(&texto) else {
        panic!("o arquivo precisa ter um ponto que o índice não tipa");
    };
    let _ = pollster::block_on(host.completion(
        host.request_context(),
        CompletionRequest {
            document_id: DocumentId(1),
            position: TextPosition { line: linha, column: coluna },
            prefix: String::new(),
        },
    ));
    for provider_id in host.take_pending_activation() {
        assert!(host.activate_provider(&provider_id).is_ok());
    }

    // Ele subiu. E o host precisa dizer que o documento falta a ele.
    assert_eq!(
        host.documents_missing_providers(),
        vec![DocumentId(1)],
        "sem isto, o analisador fica de pé sem projeto para montar, e a animação          de carregamento não para nunca"
    );

    // A aplicação reoferece, e aí não falta mais nada.
    assert!(
        pollster::block_on(host.open_document(host.request_context(), instantaneo)).is_ok()
    );
    assert!(
        host.documents_missing_providers().is_empty(),
        "reoferecido, o documento chegou a quem faltava"
    );
}

/// A posição de um ponto que o índice **não** sabe tipar.
///
/// Duas formas servem, e a segunda existe porque a primeira não aparece em todo
/// projeto: o ponto depois de `)`, que pede tipo de retorno, e o segundo ponto
/// de uma cadeia — `a.b.` —, que pede o tipo de um membro. As duas estão
/// declaradas como fora do alcance na fase 4 da `25`.
fn posicao_de_um_ponto_dificil(texto: &str) -> Option<(u32, u32)> {
    for (numero, linha) in texto.lines().enumerate() {
        if linha.trim_start().starts_with("import ") || linha.trim_start().starts_with("//") {
            continue;
        }
        if let Some(byte) = linha.find(").") {
            return Some((numero as u32, linha[..byte + 2].chars().count() as u32));
        }
        // `a.b.` — o segundo ponto da cadeia.
        let mut anterior: Option<usize> = None;
        for (byte, _) in linha.match_indices('.') {
            if let Some(primeiro) = anterior {
                let meio = linha.get(primeiro + 1..byte)?;
                if !meio.is_empty()
                    && meio.chars().all(|c| c.is_alphanumeric() || c == '_')
                    && linha[byte + 1..]
                        .chars()
                        .next()
                        .is_some_and(char::is_alphabetic)
                {
                    return Some((numero as u32, linha[..=byte].chars().count() as u32));
                }
            }
            anterior = Some(byte);
        }
    }
    None
}
