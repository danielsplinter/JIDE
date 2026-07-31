# 17 — Adoção do arranjo

## Situação

A IDE calcula **todas** as suas áreas por conta própria e entrega retângulos
prontos ao anfitrião, por `UiHost::place`:

| onde | o que calcula |
|---|---|
| `layout.rs` (`shell_geometry`) | a moldura inteira: título, barra de atividades, barra lateral, editor, terminal, painel de depuração |
| `ide_shell/geometry.rs` | as áreas das Configurações e da inspeção |
| `FormLayout` (na ERLibUi) | campos, legendas e a fileira de ações dos diálogos |
| cada superfície | a conta final, somando margens sobre o painel |

Enquanto isso, a biblioteca tem motor de arranjo — `TaffyLayoutEngine`, atrás do
contrato `LayoutEngine` — e o anfitrião já sabe usá-lo: `UiHost::layout` calcula,
posiciona cada componente e publica o instantâneo. **A IDE não o usa.** O `place`
foi criado justamente para adiar esta decisão, e está escrito na especificação
`16` como o que não mudava ali.

## Motivo

Três coisas decorrem de a aplicação ser dona do arranjo:

- **a camada é grande e duplicada.** As mesmas margens aparecem no cálculo e na
  pintura, e já divergiram antes: foi o que motivou o `FormLayout`, quando cinco
  janelas tinham medidas que deviam ser iguais e não eram;
- **não há resposta a restrição.** Tudo é conta fixa sobre o tamanho da janela.
  Uma barra lateral com largura mínima, um painel que encolhe até um limite e
  então empurra o vizinho — nada disso se expressa; se expressa em `LayoutStyle`;
- **o instantâneo já existe e é subaproveitado.** O `LayoutSnapshot` responde
  acerto e área, mas hoje é preenchido à mão pelo `place`. Com o motor, ele passa
  a ser derivado, e a IDE deixa de ter como discordar de si mesma.

## Fase 0 — Medir, antes de decidir ✅

**Esta fase era um portão.** O `TaffyLayoutEngine` recalcula o arranjo **inteiro**
a cada quadro, e isso nunca tinha sido medido com a tela cheia da IDE.

Três medições, em `release`, numa janela de 1600×900:

| o quê | onde | resultado |
|---|---|---|
| o quadro da IDE **hoje** | `ide-ui/benches/frame.rs` | 125 µs — **0,7%** do orçamento |
| só o motor, com medição de graça | `ui-layout-taffy/benches/ide_screen.rs` | 143 nós → 510 µs |
| o caminho inteiro, com componentes e fonte reais | `ui-host/benches/ide_screen.rs` | 129 folhas → 624 µs — **3,7%** |

O caminho inteiro é o número que decide: `UiHost::layout` com `Label` de verdade
e `CosmicTextEngine` medindo pela mesma fonte que desenha. Como ele escala:

| folhas | primeiro quadro | quadro estável | orçamento de 16,7 ms |
|---|---|---|---|
| 129 | 4,4 ms | 624 µs | 3,7% |
| 289 | 7,0 ms | 1,68 ms | 10,1% |
| 1089 | 23,9 ms | 4,88 ms | 29,3% |

**Veredito: seguir.** 624 µs está abaixo do limite de 1 ms que esta fase fixou
antes de medir. Adotar o motor multiplica o custo do quadro por cinco — de 125 µs
para ~625 µs — e ainda assim sobra 96% do orçamento.

### O que a medição corrigiu

**A causa do custo não é a que a especificação `16` supôs.** Ela registrou o risco
como "o Taffy recalcula o arranjo inteiro" e apontou a invalidação do `WidgetTree`
como remédio. Medido, o custo é outro: comparando a segunda linha com a terceira,
**510 dos 624 µs são estruturais** — e o `compute` do adaptador faz
`TaffyTree::new()` e reconstrói **todos** os nós a cada chamada, porque o contrato
`LayoutEngine` é sem estado.

