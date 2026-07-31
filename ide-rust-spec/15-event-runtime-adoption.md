# 15 — Adoção do runtime de eventos da ERLibUi

## Situação

A ERLibUi tem a maquinaria de interação inteira, escrita e testada:

| crate | o que oferece |
|---|---|
| `ui-tree` | árvore persistente de widgets, com invalidação |
| `ui-events` | `EventRouter`: teste de acerto, captura, alvo, bolha, propagação |
| `ui-focus` | `FocusManager`: registro, ordem de percurso, escopos que prendem e restauram |
| `ui-commands` | `CommandBus` e `ShortcutMap` |
| `ui-layout-api` | `LayoutSnapshot` com `hit_test` em ordem de pintura |

**A IDE não depende de nenhum deles.** Não aparecem em `Cargo.toml` nenhum dos
seus crates. O único consumidor é o `ui-showcase`, que os liga à mão dentro do
próprio `main.rs`.

No lugar disso, a IDE roteia eventos por conta própria, em três regimes que
convivem no mesmo arquivo:

1. **entrega de verdade** — o widget recebe o evento e guarda o que aprendeu:
   botões e combos das Configurações, campo e lista da renomeação, barras de
   rolagem, divisores, `ModalHost`;
2. **entrega a um clone descartável** — a IDE clona o widget, posiciona, entrega
   o clique, lê a resposta e joga o clone fora. O componente vira **função** —
   "dado este ponto, qual linha?" — e todo estado de interação morre com o clone:
   árvore do Explorer, lista da geração, árvore da inspeção;
3. **sem entrega nenhuma** — `if geometry.ok.contains(point) { … }`. O componente
   é só desenho e nunca sabe que existe um ponteiro: os botões de ação de quatro
   das cinco janelas.

O foco segue o mesmo padrão: o `FocusManager` não é usado, e cada janela guarda
por conta própria quem está em foco.

## Motivo

Os quatro defeitos que motivaram a especificação `14` tinham todos a mesma forma
— o componente certo, testado e funcionando, e o caminho do evento até ele
faltando. A `14` tratou o sintoma: organizou o roteamento manual em um funil que
não dá para esquecer de preencher. Este documento trata a causa: **o roteamento
manual não deveria existir.**

Três consequências mensuráveis do estado atual:

- **realimentação visual perdida.** Um botão que nunca recebe `PointerDown` não
  acende sob o ponteiro nem afunda ao ser pressionado. O componente sabe fazer as
  duas coisas;
- **cada consumidor remonta o meio de campo.** O `ui-showcase` de um jeito, a IDE
  de outro. Duas montagens, nenhuma na biblioteca;
- **o que a IDE escreve à mão é o que apodrece.** Foi assim com o foco, com a
  largura de caractere estimada e com as medidas dos diálogos, todos corrigidos
  nas últimas sessões — sempre depois de terem divergido em silêncio.

O modelo pretendido está claro no próprio código da biblioteca:

```text
evento → EventRouter → componente.event() → WidgetAction → CommandBus → aplicação
```

É o mesmo contrato do `addActionListener` do Swing, invertido: em vez de guardar
um fecho na construção, o componente **devolve** o que aconteceu e a aplicação
casa na chegada. A inversão é necessária em Rust — um fecho guardado dentro do
botão precisaria de acesso mutável ao estado da aplicação enquanto o botão está
emprestado — e é a mesma escolha do `iced`.

## Decisão

Adotar o runtime da biblioteca por fases, cada uma terminando com a suíte verde.
A primeira mudança é **na biblioteca**, não na IDE: hoje faltam a montagem e o
anfitrião, e apagar o roteamento da IDE antes disso a deixaria sem substituto.

### Fase 1 — Desfazer a duplicação de foco ✅ Concluída

O `FocusGroup`, criado em `ui-components` numa sessão anterior, duplica o
`FocusManager` do `ui-focus`, que já existia e faz mais — tem escopos. Ele sai;
a IDE passa a depender do `ui-focus`; as duas janelas que guardam foco à mão
(criar item e a página de depuração das Configurações) passam a usá-lo.

