//! Controllers de aplicação com dependências explícitas e sem conhecimento de Winit.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{
        Arc,
        mpsc::{Receiver, Sender},
    },
    thread,
    time::{Instant, SystemTime},
};

use ide_application::{
    ContributionRegistry, IdeEvent, TaskController as ApplicationTaskController,
    TaskExecutionContext, TaskId, ToolchainRegistry,
};
use ide_domain::{DocumentId, DocumentSnapshot, SyntaxSnapshot};
use ide_language_host::{LanguageHost, LanguageHostError};
use ide_project::{
    build::{BuildCommandRequest, BuildSystemAdapter, BuildSystemRegistry},
    model::{ProjectDescriptor, ProjectModel},
};
use ide_ui::DebugView;
use ide_workspace::{FileNode, WorkspaceError, WorkspaceService};
use tokio::sync::oneshot;
use ui_core::Point;
use ui_render_wgpu::WgpuRenderer;
use ui_window_winit::WinitWindow;

use crate::{
    bootstrap::default_goals,
    bridges::{ToolEvent, document_change},
    debug,
    window::ClickTracker,
};

#[derive(Default)]
pub(super) struct NativeWindowState {
    pub(super) window: Option<WinitWindow>,
    pub(super) renderer: Option<WgpuRenderer>,
    /// A tela de abertura, enquanto ela está na tela.
    ///
    /// Fica aqui, e não numa variável do arranque, porque **esperar por ela não
    /// pode bloquear**: uma janela que não processa mensagens por cinco
    /// segundos é declarada travada pelo sistema, que a troca pela janela
    /// fantasma dele — com moldura, título e o aviso de "não respondendo".
    ///
    /// Guardada aqui, quem conta o tempo é o laço de eventos, que continua
    /// girando. Soltá-la fecha a janela.
    pub(super) splash: Option<crate::splash::Splash>,
    pub(super) cursor: Point,
    pub(super) click_tracker: ClickTracker,
    pub(super) control_pressed: bool,
    pub(super) shift_pressed: bool,
    /// `Alt` segurado. Com `Ctrl`, é o que separa as setas do editor do caminho
    /// de volta da navegação.
    pub(super) alt_pressed: bool,
    /// Botão primário segurado, o que caracteriza um arrasto em curso.
    ///
    /// É o que autoriza o relógio a continuar rolando a seleção; sem essa
    /// guarda, uma soltura perdida — a janela some do foco no meio do gesto —
    /// deixaria a vista rolando sozinha para sempre.
    pub(super) primary_pressed: bool,
}

#[derive(Default)]
pub(super) struct WorkspaceController {
    pub(super) service: WorkspaceService,
    /// A busca textual em curso, que é trabalho de workspace como o resto daqui.
    pub(super) search: SearchController<Vec<ide_workspace::SearchMatch>>,
}

impl WorkspaceController {
    /// Os arquivos de uma extensão sob a raiz, lidos do filesystem.
    pub(super) fn source_files(&self, root: &Path, extension: &str) -> Vec<std::path::PathBuf> {
        self.service.source_files(root, extension)
    }

    /// Os níveis até uma pasta, da raiz para ela.
    pub(super) fn scan_path(
        &self,
        root: &Path,
        target: &Path,
    ) -> Vec<(std::path::PathBuf, Vec<FileNode>)> {
        self.service.scan_path(root, target)
    }

    pub(super) fn scan(&self, root: &Path) -> Result<FileNode, WorkspaceError> {
        self.service.scan(root)
    }

    pub(super) fn read_document(&self, path: &Path) -> Result<String, WorkspaceError> {
        self.service.read_document(path)
    }

    /// Move um arquivo do workspace, para seguir um tipo renomeado.
    pub(super) fn rename_path(&self, from: &Path, to: &Path) -> Result<(), WorkspaceError> {
        self.service.rename_path(from, to)
    }

    pub(super) fn save_document(&self, path: &Path, text: &str) -> Result<(), WorkspaceError> {
        self.service.save_document(path, text)
    }

    pub(super) fn modified_at(&self, path: &Path) -> Option<SystemTime> {
        self.service.modified_at(path)
    }
}

