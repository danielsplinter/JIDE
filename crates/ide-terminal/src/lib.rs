#![doc = "Modelo e execução de comandos do terminal integrado."]

use std::{collections::VecDeque, path::PathBuf, process::Command};
use thiserror::Error;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TerminalLine {
    pub text: String,
    pub is_error: bool,
}

pub struct TerminalSession {
    working_directory: PathBuf,
    lines: VecDeque<TerminalLine>,
    max_lines: usize,
}

impl TerminalSession {
    pub fn new(working_directory: PathBuf, max_lines: usize) -> Result<Self, TerminalError> {
        if !working_directory.is_dir() { return Err(TerminalError::InvalidWorkingDirectory); }
        if max_lines == 0 { return Err(TerminalError::InvalidCapacity); }
        Ok(Self { working_directory, lines: VecDeque::new(), max_lines })
    }

    pub fn execute(&mut self, program: &str, args: &[String]) -> Result<i32, TerminalError> {
        if program.trim().is_empty() { return Err(TerminalError::InvalidProgram); }
        let output = Command::new(program)
            .args(args)
            .current_dir(&self.working_directory)
            .output()?;
        self.push_output(&String::from_utf8_lossy(&output.stdout), false);
        self.push_output(&String::from_utf8_lossy(&output.stderr), true);
        Ok(output.status.code().unwrap_or(-1))
    }

    pub fn lines(&self) -> impl Iterator<Item = &TerminalLine> { self.lines.iter() }

    fn push_output(&mut self, text: &str, is_error: bool) {
        for line in text.lines() {
            self.lines.push_back(TerminalLine { text: line.to_owned(), is_error });
            while self.lines.len() > self.max_lines { self.lines.pop_front(); }
        }
    }
}

#[derive(Debug, Error)]
pub enum TerminalError {
    #[error("terminal working directory does not exist")]
    InvalidWorkingDirectory,
    #[error("terminal history capacity must be positive")]
    InvalidCapacity,
    #[error("program cannot be empty")]
    InvalidProgram,
    #[error("terminal process failed: {0}")]
    Io(#[from] std::io::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_zero_history() {
        assert!(matches!(
            TerminalSession::new(PathBuf::from("."), 0),
            Err(TerminalError::InvalidCapacity)
        ));
    }
}
