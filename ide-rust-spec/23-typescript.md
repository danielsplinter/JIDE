# 23 — TypeScript

## Situação

A IDE diz ser multilíngue desde a `00`, e nunca teve duas linguagens. Java é a
única, e uma afirmação com um exemplo só não foi verificada — foi escrita.

Esta especificação é a primeira vez que a afirmação é cobrada. O que ela entrega
de fato não é TypeScript: é a **prova de que o desenho aguenta a segunda
linguagem**, com TypeScript no papel de quem faz a pergunta. Toda a parte cara
está nessa prova, e ela começa por um levantamento incômodo.

Angular fica **fora** daqui, na `24`. Ele não é uma linguagem, e o que ele pede
entra por portas que esta especificação não precisa abrir — a `24` explica por
quê, e depende desta.

## Onde a IDE sabe de Java hoje

O `02` diz que o domínio não depende de Java, e a `ide-core/tests/architecture.rs`
tem uma guarda para isso. Mesmo assim:

**A configuração conhece JDK e Maven pelo nome.** Em
[`ide-core/src/lib.rs:44`](../crates/ide-core/src/lib.rs), `ToolchainConfig` tem
dois campos, `jdk_home` e `maven_home`, com `resolved_jdk_home`,
`resolved_maven_home` e `remember_jdk` em volta. É a configuração persistida da
IDE inteira, e ela tem o vocabulário de uma linguagem só. Node exigiria um
terceiro campo, e a linguagem seguinte um quarto — o formato cresce com o número
de linguagens, que é a definição de não ser neutro.

**Os comandos da tela de configurações não dizem de qual linguagem falam.**
`BrowseToolchain`, `SelectToolchain(usize)`, `SelectSecondaryTool(usize)` e
`BrowseSecondaryTool`, em
[`ide-application/src/commands.rs:25`](../crates/ide-application/src/commands.rs),
não carregam `LanguageId`. Funcionam porque só existe uma seção. Com duas, o
comando não tem como dizer qual foi clicada.

**A aplicação liga o botão genérico direto no Maven.** Em
[`ide-app/src/native_ide.rs:1563`](../crates/ide-app/src/native_ide.rs),
`UiAction::BrowseSecondaryTool` chama `choose_maven_home`. O contrato em
`contributions.rs` está certo — `SettingsSection::secondary_caption` é neutro e o
comentário lá até prevê "em outra linguagem será outra coisa" —, e é a fiação que
curto-circuita.

**O contexto de execução de tarefa fala JVM.** `TaskExecutionContext` tem
`classpath_entries` em
[`ide-application/src/contributions.rs:44`](../crates/ide-application/src/contributions.rs).
*Classpath* é vocabulário da JVM. TypeScript não tem um, e teria de preencher o
campo com nada ou com outra coisa disfarçada.

**E a guarda tem um furo exatamente onde está o vazamento.**
`neutral_crates_expose_no_language_specific_public_api` procura por `java`,
`jdk`, `jvm`, `maven` e `gradle` — mas só em `ide-application`, `ide-ui` e
`ide-workspace`. **`ide-core` não está na lista**, e é onde `jdk_home` mora. Além
disso ela examina linhas de declaração de item público, e um campo de struct não
é uma delas: `pub jdk_home: Option<PathBuf>` passaria mesmo se a crate estivesse
listada.

Nada disso é acidente moral de quem escreveu — é o que acontece com toda regra
que nunca foi exercida. **Uma abstração com uma implementação só é uma hipótese.**
A segunda linguagem é o teste, e por isso ela vem antes de qualquer sintaxe nova
nas fases desta especificação.

Há ainda um resíduo menor, de comentário: em
[`ide-language-host/src/host.rs:107`](../crates/ide-language-host/src/host.rs), a
linha "JDK escolhido na IDE, repassado a cada ativação" está sobre o campo
`source_roots`, e o campo que ela descreve é genérico
(`HashMap<LanguageId, LanguageToolchainConfig>`). O host está certo; o comentário
envelheceu. Some junto.

## Quem responde por cada extensão

```text
.ts   → typescript.syntax   (nativo, tree-sitter)
        typescript.service  (externo, quando houver Node)
.css  → style.basic         (realce e estrutura, nada mais)
.js   → nada
.json → nada
```

**CSS ganha o mínimo, e é uma decisão e não um esquecimento.** Realce e estrutura
cobrem o que se faz com CSS dentro de uma IDE de aplicação; completação de
propriedade e resolução de seletor são um projeto próprio.

**Feito ✅, e `.scss` entrou junto** — porque é o que um projeto Angular usa de
verdade: o `angular.json` do projeto de referência declara `"style": "scss"`, e
ele tem 37 arquivos `.scss` e **zero** `.css`. Um provider só de CSS não
atenderia arquivo nenhum.

A gramática é a de CSS: a de SCSS não compila com o MSVC daqui. Medido, ela erra
**quatro nós de sessenta e dois** numa amostra pequena de SCSS — `$cor`, `&:hover`
e `@mixin` viram erro num arquivo correto. Por isso o **diagnóstico sai só para
`.css`**: realçar quase tudo é útil, acusar o que não se entende é mentira. É a
mesma regra que a `24` fixa para o template.

**JavaScript fica de fora por honestidade.** O provider de TypeScript leria `.js`
mal e sem tipo nenhum. Um arquivo sem provider abre como texto, que é uma resposta
correta — melhor do que meia resposta com cara de resposta inteira.

**TypeScript tem dois providers, como Java já tem.** A `04` descreve isso pelo
nome — provider principal, `fallback_provider` —, e o exemplo dela é justamente
`native-java-analyzer` com `jdtls-adapter` atrás. TypeScript usa a mesma mecânica,
e a seção seguinte explica por que precisa dela mais do que Java.

## O analisador: o que cabe em Rust, e o que não cabe

O sistema de tipos do TypeScript é grande de um jeito que não se resolve com
esforço. Tipos condicionais, mapeados, literais de template, inferência de
genéricos, estreitamento por fluxo de controle — reimplementar isso em Rust não é
uma fase de trabalho, é um projeto do tamanho da IDE. **Não será feito.**

O que cabe, e cabe bem, é o mesmo que o provider Java faz: tree-sitter para
sintaxe, e uma camada semântica modesta por cima — símbolos declarados, `import`
e `export`, ir para a definição pelo nome, referências, estrutura do arquivo. Sem
verificação de tipo. É útil, é rápido, e não precisa de nada instalado.

O resto — completação com o tipo certo, diagnóstico real, renomeação segura —
vem do analisador do próprio TypeScript.

### Ele não é uma escolha nossa: é o que o projeto fixa

A porta não é "adapter do `tsserver`". É **o analisador que o projeto declara**,
resolvido na abertura, com mais de um adapter atrás:

```text
porta: análise de TypeScript
├── tsserver   protocolo próprio, sobre Node — qualquer versão que o projeto tenha
└── tsgo       LSP nativo, sem Node — só quando o projeto já estiver nele
```

