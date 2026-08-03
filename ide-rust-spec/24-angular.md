# 24 — Angular

## Situação

A `23` entrega TypeScript: realce, navegação, projeto lido do `tsconfig.json`,
tarefas de npm e o analisador externo respondendo com tipo. Com isso, um projeto
Angular já **abre e é editável** — os `.ts`, que é onde está a maior parte do
trabalho, respondem como qualquer TypeScript.

O que falta é o `.html`. Num código Angular, uma parte real da edição do dia
acontece dentro dos templates, e sem esta especificação eles são texto com
realce de HTML: `{{ pedido.total }}` não sabe o que é `pedido`, e `(click)="salvar()"`
não sabe se `salvar` existe.

Esta especificação **depende da fase 3 da `23`** — o analisador externo —, e a
seção "O componente são três arquivos" explica por que essa dependência não é
comodismo, e sim a única forma barata de responder.

## Angular não é uma linguagem

Esta é a afirmação que organiza tudo, e ela não é retórica: é o que permite que
Angular entre sem nenhuma porta nova.

| o que Angular é | onde entra |
|---|---|
| convenção de projeto (`src/app`, `angular.json`) | `BuildSystemAdapter`, e quase nada |
| tarefas (`ng serve`, `ng build`, `ng test`) | scripts do `package.json`, que a `23` já lê |
| uma linguagem de template dentro do `.html` | um `LanguageProvider` próprio |
| modelos de arquivo (componente, serviço, módulo) | `NewItemTemplate` |

Nenhuma dessas portas é nova. É o que a `04` chama de composição de capacidades:
o usuário percebe "a IDE suporta Angular" e por dentro são contribuições
independentes registradas no mesmo host.

**E a IDE continua sem saber o que significa nenhuma delas.** Ela lê
`LanguageDescriptor { display_name, extensions, source_root_names }`, desenha uma
seção de configurações com os rótulos que recebeu, e mostra na lista de tarefas os
títulos que a contribuição declarou. *Angular*, *componente* e *template* são
texto vindo de baixo, como `Getter` e `Constructor` já são hoje — a `04` descreve
esse mecanismo em "Geração de membros", e ele vale igual aqui.

## Uma crate, e nada de Angular fora dela

`language-angular`, uma só, com as capacidades em módulos. É a ADR-024 aplicada
de novo — o que separa a IDE do assunto não é a fronteira de crate, é a
privacidade de módulo, e `pub(crate)` faz o compilador garantir o que seria
disciplina.

```text
language-angular/src/
├── lib.rs         a superfície pública, e ela é pequena
├── project.rs     o que faz um projeto ser Angular
├── component.rs   a unidade de três arquivos, lida do decorador
├── template.rs    o provider dos `.html`  (vira pasta quando crescer)
├── new_item.rs    componente, serviço e módulo
├── tasks.rs       alvos do `angular.json`  (fase 3)
└── analyzer.rs    o descritor de plugin entregue ao TypeScript
```

Sem fragmentação prematura: `template.rs` vira `template/` quando pesar, e como o
`lib.rs` reexporta a superfície, a promoção não quebra ninguém.

### Quem depende de quem, e por quê nessa direção

Aqui está a única pergunta difícil desta seção. O plugin do Angular é carregado
**dentro** do analisador de TypeScript — mesmo processo, mesmo grafo. Como é que
uma crate diz isso à outra sem que o TypeScript passe a conhecer Angular?

Pela inversão: **`language-typescript` expõe uma entrada genérica de plugins**, e
`language-angular` contribui um descritor — nome do módulo e caminho resolvido no
`node_modules`. A crate de TypeScript nunca escreve a palavra *angular*; ela
carrega o que lhe entregaram.

Isso não é invenção nossa para salvar o desenho: é o mecanismo do próprio
`tsserver`, cujo `tsconfig.json` já tem um `plugins` genérico, e é como o VS Code
o expõe às extensões. Estamos usando um ponto de extensão que existe, e não
abrindo um.

A aresta de dependência, portanto, é:

```text
ide-app                       a raiz de composição, a única que nomeia as duas
├── language-typescript
└── language-angular ──> language-typescript     (só pelo descritor de plugin)
```

