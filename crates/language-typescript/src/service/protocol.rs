//! Como se conversa com o `tsserver`.
//!
//! O formato foi **sondado**, e não lido: a entrada é um JSON por linha, e a
//! saída vem enquadrada por tamanho, com eventos não solicitados no meio.
//!
//! ```text
//! →  {"seq":1,"type":"request","command":"status"}
//! ←  Content-Length: 105
//! ←
//! ←  {"seq":0,"type":"response","command":"status","request_seq":1,...}
//! ←  Content-Length: 76
//! ←
//! ←  {"seq":0,"type":"event","event":"typingsInstallerPid",...}
//! ```
//!
//! Duas consequências, e as duas mudam a forma do adapter:
//!
//! **A resposta não é o que chega a seguir.** Um `send` seguido de um `receive`
//! casaria a pergunta com um evento. A correlação é por `request_seq`, e por
//! isso a leitura é um laço próprio, separado de quem pergunta.
//!
//! **O corpo é lido por tamanho.** Ler por linha funcionaria enquanto nenhum
//! JSON trouxesse quebra de linha dentro de uma string — e uma resposta de
//! completação com trecho de código traz. Ver a fase 3a da `23`.

use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};

use ide_process::ProcessConversation;
use tokio::sync::oneshot;

const CONTENT_LENGTH: &str = "Content-Length:";

/// O que uma mensagem do `tsserver` pode ser.
#[derive(Clone, Debug)]
pub(crate) enum Message {
    Response {
        request_seq: u64,
        success: bool,
        body: serde_json::Value,
        message: Option<String>,
    },
    /// Evento não solicitado — aviso de projeto, instalador de tipos.
    ///
    /// Só o nome é guardado: hoje nenhum evento é consumido, e o corpo iria
    /// junto sem ninguém ler. O `--suppressDiagnosticEvents` já cala o único
    /// que interessaria, porque nós perguntamos por diagnóstico quando queremos.
    Event { name: String },
}

/// Lê uma mensagem enquadrada por tamanho.
///
/// `None` é o fim da saída: o processo morreu. Cabeçalho ilegível também é
/// morte — se o enquadramento se perdeu, o resto do canal não é confiável, e
/// continuar lendo produziria lixo com cara de resposta.
pub(crate) async fn read_message(conversa: &dyn ProcessConversation) -> Option<serde_json::Value> {
    let mut tamanho = None;
    loop {
        let linha = conversa.receive().await.ok()??;
        if linha.is_empty() {
            // Linha vazia **antes** do cabeçalho não é o separador: é a quebra
            // que fecha a mensagem anterior. O `tsserver` termina o corpo com
            // uma quebra de linha, e tomá-la por início de mensagem faria a
            // leitura ver um bloco sem `Content-Length` e concluir que o
            // processo morreu — o analisador seria dado por morto logo na
            // primeira resposta, e o provider cairia para o nativo sem motivo.
            if tamanho.is_some() {
                break;
            }
            continue;
        }
        if let Some(valor) = linha.strip_prefix(CONTENT_LENGTH) {
            tamanho = valor.trim().parse::<usize>().ok();
        }
        // Outros cabeçalhos são ignorados de propósito: o `tsserver` só manda
        // este hoje, e recusar o que não se conhece quebraria numa versão nova
        // por uma linha que não faria falta.
    }
    let bytes = conversa.receive_exact(tamanho?).await.ok()??;
    serde_json::from_slice(&bytes).ok()
}

pub(crate) fn classify(valor: &serde_json::Value) -> Option<Message> {
    match valor.get("type").and_then(serde_json::Value::as_str)? {
        "response" => Some(Message::Response {
            request_seq: valor.get("request_seq")?.as_u64()?,
            success: valor
                .get("success")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false),
            body: valor.get("body").cloned().unwrap_or(serde_json::Value::Null),
            message: valor
                .get("message")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned),
        }),
        "event" => Some(Message::Event {
            name: valor.get("event")?.as_str()?.to_owned(),
        }),
        _ => None,
    }
}

/// Quem está esperando resposta, por número de sequência.
type Pendentes = Arc<Mutex<HashMap<u64, oneshot::Sender<Result<serde_json::Value, String>>>>>;

/// Uma conversa com o analisador, com as respostas já correlacionadas.
pub(crate) struct Session {
    conversa: Arc<dyn ProcessConversation>,
    proxima_seq: AtomicU64,
    pendentes: Pendentes,
    /// Ligado quando a saída fecha: o processo morreu.
    morto: Arc<AtomicBool>,
}

