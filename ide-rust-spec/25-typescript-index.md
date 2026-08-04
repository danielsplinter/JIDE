# 25 — Índice próprio de TypeScript

## Situação

A `23` entregou TypeScript com tipos, e o preço apareceu ao abrir o primeiro
projeto grande de verdade — um monorepo Angular de 8 958 arquivos:

| | medido |
| --- | --- |
| memória do analisador externo | **1 900 MB** |
| a IDE, ao lado | 411 MB |
| tempo até responder a primeira pergunta | **30,4 s** |
| busca por tipo antes disso | nada, e sem dizer por quê |
| projeto sem `node_modules` | nenhuma capacidade de projeto |

Nada disso é defeito do analisador: ele faz o que precisa fazer. É o custo da
forma como ele responde, e esta especificação pergunta se há outra.

## De onde vêm os números, e o que eles não dizem

A caracterização do problema — 1,9 GB, 30,4 s, 11 287 arquivos — foi medida num
projeto só: o `spartacus-develop`, um monorepo de biblioteca com 8 958 arquivos.
Ele é um **caso extremo escolhido de propósito**, e é onde os defeitos
apareceram. Mas um caso extremo não caracteriza a média, e nada aqui deve ser
lido como "todo projeto Angular custa isso".

O que **não** pode depender de projeto é a solução. Os critérios das fases são
verificados contra qualquer projeto apontado por `IDE_PROJETO_GRANDE`, com a
consulta e o resultado esperado tirados do próprio projeto — ver "O critério não
sabe de que projeto se trata".

## O que se mediu antes de propor

### A memória são as árvores, não os tipos

Depois da carga, pedir verificação semântica de 200 arquivos somou **11 MB** a
1 920 MB. O verificador é preguiçoso e barato; **a memória já está toda lá antes
de qualquer tipo ser calculado**.

Os 1,9 GB são *parse* e *bind* de 11 287 arquivos — 9 418 do projeto e 1 869 de
`node_modules`. Os 23 MB de fonte viram 1,9 GB residentes: **80 vezes**.

Isto é a descoberta que sustenta a especificação inteira. Se o caro fosse
calcular tipos, não haveria conversa. O caro é **manter tudo carregado**.

### O nosso índice de Java já faz o que se propõe aqui

Ele guarda o que o *bind* produz — nome, posição, tipo declarado — e **descarta
texto e árvore ao sair da função que indexa**. Quando precisa de detalhe, reabre
o arquivo.

| | |
| --- | --- |
| declarações indexadas | 339 664 |
| memória | **103 MB** |
| percorrer todas por prefixo | 12,6 ms |
| 9 314 ocorrências de um nome | 1,2 ms |

E ele responde `.` — `member_access` e `type_members` são capacidades do provider
nativo de Java hoje, sem processo externo nenhum.

### O tamanho do problema aqui

| | |
| --- | --- |
| arquivos `.ts` | 8 958 |
| fonte | 23 MB |
| declarações de tipo (`class`, `interface`, `enum`) | 8 710 |
| membros aparentes em nível de classe | ~186 000 |
| entradas em `paths` do `tsconfig.json` | **315** |
| barris (`index.ts`, `public_api.ts`) | 2 279 |

Cerca de 195 mil símbolos, contra os 339 664 que o índice Java guarda em 103 MB.
Um índice na mesma forma ocuparia dezenas de MB **em disco** — e a memória que ele
custa não é esse número. Ver "O índice mora no disco".

## O índice mora no disco, e a memória é só o que a consulta tocou

O erro a não cometer é trocar um programa residente por um índice residente. A
`20` já resolveu isto para Java, e o desenho é o mesmo aqui: **o índice é um
arquivo, e as consultas saem dos bytes**, sem materializar estrutura nenhuma.

### O que a `20` conseguiu, e onde ela parou

Ela queria mapear o arquivo em memória e deixar o sistema operacional ser o cache
— residente o que foi tocado, despejado o frio sob pressão —, e por duas razões
explícitas **não** escreveu um cache próprio: não há política de despejo para
acertar melhor que a do sistema, e um cache próprio seria uma terceira cópia da
mesma informação, que é a origem do defeito silencioso que a `19` combateu.

O mapeamento esbarrou em `unsafe_code = "forbid"` (ADR-023). O que ficou no lugar
foi **ler o arquivo inteiro para um vetor de bytes**: as consultas já saem dos
bytes, mas os 103 MB são memória nossa, retida enquanto a IDE viver. A própria
`20` registra a perda: "é redução, não empréstimo".

### O caminho que não precisa nem de mapeamento nem de cache escrito à mão

**Ler sob demanda.** Abrir o arquivo, procurar o deslocamento na tabela, e ler só
os bytes daquele registro. É I/O comum, seguro, sem dependência nova — e quem
mantém em cache o que foi lido é o sistema operacional, de graça, com a política
dele.

Medido contra um arquivo de 100 MB, leituras de 4 KB em posições aleatórias:

| | |
| --- | --- |
| primeira passada | 30,3 µs por leitura |
| segunda passada, cache do sistema quente | 28,1 µs |
| memória retida por nós | **o descritor do arquivo** |

Os 30 µs são teto: a medição foi feita por um interpretador, com o laço dele por
cima. Uma consulta que toque vinte registros custa 600 µs — a mesma ordem do
`Ctrl+clique` de 1,2 ms que a `20` já entrega e que ninguém reclamou.

É exatamente o que o mapeamento daria, obtido por leitura: **memória elástica**,
que o sistema recupera sob pressão, e que não aparece como memória do processo da
IDE.

### O que isso significa para o número desta especificação

| | |
| --- | --- |
| índice em disco | dezenas de MB |
| memória nossa, em repouso | perto de zero |
| memória nossa, respondendo | o que a consulta materializa |
| memória do sistema, elástica | o que foi tocado recentemente |

E a consequência que vale dizer em voz alta: **isto também se aplica ao índice de
Java**. Os 103 MB retidos hoje são leitura antecipada de um arquivo que já é
consultado por bytes; trocar por leitura sob demanda é a mesma mudança, e a `20`
já diz que "trocar leitura por mapeamento é uma linha" porque tudo abaixo do
carregamento trabalha sobre `&[u8]`. Fica **registrado aqui e proposto à `20`**,
não decidido por esta especificação.

## O que decide tudo: em Java o nome basta, em TypeScript não

A `23` já registrou isto e é a barreira central. Em Java, pacote e classpath
tornam um nome globalmente resolvível: `Pedido` é uma coisa só. Em TypeScript
quem decide o que um nome alcança é o **`import`** — o mesmo nome em dois
arquivos são duas coisas, e um nome pode não estar ao alcance.

Por isso as capacidades se separam em três grupos, e não em dois.

### Grupo 1 — o índice resolve sozinho

| capacidade | por quê |
| --- | --- |
| busca por tipo (`Ctrl+L`) | é "quais símbolos se chamam X"; nenhum `import` envolvido |
| estrutura do arquivo | já existe hoje |
| realce | já existe hoje |

A busca é o caso mais gritante: hoje ela espera 30 s na primeira vez e não
funciona sem `node_modules`. Java responde a mesma pergunta em 12,6 ms.

### Grupo 2 — o índice resolve com resolução de módulos

| capacidade | o que exige |
| --- | --- |
| ir para definição | mapear o nome importado ao arquivo que o declara |
| referências | o mesmo, ao contrário |
| `.` sobre receptor **declarado** | tabela de membros do tipo, mais o de cima |

O `.` declarado cobre mais do que parece num código Angular:

| forma | |
| --- | --- |
| `constructor(private svc: LoginService)` | o padrão de injeção de dependência |
| `inject(LoginService)` | o mesmo, no idioma novo |
| `const p: Pedido` | anotação explícita |
| `const p = new Pedido()` | inferência trivial |
| `this.` dentro da classe | onipresente |

**Quanto disso o índice alcançaria, medido em quatro projetos.** Contando os dois
idiomas de injeção e classificando cada tipo injetado como declarado no projeto
ou vindo de dependência:

| projeto | injeções | do próprio projeto |
| --- | --- | --- |
| monorepo de biblioteca | 8 251 | **85%** |
| aplicação | 108 | **76%** |
| biblioteca pequena | 25 | **72%** |
| aplicação pequena | 6 | 67% |

Os de fora se repetem: `Store`, `HttpClient`, `Router`, `FormBuilder`,
`ElementRef`. A expectativa de que uma aplicação comum fosse dominada por tipos
do framework **não se confirmou** — em todos, a maioria dos receptores injetados
é declarada no próprio código.

Duas ressalvas, e elas importam: isto mede o **primeiro** `.` de um receptor
injetado, e não a cadeia depois dele — `this.svc.buscar().` já saiu do projeto,
porque o retorno costuma ser um `Observable`. E três dos quatro projetos são do
mesmo autor, o que não é uma amostra de "qualquer projeto Angular".

A resolução de módulos é a parte cara: 315 entradas em `paths`, `baseUrl`,
`moduleResolution: "bundler"` e 2 279 barris que reexportam barris.

### Grupo 3 — o índice **não** resolve, e não vai resolver

`store.select(sel).pipe(map(x => x.` é o exemplo, e ele não é exótico — é o
código idiomático de Angular com rxjs. Para tipar o `x`:

1. `.select(sel)` devolve `Observable<K>`, e **K se infere do retorno do
   argumento**;
2. `.pipe(...)` tem **10 sobrecargas genéricas** (contadas no rxjs instalado), e
   é preciso escolher uma;
3. `map(x => …)` — o tipo de `x` **não está escrito em lugar nenhum**. Ele volta
   da assinatura de `pipe` para dentro da lambda.

O terceiro é o que fecha a porta: o tipo flui **para trás**, do contexto para o
parâmetro. Isso é tipagem contextual, e quem a implementa implementa genéricos,
sobrecargas e inferência bidirecional — e daí não escapa dos tipos condicionais e
mapeados que os `.d.ts` de Angular e rxjs usam. É o verificador de tipos, que a
ADR-025 recusou escrever.

#### O atalho que fica recusado