A **entrega** de `FocusGained`/`FocusLost` continua, por ora, na IDE. Não é
descuido: o `ui-focus` depende apenas do `ui-core` e não conhece o traço
`Widget`, de propósito. Entregar evento é trabalho de quem possui os componentes
— o anfitrião da fase 3. Até lá ficam três linhas na IDE, marcadas como
temporárias.

**Critério:** `FocusGroup` não existe mais; `ide-ui` depende de `ui-focus`;
nenhuma janela guarda foco em campo próprio; 315 testes na IDE.

**Executada.** O tipo duplicado saiu e a cobertura útil dos testes dele foi para o
`ui-focus` — nasce sem foco, id de fora não desloca, a troca nomeia os dois lados
do par. O gerenciador ganhou `clear_focus`, que faltava de verdade: clicar fora
dos campos desfoca, e sem isso quem quer desfocar precisa inventar um id que não
existe. Biblioteca de 150 para 154 testes; IDE nos mesmos 315.

### Fase 2 — Contrato do anfitrião (documento, sem código) ✅ Concluída

Levantamento das APIs reais e decisão do modelo. A pergunta que trava tudo:

> A IDE **reconstrói os widgets a cada quadro** a partir do próprio estado — é
> escolha deliberada e documentada, e é por isso que `with_pointer` existe nas
> abas. A árvore e o roteador pressupõem instâncias que persistem, com id estável
> e retângulo publicado no `LayoutSnapshot`.

Ou o anfitrião aceita descrição declarativa e **reconcilia por identidade**,
preservando o estado de interação, ou a IDE passa a manter instâncias vivas. O
ecossistema já respondeu isso em camadas: `Masonry` retém e roteia, `Xilem`
descreve e reconcilia. A escolha aqui deve seguir a mesma separação.

Também precisa ser verificado se a sobreposição das janelas modais cabe no
`hit_test`, que hoje é uma lista plana em ordem de pintura. Se couber, o funil de
superfícies da `14` — `SurfaceKind`, `SURFACES`, `COMPLETION_DEPTH` e os
`surface_*` — desaparece inteiro.

**Entregável:** especificação do anfitrião em `rust-wgpu-ui-spec`, com ADR.
**Critério:** documento revisado, nenhuma linha de código.

**Executada.** O contrato está em `rust-wgpu-ui-spec/17-ui-host.md`, com a
ADR-020 registrando a decisão.

As duas escolhas que travavam a fase: reconciliação **retida com troca por
identidade** — o anfitrião possui as instâncias, a aplicação substitui o widget de
um id, e o estado de interação é transplantado; widget que possui estado de edição
não é substituído, é mutado, que é o que a IDE já pratica sem ter nome. E a
sobreposição resolvida pela **ordem da árvore**: declarar por último é cobrir, e o
`ModalHost` aberto já devolve `Handled`, o que faz o funil da `14` —
`SurfaceKind`, `SURFACES`, `COMPLETION_DEPTH` e os seis `surface_*` — desaparecer
inteiro.

O levantamento achou duas lacunas que só aparecem quando se descreve o encaixe: a
árvore mede pelo `intrinsic_size` do nó e **nunca pergunta ao `Widget::measure`**,
e não há caminho para transplantar `InteractionState` entre instâncias do mesmo
id. As duas são trabalho da fase 3.

### Fase 3 — O anfitrião na biblioteca ✅ Concluída

Implementar a peça que possui, juntos, os componentes, a árvore e o instantâneo
de layout, e roda o roteamento. O consumidor passa a ter três chamadas: descrever
a interface, entregar o evento do sistema, pedir o quadro.

O **primeiro consumidor é o `ui-showcase`**, não a IDE: ele é menor, já tem a
montagem improvisada no `main.rs` para servir de referência, e migrá-lo prova o
contrato antes de mexer em 12 mil linhas.