#[derive(Default)]
pub(super) struct DocumentController {
    pub(super) language: HashMap<DocumentId, DocumentSnapshot>,
    pub(super) application: HashMap<DocumentId, DocumentSnapshot>,
    pub(super) remembered: Vec<PathBuf>,
}

impl DocumentController {
    pub(super) fn clear(&mut self) {
        self.language.clear();
        self.application.clear();
        self.remembered.clear();
    }

    pub(super) fn synchronize_application(
        &mut self,
        snapshots: &[DocumentSnapshot],
    ) -> Vec<IdeEvent> {
        let open_ids = snapshots
            .iter()
            .map(|snapshot| snapshot.id)
            .collect::<std::collections::HashSet<_>>();
        let closed = self
            .application
            .keys()
            .filter(|id| !open_ids.contains(id))
            .copied()
            .collect::<Vec<_>>();
        let mut events = Vec::new();
        for document_id in closed {
            self.application.remove(&document_id);
            events.push(IdeEvent::DocumentClosed(document_id));
        }
        for snapshot in snapshots {
            let event = match self.application.get(&snapshot.id) {
                None => Some(IdeEvent::DocumentOpened {
                    document_id: snapshot.id,
                    path: snapshot.path.clone(),
                }),
                Some(previous) if previous.version != snapshot.version => {
                    Some(IdeEvent::DocumentChanged {
                        document_id: snapshot.id,
                        version: snapshot.version,
                    })
                }
                Some(_) => None,
            };
            self.application.insert(snapshot.id, snapshot.clone());
            events.extend(event);
        }
        events
    }
}

/// O que a busca por tipo devolve quando termina.
///
/// Traz junto **por que** veio vazia. Sem isso, "nenhuma ocorrência" num projeto
/// cheio de tipos não tem explicação em lugar nenhum — que foi o defeito achado
/// no primeiro projeto grande de verdade.
pub(super) struct TypeSearchOutcome {
    pub(super) found: Vec<ide_domain::SemanticSymbol>,
    pub(super) failure: Option<String>,
}

/// O que a navegação achou, quando ela termina.
pub(super) struct NavigationOutcome {
    /// O nome sobre o qual se clicou, para a mensagem dizer de quem se fala.
    pub(super) token: String,
    pub(super) location: Option<ide_domain::Location>,
    pub(super) failure: Option<String>,
}

/// O que a completação trouxe, e para qual pergunta.
///
/// A janela de inspeção precisa de **duas** perguntas ao host em sequência, e
/// entre elas há uma decisão que só a tela sabe tomar. Por isso ela aparece aqui
/// em duas etapas: a primeira volta ao laço de quadros, que decide o alvo e
/// dispara a segunda. Cada etapa é uma ida à thread; nenhuma é uma espera.
pub(super) enum CompletionOutcome {
    /// A lista do editor, com a pergunta que a originou.
    ///
    /// A pergunta volta junto porque a resposta pode chegar depois de o cursor
    /// já ter andado: quem digita `for` recebe três respostas, e as duas
    /// primeiras já não valem quando chegam. Sem esta comparação a lista
    /// piscaria com o conteúdo de duas teclas atrás.
    Editor {
        pedido: ide_domain::CompletionRequest,
        items: Result<Vec<ide_domain::CompletionItem>, String>,
    },
    /// O receptor sob o cursor, na janela de inspeção. Primeira etapa.
    AcessoNaInspecao {
        document_id: DocumentId,
        access: Result<Option<ide_language_api::MemberAccess>, String>,
    },
    /// Os membros do tipo escolhido, na janela de inspeção. Segunda etapa.
    MembrosNaInspecao(Result<Vec<ide_domain::CompletionItem>, String>),
    /// O que mais muda no arquivo por causa do item aceito — o `import`.
    ///
    /// Vazio é a resposta normal: quase todo item já está ao alcance.
    Trocas(Vec<ide_domain::TextEdit>),
}