Tratar rxjs como caso especial — "`pipe` de `Observable<T>` com `map(f)` dá
`Observable<retorno de f>`" — funcionaria e é **tabela de compatibilidade de
biblioteca**. A `23` proíbe isso pelo nome, e a proibição nasceu de um requisito
explícito: não alterar o código da IDE a cada versão nova de Node ou Angular.
Uma tabela dessas quebra no rxjs 8, e depois em cada biblioteca com genéricos
próprios.

## A regra que torna isto seguro: dizer que não soube

**Inferência parcial falha em silêncio**, e é a família de defeito que esta IDE
já encontrou três vezes: busca vazia sem `node_modules`, analisador caído sem
aviso, primeira busca antes da carga. Nas três, a resposta errada era
indistinguível da resposta certa.

Um índice que devolvesse lista vazia ao não tipar o receptor repetiria o erro:
"não sei o tipo de `x`" e "`x` não tem membros" apareceriam iguais.

Por isso o contrato desta especificação não é "responder", é **responder ou
declarar que não soube**. Três respostas, não duas:

| resposta | o que a IDE faz |
| --- | --- |
| membros | mostra |
| tipo desconhecido | pergunta ao analisador externo, se houver |
| tipo sem membros | mostra lista vazia, que agora significa isso |

## Arquitetura: o índice na frente, o analisador atrás

Não é escolher um. O host já roteia por capacidade — `provider_for_capability` —,
e a composição da `04` é exatamente isto um nível abaixo.

O que muda em relação a hoje: abrir um `.ts` **não sobe mais o analisador**. Ele
sobe na primeira pergunta que o índice não souber responder.

### O ganho de memória é honesto, e é condicional

**Enquanto o analisador roda, não há ganho nenhum.** Ele carrega os 11 287
arquivos de qualquer forma; o índice não muda nada do que ele faz.

| postura | memória nossa | custo |
| --- | --- | --- |
| índice responde, sem analisador | **perto de zero em repouso** | sem `.` nos casos do grupo 3 |
| índice na frente, analisador sob demanda | idem, até o primeiro caso do grupo 3 | nenhum, mas o ganho é adiamento |
| como está hoje | 1 900 MB desde o primeiro `.ts` | nenhum |

Num código Angular, tocar num `Observable` é questão de minutos. **O ganho
principal desta especificação não é memória — é latência e independência do
Node.** Dizer o contrário seria vender o que ela não entrega.

### O que ela entrega, então

- `Ctrl+L` em milissegundos em vez de 30 s;
- busca, navegação e `.` declarado **sem `node_modules`** — que foi a primeira
  falha encontrada no primeiro projeto real;
- IDE útil durante os 30 s de carga do analisador;
- IDE útil quando o analisador cai, em vez de silenciosamente pior.

## E se a memória for mesmo o objetivo

Então esta especificação não é o caminho mais curto, e é honesto dizê-lo aqui.

| ação | ganho medido | custo |
| --- | --- | --- |
| excluir `.spec.ts` do `tsconfig.json` | ~380 MB (1 852 de 9 418 arquivos) | sem tipos nos testes; é decisão do projeto |
| suspender o analisador com documento aberto | ~1 900 MB enquanto parado | 30 s na volta |
| este índice | 0 MB enquanto o analisador roda | esta especificação inteira |

Há um desenho que daria tipos completos **e** memória baixa: um verificador
**dirigido por demanda**, que tipa só o que a pergunta alcança e descarta. A
medição dos 11 MB sustenta a ideia — calcular é barato, guardar é caro. É o que o
rust-analyzer faz para Rust, levou anos e uma equipe, e para uma linguagem com
sistema de tipos menor. Fica **registrado e fora de escopo**.

## O critério não sabe de que projeto se trata

Um teste que dissesse um nome de tipo à mão só valeria para o projeto de onde
esse nome veio. Os critérios desta especificação **tiram a pergunta do projeto**:
uma varredura independente do índice acha nomes declarados, e o teste cobra que o
índice ache os mesmos. Apontar `IDE_PROJETO_GRANDE` para outro projeto passa a
verificar aquele outro.

Verificado em quatro, de tamanhos e naturezas diferentes:

| projeto | arquivos | varredura | pior consulta | memória |
| --- | --- | --- | --- | --- |
| monorepo de biblioteca | 8 956 | 7,0 s | 7,6 ms | +1 MB |
| aplicação | 82 | 101 ms | 58 µs | +1 MB |
| biblioteca pequena | 49 | 51 ms | 84 µs | 0 |
| aplicação pequena | 37 | 50 ms | 148 µs | 0 |

### O teste ingênuo estava errado, e o índice estava certo

A primeira versão da varredura de conferência aceitava `export class X` em
qualquer indentação. Num `.spec.ts` de regra de ESLint, ela achou
`LoadProductsFail` **dentro de um literal de texto** — código citado como string,
não declarado. O índice não o indexou, e não deveria.

A gramática distingue declaração de texto; o varredor ingênuo não distinguia. A
conferência passou a contar só o que está na coluna zero, e o registro fica
porque o instinto seria o contrário: quando os dois discordam, o mais simples
parece o mais confiável.

**E isso só apareceu porque o critério deixou de usar um nome escolhido a dedo.**

## Fases

### Fase 0 — Poder desligar o analisador ✅

Sem isto, nenhuma fase seguinte pode ser medida: com o analisador de pé, não há
como saber o que o índice responde sozinho. É a mesma armadilha que já deu três
defeitos nesta especificação e na `23` — testar uma camada e concluir sobre outra.

#### O que já existia, e eu tinha dito que não

`LanguageHost::disable` **já estava escrito**, e afirmei o contrário na primeira
versão desta especificação porque procurei por `pub fn disable` num método que é
`pub async fn`. Ele já encerra o worker, tira as rotas e deixa o provider
**listado** como `Disabled` — que é o que permite dizer "está desligado" em vez de
a pergunta apenas não achar nada.

Faltava ligá-lo a alguma coisa.

#### O que se fez

Uma chave em `AppConfig`, e ela é neutra: **uma lista de identificadores de
provider**, sem nenhuma menção a TypeScript, Java ou analisador externo. A IDE não
sabe o que `typescript.service` é — é uma cadeia vinda do arquivo, do mesmo jeito
que os nomes de ferramenta em `ToolchainConfig`.

```toml
disabled_providers = ["typescript.service"]
```

Ela é aplicada **depois** do registro de todas as contribuições: um provider
precisa existir para poder sair de serviço. Identificador que não existe vira
aviso no log, e não impede a IDE de abrir — a configuração é escrita à mão.

E a queixa passou a distinguir **desligado** de **caído**. Dizer "indisponível"
para quem desligou faria procurar defeito onde houve escolha.

#### O que a serialização quase quebrou

O campo nasceu no fim da estrutura, depois das tabelas. Em TOML, um valor escrito
depois de uma tabela pertence a ela, e `to_string_pretty` falha. O teste de ida e
volta pegou; ele existe por isso, e não por formalidade.

#### Java não foi tocado, e há teste dizendo isso

Era a condição desta fase. `disabling_one_language_does_not_touch_the_other`
desliga o provider de TypeScript e cobra que Java continue achando o provider
dele, abrindo `.java`, e que o desligado continue **listado** com o estado certo.

**Critério, atendido:** com o analisador desligado, um `.ts` abre com realce e
estrutura — o provider nativo continua em serviço —, e `Ctrl+L` diz que o
analisador está desligado e que a análise nativa não tem índice, em vez de dizer
que não achou nada.

### Fase 1 — O índice, e a busca por tipo ✅

O grupo 1 inteiro, sem resolução de módulos.

#### O que se mediu no projeto real

| | antes, pelo analisador externo | pelo índice |
| --- | --- | --- |
| primeira resposta depois de abrir | **30,4 s** | **4,2 ms** |
| ativar o provider | — | **196 µs** |
| memória do processo | +1 900 MB (externo) | **+4 MB** |
| projeto sem `node_modules` | nada | responde igual |

Quatro milissegundos contra trinta segundos. E os 4 MB são o critério que separa
"índice em disco" de "índice residente" — sem ele, esta fase poderia ser dada por
cumprida respondendo depressa **porque carregou tudo**, que é exatamente o que se
está tentando evitar.

#### A capacidade teve de ser separada

`workspace_types` era roteado por `COMPLETION`. Declará-la no provider nativo
faria ele prometer o **ponto**, que é a fase 4 — e prometer o que não se faz é
como a busca vazia começou.

Entrou `WORKSPACE_SYMBOLS` no contrato: "sei quais tipos existem" é outra coisa
de "sei o tipo desta expressão", e os preços são de ordens diferentes. Quem já
respondia busca — Java e o analisador externo — passou a declará-la, e o teste do
host que a exercia foi ajustado junto.

#### O formato, e o que ele faz diferente da `20`

Registros de tamanho fixo, textos à parte, tabela de nomes ordenada — o desenho
que a `20` provou. **A diferença é a leitura.**

A `20` lê o arquivo inteiro para um vetor de bytes, e por isso os 103 MB do
índice de Java são memória nossa. Aqui só a **tabela de nomes** entra em memória,
porque toda busca por texto a percorre; os registros de símbolo ficam no disco e
saem por deslocamento, quando um nome casa. É I/O comum: sem mapeamento, sem
`unsafe`, sem cache escrito à mão — as três coisas em que a `20` esbarrou.

#### A varredura não pode segurar a ativação

Ela levava 7,8 s no projeto real, e **muito mais com o cache do sistema frio**.
Ativar é o que dá realce ao arquivo recém-aberto; fazer o realce esperar pela
varredura do projeto inteiro seria trocar um problema conhecido por outro.

Ela foi para uma thread, e quem avisa que terminou é o **`ReadinessSignal`** que a
`23` acabou de introduzir para o analisador externo. A IDE já sabe mostrá-lo:
gira no meio da tela enquanto dura e some no fim, sem saber o que está sendo
preparado. Ativar caiu de 7,8 s para **196 µs**.

E durante a varredura a busca responde **"o projeto ainda está sendo indexado"** —
não uma lista vazia. As duas se pareceriam na tela.

#### O que a medição desmentiu, de novo

A primeira construção levou **164 s**, e a explicação óbvia seria "a extração é
lenta". Medido em separado: ler os 8 956 arquivos custa 621 ms, analisá-los
4,1 s, percorrer as árvores 1,2 s — **seis segundos ao todo**. Os outros 158 eram
cache do sistema frio e dois testes construindo o mesmo índice ao mesmo tempo,
disputando o disco. Com o cache quente e um de cada vez: 7,8 s.

