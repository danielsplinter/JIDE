//! Um processo longevo, com quem se troca linhas.
//!
//! É a outra forma de usar um processo externo, e ela **fica ao lado** de
//! `execute`, não no lugar dele. Rodar e coletar continua sendo o certo para
//! `javac`, Maven e `npm run`: eles executam, respondem e morrem. Um analisador
//! de linguagem é o oposto — vive junto com a IDE e mantém estado entre pedidos.
//!
//! Trocar todos por um mecanismo conversacional seria pagar complexidade em quem
//! não precisa. Ver a fase 3a da `23`.
//!
//! # A conversa é dona da própria thread
//!
//! Os canais de um processo filho ficam presos ao **reator do runtime que os
//! criou**: lê-los de outro runtime não funciona. E um `tokio::spawn` num
//! runtime de thread única só progride enquanto alguém está dentro de um
//! `block_on` — o que faria a leitura avançar apenas durante uma chamada, e
//! travar quando a resposta chegasse entre dois pedidos.
//!
//! Por isso a conversa sobe uma thread própria, com runtime próprio, e é ali que
//! o processo nasce e todo I/O acontece. Quem chama fala por canal, e canal não
//! precisa de reator: funciona de qualquer runtime, ou de nenhum.
//!
//! Isto foi descoberto rodando contra o `tsserver` de verdade, e não seria
//! encontrado por teste de unidade nenhum. Ver a fase 3c da `23`.

use std::{
    path::PathBuf,
    process::Stdio,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
};

use async_trait::async_trait;
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    process::Command,
    sync::{mpsc, oneshot},
};

use crate::{ProcessError, ProcessRequest};

/// Conversa aberta com um processo que continua rodando.
#[async_trait]
pub trait ProcessConversation: Send + Sync {
    /// Escreve uma linha na entrada do processo.
    async fn send(&self, line: &str) -> Result<(), ProcessError>;

    /// Lê a próxima linha da saída.
    ///
    /// `None` é o fim da saída — o processo fechou o canal, o que na prática
    /// quer dizer que ele morreu ou está morrendo. **É este o sinal de morte**,
    /// e quem chama trata dele degradando, e não esperando para sempre.
    async fn receive(&self) -> Result<Option<String>, ProcessError>;

    /// Lê exatamente `bytes` da saída.
    ///
    /// Serve a quem enquadra a mensagem pelo tamanho, e não pela quebra de
    /// linha. É o que o LSP faz, e o que o `tsserver` faz: um cabeçalho
    /// `Content-Length`, uma linha em branco, e o corpo com o tamanho anunciado.
    ///
    /// Ler o corpo com `receive` funcionaria **por acidente**, enquanto nenhum
    /// JSON trouxesse quebra de linha dentro de uma string. Uma resposta de
    /// completação que carregue trecho de código traz, e aí a mensagem seria
    /// partida ao meio sem erro nenhum a apontar.
    ///
    /// Descoberto sondando o `tsserver` de verdade, e não lendo documentação:
    /// a fase 3a nasceu supondo que processo longevo conversa por linha, e a
    /// suposição passou sem prova. Ver a fase 3c da `23`.
    ///
    /// `None` é o fim da saída, inclusive quando ele chega no meio de uma
    /// mensagem: metade de mensagem é morte, e não conteúdo.
    async fn receive_exact(&self, bytes: usize) -> Result<Option<Vec<u8>>, ProcessError> {
        let _ = bytes;
        Err(ProcessError::ConversationUnsupported)
    }

    /// Encerra o processo e libera o que ele segurava.
    ///
    /// É o instrumento cego: fecha a entrada e mata. Um encerramento educado —
    /// pedir ao protocolo que termine — é de quem conhece o protocolo, e não
    /// desta camada.
    async fn shutdown(&self) -> Result<(), ProcessError>;

    /// Se o processo ainda está de pé.
    async fn is_running(&self) -> bool;

    /// O processo do sistema, para quem precisa medi-lo.
    ///
    /// `None` quando não há um, ou quando o sistema não o informa. Medir é
    /// opcional — uma conversa que não diga seu processo continua servindo.
    fn process_id(&self) -> Option<ide_domain::ProcessId> {
        None
    }
}

/// O que se pede ao lado que escreve.
///
/// **Escrita e leitura têm canais separados, e não é detalhe.** Numa fila só,
/// uma leitura que espera resposta bloqueia a escrita enfileirada atrás dela — e
/// a escrita bloqueada é justamente o pedido que produziria a resposta
/// esperada. O impasse não aparece sempre: depende de quem chega primeiro, o que
/// o torna pior de diagnosticar. Ver a fase 3c da `23`.
enum Escrita {
    Linha(String, oneshot::Sender<Result<(), ProcessError>>),
    Encerrar(oneshot::Sender<Result<(), ProcessError>>),
}