#[derive(Default)]
pub(super) struct LanguageController {
    /// O host é compartilhado porque a busca por tipo o consulta de outra thread.
    pub(super) host: Option<Arc<LanguageHost>>,
    pub(super) contributions: ContributionRegistry,
    pub(super) toolchains: ToolchainRegistry,
    /// Quando se perguntou pela última vez se havia provider ocioso.
    pub(super) last_suspension_check: Option<Instant>,
    /// Quando a memória foi medida pela última vez, e quanto deu.
    pub(super) last_memory_check: Option<Instant>,
    pub(super) memory: ide_core::MemoryReading,
    /// Documentos já reoferecidos a um provider que subiu depois.
    ///
    /// Ver a reoferta em `synchronize_languages`: uma tentativa por documento.
    pub(super) reoffered: std::collections::HashSet<DocumentId>,
    /// Desde quando alguma linguagem prepara o projeto, para o giro ter relógio.
    pub(super) preparing_since: Option<Instant>,
    /// Se já se reclamou deste giro no log.
    ///
    /// Uma vez por carregamento, e não a cada quadro: o laço passa aqui trinta
    /// vezes por segundo, e um aviso repetido nessa cadência esconde o resto.
    pub(super) reclamou_do_giro: bool,
    /// Providers cuja preparação **não** é da IDE.
    ///
    /// A lista vem da raiz de composição, e não daqui: este módulo não sabe o
    /// nome de analisador nenhum, e a guarda de arquitetura existe para que
    /// continue assim. Ver a fase 6 da `25`.
    pub(super) alheios: Vec<ide_domain::ProviderId>,
    /// A busca por tipo em curso, que fala com o analisador e não pode esperar.
    pub(super) type_search: SearchController<TypeSearchOutcome>,
    /// Que espécie de tipo cada arquivo declara, para o crachá do Explorer.
    ///
    /// Uma varredura do índice inteiro, e por isso fora da thread da janela.
    /// Chega quando chegar: até lá o Explorer mostra os nomes sem crachá, que é
    /// a verdade — ninguém sabia ainda.
    pub(super) declaration_kinds: SearchController<HashMap<u64, ide_domain::SymbolKind>>,
    /// A navegação em curso. Ver `navigate_to_definition`.
    pub(super) navigation: SearchController<NavigationOutcome>,
    /// Os usos de um nome, procurados fora da thread da interface.
    ///
    /// Medido no monorepo de referência: 6,8 s para um nome muito usado.
    /// Na thread da janela isso seria a IDE congelada por sete segundos —
    /// o defeito que a `25` caçou cinco vezes.
    pub(super) referencias: SearchController<Vec<ide_domain::Location>>,
    /// A completação em curso. **Sexta vez do mesmo defeito.**
    ///
    /// Ela era feita na chamada, e por isso o ponto congelava a tela: o prazo
    /// com o analisador é de cinco segundos, e enquanto ele corria nenhum quadro
    /// era desenhado. As teclas seguintes não sumiam — ficavam na fila da janela
    /// e apareciam todas de uma vez quando a resposta voltava, que é como quem
    /// usa descreveu: *"como se eu estivesse digitando algo invisível"*.
    ///
    /// Ela escapou das cinco correções anteriores porque sempre respondia
    /// rápido nos testes: o índice alcança o que está declarado no projeto, em
    /// milissegundos. Só apareceu quando alguém pediu um tipo do próprio
    /// TypeScript — `let w: String[]` —, que o índice não tem e o analisador
    /// responde devagar.
    pub(super) completion: SearchController<CompletionOutcome>,
    /// Quedas já anunciadas, para não repetir o aviso a cada verificação.
    pub(super) announced_failures: std::collections::HashSet<String>,
    /// Realces pedidos e ainda não entregues.
    ///
    /// Guardá-los é o que permite não esperar: a tecla posta o pedido, e o
    /// resultado é colhido quando o provider terminar.
    pending_syntax: Vec<oneshot::Receiver<Result<SyntaxSnapshot, LanguageHostError>>>,
}

impl LanguageController {
    /// Recolhe os realces que já chegaram do provider, sem esperar por nenhum.
    ///
    /// A análise roda na thread do provider desde sempre; o que a punha no meio
    /// da digitação era esperar por ela. Agora a tecla posta o pedido e o
    /// resultado é colhido aqui, um ou dois quadros depois — o editor já ignora
    /// realce de revisão vencida, então chegar tarde não desenha nada errado.
    pub(super) fn collect_syntax(&mut self) -> Vec<SyntaxSnapshot> {
        let mut prontos = Vec::new();
        self.pending_syntax.retain_mut(|receiver| {
            match receiver.try_recv() {
                Ok(Ok(snapshot)) => {
                    prontos.push(snapshot);
                    false
                }
                Ok(Err(error)) => {
                    tracing::warn!(%error, "syntax snapshot failed");
                    false
                }
                // Vazio significa que o provider ainda está analisando.
                Err(oneshot::error::TryRecvError::Empty) => true,
                Err(oneshot::error::TryRecvError::Closed) => false,
            }
        });
        prontos
    }

