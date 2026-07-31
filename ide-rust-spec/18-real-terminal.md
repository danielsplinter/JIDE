# 18 — Um terminal de verdade

## Situação

O painel de terminal da IDE não é um terminal — é um **console de linhas**. O
`TerminalSession` guarda `VecDeque<TerminalLine>`, cada uma com texto e um sinal de
erro que vem do canal, e o `strip_terminal_controls` **descarta** as sequências de
escape antes de guardar: `\u{1b}[31mred\u{1b}[0m` vira `red`.

A linha de comando é uma faixa que a **IDE** desenha, com o texto que ela mesma
acumula em `session.input()`. O prompt é pedido ao shell e desenhado à parte.

Isso serve para ver a saída de um `mvn`, e é o que temos. Não é o que um terminal é.

## O que falta, e o que cada falta custa

| falta | o que se vê |
|---|---|
| **grade e cursor endereçável** | nada que reposicione o cursor funciona: barras de progresso que reescrevem a mesma linha empilham dezenas de linhas |
| **cores e atributos** | tudo cinza; sucesso e erro só se distinguem pelo canal de origem, não pelo que o programa pintou |
| **tela alternativa** | `vim`, `less`, `htop`, `git log` não têm onde acontecer |
| **eco pelo shell** | seta para cima, `Tab`, `Ctrl+R` não chegam ao shell: quem desenha a linha é a IDE, e o shell nunca soube que houve tecla |

O último é o que mais faz parecer falso. Num terminal, **o prompt é saída**: o shell
o escreve, apaga e reescreve conforme se digita. Aqui a IDE o desenha por fora, e
por isso tudo o que o shell faria redesenhando a linha simplesmente não acontece.

## Decisão de fundo: não escrever o emulador

Uma máquina de estados VT completa é grande, cheia de casos de borda e já existe
madura em Rust. A fase 0 escolheu qual usar.

## Fase 0 — Escolher, com uma prova ✅

**A recomendação inicial estava errada.** Esta especificação sugeria
`wezterm-term` "pela integração direta com o `portable_pty` que já usamos". Ele
**não existe no crates.io** — só há forks de terceiros. Sugeri de memória, e a
primeira consulta ao registro derrubou.

Os três candidatos reais:

| candidato | o que entrega | o que faltaria |
|---|---|---|
| `vte` 0.15 (Alacritty) | só o **analisador**: devolve eventos | grade, cursor, rolagem, tela alternativa, atributos — tudo nosso |
| `tattoy-wezterm-term` | fork de terceiro do motor do wezterm | depender de um fork mantido por outro projeto |
| **`justerm-core` 0.12** | grade, rolagem, dano, codificação de teclas | nada do que precisamos |

**Escolhido: `justerm-core`.** A descrição dele é a fronteira desta especificação
dita por outra pessoa: *"pure terminal engine: VT byte stream to grid + scrollback
+ damage. No I/O, no rendering"*. Sem PTY e sem desenho — exatamente o que a fase 1
deixou de fora da ERLibUi.

A API é pequena e é a que precisamos: `feed(&[u8])`, `grid()`, `cursor()`,
`resize(cols, rows)`, `scrollback_len()` — e `encode_key`, que transforma uma tecla
nos bytes que o shell espera, que é a fase 3 inteira.

### A prova

Um protótipo descartável, alimentando as sequências direto — sem PTY, o que é mais
determinístico e testa a mesma máquina de estados:

| # | o que hoje quebra | resultado |
|---|---|---|
| 1 | `\x1b[31merro\x1b[0m ok` | texto `"erro ok"`, `fg[0]=Indexed(1)` ≠ `fg[5]=Default` |
| 2 | `Progresso: 10%\rProgresso: 100%` | **uma** linha, com `"Progresso: 100%"` |
| 3 | `\x1b[2;1H` e escrever | a linha 2 foi reescrita, as outras intactas |
| 4 | `\x1b[?1049h` / `l` | dentro: `"dentro do vim"`; ao sair, `"no shell"` de volta |
| 5 | 50 linhas numa grade de 5 | 46 linhas no histórico |
| 6 | `resize(40, 10)` | grade de 40×10 |