/// O que se pede ao lado que lê.
enum Leitura {
    Linha(oneshot::Sender<Result<Option<String>, ProcessError>>),
    Bytes(usize, oneshot::Sender<Result<Option<Vec<u8>>, ProcessError>>),
}

pub(crate) struct NativeConversation {
    escritas: mpsc::UnboundedSender<Escrita>,
    leituras: mpsc::UnboundedSender<Leitura>,
    vivo: Arc<AtomicBool>,
    /// O processo que esta conversa criou, para quem precisa medi-lo.
    ///
    /// `None` quando o sistema não o informa. Medir é opcional; conversar não.
    pid: Option<u32>,
}

impl NativeConversation {
    pub(crate) fn start(request: ProcessRequest) -> Result<Self, ProcessError> {
        if !request.program.is_file() && which(&request.program).is_none() {
            return Err(ProcessError::InvalidProgram(request.program));
        }
        let (escritas, receptor_escritas) = mpsc::unbounded_channel();
        let (leituras, receptor_leituras) = mpsc::unbounded_channel();
        let (pronto, aberto) = std::sync::mpsc::sync_channel::<Result<Option<u32>, ProcessError>>(1);
        let vivo = Arc::new(AtomicBool::new(true));
        let sinal = Arc::clone(&vivo);

        thread::Builder::new()
            .name("process-conversation".to_owned())
            .spawn(move || {
                conversa(request, receptor_escritas, receptor_leituras, &pronto, &sinal);
            })?;

        // A partida é esperada aqui para que um executável inválido vire erro de
        // quem chamou, e não uma conversa que nunca responde.
        match aberto.recv() {
            Ok(Ok(pid)) => Ok(Self {
                escritas,
                leituras,
                vivo,
                pid,
            }),
            Ok(Err(erro)) => Err(erro),
            Err(_) => Err(ProcessError::ConversationClosed),
        }
    }

    async fn escreve<T>(
        &self,
        montar: impl FnOnce(oneshot::Sender<Result<T, ProcessError>>) -> Escrita + Send,
    ) -> Result<T, ProcessError> {
        let (remetente, receptor) = oneshot::channel();
        self.escritas
            .send(montar(remetente))
            .map_err(|_| ProcessError::ConversationClosed)?;
        receptor
            .await
            .map_err(|_| ProcessError::ConversationClosed)?
    }

    async fn le<T>(
        &self,
        montar: impl FnOnce(oneshot::Sender<Result<T, ProcessError>>) -> Leitura + Send,
    ) -> Result<T, ProcessError> {
        let (remetente, receptor) = oneshot::channel();
        self.leituras
            .send(montar(remetente))
            .map_err(|_| ProcessError::ConversationClosed)?;
        receptor
            .await
            .map_err(|_| ProcessError::ConversationClosed)?
    }
}

/// A thread dona do processo: aqui ele nasce, e aqui todo I/O acontece.
///
/// Os dois lados rodam **em paralelo** dentro deste runtime. Um laço só, com
/// leitura e escrita na mesma fila, trava: a leitura fica esperando resposta e a
/// escrita que produziria essa resposta espera atrás dela.
fn conversa(
    request: ProcessRequest,
    mut escritas: mpsc::UnboundedReceiver<Escrita>,
    mut leituras: mpsc::UnboundedReceiver<Leitura>,
    pronto: &std::sync::mpsc::SyncSender<Result<Option<u32>, ProcessError>>,
    vivo: &AtomicBool,
) {
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(erro) => {
            let _ = pronto.send(Err(ProcessError::Io(erro)));
            return;
        }
    };

    runtime.block_on(async move {
        let mut command = Command::new(&request.program);
        command
            .args(&request.args)
            .envs(request.environment)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            // Matar ao soltar é o que impede processo órfão sobrevivendo à IDE.
            .kill_on_drop(true);
        if let Some(diretorio) = &request.working_directory {
            command.current_dir(diretorio);
        }
        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(erro) => {
                let _ = pronto.send(Err(ProcessError::Io(erro)));
                return;
            }
        };
        let mut stdin = child.stdin.take();
        let mut stdout = child.stdout.take().map(BufReader::new);

        // A saída de erro é **drenada**, e isso não é zelo: um canal ligado e
        // nunca lido enche o buffer do sistema operacional, e aí o processo
        // filho trava ao escrever nele. Aqui o `spawn` é seguro porque o runtime
        // é desta thread e vive dentro deste `block_on`.
        if let Some(stderr) = child.stderr.take() {
            tokio::spawn(async move {
                let mut linhas = BufReader::new(stderr).lines();
                while let Ok(Some(linha)) = linhas.next_line().await {
                    tracing::debug!(linha, "saída de erro do processo");
                }
            });
        }
        let _ = pronto.send(Ok(child.id()));

        // O lado que lê vive numa tarefa própria: enquanto ele espera por uma
        // linha que ainda não chegou, o lado que escreve continua atendendo.
        let lendo = tokio::spawn(async move {
            while let Some(pedido) = leituras.recv().await {
                let Some(saida) = stdout.as_mut() else {
                    match pedido {
                        Leitura::Linha(resposta) => {
                            let _ = resposta.send(Err(ProcessError::ConversationClosed));
                        }
                        Leitura::Bytes(_, resposta) => {
                            let _ = resposta.send(Err(ProcessError::ConversationClosed));
                        }
                    }
                    continue;
                };
                match pedido {
                    Leitura::Linha(resposta) => {
                        let _ = resposta.send(ler_linha(saida).await);
                    }
                    Leitura::Bytes(quantos, resposta) => {
                        let _ = resposta.send(ler_bytes(saida, quantos).await);
                    }
                }
            }
        });

        while let Some(pedido) = escritas.recv().await {
            match pedido {
                Escrita::Linha(linha, resposta) => {
                    let resultado = match stdin.as_mut() {
                        Some(entrada) => escrever(entrada, &linha).await,
                        None => Err(ProcessError::ConversationClosed),
                    };
                    let _ = resposta.send(resultado);
                }
                Escrita::Encerrar(resposta) => {
                    // Fechar a entrada primeiro: quem estiver esperando por mais
                    // pedido vê o fim e sai sozinho, o que é mais limpo do que
                    // ser morto no meio.
                    stdin.take();
                    vivo.store(false, Ordering::Release);
                    let resultado = child.kill().await.map_err(ProcessError::Io);
                    let _ = resposta.send(resultado);
                    lendo.abort();
                    return;
                }
            }
        }
        // O último dono da conversa a soltou: o processo cai junto.
        vivo.store(false, Ordering::Release);
        let _ = child.kill().await;
        lendo.abort();
    });
}

