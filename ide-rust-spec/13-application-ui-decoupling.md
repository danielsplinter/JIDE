# 13 — Desacoplamento da aplicação e da apresentação

## Objetivo

Fazer a extensibilidade já existente nos contratos e no host alcançar a IDE
executável inteira. Adicionar uma linguagem não deve exigir novos campos,
condicionais, comandos ou elementos visuais específicos em `NativeIde` ou
`IdeShell`.

A refatoração deve preservar o comportamento Java atual, as fronteiras de crates
consolidadas, a compatibilidade dos projetos e a construção nativa com ERLibUi.
Mover código para arquivos menores sem transferir estado e decisões não atende
ao objetivo.

## Diagnóstico de referência

Medições realizadas em 29/07/2026:

- o workspace possui 19 crates e não apresenta ciclos;
- `ide-domain` é o núcleo mais reutilizado, com 13 consumidores internos;
- `ide-app` possui 16 dependências internas diretas, das quais cinco são
  implementações Java concretas;
- `NativeIde` possui 29 campos e `ide-app/src/main.rs` possui aproximadamente
  2.400 linhas;
- `IdeShell` possui 83 campos e `ide-ui/src/lib.rs` possui aproximadamente
  9.100 linhas;
- `language-java/src/lib.rs` possui aproximadamente 2.250 linhas;
- `ide-language-host/src/lib.rs` possui aproximadamente 1.640 linhas.

As fronteiras inferiores estão saudáveis:

- `ide-domain` não depende de UI, runtime ou linguagem concreta;
- `ide-language-api`, `ide-toolchain-api` e `ide-debug-api` apontam para o
  domínio, não para adapters;
- `ide-language-host` registra e ativa providers sem conhecer Java;
- `ide-ui` não depende diretamente de `language-java` nem de
  `ide-language-host`;
- o grafo de crates é acíclico.

O acoplamento restante está no topo:

- `ide-app` instancia e guarda provider, toolchain, compilador, runtime, testes,
  Maven, Gradle e depurador Java;
- seleção de JDK e execução de tarefas Java fazem parte do estado central;
- `ide-application` expõe criação de `Package`, `Class` e `Interface`;
- `ide-ui` contém textos e fluxos próprios de Java e JDK;
- `ide-workspace` oferece uma operação chamada `search_java_content`;
- módulos internos pequenos coexistem com estruturas centrais que ainda
  concentram quase todo o estado e a coordenação;
- os testes arquiteturais protegem apenas parte das crates e não impedem o
  `ide-app` de ganhar novas dependências concretas.

O teste de provider fictício prova a extensibilidade do host, mas não prova que
a IDE executável aceite uma linguagem adicional sem mudanças.

## Princípios obrigatórios

1. O núcleo da aplicação manipula `LanguageId`, `ToolchainId`, capacidades e
   contratos, nunca `Java*`, JDK, Maven ou Gradle.
2. Implementações concretas só podem aparecer na composição inicial ou em um
   carregador de contribuições.
3. Estado específico de linguagem pertence à contribuição da linguagem.
4. A apresentação recebe modelos e ações genéricos; não interpreta sintaxe nem
   inventa tipos de arquivo de uma linguagem.
5. Cada módulo extraído deve possuir seu estado e sua API mínima.
6. Nenhum módulo de feature pode receber `&mut NativeIde` ou `&mut IdeShell`.
7. A comunicação entre features ocorre por comandos, eventos e modelos
   explícitos.
8. A refatoração não deve criar uma crate para cada arquivo pequeno.
9. As crates de contrato permanecem independentes dos adapters concretos.
10. Toda regra arquitetural relevante deve ser fiscalizada automaticamente.

## Arquitetura-alvo

### Contribuições de linguagem

`ide-application` deve possuir um registro de contribuições consumido pela
aplicação e pela UI. Uma contribuição descreve capacidades sem expor sua
implementação:

```rust
pub struct LanguageContribution {
    pub language_id: LanguageId,
    pub display_name: String,
    pub extensions: Vec<String>,
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
```

Os nomes finais podem mudar, mas as seguintes propriedades são obrigatórias:

- o registro é indexado por `LanguageId`;
- a aplicação não cria campos por linguagem;
- capacidades opcionais são representadas por contratos;
- templates, configurações e tarefas são dados, não condicionais na UI;
- o provider continua isolado pelo `ide-language-host`.