**E ela aponta para esse lado por ser a verdade.** Angular é uma extensão do
ferramental de TypeScript; inverter a seta faria o TypeScript carregar um conceito
que não é dele. A alternativa — pôr "plugin de analisador" no contrato neutro de
`ide-language-api` — seria pior: um conceito com uma implementação só, e emprestado
da arquitetura de uma ferramenta concreta, vazando forma de `tsserver` para a
camada que existe justamente para não ter forma nenhuma.

**A contribuição é montada em `ide-app`**, num `angular_contribution.rs`, como
`java_contribution.rs` já faz. Não é estilo: a guarda de arquitetura registra que
`language-java` depende apenas de `ide-domain`, `ide-language-api` e
`java-classfile` — crates de linguagem **não** dependem de `ide-application`, e
`LanguageContribution` é tipo dela. `language-angular` segue a mesma restrição, e
a guarda passa a listá-la.

### O que isto não toca

**Java não é afetado por nada desta especificação.** Nenhuma crate de Java ganha
dependência, nenhum arquivo delas é editado, e a guarda que mapeia dependência
permitida por crate continua com as entradas de Java exatamente como estão.

Vale ser exato sobre onde mora o risco que essa preocupação aponta, porque ele
existe e não é aqui: quem mexe em código compartilhado é a **fase 0 da `23`**, que
troca o formato da configuração de ferramentas. Ela afeta Java por construção, e é
por isso que o critério dela exige que as escolhas de JDK e Maven sobrevivam à
migração e que nenhum comportamento visível mude. A `24` chega depois disso, e
acrescenta uma crate ao lado.

## Como o VS Code faz, e o que se aproveita

Vale registrar, porque a resposta é mais instrutiva do que se espera: **o VS Code
não reconhece projetos Angular**. Ele não tem conceito de Angular e não tem modelo
de projeto nenhum. O que ele tem é identificador de linguagem por extensão, pastas
de workspace, e um `tsserver` que faz a própria descoberta pelo `tsconfig.json` —
que é exatamente a origem única que a `23` adotou.

A impressão de que ele entende Angular vem de três camadas, nenhuma no núcleo:

- a extensão embutida de TypeScript, que sobe o `tsserver`;
- a extensão do Angular, que **não é um servidor separado**: ela se declara em
  `contributes.typescriptServerPlugins`, e o efeito é o Angular Language Service
  ser carregado **dentro do mesmo processo `tsserver`**;
- a extensão embutida de npm, que oferece os `scripts` do `package.json` como
  tarefas — `ng serve` aparece porque está escrito lá, não porque alguém sabe o
  que `ng` é.

Duas coisas se aproveitam disso, e uma não.

**Aproveita-se o mecanismo do plugin**, que é o assunto da próxima seção.

**Aproveita-se o teste de reconhecimento**, que é a próxima depois dela.

**Não se aproveita a ausência de modelo de projeto.** O VS Code pode delegar ao
`tsserver` a noção de quais arquivos existem porque não tem uma própria; a nossa
IDE tem, e nesse ponto está mais perto do IntelliJ. É de onde vem a decisão da
`23` de ler o mesmo arquivo em vez de perguntar ao processo.

## O componente são três arquivos

Um componente é `pedido.component.ts`, `pedido.component.html` e
`pedido.component.css`, e o template fala do TypeScript: `{{ pedido.total }}`
resolve um campo da classe, `(click)="salvar()"` resolve um método dela.

**O contrato de hoje não tem como expressar isso.** `LanguageProvider` roteia por
extensão, um documento por vez, e o provider do `.html` não tem como perguntar
nada ao do `.ts`. Seria preciso um conceito de **documentos companheiros** — um
pedido que carrega os irmãos do mesmo componente — e um caminho de consulta entre
providers que não existe.

**Ele não será construído**, e a razão é boa: o `@angular/language-service` é um
plugin do `tsserver`, o que significa que quem responde sobre o template e quem
responde sobre a classe são **o mesmo processo**, com o mesmo grafo de símbolos. O
problema atravessado desaparece do nosso lado por completo.