Ou seja: o gasto é **construir a árvore**, não executar o algoritmo, e a medição
de texto — memorizada no `CosmicTextEngine` — custa pouco depois do primeiro
quadro. Invalidar o `WidgetTree` não ajudaria: o adaptador reconstruiria tudo de
qualquer jeito. **Se o custo aparecer, o remédio é guardar a árvore do Taffy entre
quadros no adaptador**, e não pular nós na árvore de widgets. Fica registrado, e
não é pré-requisito de nenhuma fase — a folga é grande demais para pagar essa
complexidade agora.

### As duas condições que a medição impõe

- **o explorador tem que continuar virtualizado.** Com mil linhas na árvore, o
  arranjo passa a 4,9 ms, quase um terço do orçamento. Só as linhas visíveis podem
  virar nó — o que já é o comportamento de hoje, e agora é uma restrição escrita;
- **o primeiro quadro paga a moldagem**: 4,4 ms na tela realista, 23,9 ms com mil
  linhas. É custo de texto novo, não de arranjo, e some quando a medida entra no
  cache. Importa para quem muda muito texto de uma vez — abrir um projeto, trocar
  de aba — e é o que a fase 3 deve observar quando a moldura virar árvore.

### Como refazer

```text
cargo bench -p ui-host --bench ide_screen
cargo bench -p ui-layout-taffy --bench ide_screen
cargo bench -p ide-ui --bench frame
```

O número volta a ser medido ao final da fase 3, quando a árvore já for a real.

## Fase 1 — O vocabulário, na ERLibUi ✅

O gerenciador que existe hoje é um `BoxLayout`: `Row`, `Column`, `Overlay`, com
`gap`, `padding` e `flex_grow`. É o bastante para a moldura, e **não** é o
bastante para o que a IDE calcula à mão. Faltam quatro coisas, e sem elas a
conversão ou trava ou recria número mágico dentro do estilo:

| falta | para que serve | o equivalente em Java |
|---|---|---|
| alinhamento no eixo principal e no cruzado | centralizar a fileira de ações de um diálogo, encostar um rótulo à direita | `FlowLayout.CENTER`, `GridBagConstraints.anchor` |
| margem e borda **por lado** | as margens assimétricas que hoje somam à mão, e a linha de 1px que ficou pendente da `15` | `EmptyBorder(topo, esq, base, dir)` |
| mínimo e máximo | largura mínima da barra lateral, altura mínima do terminal | `getMinimumSize`, `getMaximumSize` |
| quebra de linha | fileiras que dobram quando a janela estreita | `FlowLayout` |

Os quatro já existem no Taffy — `align_items`, `justify_content`, um `Rect` de
comprimentos para padding e borda, `min_size`/`max_size`, `flex_wrap`. É
mapeamento no adaptador, não motor novo. O `LayoutStyle` cresce, e o `Default`
continua sendo o comportamento de hoje, para nenhuma tela existente mudar de
lugar.

**Critério:** cada campo novo com teste no `ui-layout-taffy` provando o arranjo
que ele produz; showcase inalterado.

**Feito.** `LayoutStyle` ganhou `wrap`, `main_align`, `cross_align`, `border` e os
quatro limites, e o `padding` virou `EdgeInsets`. Onze testes novos no
`ui-layout-taffy` — 171 para 182 na ERLibUi —, um por campo, mais dois que fixam o
que **não** pode mudar: o padrão continua encostando os filhos no começo e
esticando na transversal, que é o que o motor já fazia. Espaço negativo é lido
como nenhum, em vez de virar erro.

A IDE não mudou uma linha e segue com 316 testes: o `Default` preserva o
comportamento anterior, que era a condição para esta fase não mexer em tela
nenhuma. Documentado em `05-layout.md` e na ADR-021 da biblioteca.

## Fase 2 — Os diálogos 🔶