É a composição de capacidades da `04` aplicada um nível abaixo. **Só um adapter
é construído: o `tsserver`.** O `tsgo` está no desenho apenas para que a porta não
nasça com o nome da ferramenta — não há trabalho previsto para ele, e a ADR-028
explica por que não haverá tão cedo.

O que torna isso barato é o protocolo do `tsserver` ser **estável** ao longo das
versões que interessam. Um adapter só atende TypeScript 4.x e 5.x sem ramificar.
O que muda entre projetos é qual arquivo se executa, não como se conversa com ele.

### Nenhuma tabela de compatibilidade no nosso código

Esta é uma regra, e é a que impede a IDE de envelhecer a cada lançamento de
terceiro.

Existem correspondências reais no mundo — cada versão do Angular fixa uma faixa de
TypeScript, cada versão do Angular exige uma faixa de Node. **Nenhuma delas pode
ser escrita aqui dentro.** Uma tabela dessas nasce certa, é copiada de um blog, e
está errada no lançamento seguinte — e errada em silêncio, que é como a `21`
descreve o pior tipo de defeito.

O padrão é sempre o mesmo: **não validar; delegar e relatar.**

- qual TypeScript usar? o que está no `node_modules`. Não perguntamos qual Angular
  é para deduzir qual TypeScript deveria ser;
- o Node serve para rodar a CLI? **a CLI que responda.** Ela já checa a própria
  exigência e falha com mensagem clara; o nosso trabalho é mostrar essa mensagem,
  e não antecipá-la com uma faixa nossa;
- a versão do plugin do Angular? a do projeto, pelo mesmo caminho.

A IDE não sabe que existe Angular 11 ou Angular 19, e não sabe que Node 12 ou
Node 20 existem. Ela sabe executar o que o projeto aponta e mostrar o que voltou.
Os números que aparecem nesta especificação e na ADR-028 são **ilustração de um
argumento**, e não dado que o código consulta.

### O que exatamente se executa, e como se fala com ele

Duas dependências separadas, e confundi-las leva a erro de diagnóstico:

```text
Node instalado      → executa JavaScript
pacote typescript   → fornece o node_modules/typescript/lib/tsserver.js
```

O `tsserver` **não vem com o Node**. Ele vem com o pacote `typescript`, que num
projeto normal é `devDependency` e está no `node_modules` — que é de onde o
adapter o pega, pela regra da versão. Faltando qualquer um dos dois, o provider
externo não sobe e o nativo responde.

E a IDE **não se conecta** a ele. Não há porta, socket nem serviço escutando: é um
**processo filho**, com JSON delimitado por linha indo e voltando por
stdin/stdout. A distinção decide qual maquinário se usa:

| | forma | contrato |
|---|---|---|
| depuração (JDWP, CDP) | `host` + `port`, alvo já em execução | `DebugAdapter`, da `03` |
| analisador | processo filho, stdio, longevo | `ProcessSupervisor`, da `02` |

É o mesmo supervisor que roda `javac` e Maven, com um ciclo de vida diferente:
aqueles executam, respondem e morrem; este vive junto com a IDE e mantém estado
entre pedidos.

### O caminho mais barato de memória

O analisador é a maior linha de memória que a IDE terá, e quase tudo o que a
reduz já está previsto em outro documento e nunca foi usado. Quatro regras, em
ordem de quanto economizam:

**Um processo por workspace, e não por `tsconfig`.** O `tsserver` gerencia vários
projetos por dentro — subir um por arquivo de configuração multiplicaria a
biblioteca padrão e os `.d.ts` compartilhados por nada. Um processo, vários
projetos dentro dele.

**Ele não sobe até ser preciso.** A `04` já determina que a ativação é preguiçosa:
registrar um provider não cria o runtime dele. Um workspace só de Java nunca paga
por TypeScript, e abrir um `.ts` é o que acorda o analisador.

**Ele desce quando para de ser preciso.** A `04` define o estado `Suspended` e uma
"política de ociosidade", e nenhum provider a exerce hoje. É aqui que ela ganha
razão de existir: fechada a última aba de TypeScript, o processo cai depois de um
tempo, e a próxima abertura o traz de volta. Custa reindexar; poupa centenas de
megabytes em quem trabalha o dia todo em Java com um `.ts` aberto de manhã.

**Teto declarado, e queda em vez de paginação.** O processo sobe com limite de
heap explícito, e ultrapassá-lo derruba o analisador em favor do provider nativo.
É o orçamento da `08` aplicado a um processo.

E uma economia que vem do desenho, sem esforço: **o VS Code sobe dois
analisadores** — um semântico e um sintático em modo parcial, para o realce não
travar enquanto o outro trabalha. Nós não precisamos do segundo, porque o provider
nativo da fase 1 já é esse servidor sintático, em Rust e em processo. Fica escrito
para que ninguém o acrescente depois por imitação.

### Isso conflita com uma regra do projeto, e vale encarar

A `00` diz que ferramentas externas servem "apenas como dependências de execução
para compilar, executar ou depurar projetos do usuário, nunca para implementar a
IDE". Analisar não está na lista, e um analisador externo é, sim, análise
acontecendo fora do nosso código.

E a `04` diz o contrário, com nome próprio: ela lista `jdtls-adapter` e
`remote-java-service` como providers legítimos de Java. Os dois documentos
discordam, e a discordância estava adormecida porque ninguém tinha exercido o
caso.

**A resolução é a fronteira, e não a tecnologia.** A regra da `00` protege o
*núcleo*: interface, editor, host, índice, modelo de projeto, infraestrutura. Um
`LanguageProvider` é, por definição da `04` e da ADR-003, substituível e
opcional. O que não pode existir é **dependência**: a IDE não pode ficar sem
resposta porque o usuário não tem Node.

Daí a exigência que sustenta a decisão inteira: **o provider nativo não é
provisório**. Ele continua existindo depois que o externo funcionar, e continua
sendo o principal quando o externo não estiver disponível. Sem Node instalado, um
projeto TypeScript abre, destaca, navega pela estrutura e roda as tarefas que não
precisam dele — degrada, como o observador da `21` degrada quando não consegue
observar. Ver a ADR-025.

## Uma origem só para o que é o projeto

Aqui há uma armadilha que só aparece quando a IDE tem modelo de projeto e o
analisador também tem. A nossa tem.

O `tsserver` faz descoberta própria: partindo de um arquivo, sobe procurando o
`tsconfig.json` mais próximo, e é ele que define quais arquivos compõem o projeto
— por `include`, `exclude`, `files` e `references`. Se, em paralelo, o nosso
`ProjectModel` deduzisse as raízes por convenção — `src/` porque costuma ser
`src/` —, passariam a existir **duas definições de qual é o projeto**.

