# 14 — Decomposição do `ide_shell`

## Situação

`crates/ide-ui/src/ide_shell.rs` tem **11.220 linhas**: 6.749 de código e 4.470
de teste. É o maior arquivo dos dois repositórios por uma ordem de grandeza — a
segunda maior unidade de código é `ui-components/src/ide.rs`, com 7.572, e o
segundo maior arquivo da IDE tem 4.688.

Dentro dele convivem, sem fronteira nenhuma, o estado de dez superfícies
diferentes — editor, Explorer, terminal, painel de depuração, e as janelas de
geração, renomeação, busca de tipo, criação de item, inspeção e configurações —,
o roteamento de todos os eventos de entrada, a pintura de tudo, e cerca de 620
linhas de funções utilitárias de texto e geometria.

## Motivo

Não é o tamanho que incomoda; é o que ele causa. **Quatro defeitos numa única
sessão de trabalho** tiveram a mesma forma: o componente certo, testado e
funcionando, e o caminho do evento até ele faltando.

- `Shift` com as setas não estendia a seleção;
- o arrasto da barra de rolagem prendia o indicador e não o movia;
- a roda do mouse sobre uma janela rolava o editor atrás dela;
- `Esc` não fechava a janela de renomear, e o clique não chegava à segunda
  escolha das configurações.

A causa é sempre a mesma: **o roteamento é escrito à mão**. Uma superfície nova
precisa ser plugada em seis lugares — `pointer_down`, `pointer_move`,
`pointer_up`, `scroll`, `key_down` e `escape` —, cada um uma cadeia de `if` com
dezenas de ramos. Esquecer um **compila**, passa nos testes de widget, e só
aparece quando alguém clica.

O ERLibUi tem árvore de widgets com `event()`. O `IdeShell` não a usa: ele
despacha manualmente. Esta especificação existe para trocar a cadeia manual por
uma lista de superfícies que não dá para esquecer de preencher.

## Decisão

Quebrar o arquivo em cinco fases, cada uma com um critério objetivo de conclusão
e verificável pelos testes existentes. Nenhuma fase muda comportamento; a última
é a única que muda desenho, e as quatro primeiras existem para torná-la legível.

### Fase 1 — Separar teste de produção ✅ Concluída

Os 4.470 de teste saem para `ide_shell/tests.rs`, submódulo do mesmo módulo, com
`use super::*` continuando a valer. O arquivo de produção cai para ~6.750 linhas.

**Critério:** `ide_shell.rs` não contém `#[cfg(test)]`; a contagem de testes não
muda.

**Por que primeiro:** é a única fase com risco zero — nenhum item muda de
visibilidade, nenhum caminho de código é tocado — e é o que torna o diff das
fases seguintes legível. Enquanto teste e código dividem o arquivo, qualquer
movimentação aparece misturada ao que não se moveu.

### Fase 2 — Extrair as funções puras ✅ Concluída

As funções sem `self` saem para dois módulos:

- **`text.rs`, na raiz da crate** — `previous_boundary`, `next_boundary`,
  `byte_at_column`, `line_column`, `offset_for_line_column`, `offset_of_line`,
  `token_at`, `identifier_prefix`, `is_identifier_character`,
  `position_in_range`, `encloses_type`, `count_outline`, `converted_syntax`,
  `token_kind_for`, `is_navigable`;
- **`ide_shell/geometry.rs`** — `rename_geometry`, `new_item_geometry`,
  `settings_dialog_geometry`, `settings_pages_rect`, `inspection_geometry` e as
  estruturas que elas devolvem.

**O texto ficou na raiz da crate, e não sob `ide_shell`**, porque a execução
revelou que **cinco dessas funções existiam duas vezes**: `byte_at_column`,
`line_column` e `offset_for_line_column` eram idênticas no shell e no `editor`, e
`previous_boundary` e `next_boundary` diziam a mesma coisa por caminhos
diferentes — divergindo só para entrada inválida, onde uma estourava e a outra
devolvia zero. Enterrá-las dentro do shell manteria a duplicação; na raiz, o
editor as consome do mesmo lugar. A versão unificada não estoura: grampeia o
deslocamento ao fim do texto.

**Critério:** nenhuma função **pura de texto ou de geometria** resta em
`ide_shell.rs`. As que sobram — `inspection_*`, `rename_reference_label`,
`generate_list`, `primary_pointer`, `click_widget`, `clipped_message`,
`tab_command`, `fill`, `stroke`, `label` — pertencem a superfícies e saem com
elas na fase 3.

**Custo colateral:** `crates/ide-ui/src/lib.rs` passou de 30 para 31 linhas, e o
teto do teste de arquitetura foi ajustado junto. O teto existe para a raiz
continuar um manifesto — e uma linha de `mod` é exatamente o que ela deve ter.

### Fase 3 — Uma superfície, um módulo ✅ Concluída