As seis janelas modais primeiro: são fechadas, de tamanho fixo, e o `FormLayout`
já descreve o que elas têm. Cada uma vira uma árvore — coluna de campos, fileira
de ações — declarada em `LayoutStyle`.

**Critério:** o `FormLayout` não é usado pela IDE; nenhuma superfície chama
`place`.

### Passo 1 — O anfitrião aceita os dois arranjos ✅

`place` e `layout` eram **mutuamente exclusivos**, e estava escrito assim no
próprio método: *"ou o arranjo é do motor, ou é do consumidor"*. Com essa regra
não existe adoção por fases — a primeira janela migrada obrigaria todas as outras
a migrar no mesmo quadro.

O `UiHost` passa a guardar as áreas declaradas por `place` à parte do instantâneo
e a **reaplicá-las depois** de calcular: o que o consumidor posicionou vence o que
o motor calculou, até que `unplace` o devolva ao motor. Dois testes fixam as duas
metades da regra, e a ERLibUi vai a 184.

Isto não é andaime: é o estado final. A fase 5 mantém `place` para o console e o
editor, que derivam área de conteúdo virtualizado.

### Passo 2 — A ordem das camadas sai da chamada e vai para a árvore ✅

**O que a execução mostrou, e que esta especificação não previa.** A ordem de
sobreposição hoje é a **ordem das chamadas** de `place`: `place_overlay` limpa
tudo a cada quadro e reposiciona na ordem de `OVERLAY`, e é daí que sai a
propriedade em que a `16` se apoiou — *quem é declarado por último engole o
gesto*.

No momento em que `layout` passa a rodar, a ordem deixa de vir da chamada e passa
a vir da **árvore**. E os nós das janelas são criados pelo próprio `place`, na
ordem em que cada janela foi aberta pela primeira vez na sessão — que é
arbitrária. Converter **uma** janela, portanto, não é mudança isolada: mexe na
ordem de todas.

Por isso as seis camadas precisam ser declaradas na árvore **uma vez, na ordem de
`OVERLAY`**, antes de qualquer janela adotar o motor. Era trabalho que esta
especificação tinha posto na fase 3, e ele é pré-requisito da 2.

**Critério:** os nós de camada existem desde a construção do shell, na ordem de
`OVERLAY`; `place_overlay` não os cria mais.

**Feito.** O `UiHost` ganhou `declare` — nó com lugar e estilo, sem componente,
que é o que uma camada é — e `children`, para a ordem ser verificável. O
`new_host` da IDE percorre `OVERLAY` e declara as sete camadas na construção; o
`place` deixa de criá-las, porque as encontra prontas.

Um teste de cada lado: no `ui-host`, que um nó declarado ocupa lugar sem
componente; na IDE, que os filhos da raiz são exatamente `OVERLAY`. O segundo é o
que impede a regressão silenciosa — sem ele, a primeira janela a adotar o motor
herdaria a ordem em que as janelas foram abertas.

Nenhum pixel mudou: enquanto tudo continua vindo de `place`, a ordem do quadro
segue sendo a das chamadas. ERLibUi 185, IDE 317.

### Passo 3 — A ordem da pintura passa a ser a da árvore ✅

**Segunda descoberta da execução.** O passo 2 alinhou a ordem da *árvore* à de
`OVERLAY`, e isso não bastou — porque a ordem que vale hoje é a do **instantâneo**,
e ela é montada por ordem de inserção.

Ligar o motor mostrou o problema em onze testes de uma vez: o `collect_layout`
percorre a árvore e insere na ordem dela, mas as áreas de `place` são reaplicadas
**depois** e vão para o fim da fila. O resultado é uma pintura em dois blocos —
tudo o que o motor calculou, e só então tudo o que o consumidor posicionou —,
quando a ordem correta intercala os dois: cada camada seguida do que há dentro
dela.