Elas discordariam em casos que não são raros: monorepo com vários `tsconfig`,
projetos com `references`, testes excluídos do build, `paths` remapeando módulos,
arquivo fora do `rootDir` puxado por um `import`. E a discordância é silenciosa,
que é a pior forma: o índice responde sobre um arquivo que o analisador considera
fora do projeto, a navegação leva a um lugar sobre o qual a completação não sabe
nada, e uma renomeação reescreve um arquivo que o compilador nunca vê.

**A decisão: manda o `tsconfig.json`.** O `BuildSystemAdapter` de TypeScript o lê
e produz o `ProjectModel` a partir dele; as raízes de fonte são **importadas**, e
não deduzidas. Ninguém no nosso lado adivinha `src/`.

E note qual é exatamente a origem única: **é o arquivo, e não um processo**. Nós
lemos o `tsconfig.json` e o `tsserver` lê o mesmo `tsconfig.json`. Não é o modelo
perguntando ao analisador — o que criaria uma dependência do modelo de projeto a
uma ferramenta externa, justamente o que a ADR-025 proíbe. São dois leitores da
mesma fonte.

Isso tem uma consequência que vale enunciar em vez de descobrir: **o nosso leitor
é aproximado, e o deles é exato.** Ler `tsconfig.json` não é `serde_json` —
o formato aceita comentários, `extends` encadeado e vírgula sobrando, e a lista
efetiva de arquivos exige expandir `include` e `exclude` como globs. Vamos errar
em algum canto.

Mas errar contra a mesma fonte é um **defeito com forma conhecida e testável**,
enquanto duas definições diferentes seriam um desacordo por desenho, que nenhum
teste apanha porque os dois lados estão certos. Daí sai um item concreto de
verificação: com o analisador presente, comparar a nossa lista de arquivos com a
que ele reporta, e tratar divergência como defeito nosso. Ver a ADR-027.

## O índice e o observador

Cada provider tem o seu índice. O de Java vive em `language-java`, e o de
TypeScript viverá no seu — a `04` já põe `index` dentro do `LanguageRuntime` de
cada linguagem, e não há índice compartilhado a disputar.

O que é compartilhado é o **observador de arquivos**, e ele está errado de lugar:
mora dentro de `language-java`. A `21` já anotou o Explorer como segundo
consumidor sem dono, a `22` acrescentou o Git como terceiro, e TypeScript é o
quarto. Quatro é quando parar de adiar.

A extração está na fase 4 da `22`, e esta especificação **depende dela**, em vez
de duplicá-la: um registro no sistema operacional, vários consumidores, cada um
com o seu filtro. `fonte_java` responde por `.java`; um `fonte_typescript`
responde pelo resto. São filtros de perguntas diferentes, e a regra da `21` — um
filtro só por pergunta — continua respeitada.

`node_modules` **já** está entre as pastas ignoradas da varredura, desde a `19`.
Não é preparação para este momento; é o que impede que abrir um projeto
TypeScript custe minutos.

## Linguagem dentro de linguagem

Um `.ts` pode ter outra linguagem embutida num literal de template. Parece pedir
mudança de contrato, e **não pede**.

`SyntaxSnapshot` é uma lista de trechos com uma classificação cada. Quem é dono
do documento produz a lista inteira, inclusive das regiões embutidas — o
tree-sitter faz isso com injeções, e o resultado atravessa o contrato como
qualquer outro realce. A IDE recebe intervalos e categorias, sem saber que houve
troca de linguagem no meio do arquivo.

Vale como confirmação do desenho: um contrato que fala de intervalos e categorias
neutras, e não de nós de gramática, absorve o caso sem uma linha nova. O `tree`
que saiu do `SyntaxSnapshot` na ADR-016 teria sido o problema aqui. A `24` cobra
essa mesma propriedade com mais força.

## Depuração

TypeScript executa depois de virar JavaScript, no navegador ou no Node, e os dois
expõem depuração numa porta — o Chrome DevTools Protocol. O contrato da `03`
descreve "uma sessão conectada a um alvo já em execução com depuração
habilitada", com `host` e `port`, e diz explicitamente que nada nele identifica
um protocolo. CDP entra por onde JDWP entrou.

Um detalhe encaixa bem demais para não ser dito: o contrato já determina que "o
mapeamento entre o alvo e os arquivos do workspace é responsabilidade do
adapter". Para Java isso é o nome da classe virando caminho; aqui é o **source
map** — o `.js` que executa não é o `.ts` que se escreve. É a mesma
responsabilidade, exercida por um mecanismo diferente, e o contrato não muda.

O que muda de verdade é uma coisa só: em Java a IDE se **conecta** a um processo
que já roda; aqui é comum que quem inicia o alvo seja uma tarefa da própria IDE.
Iniciar continua fora do contrato de depuração, como a `03` manda — é uma tarefa,
e a sessão se conecta depois.

## Fases

### Fase 0 — A IDE deixa de saber Java ✅ Concluída

**Estado: concluída em 02/08/2026.**

Nenhuma linha de TypeScript. Esta fase corrige o levantamento da segunda seção, e
é a única cujo resultado se mede sem uma segunda linguagem existir.

`ToolchainConfig` deixa de ter campos por ferramenta e passa a guardar escolhas
por `LanguageId` e por **papel declarado pela contribuição** — a principal e a
secundária que a `SettingsSection` já prevê. Os comandos de configuração passam a
carregar de qual seção falam. `classpath_entries` sai do `TaskExecutionContext`
genérico. E a guarda ganha `ide-core` na lista e passa a examinar campo de struct,
não só declaração de item.

### A escolha é por projeto, com um padrão por trás

Uma escolha global por linguagem não chega. Duas cópias do mesmo argumento:

- Angular 11 e Angular 15 exigem Node de faixas diferentes, e a CLI de cada um
  recusa a do outro. Quem tem os dois projetos não tem um Node que sirva aos dois;
- e isso não é novidade trazida pelo TypeScript: um projeto em Java 8 e outro em
  Java 21 sempre tiveram o mesmo problema, resolvido até hoje por trocar a escolha
  na mão ao trocar de projeto.

O formato passa a ser **padrão global mais sobreposição por projeto**, com esta
ordem de resolução:

```text
1. sobreposição do projeto     (raiz do workspace, linguagem, papel)
2. padrão global               (linguagem, papel)
3. detecção automática
4. nada  →  degrada, e diz o que falta
```

**A sobreposição mora na configuração da IDE, e não dentro do projeto.** É a
escolha certa por um motivo concreto: um caminho para uma instalação de Node ou de
JDK é **específico da máquina**, e escrevê-lo dentro do repositório o tornaria
inútil para qualquer outra pessoa — além de virar arquivo a ser comitado sem que
ninguém tenha pedido. A chave é a raiz do workspace, canonicalizada.

**A tela mostra de onde veio o valor em vigor.** Um campo preenchido por padrão
global, por sobreposição ou por detecção parece igual, e agir sobre a origem
errada é a família de defeito que a `21` já nomeou: quem lê não distingue.

