#![doc = "Configuração, logging e ciclo de vida do núcleo da IDE."]

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

mod memory;

pub use ide_domain::ToolRole;
pub use memory::{MemoryMeter, MemoryReading};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing_subscriber::EnvFilter;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AppConfig {
    #[serde(default = "default_event_capacity")]
    pub event_capacity: usize,
    /// Subir todo provider de linguagem ao abrir um arquivo, e não sob demanda.
    ///
    /// # As duas posturas, e por que a escolha é de quem usa
    ///
    /// **Ligado** (o padrão): tudo o que sabe responder pela linguagem sobe
    /// junto. Custa memória e tempo desde o primeiro arquivo aberto — num
    /// monorepo Angular, 1,9 GB e trinta segundos —, e em troca a primeira
    /// pergunta difícil já encontra todo mundo pronto. É como as outras IDEs
    /// fazem.
    ///
    /// **Desligado**: sobe quem responde primeiro, e o resto entra quando
    /// alguém perguntar algo que ninguém de pé soube. Quem só navega, busca e
    /// edita código com tipos declarados nunca paga pelo analisador externo.
    ///
    /// A chave é neutra: ela não menciona linguagem nem analisador. Vale para
    /// qualquer provider que venha depois.
    #[serde(default = "sim")]
    pub eager_language_providers: bool,
    /// Providers de linguagem que não devem entrar em serviço.
    ///
    /// # Por que é uma lista de identificadores, e não uma opção por linguagem
    ///
    /// A IDE não sabe o que nenhum deles é. `typescript.service` é uma cadeia
    /// vinda do arquivo de configuração, do mesmo jeito que os nomes de
    /// ferramenta em `ToolchainConfig` — nada aqui menciona TypeScript, Java ou
    /// analisador externo, e é o que permite a mesma chave servir a qualquer
    /// linguagem que venha depois.
    ///
    /// # Para que serve
    ///
    /// Desligar um provider é o que permite **medir** o que os outros respondem
    /// sozinhos. Com dois providers da mesma linguagem em serviço, não há como
    /// saber qual respondeu — e concluir sobre o errado é a família de defeito
    /// que esta IDE já encontrou várias vezes. Ver a fase 0 da `25`.
    ///
    /// Fica **antes** dos campos de tabela de propósito: em TOML, um valor
    /// escrito depois de uma tabela pertenceria a ela, e a serialização falha.
    /// O teste de ida e volta é o que pega isso.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub disabled_providers: BTreeSet<String>,
    #[serde(default)]
    pub workspace: WorkspaceConfig,
    #[serde(default)]
    pub run: RunConfig,
    #[serde(default)]
    pub debug: DebugConfig,
    #[serde(default)]
    pub toolchains: ToolchainConfig,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            event_capacity: default_event_capacity(),
            workspace: WorkspaceConfig::default(),
            run: RunConfig::default(),
            debug: DebugConfig::default(),
            toolchains: ToolchainConfig::default(),
            eager_language_providers: sim(),
            disabled_providers: BTreeSet::new(),
        }
    }
}

/// De onde veio o valor em vigor.
///
/// A tela mostra isto. Um campo preenchido pelo padrão, por sobreposição de
/// projeto ou por detecção parece igual, e agir sobre a origem errada é a
/// família de defeito que a `21` nomeou: quem lê não distingue.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ToolOrigin {
    Project,
    Default,
}

/// Ferramenta em vigor, com a origem junto.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedTool {
    pub home: PathBuf,
    pub origin: ToolOrigin,
}

