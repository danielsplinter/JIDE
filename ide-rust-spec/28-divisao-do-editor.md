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

## Fase 1 — dois editores, e a divisa entre eles ✅

- clique secundário sobre a faixa de abas abre o menu com "Split direita";
- escolher divide: o documento da aba clicada passa a ser mostrado também à
  direita, e **continua à esquerda** — dividir não fecha o que estava aberto;
- cada lado tem cursor e rolagem próprios sobre o mesmo texto;
- clicar num lado dá foco a ele, e o documento ativo passa a ser o dele;
- a divisa é um `Splitter` horizontal, e arrastá-la muda a largura dos dois;
- fechar a última aba da direita desfaz a divisão.

**Critério:** um teste que divide, escreve de um lado e afirma que o outro lado
mostra a mesma mudança com o cursor no lugar em que ele estava.

## Fase 2 — o foco segue o ponteiro ✅

Passar o ponteiro sobre um dos lados o torna o lado ativo, e a partir daí tudo
acontece nele: clique, rolagem, digitação.

**Mas quem recebe um arquivo aberto é o painel em que se clicou por último**, e
não o que o ponteiro atravessou. São duas perguntas diferentes: "onde o ponteiro
está" e "onde eu estava trabalhando". O caminho do mouse até o Explorer passa
por cima do painel do lado, e essa travessia não pode decidir em qual painel o
arquivo escolhido vai abrir — foi assim que um arquivo foi parar no painel
errado.

**O painel da frente é sempre o mesmo campo.** `editor_area.pane` é o painel do
lado com foco, e trocar o foco troca os dois de lugar. Parece indireto e é o
contrário: digitar, apagar, indentar, mover o cursor, colar, buscar e rolar
passam por esse campo em duas dúzias de lugares, e fazer cada um deles escolher o
painel pelo foco significaria que esquecer **um** faria o cursor andar no painel
que ninguém está olhando. Entre vinte e quatro, esquecer um é questão de tempo.

## O que o primeiro uso corrigiu

A fase 1 subiu compilando, com teste, e mesmo assim **sete defeitos apareceram no
primeiro uso de verdade**. Eles estão aqui porque cada um tem uma lição que não
cabe no commit que o corrigiu:

- **o componente pintava sobre as duas áreas.** O `SplitPane` da ERLibUi nasceu
  com um fundo e dois rótulos de demonstração — `Pane A` e `Pane B` —, e eles
  apagaram da tela a faixa de abas dos dois lados. Um componente-container que
  desenha dentro das áreas do hospedeiro não é usável; sobrou a divisa;
- **a faixa de abas da esquerda sumiu.** Eu lhe dei uma largura para parar na
  divisa, e com ela um estilo sem crescimento: o nó colapsou. Quem para a faixa
  na divisa é o recorte da pintura, e não a largura do nó;
- **o ponteiro ficava preso na divisa.** O soltar não chegava ao painel, e ele
  seguia em arrasto para sempre;
- **o clique no editor da direita movia o cursor da esquerda.** O clique era
  entregue ao painel guardado na divisão — que, depois da troca de foco, é o do
  **outro** lado. A correção foi deixar o clique seguir para o mesmo caminho que
  trata o editor de sempre: um caminho só para os dois painéis;
- **a barra de rolagem horizontal sumiu dos dois lados.** A trilha era a da área
  inteira, larga demais para o texto de um painel só: o componente concluía que
  não havia o que rolar. As barras e a lista de completação pertencem ao painel
  da frente;
- **a mesma aba acendia nos dois lados.** A faixa da esquerda perguntava pelo
  documento ativo da sessão, e ele segue o lado com foco. Cada faixa pergunta
  pelo documento dela;
- **o arquivo escolhido no Explorer abria no painel errado.** O caminho que trata
  os cliques da área dividida recebe **todo** clique da janela, e eu tratava
  "não caiu na direita" como "caiu na esquerda" — o clique no Explorer roubava o
  lado que ia receber, um instante antes de o arquivo abrir.

Três deles — o cursor no painel errado, a barra que sumiu e a lista de
completação no lugar errado — são a **mesma família**: alguma coisa continuava
falando da área do editor inteira depois de ela ter virado duas. Quando a área
se parte, tudo o que a media precisa ser reperguntado.

## Fase 3 — o que fica de fora ⬜

Divisão vertical, mais de duas divisões e arrastar uma aba de um lado para o
outro. Nenhuma delas é pedida agora, e cada uma multiplica os caminhos de
evento: é melhor tê-las como decisão do que como consequência.
