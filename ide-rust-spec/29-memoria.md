# 29 — O que a IDE segura sem precisar

## Situação

A IDE já mede o que gasta: a barra de estado mostra memória própria e externa,
separadas, e a `08` registra o primeiro número real medido num projeto grande.
Medir não devolve memória nenhuma — o que esta especificação lista é o que dá
para **devolver**.

Ela nasce **pendente**, e nenhum item aqui é correção de defeito: a IDE funciona
com todos eles. São escolhas que custam memória e podem custar menos.

## O que já foi feito, e não se refaz

Antes da lista, o que já está de pé — porque uma especificação de otimização que
ignora o que já existe convida a refazer:

- **suspensão por ociosidade.** Provider parado há cinco minutos **e sem
  documento aberto** é suspenso, e com ele vai o índice: a `20` mediu 103 MB só
  o de Java;
- **providers sob demanda.** `eager_language_providers` desligado faz cada um
  subir só quando alguém pergunta algo que ele responde;
- **os caches de realce morrem com a aba.** Fechar um documento apaga o
  instantâneo e os trechos convertidos dele;
- **o mapa dos crachás é chaveado por `u64`**, e não por `PathBuf`: trinta e dois
  bytes por entrada em vez de cerca de duzentos;
- **o índice de Java pagina em disco** desde a `20`.

## Item 1 — três terminais sobem na abertura, sempre ⬜

`TerminalSession::discover_profiles()` cria uma sessão para cada perfil
encontrado — PowerShell, CMD, Git Bash —, e cada sessão é **um PTY e um processo
de shell de verdade**, com dois mil linhas de histórico. Isso acontece na
construção do shell, antes de qualquer clique: quem nunca abre o painel de
terminais paga os três do mesmo jeito.

**O que fazer:** criar a sessão na primeira vez que a aba é ativada. As abas
continuam aparecendo desde o começo, porque o **perfil** é conhecido antes da
sessão — o que se adia é o processo, não a apresentação.

**Por que é o primeiro:** é o único item cujo raio é uma área só. Nada além do
terminal muda, e o efeito é medível no minuto seguinte.

**Critério:** abrir a IDE e afirmar, por teste, que nenhum processo de shell
existe até a primeira ativação de uma aba de terminal.

## Item 2 — o desfazer guarda o arquivo inteiro, dez vezes ⬜

Na ERLibUi, `EditSnapshot` carrega `text: String` — o documento **inteiro** — e
`UNDO_LIMIT` é dez. Cada passo de edição copia tudo de novo.

Um arquivo de 500 KB em edição segura até 5 MB de histórico. Com a tela dividida
são dois painéis, cada um com o seu, sobre o mesmo documento.

**O que fazer:** guardar o **trecho trocado** — posição, o que saiu, o que
entrou — em vez do documento. Desfazer passa a ser aplicar a diferença ao
contrário, que é como todo editor sério faz, e a memória do histórico deixa de
depender do tamanho do arquivo.

**É o item de maior valor e o mais delicado.** Ele mexe no editor da biblioteca,
que a janela principal, a inspeção do depurador e os dois lados da divisão usam.
Um desfazer que erra o trecho corrompe texto em silêncio, que é a pior coisa que
esta IDE pode fazer.

**Critério:** desfazer e refazer uma sequência de edições — inserção, remoção,
seleção substituída, indentação de bloco — e afirmar que o texto volta byte a
byte ao que era, com o cursor no lugar. E que a memória do histórico não cresce
com o tamanho do arquivo.

## Item 3 — a suspensão nunca alcança quem tem arquivo aberto ⬜

A regra atual exige que o provider **não tenha documento roteado**. Quem deixa um
arquivo aberto segura o índice daquela linguagem para sempre, mesmo com a IDE
parada a tarde inteira.

**O que fazer, se for para fazer:** fechar no provider os documentos sem foco há
muito tempo. O texto continua na `EditorSession` — a IDE não o perde —, e
reabrir é entregar o que já se tem.

**O risco é real e é o motivo de isto estar em terceiro:** reabrir dispara
reanálise, e reanálise no momento em que alguém volta ao arquivo é trocar
memória por uma IDE travada. Essa troca já foi feita ao contrário seis vezes
nesta base, e todas as seis foram defeito.

**Critério:** um limite de ociosidade **por documento**, e a medida de quanto
custa reabrir um documento fechado no provider — antes de ligar qualquer coisa.

## Item 4 — dois caches de realce por documento ⬜

`syntax_snapshots` guarda o que a linguagem devolveu, e `syntax_spans` guarda a
mesma coisa convertida para o desenho. Os dois vivem enquanto a aba existe.

**O que fazer:** conferir para que o bruto ainda serve depois da conversão. Se
for só pelo outline e pelos diagnósticos, guardar só isso e descartar o resto.

**Critério:** o que ficar guardado tem um leitor nomeado, e nenhum campo sobra
"por precaução".

## Item 5 — o histórico do terminal é número fixo ⬜

Duas mil linhas por sessão, escritas no código. Não é configuração, e não há
medida dizendo que duas mil é o número certo.

**Critério:** o número sai do código para a configuração, com o padrão de hoje.

## O que **não** conta como economia

Duas coisas que parecem otimização e não são:

- **o teto de 2 GB do analisador externo é teto, e não alocação.** Baixá-lo não
  devolve memória: faz o processo morrer antes num projeto grande;
- **a árvore do Explorer já é rasa.** Ela guarda o que foi aberto, e a `19` tirou
  a varredura completa do caminho da abertura.

## Antes de qualquer item: medir

O medidor existe e está na tela. Abrir um projeto real, anotar o próprio e o
externo, fechar o painel de terminais, esperar a suspensão e ver quanto cada
coisa vale é trabalho de uma sessão — e é o que transforma esta lista, hoje
ordenada por raciocínio, em uma lista ordenada por medida.

**Há precedente contra confiar no raciocínio aqui.** A guarda precisa de
`block_on` mediu 21, 0 e 4 em três tentativas antes de virar a grosseira que
está no lugar. Estimativa não é medida.
