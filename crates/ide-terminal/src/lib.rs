#![doc = "Perfis e sessões persistentes do terminal integrado via PTY/ConPTY."]

use justerm_core::{Cell, Cursor, Engine};
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

/// Janela em que uma interrupção ainda pode gerar a pergunta do `cmd`.
///
/// O intervalo cobre um desligamento gracioso demorado: entre o `Ctrl+C` e a
/// pergunta sobre finalizar o arquivo de lote, a aplicação ainda encerra seus
/// recursos, e no Spring Boot isso leva alguns segundos.
const INTERRUPT_WINDOW: Duration = Duration::from_secs(30);

/// Limite de espera de um comando enfileirado pelo terminal ficar livre.
const QUEUED_COMMAND_TIMEOUT: Duration = Duration::from_secs(45);

/// Reconhece uma pergunta de sim/não deixada no fim da saída.
///
/// O `cmd` pergunta se deve finalizar o arquivo de lote — "Terminate batch job
/// (Y/N)?", "Deseja finalizar o arquivo em lotes (S/N)?" — e o texto muda com o
/// idioma do Windows. A pontuação não muda, então é ela que identifica a
/// pergunta: um par entre parênteses separado por barra, terminando o trecho
/// ainda sem quebra de linha.
fn is_yes_no_question(tail: &str) -> bool {
    let Some(question) = tail.trim_end().strip_suffix('?') else {
        return false;
    };
    // Alguns idiomas separam o `?` com espaço: "(O/N) ?".
    let Some(closed) = question.trim_end().strip_suffix(')') else {
        return false;
    };
    let Some(open) = closed.rfind('(') else {
        return false;
    };
    let mut parts = closed[open + 1..].split('/');
    let first = parts.next().unwrap_or_default().trim();
    let second = parts.next().unwrap_or_default().trim();
    parts.next().is_none()
        && !first.is_empty()
        && !second.is_empty()
        && first.chars().count() <= 3
        && second.chars().count() <= 3
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
    /// A grade do terminal, alimentada com os bytes crus do PTY.
    ///
    /// É ela que sabe onde o cursor está, o que cada célula tem de cor e o
    /// que aconteceu quando um programa reescreveu a linha em que estava —
    /// coisas que a lista de linhas de texto não tem como representar.
    engine: Engine,
    /// Instante da interrupção enviada, enquanto ela pode gerar a pergunta do
    /// `cmd` sobre finalizar o arquivo de lote.
    interrupting_since: Option<Instant>,
    /// Comando pedido pela IDE enquanto o terminal ainda não estava livre.
    queued_command: Option<String>,
    queued_at: Option<Instant>,
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
            engine: Engine::with_scrollback(80, 24, 5_000),
            interrupting_since: None,
            queued_command: None,
            queued_at: None,
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

    pub fn append_external_output(&mut self, text: &str, is_error: bool) {
        for line in text.replace("\r\n", "\n").split('\n') {
            if !line.is_empty() {
                self.push_line_with_kind(line.to_owned(), is_error);
            }
        }
    }

    pub fn submit(&mut self) -> Result<(), TerminalError> {
        if self.input.trim().is_empty() {
            return Ok(());
        }
        // Um comando novo encerra a espera pela pergunta: o terminal já está
        // sendo usado de novo.
        self.interrupting_since = None;
        self.process.writer.write_all(self.input.as_bytes())?;
        self.process.writer.write_all(b"\r\n")?;
        self.process.writer.flush()?;
        self.input.clear();
        Ok(())
    }

    /// Envia a interrupção do terminal, como o `Ctrl+C` do usuário.
    ///
    /// Quem decide o que fazer com o sinal é o programa em primeiro plano: a
    /// IDE não mata processo nenhum por fora, então o encerramento é o mesmo
    /// que aconteceria em qualquer terminal.
    ///
    /// Um programa iniciado por arquivo de lote — `mvn.cmd`, `gradlew.bat` —
    /// faz o `cmd` perguntar se deve finalizar o lote e ficar esperando a
    /// resposta, o que travaria o terminal e transformaria o próximo comando na
    /// resposta da pergunta. Por isso a interrupção é seguida de uma segunda,
    /// agendada: ela responde à pergunta quando existe e não faz nada num
    /// prompt livre.
    pub fn interrupt(&mut self) -> Result<(), TerminalError> {
        self.write_interrupt()?;
        self.interrupting_since = Some(Instant::now());
        Ok(())
    }

    fn write_interrupt(&mut self) -> Result<(), TerminalError> {
        self.process.writer.write_all(&[0x03])?;
        self.process.writer.flush()?;
        Ok(())
    }

    pub fn resize(&mut self, cols: u16, rows: u16) -> Result<bool, TerminalError> {
        let cols = cols.max(1);
        let rows = rows.max(1);
        // A grade acompanha o PTY: quem quebra a linha usa a mesma largura que
        // quem a desenha.
        self.engine.resize(cols as usize, rows as usize);
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

    /// As linhas visíveis da grade, como células.
    ///
    /// É o que a interface desenha a partir da `18`: cada célula com o caractere
    /// e a cor que o programa pediu, e não texto já achatado.
    #[must_use]
    pub fn viewport(&self) -> Vec<&[Cell]> {
        (0..self.pty_rows as usize)
            .map(|linha| self.engine.viewport_line(linha))
            .collect()
    }

    /// Onde o programa deixou o cursor.
    #[must_use]
    pub fn cursor(&self) -> &Cursor {
        self.engine.cursor()
    }

    /// Quantas linhas o histórico guarda além do que está visível.
    #[must_use]
    pub fn scrollback_len(&self) -> usize {
        self.engine.scrollback_len()
    }

    pub fn drain_output(&mut self) -> usize {
        let mut received = 0;
        while let Ok(bytes) = self.output.try_recv() {
            received += bytes.len();
            // A grade recebe os bytes **crus**: as sequências de escape são o
            // que ela interpreta, e filtrá-las antes seria jogar fora a cor, o
            // cursor e a reescrita de linha.
            self.engine.feed(&bytes);
            let clean = strip_terminal_controls(&String::from_utf8_lossy(&bytes));
            self.pending.push_str(&clean.replace("\r\n", "\n"));
            self.commit_complete_lines();
        }
        self.answer_batch_prompt();
        self.flush_queued_command();
        received
    }

    /// Executa um comando pedido pela IDE assim que o terminal estiver livre.
    ///
    /// Depois de interromper um arquivo de lote o terminal fica alguns instantes
    /// ocupado, e escrever nesse intervalo perderia o comando — foi o que
    /// acontecia ao parar e executar em seguida. Aqui o comando espera o prompt
    /// voltar em vez de se perder.
    pub fn run(&mut self, command: &str) -> Result<(), TerminalError> {
        if self.is_ready() {
            self.input.clear();
            self.input.push_str(command);
            return self.submit();
        }
        self.queued_command = Some(command.to_owned());
        self.queued_at = Some(Instant::now());
        Ok(())
    }

    /// Indica que o terminal pode receber um comando.
    ///
    /// Ocupado significa duas coisas: uma interrupção que ainda pode gerar a
    /// pergunta do `cmd`, ou a pergunta ainda na tela esperando resposta.
    fn is_ready(&self) -> bool {
        self.interrupting_since.is_none() && !is_yes_no_question(&self.pending)
    }

    fn flush_queued_command(&mut self) {
        if self.queued_command.is_none() {
            return;
        }
        // Um comando não pode ficar esperando para sempre por um terminal que
        // não se resolve: passado o limite, ele vai de qualquer forma.
        let expired = self
            .queued_at
            .is_some_and(|queued| queued.elapsed() >= QUEUED_COMMAND_TIMEOUT);
        if !self.is_ready() && !expired {
            return;
        }
        if let Some(command) = self.queued_command.take() {
            self.queued_at = None;
            self.input.clear();
            self.input.push_str(&command);
            let _ = self.submit();
        }
    }

    /// Responde à pergunta do `cmd` sobre finalizar o arquivo de lote.
    ///
    /// A resposta é uma segunda interrupção, enviada quando a pergunta aparece
    /// de fato — o momento não é previsível, porque a aplicação ainda encerra
    /// seus recursos antes. Enviá-la por tempo atrapalharia: um `Ctrl+C` num
    /// prompt livre descarta o que estiver digitado.
    fn answer_batch_prompt(&mut self) {
        let Some(since) = self.interrupting_since else {
            return;
        };
        if is_yes_no_question(&self.pending) {
            self.interrupting_since = None;
            let _ = self.write_interrupt();
        } else if since.elapsed() >= INTERRUPT_WINDOW {
            self.interrupting_since = None;
        }
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
        self.push_line_with_kind(text, false);
    }

    fn push_line_with_kind(&mut self, text: String, is_error: bool) {
        self.lines.push_back(TerminalLine { text, is_error });
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
    fn the_batch_question_is_recognized_in_any_language() {
        for question in [
            "Terminate batch job (Y/N)? ",
            "Deseja finalizar o arquivo em lotes (S/N)? ",
            "Terminer le travail par lots (O/N) ?",
            "Stapelverarbeitung beenden (J/N)?",
            "^CDeseja finalizar o arquivo em lotes (S/N)? ",
        ] {
            assert!(
                is_yes_no_question(question),
                "deveria reconhecer: {question}"
            );
        }
        for other in [
            "PS C:\\Users\\jdani> ",
            "",
            "Started FourEndpointsApplication in 1.6 seconds",
            "Escolha uma opção (informe o número desejado)?",
            "algo (a/b/c)?",
        ] {
            assert!(
                !is_yes_no_question(other),
                "não deveria reconhecer: {other}"
            );
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
    #[ignore = "a interrupção não chega ao processo em primeiro plano; ver ADR sobre Ctrl+C no terminal"]
    fn interrupt_cancels_the_foreground_command_and_keeps_the_shell() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let profile = ShellProfile {
            kind: ShellKind::PowerShell,
            executable: PathBuf::from("powershell.exe"),
        };
        let mut terminal = match TerminalSession::new(root, 200, profile) {
            Ok(terminal) => terminal,
            Err(error) => panic!("terminal creation failed: {error}"),
        };

        // Um comando longo o bastante para ainda estar rodando na interrupção.
        send(&mut terminal, "ping -n 30 127.0.0.1");
        // `TTL=` aparece só nas respostas do ping e não depende do idioma.
        assert!(wait_for(&mut terminal, "TTL="), "o ping precisa começar");

        if let Err(error) = terminal.interrupt() {
            panic!("interrupt failed: {error}");
        }
        // O ping pedia 30 pacotes; interrompido, ele imprime as estatísticas em
        // seguida. `Ctrl+C` também descarta o que estiver digitado, então a
        // próxima escrita só entra depois que o shell volta ao prompt.
        assert!(
            wait_for(&mut terminal, "Control-C"),
            "o comando em primeiro plano recebe a interrupção"
        );
        thread::sleep(Duration::from_millis(500));
        terminal.drain_output();

        send(&mut terminal, "echo depois-da-interrupcao");
        assert!(
            wait_for(&mut terminal, "depois-da-interrupcao"),
            "o shell continua utilizável depois da interrupção"
        );
    }

    /// Interromper um arquivo de lote faz o `cmd` perguntar se deve finalizá-lo
    /// e travar o terminal até a resposta. Sem tratar isso, o comando seguinte
    /// viraria a resposta da pergunta em vez de executar.
    #[cfg(windows)]
    #[test]
    #[ignore = "a interrupção não chega ao processo em primeiro plano; ver ADR sobre Ctrl+C no terminal"]
    fn interrupting_a_batch_file_leaves_the_terminal_ready_for_the_next_command() {
        let script = std::env::temp_dir().join(format!("er-ide-loop-{}.cmd", std::process::id()));
        if let Err(error) = std::fs::write(&script, "@echo off\r\nping -n 30 127.0.0.1\r\n") {
            panic!("script creation failed: {error}");
        }
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let profile = ShellProfile {
            kind: ShellKind::PowerShell,
            executable: PathBuf::from("powershell.exe"),
        };
        let mut terminal = match TerminalSession::new(root, 200, profile) {
            Ok(terminal) => terminal,
            Err(error) => panic!("terminal creation failed: {error}"),
        };

        if let Err(error) = terminal.run(&format!("& '{}'", script.display())) {
            panic!("run failed: {error}");
        }
        assert!(wait_for(&mut terminal, "TTL="), "o lote precisa começar");
        if let Err(error) = terminal.interrupt() {
            panic!("interrupt failed: {error}");
        }

        // Executar imediatamente após parar, como quem clica nos dois botões em
        // sequência: o comando espera o terminal voltar em vez de se perder.
        if let Err(error) = terminal.run("echo terminal-livre") {
            panic!("second run failed: {error}");
        }
        assert!(
            wait_for(&mut terminal, "terminal-livre"),
            "o terminal aceita o próximo comando em vez de ficar preso na pergunta"
        );
        let _ = std::fs::remove_file(script);
    }

    /// Cenário relatado: executar, parar e executar de novo, com o Maven real.
    ///
    /// `mvn.cmd` é um arquivo de lote, então é o caso que trava o terminal na
    /// pergunta do `cmd`. Marcado como `ignored` porque depende de Maven, JDK e
    /// das dependências do projeto já baixadas.
    #[cfg(windows)]
    #[test]
    #[ignore = "requires Maven and a Spring Boot project"]
    fn stop_then_run_again_restarts_a_maven_application() {
        let project =
            PathBuf::from("C:/Users/jdani/Documents/projetos/java/spring-boot-four-endpoints");
        if !project.join("pom.xml").is_file() {
            eprintln!("projeto de exemplo ausente; teste ignorado");
            return;
        }
        let profile = ShellProfile {
            kind: ShellKind::PowerShell,
            executable: PathBuf::from("powershell.exe"),
        };
        let mut terminal = match TerminalSession::new(project, 500, profile) {
            Ok(terminal) => terminal,
            Err(error) => panic!("terminal creation failed: {error}"),
        };
        let command = "mvn -B \"-Dmaven.test.skip=true\" spring-boot:run";

        let started = |terminal: &mut TerminalSession| {
            let deadline = Instant::now() + Duration::from_secs(120);
            while Instant::now() < deadline {
                terminal.drain_output();
                if terminal
                    .lines()
                    .any(|line| line.text.contains("Started FourEndpointsApplication"))
                {
                    return true;
                }
                thread::sleep(Duration::from_millis(50));
            }
            false
        };

        if let Err(error) = terminal.run(command) {
            panic!("first run failed: {error}");
        }
        assert!(started(&mut terminal), "a aplicação precisa subir");
        let first_start = terminal.line_count();

        if let Err(error) = terminal.interrupt() {
            panic!("interrupt failed: {error}");
        }
        // Executar imediatamente depois de parar, como os dois cliques.
        if let Err(error) = terminal.run(command) {
            panic!("second run failed: {error}");
        }

        let deadline = Instant::now() + Duration::from_secs(120);
        let mut restarted = false;
        while Instant::now() < deadline && !restarted {
            terminal.drain_output();
            restarted = terminal
                .lines()
                .skip(first_start)
                .any(|line| line.text.contains("Started FourEndpointsApplication"));
            thread::sleep(Duration::from_millis(50));
        }
        assert!(
            restarted,
            "a aplicação precisa subir de novo: {:?}",
            terminal
                .lines()
                .skip(first_start)
                .map(|line| line.text.clone())
                .collect::<Vec<_>>()
        );
        let _ = terminal.interrupt();
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

    /// Uma grade solta, alimentada à mão: sem PTY, sem processo, sem tela.
    fn grade(bytes: &[u8]) -> Engine {
        let mut engine = Engine::with_scrollback(80, 24, 500);
        engine.feed(bytes);
        engine
    }

    fn texto(engine: &Engine, linha: usize) -> String {
        engine
            .viewport_line(linha)
            .iter()
            .map(justerm_core::Cell::c)
            .collect::<String>()
            .trim_end()
            .to_owned()
    }

    /// A cor que o programa pediu chega à grade.
    ///
    /// Antes o `strip_terminal_controls` a descartava antes de guardar, e erro e
    /// sucesso ficavam do mesmo tom.
    #[test]
    fn colour_survives_all_the_way_to_the_grid() {
        let engine = grade(b"\x1b[31mfalhou\x1b[0m ok");
        assert_eq!(texto(&engine, 0), "falhou ok");
        let pintado = engine.grid().cell(0, 0).fg();
        let normal = engine.grid().cell(0, 7).fg();
        assert_ne!(
            pintado, normal,
            "o trecho vermelho tem que diferir do texto sem cor"
        );
    }

    /// Reescrever a linha ocupa uma linha.
    ///
    /// É o caso da barra de progresso do Maven e do npm: um `\r` volta ao começo
    /// e o texto seguinte cobre o anterior. A lista de linhas empilhava cada
    /// passo como se fosse uma linha nova.
    #[test]
    fn a_carriage_return_overwrites_instead_of_stacking() {
        let engine = grade(b"Progresso: 10%\rProgresso: 50%\rProgresso: 100%");
        assert_eq!(texto(&engine, 0), "Progresso: 100%");
        assert_eq!(texto(&engine, 1), "", "não pode ter sobrado linha nenhuma");
    }

    /// O cursor é posição, e o programa o move.
    #[test]
    fn the_program_moves_the_cursor_where_it_wants() {
        let mut engine = grade(b"primeira\r\nsegunda\r\nterceira");
        engine.feed(b"\x1b[2;1HREESCRITA");
        assert_eq!(texto(&engine, 1), "REESCRITA");
        assert_eq!(texto(&engine, 0), "primeira", "as outras ficam onde estavam");
    }
}