**Critério:** o `main.rs` do showcase sem código de roteamento; testes da
biblioteca cobrindo o anfitrião.

**Feito.** O crate `ui-host` existe e está coberto por nove testes: clique chega
a quem está sob o ponteiro, clique fora não aciona, o componente guarda o que o
gesto deixou, a troca por identidade preserva o estado de interação, o clique dá
foco e o componente é avisado, a digitação vai a quem tem o foco, a janela
declarada por último engole o gesto, o escopo prende o `Tab` e devolve o foco ao
fechar, e o arranjo pergunta ao componente de que tamanho ele precisa.

O transplante de estado exigiu três mudanças na biblioteca: o `InteractionState`
foi para o `ui-core` (o traço `Widget` precisa alcançá-lo), o traço ganhou
`interaction`/`restore_interaction` com implementação vazia por omissão, e os 25
componentes com `WidgetCore` passaram a respondê-las.

**Três achados da implementação**, todos registrados em `17-ui-host`:

1. `LayoutDirection::Overlay` era decorativo — o adaptador Taffy o traduzia como
   `Column`, então declarar por último **não** cobria: empilhava embaixo. A
   promessa da fase 2 só passou a valer quando o adaptador ganhou sobreposição de
   verdade;
2. um pai em camada colapsa para zero, porque nenhum filho está no fluxo. A raiz
   passou a receber tamanho fixo quando a restrição é justa;
3. o anfitrião precisa carregar a medição de texto para os três contextos, ou
   todo componente cai na estimativa por contagem de caracteres.

**O `ui-showcase` migrou.** Saíram do `main.rs`: o mapa de componentes, a árvore
mantida em campo, o instantâneo de layout, o gerenciador de foco, as quatro
chamadas ao `EventRouter` e a entrega manual do par ganhar/perder. O binário
deixou de depender de `ui-events` e `ui-focus`.

A migração foi feita **sem fundir** a árvore com as instâncias: o anfitrião ganhou
`adopt`, que recebe a estrutura já declarada, e `attach`, que liga cada instância
ao nó de mesmo id. Foi a decisão que tornou a migração segura — as 240 linhas do
construtor de árvore não foram reescritas, e o que mudou foi só quem passa a
mantê-las.

Verificado rodando, e não só compilando: 37 nós arranjados, 22 componentes
pintados, 221 comandos, teste de acerto resolvendo e árvore de acessibilidade
montada.

O contrato ganhou cinco coisas que só um consumidor real pediria — quadro em duas
etapas, `EventOutcome` com `handled`, ordem de `Tab` declarada em separado,
`request_focus` para a tecnologia assistiva, e a própria adoção. Estão em
`17-ui-host`.

### Fase 4 — As janelas modais da IDE ✅ Concluída

As cinco superfícies — renomear, gerar, buscar, criar item, configurações,
inspeção — adotam o anfitrião. São o menor risco: já são unidades fechadas com
fronteira de `Outcome`, e o gesto nelas é clique e tecla, não arrasto contínuo.

Some daqui o regime 3 (teste de retângulo) e, se a fase 2 confirmar, o funil.

**Critério:** nenhuma janela resolve clique por `contains(point)`; 315 testes.

**Executada nas seis.** Cada janela passou a ter um `UiHost` próprio. O que ele
recebe são as **áreas** que o `FormLayout` já calculava — o arranjo continua sendo
da tela — e o que ele devolve é o alvo e os comandos. As cadeias de
`if rect.contains(point)` do roteamento de clique sumiram das seis.

A divisão que se firmou, e que vale para as fases seguintes:

> O anfitrião possui o que **só emite** — os botões. O que a janela **lê** —
> campo de texto, lista, árvore — continua dela, mas a área é declarada ao
> anfitrião, que resolve o acerto e diz quem foi atingido.

Os botões passaram a ser componentes de verdade: acendem sob o ponteiro e afundam
ao ser pressionados, o que não acontecia enquanto eram desenho com teste de
retângulo por trás.