**Uma escolha de Node por projeto, e não duas.** Seria possível separar "o Node
que roda o analisador" do "o Node que roda a CLI", já que o primeiro tolera
qualquer versão recente e o segundo não. Não se separa: uma escolha só, usada
pelos dois, e a falha aparece onde importa — na execução da tarefa, com a mensagem
da própria CLI, como manda "Nenhuma tabela de compatibilidade".

### E o que já está gravado continua valendo

`jdk_home` e `maven_home` viram o **padrão global** de `java`, nos papéis
principal e secundário. Quem já usa a IDE não perde escolha nenhuma, e não vê
diferença até querer a primeira sobreposição.

**Critério:** a guarda corrigida passa, com `ide-core` incluída. Escolher JDK e
Maven, fechar e reabrir a IDE devolve as duas escolhas — inclusive vindas do
formato antigo. Duas raízes de workspace diferentes guardam ferramentas
diferentes para a mesma linguagem, e cada uma volta a valer ao reabrir o
respectivo projeto. A tela diz, para cada campo, se o valor veio do projeto, do
padrão ou da detecção. E nenhum comportamento visível mudou para quem tem um
projeto só.

#### Feita, e o que ela revelou

**A guarda foi corrigida primeiro, para ser vista falhar.** Com `ide-core` na
lista e o exame de campo de struct, ela apontou os seis vazamentos antes de
qualquer correção: `jdk_home`, `maven_home`, `remember_jdk`, `remember_maven`,
`resolved_jdk_home`, `resolved_maven_home`. Guarda que ninguém viu reprovar não
protege nada.

**O termo `node` reprovou `FileNode`.** Ao acrescentar os termos de TypeScript à
lista, `node` casou com o tipo da árvore de arquivos, em `ide-workspace` e
`ide-ui` — vocabulário legítimo. Ficou `node_`, que pega `node_home` e
`node_modules` sem pegar o falso positivo.

**A guarda empurrou a migração para o lugar certo.** Traduzir `jdk_home` para
Java/principal é conhecimento de linguagem, e `ide-core` não pode tê-lo. O núcleo
passou a apenas **recolher** as chaves antigas cruas, e quem traduz é a raiz de
composição. Era para ser uma concessão à regra e ficou melhor do que o desenho
original.

**`ToolRole` existia em três lugares.** Ao fazer os comandos carregarem o papel,
apareceu que a mesma ideia estava em `ide-core` como `ToolRole`, em `ide-ui` como
`ToolSlot`, e em lugar nenhum em `ide-application` — que era justamente por que o
comando não podia carregá-la. Subiu para `ide-domain`, como o `CancellationToken`
antes dela.

**A janela já sabia a resposta.** O diálogo de configurações mantém
`SettingsPage::Contribution(index)` desde sempre; ele só não dizia isso no
resultado. O caminho ficou: a janela devolve o índice, o shell o troca pelo
**identificador** da seção que recebeu no catálogo — sem saber o que significa —,
e `ContributionRegistry::language_for_section` o transforma de volta em linguagem
já na aplicação.

**E `classpath_entries` virou `library_paths`**, com a razão escrita no contrato:
cada linguagem chama isso de um jeito — *classpath*, referências, `sys.path` — e o
contrato descreve a coisa, não o nome que uma delas lhe dá. O termo entrou na
guarda para não voltar.

### Fase 1 — A segunda linguagem existe ✅ Concluída

**Estado: concluída em 02/08/2026**, com o realce e a estrutura. O índice de
símbolos e a navegação por nome **não** entraram — ver "O que ficou de fora".

`language-typescript` com tree-sitter: realce, estrutura, diagnóstico de sintaxe,
índice de símbolos, definição e referências por nome. Sem tipos. Registrada como
contribuição ao lado da de Java.

É aqui que o roteamento por extensão é exercido pela primeira vez com dois
providers ativos, que a tela de configurações mostra duas seções, e que a lista de
tarefas tem tarefas de duas origens.

**Critério:** abrir um projeto com `.java` e `.ts` lado a lado dá realce e
navegação nos dois, sem que um interfira no outro. Trocar de aba entre um e outro
não confunde provider, e o tempo de resposta ao digitar é medido nas duas
linguagens.

#### Feita, e o que ela revelou

**A conta da fase 8 da `12` foi cobrada e bateu.** A segunda linguagem custou
**uma** crate e uma linha de `mod` na raiz de composição — o teto do `main.rs`
subiu de 15 para 16. No formato antigo teria custado até seis crates.

**E a guarda generalizada naquela fase passou sozinha.**
`concrete_language_crates_stay_behind_the_composition_root` fala de `language-*`,
e aceitou a crate nova sem ser tocada. As duas que falharam foram as que enumeram
crates à mão, e falharam pelo motivo certo.

**Três decisões de escopo, todas por recusar o palpite:**

- **sem caractere de gatilho.** Sem tipos não há o que oferecer depois do ponto, e
  prometer completação que adivinha é pior do que não prometer. O gatilho volta
  com o analisador externo;
- **`imports` fica vazio no snapshot.** O `ImportItem` do domínio foi desenhado
  sobre `import a.b.C` de Java: tem `path`, `is_static` e `wildcard`. Um
  `import { X } from "y"` não cabe nele sem mentir, e a mentira apareceria como
  navegação errada;
- **`source_root_names` vazio na contribuição.** A raiz vem do `tsconfig.json`
  (ADR-027); um nome de convenção aqui seria uma segunda origem para a mesma
  pergunta.

**Uma dívida ficou anotada no código.** `OutlineKind` é o vocabulário de Java e
não cobre TypeScript: `type` e `function` solta não têm correspondente. Foram
mapeados para o mais próximo honesto — `Class` para o que declara tipo nomeado,
`Method` para o que declara código chamável — em vez de alargar o contrato por
uma linguagem. Fica para o dia em que a terceira chegar, que é quando se saberá
se o problema é geral ou desta.

#### O que ficou de fora

**O índice de símbolos e a navegação por nome não foram construídos.** O texto da
fase os prometia, e o que existe é realce, estrutura e erro de sintaxe. A razão é
que a `04` os desenhou sobre o nome simples — `references_to_name` pergunta por um
nome, sem posição —, e em TypeScript quem decide o que um nome alcança é o
`import`, não o nome. Um índice por nome responderia a mais do que devia e
navegaria para o arquivo errado num projeto com dois `Pedido`.

Entra com o analisador externo, que sabe de módulos. Até lá, `.ts` não oferece
navegação, e não oferecer é melhor do que oferecer errado.

### Fase 1b — O `.ts` chega ao provider ✅ Concluída

**Estado: concluída em 02/08/2026.** Não estava no plano: saiu do levantamento
da fase 3b.

`LanguageController::synchronize_documents` descartava, antes de falar com o
host, todo documento cuja extensão não fosse `java` — com a palavra escrita à
mão. O provider de TypeScript estava registrado, o roteamento por extensão
funcionava, e nada disso importava: **na tela, um `.ts` abria sem realce
nenhum.**

O teste passa a perguntar às contribuições quais extensões têm provider. Uma
linguagem nova é vista sem que esse arquivo mude.