O preço fica à vista, e é a razão de a fase 1 não ter versão nativa: **sem o
analisador externo, o template não tem resposta**. Ele abre com realce de HTML e
nada mais — e não com diagnóstico errado, que seria pior.

No dia em que isso incomodar, o conceito que falta está nomeado aqui. Escrito é
melhor do que descoberto por defeito, e é o mesmo tratamento que a `21` deu ao
diário de alterações do NTFS.

### O que amarra o template ao analisador não é o Angular

O WebStorm é a prova de que a amarra não é obrigatória — e de onde ela vem de
verdade.

Ele **não** usa o Angular Language Server. Tem engine própria de TypeScript, e
para os templates adotou o **TCB** (*type-check block*) do Angular: a peça do
compilador que traduz o template para TypeScript equivalente, para então
type-checá-lo como código comum. É o mesmo truque que o language server usa por
dentro; a JetBrains pegou o motor e dispensou o servidor.

Só que o TCB só serve a quem já tem para onde mandar o TypeScript gerado. O
WebStorm pode fazer isso porque pagou por uma engine de tipos própria — exatamente
o que a `23` recusou construir, por escrito.

**Logo: o que torna o template dependente do analisador externo não é o Angular,
é a decisão da ADR-025 sobre tipos.** Se um dia a `23` mudar de ideia, esta
especificação muda junto, e o caminho está aberto — o TCB é biblioteca do Angular,
separável do servidor, e o WebStorm demonstra que dá.

Vale saber também que nem o WebStorm escapa inteiro: a documentação deles admite
que a engine interna diverge do algoritmo do language service, com defeitos de
resolução de tipo como consequência, e por isso oferece um botão para delegar ao
serviço. Engine própria é caro **e** aproximado.

## Como se reconhece um projeto Angular

**Não pelo `angular.json`.** Dois testes, e nenhum deles é leitura de
configuração:

- **o projeto** é Angular quando `@angular/core` está entre as dependências
  resolvidas. É o teste que o VS Code usa, e é robusto porque descreve o que o
  código de fato importa, e não o que um arquivo de configuração declara;
- **o arquivo** `.html` é template de alguém quando um decorador `@Component`
  aponta para ele por `templateUrl`. Quem sabe disso é o analisador, que já tem o
  grafo — não é preciso deduzir por nome de arquivo.

Isso importa mais do que parece. `pedido.component.html` é **convenção, não
regra**: qualquer código que dependa de `.component.` no nome quebra num projeto
que não a siga, e quebra em silêncio. A relação vem do decorador.

`angular.json` sobra para uma coisa só — as tarefas da CLI que não estejam nos
`scripts` do `package.json`. É a fase 3, e é opcional.

## Linguagem dentro de linguagem

Um `.ts` de Angular tem HTML dentro (`template:`) e CSS dentro (`styles: []`), e
o `.html` tem expressões dentro dos atributos. Parece pedir mudança de contrato,
e **não pede**.

`SyntaxSnapshot` é uma lista de trechos com uma classificação cada. Quem é dono
do documento produz a lista inteira, inclusive das regiões embutidas — o
tree-sitter faz isso com injeções, e o resultado atravessa o contrato como
qualquer outro realce. A IDE recebe intervalos e categorias, sem saber que houve
troca de linguagem no meio do arquivo.

É a confirmação mais forte do desenho em toda a família `23`–`24`: o caso que mais
parecia exigir contrato novo não exige nenhum. O `tree` que saiu do
`SyntaxSnapshot` na ADR-016 é que teria sido o problema aqui — um contrato que
falasse de nós de gramática teria de escolher **qual** gramática.

## Fases

### Fase 1 — O template responde ✅ desbloqueada

**A premissa desta fase estava certa. A sondagem que a declarou impossível é que
estava errada, e o registro do erro fica aqui porque ele quase custou o desenho.**

A fase dizia: carregar o `@angular/language-service` como plugin no `tsserver` da
fase 3 da `23`, e rotear os `.html` para lá. É isso mesmo, e funciona.

#### O erro, e como ele se sustentou