async fn escrever(
    entrada: &mut tokio::process::ChildStdin,
    linha: &str,
) -> Result<(), ProcessError> {
    entrada.write_all(linha.as_bytes()).await?;
    entrada.write_all(b"\n").await?;
    entrada.flush().await?;
    Ok(())
}

async fn ler_linha(
    saida: &mut BufReader<tokio::process::ChildStdout>,
) -> Result<Option<String>, ProcessError> {
    let mut linha = String::new();
    let lidos = saida.read_line(&mut linha).await?;
    if lidos == 0 {
        return Ok(None);
    }
    // O `\r` do CRLF não faz parte da linha, e deixá-lo sujaria toda comparação
    // de quem consome.
    while linha.ends_with('\n') || linha.ends_with('\r') {
        linha.pop();
    }
    Ok(Some(linha))
}

async fn ler_bytes(
    saida: &mut BufReader<tokio::process::ChildStdout>,
    quantos: usize,
) -> Result<Option<Vec<u8>>, ProcessError> {
    let mut corpo = vec![0_u8; quantos];
    match saida.read_exact(&mut corpo).await {
        Ok(_) => Ok(Some(corpo)),
        // Fim da saída no meio da mensagem: o processo morreu enquanto escrevia,
        // e meia mensagem não é resposta.
        Err(erro) if erro.kind() == std::io::ErrorKind::UnexpectedEof => Ok(None),
        Err(erro) => Err(ProcessError::Io(erro)),
    }
}

#[async_trait]
impl ProcessConversation for NativeConversation {
    async fn send(&self, line: &str) -> Result<(), ProcessError> {
        let linha = line.to_owned();
        self.escreve(|resposta| Escrita::Linha(linha, resposta)).await
    }

    async fn receive(&self) -> Result<Option<String>, ProcessError> {
        let resultado = self.le(Leitura::Linha).await;
        if matches!(resultado, Ok(None)) {
            self.vivo.store(false, Ordering::Release);
        }
        resultado
    }

    async fn receive_exact(&self, bytes: usize) -> Result<Option<Vec<u8>>, ProcessError> {
        let resultado = self.le(|resposta| Leitura::Bytes(bytes, resposta)).await;
        if matches!(resultado, Ok(None)) {
            self.vivo.store(false, Ordering::Release);
        }
        resultado
    }

    async fn shutdown(&self) -> Result<(), ProcessError> {
        self.escreve(Escrita::Encerrar).await
    }

    async fn is_running(&self) -> bool {
        self.vivo.load(Ordering::Acquire)
    }

    fn process_id(&self) -> Option<ide_domain::ProcessId> {
        self.pid.map(|pid| ide_domain::ProcessId(u64::from(pid)))
    }
}

/// O executável, quando o que veio foi um nome e não um caminho.
fn which(program: &std::path::Path) -> Option<PathBuf> {
    program
        .to_str()
        .filter(|nome| !nome.contains(['/', '\\']))
        .and_then(crate::find_in_path)
}