    /// Se alguma contribuição declara a extensão deste arquivo.
    ///
    /// Este teste dizia `java` à mão, e por isso um `.ts` era descartado **antes**
    /// de chegar ao host: o provider estava registrado, o roteamento por extensão
    /// funcionava, e nada disso importava. Na tela, um `.ts` abria sem realce
    /// nenhum.
    ///
    /// O defeito sobreviveu à fase 1 da `23` porque o teste que a deu por
    /// cumprida montava um `LanguageHost` e falava com ele — a camada que eu
    /// acabara de mexer —, e concluía sobre a camada de cima. Ver a fase 1b.
    ///
    /// Perguntar às contribuições é o que faz uma linguagem nova ser vista sem
    /// que este arquivo mude.
    fn has_provider(&self, path: &std::path::Path) -> bool {
        let Some(extension) = path.extension().and_then(|value| value.to_str()) else {
            return false;
        };
        self.contributions.iter().any(|contribution| {
            contribution
                .descriptor
                .extensions
                .iter()
                .any(|declarada| declarada.eq_ignore_ascii_case(extension))
        })
    }

    /// Quantos realces ainda estão sendo esperados.
    ///
    /// Só os testes precisam disso: é como se afirma que a tecla deixou a
    /// análise pendente em vez de esperar por ela.
    #[cfg(test)]
    pub(super) fn pending_syntax(&self) -> usize {
        self.pending_syntax.len()
    }

    pub(super) fn synchronize_documents(
        &mut self,
        documents: &mut DocumentController,
        snapshots: &[DocumentSnapshot],
    ) -> Vec<SyntaxSnapshot> {
        let Some(host) = self.host.as_ref() else {
            return Vec::new();
        };
        let open_ids = snapshots
            .iter()
            .map(|snapshot| snapshot.id)
            .collect::<std::collections::HashSet<_>>();
        let closed = documents
            .language
            .keys()
            .filter(|id| !open_ids.contains(id))
            .copied()
            .collect::<Vec<_>>();
        for document_id in closed {
            let _ = pollster::block_on(host.close_document(host.request_context(), document_id));
            documents.language.remove(&document_id);
        }

        // **Um provider que subiu depois não tem o texto de nada.**
        //
        // Desde a fase 5 da `25`, o analisador externo sobe na primeira pergunta
        // que o índice não soube — e sobe sem documento nenhum. Quem tem o texto
        // é esta camada, e é aqui que ele é reentregue: esquecer o registro faz
        // o laço abaixo reabrir, e reabrir agora alcança quem acabou de subir.
        //
        // Sem isto, o analisador ficava de pé sem projeto para montar. O
        // `tsserver` não carrega projeto sem um arquivo aberto, então o sinal de
        // prontidão nunca chegava e **o giro do carregamento não parava mais**.
        //
        // **Uma tentativa por documento.** Se o analisador recusar o arquivo, o
        // documento continua faltando a ele para sempre, e sem esta marca o laço
        // o reabriria a cada quadro — trinta vezes por segundo, contra um
        // processo que já disse não. Insistir não ajuda ninguém.
        for document_id in host.documents_missing_providers() {
            if self.reoffered.insert(document_id) {
                documents.language.remove(&document_id);
            }
        }

        let mut syntax = Vec::new();
        for snapshot in snapshots {
            if !self.has_provider(&snapshot.path) {
                continue;
            }
            let changed = documents
                .language
                .get(&snapshot.id)
                .is_none_or(|previous| previous.version != snapshot.version);
            // Abrir é raro e o resto depende dele: esse ainda espera. A mudança,
            // que é o que acontece a cada tecla, apenas entra na fila.
            let result = match documents.language.get(&snapshot.id) {
                None => {
                    // **Abrir também não espera.** O comentário antigo aqui dizia
                    // que abrir é raro e que o resto depende dele; o que faltava
                    // ver é que o worker atende um pedido por vez. Com um
                    // analisador ocupado montando o projeto, a abertura ficava
                    // enfileirada atrás de um pedido com prazo de cinco segundos
                    // — e a janela parava, a cada quadro, até ele terminar.
                    host.post_open_document(host.request_context(), snapshot.clone())
                        .map(|_| ())
                }
                Some(previous) if previous.version != snapshot.version => {
                    let change = document_change(previous, snapshot);
                    host.post_change_document(host.request_context(), change)
                        .map(|_| ())
                }
                Some(_) => Ok(()),
            };
            if let Err(error) = result {
                // Duas falhas, e o tratamento é oposto.
                //
                // O provider morreu e o documento perdeu a rota: **esquecer** o
                // registro é o que faz a próxima sincronização reabri-lo, e a
                // reabertura escolhe o candidato seguinte — o provider nativo.
                // É a outra metade da queda da fase 3b da `23`; o host desfaz a
                // rota, e quem tem o texto para reabrir é esta camada.
                //
                // A fila cheia é o contrário: o documento continua como estava
                // do lado do provider, e por isso o registro **não** avança. A
                // próxima sincronização calcula a diferença do mesmo ponto e
                // tenta de novo, com um pedaço maior (ADR-017).
                if matches!(
                    error,
                    LanguageHostError::ProviderGone(_) | LanguageHostError::DocumentNotRouted(_)
                ) {
                    tracing::warn!(%error, document_id = snapshot.id.0, "documento sem provider, será reaberto");
                    documents.language.remove(&snapshot.id);
                } else {
                    tracing::warn!(%error, document_id = snapshot.id.0, "syntax update failed");
                }
                continue;
            }
            documents.language.insert(snapshot.id, snapshot.clone());
            if changed {
                match host.post_syntax(host.request_context(), snapshot.id) {
                    Ok(receiver) => self.pending_syntax.push(receiver),
                    Err(error) => {
                        tracing::warn!(%error, document_id = snapshot.id.0, "syntax snapshot failed");
                    }
                }
            }
        }
        // O que já estava pronto de rodadas anteriores entra agora.
        syntax.extend(self.collect_syntax());
        syntax
    }
}