É a terceira vez nesta IDE que o número grande estava no disco e não no código.

Esse mesmo teste em duplicata revelou um defeito de verdade: os dois escreviam no
**mesmo arquivo temporário**, e um renomeava o pedaço do outro. Acontece com duas
IDEs abertas no mesmo projeto, e o temporário passou a levar o número do processo.

#### O que ficou de fora

**Reconstrói a cada ativação.** Saber o que mudou desde a última vez é a fase 4
da `20` para Java, e trazê-la para cá antes de haver o que aproveitar seria
escrever invalidação sem uso. Enquanto isso, o índice é sempre fresco — o custo é
a varredura, que agora roda em segundo plano.

**Só tipos.** `class`, `interface`, `enum` e `type X = …`. Função e variável de
módulo não entram: a pergunta é "ir para o tipo", e enchê-la com o resto é o que
a `23` já teve de desfazer no analisador externo.

**`node_modules` fica fora.** O índice responde pelos tipos **do projeto**; os das
dependências são o que o analisador externo traz.

### Fase 2 — Resolução de módulos ✅

A parte cara, e a que decidia se as fases 3 e 4 existem. Ela convergiu.

#### O critério é uma comparação, e não uma lista

Para uma amostra de `import` do projeto apontado, **o arquivo que nós resolvemos
tem de ser o mesmo que o analisador resolve**. É o desenho da ADR-027: a origem é
o `tsconfig.json`, nós dois o lemos, e a divergência é defeito nosso.

Uma lista escrita à mão só provaria que o teste concorda com quem o escreveu. O
analisador discorda quando estamos errados — e discordou.

| projeto | importações conferidas | divergentes |
| --- | --- | --- |
| monorepo de biblioteca | 174 | **0** |
| aplicação | 46 | **0** |

A amostra é enviesada de propósito, e o viés não é o nosso: só entram as
importações que o **projeto** declara como internas — as relativas e as que casam
um apelido do `paths`. Filtrar pelo que já sabemos resolver provaria só que
sabemos o que sabemos.

#### O defeito que a comparação achou

Sete divergências, todas o mesmo caso: `./require-logged-in.commands` resolvia
para lugar nenhum.

`Path::with_extension` **substitui** o que vem depois do último ponto. Um
especificador com ponto no nome — `./pedido.service`, `./pedido.model`,
`./algo.commands`, que são o idioma do Angular — virava `pedido.ts`, um arquivo
que não existe.

O nome de um módulo em TypeScript pode ter quantos pontos quiser; a extensão é só
o que o disco tem a mais. A construção do candidato passou a **acrescentar** em
vez de trocar, e ficaram dois testes de unidade nomeando o caso.

**Nenhum teste de unidade teria pego isso**: os que eu escrevi usavam
`./pedido`, sem ponto, porque é assim que se escreve um exemplo.

#### O que ficou resolvido

| forma | |
| --- | --- |
| relativa | `./pedido`, `../modelo/pedido`, com `..` normalizado sem tocar no disco |
| pasta | `./modelo` como `modelo/index.ts` |
| pasta, jeito Angular | `./libs/core` como `core/public_api.ts`, que é o que o `ng-packagr` gera |
| apelido do `paths` | exato e com `*`, com os destinos tentados em ordem |
| `baseUrl` | importar por caminho absoluto dentro do projeto |
| barril | atravessado até quem declara, com ciclo detectado e teto de profundidade |

Dependência instalada devolve `None` **de propósito**, e isso não é erro: é dizer
"não alcanço", que é diferente de "não existe".

#### Onde isto mora, e por que não é no analisador

Resolver módulo é conhecimento de **projeto** — depende do `tsconfig.json`, do
`baseUrl`, do `paths`. O analisador responde sobre texto, e a guarda de
arquitetura o mantém assim. O resolvedor é um módulo irmão: recebe o que o
analisador extraiu — a lista de `import` e de reexportação, que é texto — e
decide para onde cada um aponta.

Também por isso esta fase **não liga nenhuma capacidade**. Ela entrega uma peça
correta e conferida; usá-la é a fase 3.

### Fase 3 — Definição ✅, referências ✅

`Ctrl+clique` abre a declaração certa, sem tipos e sem processo externo.

#### O critério, e o teste que importa

**Dois módulos declaram `LoginService`.** Um índice que respondesse "quem se
chama assim" acertaria por sorte metade das vezes; quem decide é o `import` do
arquivo que pergunta. O caminho é: o nome sai do texto, o `import` diz de que
módulo ele vem, o resolvedor da fase 2 diz que arquivo é esse, e os barris são
atravessados até quem declara.

Seis casos montados à mão — nome repetido, declaração local, barril, apelido do
`paths`, dependência instalada, `import { A as B }` — e a conferência contra o
analisador em projeto real:

| projeto | definições conferidas | divergentes |
| --- | --- | --- |
| monorepo de biblioteca | 94 | **0** |
| aplicação | 46 | **0** |

#### As duas divergências que a conferência achou

**Tipo não bastava.** O índice em disco guarda tipos, porque a pergunta dele é
"ir para o tipo". Mas `Ctrl+clique` cai sobre qualquer nome: `appConfig`,
`routes`, uma função utilitária. **79 de 94** definições divergiam, todas por
isso — devolvíamos "não alcanço" para nome que não fosse tipo.

Consertar não custou nada no arquivo do índice: as declarações de um arquivo são
extraídas **sob demanda**, e passaram a incluir função e `const` de nível de
módulo. Variável dentro de bloco fica de fora — ela não é destino de navegação
entre arquivos, e registrá-la faria a busca achar a primeira homônima de qualquer
função.

**Posição importa, e não só o texto.** Sobrou uma divergência:

```ts
import { login as fetchingToken, visitLoginPage } from '../support/utils/login';
```

Com o cursor sobre `login`, ele está sobre o nome de **origem** — e `login` não é
um nome que este arquivo use. Procurar "de onde vem `login`" pela lista de
importados achava **outro** `login`, vindo de outro módulo na mesma tela, e abria
o arquivo errado com a mesma cara de certo.

O cursor dentro de um `import` passou a decidir sozinho: ali o módulo está
escrito na mesma linha.

#### Onde o provider passou a morar

`definition` precisa do resolvedor, e resolver módulo depende do
`tsconfig.json` — projeto. O `analyzer` promete não alcançar projeto, e a
promessa vale: o provider **compõe** as duas coisas, e por isso saiu de
`analyzer/` para um módulo irmão. A análise continua sobre texto; quem alcança
projeto é quem compõe.

#### Referências, sem a tabela de ocorrências

Esta seção previa que achar quem *usa* um nome pediria uma tabela de ocorrências
— o índice de Java guarda 2,7 milhões delas — e que isso mudaria o formato do
arquivo. **Não foi preciso**, e vale registrar por quê.

A pergunta é feita **por gente**, e não a cada tecla: dá para percorrer o
projeto na hora. O caminho tem dois passos:

1. **Filtro barato:** os `.ts` cujo texto contém as letras do nome. Quem não as
   contém não pode conter o identificador, e a esmagadora maioria não contém;
2. **Confirmação por resolução:** cada candidato só entra se declarar o nome e
   *ser* o arquivo da declaração, ou se o `import` dele resolver para lá.

**É o segundo passo que separa isto de uma busca por texto**, que a IDE já
tinha. Dois módulos podem declarar `LoginService`, e listar os usos dos dois
juntos seria acertar por sorte metade das vezes — a mesma armadilha que decidiu
a definição nesta fase.

E menção não é uso: o nome dentro de um comentário ou de uma string escreve as
mesmas letras e não usa nada. As ocorrências saem da **árvore**, e só de nós de
identificador.

##### A imprecisão conhecida, e de que lado ela erra

A confirmação é **por arquivo**, e não por ocorrência: um arquivo que importa a
declaração certa tem todas as suas ocorrências contadas. Se ele declarar uma
sombra local de mesmo nome num escopo interno, ela entra junto.

**Ela sobra, e nunca falta** — que é o lado certo de errar numa lista de usos:
quem procura vê um a mais e descarta, em vez de concluir que não há.

##### Medido no monorepo de referência

| nome | usos | arquivos | tempo |
| --- | --- | --- | --- |
| `FutureStockFacade` | 10 | 5 | 1,5 s |
| `IconComponent` | 454 | 220 | 9,8 s |

Segundos, e não milissegundos — é o preço de não ter a tabela. Aceitável porque a
pergunta é rara e explícita, e porque a IDE já sabe rodar trabalho assim fora da
thread da interface, com giro na tela.

##### E a tabela de ocorrências não é onde está o tempo

Antes de mudar o formato do índice, os 10,2 s de `IconComponent` foram abertos:

| | |
| --- | --- |
| varredura do projeto — o que a tabela substituiria | **1,4 s** |
| confirmar os 228 candidatos | **8,8 s** |

**A tabela tiraria 13%.** Os 86% eram trabalho repetido: o mesmo `import` —
`IconComponent` vindo de `@spartacus/storefront` — era resolvido **uma vez por
arquivo**, e cada resolução atravessa barris e analisa os arquivos do caminho.
Lembrar a resposta por especificador, e não por arquivo, levou os 10,2 s a
**7,0 s**, e a confirmação de 8,8 s a 5,6 s.

Compartilhar a árvore entre as duas perguntas do mesmo arquivo — de onde o nome
vem, e onde ele aparece — rendeu **7,0 s para 6,8 s**. Três por cento, e não a
metade que a segunda análise parecia valer: **a sexta previsão de custo errada
desta sessão**. Ficou porque é mais simples assim, e não porque foi rápida.

O que ainda sobra não foi medido por partes. Os candidatos são 228 e o tempo é
de segundos, então há algo maior do que uma análise repetida — provavelmente a
travessia de barris dentro de `declarante`, que lê arquivos do caminho. **Medir
antes de mexer**, que é a lição que esta seção inteira registra.

**A tabela continua na mesa**, com o número certo ao lado: ela vale os 1,4 s da
varredura, e não os dez segundos que pareciam ser dela.

##### Como se pergunta: `Ctrl+Shift+clique` ✅