impl Session {
    pub(crate) fn new(conversa: Arc<dyn ProcessConversation>) -> Self {
        let pendentes: Pendentes = Arc::new(Mutex::new(HashMap::new()));
        let morto = Arc::new(AtomicBool::new(false));
        let leitura_conversa = Arc::clone(&conversa);
        let entregar = Arc::clone(&pendentes);
        let avisar = Arc::clone(&morto);

        // O laço de leitura é próprio, e não pode ser o de quem pergunta:
        // eventos chegam sem pedido, e a resposta de uma pergunta pode vir
        // depois de três eventos.
        //
        // E é uma **thread**, não um `tokio::spawn`: uma tarefa hospedada no
        // runtime de quem chama só progride enquanto alguém está dentro de um
        // `block_on`, e a resposta que este laço precisa ler chega justamente
        // quando ninguém está. Funcionaria por acidente às vezes, e travaria
        // quando a resposta chegasse entre dois pedidos. Ver a fase 3c da `23`.
        //
        // Aqui só se conversa por canal, porque a conversa é dona do próprio
        // I/O — e canal funciona de qualquer runtime, ou de nenhum.
        std::thread::Builder::new()
            .name("typescript-service-reader".to_owned())
            .spawn(move || {
                let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                else {
                    return;
                };
                runtime.block_on(leitura(&leitura_conversa, &entregar, &avisar));
            })
            .ok();

        Self {
            conversa,
            proxima_seq: AtomicU64::new(1),
            pendentes,
            morto,
        }
    }

    pub(crate) fn is_alive(&self) -> bool {
        !self.morto.load(Ordering::Acquire)
    }

    /// Manda um pedido e espera a resposta correspondente.
    pub(crate) async fn request(
        &self,
        command: &str,
        arguments: serde_json::Value,
        timeout: Duration,
    ) -> Result<serde_json::Value, String> {
        let seq = self.proxima_seq.fetch_add(1, Ordering::Relaxed);
        let (remetente, receptor) = oneshot::channel();
        self.pendentes
            .lock()
            .map_err(|_| "registro de pedidos indisponível".to_owned())?
            .insert(seq, remetente);
        self.enviar(seq, command, arguments).await?;
        match tokio::time::timeout(timeout, receptor).await {
            Ok(Ok(resposta)) => resposta,
            // O canal fechou sem resposta: o laço de leitura terminou.
            Ok(Err(_)) => Err("o analisador encerrou".to_owned()),
            Err(_) => {
                // Prazo esgotado: tira o pedido da espera para o mapa não
                // crescer com quem nunca vai ser respondido.
                if let Ok(mut pendentes) = self.pendentes.lock() {
                    pendentes.remove(&seq);
                }
                Err("o analisador não respondeu a tempo".to_owned())
            }
        }
    }

    /// Manda um pedido que não tem resposta.
    ///
    /// Abrir e fechar arquivo são assim no protocolo do `tsserver`: ele não
    /// confirma. Esperar por confirmação que não vem travaria a cada abertura.
    pub(crate) async fn notify(
        &self,
        command: &str,
        arguments: serde_json::Value,
    ) -> Result<(), String> {
        let seq = self.proxima_seq.fetch_add(1, Ordering::Relaxed);
        self.enviar(seq, command, arguments).await
    }

    async fn enviar(
        &self,
        seq: u64,
        command: &str,
        arguments: serde_json::Value,
    ) -> Result<(), String> {
        let pedido = serde_json::json!({
            "seq": seq,
            "type": "request",
            "command": command,
            "arguments": arguments,
        });
        self.conversa
            .send(&pedido.to_string())
            .await
            .map_err(|erro| erro.to_string())
    }

    pub(crate) async fn shutdown(&self) {
        let _ = self.conversa.shutdown().await;
    }
}

/// Lê tudo o que o analisador manda, e entrega a quem estiver esperando.
async fn leitura(
    conversa: &Arc<dyn ProcessConversation>,
    entregar: &Pendentes,
    avisar: &AtomicBool,
) {
    while let Some(valor) = read_message(conversa.as_ref()).await {
        match classify(&valor) {
            Some(Message::Response {
                request_seq,
                success,
                body,
                message,
            }) => {
                let esperando = entregar
                    .lock()
                    .ok()
                    .and_then(|mut pendentes| pendentes.remove(&request_seq));
                if let Some(canal) = esperando {
                    let _ = canal.send(if success {
                        Ok(body)
                    } else {
                        Err(message.unwrap_or_else(|| "pedido recusado".to_owned()))
                    });
                }
            }
            Some(Message::Event { name }) => {
                tracing::debug!(evento = name, "evento do analisador");
            }
            None => tracing::debug!("mensagem do analisador em formato desconhecido"),
        }
    }
    // A saída fechou. Quem espera é acordado agora, e não daqui a nunca: é este
    // o sinal que faz o provider dizer que deixou de existir.
    avisar.store(true, Ordering::Release);
    if let Ok(mut pendentes) = entregar.lock() {
        for (_, canal) in pendentes.drain() {
            let _ = canal.send(Err("o analisador encerrou".to_owned()));
        }
    }
}
