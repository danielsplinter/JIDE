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
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            event_capacity: default_event_capacity(),
            workspace: WorkspaceConfig::default(),
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct WorkspaceConfig {
    pub last_path: Option<PathBuf>,
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

    #[test]
    fn absent_config_uses_bounded_defaults() {
        let config = AppConfig::load(Path::new("missing-ide-config.toml"));
        assert!(matches!(config, Ok(value) if value.event_capacity == 1_024));
    }
}