pub(super) struct ImportedProject {
    pub(super) adapter: Arc<dyn BuildSystemAdapter>,
    pub(super) descriptor: ProjectDescriptor,
    pub(super) model: ProjectModel,
    pub(super) manifest_modified: Option<SystemTime>,
}

/// Instalações do Maven encontradas e a escolhida.
///
/// Fica junto do projeto porque é dele que o Maven serve: compilar, importar e
/// executar passam pelo mesmo executável.
#[derive(Default)]
pub(super) struct MavenController {
    pub(super) installations: Vec<language_java::MavenInstallation>,
    pub(super) selected: Option<usize>,
}

impl MavenController {
    /// Casa do Maven escolhido, se houver escolha válida.
    pub(super) fn home(&self) -> Option<PathBuf> {
        self.selected
            .and_then(|index| self.installations.get(index))
            .map(|instalacao| instalacao.home.clone())
    }

    /// Rótulos para a lista da janela de configurações.
    pub(super) fn labels(&self) -> Vec<String> {
        self.installations
            .iter()
            .map(language_java::MavenInstallation::label)
            .collect()
    }

    /// Põe uma instalação na lista, sem repetir, e a deixa escolhida.
    pub(super) fn adopt(&mut self, instalacao: language_java::MavenInstallation) -> usize {
        let indice = self
            .installations
            .iter()
            .position(|outra| outra.home == instalacao.home)
            .unwrap_or_else(|| {
                self.installations.push(instalacao);
                self.installations.len() - 1
            });
        self.selected = Some(indice);
        indice
    }
}

#[derive(Default)]
pub(super) struct ProjectController {
    pub(super) build_systems: BuildSystemRegistry,
    /// Instalações do Maven e a escolhida.
    pub(super) maven: MavenController,
    pub(super) imported: Option<ImportedProject>,
    pub(super) last_manifest_check: Option<Instant>,
}

impl ProjectController {
    pub(super) fn reset_import(&mut self) -> Option<ImportedProject> {
        self.last_manifest_check = None;
        self.imported.take()
    }

