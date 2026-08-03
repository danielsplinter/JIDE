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
Um índice na mesma forma ficaria na casa das **dezenas de MB**.

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

| postura | memória | custo |
| --- | --- | --- |
| índice responde, sem analisador | ~**60 MB** | sem `.` nos casos do grupo 3 |
| índice na frente, analisador sob demanda | 60 MB até o primeiro caso do grupo 3 | nenhum, mas o ganho é adiamento |
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

### Fase 0 — Poder desligar o analisador ⬜

Hoje não há como. Existe `enable`, não existe `disable`, e o analisador entra na
composição em `typescript_contribution::selection()`.

Sem isto, nenhuma fase seguinte pode ser medida: com o analisador de pé, não há
como saber o que o índice responde sozinho.

- um ajuste que tire o provider externo da seleção, sem recompilar;
- o aviso de degradação já existente cobre o resto: com ele desligado, o que só
  ele responde passa a dizer que não soube.

**Critério:** com o analisador desligado, um `.ts` abre com realce e estrutura, e
`Ctrl+L` diz que não há índice — em vez de dizer que não achou nada.

### Fase 1 — O índice, e a busca por tipo ⬜

O grupo 1 inteiro, sem resolução de módulos.

- varredura do projeto pelas raízes que o `tsconfig.json` já entrega (ADR-027);
- tabela de declarações no formato do índice Java: nome, tipo, posição, arquivo
  por número;
- `workspace_types` respondido do índice.

**Critério:** `Ctrl+L` num projeto **sem `node_modules`** encontra os tipos, em
menos de 50 ms, e o número de resultados bate com o que o analisador devolve no
mesmo projeto **com** `node_modules`. A comparação com o analisador é o teste —
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
