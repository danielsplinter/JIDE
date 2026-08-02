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

### Fase 0 — A IDE deixa de saber Java

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

### Fase 1 — A segunda linguagem existe

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

### Fase 2 — O projeto, lido de onde ele está escrito

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

### Fase 3 — O analisador externo

O provider `typescript.service` sobre o `tsserver` do projeto, como processo
supervisionado, com o nativo como fallback. Completação, diagnóstico e definição
com tipo.

Três exigências que não são negociáveis:

**Cair para o nativo** quando o processo morrer, quando o Node não existir ou
quando o projeto não trouxer o pacote `typescript` — sem a IDE parar.

**Encerrar o processo** com a desativação do provider. Um `tsserver` órfão come
memória de um projeto grande com folga, e sobrevive à IDE se ninguém o matar.

**Orçamento explícito, e degradação pelo total.** O processo sobe com teto de
heap declarado, e o gasto entra no segundo número do orçamento da `08` — o da
máquina, não o do nosso heap. Passar do teto derruba o analisador e devolve a
palavra ao nativo, em vez de deixar a máquina paginar.

**Um processo por workspace, preguiçoso na subida e suspenso na ociosidade**, como
a seção "O caminho mais barato de memória" determina. A suspensão usa o estado
`Suspended` que a `04` define e que nenhum provider exerce até hoje.

E uma quarta que é o fecho da fase 2: com o analisador de pé, comparar a nossa
lista de arquivos do projeto com a dele. Divergência é defeito **nosso**, e o
teste diz onde.

**Critério:** a completação dentro de um método responde com os membros do tipo
certo, num projeto preso ao TypeScript 4.1 e noutro no 5.6, com o mesmo adapter.
Matar o processo do analisador na mão degrada para o nativo e a IDE continua
respondendo. O consumo de memória do conjunto está medido — os dois números —, e
o teto derruba antes de a máquina sofrer. E as duas listas de arquivos batem, ou
a diferença está registrada com nome.

### Fase 4 — Depurar

Adapter CDP, ponto de parada no `.ts`, com o source map fazendo o mapeamento.

**Critério:** um ponto de parada colocado no `.ts` para a execução no lugar certo,
com as variáveis do quadro legíveis.

## O que fica de fora, e por quê

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