Enquanto o carregamento dinâmico de plugins descrito em
`07-plugins.md` não estiver implementado, contribuições embutidas podem ser
ligadas estaticamente no composition root. Adicionar uma contribuição embutida
pode alterar o catálogo de composição e o `Cargo.toml`, mas não pode alterar
estado, casos de uso ou apresentação.

### Registro de toolchains e tarefas

O estado atual separado em `java_toolchains`, `java_compiler`, `java_runtime` e
`java_tests` deve ser substituído por registros genéricos:

```rust
pub struct ToolchainRegistry {
    selections: HashMap<LanguageId, ToolchainSelection>,
}

pub struct TaskRegistry {
    tasks: HashMap<TaskId, TaskDescriptor>,
}
```

Compilar, executar e testar devem ser o mesmo caso de uso para qualquer
linguagem. Detalhes como `javac`, classe principal e argumentos JVM pertencem ao
adapter ou à contribuição Java.

### Build systems

`BuildSystemRegistry` permanece genérico. Maven e Gradle são contribuições Java
ou contribuições de projeto registradas na composição, não campos nem ramos
especiais de `NativeIde`.

O modelo importado determina source roots, classpath, tarefas disponíveis e
ambiente. A aplicação apenas coordena o contrato `BuildSystemAdapter`.

### Apresentação orientada a modelos

A UI deve receber modelos genéricos:

- `ToolchainSettingsModel`, sem campo ou label fixo de JDK;
- `NewItemTemplate`, sem enum fechado em pacote, classe e interface;
- `SearchScope`, sem método chamado `search_java_content`;
- `TaskDescriptor`, para botões e menus de compilar, executar e testar;
- `LanguageStatus`, sem texto fixo iniciado por `Java:`.

Textos próprios de Java vêm da contribuição Java. A UI pode renderizar esses
textos, mas não pode produzi-los com regras Java.

### Estado interno de `ide-ui`

`IdeShell` deve se tornar um coordenador pequeno composto por estados de feature:

```text
IdeShell
├── ExplorerState
├── EditorAreaState
├── TerminalPanelState
├── SearchState
├── SettingsState
├── DebugPanelState
├── MenuState
└── ShellCommandQueue
```

Cada estado é responsável por seus widgets, foco, seleção, rolagem e ações. O
layout recebe referências imutáveis aos modelos necessários e devolve
geometrias; eventos são encaminhados à feature proprietária.

Meta estrutural:

- `IdeShell` com no máximo 15 campos de coordenação;
- nenhuma feature com mais de 20 campos sem uma justificativa documentada;
- `ide-ui/src/lib.rs` com no máximo 1.500 linhas;
- nenhum módulo extraído acessando todos os estados da shell.

Os limites de linhas e campos são guardrails, não objetivos isolados. Cumpri-los
sem criar fronteiras coesas não conclui a refatoração.

### Estado interno de `ide-app`

`NativeIde` deve separar:

```text
NativeIde
├── NativeWindowState
├── WorkspaceController
├── DocumentController
├── LanguageController
├── ProjectController
├── TaskController
├── DebugController
└── UiBridge
```

Cada controller recebe somente suas portas e produz comandos ou eventos. A
janela Winit continua no adapter nativo, e `NativeIde` apenas coordena o ciclo de
eventos.

Meta estrutural:

- `NativeIde` com no máximo 12 campos de coordenação;
- `ide-app/src/main.rs` com no máximo 800 linhas;
- nenhum controller recebendo `&mut NativeIde`;
- implementações Java ausentes dos campos de `NativeIde`;
- lógica de casos de uso testável sem Winit ou WGPU.

### Providers e host

`JavaLanguage` e `LanguageHost` já possuem estado central relativamente pequeno,
mas seus arquivos raiz continuam grandes. A extração deve mover implementações,
não apenas helpers:

- documentos analisados e parsing incremental para `documents`;
- construção e consulta do índice para `index`;
- semântica e navegação para seus módulos;
- ciclo do worker e despacho de mensagens para `worker`;
- registro e seleção de providers para `registry` e `routing`.

Os arquivos raiz devem expor a fachada pública e coordenar módulos proprietários
do estado.

## Plano incremental

### Fase 1 — Fiscalização completa ✅

- [x] registrar uma linha de base do grafo e dos arquivos centrais;
- [x] ampliar as regras de dependência para todas as crates;
- [x] proibir dependências concretas Java fora da composição autorizada;
- [x] testar que `ide-ui`, `ide-application` e `ide-workspace` não contêm APIs
  específicas de Java;
