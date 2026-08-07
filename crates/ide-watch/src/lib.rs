//! O que muda no disco com a IDE aberta.
//!
//! # Por que isto é uma crate, e não um módulo de quem observa
//!
//! Ele nasceu dentro de `language-java`, porque quando nasceu **era** do índice
//! de Java. A `21` já anotava a árvore do Explorer como segundo consumidor sem
//! dono; o Git é o terceiro, e três consumidores é quando o observador deixa de
//! ser detalhe de um indexador e vira infraestrutura. É a fase 4 da `22`.
//!
//! # A regra que decide o desenho
//!
//! **O registro no sistema operacional é um só; o filtro é de cada um.** Dois
//! observadores sobre a mesma árvore são dois registros, e no Linux eles contam
//! duas vezes contra o limite por usuário — que a `21` já registrou como o
//! motivo de a raiz inteira ser tentada antes das raízes de fonte. E são duas
//! rajadas para o mesmo evento, com duas reações fora de ordem.
//!
//! Quem se registra diz o que lhe interessa. O índice de Java quer `.java` nas
//! raízes de fonte e **ignora `.git`**; o Git quer exatamente `.git/HEAD`,
//! `.git/index` e `.git/refs/` e ignora o resto. Os dois filtros estão certos, e
//! são de perguntas diferentes: o erro que a `21` nomeou não é ter dois filtros,
//! é ter dois filtros para a **mesma** pergunta.

use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, mpsc},
    thread,
    time::Duration,
};

use notify::{RecommendedWatcher, RecursiveMode, Watcher};

/// Quanto silêncio basta para acreditar que a rajada acabou.
///
/// Gravar um arquivo dispara três ou quatro eventos; um build dispara milhares
/// durante um minuto; um `checkout` reescreve o projeto inteiro. Reagir ao
/// **silêncio**, e não ao evento, resolve os três com uma regra só: a rajada
/// vira uma reação, com a lista já sem repetição.
///
/// Os 300 ms são os que a `21` mediu, e continuam sendo os mesmos.
const SILENCIO: Duration = Duration::from_millis(300);

/// Enquanto não há nada pendente, esperar de olho aberto não serve a ninguém.
const OCIOSO: Duration = Duration::from_secs(60);

/// Quem quer saber o que mudou no disco.
///
/// As três respostas são de quem consome, e não do observador: ele não sabe o
/// que é um `.java` nem o que é uma referência de Git.
pub trait WatchConsumer: Send + Sync {
    /// Se este caminho interessa a este consumidor.
    ///
    /// Chamado **por evento**, e por isso barato de propósito: é comparação de
    /// extensão e de prefixo, e não leitura de disco.
    fn interessa(&self, path: &Path) -> bool;

    /// A rajada acabou, e estes caminhos mudaram.
    ///
    /// Roda na linha de execução do observador, e não na da janela. Quem
    /// precisar da tela manda recado — é o que os dois consumidores da IDE
    /// fazem, com um canal.
    fn mudou(&self, lote: &[PathBuf]);

    /// O sistema perdeu eventos, e o que se sabe pode estar velho.
    ///
    /// Toda plataforma perde quando a rajada passa do que ela guarda. Perder não
    /// pode virar estado inventado: quem implementa isto refaz o que precisar,
    /// como a varredura da abertura já faz.
    fn perdeu_eventos(&self) {}
}

/// O observador vivo. Soltá-lo para de observar.
pub struct FileWatcher {
    /// Segurar o observador é o que o mantém registrado no sistema operacional.
    _watcher: RecommendedWatcher,
    /// Quem quer saber. Cresce depois do começo de propósito: o índice de Java
    /// só existe quando termina de ser construído, e o Git só quando o projeto
    /// é reconhecido — e nenhum dos dois pode atrasar a observação do outro.
    consumidores: Arc<Mutex<Vec<Arc<dyn WatchConsumer>>>>,
}

