//! A instalação de Node que executa as ferramentas do projeto.
//!
//! **Uma escolha só, usada pelo analisador e pela CLI.** Separar as duas seria
//! mais preciso e pior de usar: a análise tolera qualquer versão recente, e quem
//! é rígido é a CLI — que recusa sozinha, com mensagem própria, no momento de
//! executar. Ver a fase 0 da `23`.
//!
//! **E nenhuma tabela de compatibilidade aqui dentro.** Não se pergunta qual
//! Angular o projeto usa para deduzir qual Node ele exige; roda-se a ferramenta,
//! e a mensagem dela é o que a IDE mostra.

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use ide_domain::LanguageId;
use ide_process::find_in_path;
use ide_toolchain_api::{
    DetectionContext, ToolchainError, ToolchainId, ToolchainInstallation, ToolchainProvider,
    ToolchainValidation,
};

use crate::analyzer::TYPESCRIPT_LANGUAGE_ID;

pub const NODE_TOOLCHAIN_ID: &str = "node";

pub struct NodeToolchainProvider;

impl NodeToolchainProvider {
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl Default for NodeToolchainProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ToolchainProvider for NodeToolchainProvider {
    fn toolchain_id(&self) -> ToolchainId {
        ToolchainId(NODE_TOOLCHAIN_ID.to_owned())
    }

    fn supported_languages(&self) -> Vec<LanguageId> {
        vec![LanguageId(TYPESCRIPT_LANGUAGE_ID.to_owned())]
    }

    async fn detect(
        &self,
        _context: DetectionContext,
    ) -> Result<Vec<ToolchainInstallation>, ToolchainError> {
        // O que está no `PATH` é o que o terminal usaria, e é a resposta certa
        // para quem não escolheu nada. Quem usa gerenciador de versão troca o
        // `PATH` do shell, e a IDE aberta não vê a troca — por isso a escolha
        // por projeto da fase 0 existe.
        let Some(executable) = find_in_path(NODE_EXECUTABLE) else {
            return Ok(Vec::new());
        };
        let Some(home) = home_of(&executable) else {
            return Ok(Vec::new());
        };
        let version = version_of(&home);
        Ok(vec![ToolchainInstallation {
            id: self.toolchain_id(),
            home,
            version,
        }])
    }

    async fn validate(
        &self,
        installation: &ToolchainInstallation,
    ) -> Result<ToolchainValidation, ToolchainError> {
        let executable = node_executable(&installation.home);
        if executable.is_file() {
            return Ok(ToolchainValidation {
                valid: true,
                details: Vec::new(),
            });
        }
        Ok(ToolchainValidation {
            valid: false,
            details: vec![format!(
                "{} não contém o executável de Node",
                installation.home.display()
            )],
        })
    }

    async fn resolve_installation(
        &self,
        home: PathBuf,
    ) -> Result<ToolchainInstallation, ToolchainError> {
        if !node_executable(&home).is_file() {
            return Err(ToolchainError::Operation(format!(
                "{} não contém o executável de Node",
                home.display()
            )));
        }
        let version = version_of(&home);
        Ok(ToolchainInstallation {
            id: self.toolchain_id(),
            home,
            version,
        })
    }
}

/// A versão que esta instalação relata, perguntando a ela.
///
/// **Existe para a IDE poder mostrá-la.** Ela resolve o Node pelo `PATH` do
/// próprio processo, e quem troca de versão com um gerenciador muda o `PATH` do
/// **shell** — a IDE aberta não vê a troca. Não há como ler o shell de outra
/// pessoa; há como dizer qual foi encontrada, e deixar quem usa perceber que não
/// é a esperada.
///
/// Sem número, a barra de estado mostrava só o caminho, e um caminho que é um
/// link simbólico não diz versão nenhuma — que é exatamente como um gerenciador
/// de versões instala.
///
/// Falhar é silencioso: uma instalação sem versão legível continua utilizável, e
/// recusá-la por não saber se apresentar seria pior.
fn version_of(home: &Path) -> Option<String> {
    let saida = std::process::Command::new(node_executable(home))
        .arg("--version")
        .output()
        .ok()?;
    if !saida.status.success() {
        return None;
    }
    let texto = String::from_utf8_lossy(&saida.stdout).trim().to_owned();
    (!texto.is_empty()).then_some(texto)
}

#[cfg(windows)]
const NODE_EXECUTABLE: &str = "node.exe";
#[cfg(not(windows))]
const NODE_EXECUTABLE: &str = "node";

/// O executável fica na raiz da instalação no Windows e em `bin` no resto.
#[must_use]
pub fn node_executable(home: &Path) -> PathBuf {
    let direto = home.join(NODE_EXECUTABLE);
    if direto.is_file() {
        return direto;
    }
    home.join("bin").join(NODE_EXECUTABLE)
}

/// A raiz da instalação, a partir do executável encontrado no `PATH`.
fn home_of(executable: &Path) -> Option<PathBuf> {
    let parent = executable.parent()?;
    if parent.file_name().is_some_and(|name| name == "bin") {
        return parent.parent().map(Path::to_path_buf);
    }
    Some(parent.to_path_buf())
}
