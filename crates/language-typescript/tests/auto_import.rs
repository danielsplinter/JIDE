//! **Escolher um tipo que não está importado traz o `import` junto.**
//!
//! # O que este teste guarda
//!
//! O analisador só oferece o que está no escopo do arquivo. Medido: `private c:
//! Ht` respondia 38 itens com o `import` no topo e **zero** sem ele — e foi
//! isso, e não um defeito da IDE, que fez `private Http` não completar.
//!
//! Ligar as sugestões de fora do escopo sem trazer o `import` seria oferecer um
//! nome que não compila — a família de defeito que esta especificação mais
//! persegue. As duas metades andam juntas, e é isso que se afirma aqui.
//!
//! ```text
//! ER_IDE_PROJETO_TS=C:/caminho/do/projeto cargo test --release -p language-typescript --test auto_import -- --ignored --nocapture
//! ```

use std::path::PathBuf;

use ide_domain::{CompletionRequest, DocumentId, DocumentSnapshot, TextPosition};
use ide_language_api::{LanguageActivationContext, LanguageProvider};
use language_typescript::TypeScriptServiceProvider;

const CODIGO: &str = "export class Pagina {\n  constructor(private c: Ht\n}\n";

#[test]
#[ignore = "exige ER_IDE_PROJETO_TS com node_modules instalado"]
fn choosing_a_type_that_is_not_imported_brings_the_import() {
    let Ok(raiz) = std::env::var("ER_IDE_PROJETO_TS") else {
        panic!("aponte ER_IDE_PROJETO_TS");
    };
    let raiz = PathBuf::from(raiz);
    let arquivo = raiz.join("src/app/er-teste-auto-import.ts");
    assert!(std::fs::write(&arquivo, CODIGO).is_ok());

    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(erro) => panic!("{erro}"),
    };
    let processos = std::sync::Arc::new(ide_process::NativeProcessSupervisor::default());
    let servico = match runtime.block_on(TypeScriptServiceProvider::new(processos).activate(
        LanguageActivationContext {
            workspace_root: raiz.clone(),
            source_roots: vec![raiz.join("src")],
            toolchains: Vec::new(),
        },
    )) {
        Ok(ativo) => ativo,
        Err(erro) => panic!("o analisador precisa subir: {erro}"),
    };
    assert!(
        runtime
            .block_on(servico.open_document(DocumentSnapshot {
                id: DocumentId(1),
                path: arquivo.clone(),
                version: 1,
                text: CODIGO.to_owned(),
            }))
            .is_ok()
    );

    // Logo depois de `Ht`, que é onde a IDE abre a lista.
    let (linha, coluna) = CODIGO
        .lines()
        .enumerate()
        .find_map(|(n, l)| {
            l.rfind("Ht")
                .map(|b| (n as u32, (l[..b + 2].chars().count()) as u32))
        })
        .unwrap_or((0, 0));
    let itens = match runtime.block_on(servico.completion(CompletionRequest {
        document_id: DocumentId(1),
        position: TextPosition {
            line: linha,
            column: coluna,
        },
        prefix: "Ht".to_owned(),
    })) {
        Ok(itens) => itens,
        Err(erro) => panic!("a lista precisa vir: {erro}"),
    };
    let rotulos: Vec<&str> = itens.iter().map(|item| item.label.as_str()).collect();
    println!("[auto-import] {} itens: {:?}", itens.len(), &rotulos[..rotulos.len().min(4)]);
    assert!(
        rotulos.contains(&"HttpClient"),
        "um tipo fora do escopo precisa ser oferecido: {rotulos:?}"
    );

    let trocas = match runtime
        .block_on(servico.completion_edits(DocumentId(1), "HttpClient".to_owned()))
    {
        Ok(trocas) => trocas,
        Err(erro) => panic!("as trocas precisam vir: {erro}"),
    };
    println!("[auto-import] {} trocas: {trocas:?}", trocas.len());
    assert!(
        trocas.iter().any(|troca| troca.text.contains("import")
            && troca.text.contains("HttpClient")),
        "escolher o tipo precisa trazer o `import`: {trocas:?}"
    );

    // **E um item que já está no escopo não muda mais nada.** Sem esta metade,
    // a IDE pediria uma ação ao analisador a cada escolha, e escreveria coisa
    // onde não há o que escrever.
    let nenhuma = runtime
        .block_on(servico.completion_edits(DocumentId(1), "private".to_owned()))
        .unwrap_or_default();
    assert!(nenhuma.is_empty(), "veio: {nenhuma:?}");

    let _ = runtime.block_on(servico.shutdown());
    let _ = std::fs::remove_file(&arquivo);
}