#### Por que a fase 1 passou por cima disso

O teste que deu a fase por cumprida monta um `LanguageHost`, registra os dois
providers e fala com ele. Ele está correto no que afirma — o host roteia certo, e
sempre roteou. Só que o critério da fase é *"abrir um projeto com `.java` e `.ts`
lado a lado dá realce nos dois"*, e isso é o caminho da **aplicação**.

**Testar a camada que se acabou de mexer e concluir sobre a de cima** é o defeito,
e ele não estava no código: estava em como a fase foi dada por pronta. Fica
registrado porque a forma de errar é mais reaproveitável que o erro.

O teste que faltava existe agora, no caminho da aplicação, e foi verificado que
ele **reprova** com o filtro antigo de volta.

Junto vieram três testes que registravam o provider no host e não a contribuição
no registro — passavam porque o filtro dizia `java` sozinho. Agora registram as
duas coisas, que é o que a IDE faz de verdade.

### Fase 2 — O projeto, lido de onde ele está escrito ✅ Concluída

**Estado: concluída em 02/08/2026.**

`ToolchainProvider` para Node, `BuildSystemAdapter` para npm lendo `package.json`
e `tsconfig.json`, e os scripts do `package.json` oferecidos como tarefas.

O `tsconfig.json` é lido de verdade: comentários, `extends` encadeado, `include` e
`exclude` expandidos. As raízes de fonte do `ProjectModel` **saem dele**, e não de
convenção. É o que a seção "Uma origem só" decide, e é o trabalho principal desta
fase — o resto é encanamento.

**Critério:** apontar a IDE para um projeto TypeScript real reconhece as raízes
declaradas no `tsconfig.json`, inclusive quando ele estende outro, e lista os
scripts como tarefas. Um projeto com dois `tsconfig` não mistura os dois
conjuntos. Sem Node configurado, o projeto abre e as tarefas explicam o que falta
em vez de falhar em silêncio.

#### Feita, e o que ela revelou

**O leitor de `tsconfig.json` foi o trabalho, como previsto.** Treze testes, e os
que mais custaram não foram os óbvios: comentários e vírgula sobrando (o arquivo
que a própria CLI gera vem cheio deles — um leitor estrito recusaria o padrão da
ferramenta), `extends` resolvendo caminhos relativos ao arquivo onde foram
escritos, e o filho **substituindo** o `include` da base em vez de somar.

**A crate precisou da mesma cerca que Java tem.** Ao ganhar `ide-process` e
`ide-project` para o adapter de npm, o analisador de TypeScript passou a poder
alcançá-los — e `pub(crate)` não separa isso. A crate foi reestruturada para
`analyzer/`, como a de Java, e entrou na guarda. Verificado que ela reprova:
um `use ide_project::…` em `analyzer/parser.rs` a quebra apontando o arquivo.

Isso tornou o formato "uma crate por linguagem" consistente de verdade entre as
duas, e não só no nome.

**As tarefas exigiram uma capacidade nova.** As de Java são conhecidas na partida
— compilar, executar e testar existem antes de haver projeto. As de npm não: são
os `scripts` do `package.json`, e mudam de projeto para projeto. Entrou
`replace_language_tasks` no registro de contribuições e no de tarefas. Declarar um
conjunto fixo teria sido adivinhar nomes, que é a tabela de compatibilidade
proibida com outro nome.

**E a detecção de ferramenta era chamada só para Java.** Com uma linguagem só
ninguém percebia. Virou `detect_all_toolchains`, que percorre as contribuições que
declaram uma toolchain — uma linguagem nova entra sem que o laço mude.

**Ao mexer nisso apareceu um defeito antigo:** a escolha de ferramenta era gravada
e **nunca restaurada**. `tool_home(…, Primary)` não era lido em lugar nenhum, e a
detecção sobrescrevia a escolha do usuário a cada abertura — a ordem em que a
máquina responde decidia por ele. É exatamente o que o comentário do teste em
`ide-core` já dizia ter sido resolvido, e o lado de gravar existia sozinho. Agora
a ordem da fase 0 é aplicada de verdade.

**E cinco textos diziam "JDK" no caminho genérico.** `"Selecionar pasta do JDK"`,
`"JDK a salvar"`, `"No JDK selected"`, `"Selected JDK"` e a mensagem de pasta
inválida — todos no código que agora atende as duas linguagens. Escolher Node
abriria uma janela pedindo um JDK. O rótulo passou a vir do `field_caption` que a
contribuição declara.

**Uma nota sobre a guarda que os protege.** Ela nasceu como varredura por nome de
ferramenta em todo o `native_ide.rs`, e reprovou duas coisas legítimas: o
comentário que explicava a própria regra, e o `choose_maven_home`, que esta
especificação deixou Java-específico **de propósito**. Virou uma lista dos textos
exatos, com a razão ao lado. Uma guarda que reprova o que se decidiu fazer não
protege nada — ela ensina a desligá-la.

#### O que ficou pendente

**A detecção usa o `PATH` do processo da IDE.** Quem troca de versão de Node com
um gerenciador num terminal não muda o que a IDE já resolveu na partida — é o
motivo de a escolha por projeto existir, e ela cobre o caso. Mas a IDE não avisa
que a versão detectada pode não ser a que o terminal usaria, e devia.

### Fase 3 — O analisador externo ✅ Concluída

**Levantamento de 02/08/2026, antes do código.** Ele mudou a forma da fase: o que
parecia ser sobre TypeScript é, na maior parte, sobre o que a IDE ainda não sabe
fazer. Três coisas que este documento assumia prontas não existem, e nenhuma
delas é de linguagem nenhuma.

É o mesmo padrão da fase 0, e vale registrá-lo como padrão: **uma capacidade nova
cobra primeiro o que a infraestrutura devia e ninguém tinha cobrado.**

#### O que o levantamento encontrou

**O `ProcessSupervisor` não sabe conversar.** Ele oferece `spawn`, `terminate`,
`status` e `execute`, e `spawn` não configura `Stdio::piped()` — herda a saída do
processo pai e guarda o `Child` num mapa. Não há como escrever no stdin nem ler o
stdout incrementalmente.

A porta existe pelo nome e não pela forma: foi desenhada para `javac` e Maven —
rodar, responder, morrer. O analisador é o oposto: vive junto com a IDE e mantém
estado entre pedidos. A tabela da seção "O que exatamente se executa" aponta o
`ProcessSupervisor` como o maquinário certo, e continua certa; o que falta é ele
ter a forma que ela descreve.

**A queda para o nativo não acontece em tempo de execução.** `ProviderSelection`
com `primary` e `fallbacks` existe e é testado — mas o teste cobre falha na
**ativação**. Depois que um documento é aceito, `worker_for_document` o mantém
preso ao provider que o aceitou, e um erro depois disso não re-roteia. Um
`tsserver` que morra no meio da sessão deixaria o documento preso ao provider
morto, que é exatamente o que a ADR-025 promete não acontecer.