O gesto nasce **no mesmo lugar** que o `Ctrl+clique`, e leva o mesmo pedido —
documento, posição e nome. É a mesma pergunta virada do avesso: a definição anda
do uso para a declaração, e esta anda da declaração para os usos.

Ligar custou pouco porque o `NavigationRequest` já carregava tudo, e porque a
lista de usos tem a forma que o painel da busca por conteúdo já mostra. Nada de
novo na interface: um `EditorAction`, um comando, e o resultado num painel que
existia.

**E ela roda fora da thread da janela**, no `SearchController`. Não é zelo
abstrato: são 6,8 s no pior caso medido, e na thread da interface isso seria a
IDE congelada por sete segundos — o defeito que esta especificação caçou cinco
vezes.

#### A árvore é reaproveitada entre teclas ✅

Achado ao medir outra coisa: **o realce reconstruía a árvore inteira a cada
tecla**. O `parse` recebia `None` no lugar da árvore anterior, e a
`ParsedDocument` guardava texto e realce, mas jogava a árvore fora.

| arquivo | por tecla, antes | depois |
| --- | --- | --- |
| componente de 35 linhas | 288 µs | 195 µs |
| arquivo de 3 144 linhas | **45 ms** | **27 ms** |

**Menos do que eu esperava, e isso é informação.** Reaproveitar a árvore cortou
40%, e não 90%: o parse não era o grosso do custo. O que sobra é a
`syntax::analyze`, que percorre a **árvore inteira** para montar o realce, a
estrutura e os diagnósticos do arquivo todo — a cada mudança, porque o
`SyntaxSnapshot` do contrato é do arquivo inteiro.

Cortar isso é mudar o contrato para snapshot parcial, e é outra conversa.

##### E o resto do custo era `Node::parent()`

Com a árvore reaproveitada, os 27 ms que sobravam foram medidos por partes:

| | |
| --- | --- |
| percorrer os 24 514 nós | 6 ms |
| **classificar** | +14 ms |
| converter posição e alocar os 7 783 realces | +1,4 ms |
| montar a estrutura | +5,7 ms |

A leitura óbvia — e a minha — foi que os 14 ms eram das comparações de texto: a
classificação fazia uma busca de substring mais duas varreduras lineares de 61
entradas por nó. Trocá-las por uma tabela indexada pelo `kind_id` **não moveu o
número**: 27,0 ms viraram 26,8 ms.

O custo era `Node::parent()`. O tree-sitter **reconstrói o pai descendo a
árvore**, e a classificação o chamava para cada identificador. Quem percorre já
sabe quem é o pai, porque veio dele; passá-lo adiante troca uma reconstrução por
um argumento.

| | por tecla, 3 144 linhas |
| --- | --- |
| antes de tudo | 45 ms |
| com a árvore reaproveitada | 27 ms |
| **sem reconstruir o pai** | **13,7 ms** |

A tabela ficou, e vale **0,7 ms** dos 13,7 — medido desligando-a. Pequeno, real,
e registrado como pequeno para ninguém a confundir com a correção.

##### A estrutura acompanha em repouso

Sobravam 13,7 ms, dos quais a lista de símbolos era **35%** — seis
milissegundos, a cada tecla, para produzir catorze itens que quase nunca mudam.
E a árvore era percorrida **duas vezes**: uma pelo realce, outra por ela.

Duas saídas, e a medição escolheu — números da mesma execução, porque entre
execuções a máquina varia muito:

| arranjo | por tecla |
| --- | --- |
| duas travessias, como estava | 20,3 ms |
| **a lista reaproveitada** | **13,1 ms** |
| uma travessia só, juntando as duas | 16,1 ms |

**Reaproveitar rende quase o dobro de juntar**, e a diferença é maior do que a
tabela mostra: o número da travessia única mede só o custo de *reconhecer* a
declaração, sem remontar o aninhamento, que é o trabalho de verdade.

A razão fica óbvia depois de vista: juntar as passadas continua fazendo o
trabalho da lista **a cada tecla**; reaproveitar não faz nenhum. E as duas quase
não se somam — com a lista reaproveitada, a travessia única economizaria num
caminho que quase não acontece.

**O gatilho é o relógio, e não uma adivinhação.** Saber se uma edição mexeu na
estrutura custa quase o mesmo que refazê-la: é preciso percorrer a árvore para
descobrir. Adivinhar pelo texto editado — "tem chave? tem `class`?" — seria
heurística, e erraria calada. Com o relógio, a lista pode ficar até 150 ms atrás
do texto, e nunca mais do que isso.

O resultado no caminho real: **13,7 → 9,9 ms** de mediana, com o pior caso em
20,9 ms, que é quando ela é refeita.

##### E os testes da árvore incremental passaram a esperar

Os três que afirmam "incremental é igual ao do zero" compararam também a lista de
símbolos, e passaram a falhar — corretamente, porque agora ela tem outro ritmo.
Eles esperam o repouso antes de comparar.

**A invariante ficou mais forte, e não mais fraca:** ela agora diz que, quando o
texto sossega, o caminho incremental e o do zero produzem exatamente a mesma
coisa — realce, estrutura e diagnóstico.

##### Cinco hipóteses, e o que ensinaram

O custo do realce foi diagnosticado errado cinco vezes seguidas nesta sessão: era
o parse, era I/O, era a dedução linear, era a alocação, era a comparação de
texto. Nenhuma era.

Duas armadilhas de medição valem o registro, porque são o motivo de eu ter
errado:

- **medir em build de depuração**, que atribuiu metade do custo ao lugar errado;
- **um baseline que o compilador apaga.** A primeira medição comparou percorrer
  sem fazer nada contra percorrer classificando; o "sem fazer nada" virou código
  morto, e a diferença inflou. Quando o baseline passou a **usar** o resultado,
  a conta mudou.

##### O `InputEdit` é a parte perigosa

Passar a árvore anterior **sem** descrever a edição não dá erro: dá uma árvore
certa para um texto que não é este. O tree-sitter reaproveita nós em posições
que mudaram, e o realce e a navegação passam a apontar para o lugar errado,
calados. É a família de defeito que a `21` nomeia.

Por isso o critério dos testes não é "funciona", é **igual ao do zero**: inserir
um caractere, apagar um intervalo, inserir uma linha inteira e substituir o
documento todo produzem o mesmo realce, a mesma estrutura e os mesmos
diagnósticos que abrir o resultado do zero.

E há um teste só para a armadilha de unidade: o `InputEdit` conta colunas em
**bytes** e o domínio conta em **caracteres**. Editar depois de um acento — que
em português é a regra, e não a exceção — é onde os dois divergem.

#### O caminho entre aspas também é destino ✅

Veio de uso real: `Ctrl+clique` no `'./future-stock-accordion.component.html'` de
um `templateUrl` não abria nada.

**A causa é a mesma que esta spec já nomeou uma vez.** Dentro de aspas não há
identificador, e o caminho nativo devolvia **lista vazia** — que afirma que a
posição não tem destino, e faz a pergunta morrer ali em vez de descer para quem
tem tipos. É a distinção entre "não existe" e "não sei", agora aplicada a uma
terceira posição.

A resposta passa a ser em três níveis, e cada um é uma afirmação diferente:

| onde o cursor está | resposta |
| --- | --- |
| identificador | o que a resolução de módulos achar |
| texto que nomeia um **arquivo vizinho** | aquele arquivo |
| texto que não é arquivo vizinho | **não sei** — desce para o analisador |
| pontuação, espaço | lista vazia — ali não há o que abrir |

A última linha importa tanto quanto a segunda: dizer "não sei" para um clique
perdido faria **cada** clique descer para o analisador externo, que é a espera
que a fase 5 existe para evitar.

**E a regra não menciona Angular.** Um texto literal que nomeia um arquivo ao
lado leva a ele — vale para `styleUrls`, para outro framework, para um caminho
escrito à mão. Especificador de módulo fica de fora por uma cerca explícita: ele
precisa da resolução que atravessa barril e acrescenta extensão.

**O analisador também sabe responder isto** — verificado contra o `tsserver` com
o plugin, que devolve o `.html` correto. Mas ele custou 63 s para responder no
monorepo de referência, e um arquivo vizinho ou existe ou não existe.

### Fase 4 — O `.` declarado ✅

Completação depois do ponto, sem tipos inferidos e sem processo externo.

#### As duas metades do critério, e a segunda é a que importa

Num componente, `this.` e o serviço injetado completam com os membros certos —
inclusive os **herdados**, que num código Angular são metade do que aparece
depois de `this.`; uma lista sem eles parece certa e está incompleta.

E `.pipe(map(x => x.` responde **"não sei o tipo desta expressão"**.

São **três** respostas, e não duas:

| situação | resposta |
| --- | --- |
| o tipo é conhecido | os membros dele |
| o tipo é conhecido e não tem membros | lista vazia, que **afirma** isso |
| o tipo não é conhecido | `Unavailable`, dizendo que não se sabe |

A terceira é o assunto inteiro. Lista vazia é uma **afirmação** — "este tipo não
tem membros" —, e dizê-la sem saber o tipo é a resposta errada com a mesma cara
da certa, que é a família de defeito que esta IDE encontrou cinco vezes esta
semana. Dizendo `Unavailable`, o host encaminha a pergunta a quem alcança mais.

#### Quanto do ponto ele alcança, medido

A `25` estimou 22% a 44%, contando construtores. Perguntando ao provider ponto a
ponto, numa amostra espalhada pelos `.ts` do projeto:

| projeto | pontos | respondidos | "não sei" |
| --- | --- | --- | --- |
| monorepo de biblioteca | 7 828 | **17%** | 6 428 |
| aplicação | 402 | **14%** | 332 |

**A estimativa estava alta.** Ela contava o receptor injetado como alcançável e
esquecia que o segundo ponto de uma cadeia já não é — `this.svc` é alcançável,
`this.svc.buscar` não. O número real é o que decide a fase 5, e ele diz que o
analisador externo continua sendo pedido cedo.

**Zero respostas disfarçadas.** Dos 6 428 pontos que o índice não alcança no
projeto real, todos vieram como `Unavailable`; nenhum como lista vazia. A
terceira resposta vale no código de verdade, e não só nos casos montados.

#### As duas correções que a medição impôs, e as duas eram minhas