**Duas coisas que a migração exigiu do anfitrião**, ambas achadas por teste
vermelho e não por leitura:

1. **`place`** — posicionar por área calculada pelo consumidor. Sem isso, adotar o
   anfitrião obrigaria a redesenhar o arranjo das seis janelas em Taffy antes de
   ganhar qualquer roteamento. E `place` precisou declarar o nó na árvore: sem
   caminho da raiz até o alvo, o roteador não monta rota — o primeiro teste
   vermelho da fase;
2. **`click`** — pressionar e soltar no mesmo ponto. O botão aciona na **soltura**,
   e a IDE só encaminha a pressão; o segundo teste vermelho mostrou as três
   primeiras janelas migradas sem acionar nada. Foi um defeito que eu havia
   introduzido e que os testes existentes pegaram.

**O que sobrou de `contains(point)`** nas janelas, e por quê: dois são a área da
roda do mouse — `generate` e `type_search` decidem se a rolagem é da lista antes
de repassá-la —, e um é o guarda da lista de páginas das Configurações. Nenhum é
roteamento de clique. Passam a fazer sentido quando o arranjo também for do
anfitrião, na fase em que o `LayoutSnapshot` responder por todas as áreas.

**O funil não desapareceu**, ao contrário do que a fase 2 previu. Ele deixará de
existir quando houver **um** anfitrião para a janela inteira, e não um por
superfície: é ele que hoje decide qual janela está aberta, e essa pergunta some
quando a sobreposição for a da árvore. Fica para a fase 5, junto com os painéis.

### Fase 5 — Os painéis contínuos ✅ Concluída

Editor, Explorer e terminal. São o maior risco: arrasto, seleção, rolagem
automática ao sair da área visível — exatamente onde nasceram os defeitos da
`14`. Some o regime 2 (clone descartável), e com ele a perda de destaque sob o
ponteiro.

**Critério:** nenhum `clone()` de widget para receber evento.

**Executada.** Os dois sítios do regime 2 eram a árvore do Explorer e a árvore da
inspeção. Os dois agora entregam o gesto ao widget **de verdade**.

A causa do clone era estrutural e vale registrar: posicionar um widget exige
acesso mutável, e a pintura recebe `&self`. Como a mesma função servia às duas
coisas, ela clonava para poder posicionar. Só que o clone morre no fim da chamada,
levando junto o destaque sob o ponteiro e a marca de que o gesto começou naquela
linha — o componente sabia responder, e ninguém guardava a resposta.

A separação é a que faltava: **posicionar-e-entregar** é caminho mutável, e a
pintura continua recebendo uma cópia posicionada. Quem recebe evento tem de ser
quem sobrevive ao quadro.

**O que continua, e por quê.** As abas — do editor e do terminal — não são clones:
são **reconstruídas a cada uso** a partir do estado da sessão, o que perde o mesmo
estado de interação pelo mesmo motivo. O remédio já existe e está testado: a troca
por identidade do anfitrião, que transplanta o `InteractionState`. Aplicá-la exige
que as abas passem a viver num anfitrião — o da janela inteira, e não um por
superfície —, e é por isso que ela anda junto com o desaparecimento do funil.

Ficam as duas coisas para a mesma etapa seguinte: **um anfitrião só**, que dissolve
o funil, transplanta o estado das abas e responde pelas áreas que hoje ainda são
testadas à mão na roda do mouse.

### Fase 6 — Só a biblioteca desenha 🔶 Regra travada, dívida em aberto

Hoje a IDE monta **48 primitivas visuais** à mão — `FillRect`, `StrokeRect`,
`DrawText`. Parte é arranjo legítimo (faixas de fundo), parte contraria regra já
escrita da biblioteca: os ícones da barra de atividades são desenhados como texto
(`"⌕"`, `"▣"`), e o botão de recolher o terminal é retângulo mais borda mais
glifo.