impl FileWatcher {
    /// Começa a observar, sem consumidor nenhum ainda.
    ///
    /// `alternativas` são as pastas a tentar quando a raiz inteira não dá: no
    /// Windows e no macOS um registro recursivo cobre a árvore toda, e no Linux
    /// a biblioteca percorre pasta por pasta contra um limite por usuário.
    ///
    /// Devolve `None` quando não dá — limite do sistema, permissão, plataforma.
    /// **Falhar aqui não quebra nada:** sem observador a IDE volta a ser o que
    /// era, com o que se sabe envelhecendo até a próxima abertura. É degradação,
    /// e por isso não há erro para tratar mais acima.
    #[must_use]
    pub fn iniciar(raiz: &Path, alternativas: Vec<PathBuf>) -> Option<Self> {
        let (envio, recepcao) = mpsc::channel();
        let mut watcher = notify::recommended_watcher(move |resultado| {
            // O canal fechado quer dizer que o observador morreu; o erro de
            // envio é a forma de saber, e não há o que fazer com ele.
            let _ = envio.send(resultado);
        })
        .ok()?;
        if watcher.watch(raiz, RecursiveMode::Recursive).is_err() {
            let mut alguma = false;
            for pasta in &alternativas {
                alguma |= watcher.watch(pasta, RecursiveMode::Recursive).is_ok();
            }
            if !alguma {
                return None;
            }
        }
        let consumidores: Arc<Mutex<Vec<Arc<dyn WatchConsumer>>>> =
            Arc::new(Mutex::new(Vec::new()));
        let lista = Arc::clone(&consumidores);
        thread::spawn(move || laco(&recepcao, &lista));
        Some(Self {
            _watcher: watcher,
            consumidores,
        })
    }

    /// Acrescenta quem quer saber.
    pub fn registrar(&self, consumidor: Arc<dyn WatchConsumer>) {
        if let Ok(mut lista) = self.consumidores.lock() {
            lista.push(consumidor);
        }
    }

    /// Quantos estão registrados, para quem precisa afirmar isso.
    #[must_use]
    pub fn registrados(&self) -> usize {
        self.consumidores.lock().map_or(0, |lista| lista.len())
    }
}

