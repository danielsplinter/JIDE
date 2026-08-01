//! O que muda no disco com a IDE aberta.
//!
//! É a fase 1 da `21`. Sem isto o índice só sabe do que a própria IDE grava:
//! trocar de branch, gerar código ou editar noutro programa deixava a
//! completação e a navegação respondendo pelo texto anterior, **sem avisar** —
//! a família de defeito que a `19` chamou de a mais perigosa.

use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::{Arc, RwLock, mpsc},
    thread,
    time::Duration,
};

use notify::{RecommendedWatcher, RecursiveMode, Watcher};

use crate::{
    documents::Documents,
    index::{WorkspaceIndex, fonte_java},
};

/// Quanto silêncio basta para acreditar que a rajada acabou.
///
/// Gravar um arquivo dispara três ou quatro eventos; um build dispara milhares
/// durante um minuto. Reagir ao **silêncio**, e não ao evento, resolve os dois
/// com uma regra só: a rajada vira uma reação, com a lista já sem repetição.
const SILENCIO: Duration = Duration::from_millis(300);

/// Enquanto não há nada pendente, esperar de olho aberto não serve a ninguém.
const OCIOSO: Duration = Duration::from_secs(60);

/// O observador vivo. Soltá-lo para de observar.
pub(super) struct Observador {
    /// Segurar o observador é o que o mantém registrado no sistema operacional.
    _watcher: RecommendedWatcher,
}

impl Observador {
    /// Começa a observar a raiz do projeto.
    ///
    /// Devolve `None` quando não dá — limite do sistema, permissão, plataforma.
    /// **Falhar aqui não quebra nada:** sem observador a IDE volta a ser o que
    /// era, com o índice envelhecendo até a próxima abertura. É degradação, e
    /// por isso não há erro para tratar mais acima.
    pub(super) fn iniciar(
        raiz: &Path,
        fontes: Vec<PathBuf>,
        indice: Arc<RwLock<WorkspaceIndex>>,
    ) -> Option<Self> {
        let (envio, recepcao) = mpsc::channel();
        let mut watcher = notify::recommended_watcher(move |resultado| {
            // O canal fechado quer dizer que a linguagem morreu; o erro de envio
            // é a forma de saber, e não há o que fazer com ele.
            let _ = envio.send(resultado);
        })
        .ok()?;
        // Recursivo: no Windows e no macOS isso é **um** registro para a árvore
        // inteira, de dez ou de sessenta mil arquivos. No Linux a biblioteca
        // percorre e registra pasta por pasta, contra um limite por usuário —
        // e é por isso que a raiz inteira é tentada antes das raízes de fonte.
        if watcher.watch(raiz, RecursiveMode::Recursive).is_err() {
            let mut alguma = false;
            for fonte in &fontes {
                alguma |= watcher.watch(fonte, RecursiveMode::Recursive).is_ok();
            }
            if !alguma {
                return None;
            }
        }

        let raiz_do_laco = raiz.to_path_buf();
        thread::spawn(move || {
            laco(&recepcao, &raiz_do_laco, &fontes, &indice);
        });
        Some(Self { _watcher: watcher })
    }
}

/// Junta o que chega e reage quando para de chegar.
fn laco(
    recepcao: &mpsc::Receiver<notify::Result<notify::Event>>,
    raiz: &Path,
    fontes: &[PathBuf],
    indice: &Arc<RwLock<WorkspaceIndex>>,
) {
    let Ok(documentos) = Documents::new() else {
        return;
    };
    let mut pendentes: HashSet<PathBuf> = HashSet::new();
    loop {
        let espera = if pendentes.is_empty() {
            OCIOSO
        } else {
            SILENCIO
        };
        match recepcao.recv_timeout(espera) {
            Ok(Ok(evento)) => {
                for caminho in evento.paths {
                    if fonte_java(&caminho, fontes) {
                        pendentes.insert(caminho);
                    }
                }
            }
            Ok(Err(_)) => {
                // Toda plataforma perde evento quando a rajada passa do que ela
                // guarda. Perder não pode virar índice inventado: a resposta é
                // a mesma varredura da abertura, que já existe e já está certa.
                revarrer(raiz, fontes, indice, &documentos);
                pendentes.clear();
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if pendentes.is_empty() {
                    continue;
                }
                let lote: Vec<PathBuf> = pendentes.drain().collect();
                reindexar(&lote, indice, &documentos);
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
        }
    }
}

fn reindexar(lote: &[PathBuf], indice: &Arc<RwLock<WorkspaceIndex>>, documentos: &Documents) {
    let Ok(mut guarda) = indice.write() else {
        return;
    };
    let _ = documentos.with_parser_mut(|parser| {
        for caminho in lote {
            guarda.reindex_file(caminho, parser);
        }
    });
}

fn revarrer(
    raiz: &Path,
    fontes: &[PathBuf],
    indice: &Arc<RwLock<WorkspaceIndex>>,
    documentos: &Documents,
) {
    let mudaram = match indice.read() {
        Ok(guarda) => guarda.diferenca(raiz, fontes),
        Err(_) => return,
    };
    if mudaram.is_empty() {
        return;
    }
    reindexar(&mudaram, indice, documentos);
}

/// Os caminhos que o observador aceitaria, para quem precisa afirmar o filtro.
#[cfg(test)]
pub(super) fn aceita(caminho: &Path, fontes: &[PathBuf]) -> bool {
    fonte_java(caminho, fontes)
}

/// Se o caminho está numa pasta que a indexação ignora.
#[cfg(test)]
pub(super) fn ignorado(caminho: &Path) -> bool {
    crate::index::caminho_ignorado(caminho)
}
