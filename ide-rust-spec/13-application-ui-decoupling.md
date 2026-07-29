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

### Fase 4 — UI neutra

- [ ] substituir `NewItemKind` fechado por templates registrados;
- [ ] substituir configurações de JDK por seções genéricas;
- [ ] substituir textos fixos Java por modelos fornecidos;
- [ ] tornar busca de conteúdo neutra e dirigida por `SearchScope`;
- [ ] gerar menus e botões de tarefas a partir de descritores;
- [ ] manter todos os fluxos Java atuais sem regressão visual.

### Fase 5 — Decomposição de `IdeShell`

- [ ] transferir estado e comportamento do Explorer para `ExplorerState`;
- [ ] transferir terminais e seleção para `TerminalPanelState`;
- [ ] transferir pesquisas e modais para `SearchState`;
- [ ] transferir configurações para `SettingsState`;
- [ ] transferir depuração e inspeção para `DebugPanelState`;
- [ ] reduzir `IdeShell` e `ide-ui/src/lib.rs` às metas estruturais;
- [ ] proibir referências cruzadas diretas entre estados de feature.

### Fase 6 — Decomposição de `NativeIde`

- [ ] criar controllers com dependências explícitas;
- [ ] mover documentos e sincronização de linguagem para controllers próprios;
- [ ] mover projeto, build e tarefas para controllers próprios;
- [ ] isolar tradução entre `ApplicationCommand` e casos de uso em `UiBridge`;
- [ ] reduzir `NativeIde` e `ide-app/src/main.rs` às metas estruturais;
- [ ] testar controllers sem criar janela nativa.

### Fase 7 — Fachadas de linguagem e host

- [ ] mover propriedade do índice para `language-java::index`;
- [ ] mover documentos analisados para `language-java::documents`;
- [ ] mover o worker completo para `ide-language-host::worker`;
- [ ] mover registro e roteamento completos para seus módulos;
- [ ] deixar os arquivos raiz como fachadas públicas;
- [ ] preservar a versão dos contratos ou documentar qualquer quebra.

### Fase 8 — Validação final

- [ ] registrar uma linguagem falsa sem alterar `ide-app`, `ide-ui`,
  controllers ou comandos;
- [ ] executar uma tarefa falsa e exibir seu estado por modelos genéricos;
- [ ] verificar ausência de nomes Java nas APIs neutras;
- [ ] comparar grafo, fan-in, fan-out, linhas e campos com a linha de base;
- [ ] executar testes, Clippy estrito e build release;
- [ ] atualizar esta especificação com o estado efetivamente entregue.

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
