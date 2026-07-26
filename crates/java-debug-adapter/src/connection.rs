//! Conexão TCP com o alvo depurado.
//!
//! Um leitor dedicado consome o socket, entrega respostas a quem as pediu e
//! encaminha os eventos assíncronos do alvo. Perder a conexão falha todas as
//! requisições pendentes com `Detached`, nunca deixa uma espera pendurada.

use std::{
    collections::HashMap,
    sync::{
        Mutex, OnceLock,
        atomic::{AtomicU32, Ordering},
    },
    time::Duration,
};

use ide_debug_api::DebugError;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{
        TcpStream,
        tcp::{OwnedReadHalf, OwnedWriteHalf},
    },
    sync::{Mutex as AsyncMutex, mpsc, oneshot},
};

use crate::wire::{
    Decoder, Encoder, HANDSHAKE, HEADER_LEN, IdSizes, command, command_set, decode_header,
    encode_command,
};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

type Pending = Mutex<HashMap<u32, oneshot::Sender<Result<Vec<u8>, DebugError>>>>;

pub(crate) struct Connection {
    writer: AsyncMutex<OwnedWriteHalf>,
    pending: Pending,
    next_id: AtomicU32,
    sizes: OnceLock<IdSizes>,
}

impl Connection {
    /// Conecta, cumprimenta o alvo e negocia as larguras de identificador.
    pub(crate) async fn attach(
        address: &str,
        timeout: Option<Duration>,
    ) -> Result<(std::sync::Arc<Self>, mpsc::UnboundedReceiver<Vec<u8>>), DebugError> {
        let connect = TcpStream::connect(address);
        let mut stream = match timeout {
            Some(timeout) => tokio::time::timeout(timeout, connect)
                .await
                .map_err(|_| DebugError::Connect(format!("{address} did not answer in time")))?,
            None => connect.await,
        }
        .map_err(|error| DebugError::Connect(format!("{address}: {error}")))?;
        stream
            .set_nodelay(true)
            .map_err(|error| DebugError::Connect(error.to_string()))?;

        stream
            .write_all(HANDSHAKE)
            .await
            .map_err(|error| DebugError::Connect(error.to_string()))?;
        let mut greeting = [0_u8; HANDSHAKE.len()];
        stream
            .read_exact(&mut greeting)
            .await
            .map_err(|error| DebugError::Connect(error.to_string()))?;
        if greeting != HANDSHAKE {
            return Err(DebugError::Connect(format!(
                "{address} is not a debug port: unexpected handshake"
            )));
        }

        let (reader, writer) = stream.into_split();
        let (events_sender, events_receiver) = mpsc::unbounded_channel();
        let connection = std::sync::Arc::new(Self {
            writer: AsyncMutex::new(writer),
            pending: Mutex::new(HashMap::new()),
            next_id: AtomicU32::new(1),
            sizes: OnceLock::new(),
        });
        tokio::spawn(read_loop(
            reader,
            std::sync::Arc::clone(&connection),
            events_sender,
        ));
        let sizes = connection.read_id_sizes().await?;
        let _ = connection.sizes.set(sizes);
        Ok((connection, events_receiver))
    }

    pub(crate) fn sizes(&self) -> IdSizes {
        self.sizes.get().copied().unwrap_or_default()
    }

    pub(crate) fn encoder(&self) -> Encoder {
        Encoder::new(self.sizes())
    }

    pub(crate) fn decoder<'a>(&self, payload: &'a [u8]) -> Decoder<'a> {
        Decoder::new(payload, self.sizes())
    }

    pub(crate) async fn request(
        &self,
        command_set: u8,
        command: u8,
        payload: Vec<u8>,
    ) -> Result<Vec<u8>, DebugError> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (sender, receiver) = oneshot::channel();
        match self.pending.lock() {
            Ok(mut pending) => {
                pending.insert(id, sender);
            }
            Err(_) => return Err(DebugError::Protocol("pending table is poisoned".to_owned())),
        }
        let packet = encode_command(id, command_set, command, &payload);
        if let Err(error) = self.writer.lock().await.write_all(&packet).await {
            self.forget(id);
            return Err(DebugError::Protocol(error.to_string()));
        }
        match tokio::time::timeout(REQUEST_TIMEOUT, receiver).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err(DebugError::Detached),
            Err(_) => {
                self.forget(id);
                Err(DebugError::Protocol(
                    "the target did not answer in time".to_owned(),
                ))
            }
        }
    }

    fn forget(&self, id: u32) {
        if let Ok(mut pending) = self.pending.lock() {
            pending.remove(&id);
        }
    }

    fn complete(&self, id: u32, result: Result<Vec<u8>, DebugError>) {
        let sender = self
            .pending
            .lock()
            .ok()
            .and_then(|mut pending| pending.remove(&id));
        if let Some(sender) = sender {
            let _ = sender.send(result);
        }
    }

    fn fail_all(&self) {
        let pending = self
            .pending
            .lock()
            .map(|mut pending| pending.drain().collect::<Vec<_>>())
            .unwrap_or_default();
        for (_, sender) in pending {
            let _ = sender.send(Err(DebugError::Detached));
        }
    }

    async fn read_id_sizes(&self) -> Result<IdSizes, DebugError> {
        let payload = self
            .request(
                command_set::VIRTUAL_MACHINE,
                command::VM_ID_SIZES,
                Vec::new(),
            )
            .await?;
        let mut decoder = Decoder::new(&payload, IdSizes::default());
        Ok(IdSizes {
            field: decoder.i32()?.max(1) as usize,
            method: decoder.i32()?.max(1) as usize,
            object: decoder.i32()?.max(1) as usize,
            reference_type: decoder.i32()?.max(1) as usize,
            frame: decoder.i32()?.max(1) as usize,
        })
    }

    /// Encerra a sessão liberando o alvo, que volta a executar normalmente.
    pub(crate) async fn dispose(&self) {
        let _ = self
            .request(
                command_set::VIRTUAL_MACHINE,
                command::VM_DISPOSE,
                Vec::new(),
            )
            .await;
        let _ = self.writer.lock().await.shutdown().await;
        self.fail_all();
    }
}

async fn read_loop(
    mut reader: OwnedReadHalf,
    connection: std::sync::Arc<Connection>,
    events: mpsc::UnboundedSender<Vec<u8>>,
) {
    loop {
        let mut header_bytes = [0_u8; HEADER_LEN];
        if reader.read_exact(&mut header_bytes).await.is_err() {
            break;
        }
        let header = decode_header(&header_bytes);
        let body_len = (header.length as usize).saturating_sub(HEADER_LEN);
        let mut body = vec![0_u8; body_len];
        if body_len > 0 && reader.read_exact(&mut body).await.is_err() {
            break;
        }
        if header.is_reply() {
            let result = if header.error_code == 0 {
                Ok(body)
            } else {
                Err(DebugError::Target(format!(
                    "JDWP error {}",
                    header.error_code
                )))
            };
            connection.complete(header.id, result);
        } else if header.command_set == command_set::EVENT
            && header.command == command::EVENT_COMPOSITE
            && events.send(body).is_err()
        {
            break;
        }
    }
    connection.fail_all();
}