Enquanto isso valer, converter uma janela põe os filhos dela acima ou abaixo de
tudo, conforme o lado em que caírem. **Não é defeito da conversão; é do
instantâneo.**

O que falta é o `LayoutSnapshot` deixar de ordenar por inserção e passar a
ordenar pela árvore, com a área vindo do motor ou da declaração conforme o nó. Aí
os dois arranjos convivem sem que a ordem dependa de quem chegou primeiro.

**Critério:** com o motor ligado e nenhuma janela convertida, os 317 testes
continuam passando — que é a prova de que a ordem não mudou.

**Feito.** O `UiHost` deixa de montar o instantâneo por ordem de inserção e passa
a percorrer a árvore: para cada nó, a área é a do consumidor se houver, senão a do
motor; quem não tem nenhuma das duas está fora do quadro, e junto com ele sai o
que estiver dentro. Os dois arranjos passam a se intercalar em vez de sair em dois
blocos.

Do lado da IDE, três coisas fecham a conta:

- as camadas alternam por `hidden` em vez de entrar e sair da árvore, o que mantém
  a ordem independente do uso;
- a moldura — abas do editor e do terminal — é declarada **antes** das camadas,
  porque é o que elas cobrem. Sem isso ela nasceria no fim da lista, acima das
  janelas, que foi o defeito que sobrou dos onze;
- a lista da completação entra logo depois da camada dela.

O `place_overlay` termina chamando `host.layout(size)`. **Os 317 testes continuam
passando**, e o quadro estável foi de 125 µs para 132 µs — os 5% de aumento são o
motor calculando uma árvore que ainda é quase toda posicionada à mão.

O guarda da ordem foi reescrito **por propriedade**: as camadas aparecem na ordem
de `OVERLAY` entre os filhos da raiz, e a moldura vem antes da primeira. A primeira
versão comparava a lista inteira e quebrou assim que a moldura entrou — pela
terceira vez nesta especificação, um critério escrito pelo símbolo em vez da
propriedade.

### Passo 4 — As janelas, uma por vez 🔶

Com a ordem estável, cada janela troca `place` por estilo: a camada é a área cheia
com `main_align` e `cross_align` centralizados, o painel é o filho de tamanho
fixo, e dentro dele a coluna de campos e a fileira de ações — que é onde o
`FormLayout` deixa de ser necessário.

**Critério:** nenhuma das seis chama `place`; `FormLayout` sem uso na IDE.

**Renomear, feita.** É o padrão que as outras cinco repetem:

- `attach` deixa de anexar dois botões e passa a **declarar a janela inteira**: o
  painel é uma coluna sob a camada, o campo tem altura fixa, a lista fica com
  `flex_grow`, e a fileira de ações é uma linha com `MainAlign::End` — que é o que
  encosta os botões à direita sem ninguém subtrair larguras;
- `place_widgets` e a função `geometry` **deixaram de existir**. Quem precisa de
  área pergunta ao anfitrião;
- `RenameGeometry` e a chamada ao `FormLayout` saíram junto.

**O que mudou de posição, e por quê.** O `FormLayout` usava folgas diferentes
entre as peças; a coluna usa uma só, de 34 px. Com isso a lista ficou **18 px mais
curta** e a fileira de ações recuou **8 px** da borda direita, porque agora ela
respeita a mesma margem de 24 px do campo em vez dos 16 px que usava. Os 317
testes continuam passando — eles perguntam a geometria em vez de afirmar
coordenadas, que é o que a decomposição das especificações `14` a `16` deixou
pronto.

**Busca, feita — e sem mover um pixel.** A coluna reproduz exatamente o que a
conta fazia: margem 16, campo a 56 do alto, folga 12, e a lista com `flex_grow` no
lugar da altura subtraída. `panel_geometry` e `geometry` saíram; o teste que
apontava a roda para dentro da lista passou a perguntar a área ao anfitrião.

