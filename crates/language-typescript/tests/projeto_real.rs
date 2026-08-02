//! Contra um projeto Angular de verdade, e não contra um construído para passar.
//!
//! O caminho vem de `ER_IDE_PROJETO_TS`; sem ele, os testes são ignorados. É
//! leitura apenas: nada aqui escreve no projeto.
//!
//! ```text
//! ER_IDE_PROJETO_TS=C:/caminho/do/projeto cargo test -p language-typescript --test projeto_real -- --ignored
//! ```

use std::path::PathBuf;

use language_typescript::{tsconfig, tsserver_in};

fn projeto() -> Option<PathBuf> {
    std::env::var("ER_IDE_PROJETO_TS").ok().map(PathBuf::from)
}

/// O que o nosso leitor conclui sobre as raízes de um projeto Angular real.
#[test]
#[ignore = "exige ER_IDE_PROJETO_TS apontando para um projeto TypeScript"]
fn what_our_reader_concludes_about_a_real_project() {
    let Some(raiz) = projeto() else {
        panic!("defina ER_IDE_PROJETO_TS");
    };
    let Ok(config) = tsconfig::load(&raiz.join("tsconfig.json")) else {
        panic!("o tsconfig do projeto precisa ser lido");
    };
    let raizes = config.source_roots();
    println!("raizes: {raizes:?}");
    println!("excluido: {:?}", config.excluded());
    println!("references: {:?}", config.references);
    println!("tsserver: {:?}", tsserver_in(&raiz));

    // O que se cobra: a raiz não pode ser o projeto inteiro, porque isso põe
    // `node_modules` e `dist` dentro do que a IDE considera código-fonte.
    assert!(
        !raizes.contains(&raiz),
        "a raiz não pode ser o projeto inteiro: {raizes:?}"
    );
}
