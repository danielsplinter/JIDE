#![doc = "Perfis e sessões persistentes do terminal integrado via PTY/ConPTY."]

use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};
use std::{
    collections::VecDeque,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver},
    thread,
    time::{Duration, Instant},
};
use thiserror::Error;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShellKind {
    PowerShell,
    Cmd,
    GitBash,
}

impl ShellKind {
    pub const fn label(self) -> &'static str {
        match self {
            Self::PowerShell => "PowerShell",
            Self::Cmd => "CMD",
            Self::GitBash => "Git Bash",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShellProfile {
    pub kind: ShellKind,
    pub executable: PathBuf,
}

impl ShellProfile {
    fn interactive_args(&self) -> &'static [&'static str] {
        match self.kind {
            ShellKind::PowerShell => &[
                "-NoLogo",
                "-NoProfile",
                "-NoExit",
                "-Command",
                "Remove-Module PSReadLine -ErrorAction SilentlyContinue",
            ],
            ShellKind::Cmd => &["/Q"],
            ShellKind::GitBash => &["--login", "-i"],
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TerminalLine {
    pub text: String,
    pub is_error: bool,
}

struct PtyProcess {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn Child + Send + Sync>,
}

impl Drop for PtyProcess {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

pub struct TerminalSession {
    working_directory: PathBuf,
    profile: ShellProfile,
    input: String,
    lines: VecDeque<TerminalLine>,
    max_lines: usize,
    process: PtyProcess,
    output: Receiver<Vec<u8>>,
    pending: String,
    pty_cols: u16,
    pty_rows: u16,
}

impl TerminalSession {
    pub fn discover_profiles() -> Vec<ShellProfile> {
        let mut profiles = vec![
            ShellProfile {
                kind: ShellKind::PowerShell,
                executable: PathBuf::from("powershell.exe"),
            },
            ShellProfile {
                kind: ShellKind::Cmd,
                executable: PathBuf::from("cmd.exe"),
            },
        ];
        if let Some(executable) = detect_git_bash() {
            profiles.push(ShellProfile {
                kind: ShellKind::GitBash,
                executable,
            });
        }
        profiles
    }

    pub fn new(
        working_directory: PathBuf,
        max_lines: usize,
        profile: ShellProfile,
    ) -> Result<Self, TerminalError> {
        if !working_directory.is_dir() {
            return Err(TerminalError::InvalidWorkingDirectory);
        }
        if max_lines == 0 {
            return Err(TerminalError::InvalidCapacity);
        }

        let pair = native_pty_system()
            .openpty(PtySize {
                rows: 24,
                cols: 80,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|error| TerminalError::Pty(error.to_string()))?;
        let mut command = CommandBuilder::new(&profile.executable);
        command.cwd(&working_directory);
        for argument in profile.interactive_args() {
            command.arg(argument);
        }
        let child = pair
            .slave
            .spawn_command(command)
            .map_err(|error| TerminalError::Pty(error.to_string()))?;
        drop(pair.slave);
        let mut reader = pair
            .master
            .try_clone_reader()
            .map_err(|error| TerminalError::Pty(error.to_string()))?;
        let writer = pair
            .master
            .take_writer()
            .map_err(|error| TerminalError::Pty(error.to_string()))?;
        let (sender, output) = mpsc::channel();
        thread::Builder::new()
            .name(format!("terminal-{}-reader", profile.kind.label()))
            .spawn(move || {
                let mut buffer = [0_u8; 4096];
                loop {
                    match reader.read(&mut buffer) {
                        Ok(0) => break,
                        Ok(count) if sender.send(buffer[..count].to_vec()).is_err() => break,
                        Ok(_) => {}
                        Err(_) => break,
                    }
                }
            })?;

        let mut session = Self {
            working_directory,
            profile,
            input: String::new(),
            lines: VecDeque::new(),
            max_lines,
            process: PtyProcess {
                master: pair.master,
                writer,
                child,
            },
            output,
            pending: String::new(),
            pty_cols: 80,
            pty_rows: 24,
        };
        if session.profile.kind == ShellKind::PowerShell {
            let deadline = Instant::now() + Duration::from_secs(3);
            while !session.pending.contains('>') && Instant::now() < deadline {
                session.drain_output();
                thread::sleep(Duration::from_millis(10));
            }
            if !session.pending.contains('>') {
                return Err(TerminalError::InitializationTimeout);
            }
            session.lines.clear();
            session.pending.clear();
        }
        Ok(session)
    }

    pub fn selected_profile(&self) -> &ShellProfile {
        &self.profile
    }
    pub fn input(&self) -> &str {
        &self.input
    }
    pub fn input_mut(&mut self) -> &mut String {
        &mut self.input
    }
    pub fn prompt(&self) -> String {
        let shell_prompt = self.pending.trim();
        if !shell_prompt.is_empty()
            && shell_prompt
                .chars()
                .last()
                .is_some_and(|last| matches!(last, '>' | '$' | '#'))
        {
            shell_prompt.to_owned()
        } else {
            format!("{}>", self.working_directory.display())
        }
    }
    pub fn working_directory(&self) -> &Path {
        &self.working_directory
    }
    pub fn lines(&self) -> impl Iterator<Item = &TerminalLine> {
        self.lines.iter()
    }
    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    pub fn submit(&mut self) -> Result<(), TerminalError> {
        if self.input.trim().is_empty() {
            return Ok(());
        }
        self.process.writer.write_all(self.input.as_bytes())?;
        self.process.writer.write_all(b"\r\n")?;
        self.process.writer.flush()?;
        self.input.clear();
        Ok(())
    }

    pub fn resize(&mut self, cols: u16, rows: u16) -> Result<bool, TerminalError> {
        let cols = cols.max(1);
        let rows = rows.max(1);
        if cols == self.pty_cols && rows == self.pty_rows {
            return Ok(false);
        }
        self.process
            .master
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|error| TerminalError::Pty(error.to_string()))?;
        self.pty_cols = cols;
        self.pty_rows = rows;
        Ok(true)
    }

    pub fn drain_output(&mut self) -> usize {
        let mut received = 0;
        while let Ok(bytes) = self.output.try_recv() {
            received += bytes.len();
            let clean = strip_terminal_controls(&String::from_utf8_lossy(&bytes));
            self.pending.push_str(&clean.replace("\r\n", "\n"));
            self.commit_complete_lines();
        }
        received
    }

    fn commit_complete_lines(&mut self) {
        while let Some(index) = self.pending.find('\n') {
            let mut remainder = self.pending.split_off(index + 1);
            std::mem::swap(&mut self.pending, &mut remainder);
            let line = normalize_terminal_line(&remainder[..remainder.len().saturating_sub(1)]);
            self.push_line(line);
        }
    }

    fn push_line(&mut self, text: String) {
        self.lines.push_back(TerminalLine {
            text,
            is_error: false,
        });
        while self.lines.len() > self.max_lines {
            self.lines.pop_front();
        }
    }
}

fn strip_terminal_controls(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(character) = chars.next() {
        if character == '\u{1b}' {
            if chars.next_if_eq(&'[').is_some() {
                for value in chars.by_ref() {
                    if ('@'..='~').contains(&value) {
                        break;
                    }
                }
            } else if chars.next_if_eq(&']').is_some() {
                let mut escaped = false;
                for value in chars.by_ref() {
                    if value == '\u{7}' || (escaped && value == '\\') {
                        break;
                    }
                    escaped = value == '\u{1b}';
                }
            }
        } else if character == '\u{8}' {
            result.pop();
        } else if character == '\r' {
            result.push('\r');
        } else if character == '\t' || character == '\n' || !character.is_control() {
            result.push(character);
        }
    }
    result
}

fn normalize_terminal_line(raw: &str) -> String {
    raw.trim_end_matches('\r')
        .rsplit('\r')
        .next()
        .unwrap_or_default()
        .to_owned()
}

fn detect_git_bash() -> Option<PathBuf> {
    [
        Path::new(r"C:\Program Files\Git\bin\bash.exe"),
        Path::new(r"C:\Program Files\Git\usr\bin\bash.exe"),
        Path::new(r"C:\Program Files (x86)\Git\bin\bash.exe"),
    ]
    .into_iter()
    .find(|path| path.is_file())
    .map(Path::to_path_buf)
}

#[derive(Debug, Error)]
pub enum TerminalError {
    #[error("terminal working directory does not exist")]
    InvalidWorkingDirectory,
    #[error("terminal history capacity must be positive")]
    InvalidCapacity,
    #[error("terminal PTY failed: {0}")]
    Pty(String),
    #[error("terminal shell initialization timed out")]
    InitializationTimeout,
    #[error("terminal I/O failed: {0}")]
    Io(#[from] std::io::Error),
}

#[cfg(test)]
mod tests {
    use super::*;
    fn wait_for(terminal: &mut TerminalSession, expected: &str) -> bool {
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            terminal.drain_output();
            if terminal.lines().any(|line| line.text.contains(expected))
                || terminal.pending.contains(expected)
            {
                return true;
            }
            thread::sleep(Duration::from_millis(25));
        }
        let output = terminal
            .lines()
            .map(|line| line.text.clone())
            .collect::<Vec<_>>();
        let pending = terminal.pending.clone();
        let status = terminal.process.child.try_wait();
        eprintln!("terminal output: {output:?}; pending: {pending:?}; status: {status:?}");
        false
    }

    fn send(terminal: &mut TerminalSession, command: &str) {
        terminal.input_mut().push_str(command);
        if let Err(error) = terminal.submit() {
            panic!("command submission failed: {error}");
        }
    }

    #[test]
    fn terminal_control_sequences_are_removed() {
        assert_eq!(strip_terminal_controls("\u{1b}[31mred\u{1b}[0m"), "red");
    }

    #[test]
    fn carriage_return_replaces_instead_of_duplicating_a_line() {
        assert_eq!(
            normalize_terminal_line("progress 10%\rprogress 50%\rprogress 100%\r"),
            "progress 100%"
        );
        assert_eq!(normalize_terminal_line("single result\r"), "single result");
    }

    #[cfg(windows)]
    #[test]
    fn powershell_process_preserves_state_between_commands() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let profile = ShellProfile {
            kind: ShellKind::PowerShell,
            executable: PathBuf::from("powershell.exe"),
        };
        let mut terminal = match TerminalSession::new(root, 200, profile) {
            Ok(terminal) => terminal,
            Err(error) => panic!("terminal creation failed: {error}"),
        };
        assert_eq!(terminal.line_count(), 0);
        assert!(terminal.prompt().trim_end().ends_with('>'));
        assert!(matches!(terminal.resize(80, 24), Ok(false)));
        assert!(matches!(terminal.resize(100, 30), Ok(true)));
        assert!(matches!(terminal.resize(100, 30), Ok(false)));
        thread::sleep(Duration::from_millis(500));
        send(&mut terminal, "$env:IDE_PTY_STATE='PERSISTED'");
        send(&mut terminal, "Write-Output \"STATE=$env:IDE_PTY_STATE\"");
        assert!(wait_for(&mut terminal, "STATE=PERSISTED"));
        thread::sleep(Duration::from_millis(100));
        terminal.drain_output();
        assert_eq!(
            terminal
                .lines()
                .filter(|line| line.text.trim() == "STATE=PERSISTED")
                .count(),
            1
        );
        assert!(!terminal.lines().any(|line| line.text.starts_with(">>")));
    }

    #[cfg(windows)]
    #[test]
    fn cmd_process_produces_output() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let mut terminal = match TerminalSession::new(
            root,
            200,
            ShellProfile {
                kind: ShellKind::Cmd,
                executable: PathBuf::from("cmd.exe"),
            },
        ) {
            Ok(terminal) => terminal,
            Err(error) => panic!("terminal creation failed: {error}"),
        };
        thread::sleep(Duration::from_millis(500));
        send(&mut terminal, "echo CMD_PTY_OK");
        assert!(wait_for(&mut terminal, "CMD_PTY_OK"));
    }

    #[cfg(windows)]
    #[test]
    fn powershell_cd_is_interpreted_by_the_persistent_shell() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let mut terminal = match TerminalSession::new(
            root,
            200,
            ShellProfile {
                kind: ShellKind::PowerShell,
                executable: PathBuf::from("powershell.exe"),
            },
        ) {
            Ok(terminal) => terminal,
            Err(error) => panic!("terminal creation failed: {error}"),
        };
        send(&mut terminal, "cd crates");
        send(
            &mut terminal,
            "Write-Output \"LOCATION=$((Get-Location).Path)\"",
        );
        assert!(wait_for(&mut terminal, "LOCATION="));
        assert!(terminal.lines().any(|line| line.text.contains("crates")));
    }
}
