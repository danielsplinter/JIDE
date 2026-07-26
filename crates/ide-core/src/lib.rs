#![doc = "Configuração, logging e ciclo de vida do núcleo da IDE."]

use std::{
    fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing_subscriber::EnvFilter;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AppConfig {
    #[serde(default = "default_event_capacity")]
    pub event_capacity: usize,
    #[serde(default)]
    pub workspace: WorkspaceConfig,
    #[serde(default)]
    pub run: RunConfig,
    #[serde(default)]
    pub debug: DebugConfig,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            event_capacity: default_event_capacity(),
            workspace: WorkspaceConfig::default(),
            run: RunConfig::default(),
            debug: DebugConfig::default(),
        }
    }
}

/// Como a aplicação do projeto é executada.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct RunConfig {
    /// Comando que sobe a aplicação.
    ///
    /// Vazio significa "deduzir do projeto"; preenchido, tem prioridade sobre a
    /// dedução, porque só o usuário sabe como sua aplicação sobe. O marcador
    /// `{agent}` recebe o agente de depuração quando a execução é com
    /// depuração, e desaparece quando é sem.
    #[serde(default)]
    pub command: Option<String>,
}

/// Alvo de depuração usado pelo botão de depurar e pela janela de configuração.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DebugConfig {
    pub host: String,
    pub port: u16,
}

impl Default for DebugConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_owned(),
            port: 8000,
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct WorkspaceConfig {
    /// Último projeto aberto, reaberto na próxima inicialização.
    pub last_path: Option<PathBuf>,
}

impl WorkspaceConfig {
    /// Último projeto, apenas quando ainda existe como diretório.
    ///
    /// Uma pasta renomeada, removida ou em um disco desconectado não pode
    /// impedir a IDE de abrir; nesse caso a decisão volta para quem chamou.
    #[must_use]
    pub fn resolved_last_path(&self) -> Option<PathBuf> {
        self.last_path
            .as_ref()
            .filter(|path| path.is_dir())
            .cloned()
    }
}

impl AppConfig {
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let source = fs::read_to_string(path)?;
        let config = toml::from_str(&source)?;
        Ok(config)
    }

    /// Grava a configuração, criando o diretório quando necessário.
    pub fn save(&self, path: &Path) -> Result<(), ConfigError> {
        let contents = toml::to_string_pretty(self)?;
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, contents)?;
        Ok(())
    }

    /// Registra o projeto aberto e grava a configuração.
    pub fn remember_workspace(&mut self, root: &Path, path: &Path) -> Result<(), ConfigError> {
        self.workspace.last_path = Some(root.to_path_buf());
        self.save(path)
    }

    /// Projeto a reabrir na inicialização, se ainda existir.
    #[must_use]
    pub fn resolved_project(&self) -> Option<PathBuf> {
        self.workspace.resolved_last_path()
    }

    /// Registra o alvo de depuração usado e grava a configuração.
    pub fn remember_debug_target(
        &mut self,
        host: &str,
        port: u16,
        path: &Path,
    ) -> Result<(), ConfigError> {
        self.debug.host = host.to_owned();
        self.debug.port = port;
        self.save(path)
    }
}

/// Arquivo de configuração do usuário.
///
/// `ER_IDE_CONFIG` tem prioridade e aponta para o arquivo diretamente; sem ela,
/// vale o diretório de configuração da plataforma. A IDE nunca grava
/// configuração dentro do projeto do usuário.
#[must_use]
pub fn config_path() -> Option<PathBuf> {
    let base = if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from)
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
    };
    resolve_config_path(std::env::var_os("ER_IDE_CONFIG").map(PathBuf::from), base)
}

fn resolve_config_path(explicit: Option<PathBuf>, base: Option<PathBuf>) -> Option<PathBuf> {
    explicit.or_else(|| base.map(|base| base.join("er-ide").join("config.toml")))
}