**A asserção estava errada.** A primeira versão do teste cobrava zero listas
vazias, tratando-as como defeito. Mas vazio é a resposta **certa** quando o tipo
é conhecido e não tem membros — uma interface marcadora, uma classe só de
construtor. O provider já separa os dois casos por construção; era o teste que
os confundia.

**A amostra estava enviesada.** Ela pegava os 400 primeiros `.ts` de uma
varredura em profundidade, e caiu inteira em testes de Cypress, onde não há
classe nenhuma: **1%** de cobertura, um número que falava da ordem das pastas e
não do índice. Espalhada, 17%.

#### O que fica de fora, e por quê

`store.select(s).pipe(map(x => x.` exige instanciar genéricos, escolher entre
sobrecargas e fazer o tipo voltar da assinatura para dentro da lambda. É o
verificador de tipos, que a ADR-025 recusou — e a recusa continua valendo, com o
exemplo agora medido em vez de argumentado.

### Fase 6 — O giro relata o que a IDE prepara ✅

**O giro de carregamento cobre a preparação da IDE, e não a do analisador
externo.** Quando o índice termina, ele termina. O que precisa do analisador e o
encontra ocupado gira **por pedido**, e com cancelamento.

#### O que se mede hoje, e por que isso é informação errada

O giro dura o que o `tsserver` leva para montar o projeto — o sinal de prontidão
dele é marcado no `projectLoadingFinish`. Medido no `spartacus-develop`, no mesmo
dia e sem mudança de código: **28, 30, 53, 74 e 76 s**.

Nesse intervalo inteiro a IDE **já responde**: realce, estrutura, busca por nome,
`Ctrl+clique` e o `.` sobre tipo declarado vêm do provider nativo e do índice.
Quem olha a tela conclui que deve esperar, e não deve. O giro está tecnicamente
certo e praticamente mentindo.

| | hoje | com esta fase |
| --- | --- | --- |
| giro de abertura | até 76 s | ~8 s — 7,2 do índice, 0,8 do grafo de estilo |

#### A regra, e por que ela é uma frase

**O que a IDE constrói e guarda é dela; o resto não é.** O índice e o grafo de
estilo são cache nosso. O grafo de tipos do analisador não é cache, não é nosso,
e não sobrevive ao processo — é estado vivo de um programa alheio.

Uma versão anterior desta ideia mandava girar enquanto houvesse **capacidade que
ninguém pronto cobre**, pela composição da `04`. Chega quase ao mesmo lugar com
aritmética; a regra acima cabe numa frase e é explicável a quem usa.

#### Isto **não** é adiar a abertura no analisador

A seção logo abaixo recusa uma proposta que se parece com esta, e a diferença é o
que ela muda:

| | o que foi recusado | esta fase |
| --- | --- | --- |
| quando o analisador sobe | no primeiro pedido que o exija | **junto, como hoje** |
| quanto se espera ao clicar | a montagem inteira | o que faltar, e quase sempre nada |
| o que muda | *quando* o trabalho começa | *o que o giro relata* |

O analisador continua subindo desde a abertura. Na maioria dos casos, quando
alguém clica em algo que precisa de tipo, ele já está pronto e não há giro
nenhum.

#### Duas coisas que precisam vir junto

**O giro por pedido, com cancelamento.** Quem pedir tipo aos dez segundos tem de
ver que está esperando, e poder desistir. Sem isso, um giro honesto demais é
trocado por aviso nenhum, e a espera vira travamento sem animação. O
`SearchController` já cancela; o mesmo vale aqui.

**Quem declara o que é nosso.** O `native_ide` é guardado por teste contra saber
o nome de analisador nenhum, então a lista não pode ser escrita lá dentro — ela
vem da **raiz de composição**, como já vêm os contribuintes de plugin. Sem
mudança no contrato neutro, e sem o núcleo aprender o que é um `tsserver`.

#### O que foi feito, e o que não

**Feito:** o giro de abertura passa a olhar só os providers que preparam coisa
nossa. Quem é alheio vem de `analisadores_externos()`, na contribuição — a raiz
de composição declara, e o `native_ide` continua sem saber o nome de analisador
nenhum. Um teste fixa que o externo é o alheio e o nativo não, porque trocá-los
faria o giro voltar a durar a montagem do projeto ou sumir com o índice ainda
sendo construído.

**E a navegação não gira.** Chegou a girar — a ideia era boa no papel: o giro
apareceria exatamente quando alguém pedisse o que só o analisador oferece. Foi
experimentado e **retirado**.

O que o papel não mostrava: na maioria dos cliques o índice responde em
milissegundos, então a animação no meio da tela aparece e some antes de ser
lida. O que era para ser aviso vira piscada, e uma piscada a cada `Ctrl+clique`
é pior do que nenhum aviso. Quem espera continua com a mensagem na barra de
estado, que não pisca.

*Registrado como retirado, e não apagado da spec: a ideia volta a parecer boa
para quem a reencontrar, e o motivo de ela não estar aqui é experiência, e não
esquecimento.*

**E o aviso para quem espera saiu da lista.** Ele já foi tentado de duas formas —
o giro no meio da tela e a mensagem `Procurando X…` na barra —, e as duas foram
**retiradas depois de usadas**, pelo mesmo motivo: quase todo pedido é respondido
em milissegundos pelo índice, e o aviso aparece e some antes de ser lido.

Duas tentativas retiradas são evidência, e não coincidência. **Fica sem aviso**,
e sem item pendente: o que sobra na tela é a resposta — o arquivo abre, ou a
barra diz que não achou.

*A capacidade de desistir continua existindo por baixo: o `SearchController` da
navegação sabe cancelar, e nenhuma chamada ao host espera na thread da janela.
Se alguém for incomodado por uma espera longa de verdade, o que falta é o gesto —
e a terceira tentativa começa sabendo que as duas primeiras piscaram.*

#### Critério

Abrir o monorepo de referência e ver o giro terminar em segundos, e não em
minuto. Nenhum arquivo perde realce, estrutura ou navegação por nome enquanto
isso.

*Este critério pedia também que um pedido feito antes de o analisador ficar
pronto mostrasse giro naquele pedido, com cancelamento. As duas formas de avisar
foram tentadas e **retiradas** — ver acima —, e a exigência saiu junto. A fase
está completa sem ela.*

### Adiar a abertura no analisador ⛔ Recusado

A ideia: manter o processo de pé, que custa **66 MB**, e só lhe mandar o `open`
quando chegar uma pergunta que o índice não soube. Quem passa a manhã navegando e
buscando nunca pagaria os 1,9 GB.

**Recusada, e a razão está nesta mesma especificação.** É a fase 5 outra vez, com
outra roupa — e a fase 5 foi medida em uso: a IDE travou quando o analisador
precisou subir, e o modelo voltou a ser carregar junto.

A conta que a proposta escondia: ela troca **66 MB o tempo todo** por **uma
espera imprevisível no pior momento**. O primeiro `Ctrl+clique` que precisasse de
tipo pagaria a montagem inteira do projeto — 4 s no projeto pequeno, de 30 a 70 s
no monorepo, medidos. Economia que aparece como travamento.

E os 66 MB são o número que desfaz a premissa: **carregar junto não custa 1,9 GB
na abertura, custa 66 MB.** Os 1,9 GB chegam no primeiro `.ts` do projeto que
alguém abre — e aí a espera acontece enquanto o giro está na tela, que é onde ela
é honesta.

### Fase 5 — Subir o analisador sob demanda ✅

Abrir um `.ts` deixou de subir o analisador. Ele sobe na primeira pergunta que
ninguém de pé soube responder.

#### O defeito que esta fase teve de consertar antes de existir

A fase 4 deu ao provider nativo a resposta "não sei o tipo desta expressão", e
ela saía como `LanguageError::Unavailable`. Para o host, `Unavailable` quer dizer
**deixei de existir**: ele tira as rotas de quem a devolveu e o marca como falho.

Ou seja: **o primeiro `.pipe(map(x => x.` de uma sessão derrubava o provider que
dá realce e estrutura.** O arquivo ficava sem cor por causa de uma completação
que ninguém podia responder.

Entrou `LanguageError::Unresolved` — "esta pergunta eu não sei responder, e
continuo vivo". O host reage procurando quem saiba, e ninguém é demitido. O
limite de um provider não é a morte dele.

Conferido por sabotagem: com `Unresolved` mapeado de volta para `ProviderGone`, o
teste reprova.

#### A ordem inverteu, e é o que faz a fase funcionar

Até aqui a seleção punha o analisador como principal e o nativo como alternativa,
e com razão: o nativo não sabia tipo nenhum, e pôr o analisador depois dele seria
nunca chegar nele.

Agora o **índice vai na frente**. Ele responde busca, navegação e o ponto sobre
receptor declarado em milissegundos e sem processo externo; o que não alcança,
ele diz, e a pergunta passa adiante. Sem essa inversão não haveria sob demanda —
o analisador seria sempre o primeiro a ser perguntado.

Por isso `definition` também precisou parar de devolver lista vazia quando não
alcança: como principal, essa lista vazia impediria o analisador de ser
consultado, e `Ctrl+clique` num símbolo do Angular não abriria nada.

#### Quem é ativado ao abrir

Não é "só o primeiro", e não é "todos". É **quem acrescenta capacidade que
ninguém antes cobre** — a composição da `04`. Como o índice cobre tudo o que o
analisador cobre, ele fica parado; um provider com capacidade exclusiva continua
sendo ativado, que é o que um teste antigo cobrava e esta fase quase quebrou.

#### O critério, medido

| | |
| --- | --- |
| abrir um `.ts` | **zero processos externos** |
| primeira pergunta que ninguém soube | **um processo** |

Verificado em dois projetos, com o supervisor de processos contando de verdade —
e não pelo estado que o host reporta de si mesmo.

#### O que se trocou, dito em voz alta

**A queda deixou de ser imediata.** Antes, abrir ativava todos os candidatos, e o
de baixo já tinha o documento: a morte do de cima não custava reabertura. Agora
custa um quadro, e quem reabre é a aplicação, que tem o texto — o host não o
guarda, e guardá-lo duplicaria em memória o que já existe uma vez.

Vale pouco na prática: com o índice como principal, quem morre é o analisador, e
o índice já está de pé respondendo.

