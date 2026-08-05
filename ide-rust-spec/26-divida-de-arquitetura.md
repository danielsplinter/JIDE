# 26 — A dívida que uma sessão longa deixou

## Situação

Uma sessão de dez commits mexeu em nove crates e acrescentou capacidade de
verdade — os tipos do TypeScript no índice, as dependências instaladas, o
segundo elo da cadeia, a completação por nome com auto-import. Todas as guardas
de arquitetura continuam passando.

**E guarda que passa não quer dizer arquitetura intacta.** Quer dizer que
ninguém cruzou uma linha que alguém já tinha desenhado. O que esta especificação
registra é o que cresceu **onde não havia linha nenhuma**, e por isso cresceu
sem ninguém precisar autorizar.

Ela é curta de propósito, e nasce **pendente**: são três itens, e nenhum deles é
uma correção de defeito. São hipotecas.

## O que não se perdeu, e é a metade que importa

Antes das dívidas, o que a sessão **não** custou — porque uma lista só de dívidas
mente por omissão:

- **o contrato cresceu neutro.** `TextEdit` e `completion_edits` nasceram
  falando de "o que mais muda no arquivo", e não de `import` de TypeScript: Java
  tem a mesma necessidade, e a `04` continua valendo;
- **o `analyzer` continua sem alcançar projeto.** A fase 7 da `25` nasceu
  partida em dois módulos — `analyzer::stdlib` para o texto, `project::stdlib`
  para o disco e o `tsconfig` — porque a fronteira exigiu, e não porque ficou
  bonito;
- **as 18 crates continuam 18**, e nenhuma dependência nova entrou no grafo
  protegido;
- **a guarda do `block_on` nasceu nesta sessão**, e já pegou duas adições no dia
  em que foram escritas. Ela é o oposto de dívida: é linha nova onde não havia.

## Dívida 1 — `native_ide.rs`, 4 680 linhas ⬜

É o objeto-deus deste código. As `14` e `15` já registraram que ele deveria ser
decomposto, e nesta sessão ele cresceu de novo: a completação assíncrona, as
trocas de texto do auto-import, o `text_typed`, os diagnósticos.

**Não há teto para ele.** As `12` e `15` puseram teto em fachadas — 31 linhas na
raiz da `ide-ui`, 18 na do `language-java`, 10 na do host — e nenhum no arquivo
que mais cresce. Ele cresce em silêncio.

*E há um sinal de que o tamanho já cobra.* O tratamento da tecla vivia dentro do
`match` do evento de janela, e **nenhum teste o alcançava**: montar um evento
desses exige o winit inteiro. O resultado foi um defeito relatado três vezes,
com as duas pontas já sondadas e respondendo, enquanto o que faltava testar era
o pedaço que não dava para testar. Foi extraí-lo que resolveu.

**O primeiro passo é um teto no número de hoje**, como o das primitivas de
desenho da `15`. Ele não conserta nada; ele torna o crescimento visível e
deliberado. É pequeno, e é o único desta lista que cabe em meia hora.

**Critério:** uma guarda com o teto de linhas de `native_ide.rs`, e o número
descendo — não subindo — a cada decomposição.

## Dívida 2 — o teto de `block_on` subiu duas vezes no mesmo dia ⬜

De 27 para 28, e de 28 para 29. As duas com justificativa escrita, e as duas por
decisão de quem estava escrevendo o código — que é exatamente o desenho da
guarda: ela obriga alguém a olhar, e não decide por ninguém.

**Duas subidas no mesmo dia é o padrão de como um teto vira formalidade.** Um
número que só sobe deixa de medir; ele vira um registro do que foi feito, e não
um limite do que se pode fazer.

Não há conserto óbvio, e é por isso que está aqui em vez de ser feito: a guarda
grosseira foi escolhida **de propósito**, depois de a precisa medir 21, 0 e 4 em
três tentativas. Trocar a grosseira por uma que erra calada seria pior.

**O que vale observar:** se o número subir uma terceira vez sem uma queda no
meio, a guarda deixou de funcionar e o problema passa a ser dela, e não de quem
a levanta.

## Dívida 3 — o serviço guarda estado que pode envelhecer ⬜

O auto-import precisa devolver ao analisador a identificação da entrada que ele
ofereceu — o `data` opaco. Guardá-la no `CompletionItem` obrigaria vinte e três
construções em cinco crates a carregar um campo que só interessa a quem faz a
pergunta seguinte, e por isso ela ficou num mapa dentro do serviço: **a última
lista respondida, por documento**.

O acoplamento é novo. A escolha confia em que ela venha logo depois do pedido —
o que é verdade no uso normal, porque é a mesma lista que está na tela. Não é
verdade se algo se meter no meio.

**O que aconteceria de errado:** a escolha responderia sobre a lista de antes, e
escreveria o `import` do que não foi escolhido. É silencioso, e é o formato de
defeito que esta arquitetura mais persegue.

**A defesa honesta é uma chave**, e não a confiança: guardar junto o que
identifica o pedido — documento e posição — e recusar a resposta quando ela não
casar, como a completação já faz com a resposta vencida. É pequeno, e não foi
feito porque apareceu no fim de uma sessão longa.

**Critério:** um teste em que a escolha chega depois de outra lista ter sido
pedida, e nada é escrito.

## Por que isto é uma especificação, e não uma lista de tarefas

Porque as três têm **motivo**, e motivo é o que uma lista perde. A dívida 1 tem
uma história que a explica; a 2 é uma guarda que pode estar se gastando; a 3 é
uma troca deliberada entre acoplar em cinco crates e acoplar em duas chamadas.

Quem chegar aqui daqui a seis meses vai reencontrar as três, e a pergunta que
importa não é "o que falta" — é **"por que ficou assim"**. É isso que está
escrito.