/// Ferramentas escolhidas à mão, que valem por cima da detecção automática.
///
/// A IDE sabe detectar sozinha, mas a máquina de quem desenvolve costuma ter
/// mais de uma de cada, e a escolha precisa sobreviver ao fechamento da janela.
/// Um caminho que deixou de existir é ignorado na leitura: a IDE volta a
/// detectar em vez de recusar-se a abrir.
///
/// **Nada aqui nomeia linguagem ou ferramenta.** As chaves são o `LanguageId`
/// que a contribuição declarou e o papel que a seção de configurações define.
/// O formato antigo tinha um campo por ferramenta, e crescia com o número de
/// linguagens — ver a fase 0 da `23` e a ADR-026.
///
/// A escolha é **por projeto, com um padrão por trás**: dois projetos da mesma
/// linguagem podem exigir instalações diferentes, e isso não é novidade trazida
/// por nenhuma linguagem nova — sempre valeu para versões diferentes da mesma.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct ToolchainConfig {
    /// Padrão global, por linguagem e papel.
    #[serde(default)]
    defaults: BTreeMap<String, BTreeMap<String, PathBuf>>,
    /// Sobreposição por raiz de workspace, por linguagem e papel.
    ///
    /// Mora aqui, e não dentro do projeto: um caminho de instalação é específico
    /// da máquina, e gravá-lo no repositório o tornaria inútil para outra pessoa
    /// além de criar arquivo a ser comitado sem ninguém pedir.
    #[serde(default)]
    projects: BTreeMap<String, BTreeMap<String, BTreeMap<String, PathBuf>>>,
    /// Escolhas do formato antigo, que tinha um campo por ferramenta.
    ///
    /// São recolhidas cruas e **não** interpretadas aqui: saber que uma delas
    /// era o JDK é conhecimento de linguagem, e ele mora na raiz de composição.
    /// Quem migra chama [`ToolchainConfig::take_legacy`] na partida.
    ///
    /// Sem isto, o arquivo de quem já usa a IDE carregaria com as escolhas
    /// silenciosamente vazias — a pior forma de falhar, e a que a fase 0 da `23`
    /// listou como risco.
    #[serde(flatten, default, skip_serializing_if = "BTreeMap::is_empty")]
    legacy: BTreeMap<String, PathBuf>,
}

impl ToolchainConfig {
    /// Ferramenta em vigor, com a origem, ou `None` quando resta detectar.
    ///
    /// A ordem é sobreposição do projeto, depois padrão global. Quem chama trata
    /// a ausência caindo na detecção automática.
    #[must_use]
    pub fn resolved(
        &self,
        workspace_root: Option<&Path>,
        language: &str,
        role: ToolRole,
    ) -> Option<ResolvedTool> {
        let from_project = workspace_root
            .and_then(|root| self.projects.get(&project_key(root)))
            .and_then(|languages| languages.get(language))
            .and_then(|roles| roles.get(role.key()));
        if let Some(home) = from_project.filter(|home| home.is_dir()) {
            return Some(ResolvedTool {
                home: home.clone(),
                origin: ToolOrigin::Project,
            });
        }
        self.defaults
            .get(language)
            .and_then(|roles| roles.get(role.key()))
            .filter(|home| home.is_dir())
            .map(|home| ResolvedTool {
                home: home.clone(),
                origin: ToolOrigin::Default,
            })
    }

    /// Retira as escolhas do formato antigo, para quem souber traduzi-las.
    ///
    /// Devolve vazio depois da primeira chamada, e o que sobrar não é regravado.
    pub fn take_legacy(&mut self) -> BTreeMap<String, PathBuf> {
        std::mem::take(&mut self.legacy)
    }

    /// Grava a escolha. `workspace_root` ausente define o padrão global.
    pub fn choose(
        &mut self,
        workspace_root: Option<&Path>,
        language: &str,
        role: ToolRole,
        home: Option<&Path>,
    ) {
        let roles = match workspace_root {
            Some(root) => self
                .projects
                .entry(project_key(root))
                .or_default()
                .entry(language.to_owned())
                .or_default(),
            None => self.defaults.entry(language.to_owned()).or_default(),
        };
        match home {
            Some(home) => {
                roles.insert(role.key().to_owned(), home.to_path_buf());
            }
            None => {
                roles.remove(role.key());
            }
        }
    }
}