    pub(super) fn build_plan(
        &self,
        java_home: Option<&Path>,
    ) -> Option<(Arc<dyn BuildSystemAdapter>, BuildCommandRequest, String)> {
        let project = self.imported.as_ref()?;
        let mut request = BuildCommandRequest::new(
            project.descriptor.clone(),
            default_goals(&project.descriptor),
        );
        if let Some(home) = java_home {
            request =
                request.with_environment_variable("JAVA_HOME", home.to_string_lossy().into_owned());
        }
        let label = format!(
            "[{}] {}",
            project.descriptor.build_system.label(),
            request.goals.join(" ")
        );
        Some((project.adapter.clone(), request, label))
    }
}

#[derive(Default)]
pub(super) struct TaskController {
    pub(super) controller: ApplicationTaskController,
    pub(super) events: Option<Receiver<ToolEvent>>,
    pub(super) sender: Option<Sender<ToolEvent>>,
}

impl TaskController {
    fn spawn(
        &self,
        name: &str,
        label: String,
        work: impl FnOnce(&tokio::runtime::Runtime) -> ToolEvent + Send + 'static,
    ) -> Result<(), String> {
        let sender = self
            .sender
            .clone()
            .ok_or_else(|| "canal de ferramentas indisponível".to_owned())?;
        thread::Builder::new()
            .name(name.to_owned())
            .spawn(move || {
                let runtime = match tokio::runtime::Builder::new_current_thread()
                    .enable_time()
                    .build()
                {
                    Ok(runtime) => runtime,
                    Err(error) => {
                        let _ = sender.send(ToolEvent {
                            status: format!("{label} falhou"),
                            stdout: String::new(),
                            stderr: error.to_string(),
                        });
                        return;
                    }
                };
                let _ = sender.send(work(&runtime));
            })
            .map(|_| ())
            .map_err(|error| error.to_string())
    }

    pub(super) fn execute_build(
        &self,
        adapter: Arc<dyn BuildSystemAdapter>,
        request: BuildCommandRequest,
        label: String,
    ) -> Result<(), String> {
        let error_label = label.clone();
        self.spawn("project-build", label.clone(), move |runtime| match runtime
            .block_on(adapter.execute(request))
        {
            Ok(result) => ToolEvent {
                status: if result.success {
                    format!("{label} concluído")
                } else {
                    format!("{label} falhou ({})", result.exit_code)
                },
                stdout: format!("{}\n{}", result.command_line, result.stdout),
                stderr: result.stderr,
            },
            Err(error) => ToolEvent {
                status: format!("{error_label} falhou"),
                stdout: String::new(),
                stderr: error.to_string(),
            },
        })
    }

    pub(super) fn execute_task(
        &self,
        task_id: TaskId,
        context: TaskExecutionContext,
        label: String,
    ) -> Result<(), String> {
        let controller = self.controller.clone();
        let error_label = label.clone();
        self.spawn("language-task", label, move |runtime| {
            match runtime.block_on(controller.execute(&task_id, context)) {
                Ok(result) => ToolEvent {
                    status: result.status,
                    stdout: result.stdout,
                    stderr: result.stderr,
                },
                Err(error) => ToolEvent {
                    status: format!("{error_label} falhou"),
                    stdout: String::new(),
                    stderr: error.to_string(),
                },
            }
        })
    }
}

#[derive(Default)]
pub(super) struct DebugController {
    pub(super) session: Option<debug::DebugController>,
    pub(super) view: DebugView,
    pub(super) thread: Option<ide_debug_api::ThreadId>,
}

/// A busca textual em curso, que roda fora da thread da interface.
///
/// **Ela não cabe num quadro e não tem como caber.** Medido contra um projeto
/// real de 8 958 arquivos: 1,4 s com o cache do sistema quente e 106 s frio — e
/// frio é o estado na primeira busca depois de abrir o projeto, que é justamente
/// quando ela é pedida. Rodando inline, a janela parava de responder: foi o que
/// aconteceu.
///
/// Uma busca nova cancela a anterior. Sem isso, cada tecla numa caixa de busca
/// deixaria mais uma varredura de um minuto rodando no fundo, todas lendo o mesmo
/// disco e disputando entre si.
///
/// É genérico porque a busca por **tipo** tem o mesmo problema e a mesma forma:
/// ela pergunta ao analisador externo, e num projeto grande a resposta demora o
/// bastante para a janela parar. O que muda é o que volta pelo canal.
pub(super) struct SearchController<T> {
    pub(super) pending: Option<Receiver<T>>,
    pub(super) cancel: Option<ide_domain::CancellationToken>,
    /// Quando esta busca começou, que é de onde sai a fase do giro.
    pub(super) started: Option<Instant>,
}