Ela é a prova de que a mudança de pixel da `rename` **não é inerente** à adoção —
veio de o `FormLayout` usar folgas diferentes onde a coluna usa uma só. Onde a
conta original já era uniforme, a declaração a reproduz.

**Criação, feita — e também sem mover um pixel.** As folgas do `FormLayout` aqui
não eram uniformes: 30 px entre os dois campos, e o que sobrar até a fileira de
ações. A coluna diz isso com duas peças vazias — uma de altura fixa entre os
campos, outra com `flex_grow` empurrando as ações para o pé. É mais honesto do que
a soma escondida na posição do campo seguinte, e dá o mesmo resultado: campo em
76, o segundo em 140, ações em 182.

Dois tropeços, ambos meus e ambos instrutivos:

- **inventei ids `10_044..10_046` e eles eram da janela de inspeção.** Cinco
  testes de inspeção caíram de uma vez. A faixa `10_4xx` é a das janelas
  convertidas, e é onde eles deviam ter nascido;
- um teste lia a área do campo **antes de qualquer quadro**. Antes ela era
  calculada sob demanda; agora vem do arranjo, e arranjo acontece no quadro. O
  teste passou a desenhar um antes de perguntar — o que é mais fiel ao que a
  aplicação faz.

**Gerar, feita — e também sem mover um pixel.** Conteúdo a 56 do alto, 16 de
margem, 12 de folga até as ações, botões de 100 por 36 porque "Gerar todos" não
cabe no tamanho padrão: tudo declarado, e a lista com `flex_grow` no lugar da
altura subtraída. O `panel_bounds` da janela ficou sem uso e saiu junto.

Os sete pontos dos testes que pediam as áreas passaram a desenhar um quadro antes
de perguntar — o mesmo ajuste da `new_item`, e pela mesma razão: área agora vem do
arranjo, e arranjo acontece no quadro.

**Inspecionar, feita — e é a que mais mostra o ganho.** Ela não é uma coluna de
campos: é uma linha de duas colunas, com a árvore de objetos à esquerda e, à
direita, o detalhe em cima e o editor embaixo. As frações de largura e de altura
agora dividem o espaço **uma vez**, na declaração, em vez de aparecer no cálculo
de cada peça. A fileira de ações tem larguras diferentes — Executar tem 98,
Fechar tem 88 — e o alinhamento à direita resolve as duas sem conta nenhuma.

Sem mover um pixel: `56 + 308 + 8 + 34 + 14 = 420`, e a fileira reproduz `508` e
`616` exatamente. `inspection_geometry` e a `InspectionGeometry` saíram da
`geometry.rs`; a struct sobrevive só dentro da `inspection`, sob `cfg(test)`, como
a forma que os testes usam para apontar um gesto.

**Falta uma:** `settings`.

A `settings` é diferente das cinco em espécie, e não em tamanho: ela tem uma barra
lateral de páginas e um conteúdo cujo arranjo **muda com a página escolhida** —
combos e botões em deslocamentos próprios (126, +46), campos de depuração. Não é
uma árvore só; é uma por página.

**A moldura, feita.** Painel, barra de páginas e fileira de ações são declarados;
o interior de cada página continua sendo conta, e é a etapa seguinte. A barra
desce até o pé do painel, por baixo das ações — por isso ela é **irmã** da coluna
da direita, e não parte dela, e é essa estrutura que preserva os pixels:
`780 - 210 - 16 = 554` de conteúdo, Salvar em `676`, Cancelar em `578`, os mesmos
do `FormLayout`.

Com isso o **`FormLayout` deixa de ser usado pela IDE**. O alinhamento à direita
faz o que ele fazia, e a `geometry.rs` sobrevive só com o interior das páginas.

**Duas mudanças que precisam ser declaradas:**

