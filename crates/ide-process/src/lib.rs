#![doc = "Supervisão de processos externos com timeout e cancelamento."]
#![doc = ""]
#![doc = "Duas formas, e elas não se substituem: `execute` roda e coleta, para"]
#![doc = "quem responde e morre; `converse` abre uma conversa, para quem vive"]
#![doc = "junto com a IDE. Ver a fase 3a da `23`."]

mod conversation;

pub use conversation::ProcessConversation;

use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use async_trait::async_trait;
use ide_domain::ProcessId;
use thiserror::Error;
use tokio::{
    process::{Child, Command},
    sync::Mutex,
};

#[derive(Clone, Debug)]
pub struct ProcessRequest {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub working_directory: Option<PathBuf>,
    pub timeout: Option<Duration>,
    pub environment: Vec<(String, String)>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProcessOutput {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProcessStatus {
    Running,
    Exited(i32),
}

#[async_trait]
pub trait ProcessSupervisor: Send + Sync {
    async fn spawn(&self, request: ProcessRequest) -> Result<ProcessId, ProcessError>;
    async fn terminate(&self, process_id: ProcessId) -> Result<(), ProcessError>;
    async fn status(&self, process_id: ProcessId) -> Result<ProcessStatus, ProcessError>;
    async fn execute(&self, request: ProcessRequest) -> Result<ProcessOutput, ProcessError>;

    /// Abre uma conversa com um processo que vai continuar rodando.
    ///
    /// O `timeout` do pedido é ignorado aqui: quem conversa não tem prazo para
    /// terminar — quem tem prazo é cada pergunta, e isso é de quem pergunta.
    ///
    /// Tem padrão porque as duas formas são independentes: um supervisor pode
    /// legitimamente saber só rodar e coletar, e os dublês de teste de quem
    /// executa build são exatamente isso. Quem não implementa **recusa**, e não
    /// finge — a recusa é uma resposta, e quem chama já sabe degradar.
    async fn converse(
        &self,
        _request: ProcessRequest,
    ) -> Result<Box<dyn ProcessConversation>, ProcessError> {
        Err(ProcessError::ConversationUnsupported)
    }
}

#[derive(Default)]
pub struct NativeProcessSupervisor {
    next_id: AtomicU64,
    children: Arc<Mutex<HashMap<ProcessId, Child>>>,
    /// Processos de conversas abertas, para quem precisa medi-los.
    ///
    /// A lista mora aqui porque é aqui que eles nascem. Duplicá-la noutro lugar
    /// criaria duas verdades sobre a mesma coisa, e uma envelheceria.
    conversations: Arc<std::sync::Mutex<Vec<ProcessId>>>,
}

impl NativeProcessSupervisor {
    /// Os processos das conversas que ainda respondem.
    ///
    /// Quem morreu sai da lista na próxima consulta: perguntar é o momento em
    /// que se descobre, e um relógio próprio aqui seria uma segunda fonte de
    /// tempo no processo.
    #[must_use]
    pub fn live_conversations(&self) -> Vec<ProcessId> {
        let Ok(mut conversas) = self.conversations.lock() else {
            return Vec::new();
        };
        conversas.retain(|id| processo_vivo(*id));
        conversas.clone()
    }
}

/// Se o sistema ainda conhece este processo.
fn processo_vivo(id: ProcessId) -> bool {
    use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};
    let pid = Pid::from_u32(id.0 as u32);
    let mut sistema = System::new();
    sistema.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[pid]),
        true,
        ProcessRefreshKind::nothing(),
    );
    sistema.process(pid).is_some()
}

/// Impede que o processo filho abra uma janela de console.
///
/// No Windows, um processo **sem** console — que é o que a IDE é no binário de
/// produção — faz o sistema criar uma janela nova para cada filho de subsistema
/// console. O analisador externo aparecia como um terminal preto ao lado da
/// IDE, com o nome do executável no título.
///
/// Vale também para o binário com console: ali o filho herdava o terminal da
/// IDE, e o que ele escrevesse se misturaria ao log. A saída dele já vem por
/// canos — é assim que a IDE a lê —, então não se perde nada.
///
/// Fora do Windows não há o que fazer, e a função existe justamente para que
/// quem chama não precise saber disso.
pub fn sem_janela_de_console(command: &mut Command) {
    #[cfg(windows)]
    {
        command.creation_flags(SEM_JANELA);
    }
    #[cfg(not(windows))]
    let _ = command;
}

/// O mesmo, para quem monta o comando com a biblioteca padrão.
///
/// Uma pergunta rápida e síncrona — "qual é a sua versão?" — não justifica um
/// runtime assíncrono, e é justamente ela que pisca uma janela na cara de quem
/// abre a IDE.
pub fn sem_janela_de_console_sincrono(command: &mut std::process::Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(SEM_JANELA);
    }
    #[cfg(not(windows))]
    let _ = command;
}

/// `CREATE_NO_WINDOW`, do Win32.
#[cfg(windows)]
const SEM_JANELA: u32 = 0x0800_0000;