A primeira sondagem abriu o template de cinco maneiras e recebeu
`No content available` em todas. A conclusão registrada foi: *"um `.html` não é
código para o `tsserver`; a pergunta não chega ao plugin."*

A mensagem não quer dizer isso. Ela vem do `typescript.js`, e é o que o
`tsserver` responde **quando o handler executou e devolveu `undefined`**:

```js
} else if (responseRequired) {
  this.doOutput(undefined, request.command, request.seq,
                /*success*/ false, performanceData, "No content available.");
}
```

Não é "o arquivo não tem conteúdo". É "ninguém teve o que responder". A pergunta
chegava; a resposta é que vinha vazia — e isso é outro defeito, com outra causa.

**A lição não é sobre TypeScript.** Foi conclusão tirada de uma mensagem de erro
sem ler de onde ela vem, e ela bloqueou uma fase inteira. A `21` já tem a regra
para o caso irmão — não confiar em resposta velha com cara de nova —; esta é a
versão dela para diagnóstico: **não deduzir a causa do texto do erro.**

#### A causa real

Perguntando `projectInfo` sobre o template, ele aparece:

```
o .html pertence a: "/dev/null/inferredProject1*"
```

O `.html` cai num **projeto inferido**. O plugin está carregado e é consultado,
mas sobre um template órfão — sem o componente por perto, sem o `tsconfig`, sem
programa nenhum. Ele não tem o que responder, e devolve `undefined`.

O `ngserver` sofre do mesmo, e trata: no `didOpen` dele há um remendo que abre o
`.ts` irmão, fecha, e reabre o `.html`; e `getDefaultProjectForScriptInfo` traz o
comentário que nomeia o problema — *"to ensure HTML files always belong to a
configured project instead of the default behavior of being in an inferred
project"*.

#### A correção, e ela é um campo

Sobre o protocolo, quem escolhe o projeto é o `tsserver` — mas os comandos de
arquivo aceitam **`projectFileName`**. Nomeando na pergunta o `tsconfig` do
componente irmão, o template responde:

```
completionInfo -> success=true  itens=20:
  arrivalSlots, basePrice, cancellableQuantity, cancelledItemsPrice,
  comments, configurationInfos, cpqDiscounts, deliveryMode, ...
```

São os membros reais do tipo da entrada do carrinho, resolvidos de dentro do
`{{ cartEntry. }}` — os mesmos 20 que o `ngserver` devolve.

**Só esse campo é necessário**, e isso foi isolado ligando e desligando cada peça:

| receita | itens |
| --- | --- |
| só `projectFileName` | **20** ✅ |
| `extraFileExtensions` + `projectFileName` | 20 ✅ |
| `extraFileExtensions` + abrir/fechar o `.ts` irmão | 0 ❌ |
| nada — a sondagem original | 0 ❌ |

Nem o `configure` com `extraFileExtensions`, nem a dança de abrir e fechar o
componente. Ambos estão no `ngserver` e nenhum dos dois é o que faz funcionar
aqui; registrá-los como necessários seria gravar superstição.

#### O que a fase entrega, medido

No `lei-do-esperto`, projeto pequeno, com o plugin vindo **de um diretório nosso**:

| pergunta no template | resposta |
| --- | --- |
| `completionInfo` | 13 membros: `avatarUrl`, `moedas`, `nivel`, `nome`, `vidas`, … |
| `quickinfo` | `(variable) jogadorAtual: Jogador \| null` |
| `definitionAndBoundSpan` | `success=true`, 1 destino |

Completude, tipo ao pousar e navegação, dentro do `.html`.

#### E serve a qualquer projeto Angular

O `@angular/language-service` está em **um** dos cinco projetos locais. Nos outros
quatro ele veio de um diretório nosso, apontado por `--pluginProbeLocations`, e
funcionou igual: o `lei-do-esperto` é Angular 21.2.6 e foi servido por um
language-service 21.2.17. **O `tsserver` continua sendo o do projeto** — é ele
quem decide se um tipo bate, e a regra da `23` fica de pé.

São **14 MB** a embarcar, e nenhum processo a mais.

#### O custo em memória