**E o analisador sobe sem o texto de nada.** Ele é acordado pela pergunta, mas
responde a partir da seguinte: a aplicação reabre os documentos no quadro
seguinte. Num projeto grande ele leva trinta segundos para montar o projeto de
qualquer forma — um quadro a mais não é o que se vai sentir.

#### O giro que não parava, e o que faltava ligar

Ao abrir o projeto real e dar `Ctrl+clique`, o Node subia — e a animação de
carregamento girava para sempre.

**O host dizia o que faltava, e ninguém escutava.** `documents_missing_providers`
foi escrito nesta fase e não foi ligado à aplicação. O analisador acordava sem
documento nenhum, e o `tsserver` **não carrega projeto sem um arquivo aberto**:
`projectLoadingFinish` nunca vinha, o sinal de prontidão nunca ficava pronto, e
a IDE dizia que estava preparando o projeto até alguém fechá-la.

A aplicação passou a reoferecer os documentos que faltam — **uma tentativa por
documento**. Insistir a cada quadro contra um analisador que recusou o arquivo
seria trinta reaberturas por segundo contra um processo que já disse não.

E o sinal de prontidão passou a ser marcado também quando o analisador **morre**.
Um processo que cai antes de montar o projeto nunca manda o evento de fim, e sem
isso o giro tinha outro jeito de não parar nunca. Morrer também é uma forma de
terminar — é a mesma regra que a construção do índice já seguia, e que faltava
aqui.

#### E acordar travava a janela

Ativar um provider era síncrono, e a pergunta que o acorda vem da thread da
interface. Subir um processo Node no Windows, com antivírus no caminho, leva o
que leva — e a janela parava enquanto isso.

**É a terceira vez que este defeito aparece nesta IDE, sempre pelo mesmo
motivo**: trabalho que não cabe num quadro feito dentro do quadro. Antes foram a
busca textual, que travava por 106 s com o cache frio, e a busca por tipo, que
esperava o analisador montar o projeto.

A pergunta passou a **anotar** quem acordar; quem ativa é o laço de quadros, numa
thread própria. O host decide, e a aplicação executa — a mesma divisão que a
reoferta de documentos já seguia.

#### E o travamento seguinte não era acordar: era perguntar

Acordado fora da thread da interface, o `Ctrl+clique` **continuou travando**. A
causa era outra, e mais antiga: a navegação chamava o host com `block_on` na
thread da interface, e sempre chamou.

Antes da fase 5 isso não aparecia. O analisador subia junto com o projeto e já
estava quente quando alguém clicava: a resposta vinha em milissegundos. Subir sob
demanda tirou a espera do começo e a pôs no primeiro clique — e revelou que a
chamada sempre esteve no lugar errado.

**Quatro vezes o mesmo defeito, nesta ordem**: a busca textual (106 s com o cache
frio), a busca por tipo (30 s esperando o analisador montar o projeto), acordar o
provider (criar processo), e agora a navegação. Sempre trabalho que não cabe num
quadro, feito dentro do quadro; sempre invisível enquanto o que se esperava era
rápido.

A navegação foi para uma thread, com o resultado recolhido no quadro — o mesmo
`SearchController` das duas buscas, pela terceira vez.

#### E um analisador por clique

Um `Ctrl+clique` sobre um método do **próprio arquivo** subia o Node, e a IDE
travava. Dois defeitos, e um alimentava o outro.

**O índice não registrava método de classe.** Ele guardava classe, interface,
enum, apelido de tipo, função solta e `const` de módulo. `this.buscar()` cai
sobre um nome declarado duas linhas acima, na mesma tela, e a resposta era "não
alcanço" — que desde a fase 5 quer dizer **acorde o analisador**. A IDE subia um
processo de 1,9 GB para responder o que estava visível.

**E cada pergunta durante a subida mandava subir outro.** `ensure_active` não
guardava o estado `Activating`: com o analisador levando trinta segundos para
montar o projeto, cada clique nesse intervalo criava mais um processo, todos
montando o mesmo projeto ao mesmo tempo. A máquina engasgava, e o sintoma era a
IDE travada.

O primeiro defeito é que produzia o segundo: sem ele, aquele clique nunca teria
acordado ninguém.

#### O último travamento, e a decisão que veio dele

Abrir um documento também esperava. O comentário no código dizia que abrir é raro
e que o resto depende dele — e o que faltava ver é que **o worker atende um
pedido por vez**. Com o analisador montando o projeto, uma abertura ficava
enfileirada atrás de um pedido com prazo de cinco segundos, e a janela parava a
cada quadro até ele terminar.

Foi o quinto lugar em que o mesmo defeito apareceu — e a frase escrita aqui na
época era *"e o último: agora nenhuma chamada ao host espera na thread da
interface"*. **Ela estava errada, e ficou errada por semanas.**

Faltava a completação, achada na fase 7. Ela escapou das cinco varreduras pelo
motivo que torna este defeito difícil: **ela sempre respondia rápido**. Enquanto
o índice alcança o que se pergunta, a resposta vem em milissegundos e nada
denuncia que a espera é síncrona.

A lição não é "faltou procurar melhor". É que **procurar não bastava**: o que
denuncia este defeito não é ler o código, é a resposta demorar — e ela só demora
com um tipo que o índice não tem. Uma guarda que conte `block_on` na thread da
janela acharia o sexto no dia em que ele foi escrito, e é a única forma de a
frase acima poder ser dita com segurança.

##### Subir junto voltou a ser o padrão

Depois de a IDE travar três vezes seguidas na mão de quem a usa, a postura padrão
voltou a ser a de qualquer outra IDE: **todo provider sobe ao abrir o arquivo**.
Sob demanda continua inteiro, atrás de uma chave:

```toml
eager_language_providers = false
```

A escolha não é técnica, é de quem usa. Subir junto custa 1,9 GB e trinta
segundos desde o primeiro `.ts`, e em troca nada surpreende depois. Sob demanda
economiza isso para quem só navega, busca e edita código com tipos declarados, e
põe a espera no primeiro clique difícil.

E a chave devolveu de graça a queda imediata: com todos de pé, o provider de
baixo já tem o documento quando o de cima morre, e a troca não custa reabertura.
Cada postura tem o seu teste.

#### E quanto isso economiza, honestamente

Com 14% a 17% dos pontos alcançados pelo índice — **12,8% a 25,8% depois da fase
7** —, o analisador continua sendo pedido cedo numa sessão que mexa em código com
genéricos. O ganho não é "1,9 GB a menos"; é: quem só navega, busca e edita
código com tipos declarados nunca paga por ele, e ninguém paga por ele **antes**
de precisar.

### Fase 9 — Os tipos das dependências instaladas ✅

`this.formBuilder.` acha que `formBuilder` é um `FormBuilder`, e para aí:
`FormBuilder` vem de `@angular/forms`. A fase 8 mediu quanto isso custa — **24
dos 51** elos de cadeia sem resposta numa aplicação Angular esbarravam
exatamente aqui.

**E o argumento não é o número, é o alcance.** Uma IDE que só sabe o que o
projeto declara serve a um projeto. Ela vai ser aberta em projetos diferentes, e
em quase todos o que se injeta vem do framework, não do código de quem edita.

#### A fase 1 misturava duas perguntas

Ela deixou `node_modules` de fora, e a razão que escreveu era da **busca por
nome**: a lista não pode encher de tipos que ninguém escreveu. Isso continua
inteiro — um projeto Angular tem dezenas de milhares de tipos instalados, e eles
afogariam os poucos do projeto.

O que foi junto sem precisar era a **resolução**. São duas perguntas:

| pergunta | `node_modules` entra? |
| --- | --- |
| quais tipos existem, para a busca | **não**, e uma guarda fixa isso |
| o que este tipo tem, depois do ponto | **sim**, desta fase em diante |

Separá-las é o que permitiu esta fase sem desfazer o que a fase 1 acertou.

#### Como se acha o arquivo de tipos de um pacote

Na ordem do Node e do TypeScript, e não numa nossa:

1. **`exports`**, que é o que os pacotes modernos usam e o que decide subcaminho
   — `@angular/forms/signals` é uma entrada própria, com tipos próprios. O valor
   é um mapa de condições que aninha, e a que interessa é `types`: um `default`
   apontando para o JavaScript compilado não tem tipo nenhum para ler;
2. **`types`** ou **`typings`**, para o pacote inteiro;
3. **`index.d.ts`** ao lado, que é o costume antigo;
4. **`@types/<pacote>`**, para dependência escrita em JavaScript — com a troca de
   `@escopo/nome` por `escopo__nome`, que é a convenção do DefinitelyTyped.

O `node_modules` é procurado **subindo**, como o Node faz: num monorepo a
dependência está na raiz e o código está alguns níveis abaixo.

#### O projeto vence o pacote instalado

Relativo, depois `paths`, depois `baseUrl`, e **só então** `node_modules`. É a
ordem do TypeScript, e ela importa: num monorepo, um apelido do `paths` aponta
para o código local que substitui um pacote publicado — e o publicado costuma
estar instalado também, porque outra dependência o trouxe. Responder com ele
mostraria a versão publicada a quem está editando a local: **resposta plausível,
e errada**.

#### Quanto rendeu, e quanto custou

| projeto | pontos | sem as dependências | com | ganho |
|---|---|---|---|---|
| aplicação | 179 | 28,5% | **53,6%** | +25 pontos, quase o dobro |
| monorepo de biblioteca | 8 192 | 32,5% | **38,5%** | +6 pontos |

**É a maior de todas as fases**, e por uma margem grande: a 7 rendeu 2,5 pontos e
a 8 rendeu 3,3 no monorepo e zero na aplicação. Aqui a aplicação quase dobra —
que é coerente com o que a fase 8 tinha diagnosticado, e é a primeira vez que
uma previsão desta especificação acerta antes da medição.

**E custou tempo.** A mesma amostra, em compilação otimizada:

| projeto | antes | depois | por ponto |
|---|---|---|---|
| aplicação | 0,34 s | 1,63 s | ~9 ms |
| monorepo | 182 s | 359 s | ~44 ms |

O `forms.d.ts` do Angular tem **208 KB**, e ele é analisado inteiro a cada ponto
que passa por ele. Nada disso trava a tela — desde a correção do sexto
`block_on`, a completação não espera na thread da janela —, mas 44 ms por
pergunta num monorepo é a próxima coisa a medir de perto.

