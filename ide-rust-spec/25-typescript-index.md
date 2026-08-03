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

## O que se mediu antes de propor

Toda afirmação abaixo tem número, e todos vieram do `spartacus-develop`.

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

| forma | frequência |
| --- | --- |
| `constructor(private svc: LoginService)` | o padrão de injeção de dependência |
| `const p: Pedido` | anotação explícita |
| `const p = new Pedido()` | inferência trivial |
| `this.` dentro da classe | onipresente |

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

### Fase 1 — O índice, e a busca por tipo ⬜

O grupo 1 inteiro, sem resolução de módulos.

- varredura do projeto pelas raízes que o `tsconfig.json` já entrega (ADR-027);
- tabela de declarações no formato do índice Java: nome, tipo, posição, arquivo
  por número;
- **o índice nasce em disco**, e a consulta lê só os bytes de que precisa. Não há
  fase em que ele viva em memória "por enquanto": construir residente e migrar
  depois é o caminho que a `20` percorreu, e ela mediu 178 MB antes de chegar aos
  103 MB;
- `workspace_types` respondido do índice.

**Critério:** `Ctrl+L` num projeto **sem `node_modules`** encontra os tipos, em
menos de 50 ms, com o processo da IDE crescendo **menos de 10 MB** em relação ao
que ele ocupa sem índice nenhum — é o critério que separa "índice em disco" de
"índice residente", e sem ele a fase pode ser dada por cumprida do jeito errado.
O número de resultados bate com o que o analisador devolve no mesmo projeto
**com** `node_modules`. A comparação com o analisador é o teste —
o mesmo desenho da ADR-027, em que a divergência é defeito nosso.

### Fase 2 — Resolução de módulos ⬜

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