Medido no `spartacus-develop`, mesmo arquivo e mesma pergunta:

| arranjo | pico | processos Node |
| --- | --- | --- |
| `tsserver` sozinho, hoje | 1906 MB | 1 |
| `tsserver` + plugin, respondendo o template | ~2290 MB | **1** |
| `tsserver` + `ngserver` — o arranjo do VS Code | ~4,1 GB | 2 |

**+385 MB no processo que já existe**, contra +2,1 GB num segundo. É a diferença
entre caber e não caber.

O custo de carga medido antes — o dobro do tempo com o plugin — continua valendo,
e continua sendo razão para o plugin não subir por padrão em projeto sem Angular.

#### O que as duas ferramentas de referência fazem, verificado no disco

Vale registrar, porque foi a comparação com elas que levou à correção.

**VS Code** — a extensão `angular.ng-template` 22.0.1 embute o
`@angular/language-server`: `server/index.js`, 13 MB, mais `node_modules` com
`@angular/language-service` 22.0.1 e `typescript` **6.0.3** — 51 MB ao todo. As
duas sondas do cliente apontam para a própria extensão, e não para o projeto:

```js
args.push("--ngProbeLocations", this.context.extensionPath);
args.push("--tsProbeLocations", this.context.extensionPath);
```

O `--tsdk` é o único caminho para o TypeScript do projeto, e só é passado se
quem usa configurar. **O padrão é analisar Angular com um TypeScript que não é o
do projeto**, e num segundo processo Node, porque `angularOnly: true` está fixo
no código do servidor — ele não responde por TypeScript comum, e portanto não
substitui o `tsserver`.

**IntelliJ** — o `angular-plugin` declara **dez linguagens próprias**, cada uma
com `parserDefinition` e `fileType`, em `org.angular2.lang.html.*`:

```
Angular2Html   Angular17Html   Angular181Html   Angular20Html
Angular2Svg    Angular17Svg    Angular181Svg    Angular20Svg
Angular2       Angular20
```

Quatro versões do parser de template, porque a sintaxe mudou quatro vezes, e as
antigas ficam. O plugin de `tsserver` deles — `ws-typescript-angular-plugin`,
**144 KB**, sem `createProjectService` — é só a ponte: sobre `@volar/typescript`,
ele recebe um `transpiledTemplate` produzido do lado da JVM e o expõe ao
TypeScript como arquivo virtual. **A análise é deles; o plugin só mapeia.**

**E o caminho desta fase não é nenhum dos dois.** Ele usa o
`@angular/language-service` — que é do time do Angular, e envelhece com ele —
dentro do `tsserver` que já sobe. Sem segundo processo, como no IntelliJ; sem
parser nosso, como no VS Code.

#### O que continua valendo desta fase

A parte que não dependia da premissa: **a camada nativa de `.html` é HTML puro**.
Sem nada de Angular, um template abre com realce de HTML, e `@if`, `@for` e
`@defer` são texto comum — não destacados, e **não marcados como erro**. Isso
continua sendo o comportamento certo, e é o que a IDE faz hoje.

E o custo medido do plugin — o dobro do tempo de carga — é razão a mais para ele
não subir por padrão quando chegar a hora.

### Fase 1 (planejada) — O template responde

O plugin `@angular/language-service` carregado no `tsserver` da fase 3 da `23`, e
um provider `template.angular` para os `.html` de projeto Angular: diagnóstico e
completação dos membros da classe do componente.

### A camada nativa de `.html` é HTML puro, e isso não é limitação

Nada no nosso código entende sintaxe de Angular. O realce nativo de um `.html` é
o de HTML, e ponto: `@if`, `@for` e `@defer` são texto comum para ele — não
destacados, e **não marcados como erro**. Tudo o que é específico de Angular vem
do plugin, que por construção é da versão do projeto.

A alternativa seria escrever uma gramática de template nossa, e ela envelheceria
sozinha: o controle de fluxo embutido entrou na versão 17, e a próxima novidade
entra na próxima versão maior. Seria uma tabela de compatibilidade disfarçada de
gramática — exatamente o que a `23` proíbe.