Cada janela leva consigo o **seu** estado, geometria, pintura e tratamento de
evento, para `ide_shell/<superfície>.rs`:

- ✅ `rename` — 419 linhas;
- ✅ `generate` — 369 linhas;
- ✅ `new_item` — 403 linhas;
- ✅ `type_search` — 330 linhas;
- ✅ `settings` — 741 linhas;
- ✅ `inspection` — 744 linhas.

`ide_shell.rs` saiu de 5.909 para **4.451 linhas**, com os mesmos 315 testes.

O estado saiu de `EditorAreaState`, `SearchState`, `SettingsState` e
`InspectionState` e passou a viver no módulo da superfície. O `IdeShell` guarda a
instância; ninguém mais alcança os campos. O que restou nos arquivos antigos é só
o que atravessa a fronteira: `search.rs` guarda os acertos da busca e o caminho
encurtado, `settings.rs` guarda a página, e `debugging.rs` caiu para 66 linhas.

**A fronteira é um `Outcome`.** A janela não alcança a sessão de edição, a fila
de comandos nem a barra de estado: ela devolve o que decidiu — `Idle`,
`Message`, `Insert`, `Apply` — e o shell executa. Sem isso a "extração" seria só
mudança de arquivo, com a janela continuando a mexer no documento por um caminho
lateral.

**Critério:** cada módulo expõe abrir, fechar, e os tratadores de evento; o
`ide_shell.rs` não menciona mais nenhum widget dessas janelas.

**O que a execução ensinou:**

- **o teste alcança o estado interno.** Vários apontavam cliques por
  `shell.editor_area.<janela>.…` e pela geometria livre. A janela passa a expor,
  sob `#[cfg(test)]`, a área onde apontar e o valor a observar — o teste continua
  entrando pela porta do shell, sem enxergar o miolo;
- **portar exige ler o original inteiro.** No `generate` eu havia simplificado
  duas regras sem perceber: o clique na trilha não marca a linha, e o clique fora
  do painel dispensa a janela. Fase que muda comportamento é fase para desfazer;
- **`IdeShell` engorda antes de emagrecer.** Cada janela extraída vira um campo:
  12 → 14, e a métrica do teste de arquitetura subiu junto. Ela volta a cair na
  fase 4, quando as superfícies passarem a viver numa lista só;
- **o guarda de arquitetura aponta para o arquivo, não para a ideia.** Ele exigia
  `SearchState` em `search.rs` e `SettingsState` em `settings.rs`. O estado mudou
  de arquivo, não de dono, e o guarda passou a apontar para o novo endereço —
  `ide_shell/type_search.rs` e `ide_shell/settings.rs`. Vale reler o guarda a cada
  mudança de endereço: ele guarda o desenho, e o desenho tem endereço;
- **nem toda janela desacopla igual.** A inspeção conversa com a sessão de
  depuração viva, então o `Outcome` dela é uma **lista** de pedidos
  (`Status`, `Evaluate`, `Expand`) — uma releitura pode disparar vários. E o que
  ela compartilha com o editor principal (a lista de completação, o painel em
  foco) ficou no shell, que é quem arbitra entre os dois.

### Fase 4 — O funil de eventos

Uma superfície passa a ser um contrato, e não uma convenção:

```rust
trait Surface {
    fn is_open(&self) -> bool;
    fn pointer(&mut self, event: &PointerEvent, size: Size) -> bool;
    fn key(&mut self, key: &str, modifiers: Modifiers) -> bool;
    fn scroll(&mut self, point: Point, delta: f32, size: Size) -> bool;
}
```

O shell mantém as superfícies numa lista ordenada por profundidade — a de cima
recebe primeiro — e cada entrada de evento vira uma varredura dessa lista. As
cadeias de `if` desaparecem.

**Critério:** `pointer_down`, `pointer_move`, `pointer_up`, `scroll`,
`key_down_with_modifiers` e `escape` não citam nenhuma janela pelo nome.

**Consequência:** registrar uma janela nova passa a ser **uma** linha, e não
seis. Esquecer deixa de ser possível — não há onde esquecer.

### Fase 5 — A casca

O que sobra em `ide_shell.rs` é a composição: estado do editor, Explorer e
terminal, a fachada pública que a aplicação usa, e a lista de superfícies.

**Critério:** menos de 1.500 linhas.

## Consequência

O custo é uma sequência de movimentações grandes num arquivo central, com
conflito garantido para qualquer trabalho paralelo nele. Por isso as fases são
independentes e cada uma termina com a suíte verde: dá para parar entre
quaisquer duas.

O ganho não é estético. A classe de defeito que custou quatro rodadas numa única
sessão deixa de existir na fase 4 — e as fases 1 a 3 existem para que a 4 caiba
num diff que alguém consiga revisar.

## Verificação

Cada fase termina com:

```text
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

Nenhuma fase pode alterar a contagem de testes. Uma fase que precise mudar um
teste mudou comportamento, e isso é motivo para desfazê-la e refazer.