- [x] manter o teste de ciclos incluindo dependências normais, de build, teste e
  plataforma;
- [x] adicionar teste que compile uma contribuição falsa usando os mesmos
  contratos consumidos pela aplicação.

Implementação concluída em 29/07/2026:

- o mapa arquitetural passou a cobrir as 19 crates e falha se uma crate nova não
  receber uma regra explícita;
- somente `ide-app` pode consumir os adapters Java concretos;
  `java-classfile` permanece detalhe interno de `language-java`;
- `NewItemTemplateId`, `search_content`, `source_files` e as operações genéricas
  de toolchain removeram conceitos Java das APIs públicas das crates neutras; o
  teste arquitetural exige dívida zero;
- `LanguageContribution` estabelece o contrato mínimo já consumido pelo
  composition root real. O teste de integração em `ide-app` registra e ativa
  uma contribuição falsa por esse mesmo caminho;
- a Fase 2 ampliou esse contrato com tarefas, templates, configurações e
  toolchains sem mudar o caminho de ativação entregue nesta fase.

### Fase 2 — Registro de contribuições ✅

- [x] criar os descritores genéricos de linguagem, tarefas, templates e
  configurações;
- [x] implementar registro indexado por `LanguageId`;
- [x] substituir campos Java de `NativeIde` por registros;
- [x] mover a montagem Java para uma contribuição isolada;
- [x] preservar ativação preguiçosa e ciclo de vida dos providers.

Implementação concluída em 29/07/2026:

- `ide-application::contributions` define `LanguageDescriptor`,
  `TaskDescriptor`, `NewItemTemplate`, `SettingsSection` e
  `LanguageContribution`, com capacidades opcionais representadas pelos
  contratos de provider e toolchain;
- `ContributionRegistry` indexa contribuições por `LanguageId`, valida a
  linguagem declarada pelos providers e adapters e rejeita duplicatas;
- `ToolchainRegistry` guarda seleções por linguagem e `TaskRegistry` recebe
  tarefas declaradas pelas contribuições;
- `NativeIde` passou a possuir somente os registros genéricos
  `contributions`, `toolchains` e `tasks`; os campos `java_toolchains`,
  `java_compiler`, `java_runtime` e `java_tests` foram removidos;
- `ide-app/src/java_contribution.rs` é o único módulo que instancia o provider,
  o toolchain, os adapters de compilação, execução, testes e depuração Java,
  Maven e Gradle; o `DebugController` recebe `Arc<dyn DebugAdapter>`;
- o teste de contribuição falsa comprova registro de descritores, indexação,
  estado inicial `Registered`, ativação somente ao abrir um documento e estado
  final `Active`.

### Fase 3 — Casos de uso neutros ✅

- [x] unificar compilação, execução e testes em `TaskController`;
- [x] mover descoberta e seleção de JDK para a contribuição Java;
- [x] manter Maven e Gradle atrás de `BuildSystemAdapter`;
- [x] remover `JavaTask` e métodos `detect_java_*` de `ide-app`;
- [x] garantir que uma contribuição falsa execute uma tarefa falsa ponta a
  ponta sem alterar controllers.

Implementação concluída em 29/07/2026:

- `TaskExecutor`, `TaskExecutionContext` e `TaskExecutionResult` formam o caso
  de uso neutro; `TaskController` resolve `TaskId`, linguagem e executor sem
  ramos por linguagem;
- `NativeIde::start_task` apenas coleta contexto genérico, despacha pelo
  controller e publica o resultado. A montagem de `CompilationRequest`,
  `ExecutionRequest`, `TestRequest`, classpath e classe principal foi movida
  para o executor da contribuição Java;
- `ToolchainRegistry` registra providers por `LanguageId`, executa descoberta,
  mantém seleção e resolve uma instalação apontada manualmente. O conhecimento
  de JDK permanece em `JavaToolchainProvider`;
- `ToolchainProvider::resolve_installation` tornou a seleção manual uma
  capacidade do provider, removendo o uso direto de `JavaToolchainProvider` da
  aplicação;
- `JavaTask` e os métodos `start_java_task` e `detect_java_*` foram removidos;
- Maven e Gradle continuam acessados somente como `BuildSystemAdapter` dentro
  de `BuildSystemRegistry`;
- a contribuição fictícia registra `fake.run` e o executa ponta a ponta pelo
  mesmo `TaskController`, comprovando que controllers não precisam mudar para
  receber outra linguagem.

### Fase 4 — UI neutra ✅ Concluída

