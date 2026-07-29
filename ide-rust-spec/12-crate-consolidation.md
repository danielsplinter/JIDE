# 12 — Consolidação de crates e módulos

## Objetivo

Reduzir a fragmentação física do workspace sem remover as fronteiras que
protegem o domínio, os contratos públicos e os adapters substituíveis.

A quantidade de linhas não decide sozinha se algo deve ser uma crate. A regra é:

- **crate** para uma fronteira de dependência, substituição, isolamento ou
  distribuição independente;
- **módulo** para responsabilidades que são compiladas, versionadas e alteradas
  como uma única unidade.

Esta refatoração não pode alterar comportamento visível da IDE. Cada fase deve
preservar testes, Clippy sem warnings e a geração do executável release.

## Diagnóstico

O workspace inicial possui 23 crates e não apresenta ciclos internos. As
fronteiras de domínio, linguagem, toolchain, depuração e processos são úteis.
Entretanto, algumas crates pequenas são usadas sempre em conjunto ou sequer
estão conectadas à aplicação, enquanto arquivos centrais concentram
responsabilidades que deveriam ser módulos internos.

Problemas a corrigir:

- `ide-commands` e `ide-events` são abstrações de aplicação separadas e ainda
  não participam do fluxo real de `ide-app`;
- `ide-text` e `ide-workspace` compartilhavam a responsabilidade por documentos
  e filesystem;
- `ide-build-api` dependia integralmente de `ide-project-model`;
- `java-javac-adapter` dependia da seleção, classpath e instalação mantidos por
  `java-toolchain`;
- arquivos grandes como `ide-ui/src/lib.rs`, `ide-app/src/main.rs`,
  `language-java/src/lib.rs` e `ide-language-host/src/lib.rs` precisam de
  módulos, não de novas crates;
- contratos genéricos ainda contêm conceitos de JDK e JVM; a consolidação não
  deve perpetuar esse acoplamento.

## Estado-alvo

```text
crates/
  ide-app
  ide-application
  ide-core
  ide-domain
  ide-language-api
  ide-language-host
  ide-toolchain-api
  ide-debug-api
  ide-project
  ide-process
  ide-terminal
  ide-workspace
  ide-ui

  language-java
  java-toolchain
  java-maven-adapter
  java-gradle-adapter
  java-debug-adapter
```

`java-classfile` poderá permanecer independente se passar a ser consumida por
mais de um adapter. Enquanto tiver somente o provider Java como consumidor, sua
incorporação em `language-java::classfile` será avaliada na fase correspondente.

## Fronteiras que devem permanecer como crates

- `ide-domain`: não depende de filesystem, runtime, UI ou linguagem concreta;
- `ide-language-api`: providers dependem do contrato sem carregar o host;
- `ide-language-host`: possui registro, roteamento, workers e ciclo de vida;
- `ide-toolchain-api`: contrato neutro, separado do JDK;
- `ide-debug-api`: contrato neutro, separado do protocolo Java;
- `ide-process`: porta e implementação de supervisão de processos;
- `ide-terminal`: PTY e processos interativos com ciclo de vida próprio;
- `ide-ui`: fronteira de apresentação com a ERLibUi;
- `ide-app`: executável e composition root;
- adapters Maven, Gradle e depuração Java: integrações externas independentes.

Não são permitidas as seguintes fusões:

- contrato de linguagem com `language-java`;
- contrato de toolchain com `java-toolchain`;
- contrato de depuração com `java-debug-adapter`;
- domínio com UI, filesystem ou adapters;
- `ide-ui` com `ide-app`.

## Plano incremental

### Fase 1 — Camada de aplicação ✅ Concluída

**Estado: concluída em 29/07/2026.**