- **a ordem do quadro mudou.** Quem posiciona à mão passou a ler a moldura do
  arranjo, e ler antes de calcular daria o quadro anterior — a página de
  depuração abria vazia na primeira vez. O `place_overlay` agora faz: estilos das
  camadas, **arranjo**, e só então as áreas que a IDE ainda calcula;
- **o quadro estável foi de 132 µs para 301 µs** — 1,8% do orçamento. São dois
  arranjos por quadro, um antes e um depois das áreas declaradas à mão. O segundo
  desaparece quando não sobrar quem as declare: é dívida da transição.

**As páginas, feitas.** A lista de páginas mora na barra e tem a altura da
contagem; a página de depuração é uma coluna que **entra e sai do arranjo** com a
escolha, em vez de ser posicionada quando aparece. O que muda com o estado é
declarado **antes** do arranjo, por `sync_declaration` — declarar depois valeria
só no quadro seguinte, e foi assim que a página de depuração abriu vazia uma vez.

Com isso o `place_surface` deixou de existir: **nenhuma janela chama `place`**. Os
cinco `place` que restam são a moldura — abas do editor e do terminal — e a
completação, que é a fase 5.

**Uma mudança de comportamento que precisa ser dita:** a geometria das
Configurações agora é a **do que está na tela**. Antes era uma conta sobre o
painel, e respondia por peças invisíveis — dava a área dos campos de depuração com
a página de Java aberta. Um teste dependia disso: capturava as áreas uma vez e as
usava depois de trocar de página. Agora ele relê, que é o que a aplicação faz. Nenhuma
novidade de mecanismo — é o mesmo recorte. A `settings` é a maior, porque tem
lista de páginas própria; `new_item` e `generate` usam `FormLayout` e devem mover
os mesmos pixels que a `rename` moveu.

## Fase 3 — A moldura

`shell_geometry` vira árvore: linha com barra de atividades, barra lateral e
conteúdo; coluna com título, editor e terminal. É aqui que a resposta a restrição
aparece — largura mínima da barra lateral, altura mínima do terminal — e onde os
divisores deixam de mover números para mover **restrições**.

**Critério:** `layout.rs` sem `shell_geometry`.

## Fase 4 — Os painéis

Painel de depuração, Explorer e terminal. O **editor de código entra por último**
e provavelmente como nó opaco: ele tem rolagem própria, virtualização e cache de
texto moldado, e não deve participar do arranjo interno.

**Critério:** `ide_shell/geometry.rs` não existe mais.

## Fase 5 — O que sobra de `place`

O `place` continua legítimo para o que é área derivada de conteúdo virtualizado —
a saída do terminal, que sabe quantas linhas cabe, e o editor. O que não pode
sobrar é área de **estrutura** calculada à mão.

**Critério:** nenhuma chamada a `place` fora de console e editor.

## O risco que separa esta especificação das anteriores

As `14`, `15` e `16` não mudaram um pixel, e a regra era essa: fase que altera um
teste alterou comportamento, e é motivo para desfazer.

**Aqui a regra não vale.** Trocar contas fixas por um motor de arranjo **move
pixels** — margens que se somavam noutra ordem, arredondamentos, alturas
derivadas de conteúdo em vez de fixadas. Os testes que apontam gestos por área
calculada seguem junto, porque perguntam a geometria; os que afirmam coordenadas
exatas vão mudar.

Por isso cada fase precisa declarar **o que mudou de posição e por quê**, e a
contagem de testes deixa de ser invariante. É a diferença entre reorganizar e
redesenhar, e esta especificação redesenha.

## O que continua da IDE

A composição: quais janelas existem, o que há dentro de cada uma, e o que cada
gesto significa no domínio. O arranjo passa a ser declarado — em `LayoutStyle` —,
não calculado.

## Verificação

Cada fase termina com `cargo test --workspace` e
`cargo clippy --workspace --all-targets -- -D warnings` nos dois repositórios,
mais o número da fase 0 refeito ao final da fase 3, quando a árvore já é a real.