fn default_event_capacity() -> usize {
    1_024
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("cannot read configuration: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid configuration: {0}")]
    Parse(#[from] toml::de::Error),
    #[error("cannot write configuration: {0}")]
    Serialize(#[from] toml::ser::Error),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ServiceHealth {
    Stopped,
    Healthy,
    Suspended,
    Failed,
}

pub trait ManagedService: Send + Sync {
    fn start(&self) -> Result<(), ServiceError>;
    fn suspend(&self) -> Result<(), ServiceError>;
    fn resume(&self) -> Result<(), ServiceError>;
    fn stop(&self) -> Result<(), ServiceError>;
    fn health(&self) -> ServiceHealth;
}

#[derive(Debug, Error)]
#[error("{message}")]
pub struct ServiceError {
    pub message: String,
}

pub fn init_logging(default_filter: &str) -> Result<(), LoggingError> {
    let filter = EnvFilter::try_from_default_env()
        .or_else(|_| EnvFilter::try_new(default_filter))
        .map_err(|error| LoggingError(error.to_string()))?;
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(true)
        .try_init()
        .map_err(|error| LoggingError(error.to_string()))
}

#[derive(Debug, Error)]
#[error("cannot initialize logging: {0}")]
pub struct LoggingError(String);

#[cfg(test)]
mod tests {
    use super::*;

    fn temporary(name: &str) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("er-ide-config-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        root
    }

    #[test]
    fn absent_config_uses_bounded_defaults() {
        let config = AppConfig::load(Path::new("missing-ide-config.toml"));
        assert!(matches!(config, Ok(value) if value.event_capacity == 1_024));
    }

    #[test]
    fn the_opened_project_survives_a_restart() {
        let root = temporary("remember");
        let project = root.join("projeto");
        assert!(fs::create_dir_all(&project).is_ok());
        let file = root.join("config").join("config.toml");

        let mut config = AppConfig::default();
        assert!(config.remember_workspace(&project, &file).is_ok());
        assert!(file.is_file(), "o diretório de configuração é criado");

        let reloaded = match AppConfig::load(&file) {
            Ok(config) => config,
            Err(error) => panic!("releitura falhou: {error}"),
        };
        assert_eq!(
            reloaded.workspace.last_path.as_deref(),
            Some(project.as_path())
        );
        assert_eq!(reloaded.resolved_project(), Some(project));
        assert_eq!(
            reloaded.event_capacity, 1_024,
            "os demais valores permanecem íntegros"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn the_debug_target_survives_a_restart() {
        let root = temporary("debug-target");
        assert!(fs::create_dir_all(&root).is_ok());
        let file = root.join("config.toml");
        let mut config = AppConfig::default();
        assert_eq!(config.debug.host, "127.0.0.1");
        assert_eq!(config.debug.port, 8000);

        assert!(
            config
                .remember_debug_target("10.0.0.20", 8787, &file)
                .is_ok()
        );
        let reloaded = match AppConfig::load(&file) {
            Ok(config) => config,
            Err(error) => panic!("releitura falhou: {error}"),
        };
        assert_eq!(reloaded.debug.host, "10.0.0.20");
        assert_eq!(reloaded.debug.port, 8787);
        assert!(
            reloaded.run.command.is_none(),
            "sem comando configurado, a IDE deduz do projeto"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_project_that_no_longer_exists_is_ignored() {
        let root = temporary("missing-project");
        let project = root.join("removido");
        assert!(fs::create_dir_all(&project).is_ok());
        let file = root.join("config.toml");
        let mut config = AppConfig::default();
        assert!(config.remember_workspace(&project, &file).is_ok());
        assert!(fs::remove_dir_all(&project).is_ok());

        let reloaded = match AppConfig::load(&file) {
            Ok(config) => config,
            Err(error) => panic!("releitura falhou: {error}"),
        };
        assert!(
            reloaded.workspace.last_path.is_some(),
            "o registro é mantido"
        );
        assert!(
            reloaded.resolved_project().is_none(),
            "mas uma pasta ausente nunca é aberta"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn config_path_prefers_the_explicit_override() {
        let explicit = PathBuf::from("/tmp/er-ide.toml");
        assert_eq!(
            resolve_config_path(Some(explicit.clone()), Some(PathBuf::from("/home/.config"))),
            Some(explicit)
        );
        assert_eq!(
            resolve_config_path(None, Some(PathBuf::from("/home/.config"))),
            Some(PathBuf::from("/home/.config/er-ide/config.toml"))
        );
        assert_eq!(resolve_config_path(None, None), None);
    }
}
