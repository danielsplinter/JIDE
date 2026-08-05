# 27 — Fechar o par, e indentar ao abrir linha

## Situação

Escrever código nesta IDE é escrever cada caractere. Abrir um `(` obriga a
fechar o `)`; abrir um `{` e apertar `Enter` deixa o cursor na coluna zero, e
quem escreve arruma a indentação à mão, linha após linha.

São as duas conveniências mais básicas de um editor de código, e nenhuma existe.
Elas não acrescentam capacidade — acrescentam **cadência**: o custo delas não é
o tempo de digitar dois caracteres, é a interrupção de pensar neles.

## Onde isto mora, e por quê

A ERLibUi tem um `CodeEditor`, e a `08` dela o descreve. **Não é ali que isto
entra**, e a razão está escrita na própria `08`:

> Uma aplicação que já mantém o texto em outro modelo pode usar o `CodeEditor`
> apenas para desenhar, alimentando-o a cada mudança […]

É exatamente o que esta IDE faz. O texto vive no `ide-ui`, e é lá que a tecla
chega — `text_input`, e o buffer que ele altera. Fechar um par é **alterar
texto**, e alterar texto é de quem o tem.

A fronteira da `15` continua valendo e não é contrariada: ela fala de **desenho**
— a IDE decide onde, a biblioteca decide como se parece. Isto não é desenho.

*E é por isso que não vale escrever na biblioteca "para qualquer aplicação
aproveitar": ela não é dona do texto no único caso que existe, e código que
ninguém usa envelhece sem que ninguém perceba.*

## Fase 1 — O par se fecha, e não atrapalha quem o fecha ✅

Digitar `(`, `{` ou `[` escreve o fechamento junto, e deixa o cursor entre os
dois.

**A metade que é fácil de esquecer é a segunda.** Quem já tem o hábito de
escrever o `)` vai escrevê-lo — e receber `())`. Digitar o fechamento **quando
ele é exatamente o que está sob o cursor** move o cursor para depois dele, sem
escrever nada. Sem isso, a conveniência vira estorvo para quem digita depressa.

Três casos que a implementação encontra, e que decidem se ela presta:

- **com texto selecionado**, `(` **envolve** a seleção em vez de apagá-la. É a
  operação que mais se usa depois de aprender que ela existe;
- **apagar o `(`** apaga o `)` junto — **e só enquanto os dois estiverem na mesma
  linha**. Ter de apagar duas vezes o que se escreveu uma é a mesma quebra de
  cadência, ao contrário. Mas assim que o par se abriu em linhas, o fechamento
  deixou de ser o eco de uma tecla e passou a ser o fim de um bloco: apagá-lo
  junto levaria embora o fechamento de um corpo que já tem conteúdo, e quem
  apertou `Backspace` uma vez não pediu isso. Fora da linha, apagar o `(` apaga
  só o `(`, e o arquivo fica desbalanceado — que é exatamente o que foi pedido, e
  é visível;
- **desfazer** devolve o par inteiro, e não o fechamento sozinho. O par foi um
  gesto; desfazer é sobre gestos.

**Critério:** escrever `f(a, b)` inteiro, com os parênteses digitados como quem
não sabe que a IDE fecha, produz `f(a, b)` — e não `f(a, b))`. E apagar o `{` de
um bloco de três linhas deixa o `}` onde ele está.

### Como ficou

A decisão mora num módulo que **não toca no texto** — `editor::pares` —, e quem
escreve é o painel, que é o dono do cursor e do buffer. Partir assim não foi
arrumação: é o que faz cada regra caber num teste sem janela, sem documento
aberto e sem workspace. São treze testes, sete deles no módulo puro.

Três escolhas que a implementação obrigou a tomar, e que a especificação não
tinha antecipado:

- **a porta é `insert_pairing`, e não a `insert` de sempre.** Um painel de uma
  linha só — a expressão da inspeção, a caixa de busca — usa o mesmo
  `EditorPane`, e não quer parêntese fechando sozinho. Mudar a `insert` teria
  dado o comportamento a todos eles de graça, que é como uma conveniência vira
  defeito em outra tela;
- **o par entra numa escrita só.** É o que faz `Ctrl+Z` devolver o gesto inteiro
  em vez de deixar o `)` órfão — e o mesmo vale para apagar os dois;
- **passar por cima do fechamento não é edição.** A revisão do buffer não muda,
  e por isso a barra de estado não diz "Modified": anunciar alteração onde o
  arquivo não mudou seria mentir num lugar em que se aprende a confiar.