- [x] substituir `NewItemKind` fechado por templates registrados;
- [x] substituir configurações de JDK por seções genéricas;
- [x] substituir textos fixos Java por modelos fornecidos;
- [x] tornar busca de conteúdo neutra e dirigida por `SearchScope`;
- [x] gerar menus e botões de tarefas a partir de descritores;
- [x] manter todos os fluxos Java atuais sem regressão visual.

Implementação concluída:

- `ContributionRegistry::ui_catalog` agrega nomes de linguagem, raízes de
  fontes, `NewItemTemplate`, `SettingsSection` e `TaskDescriptor` sem expor
  providers concretos à apresentação;
- `IdeShell::set_ui_catalog` reconstrói a árvore, menus, páginas de
  configuração e o botão de tarefa a partir desse catálogo;
- o menu contextual do Explorer usa identificadores completos de template e a
  janela de criação usa título, legenda e obrigatoriedade fornecidos pelo
  descritor, sem constantes de pacote, classe ou interface;
- as páginas contribuídas usam título, legenda da toolchain e texto do botão
  fornecidos por `SettingsSection`; a página interna de depuração permanece
  independente das linguagens;
- `ApplicationCommand::ExecuteTask(TaskId)` substitui os três comandos
  fechados de compilar, executar arquivo e testar; menu, botão e atalhos
  convergem para o mesmo despacho pelo `TaskController`;
- `SearchScope` contém raízes e extensões explícitas. `ide-workspace` não
  reconhece nomes de diretório de linguagem e não pesquisa fora do escopo;
- quando o modelo de projeto ainda não está disponível, a aplicação encontra
  raízes pelos nomes declarados em `LanguageDescriptor`, constrói o
  `SearchScope` e mantém o `Ctrl+Shift+L` funcional;
- a apresentação do caminho dos resultados usa os nomes de raiz registrados,
  preservando para Java o caminho relativo após a última pasta `java`;
- testes com uma contribuição fictícia comprovam geração de template, seção e
  tarefa sem mudanças na UI, e um guardrail arquitetural impede a volta de
  templates Java fixos ou da inferência de `java` na busca.

### Fase 5 — Decomposição de `IdeShell` ✅ Concluída

- [x] transferir estado e comportamento do Explorer para `ExplorerState`;
- [x] transferir terminais e seleção para `TerminalPanelState`;
- [x] transferir pesquisas e modais para `SearchState`;
- [x] transferir configurações para `SettingsState`;
- [x] transferir depuração e inspeção para `DebugPanelState`;
- [x] reduzir `IdeShell` e `ide-ui/src/lib.rs` às metas estruturais;
- [x] proibir referências cruzadas diretas entre estados de feature.

Implementação concluída em 29/07/2026:

- `ExplorerState`, `EditorAreaState`, `TerminalPanelState`, `SearchState`,
  `SettingsState`, `DebugPanelState` e `MenuState` são donos dos dados e das
  transições locais de suas features; `IdeShell` coordena eventos que atravessam
  features, o catálogo de contribuições e a fila de comandos da aplicação;
- `IdeShell` passou de 83 para 10 campos de coordenação. Contexto compartilhado
  e ponteiro ficam em `ShellContext`, sem expor o estado inteiro a uma feature;
- `ide-ui/src/lib.rs` passou de aproximadamente 9.300 linhas para uma fachada
  pública de 30 linhas. A implementação da shell está em `ide_shell.rs`, e
  Explorer, editor, terminal, busca, configurações, depuração, menus e layout
  possuem módulos próprios;
- nenhum estado de feature possui mais de 20 campos. Os módulos extraídos não
  recebem `IdeShell` e não referenciam diretamente o estado de outra feature;
- o teste arquitetural `phase_five_keeps_ui_state_split_by_feature` mede o teto
  de campos, o tamanho da fachada e as dependências entre estados, impedindo a
  recomposição futura da classe concentradora;
- os 136 testes da UI preservam edição, Explorer, terminais, buscas, modais,
  configurações e depuração. `cargo test --workspace`, Clippy estrito e o build
  release de `ide-app` foram executados sem falhas.

### Fase 6 — Decomposição de `NativeIde` ✅ Concluída

- [x] criar controllers com dependências explícitas;
- [x] mover documentos e sincronização de linguagem para controllers próprios;
- [x] mover projeto, build e tarefas para controllers próprios;
- [x] isolar tradução entre `ApplicationCommand` e casos de uso em `UiBridge`;
- [x] reduzir `NativeIde` e `ide-app/src/main.rs` às metas estruturais;
- [x] testar controllers sem criar janela nativa.