**`ProviderState::Suspended` nunca é usado.** Os outros seis estados aparecem no
código; este não. A política de ociosidade da `04` é declarada e nunca exercida —
e a seção "O caminho mais barato de memória" conta com ela.

**`MemoryBudget` só existe na `08`.** Nenhuma linha de código o menciona, e o
orçamento de dois números não tem onde se ligar.

**Ninguém configura `ProviderSelection`.** O host tem `set_selection` e nenhum
chamador fora dos testes. Com dois providers para `.ts`, a ordem sairia da
ordenação alfabética dos identificadores — `typescript.service` antes de
`typescript.syntax` por acaso, e não por decisão.

**O que já serve:** o roteamento por extensão com vários candidatos funciona e já
respeita `primary` e `fallbacks` quando há seleção; o `CancellationToken` está no
contrato; `wait_until_indexed` existe para o analisador dizer que ainda monta; e
`max_active_providers` é 8, folgado para três providers.

#### Fase 3a — Um processo com quem se conversa ✅ Concluída

**Estado: concluída em 02/08/2026.**

Nenhuma linha de TypeScript. `ProcessSupervisor` ganha a forma longeva:
processo com stdin e stdout ligados, escrita e leitura por linha, encerramento
explícito e detecção de morte.

Fica **ao lado** de `execute`, e não no lugar dele. Rodar e coletar continua
sendo o certo para `javac`, Maven e `npm run` — trocar todos por um mecanismo
conversacional seria pagar complexidade em quem não precisa.

**Critério:** um processo de teste que ecoa linhas recebe três pedidos e responde
os três, na ordem. Matá-lo por fora é percebido, e não trava quem espera. E
encerrar o supervisor não deixa processo órfão.

##### Feita, e o que ela revelou

**A saída de erro precisou ser drenada, e isso não é zelo.** Ligar o canal de
erro e nunca lê-lo enche o buffer do sistema operacional, e o processo filho
trava ao escrever nele — um analisador falante travaria sozinho, sem erro nenhum
a apontar. A saída vai para o registro, numa tarefa própria.

**`converse` ganhou implementação padrão, e por um motivo e não por preguiça.**
Acrescentá-la como obrigatória quebrou três dublês de teste de quem executa
build — e eles estão certos em não conversar: as duas formas são independentes, e
um supervisor pode legitimamente saber só rodar e coletar. O padrão **recusa**, e
recusar é uma resposta; quem chama já sabe degradar. A suíte voltou ao verde sem
nenhum teste alterado.

**O fim da saída é o sinal de morte.** `receive` devolve `None` quando o canal
fecha, e é disso que a fase 3b vai depender para cair no provider nativo. Sem
esse sinal, quem espera resposta esperaria para sempre — que é o defeito que a
ADR-025 promete não ter.

**Os testes são presos ao Windows**, como o de `execute` que já existia: não há
processo de eco portátil, e o laço é de PowerShell. Está anotado no arquivo que é
ali que se acrescenta o equivalente no dia em que a IDE rodar noutro lugar.

##### Completada depois, e o motivo é uma lição

A fase 3a nasceu **incompleta**, e a falta só apareceu no primeiro passo da 3c: o
`tsserver` não responde por linha. A entrada é JSON por linha, mas a saída é
enquadrada por tamanho, como LSP:

```text
Content-Length: 105
                          (linha em branco)
{"seq":0,"type":"response","command":"status","request_seq":1,...}
```

Ler o corpo com `receive` funcionaria **por acidente**, enquanto nenhum JSON
trouxesse quebra de linha dentro de uma string. Uma resposta de completação que
carregue trecho de código traz, e aí a mensagem seria partida ao meio sem erro
nenhum a apontar — a família de defeito que a `21` já nomeou.

Entrou `receive_exact`, que lê uma quantidade exata de bytes, **ao lado** de
`receive`, que continua certo para quem fala por linha. O teste usa um corpo com
quebra de linha dentro, de propósito: é o caso que a leitura por linha erraria.

**Por que a falta passou.** O levantamento da fase 3 examinou o que a IDE tinha e
não examinou o que o `tsserver` fala. A suposição "processo longevo conversa por
linha" atravessou o levantamento inteiro sem prova, porque parecia óbvia demais
para ser verificada.

O que a corrigiu foi uma sonda de três minutos: instalar o TypeScript num
diretório temporário, mandar um pedido, e olhar os bytes crus. **Levantar o que se
tem não substitui sondar o que se vai integrar.**

#### Fase 3b — O provider que cai quando o de baixo falha ✅ Concluída

**Estado: concluída em 02/08/2026.** Sem o teto de memória, que fica para a 3c —
ver "O que ficou para a 3c".

Também sem TypeScript. Quando o provider ativo de um documento falha de forma que
não é do pedido — o processo morreu, o canal fechou —, o host reencaminha o
documento para o próximo candidato e responde por ele.

É o que dá dente à ADR-025. Sem isto, "o nativo é o chão" é uma frase.

Junto vem o que falta em volta: a raiz de composição passa a **declarar** a ordem
com `set_selection`, em vez de deixá-la sair da ordem alfabética; e
`ProviderState::Suspended` passa a ser exercido por uma política de ociosidade,
com o teto de memória do `MemoryBudget` saindo da `08` para o código.

**Critério:** com dois providers registrados para a mesma extensão e o primeiro
morrendo no meio da sessão, a resposta seguinte vem do segundo, e a IDE não para.
A ordem entre eles é a declarada, e não a alfabética — verificável trocando os
identificadores de lugar sem que o comportamento mude. Um provider ocioso é
suspenso e volta ao ser pedido.

##### Feita, e o que ela revelou

**A distinção entrou no contrato.** `LanguageError::Unavailable` diz "deixei de
existir", contra `Provider`, que diz "este pedido falhou". Sem ela a queda não
teria como acontecer: o host trataria a morte do processo como mais um erro.

**O worker passou a saber o próprio nome.** Quando uma resposta diz que o
provider morreu, é preciso saber qual demitir — e espalhar essa pergunta pelas
onze chamadas assíncronas seria repeti-la onze vezes. Com o identificador no
worker, a demissão fica num lugar só.

**Faltava uma peça, e o teste a encontrou.** A primeira execução reabriu o
documento **no mesmo provider morto**: `candidate_ids` excluía só `Disabled`, e um
provider `Failed` continuava candidato. Agora `Failed` sai junto, e voltar é por
`enable` — de propósito, e não por acaso.

**As duas falhas têm tratamento oposto, no mesmo ponto do código.** Rota perdida
faz a aplicação **esquecer** o documento, para reabri-lo no candidato seguinte;
fila cheia **mantém** o registro, para recalcular a diferença do mesmo ponto
(ADR-017). Ficaram comentadas lado a lado, porque a diferença não é óbvia lendo.