*O conserto óbvio é guardar os membros já extraídos por arquivo: um `.d.ts` de
dependência não muda enquanto a IDE está aberta. Não foi feito aqui de propósito
— esta fase mudou uma decisão da fase 1, e misturar isso com uma otimização
faria duas coisas ao mesmo tempo, o que esta especificação já aprendeu a não
fazer.*

#### Critério

Cumprido: `this.http.` com `HttpClient` vindo de `@angular/common/http` completa
com `get` e `post`, sem o analisador subir. Um pacote **não instalado** continua
sendo "não sei", e a busca por nome continua sem `node_modules`.

### Fase 8 — O segundo elo da cadeia ✅

`this.svc.` — o `this.svc` é alcançável e o que vem depois não. A medição da fase
7 diz que **um quarto de tudo o que a IDE não responde** está aí: 1 636 pontos no
monorepo, 102 na aplicação. Num componente Angular é o gesto mais comum que
existe.

O que falta é um passo, e um só: achado o tipo do receptor, resolver o **membro**
sob o ponto seguinte e usar o tipo dele como novo receptor. Os membros já trazem
o tipo escrito ao lado — é o `detail` que a fase 4 guarda —, e resolver esse nome
é o caminho que a fase 3 já anda.

**Um passo, e não a cadeia inteira.** `a.b.c.d.` é raro; `this.campo.` é o dia
inteiro. Cada elo a mais multiplica o custo e some na estatística, e a fase 4 já
mostrou que estimar quanto rende cada um erra para mais.

**E é ela que faz a fase 7 valer.** Um campo `nome: string` acessado por
`this.nome.` só vira resposta se as duas coisas existirem: o elo da cadeia e o
tipo da linguagem. Sozinha, a biblioteca rendeu 2,5 pontos; sozinho, o elo
esbarraria em `string` e `Observable` sem saber o que são.

**O que precisava ser consertado junto já foi.** Método não guardava o tipo de
retorno: a gramática o põe em `return_type` e a extração lia `type`, que é o
campo de uma propriedade. `buscar(): Pedido` guardava `buscar` e perdia `Pedido`.

Parecia cosmético — o método aparecia na lista sem dizer o que devolve —, e não
era: **é deste campo que sai o receptor do elo seguinte**. Feito depois, esta
fase nasceria cega para toda cadeia que passa por uma chamada, e **pareceria
funcionar**, porque `this.campo.` responderia e só `this.buscar().` não.

*A correção trouxe uma segunda, que ela própria criou:* o conteúdo gravado no
cache da biblioteca mudou — os tipos de retorno entraram, e o arquivo foi de 410
para 458 KB — **sem que a chave mudasse**, porque a chave é a versão do
TypeScript. Quem já tivesse o arquivo antigo leria para sempre um cache sem tipo
de retorno, e nada acusaria: um cache incompleto tem a mesma cara de um certo. O
arquivo passou a se identificar na primeira linha, e um de outro formato é
**descartado, e não convertido** — a mesma regra que o índice em disco já tinha.

#### O que faltava no meio do caminho: propriedade de parâmetro

`this.svc.` não respondia nem depois do elo pronto, e a causa não era o elo:
**parâmetro de construtor com modificador nunca esteve na lista de membros**.

```ts
constructor(private readonly formBuilder: FormBuilder) {}
```

É o idioma de injeção de Angular — no projeto de referência, **toda** página
injeta assim. `svc.` sozinho já funcionava, porque quem resolve o receptor de um
nome solto olha os parâmetros do construtor. `this.svc.` não, porque essa
pergunta é pelos **membros da classe**, e nessa lista o `svc` não estava.

Sem modificador não entra: `constructor(x: Foo)` recebe `x` e o esquece quando o
construtor termina, e oferecê-lo em `this.` seria um nome que não existe.

*Estava escondido desde a fase 4, e nada o denunciava: quem testasse `svc.`
veria funcionar.* Foi a medição da fase 8 que o apontou, lendo os motivos das
recusas num projeto real em vez de contá-las.

#### Quanto ela rendeu

| projeto | pontos | sem a fase 8 | com | ganho |
|---|---|---|---|---|
| monorepo de biblioteca | 8 192 | 29,2% | **32,5%** | +3,3 pontos, 271 pontos a mais |
| aplicação | 179 | 28,5% | **28,5%** | **zero** |

**Zero na aplicação, e o motivo é preciso.** O elo passou a funcionar lá também —
mas 24 dos 51 elos que sobram resolvem para tipos que moram em `node_modules`:
`this.formBuilder.` acha que `formBuilder` é um `FormBuilder`, e `FormBuilder`
vem de `@angular/forms`, que a fase 1 deixou de fora de propósito. **O elo
funcionou; o tipo é que está fora do alcance.** A recusa mudou de "não sei o tipo
desta expressão" para "não sei onde `FormBuilder` é declarado" — que é uma frase
melhor, e continua sendo uma recusa.

O que decide o ganho é, então, **de onde vêm os tipos injetados**: num monorepo
de biblioteca eles são do próprio projeto, e o elo os alcança; numa aplicação são
do framework, e ele não.

#### E o instrumento estava errado — os números anteriores também

A contagem media **os pontos dentro de aspas**: `'./pagina.html'`,
`'contato@empresa.com.br'`, `'smtp.gmail.com'`. Cada um virava uma pergunta que
ninguém faria, e — pior — era classificado como **cadeia**, porque antes dele há
um nome e um ponto.

Corrigido, a aplicação caiu de 397 para **179** pontos, e a cobertura de base
subiu de 12,8% para **28,5%**. Não porque o código melhorou: porque metade das
perguntas nunca deveria ter sido feita.

**Os números da fase 7 e da fase 4 foram medidos com esse instrumento**, e
portanto estavam baixos pelo mesmo motivo. A direção deles continua valendo — a
fase 7 rendeu pouco, e isso não muda —, mas as porcentagens absolutas de então
não são comparáveis com as de agora.

*É a terceira vez que a amostra desta medição está errada: os 400 primeiros
arquivos de uma varredura em profundidade, na fase 4; o dobro de declarações
estimado, na fase 7; e agora os pontos dentro de textos. **O instrumento merece o
mesmo cuidado que o código**, e nas três vezes o defeito só apareceu quando
alguém olhou o que ele estava contando, e não o número que ele deu.*

**Critério:** cumprido — `this.campo.`, `this.buscar().` e `this.nome.` com
`nome: string` completam num projeto real, sem o analisador subir.

### Fase 7 — Os tipos do próprio TypeScript ✅

`let w: String[];` e um ponto. O índice diz que não sabe onde `String` é
declarado, a pergunta desce para o analisador, e a resposta custa o prazo dele.

**E não se trata de `String`.** Ele foi só o que apareceu primeiro. O índice não
conhece nenhum tipo que o TypeScript traz — nem `Date`, nem `Promise`, nem
`Map`, nem `HTMLElement` —, e por isso **todo** ponto sobre algo que não seja
declarado no próprio projeto desce para o Node. O que esta fase entrega não é um
tipo a mais: é a biblioteca inteira da linguagem no cache, de uma vez, como
qualquer IDE da qual se espera que saiba o que `.length` significa.

Nada disso é de ninguém em particular — é do TypeScript, e está no disco, ao lado
do projeto.

#### De onde eles vêm

De `node_modules/typescript/lib`. São 100 arquivos `lib.*.d.ts`, 3,3 MB de texto,
e `.d.ts` é TypeScript comum: o extrator da fase 1 os lê sem mudança nenhuma.

Vir do projeto, e não de dentro do executável, é a **ADR-028** outra vez: a versão
dos tipos é a que o projeto instalou. Um projeto preso ao TypeScript 4 não pode
receber completação do 5, e embarcar uma cópia seria prometer exatamente isso.

#### Lê-se tudo; o `target` decide só o que se oferece

**São os 100.** O que o TypeScript traz cabe inteiro no cache, e é assim que ele
deve entrar: 3,3 MB de texto lidos uma vez por versão de TypeScript instalada, e
gravados como a `20` já grava. Ler por demanda seria pagar disco no meio de uma
digitação para economizar megabytes que não faltam a ninguém.

E é o que torna barato o que viria depois: trocar o `target`, abrir outro projeto
com outro `target`, ou o mesmo projeto ter `tsconfig.app.json` e
`tsconfig.spec.json` com alvos diferentes — como o `j-fis-cloud` tem. Com tudo
lido, nada disso custa uma releitura.

**O `target` entra na hora de responder, e não na de ler.** `"target": "ES2022"`
seleciona `lib.es2022.full.d.ts`, que lista os que valem:

```
/// <reference lib="es2022" />
/// <reference lib="dom" />
```

Cada um lista os seus, e seguir a corrente é a regra inteira — `lib.<nome>.d.ts`
na mesma pasta. Um `"lib"` explícito no `tsconfig` manda mais do que o `target`,
como manda no compilador. Cada declaração no cache carrega de qual arquivo veio,
e a resposta mostra só o que está no alcance do projeto.

**Filtrar importa, e não é economia de memória.** Responder sem olhar o alvo
ofereceria métodos de ES2024 num projeto que compila para ES2022: código que a
IDE sugere e o build recusa. Uma sugestão errada é pior do que sugestão nenhuma —
é a mesma regra do grupo 3, e vale aqui com mais força porque o erro só aparece
na compilação.

*Ler tudo e filtrar na resposta é uma escolha, e o que ela troca é isto: guardar
uns poucos MB a mais em disco para nunca ter que reler quando o alvo muda. É o
lado certo da troca porque a leitura é a parte cara, e a memória é a barata.*

#### A parte nova é a fusão

No código de um projeto, um tipo é declarado uma vez. Nos tipos do TypeScript,
não: `interface Array` é **reaberta em 12 arquivos**, dos quais 8 valem para
ES2022. `forEach` está no `lib.es5`, `includes` no `lib.es2016.array.include`,
`at` no `lib.es2022.array`.

O índice hoje toma a primeira declaração de um nome e para. Aqui ele precisa
**juntar** os membros de todas — e é a única coisa desta fase que muda como o
índice pensa. O resto é encanamento.