Implementação concluída em 29/07/2026:

- `NativeIde` passou de 30 campos para 9 objetos coordenadores:
  `NativeWindowState`, `WorkspaceController`, `DocumentController`,
  `LanguageController`, `ProjectController`, `TaskController`,
  `DebugController`, `UiBridge` e `RuntimeState`;
- `DocumentController` calcula eventos de abertura, alteração e fechamento;
  `LanguageController` sincroniza o ciclo dos documentos com `LanguageHost` e
  devolve snapshots de sintaxe; nenhum deles conhece Winit, WGPU ou `NativeIde`;
- `WorkspaceController` concentra leitura, gravação, varredura e metadados do
  workspace. `ProjectController` mantém build systems, projeto importado e
  relógio de reimportação; tarefas e eventos de ferramenta ficam em
  `TaskController`;
- `UiBridge` é dona da shell, do barramento de eventos e do histórico de
  navegação. A conversão exaustiva de `ApplicationCommand` para `UiAction`
  acontece antes do despacho, removendo a tradução direta de `NativeIde`;
- a implementação nativa foi movida para `native_ide.rs`; `main.rs` passou de
  2.395 para 15 linhas e contém somente declaração dos módulos, composição e
  ponto de entrada. `NativeIde` ficou abaixo do teto de 12 campos;
- testes unitários exercitam `DocumentController` e `UiBridge` sem criar janela.
  O teste arquitetural `phase_six_keeps_native_application_split_into_controllers`
  mede campos e linhas, exige os casos de uso extraídos, impede controllers de
  conhecerem `NativeIde` e impede a volta da tradução direta;
- `cargo test --workspace`, Clippy estrito e o build release de `ide-app` foram
  executados sem falhas.

### Fase 7 — Fachadas de linguagem e host ✅ Concluída

- [x] mover propriedade do índice para `language-java::index`;
- [x] mover documentos analisados para `language-java::documents`;
- [x] mover o worker completo para `ide-language-host::worker`;
- [x] mover registro e roteamento completos para seus módulos;
- [x] deixar os arquivos raiz como fachadas públicas;
- [x] preservar a versão dos contratos ou documentar qualquer quebra.

Implementação concluída em 29/07/2026:

- `language-java::documents` passou a ser o proprietário de `Documents`, do
  parser e do ciclo de vida dos documentos analisados; `language-java::index`
  passou a possuir `WorkspaceIndex`, classes externas, declarações, referências
  e a varredura de workspace/JDK;
- a coordenação do provider Java foi isolada em `language-java::language`.
  O `lib.rs` de `language-java` foi reduzido a uma fachada pública de 12 linhas;
- `ide-language-host::worker` passou a possuir o worker, sua fila, thread,
  ativação, requisições assíncronas e encerramento. Registro, seleção,
  roteamento e metadados passaram para `registry` e `routing`;
- a coordenação do host foi isolada em `ide-language-host::host`. O `lib.rs` de
  `ide-language-host` foi reduzido a uma fachada pública de 10 linhas;
- as APIs públicas anteriores continuam reexportadas pelas fachadas. A versão
  do contrato de linguagem e a validação de `LANGUAGE_API_VERSION` foram
  preservadas, sem quebra para consumidores;
- o teste arquitetural
  `phase_seven_keeps_language_state_in_its_owning_modules` verifica a
  propriedade do estado, os limites das fachadas e impede a regressão para os
  arquivos raiz monolíticos;
- `cargo test --workspace`, Clippy estrito e o build release de `ide-app` foram
  executados sem falhas.

### Fase 8 — Validação final ✅ Concluída

- [x] registrar uma linguagem falsa sem alterar `ide-app`, `ide-ui`,
  controllers ou comandos;
- [x] executar uma tarefa falsa e exibir seu estado por modelos genéricos;
- [x] verificar ausência de nomes Java nas APIs neutras;
- [x] comparar grafo, fan-in, fan-out, linhas e campos com a linha de base;
- [x] executar testes, Clippy estrito e build release;
- [x] atualizar esta especificação com o estado efetivamente entregue.

Implementação concluída em 29/07/2026:

- o teste de integração `fake_language` registra uma contribuição fictícia
  somente pelos contratos públicos, ativa seu provider pelo
  `ide-language-host`, abre um documento `.fake`, executa `fake.run` pelo
  `TaskController` e entrega o `TaskExecutionResult` à shell;