#[async_trait]
impl ProcessSupervisor for NativeProcessSupervisor {
    async fn spawn(&self, request: ProcessRequest) -> Result<ProcessId, ProcessError> {
        if !request.program.is_file() {
            return Err(ProcessError::InvalidProgram(request.program));
        }
        let mut command = Command::new(&request.program);
        command.args(&request.args).kill_on_drop(true);
        command.envs(request.environment);
        sem_janela_de_console(&mut command);
        if let Some(directory) = &request.working_directory {
            command.current_dir(directory);
        }
        let child = command.spawn()?;
        let id = ProcessId(self.next_id.fetch_add(1, Ordering::Relaxed) + 1);
        self.children.lock().await.insert(id, child);
        Ok(id)
    }

    async fn terminate(&self, process_id: ProcessId) -> Result<(), ProcessError> {
        let mut child = self
            .children
            .lock()
            .await
            .remove(&process_id)
            .ok_or(ProcessError::UnknownProcess(process_id))?;
        child.kill().await?;
        Ok(())
    }

    async fn status(&self, process_id: ProcessId) -> Result<ProcessStatus, ProcessError> {
        let mut children = self.children.lock().await;
        let child = children
            .get_mut(&process_id)
            .ok_or(ProcessError::UnknownProcess(process_id))?;
        match child.try_wait()? {
            Some(status) => Ok(ProcessStatus::Exited(status.code().unwrap_or(-1))),
            None => Ok(ProcessStatus::Running),
        }
    }

    async fn converse(
        &self,
        request: ProcessRequest,
    ) -> Result<Box<dyn ProcessConversation>, ProcessError> {
        let conversa = conversation::NativeConversation::start(request)?;
        if let Some(id) = conversa.process_id()
            && let Ok(mut conversas) = self.conversations.lock()
        {
            conversas.push(id);
        }
        Ok(Box::new(conversa))
    }

    async fn execute(&self, request: ProcessRequest) -> Result<ProcessOutput, ProcessError> {
        if !request.program.is_file() {
            return Err(ProcessError::InvalidProgram(request.program));
        }
        let mut command = Command::new(&request.program);
        command
            .args(&request.args)
            .envs(request.environment)
            .kill_on_drop(true);
        sem_janela_de_console(&mut command);
        if let Some(directory) = &request.working_directory {
            command.current_dir(directory);
        }
        let output = if let Some(timeout) = request.timeout {
            tokio::time::timeout(timeout, command.output())
                .await
                .map_err(|_| ProcessError::Timeout)??
        } else {
            command.output().await?
        };
        Ok(ProcessOutput {
            exit_code: output.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        })
    }
}

/// Procura um executável no `PATH`, testando as extensões da plataforma.
///
/// Ferramentas externas costumam ser distribuídas como scripts (`mvn.cmd`,
/// `gradle.bat`), portanto o nome é combinado com `PATHEXT` no Windows.
#[must_use]
pub fn find_in_path(name: &str) -> Option<PathBuf> {
    let extensions: Vec<String> = if cfg!(windows) {
        std::env::var("PATHEXT")
            .unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".to_owned())
            .split(';')
            .filter(|extension| !extension.is_empty())
            .map(str::to_owned)
            .collect()
    } else {
        Vec::new()
    };
    for directory in std::env::split_paths(&std::env::var_os("PATH")?) {
        let candidate = directory.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
        for extension in &extensions {
            let candidate = directory.join(format!("{name}{extension}"));
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

#[derive(Debug, Error)]
pub enum ProcessError {
    #[error("invalid process executable: {0}")]
    InvalidProgram(PathBuf),
    #[error("unknown process {0:?}")]
    UnknownProcess(ProcessId),
    #[error("process spawn timed out")]
    Timeout,
    #[error("este supervisor não abre conversas")]
    ConversationUnsupported,
    #[error("a conversa com o processo já foi encerrada")]
    ConversationClosed,
    #[error("process I/O failed: {0}")]
    Io(#[from] std::io::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_missing_executable() {
        let runtime = match tokio::runtime::Builder::new_current_thread().build() {
            Ok(runtime) => runtime,
            Err(error) => panic!("runtime creation failed: {error}"),
        };
        let supervisor = NativeProcessSupervisor::default();
        let result = runtime.block_on(supervisor.spawn(ProcessRequest {
            program: PathBuf::from("definitely-missing-ide-tool"),
            args: Vec::new(),
            working_directory: None,
            timeout: None,
            environment: Vec::new(),
        }));
        assert!(matches!(result, Err(ProcessError::InvalidProgram(_))));
    }

    #[cfg(windows)]
    #[test]
    fn execute_captures_stdout_and_exit_code() {
        let runtime = match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(runtime) => runtime,
            Err(error) => panic!("runtime creation failed: {error}"),
        };
        let supervisor = NativeProcessSupervisor::default();
        let result = runtime.block_on(supervisor.execute(ProcessRequest {
            program: PathBuf::from(r"C:\Windows\System32\cmd.exe"),
            args: vec!["/C".to_owned(), "echo process-ok".to_owned()],
            working_directory: None,
            timeout: Some(Duration::from_secs(5)),
            environment: Vec::new(),
        }));
        assert!(matches!(
            result,
            Ok(output) if output.exit_code == 0 && output.stdout.contains("process-ok")
        ));
    }
}