A crate `ide-application` passou a concentrar os comandos e eventos da
aplicação. `ide-commands` e `ide-events` foram consolidadas como seus módulos e
as crates antigas foram removidas. A `IdeShell` agora expõe uma única fila
ordenada de `ApplicationCommand`, consumida por um dispatcher central em
`ide-app`; os antigos canais paralelos `take_*` ficaram fora do código de
produção. Eventos de workspace, importação e ciclo de vida de documentos são
publicados pelos casos de uso reais com payloads tipados.

- [x] criar `ide-application`;
- [x] mover o registro de comandos para `ide_application::commands`;
- [x] mover o barramento de eventos para `ide_application::events`;
- [x] remover `ide-commands` e `ide-events` do workspace;
- [x] remover as dependências não utilizadas de `ide-app`;
- [x] substituir as flags `take_*` por uma fila ordenada de comandos tipados;
- [x] ligar os eventos tipados aos casos de uso reais.

Validação da fase: testes direcionados de `ide-application`, `ide-ui` e
`ide-app`, `cargo test --workspace`, Clippy estrito sem warnings e build
release de `ide-app`.

### Fase 2 — Projeto e build ✅ Concluída

**Estado: concluída em 29/07/2026.**

O modelo neutro e os contratos de build foram consolidados em `ide-project`,
mantendo módulos públicos separados. Maven, Gradle e o composition root
dependem agora de uma única fronteira de projeto, sem perder a possibilidade de
substituir os adapters externos.

- [x] criar `ide-project`;
- [x] mover `ide-project-model` para `ide_project::model`;
- [x] mover `ide-build-api` e o registry para `ide_project::build`;
- [x] atualizar Maven, Gradle e `ide-app`;
- [x] remover as crates substituídas.

Validação da fase: testes direcionados de `ide-project`,
`java-maven-adapter`, `java-gradle-adapter` e `ide-app`,
`cargo test --workspace`, Clippy estrito sem warnings e build release de
`ide-app`.

### Fase 3 — Documentos e workspace ✅ Concluída

**Estado: concluída em 29/07/2026.**

`ide-workspace` concentra agora documentos, árvore, busca e filesystem em
módulos separados. `TextBuffer` e `EditorSession` são estruturas puras: recebem
conteúdo e confirmação de gravação, mas não acessam o disco. A aplicação define
`WorkspacePort`; `NativeWorkspaceFileSystem` implementa essa porta e é injetado
no `WorkspaceService` pelo composition root.

A `IdeShell` deixou de varrer, abrir ou gravar arquivos. Ela emite
`OpenDocument`, `SaveDocument` e `ReloadWorkspace`, recebe árvores e conteúdos
carregados e só confirma uma gravação quando a revisão enviada ainda é a atual.

- [x] manter `TextBuffer` livre de I/O;
- [x] mover sessão de documentos, árvore, busca e filesystem para módulos de
  `ide-workspace`;
- [x] retirar `FileNode::scan`, abertura e gravação direta de `IdeShell`;
- [x] injetar uma porta de workspace na camada de aplicação;
- [x] remover `ide-text` quando nenhum consumidor externo permanecer.

Validação da fase: testes direcionados de `ide-application`, `ide-workspace`,
`ide-ui` e `ide-app`, `cargo test --workspace`, Clippy estrito sem warnings e
build release de `ide-app`.

### Fase 4 — Toolchain Java ✅ Concluída

**Estado: concluída em 29/07/2026.**

`java-toolchain` reúne agora `detection`, `selection`, `classpath` e `adapter`
como módulos com APIs mínimas. O adapter implementa compilação com `javac`,
execução com `java` e o ciclo de testes, preservando os contratos neutros de
`ide-toolchain-api`.

No `ide-app`, os casos de uso guardam os adapters como
`Arc<dyn CompilerAdapter>`, `Arc<dyn RuntimeAdapter>` e
`Arc<dyn TestAdapter>`. O `JavaToolchainAdapter` concreto aparece somente no
composition root.

- [x] mover detecção, seleção e classpath para módulos de `java-toolchain`;
- [x] mover javac, runtime e testes de `java-javac-adapter`;
- [x] fazer `ide-app` depender de adapters pelos contratos;
- [x] remover `java-javac-adapter`.

