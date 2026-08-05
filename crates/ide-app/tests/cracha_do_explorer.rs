//! O crachá do Explorer: a espécie de cada arquivo, vinda do índice.
//!
//! A árvore identifica um nó pelo **hash do caminho** que a varredura do disco
//! deu a ele. O mapa de espécies é chaveado pelo mesmo hash, calculado sobre o
//! caminho que o índice devolve. Se os dois caminhos não forem idênticos —
//! separador, prefixo, canonicalização —, o mapa fica cheio e a árvore não acha
//! nada, e o defeito é invisível: nenhum erro, nenhum log, só nenhuma letra.

use std::{fs, path::PathBuf, sync::Arc};

use ide_domain::{RequestId, SymbolKind};
use ide_language_api::{CancellationToken, LanguageProvider, LanguageRequestContext};
use ide_language_host::LanguageHost;
use ide_ui::explorer_id;

fn success<T, E: std::fmt::Debug>(result: Result<T, E>) -> T {
    match result {
        Ok(value) => value,
        Err(error) => panic!("operação deveria funcionar: {error:?}"),
    }
}

fn context() -> LanguageRequestContext {
    LanguageRequestContext {
        request_id: RequestId(1),
        cancellation: CancellationToken::new(),
    }
}

/// Projeto Java de verdade em disco, com as quatro espécies.
fn projeto(nome: &str) -> PathBuf {
    // Uma pasta por teste, e nao uma por processo: eles rodam em paralelo, e a
    // limpeza de um apagava o projeto do outro no meio da indexacao.
    let raiz = std::env::temp_dir().join(format!("er-ide-cracha-{}-{nome}", std::process::id()));
    let _ = fs::remove_dir_all(&raiz);
    let pacote = raiz.join("src").join("main").join("java").join("exemplo");
    assert!(fs::create_dir_all(&pacote).is_ok());
    let escrever = |nome: &str, corpo: &str| {
        assert!(fs::write(pacote.join(nome), corpo).is_ok());
    };
    escrever("Pedido.java", "package exemplo;\npublic class Pedido {}\n");
    escrever(
        "Repositorio.java",
        "package exemplo;\npublic interface Repositorio {}\n",
    );
    escrever(
        "Situacao.java",
        "package exemplo;\npublic enum Situacao { ABERTA }\n",
    );
    escrever("Dto.java", "package exemplo;\npublic record Dto(int id) {}\n");
    raiz
}

/// O caminho que o índice devolve é **o mesmo** que a árvore identifica.
///
/// É a junta entre as duas metades do crachá, e a única que nenhum dos dois
/// lados consegue conferir sozinho.
#[test]
fn a_especie_de_cada_arquivo_chega_pela_identidade_da_arvore() {
    let raiz = projeto("identidade");
    let host = LanguageHost::new(&raiz);
    let java: Arc<dyn LanguageProvider> = Arc::new(language_java::JavaLanguageProvider::new());
    success(host.register(java));

    // A mesma pergunta que a aplicação faz: filtro vazio, sem limite.
    let mut simbolos = success(pollster::block_on(host.workspace_types(
        context(),
        "java",
        String::new(),
        usize::MAX,
    )));
    // O provider indexa o projeto **depois** de subir, e a primeira pergunta
    // pode chegar antes disso. É este o caso que faltava na aplicação: ela
    // perguntava uma vez, cedo demais, e não perguntava de novo.
    let limite = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while simbolos.is_empty() && std::time::Instant::now() < limite {
        std::thread::yield_now();
        simbolos = success(pollster::block_on(host.workspace_types(
            context(),
            "java",
            String::new(),
            usize::MAX,
        )));
    }
    assert!(
        !simbolos.is_empty(),
        "o índice precisa devolver os tipos do projeto com filtro vazio"
    );

    let mapa: std::collections::HashMap<u64, SymbolKind> = simbolos
        .into_iter()
        .map(|simbolo| (explorer_id(&simbolo.location.path), simbolo.kind))
        .collect();

    let pacote = raiz.join("src").join("main").join("java").join("exemplo");
    let especie = |nome: &str| mapa.get(&explorer_id(&pacote.join(nome))).copied();
    assert_eq!(
        especie("Pedido.java"),
        Some(SymbolKind::Class),
        "o caminho do índice precisa bater com o da árvore — se falhar aqui, \
         o mapa está cheio e a árvore não acha nada, sem erro nenhum"
    );
    assert_eq!(especie("Repositorio.java"), Some(SymbolKind::Interface));
    assert_eq!(especie("Situacao.java"), Some(SymbolKind::Enum));
    assert_eq!(especie("Dto.java"), Some(SymbolKind::Record));

    let _ = fs::remove_dir_all(&raiz);
}

/// Uma extensão que ninguém sabe responder **erra**, e não devolve vazio.
///
/// A diferença decide o desenho do laço que monta o mapa: erro quer dizer "não é
/// comigo", e se segue adiante na hora; vazio quer dizer "não há tipos", e
/// enquanto o índice não souber dizer "ainda não sei", vazio pede insistência.
///
/// Tratar os dois igual fazia cada linguagem quieta segurar o mapa pelo prazo
/// inteiro — e como o mapa só é enviado no fim do laço, quatro extensões
/// caladas adiavam o primeiro crachá em minutos.
#[test]
fn extensao_sem_dono_erra_em_vez_de_devolver_vazio() {
    let raiz = projeto("sem-dono");
    let host = LanguageHost::new(&raiz);
    let java: Arc<dyn LanguageProvider> = Arc::new(language_java::JavaLanguageProvider::new());
    success(host.register(java));

    assert!(
        pollster::block_on(host.workspace_types(context(), "kt", String::new(), usize::MAX))
            .is_err(),
        "extensão sem provider capaz precisa errar; se ela passar a devolver \
         `Ok(vec![])`, o laço dos crachás volta a esperar por ela"
    );

    let _ = fs::remove_dir_all(&raiz);
}