/// Junta o que chega e reage quando para de chegar.
///
/// **A rajada é uma só, e a reação também.** Cada consumidor recebe o lote dele,
/// já filtrado e sem repetição; quem não tiver nada no lote não é chamado — um
/// `checkout` que só mexe em `.java` não faz o Git perguntar nada ao disco.
fn laco(
    recepcao: &mpsc::Receiver<notify::Result<notify::Event>>,
    consumidores: &Arc<Mutex<Vec<Arc<dyn WatchConsumer>>>>,
) {
    let mut pendentes: HashSet<PathBuf> = HashSet::new();
    loop {
        let espera = if pendentes.is_empty() {
            OCIOSO
        } else {
            SILENCIO
        };
        match recepcao.recv_timeout(espera) {
            Ok(Ok(evento)) => {
                // Guardado sem filtro: o filtro é de cada consumidor, e um
                // caminho que não interessa a ninguém custa uma comparação.
                pendentes.extend(evento.paths);
            }
            Ok(Err(_)) => {
                for consumidor in lista(consumidores) {
                    consumidor.perdeu_eventos();
                }
                pendentes.clear();
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if pendentes.is_empty() {
                    continue;
                }
                let lote: Vec<PathBuf> = pendentes.drain().collect();
                for consumidor in lista(consumidores) {
                    let meu: Vec<PathBuf> = lote
                        .iter()
                        .filter(|caminho| consumidor.interessa(caminho))
                        .cloned()
                        .collect();
                    if !meu.is_empty() {
                        consumidor.mudou(&meu);
                    }
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
        }
    }
}

/// A lista de consumidores, copiada para não segurar a trava durante a reação.
///
/// Uma reação pode demorar — reindexar um lote de arquivos é trabalho —, e
/// segurar a trava por todo esse tempo impediria alguém de se registrar.
fn lista(consumidores: &Arc<Mutex<Vec<Arc<dyn WatchConsumer>>>>) -> Vec<Arc<dyn WatchConsumer>> {
    consumidores
        .lock()
        .map(|lista| lista.clone())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Um consumidor que conta o que recebeu.
    struct Contador {
        extensao: &'static str,
        chamadas: AtomicUsize,
        caminhos: Mutex<Vec<PathBuf>>,
    }

    impl WatchConsumer for Contador {
        fn interessa(&self, path: &Path) -> bool {
            path.extension().is_some_and(|ext| ext == self.extensao)
        }

        fn mudou(&self, lote: &[PathBuf]) {
            self.chamadas.fetch_add(1, Ordering::Relaxed);
            if let Ok(mut caminhos) = self.caminhos.lock() {
                caminhos.extend_from_slice(lote);
            }
        }
    }

    /// Cada consumidor recebe **o lote dele**, e quem não tem nada não é
    /// chamado.
    ///
    /// É a regra que separa este observador do que ele substituiu: lá o filtro
    /// era do laço, e só existia um. Aqui o registro no sistema é um só e os
    /// filtros são de cada um — que é o que permite o índice de Java ignorar
    /// `.git` enquanto o Git só olha para lá.
    #[test]
    fn cada_consumidor_recebe_so_o_que_lhe_interessa() {
        let (envio, recepcao) = mpsc::channel();
        let java = Arc::new(Contador {
            extensao: "java",
            chamadas: AtomicUsize::new(0),
            caminhos: Mutex::new(Vec::new()),
        });
        let texto = Arc::new(Contador {
            extensao: "txt",
            chamadas: AtomicUsize::new(0),
            caminhos: Mutex::new(Vec::new()),
        });
        let consumidores: Arc<Mutex<Vec<Arc<dyn WatchConsumer>>>> = Arc::new(Mutex::new(vec![
            Arc::clone(&java) as Arc<dyn WatchConsumer>,
            Arc::clone(&texto) as Arc<dyn WatchConsumer>,
        ]));
        let lista = Arc::clone(&consumidores);
        let laco = thread::spawn(move || super::laco(&recepcao, &lista));

        let evento = notify::Event::new(notify::EventKind::Any)
            .add_path(PathBuf::from("/projeto/Pedido.java"))
            .add_path(PathBuf::from("/projeto/Cliente.java"));
        assert!(envio.send(Ok(evento)).is_ok());
        // O silêncio é o que dispara a reação: esperar mais que ele é o que o
        // laço faz, e é o que o teste espera junto.
        thread::sleep(SILENCIO * 3);

        assert_eq!(java.chamadas.load(Ordering::Relaxed), 1, "uma rajada, uma reação");
        assert_eq!(
            java.caminhos.lock().map(|c| c.len()).unwrap_or_default(),
            2,
            "com os dois arquivos no mesmo lote"
        );
        assert_eq!(
            texto.chamadas.load(Ordering::Relaxed),
            0,
            "quem não tem nada no lote não é chamado"
        );

        drop(envio);
        let _ = laco.join();
    }

    /// Quem se registra depois recebe o que vier depois.
    ///
    /// O índice de Java só existe quando termina de ser construído, e o Git só
    /// quando o projeto é reconhecido: exigir os dois no começo faria a
    /// observação esperar pelo mais lento.
    #[test]
    fn quem_chega_depois_tambem_recebe() {
        let (envio, recepcao) = mpsc::channel();
        let consumidores: Arc<Mutex<Vec<Arc<dyn WatchConsumer>>>> = Arc::new(Mutex::new(Vec::new()));
        let lista = Arc::clone(&consumidores);
        let laco = thread::spawn(move || super::laco(&recepcao, &lista));

        let tardio = Arc::new(Contador {
            extensao: "java",
            chamadas: AtomicUsize::new(0),
            caminhos: Mutex::new(Vec::new()),
        });
        if let Ok(mut guarda) = consumidores.lock() {
            guarda.push(Arc::clone(&tardio) as Arc<dyn WatchConsumer>);
        }

        let evento =
            notify::Event::new(notify::EventKind::Any).add_path(PathBuf::from("/p/Tardio.java"));
        assert!(envio.send(Ok(evento)).is_ok());
        thread::sleep(SILENCIO * 3);
        assert_eq!(tardio.chamadas.load(Ordering::Relaxed), 1);

        drop(envio);
        let _ = laco.join();
    }
}