**A suspensão só vale para quem não tem aba aberta.** Um provider com documento
aberto não está ocioso por mais parado que esteja: a tecla seguinte custaria
reindexar o projeto no meio da digitação, e o remédio seria pior. O caso que ela
resolve é o comum — abrir um `.ts` de manhã, fechá-lo, e passar o dia em Java com
o índice do outro retido.

**Quem tem relógio é a aplicação; o host tem o estado.** `suspend_idle` é chamado
do tique da janela, a cada dez segundos, com limite de cinco minutos. Um
temporizador dentro do host seria uma segunda fonte de tempo no processo.

##### O que ficou para a 3c

**O teto de memória.** `MemoryBudget` continua só na `08`, sem código. Ele foi
adiado por um motivo e não por esquecimento: um teto sem nada que o consuma é um
número que ninguém verifica. Ele entra com o processo externo, que é a primeira
coisa na IDE cuja memória vale a pena limitar — e aí o número tem contra o que ser
medido.

##### Uma correção ao levantamento

O levantamento disse que "ninguém configura `ProviderSelection`", a partir de um
grep por `set_selection`. O método existe e se chama **`configure_selection`**. A
conclusão estava certa por acidente — ninguém o chamava mesmo, fora dos testes —,
mas a evidência era outra.

#### Fase 3c — O `tsserver` ✅ Concluída

**Estado: concluída em 02/08/2026.**

Aí sim: o provider `typescript.service` sobre o `tsserver` **do projeto**, com o
nativo como fallback. Completação, diagnóstico e definição com tipo.

Três exigências que não são negociáveis, e que agora têm onde se apoiar:

**Cair para o nativo** quando o processo morrer, quando o Node não existir ou
quando o projeto não trouxer o pacote `typescript` — sem a IDE parar.

**Encerrar o processo** com a desativação do provider. Um `tsserver` órfão come
memória de um projeto grande com folga, e sobrevive à IDE se ninguém o matar.

**Orçamento explícito, e degradação pelo total.** O processo sobe com teto de
heap declarado, e o gasto entra no segundo número do orçamento da `08` — o da
máquina, não o do nosso heap. Passar do teto derruba o analisador e devolve a
palavra ao nativo, em vez de deixar a máquina paginar.

E uma quarta que é o fecho da fase 2: com o analisador de pé, comparar a nossa
lista de arquivos do projeto com a dele. Divergência é defeito **nosso**, e o
teste diz onde.

**Critério:** a completação dentro de um método responde com os membros do tipo
certo, num projeto preso ao TypeScript 4.1 e noutro no 5.6, com o mesmo adapter.
Matar o processo do analisador na mão degrada para o nativo e a IDE continua
respondendo. O consumo de memória do conjunto está medido — os dois números —, e
o teto derruba antes de a máquina sofrer. E as duas listas de arquivos batem, ou
a diferença está registrada com nome.

##### Feita, e os três defeitos que só apareceram rodando

Nenhum apareceria em teste de unidade. Todos apareceram na **primeira execução
contra o processo de verdade**, e é essa a lição da fase.

**O runtime de quem hospeda não serve.** O worker do host sobe um runtime de
thread única; um `tokio::spawn` ali só progride enquanto alguém está dentro de um
`block_on` — e a resposta que o laço de leitura precisa ler chega justamente
quando ninguém está. Pior: os canais de um processo filho ficam presos ao reator
do runtime que os criou, então nem mudar de thread resolveria. A conversa passou
a **ser dona da própria thread e do próprio runtime**, e quem chama fala por
canal, que não precisa de reator.

**Ler e escrever na mesma fila trava.** Uma leitura que espera resposta bloqueia
a escrita enfileirada atrás dela — e a escrita bloqueada é o pedido que
produziria a resposta esperada. Dois canais, dois laços em paralelo. O impasse
dependia de quem chegasse primeiro, o que o tornava pior de diagnosticar.

**O `

` depois do corpo.** O analisador fecha a mensagem com uma quebra de
linha, e a leitura a tomava por início da mensagem seguinte: bloco sem
`Content-Length`, interpretado como morte. **O analisador era dado por morto na
primeira resposta**, e o provider caía para o nativo sem motivo — a degradação
funcionando perfeitamente pelo motivo errado, que é o pior jeito de um defeito se
esconder. Este foi encontrado instrumentando a leitura, e não raciocinando.

##### O que ficou de fora, e por quê

**A memória não foi medida.** O teto está imposto — o processo sobe com limite de
heap declarado, e ultrapassá-lo o derruba, que é a queda da fase 3b. Mas os
**dois números** do orçamento da `08` não existem: `MemoryBudget` continua sem
código, e não há nada que some o processo externo ao heap da IDE e mostre isso.
O critério pede medição, e o que há é enforcement.

**O projeto em TypeScript 4.1 não foi exercido.** O teste roda contra o 5.x. Que
um adapter só atenda as duas pontas é a aposta da ADR-028, e ela continua **não
verificada** — o que existe é o argumento de que o protocolo é estável.

**`change_document` reabre o arquivo inteiro.** O protocolo tem mudança por
intervalo, e ela é o caminho rápido. Reabrir é lento e é certo: a conversão de
linha e coluna erra por um, e errar ali reescreve no lugar errado sem erro nenhum
a apontar. Trocar por incremental é trabalho com medição, e não palpite.

#### E a navegação de TypeScript entra aqui

A fase 1 prometeu índice de símbolos e navegação por nome e não os entregou,
porque em TypeScript quem decide o que um nome alcança é o `import`, e não o nome.
O analisador sabe de módulos: definição e referências de `.ts` são desta fase, e
não da 1.

### Fase 4 — Depurar ⬜ Fora de escopo

**A IDE não vai escrever um depurador de TypeScript. Quem depura é o navegador.**

O DevTools já está instalado, lê source maps sozinho, entende os mapas que um
bundler produz, e ainda dá rede, DOM e desempenho — coisas que a IDE não daria.
Construir o nosso custaria dependência de WebSocket, o protocolo CDP, e um
resolvedor de source maps nos dois sentidos, para entregar menos.

#### Por que Java é diferente, e a assimetria não é arbitrária

A depuração de Java foi construída em casa porque **não havia alternativa**: não
existe ferramenta ao lado que depure uma JVM, e construir era a única forma de a
capacidade existir.

No navegador existe, vem junto, e é melhor do que o que sairia daqui no primeiro
ano. A assimetria entre as duas linguagens reflete uma diferença real entre os
dois alvos, e não uma inconsistência de desenho.

#### O levantamento que levou a isto

Antes de escrever código, uma sonda contra o Node com `--inspect` mostrou o que o
CDP exige, e são duas camadas que a IDE não tem:

```text
HTTP  GET /json/list  →  webSocketDebuggerUrl: ws://127.0.0.1:9339/<uuid>
WebSocket             →  {"id":1,"method":"Debugger.enable"}
```

O `host` e a `port` do `DebugSessionRequest` da `03` não bastam: a porta serve um
endpoint HTTP que devolve uma URL com um identificador que muda a cada execução.
E a conversa é por WebSocket — handshake de upgrade, enquadramento por frames,
mascaramento, ping —, contra o `TcpStream` cru que o adapter de JDWP usa.

