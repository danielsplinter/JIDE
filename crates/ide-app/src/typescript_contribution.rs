//! Composição embutida da linguagem TypeScript.
//!
//! O provider nativo mais o sistema de build de npm. Sem toolchain de Node e sem
//! depuração ainda — eles chegam nas fases seguintes da `23`, e a contribuição
//! cresce por aqui sem que nada acima precise saber.
//!
//! Note o que **não** é declarado: nenhuma seção de configurações, porque não há
//! ferramenta a escolher ainda, e nenhuma tarefa. A tela desenha o que existir.

use std::{path::Path, sync::Arc};

use async_trait::async_trait;
use ide_application::{
    LanguageContribution, LanguageDescriptor, SettingsSection, TaskDescriptor,
    TaskExecutionContext, TaskExecutionError, TaskExecutionResult, TaskExecutor, TaskId,
};
use ide_domain::LanguageId;
use ide_language_api::LanguageProvider;
use ide_process::{ProcessRequest, ProcessSupervisor, find_in_path};
use ide_project::build::BuildSystemRegistry;
use ide_toolchain_api::ToolchainProvider;
use language_typescript::{
    NodeToolchainProvider, NpmAdapter, TYPESCRIPT_LANGUAGE_ID, TypeScriptLanguageProvider,
};

/// Prefixo dos identificadores de tarefa desta linguagem.
///
/// Os nomes vêm do `package.json` do projeto, e dois projetos podem ter um
/// `build` cada. O prefixo é o que impede que a tarefa de uma linguagem colida
/// com a de outra no registro, que é global.
const TASK_PREFIX: &str = "npm.";

#[must_use]
pub fn language_id() -> LanguageId {
    LanguageId(TYPESCRIPT_LANGUAGE_ID.to_owned())
}

#[must_use]
pub fn contribution(processes: Arc<dyn ProcessSupervisor>) -> LanguageContribution {
    let provider: Arc<dyn LanguageProvider> = Arc::new(TypeScriptLanguageProvider::new());
    let toolchain: Arc<dyn ToolchainProvider> = Arc::new(NodeToolchainProvider::new());
    let mut contribution = LanguageContribution::new(
        LanguageDescriptor {
            language_id: language_id(),
            display_name: "TypeScript".to_owned(),
            extensions: vec!["ts".to_owned()],
            // Vazio de propósito, e não por falta: a raiz de um projeto
            // TypeScript é declarada no `tsconfig.json`, e é de lá que o
            // `ProjectModel` a lê (ADR-027). Um nome de convenção aqui seria uma
            // segunda origem para a mesma pergunta.
            source_root_names: Vec::new(),
        },
        provider,
    );
    contribution.toolchain = Some(toolchain);
    contribution.task_executor = Some(Arc::new(NpmTaskExecutor { processes }));
    contribution.settings_sections = vec![SettingsSection {
        id: "typescript.node".to_owned(),
        title: "Node".to_owned(),
        field_caption: "Node".to_owned(),
        browse_button_title: "Procurar...".to_owned(),
        // Uma escolha só: a mesma instalação atende o analisador e a CLI. Ver o
        // módulo `toolchain` da crate.
        secondary_caption: None,
    }];
    // As tarefas **não** são declaradas aqui: elas são os `scripts` do
    // `package.json`, e só existem depois que um projeto abre. Ver
    // `project_tasks`.
    contribution
}

/// As tarefas que o projeto aberto declara, lidas do `package.json`.
///
/// `ng serve` aparece porque está escrito ali, e não porque alguém aqui saiba o
/// que `ng` é. Um projeto sem `scripts` não tem tarefa nenhuma, e é resposta
/// certa — melhor do que oferecer um `build` que não existe.
#[must_use]
pub fn project_tasks(root: &Path) -> Vec<TaskDescriptor> {
    language_typescript::scripts(root)
        .into_keys()
        .map(|nome| TaskDescriptor {
            id: TaskId(format!("{TASK_PREFIX}{nome}")),
            title: nome,
            requires_active_document: false,
            show_in_toolbar: false,
        })
        .collect()
}

struct NpmTaskExecutor {
    processes: Arc<dyn ProcessSupervisor>,
}

#[async_trait]
impl TaskExecutor for NpmTaskExecutor {
    fn supported_language(&self) -> LanguageId {
        language_id()
    }

    async fn execute(
        &self,
        task: &TaskDescriptor,
        context: TaskExecutionContext,
    ) -> Result<TaskExecutionResult, TaskExecutionError> {
        let script = task
            .id
            .0
            .strip_prefix(TASK_PREFIX)
            .unwrap_or(&task.id.0)
            .to_owned();
        // Sem npm no PATH a tarefa **diz o que falta**. O projeto continua
        // aberto e editável: é o critério da fase 2 da `23` para ambiente
        // incompleto.
        let program = find_in_path(NPM).ok_or_else(|| {
            TaskExecutionError::Failed(
                "npm não está no PATH — instale o Node ou aponte a instalação em Configurações"
                    .to_owned(),
            )
        })?;
        let output = self
            .processes
            .execute(ProcessRequest {
                program,
                args: vec!["run".to_owned(), script.clone()],
                working_directory: Some(context.workspace_root.clone()),
                timeout: None,
                environment: Vec::new(),
            })
            .await
            .map_err(|error| TaskExecutionError::Failed(error.to_string()))?;
        let sucesso = output.exit_code == 0;
        Ok(TaskExecutionResult {
            success: sucesso,
            status: if sucesso {
                format!("{script} concluído")
            } else {
                format!("{script} falhou")
            },
            stdout: output.stdout,
            stderr: output.stderr,
        })
    }
}

#[cfg(windows)]
const NPM: &str = "npm.cmd";
#[cfg(not(windows))]
const NPM: &str = "npm";

/// Registra o sistema de build de npm na ordem de detecção.
///
/// O `package.json` diz que existe um projeto; o `tsconfig.json` diz do que ele
/// é feito. Um projeto pode ter o primeiro sem TypeScript nenhum, e reconhecê-lo
/// assim é resposta certa.
pub fn register_build_systems(
    registry: &mut BuildSystemRegistry,
    processes: Arc<dyn ProcessSupervisor>,
) {
    registry.register(Arc::new(NpmAdapter::new(processes)));
}