/// Chave de um projeto: o caminho canônico, quando existir.
///
/// Canonicalizar faz `C:\proj` e `C:\Proj\..\proj` responderem pela mesma
/// entrada. Se o caminho não existir mais, guarda-se o que veio — perder a
/// escolha é pior do que guardar uma chave que talvez não volte a casar.
fn project_key(root: &Path) -> String {
    root.canonicalize()
        .unwrap_or_else(|_| root.to_path_buf())
        .to_string_lossy()
        .into_owned()
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
    /// Documentos abertos no último uso, na ordem das abas.
    #[serde(default)]
    pub open_documents: Vec<PathBuf>,
    /// Documento em foco no último uso.
    #[serde(default)]
    pub active_document: Option<PathBuf>,
    /// Projetos abertos recentemente, do mais recente para o mais antigo.
    ///
    /// Separado de `last_path` porque respondem perguntas diferentes: um diz o
    /// que reabrir sozinho, o outro oferece uma escolha. Guardar só o último
    /// obrigaria quem alterna entre dois projetos a procurar a pasta toda vez.
    #[serde(default)]
    pub recent_projects: Vec<RecentProject>,
}

/// Um projeto na lista de recentes, e a linguagem em que ele foi reconhecido.
///
/// A linguagem vem junto porque é por ela que o menu agrupa. Ela é **opcional**:
/// uma pasta aberta sem projeto reconhecido continua sendo um recente legítimo,
/// e inventar uma linguagem para ela seria mentir sobre o que a IDE sabe.
///
/// O identificador é guardado, e não o nome de exibição: quem traduz um para o
/// outro é a linguagem que se registrou, e um arquivo de configuração escrito
/// hoje precisa continuar sendo lido quando esse nome mudar.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct RecentProject {
    pub path: PathBuf,
    #[serde(default)]
    pub language: Option<String>,
}

