//! Adapter externo para projetos com `package.json`.
//!
//! O `package.json` diz que existe um projeto; o `tsconfig.json` diz do que ele
//! é feito. Os dois papéis são separados de propósito — um projeto pode ter
//! `package.json` sem TypeScript nenhum, e é resposta certa reconhecê-lo assim.

use std::{
    collections::BTreeMap,
    path::Path,
    sync::Arc,
    time::Duration,
};

use async_trait::async_trait;
use ide_process::{ProcessRequest, ProcessSupervisor, find_in_path};
use ide_project::{
    build::{
        BuildCommandRequest, BuildCommandResult, BuildError, BuildSystemAdapter,
        ProjectImportRequest,
    },
    model::{BuildSystemId, ModuleId, ProjectDescriptor, ProjectModel, ProjectModule, SourceRoots},
};

use super::tsconfig;

pub const NPM_BUILD_SYSTEM_ID: &str = "npm";
const MANIFEST: &str = "package.json";
const TSCONFIG: &str = "tsconfig.json";

pub struct NpmAdapter {
    processes: Arc<dyn ProcessSupervisor>,
    timeout: Duration,
}

impl NpmAdapter {
    #[must_use]
    pub fn new(processes: Arc<dyn ProcessSupervisor>) -> Self {
        Self {
            processes,
            timeout: Duration::from_secs(900),
        }
    }

    #[must_use]
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }
}

#[async_trait]
impl BuildSystemAdapter for NpmAdapter {
    fn build_system_id(&self) -> BuildSystemId {
        BuildSystemId(NPM_BUILD_SYSTEM_ID.to_owned())
    }

    async fn detect_project(&self, root: &Path) -> Result<Option<ProjectDescriptor>, BuildError> {
        let manifest = root.join(MANIFEST);
        if !manifest.is_file() {
            return Ok(None);
        }
        Ok(Some(ProjectDescriptor {
            build_system: self.build_system_id(),
            root: root.to_path_buf(),
            manifest,
            name: package_name(root),
            // npm não tem wrapper versionado no projeto, como o `mvnw` do Maven.
            wrapper: None,
        }))
    }

    async fn import_project(
        &self,
        request: ProjectImportRequest,
    ) -> Result<ProjectModel, BuildError> {
        let root = request.descriptor.root.clone();
        let name = request
            .descriptor
            .name
            .clone()
            .or_else(|| package_name(&root))
            .unwrap_or_else(|| "typescript".to_owned());
        let mut model = ProjectModel::new(self.build_system_id(), root.clone(), name.clone());

        let mut source_roots = SourceRoots::default();
        let mut output = root.join("dist");
        // As raízes saem do `tsconfig.json`, e não de convenção. Ver a ADR-027.
        let config_path = root.join(TSCONFIG);
        if config_path.is_file() {
            match tsconfig::load(&config_path) {
                Ok(config) => {
                    for raiz in config.source_roots() {
                        source_roots.push_main(raiz);
                    }
                    if let Some(out) = &config.out_dir {
                        output = root.join(out);
                    }
                }
                Err(error) => {
                    // Um `tsconfig.json` ilegível não impede abrir o projeto: a
                    // IDE degrada para "não sei quais são as raízes" em vez de
                    // recusar-se a abrir, como manda a `23`.
                    tracing::warn!(%error, "tsconfig.json não pôde ser lido");
                }
            }
        }
        if source_roots.main.is_empty() {
            // Sem `tsconfig.json`, a raiz é o próprio projeto. Chutar `src`
            // aqui seria a convenção que a ADR-027 recusa.
            source_roots.push_main(root.clone());
        }

        model.modules.push(ProjectModule {
            id: ModuleId(".".to_owned()),
            name,
            root: root.clone(),
            manifest: request.descriptor.manifest.clone(),
            coordinates: None,
            packaging: "package".to_owned(),
            source_roots,
            dependencies: Vec::new(),
            output_directory: output.clone(),
            test_output_directory: output,
            children: Vec::new(),
            plugins: Vec::new(),
        });
        model.properties.extend(scripts(&root));
        Ok(model)
    }

    async fn execute(
        &self,
        request: BuildCommandRequest,
    ) -> Result<BuildCommandResult, BuildError> {
        if request.goals.is_empty() {
            return Err(BuildError::Tool("nenhum script foi informado".to_owned()));
        }
        // Sem npm no PATH a tarefa falha **dizendo o que falta**, e não em
        // silêncio: é o critério da fase 2 da `23` para ambiente incompleto.
        let program = find_in_path("npm")
            .ok_or_else(|| BuildError::ToolNotFound("npm não está no PATH".to_owned()))?;
        let mut args = vec!["run".to_owned()];
        args.extend(request.goals.iter().cloned());
        args.extend(request.extra_args.iter().cloned());
        let command_line = format!("{} {}", program.display(), args.join(" "));
        let environment = request.environment.into_iter().collect();
        let output = self
            .processes
            .execute(ProcessRequest {
                program,
                args,
                working_directory: Some(request.descriptor.root),
                timeout: Some(self.timeout),
                environment,
            })
            .await
            .map_err(|error| BuildError::Tool(error.to_string()))?;
        Ok(BuildCommandResult {
            success: output.exit_code == 0,
            exit_code: output.exit_code,
            stdout: output.stdout,
            stderr: output.stderr,
            command_line,
        })
    }
}

/// Os `scripts` do `package.json`, que é de onde vêm as tarefas.
///
/// `ng serve` aparece porque está escrito ali, e não porque alguém aqui saiba o
/// que `ng` é — a mesma regra que proíbe tabela de compatibilidade no nosso
/// código.
#[must_use]
pub fn scripts(root: &Path) -> BTreeMap<String, String> {
    let Ok(source) = std::fs::read_to_string(root.join(MANIFEST)) else {
        return BTreeMap::new();
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&source) else {
        return BTreeMap::new();
    };
    value
        .get("scripts")
        .and_then(serde_json::Value::as_object)
        .map(|scripts| {
            scripts
                .iter()
                .filter_map(|(name, command)| {
                    command
                        .as_str()
                        .map(|command| (name.clone(), command.to_owned()))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn package_name(root: &Path) -> Option<String> {
    let source = std::fs::read_to_string(root.join(MANIFEST)).ok()?;
    let value: serde_json::Value = serde_json::from_str(&source).ok()?;
    value
        .get("name")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
}
