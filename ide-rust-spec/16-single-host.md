# 16 — Um anfitrião só

## Situação

A adoção do runtime da ERLibUi (especificação `15`) parou num meio-termo
deliberado: **cada janela tem o seu `UiHost`**. Seis anfitriões, um por
superfície, cada um com dois ou três nós.

Isso resolveu o que precisava resolver — o clique deixou de ser teste de retângulo
e os botões viraram componentes de verdade — mas deixou de fora tudo o que só um
anfitrião **único** entrega:

- **o funil continua vivo.** `SurfaceKind`, `SURFACES`, `COMPLETION_DEPTH` e os
  seis `surface_*` existem para responder "de quem é este gesto", que é
  exatamente a pergunta que a ordem da árvore responde sozinha;
- **as abas perdem o estado a cada quadro.** `editor_tabs()` e `terminal_tabs()`
  constroem um `Tabs` novo a cada uso, a partir da sessão. A troca por identidade
  do anfitrião — que transplanta o `InteractionState` — existe e está testada
  desde a fase 3 da `15`, e não é usada por elas. O `with_pointer` é o remendo
  dessa perda;
- **a entrega de `FocusGained`/`FocusLost` continua na IDE**, marcada como
  temporária desde a fase 1 da `15`. Ela é do anfitrião, que é quem tem o mapa de
  id para componente;
- **sobraram três `contains(point)`**: dois decidindo se a roda do mouse é da
  lista (`generate`, `type_search`) e um guardando a lista de páginas das
  Configurações.

## Motivo

Um anfitrião por janela é uma tabela de profundidade disfarçada: o shell ainda
precisa saber qual janela está aberta para escolher a qual entregar o evento.
Com um anfitrião só, a profundidade **é** a posição na árvore, e a pergunta
desaparece em vez de mudar de lugar.

A verificação disso já foi feita: o teste
`a_modal_declared_last_swallows_the_gesture`, no `ui-host`, mostra que a janela
declarada por último engole o gesto — desde que a raiz seja uma camada
(`LayoutDirection::Overlay`), implementada na mesma fase.

## Decisão

Migrar em cinco passos, do menor risco ao maior, **cada um terminando com a suíte
verde**. O critério de parada de cada passo é objetivo, e nenhum deles muda
comportamento.

### Passo 1 — O campo, e a lista de completação nele

O `IdeShell` ganha um `host: UiHost` com raiz em camada. O primeiro e único nó é
a **lista de completação** — um nó, que exercita o caminho inteiro: declarar,
posicionar por `place`, receber o clique, pintar.

Escolhida primeiro porque é a peça mais isolada e a que hoje precisa da
`COMPLETION_DEPTH` para saber que cobre a inspeção e é coberta pelas janelas de
tela cheia. Declarada no lugar certo da camada, a constante deixa de ter função.

**Critério:** `COMPLETION_DEPTH` não existe mais; 316 testes.

### Passo 2 — A primeira janela

`new_item` sai do anfitrião próprio para o do shell. É a mais migrada das seis: já
usa `FocusManager`, `FormLayout` e `place`, e o que muda é de quem é o anfitrião.

Aqui aparece a mecânica que os outros quatro passos repetem: os nós da janela
entram na árvore ao **abrir** e saem ao **fechar**, porque é a presença deles que
decide a sobreposição.

**Critério:** `new_item` sem `UiHost` próprio; 316 testes.

### Passo 3 — As outras cinco

`rename`, `generate`, `type_search`, `settings` e `inspection`, uma por vez, na
ordem de tamanho. Cada uma é um commit.

**Critério:** nenhuma superfície tem `UiHost` próprio.

### Passo 4 — O funil sai

Com todas as janelas na mesma árvore, `SurfaceKind`, `SURFACES` e os seis
`surface_*` perdem a razão de existir. As sete entradas de evento passam a
entregar ao anfitrião e a tratar o que ele devolve.

Saem junto: os três `contains(point)` restantes — a roda e o guarda das páginas
passam a ser resolvidos por acerto — e a entrega manual de foco, que vira
`push_focus_scope` ao abrir e `pop_focus_scope` ao fechar. Isso prende o `Tab`
dentro da janela aberta, o que hoje não acontece.

**Critério:** `ide_shell.rs` sem `SurfaceKind`; `grep contains(point)` vazio nos
módulos de janela.

### Passo 5 — As abas

`editor_tabs()` e `terminal_tabs()` deixam de construir e passam a **substituir**
por identidade, com `UiHost::replace`. O estado de interação atravessa, e o
`with_pointer` deixa de ser necessário.

É o passo que mais muda o que se vê: a aba sob o ponteiro passa a se destacar de
verdade, em vez de depender de a posição do ponteiro ter sido informada à mão.

**Critério:** `with_pointer` sem chamadas na IDE.

## Riscos, e o que fazer com cada um

- **o Taffy recalcula o arranjo inteiro a cada quadro.** Nunca foi medido com uma
  tela de IDE — árvore de arquivos virtualizada, editor, terminal e painel de
  depuração juntos. Medir **antes** do passo 3; se o custo aparecer, a invalidação
  do `WidgetTree` existe e ainda não é usada para pular trabalho;
- **o editor de código não é widget comum.** Tem rolagem própria, gesto contínuo e
  cache de texto moldado. Entra por último, e pode precisar ser um nó opaco que
  recebe o gesto bruto em vez de participar da propagação;
- **o transplante de estado esconde defeito.** Se um componente ganhar estado de
  edição e continuar sendo substituído a cada quadro, a perda é silenciosa. A
  regra — *widget que possui estado de edição é mutado, não substituído* — precisa
  virar teste no `ui-host` antes do passo 5.

## O que não muda

- **o arranjo continua da IDE.** `place` recebe as áreas que o `FormLayout` e o
  módulo `geometry` calculam. Trocar isso pelo motor de layout é outra decisão,
  para outra especificação;
- **a composição continua da IDE**: quais janelas existem, o que há dentro delas,
  e o que cada ação significa no domínio.

## Verificação

Cada passo termina com:

```text
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

nos dois repositórios. A contagem de testes da IDE não muda: **316**. Um passo que
precise alterar um teste alterou comportamento, e isso é motivo para desfazê-lo e
refazer — a mesma regra das especificações `14` e `15`.