Esta fase depende de peças que ainda não existem na biblioteca — um `Panel`, mais
variantes de `Icon`, e um componente de console, que hoje é a maior lacuna
(o terminal desenha cada linha e posiciona a seleção com
`TERMINAL_CHAR_WIDTH = 8.4`, uma estimativa escrita à mão).

A fronteira: **a IDE decide onde; a biblioteca decide como se parece.** A
composição — quais janelas existem, o que cada uma mostra — continua da IDE.

**Critério:** guarda de arquitetura contando zero primitivas visuais construídas
no crate da IDE. Comandos estruturais (`PushClip`, `PopClip`, `LayerBreak`) ficam
de fora da regra até a biblioteca oferecer contêineres que recortem sozinhos.

**Zero não é alcançável hoje**, e fingir o contrário seria escrever um critério
que só passa quando o console existir. O que dá para fazer agora — e foi feito —
é **impedir que a dívida cresça**, com o número real dela à vista.

As três funções que produzem primitivas passaram a se chamar `raw_fill`,
`raw_stroke` e `raw_label`. O nome não é enfeite: era impossível contá-las antes,
porque `label(` também é nome de método em meia dúzia de lugares, e um guarda que
conta errado é pior do que nenhum. Com o prefixo, a contagem é exata.

O guarda `the_ide_never_draws_more_raw_primitives_than_it_already_does` fixa o
teto em **35** chamadas, hoje concentradas em dois lugares: a moldura da janela
(`painting.rs`, 29) e a página de depuração das Configurações (`settings.rs`, 6).
Cada peça que a ERLibUi ganhar derruba um punhado delas, e o teto desce junto.

**A dívida, com endereço e preço:**

| o que | quantas | o que falta na biblioteca |
|---|---|---|
| ~~faixas e fundos da moldura~~ | ~~14~~ | ✅ `Panel` com `SurfaceTone` |
| linhas e seleção do terminal | ~8 | um console; leva junto o `TERMINAL_CHAR_WIDTH = 8.4` |
| painel de depuração | ~2 | fundo e botões já são componentes; resta a borda e dois títulos |
| ~~ícones da barra de atividades~~ | ~~2~~ | ✅ `Icon::Search` e `Icon::Panels` |
| ~~botão de recolher o terminal~~ | ~~3~~ | ✅ `Button::icon` com `ChevronUp`/`Down` |
| contorno de foco e legendas | ~4 | capacidade no componente |

**Os ícones foram corrigidos.** Eram os únicos que contrariavam regra escrita da
biblioteca — *"aplicações não devem desenhar ícones por conta própria"*. A ERLibUi
ganhou `Icon::Search`, `Icon::Panels`, `ChevronUp` e `ChevronDown`, desenhados com
primitivas, e o botão de recolher o terminal virou um `Button::icon` de verdade —
com destaque sob o ponteiro e nome acessível, que retângulo mais glifo não tinha.
O teto do guarda caiu de 35 para **30**.

Um teste precisou ser corrigido junto, e a correção é reveladora: o teste das
marcas da calha contava **qualquer** círculo à esquerda dela, e o ícone de busca é
um anel. O filtro estava frouxo — a faixa certa é a da calha mesmo, entre a barra
lateral e o texto. Um desenho novo em outra parte da tela não deveria ter como
quebrá-lo, e agora não tem.

Os cinco botões da faixa de execução do painel de depuração também já são
componentes. Eram retângulo, borda e rótulo desenhados um a um, e por isso não
acendiam sob o ponteiro nem afundavam ao ser pressionados.

A troca corrigiu um defeito de comportamento junto, e vale dizer qual: o rótulo
apagado sinalizava "indisponível" quando não havia quadro parado, mas **o clique
passava assim mesmo**, mandando um passo que o depurador não tinha como dar. O
`Button` desabilitado recusa o gesto, que é o que o desenho já prometia. Um teste
precisou declarar o quadro parado para continuar exercitando o que exercitava.