// `derive(Default)` exigiria `T: Default`, que não é verdade nem necessário: um
// controlador vazio não tem resultado nenhum para inventar.
impl<T> Default for SearchController<T> {
    fn default() -> Self {
        Self {
            pending: None,
            cancel: None,
            started: None,
        }
    }
}

impl<T> SearchController<T> {
    /// Cancela o que estiver em curso e guarda o canal do novo pedido.
    pub(super) fn start(&mut self, receiver: Receiver<T>) -> ide_domain::CancellationToken {
        if let Some(anterior) = self.cancel.take() {
            anterior.cancel();
        }
        let token = ide_domain::CancellationToken::new();
        self.cancel = Some(token.clone());
        self.pending = Some(receiver);
        self.started = Some(Instant::now());
        token
    }

    /// Onde o giro está, de 0 a 1, ou `None` se nada roda.
    ///
    /// Uma volta por segundo: rápido o bastante para se ler como espera ativa,
    /// devagar o bastante para não piscar.
    pub(super) fn spinner_phase(&self) -> Option<f32> {
        let inicio = self.started?;
        self.pending
            .as_ref()
            .map(|_| inicio.elapsed().as_secs_f32().fract())
    }

    /// O resultado, se já chegou. Não espera por nada.
    pub(super) fn collect(&mut self) -> Option<T> {
        let receiver = self.pending.as_ref()?;
        match receiver.try_recv() {
            Ok(achados) => {
                self.pending = None;
                self.cancel = None;
                self.started = None;
                Some(achados)
            }
            // Vazio significa que a varredura ainda está rodando.
            Err(std::sync::mpsc::TryRecvError::Empty) => None,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.pending = None;
                self.cancel = None;
                self.started = None;
                None
            }
        }
    }
}

#[derive(Default)]
pub(super) struct RuntimeState {
    pub(super) startup_error: Option<String>,
    pub(super) config: ide_core::AppConfig,
    pub(super) config_path: Option<PathBuf>,
    /// O supervisor concreto, para perguntar quais processos externos existem.
    pub(super) processes: Option<Arc<ide_process::NativeProcessSupervisor>>,
    /// Raiz do projeto aberto, para resolver a ferramenta em vigor.
    ///
    /// A escolha de ferramenta passou a ser por projeto na fase 0 da `23`, e sem
    /// saber qual projeto está aberto não há o que resolver. Ausente antes da
    /// primeira abertura, quando só o padrão global vale.
    pub(super) workspace_root: Option<PathBuf>,
    /// A primeira sincronização de linguagens ainda não aconteceu.
    ///
    /// Ela ativa o provider, que indexa o JDK e os fontes do projeto — mais de
    /// um segundo. Feita antes do primeiro quadro, deixava a janela já visível
    /// em branco todo esse tempo. Depois do primeiro quadro, o usuário vê a IDE
    /// montada e o realce chega em seguida.
    pub(super) languages_pending: bool,
    /// Projeto recém-aberto cuja ferramenta e importação ainda não rodaram.
    ///
    /// Pelo mesmo motivo de `languages_pending`, e com um custo maior: detectar
    /// a ferramenta e importar o projeto rodam processos externos e **esperam
    /// por eles** — o Maven chega a baixar dependências. Na abertura da IDE isso
    /// acontece antes de a janela existir, e ninguém vê; ao trocar de projeto,
    /// com a janela na tela, o mesmo trabalho é congelamento.
    ///
    /// Guardar a raiz aqui adia o trabalho para depois do primeiro quadro, e
    /// quem trocou vê a árvore nova na hora.
    pub(super) project_pending: Option<PathBuf>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use ide_domain::{DocumentId, DocumentSnapshot};