E a fusão não pode valer para o código do projeto: dois módulos que declaram
`LoginService` são **dois tipos**, e juntá-los seria a mesma armadilha que a fase
3 evitou nas referências. A fusão vale para o que se declara como `interface` no
mesmo escopo global, que é o que a linguagem define — e não uma regra nossa.

#### O tamanho, medido

Contra o TypeScript **5.9.3** do `j-fis-cloud`:

| o que | quanto |
|---|---|
| nomes de tipo lidos | **1 525** |
| membros de `Array`, já fundidos | 35 |
| análise dos 100 arquivos | **1,26 s** (compilação de depuração) |
| cache em disco | **410 KB** |
| releitura do cache | **21 ms** |

**A estimativa que abriu esta fase dizia 3 000, e o número é 1 525.** A conta
antiga somava linhas `declare` — `declare var`, `declare function` —, que não são
tipos com membros. O erro era para mais, e é o lado inofensivo de errar numa
estimativa de tamanho; registrado porque a diferença é o dobro.

**O cache paga 43 vezes o que custa**: 21 ms contra 1,26 s. E a chave é a
**versão do TypeScript**, não o projeto: dois projetos com o mesmo TypeScript têm
a mesma biblioteca, e gravar por projeto guardaria a mesma coisa várias vezes.

*O formato é de linhas, e não JSON: `CompletionItem` mora em `ide-domain`, que é
neutro e não conhece serialização — pô-la lá por causa de uma linguagem é o que a
`12` recusou. E cache que se lê a olho nu se conserta; um binário corrompido vira
relatório de defeito sem pista.*

#### O `string` minúsculo, que quase ficou de fora

`const nome: string` é a anotação mais comum que existe, e o `string` da
linguagem **não é** um nome declarado em lugar nenhum: quem declara `charAt`,
`trim` e `split` é a `interface String` do `lib.es5.d.ts`. Sem a tradução —
`string` para `String`, `number` para `Number`, e mais três — o ponto sobre
metade das variáveis de um projeto continuaria dizendo "não sei", com a
biblioteca inteira carregada ao lado.

`any`, `unknown`, `void` e `never` ficam de fora de propósito: não há interface
por trás deles, e inventar uma seria oferecer membros que ninguém declarou.

#### O que ela não resolve, e é preciso dizer antes

**Genéricos.** Depois de `w.forEach(x => x.`, o `x` é `T`, e trocar `T` por
`String` é instanciar genérico — o verificador de tipos, recusado pela ADR-025.
O ponto sobre a variável responde; o ponto dentro da lambda continua descendo
para o analisador.

*Havia aqui um segundo item — método não guardava o tipo de retorno, porque a
gramática o põe em `return_type` e a extração lia `type`. Foi um teste desta fase
que o encontrou, e ele **foi corrigido antes da fase 8**, que depende dele. Ver
lá o porquê.*

**Fora isso, ela resolve tudo o que o TypeScript declara**, e não um punhado de
tipos escolhidos a dedo: `string`, `number`, `Array`, `Date`, `Promise`, `Map`,
`Set`, `RegExp`, `JSON`, `Math`, `Object`, e os 2 136 do DOM — `HTMLElement`,
`Event`, `Response`, `FormData`. Hoje **todos** descem para o analisador.

Escolher a dedo seria a tabela por biblioteca que a `23` proibiu, e seria pior
aqui: a lista envelheceria a cada versão do TypeScript, e quem a mantivesse
decidiria por engano o que quem usa a IDE alcança. Ler o que veio instalado não
tem essa escolha para fazer.

#### Critério, e o que ele achou

`let w: String[];` e um ponto trazem `forEach`, `map` e `length` sem o analisador
subir; `String`, `Number`, `Date`, `Promise`, `Map`, `RegExp`, `HTMLElement` e
`Response` respondem contra o TypeScript de verdade, o que é o que prova que a
fase não é sobre um tipo. Um projeto com `"target": "ES2022"` **não** oferece o
que só existe em ES2024, ainda que a declaração esteja no cache — e os dois
projetos compartilham o mesmo arquivo de cache, que é o desenho.

**Três defeitos vieram dos testes, e nenhum teria aparecido lendo o código:**

- **`no-default-lib="true"` entrava como um `lib` chamado `true`.** Todo arquivo
  da biblioteca abre com essa linha, e procurar `lib="` solto colhe o `true`
  dela. Nenhum `lib.true.d.ts` existe, então nada quebrava — o engano só punha um
  nome inventado dentro do alcance, calado, esperando o dia em que alguém
  contasse quantos `lib` valem;
- **os testes se diziam TypeScript `5.9.3`**, que é a versão instalada nesta
  máquina. Como o cache é chaveado pela versão, rodar a suíte gravaria quatro
  tipos de mentira por cima da biblioteca de verdade, e o projeto seguinte
  abriria com eles. Os testes agora dizem `0.0.0-teste`, que não pode existir;
- **`wait_until_indexed` respondia `true` sem esperar.** É o padrão do contrato —
  "não há o que esperar" —, e este provider monta índice e biblioteca em outra
  linha de execução. Na IDE não aparecia, porque quem espera de verdade lê o
  sinal de prontidão direto; apareceu num teste, que sem isso mediria a corrida
  entre duas threads em vez do que o código faz.

#### Quanto ela rendeu, e é menos do que eu esperava

O instrumento agora fica no repositório — `tests/cobertura.rs`, com a chave
`ER_IDE_PROJETO_TS`. O número antigo veio de um arranjo que não foi guardado, e
por isso a comparação de então dependia de eu lembrar como tinha contado.

**A prova de que o instrumento conta o mesmo:** na aplicação ele achou **397**
pontos, contra os 402 da fase 4. Mesmo projeto, mesma ordem de grandeza, mesma
regra — cada `.` que segue um nome.

> ⚠️ **E o "mesmo" era o mesmo erro.** A fase 8 descobriu que essa contagem
> incluía os pontos **dentro de aspas** — `'./pagina.html'` valia como pergunta.
> Corrigida, a aplicação tem **179** pontos, e não 397. Os números desta seção
> ficam como foram medidos, e não são comparáveis com os da fase 8; ver lá.

Medido com e sem a biblioteca, **no mesmo instrumento e na mesma amostra**:

| projeto | pontos | sem a biblioteca | com | ganho |
|---|---|---|---|---|
| monorepo de biblioteca | 9 438 | 23,2% | **25,8%** | +2,5 pontos |
| aplicação | 397 | 12,6% | **12,8%** | +1 ponto de 397 |

**Um ponto em 397.** A fase custou 1 525 tipos, 410 KB de cache e um módulo em
cada metade da crate, e na aplicação de verdade ela respondeu **um** ponto a
mais. No monorepo rendeu dez vezes mais — 239 pontos —, e mesmo lá são 2,5
pontos percentuais.

Está escrito porque é verdade, e porque a fase parecia obviamente boa antes de
ser medida. É a mesma lição que esta especificação já registrou seis vezes por
outros nomes: **a estimativa e a medição discordam, e quem manda é a medição.**

#### E a medição disse onde o ponto morre de verdade

O gargalo não é a tabela de tipos: é o **receptor**.

| projeto | "não sei" | dos quais, segundo elo de uma cadeia |
|---|---|---|
| monorepo | 7 006 | 1 636 (**23%**) |
| aplicação | 346 | 102 (**29%**) |

Em `this.svc.buscar`, o `this.svc` é alcançável e o resto não — saber o tipo de
um membro exige resolver o membro, que é um passo além do que a fase 4 entregou.
**Um quarto de tudo o que a IDE não responde está aí**, e num componente Angular
é o gesto mais comum que existe: quase tudo se acessa por `this.`.

E é aqui que esta fase deixa de parecer desperdício: um campo `nome: string`
acessado por `this.nome.` só vira resposta se **as duas** coisas existirem — o
elo da cadeia e o tipo da linguagem. A biblioteca sozinha rende pouco porque
quase ninguém escreve `const nome: string` e usa `nome.` logo abaixo; ela rende
quando o receptor for alcançável.

*Isso não desfaz o número acima: a fase 7 sozinha entregou 2,5 pontos e 1 ponto.
O que a medição diz é qual é a próxima fase, e que ela agora tem por que ser
feita.*

### Correções que vieram desta fase

Duas, achadas antes de ela começar, ambas com o mesmo `let w: String[]`.

**A completação esperava na thread da interface.** Ver a fase 5 acima: era o sexto
lugar, e a frase que dizia não haver um sexto foi corrigida lá. Quem digitava
depois do ponto via as letras aparecerem todas de uma vez no fim — elas ficavam
na fila da janela, porque nenhum quadro era desenhado. Agora a tecla posta a
pergunta e volta.

*E a correção abriu um buraco que só apareceu ao ser pensada:* a resposta do
ponto chega **vencida** quando a letra seguinte já foi digitada, e descartá-la
deixava a lista fechada para sempre — a letra não pede nada, porque só a lista
já aberta dispara o pedido seguinte. A espera síncrona escondia isso abrindo a
lista antes de a letra ser processada. O que venceu vira pergunta nova, feita de
onde o cursor está.

**`Pedido[]` dizia `Pedido`, e `Observable<Pedido>` também.** O primeiro era o
`[]` jogado fora; o segundo era a pilha de visita retirando os filhos ao
contrário, que chegava ao **argumento** do genérico antes do nome dele — o
oposto do que a própria documentação da função prometia.

Hoje isso passava despercebido porque nem `Array` nem `Observable` estão no
índice, e a pergunta descia para o analisador. **A fase 7 é o que os traz para
dentro** — e no dia em que ela chegasse, `pedidos.` responderia os membros de um
pedido, e `fluxo.` os do conteúdo do `Observable`. Resposta errada, que é pior do
que "não sei": a fase teria estreado com um defeito que não seria dela.

É o que este projeto já viu várias vezes por outros nomes — o defeito que dorme
porque o caminho que o acorda ainda não existe.

## O que esta especificação não fará

- **verificador de tipos**: ADR-025, e reafirmada aqui com o exemplo do `pipe`;
- **tabela por biblioteca**: proibida pela `23`;
- **substituir o analisador externo**: o grupo 3 é código idiomático de Angular,
  e perdê-lo em silêncio seria pior do que a memória que se economiza;
- **templates de Angular**: é a `24`, e ela depende do analisador por razões
  próprias.