- catálogo, tarefa, saída e estado da contribuição fictícia atravessam apenas
  `UiContributionCatalog`, `TaskDescriptor` e `TaskExecutionResult`. O teste
  executa a pintura da shell e confirma que `Run fake completed` está
  efetivamente visível, sem ramo, comando ou controller específico;
- o guardrail de APIs neutras passou a percorrer recursivamente todos os
  arquivos Rust de `ide-application`, `ide-ui` e `ide-workspace`, cobrindo
  funções, estruturas, enums, traits, aliases, constantes, módulos e reexports
  públicos. Não há API pública Java, JDK, JVM, Maven ou Gradle nessas crates;
- o teste arquitetural
  `phase_eight_preserves_the_final_architecture_metrics` fixa os limites finais
  do grafo e das estruturas centrais.

Comparação final com a linha de base:

| Métrica | Linha de base | Estado final |
|---|---:|---:|
| Crates / ciclos | 19 / 0 | 19 / 0 |
| Arestas internas | não registrada | 49 |
| Fan-in de `ide-domain` | 13 | 15 |
| Fan-out de `ide-app` | 16 | 17 |
| Dependências Java concretas diretas de `ide-app` | 5 | 5 |
| Campos de `NativeIde` | 29 | 9 |
| Campos de `IdeShell` | 83 | 10 |
| Linhas de `ide-app/src/main.rs` | ~2.400 | 15 |
| Linhas de `ide-ui/src/lib.rs` | ~9.100 | 30 |
| Linhas de `language-java/src/lib.rs` | ~2.250 | 12 |
| Linhas de `ide-language-host/src/lib.rs` | ~1.640 | 10 |

O aumento de uma dependência no fan-out de `ide-app` corresponde ao contrato
genérico `ide-application`; as cinco implementações Java permanecem restritas ao
composition root e são fiscalizadas pelo teste arquitetural. O aumento do
fan-in de `ide-domain` representa maior convergência das crates para o núcleo
neutro, sem dependências de saída no domínio.

Validação final:

- `cargo test --workspace`: 274 testes aprovados, nenhuma falha e 7 ignorados;
- `cargo clippy --workspace --all-targets -- -D warnings`: limpo;
- `cargo build --release -p ide-app`: concluído;
- grafo acíclico, limites estruturais e contratos falsos cobertos pelos testes
  arquiteturais ativos.

## Testes arquiteturais obrigatórios

Os testes devem falhar quando:

- uma crate de contrato depende de adapter concreto;
- `ide-ui`, `ide-application` ou `ide-workspace` expõe tipo público com nome
  Java, JDK, Maven ou Gradle;
- `ide-app` adiciona uma dependência concreta fora do catálogo de composição;
- uma feature da UI acessa o estado privado de outra;
- um controller recebe `NativeIde` inteiro;
- uma dependência usa caminho absoluto;
- uma contribuição omite `LanguageId` ou declara capacidades incompatíveis;
- surge um ciclo no grafo de crates.

Testes baseados apenas em busca textual não substituem compilação de contratos,
mas podem ser usados como guardrails para nomes e dependências proibidos.

## Critérios de conclusão de cada fase

Uma fase somente pode ser marcada como concluída quando:

- o comportamento visível anterior permanece coberto;
- `cargo test --workspace` não apresenta falhas;
- `cargo clippy --workspace --all-targets -- -D warnings` está limpo;
- `cargo build --release -p ide-app` é concluído;
- os testes arquiteturais da fase estão ativos;
- nenhuma implementação é duplicada para manter compatibilidade temporária;
- a documentação descreve o código entregue, não apenas o estado desejado.

## Fora do escopo

- implementar imediatamente carregamento dinâmico de plugins;
- criar suporte funcional completo a uma segunda linguagem;
- alterar o protocolo de depuração Java;
- substituir ERLibUi, Winit ou WGPU;
- mudar o comportamento visual como efeito colateral da refatoração;
- aumentar a quantidade de crates sem uma nova fronteira de distribuição,
  isolamento ou substituição.

## Estratégia de reversão

Cada fase deve ser independente. Primeiro são adicionados contratos e testes;
depois consumidores migram; somente então campos, métodos e APIs antigas são
removidos.

Não devem permanecer bridges permanentes entre o estado Java antigo e os
registros novos. Se uma fase revelar que o contrato é insuficiente, ela deve ser
revertida antes da remoção da implementação anterior.