**A consequência é que não existe faixa de versões suportadas.** Não há de-11-a-20
a declarar, porque não há nada aqui dentro que saiba o que é uma versão de
Angular. A IDE carrega o que o projeto tem e mostra o que ele responde. É o título
da primeira seção desta especificação, cumprido literalmente.

O preço já estava aceito: sem o analisador, o template não recebe nada de Angular.
Isto apenas torna aquilo uma simplificação deliberada em vez de uma limitação
sofrida.

A versão do plugin é a **do projeto**, do `node_modules`, pelo mesmo motivo: o
`@angular/language-service` acompanha a versão do Angular, e roda dentro do
`tsserver` da versão de TypeScript que aquele Angular fixa. São três versões
amarradas entre si, e nenhuma delas é escolha nossa — nem precisa ser conhecida
por nós.

**O adapter é o `tsserver` clássico, e não há alternativa hoje.** O `tsgo` não
suporta Angular: o próprio Angular declara que o suporte está em prototipagem, e
que vai exigir mudanças arquiteturais grandes no compilador e no language service,
porque a integração deles com a API do compilador de TypeScript é das mais
profundas que existem. Some-se a isso que projetos em Angular 11 e 15 estão presos
a TypeScript da era 4.x, que o `tsgo` não é. Ver a ADR-028.

**Critério:** dentro de `{{ }}` num template, a completação oferece os membros da
classe do componente, e um nome que não existe é apontado. Sem o analisador, o
mesmo arquivo abre com realce de HTML e **sem** diagnóstico. Um `.html` que não é
template de ninguém não recebe nada de Angular.

E o critério que fecha a regra de não envelhecer: **o mesmo binário, sem
recompilar, atende os dois projetos de referência** — um em Angular 11 e outro em
Angular 15 —, e atenderia um em 19 se houvesse. Se atender um exigir mudar código
nosso, a fase falhou, por mais que funcione.

### Fase 2 — Criar as peças

`NewItemTemplate` para componente, serviço e módulo. Cada um cria mais de um
arquivo, o que é a novidade em relação aos modelos de Java: criar um componente
escreve `.ts`, `.html` e `.css` de uma vez, e os três precisam existir ou nenhum.

**Critério:** criar um componente pela árvore produz os arquivos, com o decorador
apontando para o template certo, e falhar no meio não deixa um componente pela
metade.

### Fase 3 — As tarefas da CLI

Leitura do `angular.json` para oferecer alvos que não estejam nos `scripts`.
Opcional, e a última de propósito: na maior parte dos projetos os `scripts` já
cobrem `start`, `build` e `test`, e a `23` já os lê.

**Critério:** um projeto cujos alvos só existem no `angular.json` mostra as
tarefas. Um projeto sem `angular.json` continua funcionando igual.

## O que fica de fora, e por quê

- **Resposta ao template sem o analisador externo.** É a consequência assumida de
  não construir documentos companheiros. Está nomeada, e não escondida;
- **React, Vue e Svelte.** O que esta especificação prova é que *um* framework
  entra pelas portas existentes. Provar que três entram não muda nenhuma decisão
  de agora — e o custo de cada um, depois desta, é uma contribuição e nada de
  arquitetura;
- **Monorepo com vários projetos no mesmo `angular.json`.** Um por vez, como a
  `23` faz com `tsconfig`;
- **Renomear um componente**, com os três arquivos e o decorador seguindo junto. É
  a renomeação da `04` aplicada a uma unidade de mais de um arquivo, e depende da
  renomeação de TypeScript, que a `23` já adiou;
- **Schematics.** Gerar código pela CLI é executar uma ferramenta externa que
  escreve no projeto; é útil e é outro assunto;
- **CSS além do realce**, como na `23`.

## Riscos

- **A gramática de template muda com a versão do Angular**, e é por isso que ela
  não é nossa. Controle de fluxo embutido entrou na 17, e a próxima novidade
  entrará na próxima versão maior. A defesa é estrutural — a camada nativa é HTML
  puro —, e a regra que sobra é a de sempre: **o que não se entende cala, e não
  acusa**;
