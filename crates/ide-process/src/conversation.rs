//! Um processo longevo, com quem se troca linhas.
//!
//! É a outra forma de usar um processo externo, e ela **fica ao lado** de
//! `execute`, não no lugar dele. Rodar e coletar continua sendo o certo para
//! `javac`, Maven e `npm run`: eles executam, respondem e morrem. Um analisador
//! de linguagem é o oposto — vive junto com a IDE e mantém estado entre pedidos.
//!
//! Trocar todos por um mecanismo conversacional seria pagar complexidade em quem
//! não precisa. Ver a fase 3a da `23`.

use std::{path::PathBuf, process::Stdio, sync::Arc};

use async_trait::async_trait;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStdin, ChildStdout, Command},
    sync::Mutex,
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

    /// Encerra o processo e libera o que ele segurava.
    ///
    /// É o instrumento cego: fecha a entrada e mata. Um encerramento educado —
    /// pedir ao protocolo que termine — é de quem conhece o protocolo, e não
    /// desta camada.
    async fn shutdown(&self) -> Result<(), ProcessError>;

    /// Se o processo ainda está de pé.
    async fn is_running(&self) -> bool;
}

pub(crate) struct NativeConversation {
    child: Arc<Mutex<Child>>,
    stdin: Mutex<Option<ChildStdin>>,
    stdout: Mutex<Option<BufReader<ChildStdout>>>,
}

impl NativeConversation {
    pub(crate) fn start(request: ProcessRequest) -> Result<Self, ProcessError> {
        if !request.program.is_file() && which(&request.program).is_none() {
            return Err(ProcessError::InvalidProgram(request.program));
        }
        let mut command = Command::new(&request.program);
        command
            .args(&request.args)
            .envs(request.environment)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            // Matar ao soltar é o que impede processo órfão sobrevivendo à IDE.
            .kill_on_drop(true);
        if let Some(directory) = &request.working_directory {
            command.current_dir(directory);
        }
        let mut child = command.spawn()?;
        let stdin = child.stdin.take();
        let stdout = child.stdout.take().map(BufReader::new);

        // A saída de erro é **drenada**, e isso não é zelo: um canal ligado e
        // nunca lido enche o buffer do sistema operacional, e aí o processo
        // filho trava ao escrever nele. Ignorar seria pior do que não ligar.
        if let Some(stderr) = child.stderr.take() {
            tokio::spawn(async move {
                let mut linhas = BufReader::new(stderr).lines();
                while let Ok(Some(linha)) = linhas.next_line().await {
                    tracing::debug!(linha, "saída de erro do processo");
                }
            });
        }

        Ok(Self {
            child: Arc::new(Mutex::new(child)),
            stdin: Mutex::new(stdin),
            stdout: Mutex::new(stdout),
        })
    }
}

#[async_trait]
impl ProcessConversation for NativeConversation {
    async fn send(&self, line: &str) -> Result<(), ProcessError> {
        let mut guarda = self.stdin.lock().await;
        let Some(stdin) = guarda.as_mut() else {
            return Err(ProcessError::ConversationClosed);
        };
        stdin.write_all(line.as_bytes()).await?;
        stdin.write_all(b"\n").await?;
        stdin.flush().await?;
        Ok(())
    }

    async fn receive(&self) -> Result<Option<String>, ProcessError> {
        let mut guarda = self.stdout.lock().await;
        let Some(stdout) = guarda.as_mut() else {
            return Err(ProcessError::ConversationClosed);
        };
        let mut linha = String::new();
        let lidos = stdout.read_line(&mut linha).await?;
        if lidos == 0 {
            return Ok(None);
        }
        // O `\r` do CRLF não faz parte da linha, e deixá-lo sujaria toda
        // comparação de quem consome.
        while linha.ends_with('\n') || linha.ends_with('\r') {
            linha.pop();
        }
        Ok(Some(linha))
    }

    async fn shutdown(&self) -> Result<(), ProcessError> {
        // Fechar a entrada primeiro: quem estiver esperando por mais pedido vê
        // o fim e sai sozinho, o que é mais limpo do que ser morto no meio.
        self.stdin.lock().await.take();
        self.stdout.lock().await.take();
        self.child.lock().await.kill().await?;
        Ok(())
    }

    async fn is_running(&self) -> bool {
        matches!(self.child.lock().await.try_wait(), Ok(None))
    }
}

/// O executável, quando o que veio foi um nome e não um caminho.
fn which(program: &std::path::Path) -> Option<PathBuf> {
    program
        .to_str()
        .filter(|nome| !nome.contains(['/', '\\']))
        .and_then(crate::find_in_path)
}
