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

### Fase 2 — Resolução de módulos ⬜### Fase 2 — Resolução de módulos ⬜

A parte cara, e a que decide se as fases 3 e 4 existem.

- `import` relativo;
- `paths` e `baseUrl` do `tsconfig.json` — 315 entradas no projeto de referência;
- barris: seguir a reexportação até a declaração, com ciclo detectado.

**Critério:** para uma amostra de nomes importados do projeto real, o arquivo que
o índice aponta é o mesmo que o analisador aponta. Divergência é defeito nosso.

**Se esta fase não convergir, a especificação para aqui** — e o que a fase 1
entregou continua valendo sozinho, que é o motivo de ela vir primeiro.

### Fase 3 — Definição e referências ⬜

Grupo 2, sem tipos ainda.

**Critério:** `Ctrl+clique` num nome importado abre a declaração certa, e não a
primeira com aquele nome. O teste que importa é o do nome repetido em dois
módulos.

### Fase 4 — O `.` declarado ⬜

- tabela de membros por tipo, incluindo herança;
- tipo do receptor para as quatro formas declaradas;
- **e a terceira resposta**: tipo desconhecido é diferente de tipo sem membros.

**Critério:** num componente Angular real, `this.` e um parâmetro de construtor
injetado completam com os membros certos, sem o analisador de pé. E `.pipe(map(x
=> x.` responde **"não soube"**, e não uma lista vazia.

### Fase 5 — Subir o analisador sob demanda ⬜

Abrir um `.ts` deixa de subir o analisador. Ele sobe na primeira pergunta que o
índice devolveu como desconhecida.

**Critério:** abrir um projeto e navegar por arquivos com tipos declarados mantém
a memória externa em zero. Tocar num `Observable` sobe o analisador, e a barra de
estado mostra os dois números mudando.

## O que esta especificação não fará

- **verificador de tipos**: ADR-025, e reafirmada aqui com o exemplo do `pipe`;
- **tabela por biblioteca**: proibida pela `23`;
- **substituir o analisador externo**: o grupo 3 é código idiomático de Angular,
  e perdê-lo em silêncio seria pior do que a memória que se economiza;
- **templates de Angular**: é a `24`, e ela depende do analisador por razões
  próprias.