    fn snapshot(id: u64, version: u64) -> DocumentSnapshot {
        DocumentSnapshot {
            id: DocumentId(id),
            path: PathBuf::from(format!("{id}.java")),
            version,
            text: String::new(),
        }
    }

    /// Uma busca nova cancela a anterior.
    ///
    /// Sem isto, cada tecla numa caixa de busca deixaria mais uma varredura de um
    /// minuto rodando no fundo, todas lendo o mesmo disco.
    #[test]
    fn a_new_search_cancels_the_previous_one() {
        let mut controller = SearchController::<Vec<ide_workspace::SearchMatch>>::default();
        let (_primeiro_envio, primeiro) = std::sync::mpsc::channel();
        let anterior = controller.start(primeiro);
        assert!(!anterior.is_cancelled());

        let (_segundo_envio, segundo) = std::sync::mpsc::channel();
        let atual = controller.start(segundo);
        assert!(
            anterior.is_cancelled(),
            "a busca anterior precisa ser desistida quando outra comeca"
        );
        assert!(!atual.is_cancelled());
    }

    /// Enquanto a varredura roda, colher nao espera por ela.
    ///
    /// E o ponto inteiro da correcao: o quadro segue desenhando.
    #[test]
    fn collecting_does_not_wait_for_the_search() {
        let mut controller = SearchController::<Vec<ide_workspace::SearchMatch>>::default();
        let (envio, receptor) = std::sync::mpsc::channel();
        let _token = controller.start(receptor);
        assert!(controller.collect().is_none());

        assert!(envio.send(Vec::new()).is_ok());
        assert!(
            controller.collect().is_some(),
            "o resultado que chegou precisa ser entregue"
        );
        // E so uma vez: colher de novo nao pode repetir o resultado anterior.
        assert!(controller.collect().is_none());
    }

    /// A fase do giro existe enquanto a busca existe, e some quando ela acaba.
    ///
    /// **É o que decide se o quadro continua sendo pedido.** Uma fase que
    /// sobrevivesse ao resultado deixaria a IDE redesenhando para sempre; uma que
    /// nascesse vazia congelaria o giro no primeiro quadro, e um giro parado
    /// parece a IDE travada — pior do que não ter giro nenhum.
    #[test]
    fn the_spinner_phase_lives_exactly_as_long_as_the_search() {
        let mut controller = SearchController::<Vec<ide_workspace::SearchMatch>>::default();
        assert_eq!(controller.spinner_phase(), None, "sem busca, nada gira");

        let (envio, receptor) = std::sync::mpsc::channel();
        let _token = controller.start(receptor);
        let fase = controller.spinner_phase();
        assert!(
            fase.is_some_and(|valor| (0.0..1.0).contains(&valor)),
            "com busca em curso a fase precisa existir e caber na volta: {fase:?}"
        );

        assert!(envio.send(Vec::new()).is_ok());
        assert!(controller.collect().is_some());
        assert_eq!(
            controller.spinner_phase(),
            None,
            "colhido o resultado, o giro precisa parar"
        );
    }

    /// A thread que morreu sem responder nao deixa a busca pendurada.
    #[test]
    fn a_search_thread_that_dies_does_not_hang_the_controller() {
        let mut controller = SearchController::<Vec<ide_workspace::SearchMatch>>::default();
        let (envio, receptor) = std::sync::mpsc::channel::<Vec<ide_workspace::SearchMatch>>();
        let _token = controller.start(receptor);
        drop(envio);
        assert!(controller.collect().is_none());
        assert!(controller.pending.is_none(), "o canal morto precisa ser largado");
    }

    #[test]
    fn document_controller_reports_open_change_and_close_without_a_window() {
        let mut controller = DocumentController::default();
        assert!(matches!(
            controller
                .synchronize_application(&[snapshot(1, 0)])
                .as_slice(),
            [IdeEvent::DocumentOpened { .. }]
        ));
        assert!(matches!(
            controller
                .synchronize_application(&[snapshot(1, 1)])
                .as_slice(),
            [IdeEvent::DocumentChanged { version: 1, .. }]
        ));
        assert!(matches!(
            controller.synchronize_application(&[]).as_slice(),
            [IdeEvent::DocumentClosed(DocumentId(1))]
        ));
    }
}
