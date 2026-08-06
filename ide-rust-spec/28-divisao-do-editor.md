# 28 — A divisão do editor

## O que se pede

Clicar com o botão direito sobre uma aba oferece **"Split direita"**. Ao
escolher, a área do editor passa a ter **dois editores lado a lado**,
independentes em navegação — cada um com seu documento, seu cursor e sua
rolagem —, e a divisa entre eles é **arrastável na horizontal**.

## O desenho, e o que ele evita

### Um armazém de documentos, duas vistas

Os documentos continuam num lugar só: a `EditorSession`. Ela guarda o texto, a
revisão e o que está sujo, e é dela que a aplicação tira os instantâneos que
manda para as linguagens. **A divisão não parte o armazém; ela parte a vista.**

Partir o armazém — duas sessões, duas listas de documentos — obrigaria a
sincronização com a aplicação a saber de qual lado veio cada documento, e o
mesmo arquivo aberto dos dois lados viraria dois documentos com duas revisões.
Editar de um lado não apareceria do outro, e gravar decidiria qual das duas
versões sobrevive. É um defeito que não se conserta depois.

### O que é de cada lado

O painel da direita guarda o que é **de vista**: quais abas ele mostra, qual
está ativa, e um `EditorPane` próprio — cursor, seleção, rolagem, a cópia de
desenho. Nada disso é do documento.

`session.active` continua significando **o documento que está sendo editado**, e
passa a seguir o lado com foco. É o que faz todo o resto da IDE — salvar,
completar, navegar, realçar, depurar — continuar funcionando sem saber que
existe divisão: elas perguntam "qual é o documento ativo", e a resposta continua
certa.

### A mesma área, repartida

A divisão acontece **dentro da área que o editor já ocupa**. A moldura do
anfitrião não muda: a faixa das abas e a faixa do editor continuam onde estavam,
e o que muda é a largura de cada metade dentro delas. Reorganizar a moldura
mexeria na geometria que o terminal, o painel de depuração e as barras de
rolagem leem — e nenhum deles tem nada a ver com esta funcionalidade.

### O painel dividido é um componente da biblioteca

A IDE não desenha e não inventa componente visual. **"Duas áreas lado a lado com
uma divisa que se arrasta" é um componente**, e ele nasce na ERLibUi:
`SplitPane`. Ele guarda a fração, põe a divisa no lugar, recebe o arrasto e a
desenha; quem pergunta recebe as duas áreas prontas.

Calcular `largura * fração - metade da alça` dentro da IDE seria desenhar sem
dizer que está desenhando: a conta é geometria de componente, e ela erra junto
com o desenho se as duas ficarem em lugares diferentes.

O que a IDE faz é o que ela sempre faz: dizer **o que** vai em cada área — um
`EditorPane` e um `Tabs` de cada lado, ambos já da biblioteca — e qual documento
cada um mostra.

## Fase 1 — dois editores, e a divisa entre eles ⬜

- clique secundário sobre a faixa de abas abre o menu com "Split direita";
- escolher divide: o documento da aba clicada passa a ser mostrado também à
  direita, e **continua à esquerda** — dividir não fecha o que estava aberto;
- cada lado tem cursor e rolagem próprios sobre o mesmo texto;
- clicar num lado dá foco a ele, e o documento ativo passa a ser o dele;
- a divisa é um `Splitter` horizontal, e arrastá-la muda a largura dos dois;
- fechar a última aba da direita desfaz a divisão.

**Critério:** um teste que divide, escreve de um lado e afirma que o outro lado
mostra a mesma mudança com o cursor no lugar em que ele estava.

## Fase 2 — abrir do lado certo ⬜

Clicar num arquivo no Explorer abre no painel **com foco**, e não sempre no da
esquerda. É o que faz a divisão servir para comparar dois arquivos.

## Fase 3 — o que fica de fora ⬜

Divisão vertical, mais de duas divisões e arrastar uma aba de um lado para o
outro. Nenhuma delas é pedida agora, e cada uma multiplica os caminhos de
evento: é melhor tê-las como decisão do que como consequência.