Com ocorrências marcadas por `Ctrl+D`, o par não nasce: cada marca é um cursor,
e um par por marca é outra pergunta. Ali a escrita segue como era.

## Fase 2 — `Enter` dentro do par abre a linha indentada ✅

Com o cursor logo depois de um `(`, `{` ou `[`, `Enter` leva para a linha
seguinte com **um nível a mais** do que a linha anterior, e o cursor fica no fim
dela — que é onde se vai escrever.

E quando o fechamento está logo à frente — `{|}`, que é o que a fase 1 produz —
são **três** linhas, e não duas: a de abertura, a linha indentada onde o cursor
fica, e o fechamento sozinho, alinhado com a abertura. Deixar o `}` grudado no
cursor obrigaria a arrumá-lo à mão toda vez, que é o trabalho que esta fase
existe para tirar.

```
class A {
  metodo() {
    |          ← linha em branco, um nível a mais; o cursor fica aqui
  }            ← fechamento, alinhado com quem o abriu
}
```

**O fechamento nunca fica mais fundo que a linha que o abriu.** É a única
disposição que qualquer formatador aceita sem reescrever, e a `27` prefere
concordar com o Prettier a discutir com ele no `diff` de quem revisa.

### Qual é o nível, e de onde ele vem

**Do arquivo, e não de uma configuração.** É a mesma regra que a `23` aplicou ao
`tsconfig` e a `20` ao índice: quem manda é o que está no disco. Um arquivo
indentado com dois espaços recebe dois; um com tabulação recebe tabulação.

Ler isso é olhar as linhas indentadas do próprio arquivo e tomar o passo mais
comum. Um arquivo novo, sem nenhuma, cai no padrão da linguagem — e aí sim é
palpite, mas é um palpite sobre um arquivo vazio, onde ele não contradiz nada.

*Uma configuração global faria a IDE indentar de um jeito num projeto que
indenta de outro, e o desacordo apareceria no `diff` de quem revisa.*

**Critério:** abrir um bloco num arquivo de dois espaços produz dois espaços; no
mesmo projeto, um arquivo de tabulação produz tabulação.

### Como ficou

A conta é sobre **degraus** — de quanto uma linha indenta a mais que a anterior,
e qual degrau se repete —, e não sobre a menor indentação que aparece no
arquivo. A menor seria quase sempre `1`, por causa do espaço que abre a linha de
dentro de um comentário de bloco:

```
/**
 * Nota.          ← esta linha indenta de um, e não é o passo de ninguém
 */
```

Um arquivo inteiro passaria a indentar de um em um por causa de três linhas de
comentário. É o tipo de erro que só aparece depois de entregue, porque nenhum
arquivo de teste tem cabeçalho.

Tabulação ganha quando **as linhas indentadas** dela são maioria, e não quando
ela vence a contagem geral: as linhas de margem zero — a que abre e a que fecha
o arquivo — são de espaço nenhum, e contá-las do lado dos espaços fazia uma
classe inteiramente tabulada perder para duas chaves.

E fora de um abridor nada mudou: `Enter` continua herdando a indentação da linha
anterior, que já era do editor da biblioteca.

## Fase 3 — Não fechar dentro de texto nem de comentário ⛔ Dispensada

**Dispensada por quem usa a IDE**, depois de ver o que ela custa: o fechamento
sobrando dentro de uma string incomoda e não quebra nada, e a fase inteira
existia só para isso.

*Fica escrita porque uma fase apagada volta como ideia nova.* Quem a reabrir vai
reencontrar o desenho pronto — e a razão de ela não ter sido feita, que é o que
uma lista de tarefas perde.

O que ela seria:

`'(' + valor` não pede fechamento. Um `(` dentro de uma string ou de um
comentário é um caractere, e não um par.

**E é ela que completa a fase 4.** Uma aspa dentro de uma string já começada é o
caso que a simetria não resolve sozinha.

**A IDE já sabe distinguir.** O realce que ela pinta a cada tecla diz o que é
texto e o que é comentário — é o `SyntaxSnapshot` da `04`, que já existe e já
está no lugar certo. Perguntar a ele é reusar o que se paga; adivinhar por
contagem de aspas seria uma segunda resposta para a mesma pergunta.

