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

### Fase 4 — As janelas modais da IDE

As cinco superfícies — renomear, gerar, buscar, criar item, configurações,
inspeção — adotam o anfitrião. São o menor risco: já são unidades fechadas com
fronteira de `Outcome`, e o gesto nelas é clique e tecla, não arrasto contínuo.

Some daqui o regime 3 (teste de retângulo) e, se a fase 2 confirmar, o funil.

**Critério:** nenhuma janela resolve clique por `contains(point)`; 315 testes.

### Fase 5 — Os painéis contínuos

Editor, Explorer e terminal. São o maior risco: arrasto, seleção, rolagem
automática ao sair da área visível — exatamente onde nasceram os defeitos da
`14`. Some o regime 2 (clone descartável), e com ele a perda de destaque sob o
ponteiro.

**Critério:** nenhum `clone()` de widget para receber evento.

### Fase 6 — Só a biblioteca desenha

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