/// Quantos projetos a lista de recentes guarda.
///
/// Uma lista que não esquece vira um arquivo de configuração que só cresce e um
/// menu onde ninguém acha nada. Dez cobre a alternância real entre projetos e
/// ainda cabe na tela.
const RECENTES: usize = 10;

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

    /// Põe um projeto no topo da lista de recentes.
    ///
    /// **Sobe em vez de repetir**: reabrir um projeto que já estava na lista o
    /// move para o topo, e não acrescenta uma segunda linha igual. Uma lista com
    /// o mesmo caminho três vezes desperdiça o pouco espaço que ela tem.
    ///
    /// A linguagem chega depois do caminho — ela só se sabe quando o projeto é
    /// importado —, e por isso `None` **não apaga** a que já estava guardada:
    /// registrar a abertura não pode desclassificar um projeto já conhecido.
    pub fn remember_recent(&mut self, path: &Path, language: Option<String>) {
        let anterior = self
            .recent_projects
            .iter()
            .position(|recente| recente.path == path)
            .map(|posicao| self.recent_projects.remove(posicao));
        let language = language.or_else(|| anterior.and_then(|recente| recente.language));
        self.recent_projects.insert(
            0,
            RecentProject {
                path: path.to_path_buf(),
                language,
            },
        );
        self.recent_projects.truncate(RECENTES);
    }

    /// Os recentes que ainda existem como diretório.
    ///
    /// Uma pasta renomeada, removida ou num disco desconectado continua no
    /// arquivo — ela pode voltar —, mas não é oferecida: um item de menu que
    /// não abre nada é pior do que um item a menos.
    #[must_use]
    pub fn resolved_recent_projects(&self) -> Vec<RecentProject> {
        self.recent_projects
            .iter()
            .filter(|recente| recente.path.is_dir())
            .cloned()
            .collect()
    }

    /// Documentos a reabrir em um projeto, apenas os que ainda são arquivos.
    ///
    /// As abas pertencem ao projeto em que foram abertas: reabrir em outro
    /// projeto mostraria arquivos que nada têm a ver com o trabalho atual. Um
    /// arquivo apagado ou renomeado no meio-tempo é ignorado em silêncio, pela
    /// mesma razão que um projeto inexistente não impede a IDE de abrir.
    #[must_use]
    pub fn resolved_documents(&self, root: &Path) -> Vec<PathBuf> {
        if self.last_path.as_deref() != Some(root) {
            return Vec::new();
        }
        self.open_documents
            .iter()
            .filter(|path| path.is_file())
            .cloned()
            .collect()
    }

    /// Documento a focar na reabertura, se ele estiver entre os restaurados.
    #[must_use]
    pub fn resolved_active_document(&self, root: &Path) -> Option<PathBuf> {
        let restored = self.resolved_documents(root);
        self.active_document
            .as_ref()
            .filter(|path| restored.contains(path))
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
    ///
    /// A linguagem é a em que o projeto foi reconhecido, quando já se sabe. Ver
    /// `WorkspaceConfig::remember_recent`.
    pub fn remember_workspace(
        &mut self,
        root: &Path,
        language: Option<String>,
        path: &Path,
    ) -> Result<(), ConfigError> {
        // Trocar de projeto descarta as abas do anterior. Sem isso elas
        // passariam a valer para a nova raiz, e voltar ao projeto antigo
        // reabriria arquivos de outro.
        if self.workspace.last_path.as_deref() != Some(root) {
            self.workspace.open_documents.clear();
            self.workspace.active_document = None;
        }
        self.workspace.last_path = Some(root.to_path_buf());
        self.workspace.remember_recent(root, language);
        self.save(path)
    }

    /// Registra a ferramenta escolhida para uma seção e grava a configuração.
    ///
    /// `workspace_root` ausente grava o padrão global.
    pub fn remember_tool(
        &mut self,
        workspace_root: Option<&Path>,
        language: &str,
        role: ToolRole,
        home: Option<&Path>,
        path: &Path,
    ) -> Result<(), ConfigError> {
        self.toolchains.choose(workspace_root, language, role, home);
        self.save(path)
    }

    /// Projeto a reabrir na inicialização, se ainda existir.
    #[must_use]
    pub fn resolved_project(&self) -> Option<PathBuf> {
        self.workspace.resolved_last_path()
    }

    /// Registra as abas abertas e grava a configuração.
    ///
    /// A gravação acompanha qualquer mudança do conjunto — abrir, fechar ou
    /// trocar de aba —, porque reabrir uma aba que o usuário fechou é tão
    /// errado quanto perder uma que ele deixou aberta.
    pub fn remember_documents(
        &mut self,
        open: &[PathBuf],
        active: Option<&Path>,
        path: &Path,
    ) -> Result<(), ConfigError> {
        self.workspace.open_documents = open.to_vec();
        self.workspace.active_document = active.map(Path::to_path_buf);
        self.save(path)
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

/// O padrão de `eager_language_providers`.
///
/// Subir junto é o comportamento que se conhece: previsível, e o mesmo de
/// qualquer outra IDE. Sob demanda economiza, e quem quiser a economia liga a
/// chave sabendo o que troca.
const fn sim() -> bool {
    true
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

    /// Providers desligados atravessam o arquivo de configuração.
    ///
    /// A lista é de identificadores, e o `ide-core` não sabe o que nenhum deles
    /// significa — é o que permite a mesma chave servir a qualquer linguagem.
    #[test]
    fn disabled_providers_survive_a_round_trip() {
        let mut config = AppConfig::default();
        assert!(
            config.disabled_providers.is_empty(),
            "por padrão nada está desligado"
        );
        config
            .disabled_providers
            .insert("alguma.coisa".to_owned());
        let texto = match toml::to_string_pretty(&config) {
            Ok(texto) => texto,
            Err(erro) => panic!("a configuração precisa serializar: {erro}"),
        };
        assert!(texto.contains("alguma.coisa"));
        let de_volta: AppConfig = match toml::from_str(&texto) {
            Ok(config) => config,
            Err(erro) => panic!("a configuração precisa voltar: {erro}"),
        };
        assert!(de_volta.disabled_providers.contains("alguma.coisa"));
    }

    /// Configuração antiga, sem a chave, continua carregando.
    ///
    /// Quem já tem um arquivo gravado não pode ficar sem abrir a IDE por causa de
    /// um campo novo.
    #[test]
    fn a_config_without_the_key_still_loads() {
        let antigo = "event_capacity = 64
";
        let config: AppConfig = match toml::from_str(antigo) {
            Ok(config) => config,
            Err(erro) => panic!("configuração antiga precisa carregar: {erro}"),
        };
        assert_eq!(config.event_capacity, 64);
        assert!(config.disabled_providers.is_empty());
    }
    use super::*;

    /// As ferramentas escolhidas sobrevivem ao fechamento da janela.
    ///
    /// Antes disso a principal era redetectada a cada início, e a escolha do
    /// usuário se perdia — a ordem em que a máquina responde decidia por ele.
    #[test]
    fn chosen_tools_survive_a_restart() {
        let raiz = std::env::temp_dir().join(format!("er-config-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&raiz);
        let arquivo = raiz.join("config.toml");
        let principal = raiz.join("principal");
        let secundaria = raiz.join("secundaria");
        assert!(std::fs::create_dir_all(&principal).is_ok());
        assert!(std::fs::create_dir_all(&secundaria).is_ok());

        let mut config = AppConfig::default();
        assert!(
            config
                .remember_tool(None, "l", ToolRole::Primary, Some(&principal), &arquivo)
                .is_ok()
        );
        assert!(
            config
                .remember_tool(None, "l", ToolRole::Secondary, Some(&secundaria), &arquivo)
                .is_ok()
        );

        // O que se lê de volta é o que foi escolhido, no mesmo arquivo.
        let Ok(relido) = AppConfig::load(&arquivo) else {
            panic!("configuração precisa ser relida");
        };
        assert_eq!(
            relido.toolchains.resolved(None, "l", ToolRole::Primary),
            Some(ResolvedTool {
                home: principal,
                origin: ToolOrigin::Default
            })
        );

        // Um caminho que deixou de existir é ignorado: a IDE volta a detectar
        // em vez de recusar-se a abrir.
        assert!(std::fs::remove_dir_all(&secundaria).is_ok());
        assert_eq!(
            relido.toolchains.resolved(None, "l", ToolRole::Secondary),
            None
        );

        let _ = std::fs::remove_dir_all(&raiz);
    }

    /// Dois projetos da mesma linguagem guardam ferramentas diferentes.
    ///
    /// É o critério da fase 0 da `23`: um projeto em Angular 11 e outro em 15
    /// pedem Node de faixas diferentes, e antes disso a escolha era global.
    #[test]
    fn each_project_keeps_its_own_tool_over_the_default() {
        let raiz = std::env::temp_dir().join(format!("er-projetos-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&raiz);
        let padrao = raiz.join("padrao");
        let do_projeto = raiz.join("do-projeto");
        let um = raiz.join("um");
        let outro = raiz.join("outro");
        for pasta in [&padrao, &do_projeto, &um, &outro] {
            assert!(std::fs::create_dir_all(pasta).is_ok());
        }

        let mut config = ToolchainConfig::default();
        config.choose(None, "l", ToolRole::Primary, Some(&padrao));
        config.choose(Some(&um), "l", ToolRole::Primary, Some(&do_projeto));

        // O projeto com sobreposição usa a dele, e diz que veio do projeto.
        let no_um = config.resolved(Some(&um), "l", ToolRole::Primary);
        assert_eq!(no_um.as_ref().map(|tool| tool.origin), Some(ToolOrigin::Project));

        // O outro cai no padrão, e diz que caiu.
        let no_outro = config.resolved(Some(&outro), "l", ToolRole::Primary);
        assert_eq!(no_outro.as_ref().map(|tool| tool.origin), Some(ToolOrigin::Default));

        // Retirar a sobreposição devolve o projeto ao padrão.
        config.choose(Some(&um), "l", ToolRole::Primary, None);
        assert_eq!(
            config
                .resolved(Some(&um), "l", ToolRole::Primary)
                .map(|tool| tool.origin),
            Some(ToolOrigin::Default)
        );

        let _ = std::fs::remove_dir_all(&raiz);
    }

    /// O formato antigo é recolhido cru, para a raiz de composição traduzir.
    ///
    /// Sem isto, o arquivo de quem já usa a IDE carregaria com as escolhas
    /// silenciosamente vazias.
    #[test]
    fn the_old_format_is_collected_instead_of_discarded() {
        let raiz = std::env::temp_dir().join(format!("er-legado-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&raiz);
        assert!(std::fs::create_dir_all(&raiz).is_ok());
        let arquivo = raiz.join("config.toml");
        assert!(
            fs::write(
                &arquivo,
                "[toolchains]\njdk_home = \"/opt/jdk-21\"\nmaven_home = \"/opt/maven\"\n",
            )
            .is_ok()
        );

        let Ok(mut config) = AppConfig::load(&arquivo) else {
            panic!("configuração antiga precisa ser lida");
        };
        let legado = config.toolchains.take_legacy();
        assert_eq!(legado.len(), 2);
        assert_eq!(legado.get("jdk_home").map(PathBuf::as_path), Some(Path::new("/opt/jdk-21")));

        // Retirado uma vez, não volta, e não é regravado.
        assert!(config.toolchains.take_legacy().is_empty());
        assert!(config.save(&arquivo).is_ok());
        let Ok(gravado) = fs::read_to_string(&arquivo) else {
            panic!("configuração precisa ser relida");
        };
        assert!(!gravado.contains("jdk_home"));

        let _ = std::fs::remove_dir_all(&raiz);
    }

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

    /// Reabrir um projeto o traz de volta ao topo, sem duplicar a linha.
    #[test]
    fn os_recentes_guardam_a_ordem_de_uso() {
        let root = temporary("recentes");
        let file = root.join("config").join("config.toml");
        let projetos = ["um", "dois", "tres"].map(|nome| root.join(nome));
        for projeto in &projetos {
            assert!(fs::create_dir_all(projeto).is_ok());
        }

        let mut config = AppConfig::default();
        for projeto in &projetos {
            assert!(
                config
                    .remember_workspace(projeto, Some("java".to_owned()), &file)
                    .is_ok()
            );
        }
        // A reabertura não sabe a linguagem — ela só se sabe depois de importar
        // — e não pode desclassificar o projeto por causa disso.
        assert!(
            config
                .remember_workspace(&projetos[0], None, &file)
                .is_ok()
        );

        let reloaded = match AppConfig::load(&file) {
            Ok(config) => config,
            Err(error) => panic!("releitura falhou: {error}"),
        };
        let caminhos: Vec<_> = reloaded
            .workspace
            .recent_projects
            .iter()
            .map(|recente| recente.path.clone())
            .collect();
        assert_eq!(
            caminhos,
            vec![
                projetos[0].clone(),
                projetos[2].clone(),
                projetos[1].clone()
            ],
            "o reaberto sobe ao topo e não aparece duas vezes"
        );
        assert_eq!(
            reloaded.workspace.recent_projects[0].language.as_deref(),
            Some("java"),
            "a linguagem já conhecida sobrevive a uma reabertura que não a soube"
        );
        let _ = fs::remove_dir_all(&root);
    }

    /// A lista não cresce sem fim, e o que sumiu do disco não é oferecido.
    #[test]
    fn os_recentes_param_de_crescer_e_escondem_o_que_sumiu() {
        let root = temporary("recentes-limite");
        let mut config = AppConfig::default();
        for indice in 0..RECENTES + 4 {
            config
                .workspace
                .remember_recent(&root.join(format!("projeto-{indice}")), None);
        }
        assert_eq!(config.workspace.recent_projects.len(), RECENTES);

        let presente = root.join("presente");
        assert!(fs::create_dir_all(&presente).is_ok());
        config
            .workspace
            .remember_recent(&presente, Some("typescript".to_owned()));
        let oferecidos: Vec<_> = config
            .workspace
            .resolved_recent_projects()
            .into_iter()
            .map(|recente| (recente.path, recente.language))
            .collect();
        assert_eq!(
            oferecidos,
            vec![(presente, Some("typescript".to_owned()))],
            "só o que ainda é pasta chega ao menu"
        );
        let _ = fs::remove_dir_all(&root);
    }

    /// As abas voltam com o projeto, e só as que ainda existem.
    #[test]
    fn the_open_tabs_survive_a_restart() {
        let root = temporary("abas");
        let project = root.join("projeto");
        assert!(fs::create_dir_all(&project).is_ok());
        let first = project.join("Primeiro.java");
        let second = project.join("Segundo.java");
        let removed = project.join("Apagado.java");
        for file in [&first, &second, &removed] {
            assert!(fs::write(file, "conteudo").is_ok());
        }
        let file = root.join("config").join("config.toml");

        let mut config = AppConfig::default();
        assert!(config.remember_workspace(&project, None, &file).is_ok());
        let open = vec![first.clone(), second.clone(), removed.clone()];
        assert!(
            config
                .remember_documents(&open, Some(&second), &file)
                .is_ok()
        );

        assert!(fs::remove_file(&removed).is_ok());
        let reloaded = match AppConfig::load(&file) {
            Ok(config) => config,
            Err(error) => panic!("releitura falhou: {error}"),
        };
        assert_eq!(
            reloaded.workspace.resolved_documents(&project),
            vec![first, second.clone()],
            "o arquivo apagado é ignorado em silêncio"
        );
        assert_eq!(
            reloaded.workspace.resolved_active_document(&project),
            Some(second)
        );
    }

    /// Abas pertencem ao projeto em que foram abertas.
    #[test]
    fn tabs_do_not_follow_the_user_to_another_project() {
        let root = temporary("outro-projeto");
        let first_project = root.join("um");
        let second_project = root.join("dois");
        assert!(fs::create_dir_all(&first_project).is_ok());
        assert!(fs::create_dir_all(&second_project).is_ok());
        let document = first_project.join("Classe.java");
        assert!(fs::write(&document, "conteudo").is_ok());
        let file = root.join("config").join("config.toml");

        let mut config = AppConfig::default();
        assert!(config.remember_workspace(&first_project, None, &file).is_ok());
        assert!(
            config
                .remember_documents(std::slice::from_ref(&document), Some(&document), &file)
                .is_ok()
        );
        assert!(
            config
                .workspace
                .resolved_documents(&second_project)
                .is_empty(),
            "outro projeto não herda as abas"
        );

        // Abrir o segundo projeto descarta as abas do primeiro, para que voltar
        // ao primeiro não traga arquivos que não são dele.
        assert!(config.remember_workspace(&second_project, None, &file).is_ok());
        assert!(config.workspace.open_documents.is_empty());
        assert!(
            config
                .workspace
                .resolved_documents(&first_project)
                .is_empty()
        );
    }

    #[test]
    fn the_opened_project_survives_a_restart() {
        let root = temporary("remember");
        let project = root.join("projeto");
        assert!(fs::create_dir_all(&project).is_ok());
        let file = root.join("config").join("config.toml");

        let mut config = AppConfig::default();
        assert!(config.remember_workspace(&project, None, &file).is_ok());
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
        assert!(config.remember_workspace(&project, None, &file).is_ok());
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
