#![doc = "Contratos para toolchains externas."]

use std::path::PathBuf;

use async_trait::async_trait;
use ide_domain::LanguageId;
use thiserror::Error;

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct ToolchainId(pub String);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToolchainInstallation {
    pub id: ToolchainId,
    pub home: PathBuf,
    pub version: Option<String>,
}

#[derive(Clone, Debug)]
pub struct DetectionContext {
    pub workspace_root: Option<PathBuf>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToolchainValidation {
    pub valid: bool,
    pub details: Vec<String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Classpath {
    pub entries: Vec<PathBuf>,
}

impl Classpath {
    pub fn argument(&self) -> Result<String, ToolchainError> {
        std::env::join_paths(&self.entries)
            .map(|value| value.to_string_lossy().into_owned())
            .map_err(|error| ToolchainError::Operation(error.to_string()))
    }
}

#[derive(Clone, Debug)]
pub struct CompilationRequest {
    pub installation: ToolchainInstallation,
    pub source_files: Vec<PathBuf>,
    pub output_directory: PathBuf,
    pub classpath: Classpath,
    pub source_level: Option<String>,
    pub target_level: Option<String>,
    pub additional_args: Vec<String>,
    pub working_directory: PathBuf,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompilationResult {
    pub success: bool,
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
    pub output_directory: PathBuf,
}

#[derive(Clone, Debug)]
pub struct ExecutionRequest {
    pub installation: ToolchainInstallation,
    pub main_class: String,
    pub classpath: Classpath,
    pub args: Vec<String>,
    pub jvm_args: Vec<String>,
    pub working_directory: PathBuf,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExecutionResult {
    pub success: bool,
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
}

#[derive(Clone, Debug)]
pub struct TestRequest {
    pub compilation: CompilationRequest,
    pub test_classes: Vec<String>,
    pub args: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TestCaseResult {
    pub class_name: String,
    pub execution: ExecutionResult,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TestResult {
    pub compilation: CompilationResult,
    pub cases: Vec<TestCaseResult>,
}

#[async_trait]
pub trait CompilerAdapter: Send + Sync {
    fn supported_language(&self) -> LanguageId;
    async fn compile(
        &self,
        request: CompilationRequest,
    ) -> Result<CompilationResult, ToolchainError>;
}

#[async_trait]
pub trait RuntimeAdapter: Send + Sync {
    fn supported_language(&self) -> LanguageId;
    async fn run(&self, request: ExecutionRequest) -> Result<ExecutionResult, ToolchainError>;
}

#[async_trait]
pub trait TestAdapter: Send + Sync {
    fn supported_language(&self) -> LanguageId;
    async fn run_tests(&self, request: TestRequest) -> Result<TestResult, ToolchainError>;
}

#[async_trait]
pub trait ToolchainProvider: Send + Sync {
    fn toolchain_id(&self) -> ToolchainId;
    fn supported_languages(&self) -> Vec<LanguageId>;
    async fn detect(
        &self,
        context: DetectionContext,
    ) -> Result<Vec<ToolchainInstallation>, ToolchainError>;
    async fn validate(
        &self,
        installation: &ToolchainInstallation,
    ) -> Result<ToolchainValidation, ToolchainError>;
}

#[derive(Debug, Error)]
pub enum ToolchainError {
    #[error("toolchain executable was not found")]
    NotFound,
    #[error("toolchain operation failed: {0}")]
    Operation(String),
}