Os seis passam. Os casos 2, 3 e 4 são exatamente os que o console de linhas de hoje
não tem como fazer.

Uma correção de percurso na própria prova: o caso 4 pareceu falhar porque eu lia a
linha 0, e o `1049` **preserva a posição do cursor** — o texto foi para a linha 1,
que é o comportamento correto. Sensor errado, não motor errado.

### O risco de escolher este, dito na cara

`justerm-core` é jovem e de pouca circulação — versão 0.12, um mantenedor. `vte`
tem anos de Alacritty atrás.

O que torna o risco aceitável é a **fronteira**: o motor fica atrás do
`ide-terminal`, e a superfície que usamos são seis funções. Trocá-lo por `vte` mais
uma grade nossa é um crate, não uma reescrita. Se ele parar de ser mantido, o custo
é conhecido e está contido.

**Critério cumprido:** escolha registrada, com motivo, tamanho da superfície e o
risco.

## Fase 1 — A grade, na ERLibUi ✅

A biblioteca não tem componente de terminal. O `Console` de hoje desenha **linhas**;
um terminal desenha **células**, cada uma com caractere, cor de frente, cor de
fundo e atributos.

Nasce um `TerminalView`: recebe uma grade e um cursor, desenha, e responde onde um
ponto caiu — `(linha, coluna)`, como o `Console` já faz. A seleção continua sendo
do consumidor; o componente desenha o que lhe disserem estar marcado.

O que **não** entra na biblioteca: o PTY, o processo, o analisador de escape. A
ERLibUi desenha; quem fala com o sistema operacional é a IDE.

**Critério:** a vitrine mostra uma grade com cor e cursor, sem nenhum PTY envolvido.

**Feito.** O `TerminalView` recebe `Vec<Vec<TerminalCell>>` e um `TerminalCursor`.
Cada célula tem caractere, cor de frente, cor de fundo e realce — as cores
**opcionais**, porque a maior parte de uma tela não pinta nada, e o que não declara
cor fica com a do tema. É assim que a grade acompanha a paleta sem o emulador saber
que ela existe.

Duas decisões de desenho que os testes prendem:

- **só as linhas visíveis são desenhadas.** Mil linhas com quatro visíveis produzem
  quatro comandos de texto. A virtualização é requisito da fase, não otimização;
- **o texto vai em trechos de mesma cor.** Oitenta colunas iguais são **um** comando
  de desenho; uma linha com um trecho vermelho vira dois. Célula a célula seriam
  oitenta, e o quadro não aguentaria uma compilação verbosa.

O cursor é posição **absoluta** na grade: rolar a saída para trás o tira de vista
sem mudar onde o programa o deixou.

Seis testes, e a vitrine ganhou uma grade de exemplo — saída de build com erro em
vermelho, sucesso em verde e o cursor esperando no prompt. ERLibUi 191 → 197.

## Fase 2 — O `ide-terminal` vira emulador 🔶

`VecDeque<TerminalLine>` sai; entra a grade do emulador escolhido. Os bytes do PTY
passam a ser alimentados nele em vez de filtrados pelo `strip_terminal_controls`.

O que a IDE lê deixa de ser "linhas de texto" e passa a ser "a grade e o cursor
neste instante", mais o histórico de rolagem que o emulador mantém.

**Critério:** um comando com cor chega colorido à grade; um que reescreve a linha
ocupa uma linha só. Ambos com teste, sem tela.

**Critério cumprido.** O `TerminalSession` passou a manter um `Engine` do
`justerm-core`, alimentado com os bytes **crus** do PTY — filtrá-los antes seria
jogar fora exatamente o que a grade existe para interpretar. Ele acompanha o
`resize`, para quem quebra a linha usar a mesma largura de quem a desenha.