- **A CLI recusa Node fora da faixa dela**, e a faixa muda a cada versão do
  Angular. Não há tabela nossa: roda-se `ng`, e a mensagem dele é o que a IDE
  mostra. Reclamar é no momento de executar a tarefa, nunca na abertura do
  projeto — analisar não precisa do Node que a CLI exige;
- **O plugin amarra três versões.** Angular, `@angular/language-service` e
  TypeScript têm de ser compatíveis entre si, e quando não forem, quem falha é o
  `tsserver` inteiro — inclusive para os `.ts`, que funcionavam antes desta
  especificação. **Carregar o plugin não pode derrubar o que já respondia:** falha
  ao carregá-lo degrada o template, e não o TypeScript;
- **Esta especificação prende a `23` ao Node por tempo indeterminado.** A `23`
  poderia um dia trocar o `tsserver` pelo `tsgo` e deixar de precisar de runtime
  de terceiros; enquanto o Angular não estiver portado, a `24` não pode
  acompanhar. As duas passariam a ter requisitos diferentes, e quem decide é o
  projeto aberto — não a IDE. É um risco de planejamento, não de código, e é o
  motivo de a porta da `23` já nascer com dois adapters previstos;
- **Convenção de nome.** Já dito, e é o defeito mais provável de aparecer em
  código de verdade: alguém escreve `.component.` num teste e a coisa parece
  funcionar;
- **Um componente pela metade.** A fase 2 escreve três arquivos, e a `04` já
  estabeleceu para a renomeação que meio caminho é pior que nenhum;
- **Memória.** O plugin roda dentro do `tsserver` e cresce com ele; o orçamento da
  `08` é do processo inteiro, e o número medido na fase 3 da `23` deixa de valer
  quando o plugin entra.

## O que não muda

- **a IDE não conhece framework nenhum.** *Angular* é texto que sobe da
  contribuição, e a guarda da `23` passa a incluir o termo;
- **o modelo de projeto continua lendo o `tsconfig.json`.** O `angular.json` não
  vira uma segunda origem para o que é o projeto — na fase 3 ele é lido para
  tarefas, e só;
- **um `.html` sem dono não recebe nada.** Nem realce de expressão, nem
  diagnóstico;
- **o observador é um só**, e não ganha consumidor novo aqui: os arquivos de
  Angular são os mesmos `.ts` e `.html` que a `23` e o Explorer já observam.

## Verificação

Cada fase termina com `cargo test --workspace` e
`cargo clippy --workspace --all-targets -- -D warnings`.

As guardas da `23` valem aqui com um termo a mais: `angular` entra na lista de
`neutral_crates_expose_no_language_specific_public_api`, e `language-angular`
entra na guarda que mantém adapters concretos atrás da raiz de composição, com
`ide-app` como único consumidor.

**A palavra `angular` só pode aparecer em dois lugares:** dentro de
`language-angular`, e no `angular_contribution.rs` da raiz de composição. Em mais
nenhuma crate — nem em `language-typescript`, que é a que mais teria motivo e é
justamente onde a inversão de plugin existe para impedir. É a mesma guarda que a
`22` usa para `Command::new("git")`, e ela falha no dia em que alguém tomar o
atalho.

**O mapa de dependências permitidas ganha duas entradas e não altera nenhuma.**
`language-angular` depende de `ide-domain`, `ide-language-api` e
`language-typescript`, e de mais nada — em especial, não de `ide-application`,
pela mesma restrição que `language-java` já cumpre. As entradas de Java ficam
byte a byte como estão, e é isso que torna verificável a afirmação de que esta
especificação não toca no que já existe.

Uma guarda própria desta especificação: **nenhuma crate pode decidir por nome de
arquivo.** `.component.` não aparece em código nosso — a associação entre template
e classe vem do decorador, pelo analisador. É um teste de texto, e ele existe
porque a alternativa parece funcionar até o dia em que não parece.

E número medido:

| fase | o que medir |
|---|---|
| 1 | primeira resposta dentro de um template; memória do `tsserver` com o plugin, contra a medição sem ele na fase 3 da `23` |
| 2 | nada a medir — o critério é o comportamento |
