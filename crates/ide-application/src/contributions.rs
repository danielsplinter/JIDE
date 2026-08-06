//! Catálogo de capacidades fornecidas por linguagens.

use std::{collections::BTreeMap, path::PathBuf, sync::Arc};

use async_trait::async_trait;
use ide_debug_api::DebugAdapter;
use ide_domain::{DocumentSnapshot, LanguageId};
use ide_language_api::LanguageProvider;
use ide_toolchain_api::{
    CompilerAdapter, RuntimeAdapter, TestAdapter, ToolchainError, ToolchainId,
    ToolchainInstallation, ToolchainProvider,
};
use thiserror::Error;

use crate::NewItemTemplateId;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LanguageDescriptor {
    pub language_id: LanguageId,
    pub display_name: String,
    pub extensions: Vec<String>,
    /// Nomes de diretórios que delimitam raízes de fontes desta linguagem.
    ///
    /// A aplicação usa estes dados para construir escopos e a UI para
    /// apresentar pacotes, sem conhecer convenções de uma linguagem concreta.
    pub source_root_names: Vec<String>,
    /// Identificadores dos sistemas de build que esta linguagem contribui.
    ///
    /// É o que permite dizer **em que linguagem um projeto foi reconhecido**
    /// sem a aplicação saber o nome de nenhuma: a detecção devolve o sistema de
    /// build que reconheceu a pasta, e quem o registrou diz de quem ele é. A
    /// alternativa seria uma tabela de manifestos escrita fora das
    /// contribuições, e ela envelheceria a cada linguagem nova.
    ///
    /// Vazio é legítimo: uma linguagem que não traz projeto próprio — marcação,
    /// folhas de estilo — não reconhece pasta nenhuma sozinha.
    pub build_systems: Vec<String>,
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct TaskId(pub String);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TaskDescriptor {
    pub id: TaskId,
    pub title: String,
    pub requires_active_document: bool,
    pub show_in_toolbar: bool,
}

#[derive(Clone, Debug)]
pub struct TaskExecutionContext {
    pub workspace_root: PathBuf,
    pub source_files: Vec<PathBuf>,
    pub active_document: Option<DocumentSnapshot>,
    /// Onde o código já compilado está: a saída do próprio projeto e os
    /// artefatos das dependências, na ordem de resolução.
    ///
    /// Chamava-se `classpath_entries`, que é vocabulário da JVM num contrato que
    /// não pode ter nenhum. Cada linguagem chama isto de um jeito — *classpath*,
    /// referências, `sys.path` —, e o contrato descreve a coisa, não o nome que
    /// uma delas lhe dá. Ver a fase 0 da `23`.
    pub library_paths: Vec<PathBuf>,
    pub installation: ToolchainInstallation,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TaskExecutionResult {
    pub success: bool,
    pub status: String,
    pub stdout: String,
    pub stderr: String,
}

#[async_trait]
pub trait TaskExecutor: Send + Sync {
    fn supported_language(&self) -> LanguageId;
    async fn execute(
        &self,
        task: &TaskDescriptor,
        context: TaskExecutionContext,
    ) -> Result<TaskExecutionResult, TaskExecutionError>;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NewItemTemplate {
    pub id: NewItemTemplateId,
    pub title: String,
    pub name_caption: String,
    pub file_extension: Option<String>,
    pub allows_empty_name: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SettingsSection {
    pub id: String,
    pub title: String,
    pub field_caption: String,
    pub browse_button_title: String,
    /// Rótulo de uma **segunda** escolha na mesma seção, quando existir.
    ///
    /// Em Java é a instalação do Maven, ao lado da do JDK; em outra linguagem
    /// será outra coisa, ou nenhuma. A tela desenha o que a seção declarar, sem
    /// saber o que é: é o que a mantém neutra.
    #[allow(clippy::struct_field_names)]
    pub secondary_caption: Option<String>,
}

/// Um projeto recente, como a aplicação o entrega à tela.
///
/// A linguagem vem pronta e como **nome de exibição**: quem sabe em que
/// linguagem um projeto foi reconhecido é a aplicação, e traduzir identificador
/// em nome é trabalho de quem os registrou. Ausente quando a IDE não reconheceu
/// projeto nenhum ali — e é o que separa "ainda não sei" de um grupo inventado.
///
/// Mora aqui, ao lado do catálogo, pelo mesmo motivo que ele: é dado de
/// apresentação que a aplicação monta e a tela desenha, sem nenhuma das duas
/// precisar saber o nome de linguagem nenhuma.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecentProject {
    pub path: PathBuf,
    pub language: Option<String>,
}

/// Dados de apresentação agregados das contribuições registradas.
///
/// Este é o único modelo de linguagem que atravessa para `ide-ui`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct UiContributionCatalog {
    pub language_names: Vec<String>,
    pub source_root_names: Vec<String>,
    pub new_item_templates: Vec<NewItemTemplate>,
    pub settings_sections: Vec<SettingsSection>,
    pub tasks: Vec<TaskDescriptor>,
}

#[derive(Clone)]
pub struct LanguageContribution {
    pub descriptor: LanguageDescriptor,
    pub provider: Arc<dyn LanguageProvider>,
    pub toolchain: Option<Arc<dyn ToolchainProvider>>,
    pub compiler: Option<Arc<dyn CompilerAdapter>>,
    pub runtime: Option<Arc<dyn RuntimeAdapter>>,
    pub tests: Option<Arc<dyn TestAdapter>>,
    pub debugger: Option<Arc<dyn DebugAdapter>>,
    pub task_executor: Option<Arc<dyn TaskExecutor>>,
    pub new_item_templates: Vec<NewItemTemplate>,
    pub settings_sections: Vec<SettingsSection>,
    pub tasks: Vec<TaskDescriptor>,
}

impl LanguageContribution {
    #[must_use]
    pub fn new(descriptor: LanguageDescriptor, provider: Arc<dyn LanguageProvider>) -> Self {
        Self {
            descriptor,
            provider,
            toolchain: None,
            compiler: None,
            runtime: None,
            tests: None,
            debugger: None,
            task_executor: None,
            new_item_templates: Vec::new(),
            settings_sections: Vec::new(),
            tasks: Vec::new(),
        }
    }
}

#[derive(Default)]
pub struct ContributionRegistry {
    contributions: BTreeMap<LanguageId, LanguageContribution>,
}

impl ContributionRegistry {
    pub fn register(
        &mut self,
        contribution: LanguageContribution,
    ) -> Result<(), ContributionError> {
        validate_contribution(&contribution)?;
        let language_id = contribution.descriptor.language_id.clone();
        if self.contributions.contains_key(&language_id) {
            return Err(ContributionError::DuplicateLanguage(language_id));
        }
        self.contributions.insert(language_id, contribution);
        Ok(())
    }

    #[must_use]
    pub fn get(&self, language_id: &LanguageId) -> Option<&LanguageContribution> {
        self.contributions.get(language_id)
    }

    pub fn iter(&self) -> impl Iterator<Item = &LanguageContribution> {
        self.contributions.values()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.contributions.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.contributions.is_empty()
    }

    /// Troca as tarefas que a contribuição de uma linguagem publica na tela.
    ///
    /// O catálogo é remontado a partir das contribuições, então a lista da tela
    /// acompanha sem que ninguém acima saiba de onde as tarefas vieram.
    pub fn replace_language_tasks(&mut self, language_id: &LanguageId, tasks: Vec<TaskDescriptor>) {
        if let Some(contribution) = self.contributions.get_mut(language_id) {
            contribution.tasks = tasks;
        }
    }

    /// Qual linguagem declarou esta seção de configurações.
    ///
    /// A janela devolve o identificador da seção que recebeu o clique, e é aqui
    /// que ele volta a ser uma linguagem. A tela não conhece linguagem nenhuma;
    /// ela ecoa o que lhe foi entregue no catálogo.
    #[must_use]
    pub fn language_for_section(&self, section_id: &str) -> Option<LanguageId> {
        self.contributions.values().find_map(|contribution| {
            contribution
                .settings_sections
                .iter()
                .any(|section| section.id == section_id)
                .then(|| contribution.descriptor.language_id.clone())
        })
    }

    /// Qual linguagem contribuiu este sistema de build.
    ///
    /// É por aqui que a aplicação diz em que linguagem um projeto foi
    /// reconhecido sem saber o nome de nenhuma: a detecção devolve o sistema de
    /// build, e a resposta vem de quem o registrou.
    #[must_use]
    pub fn language_for_build_system(&self, build_system: &str) -> Option<&LanguageDescriptor> {
        self.contributions
            .values()
            .map(|contribution| &contribution.descriptor)
            .find(|descriptor| {
                descriptor
                    .build_systems
                    .iter()
                    .any(|declarado| declarado == build_system)
            })
    }

    #[must_use]
    pub fn ui_catalog(&self) -> UiContributionCatalog {
        let mut catalog = UiContributionCatalog::default();
        for contribution in self.contributions.values() {
            catalog
                .language_names
                .push(contribution.descriptor.display_name.clone());
            catalog
                .source_root_names
                .extend(contribution.descriptor.source_root_names.iter().cloned());
            catalog
                .new_item_templates
                .extend(contribution.new_item_templates.iter().cloned());
            catalog
                .settings_sections
                .extend(contribution.settings_sections.iter().cloned());
            catalog.tasks.extend(contribution.tasks.iter().cloned());
        }
        catalog.source_root_names.sort();
        catalog.source_root_names.dedup();
        catalog.language_names.sort();
        catalog.language_names.dedup();
        catalog
    }
}

fn validate_contribution(contribution: &LanguageContribution) -> Result<(), ContributionError> {
    let language_id = &contribution.descriptor.language_id;
    let metadata = contribution.provider.metadata();
    if &metadata.language_id != language_id {
        return Err(ContributionError::ProviderLanguageMismatch {
            expected: language_id.clone(),
            actual: metadata.language_id,
        });
    }
    for adapter_language in [
        contribution
            .compiler
            .as_ref()
            .map(|adapter| adapter.supported_language()),
        contribution
            .runtime
            .as_ref()
            .map(|adapter| adapter.supported_language()),
        contribution
            .tests
            .as_ref()
            .map(|adapter| adapter.supported_language()),
        contribution
            .debugger
            .as_ref()
            .map(|adapter| adapter.supported_language()),
        contribution
            .task_executor
            .as_ref()
            .map(|executor| executor.supported_language()),
    ]
    .into_iter()
    .flatten()
    {
        if &adapter_language != language_id {
            return Err(ContributionError::AdapterLanguageMismatch {
                expected: language_id.clone(),
                actual: adapter_language,
            });
        }
    }
    if let Some(toolchain) = &contribution.toolchain
        && !toolchain.supported_languages().contains(language_id)
    {
        return Err(ContributionError::UnsupportedToolchainLanguage(
            language_id.clone(),
        ));
    }
    Ok(())
}

#[derive(Clone, Debug, Default)]
pub struct ToolchainSelection {
    installations: Vec<ToolchainInstallation>,
    selected: Option<ToolchainId>,
}

impl ToolchainSelection {
    #[must_use]
    pub fn new(installations: Vec<ToolchainInstallation>) -> Self {
        let selected = installations
            .first()
            .map(|installation| installation.id.clone());
        Self {
            installations,
            selected,
        }
    }

    pub fn select(&mut self, id: &ToolchainId) -> Result<(), ToolchainError> {
        if !self
            .installations
            .iter()
            .any(|installation| &installation.id == id)
        {
            return Err(ToolchainError::NotFound);
        }
        self.selected = Some(id.clone());
        Ok(())
    }

    pub fn add(&mut self, installation: ToolchainInstallation) -> usize {
        if let Some(index) = self
            .installations
            .iter()
            .position(|existing| existing.id == installation.id)
        {
            self.installations[index] = installation;
            return index;
        }
        self.installations.push(installation);
        self.installations.len().saturating_sub(1)
    }

    #[must_use]
    pub fn selected(&self) -> Option<&ToolchainInstallation> {
        let selected = self.selected.as_ref()?;
        self.installations
            .iter()
            .find(|installation| &installation.id == selected)
    }

    #[must_use]
    pub fn installations(&self) -> &[ToolchainInstallation] {
        &self.installations
    }
}

#[derive(Default)]
pub struct ToolchainRegistry {
    selections: BTreeMap<LanguageId, ToolchainSelection>,
    providers: BTreeMap<LanguageId, Arc<dyn ToolchainProvider>>,
}

impl ToolchainRegistry {
    pub fn register_contribution(&mut self, contribution: &LanguageContribution) {
        if let Some(provider) = &contribution.toolchain {
            let language_id = contribution.descriptor.language_id.clone();
            self.providers.insert(language_id.clone(), provider.clone());
            self.selections.entry(language_id).or_default();
        }
    }

    pub async fn detect(
        &mut self,
        language_id: &LanguageId,
        context: ide_toolchain_api::DetectionContext,
    ) -> Result<&ToolchainSelection, ToolchainError> {
        let provider = self
            .providers
            .get(language_id)
            .cloned()
            .ok_or(ToolchainError::NotFound)?;
        let installations = provider.detect(context).await?;
        self.set_installations(language_id.clone(), installations);
        self.selection(language_id).ok_or(ToolchainError::NotFound)
    }

    pub async fn add_from_home(
        &mut self,
        language_id: &LanguageId,
        home: PathBuf,
    ) -> Result<usize, ToolchainError> {
        let provider = self
            .providers
            .get(language_id)
            .cloned()
            .ok_or(ToolchainError::NotFound)?;
        let installation = provider.resolve_installation(home).await?;
        Ok(self.ensure_selection(language_id.clone()).add(installation))
    }

    pub fn set_installations(
        &mut self,
        language_id: LanguageId,
        installations: Vec<ToolchainInstallation>,
    ) {
        self.selections
            .insert(language_id, ToolchainSelection::new(installations));
    }

    #[must_use]
    pub fn selection(&self, language_id: &LanguageId) -> Option<&ToolchainSelection> {
        self.selections.get(language_id)
    }

    pub fn selection_mut(&mut self, language_id: &LanguageId) -> Option<&mut ToolchainSelection> {
        self.selections.get_mut(language_id)
    }

    pub fn ensure_selection(&mut self, language_id: LanguageId) -> &mut ToolchainSelection {
        self.selections.entry(language_id).or_default()
    }
}

#[derive(Clone, Default)]
pub struct TaskRegistry {
    tasks: BTreeMap<TaskId, (LanguageId, TaskDescriptor)>,
}

#[derive(Clone, Default)]
pub struct TaskController {
    tasks: TaskRegistry,
    executors: BTreeMap<LanguageId, Arc<dyn TaskExecutor>>,
}

impl TaskController {
    pub fn register_contribution(
        &mut self,
        contribution: &LanguageContribution,
    ) -> Result<(), ContributionError> {
        if !contribution.tasks.is_empty() && contribution.task_executor.is_none() {
            return Err(ContributionError::MissingTaskExecutor(
                contribution.descriptor.language_id.clone(),
            ));
        }
        self.tasks.register_contribution(contribution)?;
        if let Some(executor) = &contribution.task_executor {
            self.executors.insert(
                contribution.descriptor.language_id.clone(),
                executor.clone(),
            );
        }
        Ok(())
    }

    /// Repassa à lista de tarefas o que o projeto aberto declarou.
    pub fn replace_language_tasks(&mut self, language_id: &LanguageId, tasks: Vec<TaskDescriptor>) {
        self.tasks.replace_language_tasks(language_id, tasks);
    }

    #[must_use]
    pub fn task(&self, id: &TaskId) -> Option<(LanguageId, TaskDescriptor)> {
        self.tasks
            .get(id)
            .map(|(language_id, task)| (language_id.clone(), task.clone()))
    }

    pub async fn execute(
        &self,
        id: &TaskId,
        context: TaskExecutionContext,
    ) -> Result<TaskExecutionResult, TaskControllerError> {
        let (language_id, task) = self
            .tasks
            .get(id)
            .ok_or_else(|| TaskControllerError::UnknownTask(id.clone()))?;
        let executor = self
            .executors
            .get(language_id)
            .ok_or_else(|| TaskControllerError::MissingExecutor(language_id.clone()))?;
        executor
            .execute(task, context)
            .await
            .map_err(TaskControllerError::Execution)
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.tasks.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.tasks.is_empty()
    }
}

impl TaskRegistry {
    pub fn register_contribution(
        &mut self,
        contribution: &LanguageContribution,
    ) -> Result<(), ContributionError> {
        for task in &contribution.tasks {
            if self.tasks.contains_key(&task.id) {
                return Err(ContributionError::DuplicateTask(task.id.clone()));
            }
        }
        for task in &contribution.tasks {
            self.tasks.insert(
                task.id.clone(),
                (contribution.descriptor.language_id.clone(), task.clone()),
            );
        }
        Ok(())
    }

    /// Troca as tarefas de uma linguagem pelas que o projeto aberto declarou.
    ///
    /// Nem toda tarefa é conhecida na partida. As de Java são — compilar,
    /// executar, testar existem antes de haver projeto. As de npm não: são os
    /// `scripts` do `package.json`, e mudam de projeto para projeto. Declarar
    /// um conjunto fixo aqui seria adivinhar nomes, que é a tabela de
    /// compatibilidade que a `23` proíbe com outro nome.
    pub fn replace_language_tasks(&mut self, language_id: &LanguageId, tasks: Vec<TaskDescriptor>) {
        self.tasks.retain(|_, (owner, _)| owner != language_id);
        for task in tasks {
            self.tasks
                .insert(task.id.clone(), (language_id.clone(), task));
        }
    }

    #[must_use]
    pub fn get(&self, id: &TaskId) -> Option<(&LanguageId, &TaskDescriptor)> {
        self.tasks
            .get(id)
            .map(|(language_id, descriptor)| (language_id, descriptor))
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.tasks.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.tasks.is_empty()
    }
}

#[derive(Debug, Error, Eq, PartialEq)]
pub enum ContributionError {
    #[error("language contribution is already registered: {0:?}")]
    DuplicateLanguage(LanguageId),
    #[error("task is already registered: {0:?}")]
    DuplicateTask(TaskId),
    #[error("provider language mismatch: expected {expected:?}, got {actual:?}")]
    ProviderLanguageMismatch {
        expected: LanguageId,
        actual: LanguageId,
    },
    #[error("adapter language mismatch: expected {expected:?}, got {actual:?}")]
    AdapterLanguageMismatch {
        expected: LanguageId,
        actual: LanguageId,
    },
    #[error("toolchain does not support language {0:?}")]
    UnsupportedToolchainLanguage(LanguageId),
    #[error("language contribution has tasks but no executor: {0:?}")]
    MissingTaskExecutor(LanguageId),
}

#[derive(Debug, Error, Eq, PartialEq)]
pub enum TaskExecutionError {
    #[error("task execution failed: {0}")]
    Failed(String),
}

#[derive(Debug, Error, Eq, PartialEq)]
pub enum TaskControllerError {
    #[error("task is not registered: {0:?}")]
    UnknownTask(TaskId),
    #[error("task executor is not registered for language: {0:?}")]
    MissingExecutor(LanguageId),
    #[error(transparent)]
    Execution(#[from] TaskExecutionError),
}

#[cfg(test)]
mod tests {
    use async_trait::async_trait;
    use ide_domain::{Diagnostic, DocumentChange, DocumentId, DocumentSnapshot, ProviderId};
    use ide_language_api::{
        ActiveLanguage, LANGUAGE_API_VERSION, LanguageActivationContext, LanguageCapabilities,
        LanguageError, LanguageMetadata,
    };

    use super::*;

    struct FakeProvider {
        language_id: LanguageId,
    }

    #[async_trait]
    impl LanguageProvider for FakeProvider {
        fn metadata(&self) -> LanguageMetadata {
            LanguageMetadata {
                language_id: self.language_id.clone(),
                provider_id: ProviderId(format!("{}.test", self.language_id.0)),
                display_name: self.language_id.0.clone(),
                extensions: vec![self.language_id.0.clone()],
                api_version: LANGUAGE_API_VERSION,
                trigger_characters: Vec::new(),
            }
        }

        fn capabilities(&self) -> LanguageCapabilities {
            LanguageCapabilities::empty()
        }

        async fn activate(
            &self,
            _context: LanguageActivationContext,
        ) -> Result<Box<dyn ActiveLanguage>, LanguageError> {
            Ok(Box::new(FakeActive {
                language_id: self.language_id.clone(),
            }))
        }
    }

    struct FakeActive {
        language_id: LanguageId,
    }

    #[async_trait]
    impl ActiveLanguage for FakeActive {
        fn language_id(&self) -> &LanguageId {
            &self.language_id
        }

        async fn open_document(&self, _document: DocumentSnapshot) -> Result<(), LanguageError> {
            Ok(())
        }

        async fn change_document(&self, _change: DocumentChange) -> Result<(), LanguageError> {
            Ok(())
        }

        async fn close_document(&self, _document_id: DocumentId) -> Result<(), LanguageError> {
            Ok(())
        }

        async fn diagnostics(
            &self,
            _document_id: DocumentId,
        ) -> Result<Vec<Diagnostic>, LanguageError> {
            Ok(Vec::new())
        }

        async fn shutdown(&self) -> Result<(), LanguageError> {
            Ok(())
        }
    }

    fn contribution(language: &str) -> LanguageContribution {
        let language_id = LanguageId(language.to_owned());
        let mut contribution = LanguageContribution::new(
            LanguageDescriptor {
                language_id: language_id.clone(),
                display_name: language.to_owned(),
                extensions: vec![language.to_owned()],
                source_root_names: vec![language.to_owned()],
                build_systems: vec![format!("{language}.build")],
            },
            Arc::new(FakeProvider { language_id }),
        );
        contribution.tasks.push(TaskDescriptor {
            id: TaskId(format!("{language}.run")),
            title: format!("Run {language}"),
            requires_active_document: true,
            show_in_toolbar: true,
        });
        contribution
    }

    #[test]
    fn registry_is_indexed_by_language_and_rejects_duplicates() {
        let mut registry = ContributionRegistry::default();
        assert!(registry.register(contribution("fake")).is_ok());
        assert_eq!(registry.len(), 1);
        assert_eq!(
            registry
                .get(&LanguageId("fake".to_owned()))
                .map(|entry| entry.descriptor.display_name.as_str()),
            Some("fake")
        );
        assert_eq!(
            registry.register(contribution("fake")),
            Err(ContributionError::DuplicateLanguage(LanguageId(
                "fake".to_owned()
            )))
        );
    }

    #[test]
    fn tasks_are_registered_from_contribution_data() {
        let contribution = contribution("fake");
        let mut tasks = TaskRegistry::default();
        assert!(tasks.register_contribution(&contribution).is_ok());
        assert_eq!(tasks.len(), 1);
        assert_eq!(
            tasks
                .get(&TaskId("fake.run".to_owned()))
                .map(|(language, task)| (language.0.clone(), task.title.clone())),
            Some(("fake".to_owned(), "Run fake".to_owned()))
        );
    }

    #[test]
    fn ui_catalog_is_derived_only_from_registered_contributions() {
        let mut fake = contribution("fake");
        fake.new_item_templates.push(NewItemTemplate {
            id: NewItemTemplateId::new("fake.module"),
            title: "New module".to_owned(),
            name_caption: "Name".to_owned(),
            file_extension: Some("fake".to_owned()),
            allows_empty_name: false,
        });
        fake.settings_sections.push(SettingsSection {
            id: "fake.runtime".to_owned(),
            title: "Runtime".to_owned(),
            field_caption: "SDK".to_owned(),
            browse_button_title: "Browse".to_owned(),
            secondary_caption: None,
        });
        let mut registry = ContributionRegistry::default();
        assert!(registry.register(fake).is_ok());
        let catalog = registry.ui_catalog();
        assert_eq!(catalog.language_names, vec!["fake"]);
        assert_eq!(catalog.source_root_names, vec!["fake"]);
        assert_eq!(catalog.new_item_templates[0].id.as_str(), "fake.module");
        assert_eq!(catalog.settings_sections[0].field_caption, "SDK");
        assert_eq!(catalog.tasks[0].id, TaskId("fake.run".to_owned()));
    }
}