Três testes novos, todos sem PTY e sem tela — a grade é alimentada à mão, o que é
determinístico e testa a mesma máquina de estados: a cor sobrevive até a célula, um
`` reescreve em vez de empilhar, e `[2;1H` reescreve a segunda linha deixando
as outras onde estavam.

`TerminalSession` ganhou `viewport()`, `cursor()` e `scrollback_len()`, que é o que
a fase 3 vai desenhar.

### O que ficou por fazer, e é preciso dizer

**A lista de linhas continua existindo e ainda é ela que alimenta a interface.** Por
um período há duas representações da mesma saída: a grade, que é a correta, e a
lista, que é a que se vê.

Isso é o oposto do que esta base vinha fazendo — passamos a especificação `17`
inteira eliminando fontes duplicadas de verdade. A justificativa é o tamanho: trocar
o que a interface lê é a fase 3, e fundir as duas coisas numa fase só faria uma
mudança grande demais para verificar de uma vez.

**Enquanto durar, a lista manda no que aparece.** O que a fase 2 entrega é a grade
existir, estar correta e estar testada — não a tela mudar. Se a fase 3 não vier, isto
aqui é peso morto e deve ser revertido, não deixado.

## Fase 3 — As teclas vão ao shell, e a faixa de entrada some

A IDE para de acumular texto e desenhar prompt. Cada tecla é escrita no PTY; o
shell ecoa, e o eco aparece na grade como qualquer outra saída.

Some `session.input()`, some `session.prompt()`, some a faixa `TERMINAL_INPUT_ID` —
e com ela a última coisa que fazia o terminal parecer uma caixa de texto com um log
embaixo.

**Critério:** seta para cima traz o comando anterior; `Tab` completa; `Ctrl+C`
interrompe. Nenhum deles é implementado pela IDE — todos são o shell respondendo.

**É a fase que mais muda o que se vê**, e a que só faz sentido depois da 2: sem
grade, o eco do shell não teria onde aparecer corretamente.

## Fase 4 — O que a grade permite

Com grade e cursor, três coisas passam a ser possíveis e hoje não são:

- **seleção por célula**, incluindo retangular;
- **busca na saída**, que precisa saber em que célula o achado está;
- **links clicáveis** — caminho de arquivo com linha e coluna vira navegação.

Nenhuma é requisito para o terminal parecer de verdade. Entram depois, se valerem.

## Riscos

- **Windows.** O ConPTY já emite sequências de escape por conta própria, inclusive
  quando o programa não pede. Um emulador correto lida com isso; um parcial produz
  lixo na tela. É mais um argumento contra escrever o próprio;
- **desempenho da grade.** Um `mvn` verboso enche a grade rápido. O `TerminalView`
  precisa desenhar **só as células visíveis**, como o Explorer já faz com as linhas
  — a virtualização é requisito, não otimização posterior;
- **o teste que existe hoje.** `the_command_line_waits_below_what_already_ran`
  afirma que a faixa de entrada fica embaixo da saída. Na fase 3 essa faixa deixa
  de existir, e o teste morre junto — o que ele garante passa a ser garantido por
  outra coisa: o cursor está na grade, no fim da saída.

## O que continua da IDE

Quais terminais existem, em que diretório abrem, qual perfil usam, e o que um
comando significa no domínio — rodar testes, compilar, depurar. O desenho é da
ERLibUi; a conversa com o sistema operacional é do `ide-terminal`.

## Verificação

Cada fase termina com `cargo test --workspace` e
`cargo clippy --workspace --all-targets -- -D warnings` nos dois repositórios.

**A contagem de testes vai mudar**, e aqui isso é esperado: a fase 2 troca o modelo
de dados e a fase 3 remove uma peça de interface. Como na `17`, cada fase declara o
que mudou e por quê.
