#![doc = "Supervisão de processos externos com timeout e cancelamento."]

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
use tokio::{process::{Child, Command}, sync::Mutex};

#[derive(Clone, Debug)]
pub struct ProcessRequest {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub working_directory: Option<PathBuf>,
    pub timeout: Option<Duration>,
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
}

#[derive(Default)]
pub struct NativeProcessSupervisor {
    next_id: AtomicU64,
    children: Arc<Mutex<HashMap<ProcessId, Child>>>,
}

#[async_trait]
impl ProcessSupervisor for NativeProcessSupervisor {
    async fn spawn(&self, request: ProcessRequest) -> Result<ProcessId, ProcessError> {
        if !request.program.is_file() {
            return Err(ProcessError::InvalidProgram(request.program));
        }
        let mut command = Command::new(&request.program);
        command.args(&request.args).kill_on_drop(true);
        if let Some(directory) = &request.working_directory {
            command.current_dir(directory);
        }
        let child = command.spawn()?;
        let id = ProcessId(self.next_id.fetch_add(1, Ordering::Relaxed) + 1);
        self.children.lock().await.insert(id, child);
        Ok(id)
    }

    async fn terminate(&self, process_id: ProcessId) -> Result<(), ProcessError> {
        let mut child = self.children.lock().await.remove(&process_id)
            .ok_or(ProcessError::UnknownProcess(process_id))?;
        child.kill().await?;
        Ok(())
    }

    async fn status(&self, process_id: ProcessId) -> Result<ProcessStatus, ProcessError> {
        let mut children = self.children.lock().await;
        let child = children.get_mut(&process_id)
            .ok_or(ProcessError::UnknownProcess(process_id))?;
        match child.try_wait()? {
            Some(status) => Ok(ProcessStatus::Exited(status.code().unwrap_or(-1))),
            None => Ok(ProcessStatus::Running),
        }
    }
}

#[derive(Debug, Error)]
pub enum ProcessError {
    #[error("invalid process executable: {0}")]
    InvalidProgram(PathBuf),
    #[error("unknown process {0:?}")]
    UnknownProcess(ProcessId),
    #[error("process spawn timed out")]
    Timeout,
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
            }));
        assert!(matches!(result, Err(ProcessError::InvalidProgram(_))));
    }
}