**O que decide não é o transporte: são os source maps.** O `.js` que executa não é
o `.ts` que se escreve, e traduzir nos dois sentidos é o corpo real do trabalho.
O modo de falhar é o pior que existe: o ponto de parada cai numa linha próxima, ou
não dispara, **sem erro nenhum** — a família que a `21` nomeou e que a fase 3c
encontrou de novo.

#### O que se perde, e é honesto dizer

**Contexto.** O ponto de parada é posto no navegador, e não no editor onde o
código foi escrito; as duas janelas não compartilham nada. É o único item da
conta, e é real.

#### O que a IDE entrega no lugar

Uma **tarefa que abre o alvo com a depuração à mão** — o `ng serve` já rodando, e
a IDE abrindo o navegador na porta certa. Um item na lista de tarefas, sem
protocolo, sem dependência, sem processo. Captura a maior parte do valor por
quase nada.

#### O que isto custa se a decisão mudar

No dia em que a IDE quiser ponto de parada no editor de `.ts`, **nada do que
existe hoje serve**: a fase volta inteira, com o transporte, o protocolo e os
mapas. Fica escrito para que seja decisão revista, e não esquecimento
descoberto.

E o caminho, se voltar, tem uma bifurcação já mapeada: falar CDP por conta, ou
usar o `js-debug` do VS Code como adapter externo — que fala DAP, o mesmo
enquadramento por `Content-Length` que a fase 3a já implementa, e que não exigiria
dependência nova. A escolha entre os dois depende de quanto do trabalho de source
map se quer possuir.

## O que fica de fora, e por quê

- **Depurar dentro da IDE**, que era a fase 4 e virou decisão: quem depura
  TypeScript é o navegador, que já tem a ferramenta e a faz melhor. A razão está
  na fase, e o custo de reverter também;
- **Angular**, e qualquer framework. É a `24`, e a razão de ser uma especificação
  separada está lá: framework não é linguagem, e o que ele pede entra por portas
  diferentes das desta;
- **Verificação de tipo escrita por nós.** Já dito, e vale repetir para quem ler
  só esta seção;
- **JavaScript puro**, como já dito;
- **Formatação e lint.** Prettier e ESLint são ferramentas externas por processo,
  como o compilador Java é, e cabem no `FormatterAdapter` que a `04` já prevê. É
  trabalho próprio;
- **Renomeação em TypeScript.** A renomeação da `04` reescreve referências pelo
  nome, e aqui isso depende de `import` e de módulo para não trocar o que não
  devia. Fica para depois da fase 3, com o analisador respondendo;
- **CSS além do realce**, como já dito;
- **Monorepo com vários pacotes.** Um `tsconfig` raiz por vez na primeira versão —
  as `references` são lidas para não errar as raízes, e não para manter vários
  projetos ativos ao mesmo tempo.

## Riscos

- **A fase 0 mexe em configuração persistida.** Quem já usa a IDE tem JDK e Maven
  gravados no formato antigo, e uma migração malfeita apaga a escolha em silêncio
  — o pior jeito de falhar, e o mesmo que a `21` chama de resposta velha com cara
  de certa. A leitura precisa aceitar os dois formatos até o antigo desaparecer;
- **O nosso leitor de `tsconfig.json` vai divergir do deles.** É esperado, é
  tratado como defeito e é medido na fase 3 — mas até lá a lista de arquivos é
  aproximada, e o índice herda a aproximação;
- **`tsserver` é um processo, com tudo o que isso traz.** Morre, trava, come
  memória e sobrevive à IDE se ninguém o matar. O `ProcessSupervisor` existe para
  isto, e é a razão de o provider externo não ser a fase 1;
- **A memória dele é bem maior que a do índice nativo.** A `20` mediu 103 MB para
  o índice Java inteiro; um analisador de tipos carrega a árvore de todo arquivo
  do programa mais todos os `.d.ts` alcançados, e o pior caso, em monorepo, chega
  à casa dos gigabytes — é por isso que editores expõem teto configurável para
  ele. Projeto médio fica bem abaixo, e nada disso está medido **aqui**: é o que a
  fase 3 mede;
- **A versão do TypeScript é do projeto, não nossa.** Cada projeto traz o seu em
  `node_modules`, e usar o nosso daria respostas que não batem com o build. O
  adapter usa o do projeto, e falta dele é motivo de degradar, não de improvisar;
- **Duas linguagens ativas dobram a memória de índice.** A `08` põe orçamento como
  requisito arquitetural, e a `20` mediu 103 MB só para Java. Um número por
  linguagem, medido, e não uma estimativa;
- **`node_modules` tem dezenas de milhares de arquivos.** Já é ignorado pela
  varredura desde a `19`; o risco é alguma parte nova esquecer disso e descobrir
  pelo relógio.

## O que não muda

- **a IDE não conhece nome de linguagem nenhum.** Depois da fase 0 isso passa a
  ser verificável, e não declarado;
- **o provider nativo é o chão.** Nenhuma capacidade da IDE pode depender de
  ferramenta externa estar instalada; sem ela, degrada;
- **o modelo de projeto não depende do analisador.** Ele lê o mesmo arquivo, não
  pergunta ao processo;
- **um contrato por capacidade, composto.** Nada de `TypeScriptService` que faz
  tudo, pela mesma razão que não há `git_service` na `22`;
- **o observador é um só.** Ele muda de lugar na fase 4 da `22`, e ganha um
  consumidor aqui — não uma segunda instância;
- **a IDE não desenha e não arranja** (ADR-020 e ADR-022).

## Verificação

Cada fase termina com `cargo test --workspace` e
`cargo clippy --workspace --all-targets -- -D warnings`.

**As guardas de arquitetura são o entregável da fase 0**, e não um efeito
colateral:

- `neutral_crates_expose_no_language_specific_public_api` passa a incluir
  `ide-core` e a examinar campos de struct. Com a lista de termos atual, ela
  **falha hoje** — é assim que se sabe que ela vale alguma coisa;
- a lista de termos ganha `typescript`, `node`, `npm` e `tsconfig`, e as crates
  concretas novas entram em
  `concrete_java_crates_stay_behind_the_composition_root`, que passa a se chamar
  pelo que faz e não pela linguagem que vigiava;
- nenhuma crate neutra pode mencionar extensão de arquivo de linguagem: `.java`,
  `.ts` e `.css` vêm todos do descritor da contribuição.

E número medido, como a `19`, a `20`, a `21` e a `22` pedem:

| fase | o que medir |
|---|---|
| 1 | tempo de realce por tecla em `.ts`, contra o de `.java`; memória do índice de TypeScript sozinho |
| 2 | tempo de abertura de um projeto TypeScript real, do clique à árvore pronta |
| 3 | primeira resposta do analisador externo; memória do processo em regime; divergência entre as duas listas de arquivos |
| 4 | tempo entre o ponto de parada e a parada de fato |