*Esta fase é a terceira de propósito.* Ela é a única que depende de linguagem, e
as duas primeiras valem sem ela — com o defeito conhecido de fechar parêntese
dentro de string, que incomoda e não quebra nada. Amarrá-las juntas atrasaria o
que já serve.

**Critério:** digitar `(` dentro de uma string não escreve `)`. Num arquivo sem
realce disponível, o par fecha — degradar para o comportamento da fase 1 é
melhor do que degradar para nenhum.

### O que dispensá-la deixa em pé

- **um `(` digitado dentro de uma string ou de um comentário fecha**, e quem
  escreveu apaga o que sobrou;
- **a aspa dentro de uma string já começada** continua decidida só pela simetria
  da fase 4: sob o cursor, passa por cima; fora dele, abre um par;
- **o primeiro caractere depois de abrir uma aspa** nunca teria acertado mesmo,
  porque o realce chega uma revisão atrás — a fase 3 não consertaria esse caso,
  e é bom que isso esteja dito antes de alguém tentar.

Nada disso quebra código: o compilador continua sendo quem valida, e um
parêntese sobrando aparece na hora, na tela.

**O que faria reabrir:** um relato de que apagar o que sobrou atrapalha mais do
que o par ajuda. Foi o mesmo critério que decidiu fazer as fases 1 e 2 — o
incômodo de quem digita, e não a completude da lista.

## Fase 4 — Aspas ✅

`'`, `"` e crase fecham sozinhas. Elas estavam registradas aqui como fora de
escopo, para depois da 3; foram pedidas antes, e a 3 continua sendo o que as
deixa completas.

**Elas são pares simétricos**, e é isso que muda tudo: o mesmo caractere abre e
fecha, e a mesma tecla é as duas coisas. Por isso a ordem das perguntas se
inverte — **primeiro se passa por cima, só depois se abre**. Um par de chaves
nunca precisa dessa dúvida, e é por isso que as aspas vivem numa lista à parte
em vez de entrarem na dos pares.

Duas defesas que a simetria exigiu, e que não existem para as chaves:

- **`don't` não abre string.** Uma aspa **encostada numa palavra** — letra,
  dígito ou `_` antes dela — não fecha nada. Sem isso, escrever um apóstrofo num
  comentário ou numa mensagem devolveria `don''t`, e é o defeito que faz alguém
  desligar a conveniência inteira;
- **apagar leva só a aspa encostada.** Nos pares de verdade a busca conta
  profundidade, porque `f(g())` diz qual `)` é de quem. `'a' 'b'` não diz nada:
  são dois pares na mesma linha e nenhuma aspa carrega a marca do seu par. Ali
  só se apaga o que a tecla anterior acabou de criar.

E uma aspa **não abre bloco**: `Enter` depois dela herda a indentação da linha,
como sempre fez. Abrir bloco partiria a string em duas.

**O que a fase 3 ainda vai consertar:** `'it|'` — digitar a aspa dentro da
string passa por cima e encerra o texto cedo. É inerente à simetria, e só quem
sabe onde a string começa resolve.

**Critério:** escrever `const a = 'oi';` inteiro, aspas digitadas à mão, produz
`const a = 'oi';`.

## O que fica de fora, e por quê

- **fechar tags de HTML**, que é outro assunto e mora na `24`;
- **reindentar um bloco inteiro**, ou ao colar. É formatação, e formatação num
  projeto TypeScript é do Prettier — que já está instalado ali e faz melhor;
- **indentar por gramática**, olhando a árvore em vez do caractere anterior. É
  mais correto e é caro, e o caractere anterior acerta o caso que se pediu.

## Riscos

- **A conveniência que atrapalha é pior do que a ausência.** Cada uma das três
  fases tem um caso em que ela erra na mão de quem digita depressa, e os três
  estão nomeados: o fechamento duplicado, o `}` grudado, e o parêntese dentro de
  string. Uma fase entregue sem o seu caso vira reclamação;
- **isto mexe no caminho da tecla**, que é o mais quente da IDE e o que a `26`
  registra como o menos testável — o tratamento da tecla saiu do tratador de
  eventos justamente para caber num teste. Cada fase daqui entra por ali, e o
  teste vem junto ou não entra.

## Verificação

Cada fase termina com `cargo test --workspace` e `cargo clippy --workspace
--all-targets -- -D warnings` nos dois repositórios, e com **um teste que digita**
— a mesma porta que a `23` abriu ao extrair `text_typed`. Uma conveniência de
digitação que só foi conferida à mão não foi conferida.