**O `Panel` chegou.** A ERLibUi ganhou a superfície — preenchimento com tom do
tema e borda opcional — e o `SurfaceTone`, que nomeia o nível em vez da cor:
`Background`, `Surface`, `Elevated`. Por que o tom e não a cor: escrita à mão,
cada faixa fixava a cor no lugar, e uma tela com dezenas delas deixa de trocar de
tema **mesmo tendo tema**. O teste do componente verifica exatamente isso — a
mesma superfície pinta cor diferente no tema escuro e no de alto contraste.

**E os rótulos foram junto.** Tudo o que era texto cru e já tinha peça virou
`Label`: o estado e os dois títulos do painel de depuração, o nome do produto, o
título da barra lateral, o nome do projeto, o convite de tela vazia, a linha de
comando do terminal, o rótulo do terminal recolhido e os quatro textos da página
de depuração. As faixas restantes — barra lateral das Configurações e fundo da
linha de comando — viraram `Panel`.

**O console chegou, e com ele o `raw_label` deixou de existir.** A ERLibUi ganhou
o `Console`: linhas com tom de erro, rolagem e seleção por coluna. Ele mede a
fonte **de código** — a da interface é proporcional e mediria a coluna errada — e
guarda a medida do `layout`, porque pintar o realce e responder onde o clique caiu
precisam do mesmo número. Era exatamente isso que a IDE não tinha como fazer, e
por isso escrevia `TERMINAL_CHAR_WIDTH = 8.4`.

Com a saída do terminal virando componente, **nenhum texto da IDE é mais desenhado
à mão**: a função `raw_label` ficou sem uso e foi removida.

**O teto do guarda caiu de 35 para 2.** O que sobrou:

| o que | quantas | por quê |
|---|---|---|
| divisória de 1 ponto do painel de depuração | 1 | é régua, não superfície: a biblioteca tem `Splitter` para a arrastável e nada para a fixa |
| contorno de foco dos campos de depuração | 1 | devia ser capacidade do `TextInput`, não desenho de quem o hospeda |

As duas pedem decisão de projeto na biblioteca, não trabalho na IDE.

**O `TERMINAL_CHAR_WIDTH = 8.4` não existe mais.** O console passou a viver no
estado do painel, em vez de nascer a cada pintura, e é ele que responde
`position_at` — a mesma medição com que desenhou. Era a última estimativa de
largura de caractere na IDE, e o motivo de ela existir era não haver quem medisse:
o `terminal_position_at` deixou de precisar até do tamanho da janela, porque quem
sabe onde a saída está é o componente.

**Desvio da regra de verificação, declarado.** Esta especificação exigia que a
contagem de testes da IDE não mudasse. Ela foi de 315 para **316**, e o teste novo
é o próprio guarda. A regra existe contra refatoração que muda comportamento; um
invariante novo sendo passado a valer é outra coisa, e vale dizer qual das duas
está acontecendo em vez de contornar o número.

## O que não sai da IDE

Vale dizer o que **não** é alvo, para a adoção não virar transferência de
funcionalidade para a biblioteca — o que mataria a ADR-011 de lá, *UI
independente da IDE*:

- a composição: quais janelas existem e o que há dentro de cada uma;
- os `Outcome` e o que cada decisão significa no domínio;
- a fila de `ApplicationCommand` e tudo que fala de Java, catálogo ou projeto.

A biblioteca fornece o **vocabulário**; a IDE **compõe** com ele. Um "painel de
depuração" não deve existir na ERLibUi — ele deve ser `Panel`, `Button`, `Label`
e `ListView` montados pela IDE.

## Verificação

Cada fase termina com:

```text
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

nos **dois** repositórios. A contagem de testes da IDE não pode mudar: 315. A da
biblioteca pode subir, quando a fase acrescenta capacidade a ela, e pode descer
quando um tipo duplicado sai — o que a fase 1 faz, movendo a cobertura útil para
o `ui-focus`, onde ela pertence.