Validação da fase: testes direcionados de `java-toolchain` e `ide-app`,
`cargo test --workspace`, Clippy estrito sem warnings e build release de
`ide-app`.

### Fase 5 — Modularização interna ✅ Concluída

**Estado: concluída em 29/07/2026.**

Os arquivos centrais permanecem como orquestradores, enquanto estado e
operações coesas passaram a módulos internos:

- `ide-ui`: `shell` possui foco e fila de comandos; `terminal` possui abas,
  seleção e rolagem; `search`, `settings` e `debugging` possuem seus modelos;
  `explorer`, `menus` e `layout` concentram respectivamente a árvore, a criação
  de menus e a geometria; `editor` continua sendo a fronteira do editor;
- `ide-app`: `bootstrap` monta e inicia a aplicação, `window` possui o estado de
  interação da janela e `bridges` traduz mudanças de documento e eventos de
  ferramentas; os módulos existentes `run` e `debug` preservam seus casos de
  uso;
- `language-java`: `parser` encapsula o parser mutável; `index`, `symbols`,
  `semantics`, `completion` e `navigation` expõem somente as operações de cada
  etapa da linguagem;
- `ide-language-host`: `registry` possui providers, seleções e rotas;
  `routing` normaliza e resolve o contexto das requisições; `worker` define o
  protocolo isolado enviado aos workers.

Nenhum desses módulos recebe a estrutura central inteira. Eles possuem estado
mínimo ou funções sobre entradas explícitas e deixam aos arquivos raiz somente
a coordenação entre fronteiras.

- [x] dividir `ide-ui` em shell, editor, Explorer, terminal, pesquisa,
  configurações, depuração, menus e layout;
- [x] dividir `ide-app` em bootstrap, janela e bridges de aplicação;
- [x] dividir `language-java` em parser, índice, símbolos, semântica,
  completação e navegação;
- [x] dividir `ide-language-host` em registry, routing e worker.

Validação da fase: testes direcionados de `ide-ui`, `ide-app`,
`language-java` e `ide-language-host`, `cargo test --workspace`, Clippy
estrito sem warnings e build release de `ide-app`.

### Fase 6 — Neutralidade dos contratos

- [ ] substituir `jdk_home` no contexto de linguagem por configuração de
  toolchain associada a `LanguageId`;
- [ ] remover `java_home` dos contratos genéricos de build;
- [ ] retirar `main_class`, `jvm_args`, `source_level`, `target_level` e
  `test_classes` dos contratos genéricos;
- [ ] mover análise de acesso por ponto para providers de linguagem;
- [ ] usar source roots do `ProjectModel` como escopo de busca.

### Fase 7 — Portabilidade e fiscalização

- [ ] substituir caminhos absolutos da ERLibUi por dependências portáveis e
  versionadas;
- [ ] adicionar testes arquiteturais para dependências proibidas;
- [ ] verificar que uma linguagem falsa pode ser registrada sem modificar
  `ide-app` ou `ide-ui`;
- [ ] verificar ausência de ciclos no grafo de crates.

## Critérios de conclusão de cada fase

Uma fase somente pode ser marcada como concluída quando:

- `cargo test --workspace` não apresenta falhas;
- `cargo clippy --workspace --all-targets -- -D warnings` está limpo;
- `cargo build --release -p ide-app` é concluído;
- não existem duas crates mantendo cópias da mesma implementação;
- o número de dependências concretas de `ide-app` não aumenta;
- a documentação e o índice refletem o estado efetivamente entregue.

## Estratégia de reversão

Cada fase deve formar uma mudança independente. Movimentações preservam testes
junto do código e não misturam alteração comportamental com renomeação em massa.
Se uma fase revelar dependência oculta, ela deve ser interrompida antes da
remoção das crates antigas; não serão mantidos adaptadores de compatibilidade
permanentes apenas para sustentar a estrutura anterior.
