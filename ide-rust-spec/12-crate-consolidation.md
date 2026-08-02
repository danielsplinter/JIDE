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
```

**Uma crate por linguagem.** Tudo o que é exclusivo de uma linguagem — analisador,
toolchain, build e depuração — mora numa crate só, com módulos dentro. `language-java`
absorve `java-classfile`, `java-toolchain`, `java-maven-adapter`,
`java-gradle-adapter` e `java-debug-adapter` na fase 8, e `language-typescript`,
`language-angular` e qualquer C# ou C++ que venham nascem já assim.

**A avaliação do `java-classfile` foi feita, e a resposta é incorporar.** Este
documento dizia que ele "poderá permanecer independente se passar a ser consumido
por mais de um adapter"; o levantamento de 02/08/2026 mostra um consumidor só.

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
- **uma crate por linguagem**, e a fronteira é entre linguagens, não dentro de
  cada uma. `language-java` não pode fundir-se com `language-typescript`, nem
  qualquer das duas com uma crate neutra.

> **Revisão de 02/08/2026.** Esta lista dizia "adapters Maven, Gradle e depuração
> Java: integrações externas independentes", e tratava cada adapter como fronteira
> própria. A fase 8 desfaz isso. "Independentes" descrevia a relação deles com a
> **IDE**, não entre si — e essa independência quem entrega é o
> `BuildSystemAdapter`, não a fronteira de crate. O critério que passa a valer é o
> da **linguagem**, porque é ele que não explode quando a quarta linguagem entrar.

Não são permitidas as seguintes fusões:

- qualquer contrato com a implementação que o realiza — o de linguagem com
  `language-java`, o de toolchain com o JDK, o de depuração com o JDWP;
- duas linguagens na mesma crate;
- domínio com UI, filesystem ou adapters;
- `ide-ui` com `ide-app`;
- os contratos entre si. `ide-language-api`, `ide-toolchain-api` e `ide-debug-api`
  somam 710 linhas e a tentação de uni-los é óbvia — mas uma linguagem que só
  depure, sem analisar, carregaria as três portas. Tamanho pequeno não é motivo
  para unir;
- `ide-process` com `ide-domain`, que colocaria criação de processo dentro do
  domínio.

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

### Fase 6 — Neutralidade dos contratos ✅ Concluída

**Estado: concluída em 29/07/2026.**

O contexto de linguagem passou a transportar `LanguageToolchainConfig`
associada por `LanguageId`, além das `source_roots` do modelo. O host mantém
essas configurações sem conhecer JDK, e o provider Java interpreta somente a
entrada da linguagem `java`.

As requisições genéricas de build agora recebem um mapa de ambiente; `JAVA_HOME`
é preenchido apenas na integração Java. Os contratos de toolchain usam
`entry_point`, `runtime_args`, `additional_args` e `targets`, retirando nomes e
opções próprios da JVM.

A análise de acesso por ponto saiu de `ide-domain` e de `ide-ui`: o host a
roteia pelo documento e `language-java::navigation` implementa a sintaxe Java.
As buscas por tipos e conteúdo usam as `source_roots` de `ProjectModel`; o
fallback estrutural existe somente antes de um modelo ser importado.

- [x] substituir `jdk_home` no contexto de linguagem por configuração de
  toolchain associada a `LanguageId`;
- [x] remover `java_home` dos contratos genéricos de build;
- [x] retirar `main_class`, `jvm_args`, `source_level`, `target_level` e
  `test_classes` dos contratos genéricos;
- [x] mover análise de acesso por ponto para providers de linguagem;
- [x] usar source roots do `ProjectModel` como escopo de busca.

Validação da fase: testes direcionados dos contratos e consumidores afetados,
`cargo test --workspace`, Clippy estrito sem warnings e build release de
`ide-app`.

### Fase 7 — Portabilidade e fiscalização ✅ Concluída

**Estado: concluída em 29/07/2026.**

As dependências da ERLibUi foram centralizadas em `workspace.dependencies`.
Cada uma declara a versão compatível `0.1.0` e um caminho relativo ao
repositório irmão `../ERLibUi`; `ide-app` e `ide-ui` apenas herdam essas
entradas. Assim, nenhum manifest contém usuário, unidade ou diretório absoluto.
Para desenvolvimento conjunto, os repositórios `ide` e `ERLibUi` devem ser
checados lado a lado; a versão explícita impede o uso acidental de uma revisão
com versão incompatível.

Testes arquiteturais em `ide-core/tests/architecture.rs` leem os manifests
reais e falham quando uma dependência usa caminho absoluto, uma crate protegida
atravessa uma fronteira proibida ou o grafo interno contém ciclo. O grafo inclui
dependências de desenvolvimento, build e condicionais por plataforma.

O host possui ainda um teste ponta a ponta que registra o provider fictício
`fake.native`, roteia a extensão `.fake`, ativa o worker e abre um documento.
O teste vive somente em `ide-language-host`, demonstrando que uma linguagem
nova não exige alteração em `ide-app` nem `ide-ui`.

- [x] substituir caminhos absolutos da ERLibUi por dependências portáveis e
  versionadas;
- [x] adicionar testes arquiteturais para dependências proibidas;
- [x] verificar que uma linguagem falsa pode ser registrada sem modificar
  `ide-app` ou `ide-ui`;
- [x] verificar ausência de ciclos no grafo de crates.

Validação da fase: testes arquiteturais e do provider falso,
`cargo test --workspace`, Clippy estrito sem warnings e build release de
`ide-app`.

### Fase 8 — Uma crate por linguagem, e Java se ajusta ao padrão ✅ Concluída

**Estado: concluída em 02/08/2026.** Levantamento sobre 19 crates; o workspace
terminou com 14.

#### Por que o critério muda

As fases 1 a 7 consolidaram por **camada**. Esta consolida por **linguagem**, e a
razão é que o critério antigo não sobrevive à terceira linguagem.

Java está espalhado em seis crates. Isso não foi decisão arquitetural — é resíduo
de ter sido a primeira, construída por partes. Extrapolado, o formato é insustentável:

```text
hoje, extrapolado:   6 crates × 4 linguagens = 24
uma por linguagem:   1 crate  × 4 linguagens =  4
```

E o que multiplica é só a **implementação**. Os contratos não: `ide-language-api`,
`ide-toolchain-api` e `ide-debug-api` continuam sendo três, com dez linguagens ou
com uma. A confusão que se evita está exatamente onde é evitável.

O argumento decisivo, porém, não é a aritmética: **o formato de uma crate por
assunto já está decidido para tudo o que vem depois.** `language-typescript` é uma
(`23`), `language-angular` é uma (`24`), `ide-git` é uma (ADR-024). Java ser seis
faz do padrão do projeto uma exceção — e a exceção é a parte mais antiga e maior,
que é a pior combinação para quem chega.

#### O estado de partida

| crate | linhas | dependências próprias | consumidores |
|---|---|---|---|
| `language-java` | 6.437 | tree-sitter, notify, java-classfile | `ide-app` |
| `java-debug-adapter` | 3.128 | tokio (com `net`), tracing | `ide-app` |
| `java-maven-adapter` | 1.503 | ide-process, ide-project | `ide-app` |
| `java-toolchain` | 850 | ide-process, ide-toolchain-api | `ide-app` |
| `java-gradle-adapter` | 778 | ide-process, ide-project | `ide-app` |
| `java-classfile` | 354 | zip | `language-java` |

Cinco das seis têm **um consumidor só**, e ele é a raiz de composição.

#### O formato

```text
language-java/src/
├── lib.rs
├── analyzer/          tree-sitter, índice, símbolos, completação
│   └── classfile/     leitura de `.class` e de `.jar`
├── toolchain/         detecção de JDK, seleção, classpath
├── build/             javac, maven, gradle
└── debug/             JDWP
```

E o mesmo desenho para toda linguagem que vier. A raiz de composição continua
montando a `LanguageContribution`, como a fase 4 deixou: os adapters concretos
aparecem só lá, guardados como `Arc<dyn CompilerAdapter>` e companhia.

#### O que se perde, e como se recupera

**Hoje o compilador garante que o analisador não dispara processo.**
`language-java` depende apenas de `ide-domain`, `ide-language-api` e
`java-classfile` — ele não alcança `ide-process` nem `ide-project`, e portanto não
pode executar um comando nem ler o modelo de projeto. Recebe as raízes de fonte
pelo `LanguageActivationContext`, e nada mais.

Numa crate única isso some. `pub(crate)` protege o lado de fora do lado de dentro,
e **não particiona o lado de dentro**: o índice passaria a poder chamar o Maven, e
nada avisaria.

A garantia é recuperada por guarda, e não por tipo:

> nenhum arquivo em `language-java/src/analyzer/` menciona `ide_process` ou
> `ide_project`.

É mais fraca — texto contra compilador —, e vale dizer isso sem enfeite. Mas o
projeto já confia em guardas de texto para invariantes que considera importantes:
a que proíbe `Command::new("git")` fora do `ide-git`, a que proíbe decidir por
nome de arquivo na `24`, a que conta os campos do `IdeShell`. Uma a mais é
coerente, e ela falha no mesmo commit em que alguém escrever a linha.

**Vinte crates evitadas por uma guarda de texto** é a troca, e ela está sendo
feita de olhos abertos.

#### Duas objeções examinadas

**A largura de dependências.** A crate resultante depende de tree-sitter, notify,
zip, tokio com rede, tracing, `ide-process`, `ide-project`, `ide-toolchain-api`,
`ide-debug-api`, `ide-language-api` e `ide-domain`. Parece caro e não é: `tokio`
já está no workspace, e a unificação de features do Cargo já entrega `net` e
`io-util` ao build inteiro por causa dele. **Nenhuma dependência nova entra no
grafo** — o que muda é a superfície dentro de uma crate, que é o que a guarda
acima trata.

**A compilação.** Uma crate de ~13 mil linhas recompila inteira a cada mudança,
contra seis que recompilam em paralelo. **Não foi medido.** Linguagem madura muda
pouco, e não se deixaria isso decidir — mas se o ciclo de edição piorar, é aqui
que estará a causa, e o caminho de volta é extrair de novo o módulo que doer.

#### O que fazer

- [x] mover `java-classfile` para `language-java::analyzer::classfile`;
- [x] mover `java-toolchain` para `language-java::toolchain`;
- [x] mover Maven e Gradle para `language-java::build`;
- [x] mover `java-debug-adapter` para `language-java::debug`;
- [x] reduzir a superfície pública ao que `ide-app` de fato usa;
- [x] remover as cinco crates absorvidas;
- [x] escrever a guarda do `analyzer/`, e **verificar que ela falha** ao se
      acrescentar de propósito uma menção a `ide_process`;
- [x] substituir `concrete_java_crates_stay_behind_the_composition_root` por uma
      guarda que fale de linguagens, e não de Java;
- [x] conferir que `ide-app` continua guardando os adapters pelos contratos.

De 19 crates para 14, e a quarta linguagem passa a custar **uma**.

#### Feita, e o que ela revelou

**A previsão de que só falhariam testes que nomeiam crates se confirmou.** Das
suítes inteiras, quatro guardas de arquitetura falharam e nenhum outro teste — os
179 de `language-java`, os 82 de `ide-ui` e o resto passaram sem uma linha
alterada.

**Os testes de integração precisam de mudança de endereço, e quase se perderam.**
As cinco crates absorvidas tinham `src/`, e duas delas também `tests/` — o que
some junto quando se remove o diretório da crate. Os sete testes do adapter de
depuração foram para `language-java/tests/`, com prefixo `debug_` para dizerem de
qual módulo falam. É a armadilha óbvia desta fase em retrospecto, e não estava na
lista de tarefas.

**A união encontrou código morto que a fronteira de crate escondia.**
`JavaToolchainSelection`, com 125 linhas e dois testes próprios, não era usado por
ninguém — mas era `pub` numa crate, e "público" e "usado" não se distinguem de
fora. Ao encolher a superfície para o que `ide-app` de fato consome, ele apareceu
como o que era. Foi removido; está no histórico se voltar a ser necessário.

Vale como argumento retroativo a favor da fase: **crate demais esconde sobra**.

**Uma guarda tinha a forma errada, e só a consolidação mostrou.**
`phase_eight_preserves_the_final_architecture_metrics` afirmava
`domain_fan_in >= 13` — quantas crates convergem para `ide-domain`. É um limite
**inferior absoluto**, e ele caiu para 11 sozinho quando cinco crates viraram
módulos, sem que nada tivesse deixado de convergir. Na verdade a proporção
*melhorou*: de 13 em 19 para 11 em 14.

As outras métricas do mesmo teste são limites superiores, que sobrevivem a
consolidação; esta era a única invertida, e ninguém tinha reparado porque o
workspace nunca havia encolhido. Virou proporcional — a maioria das crates fala o
vocabulário do domínio — e agora não precisa ser tocada quando o número de crates
mudar de novo.

**Os tetos que se moveram**, todos com a razão escrita ao lado no próprio teste:
crates de 19 para 14, arestas do grafo interno de 49 para 40, fan-out do `ide-app`
de 17 para 13, e a fachada de `language-java` de 13 para 18 linhas — ela agora
declara quatro módulos e reexporta o que a raiz consome de cada um, e continua
sendo só `mod` e `pub use`.

Validação: `cargo test --workspace` sem falhas, `cargo clippy --workspace
--all-targets -- -D warnings` limpo, `cargo build --release -p ide-app` concluído.
E a guarda do analisador foi vista **falhar**: acrescentar `use ide_process::…` em
`analyzer/parser.rs` a quebra, e removê-lo a devolve ao verde.

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
