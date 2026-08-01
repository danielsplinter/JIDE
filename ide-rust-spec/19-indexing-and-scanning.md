# 19 — Varredura e indexação: sair do bloqueio

## Situação

Duas coisas acontecem de forma **síncrona** e seguram a IDE:

| o quê | quando | custo medido |
|---|---|---|
| varredura da árvore do Explorer | ao abrir o projeto | **2,17 s** sobre 56 mil arquivos |
| índice Java | ao ativar o provider | **1,6 s** num projeto de 121 arquivos |

O índice se protege com tetos — 600 caminhos, 500 arquivos `.java`, 64 jars,
24.000 classes do JDK — e por isso termina rápido em qualquer projeto. A varredura
do Explorer **não tem teto nenhum**.

As três pendências do índice estão na ADR-015; esta especificação as ordena, e
acrescenta a varredura, que é de outro subsistema.

## O que o teto realmente é

O teto de 600 caminhos parece a causa e é o **sintoma**. Ele existe porque a
indexação bloqueia: sem ele, um monorepo travaria a IDE por minutos.

Então:

- **hoje:** resposta rápida e **errada em silêncio** — a completação não conhece
  parte do código, e quem usa não distingue isso de um tipo que não existe;
- **tirando só o teto:** resposta certa e **travada**;
- **o que se quer:** demorar deixa de travar alguém, e aí o teto não precisa
  existir.

Por isso tirar o teto não é o primeiro passo. É o terceiro, e fica seguro sozinho.

## A ordem

### Fase 1 — Varredura preguiçosa do Explorer ✅

**Por que primeiro**, mesmo sendo de outro subsistema:

- **bloqueia antes de tudo.** Acontece ao abrir o projeto, e vale para qualquer
  linguagem — inclusive um projeto sem Java nenhum;
- **é independente.** Não depende de decisão sobre o contrato do provider;
- **é mais barata, e de outra natureza.** O índice precisa de resposta parcial —
  protocolo novo. A árvore precisa apenas ler os filhos de uma pasta **quando ela
  é expandida**;
- **o desperdício é estrutural.** O Explorer já é virtualizado no desenho: só as
  linhas visíveis são materializadas. Hoje a IDE varre 56 mil entradas para
  mostrar quarenta.

**Critério:** abrir um projeto grande não varre o que não está expandido; o tempo
de abertura deixa de crescer com o tamanho do projeto.

**O consumidor que precisa ser resolvido antes.** `source_files(extensão)`, em
`ide_shell/documents.rs`, percorre a árvore **inteira** recursivamente para achar
todos os arquivos de uma extensão. Com varredura preguiçosa ela passaria a
responder só o que já foi expandido — o que é errado em silêncio, exatamente o
defeito que esta especificação combate.

Ela precisa de outra fonte: perguntar ao sistema de arquivos na hora, ou ao índice.
Decidir isso é parte da fase, e não detalhe de implementação.

### O que já está feito

**Os consumidores da árvore inteira foram resolvidos primeiro**, e isso valia por
si só:

- **`source_files`** saiu da árvore e foi para o `WorkspaceService`, lendo o
  filesystem. O único chamador — a execução de tarefas de linguagem, em
  `native_ide` — passou a pedir ao serviço;
- **a busca por conteúdo** fazia o mesmo: andava na árvore em memória. Agora lê o
  diretório na hora, e responde pelo projeto inteiro em vez de pelo que o usuário
  abriu no Explorer. Um teste do `ide-workspace` pegou isso.

**O encanamento da carga sob demanda existe:** `children_of` no `ide-workspace`,
`ApplicationCommand::LoadDirectory`, e `insert_directory_children` no shell, que o
Explorer dispara ao expandir uma pasta ainda não lida.

### Feito, e o número

Medido no projeto de referência — `camel-main`, **65.322 entradas**:

| | tempo |
|---|---|
| varredura profunda, como era | **3,22 s** |
| varredura rasa, como ficou | **3,0 ms** |
| carregar um caminho até uma pasta | 5,3 ms |

Mil vezes. E o tempo de abertura deixou de depender do tamanho do projeto: são 47
nós lidos, os do primeiro nível, em vez de 65 mil.

### As três coisas que a fase custou descobrir

**1. A seleção desistia antes de tentar.** `sync_explorer_to_active` fazia
`if self.explorer_path_for(target).is_none() { return; }`. Com a árvore profunda o
caminho estava sempre lá; com a rasa, ele saía **antes de expandir os ancestrais**,
e nenhuma leitura era pedida. Agora ele revela a pasta e volta a rodar quando os
filhos chegam.

**2. Pedir pasta a pasta perdia o nível fundo.** Os pedidos eram calculados de uma
vez; o de uma pasta funda era consumido antes de o pai existir, o `insert` não
achava o nó e o pedido sumia. A cadeia parava no penúltimo nível.

A saída é a **carga por caminho**: `scan_path(raiz, alvo)` devolve todos os níveis,
da raiz para a folha, **numa resposta só**. Quem insere sempre encontra o pai já
lá, e não há nada a repedir — o que também elimina o laço infinito que a tentativa
anterior produziu.

**3. Carregar não redesenhava.** `sync_explorer_tree` só ajusta quais nós estão
expandidos; as linhas da `TreeView` vêm de `items(&workspace)`, montadas quando a
árvore é substituída. Inserir filhos mudava o `FileNode` e não a lista — o arquivo
estava na árvore e não aparecia. Nasceu `rebuild_items`, chamado a cada carga.

Nenhuma das três aparecia no plano: a fase foi escrita como "ler a pasta ao
expandir", e a leitura era a parte fácil.

### Fase 2 — Índice assíncrono

O provider passa a responder **"ainda indexando"** e a completar em segundo plano.
Hoje a primeira consulta espera o índice inteiro; adiar a ativação para depois do
primeiro quadro tirou a janela em branco, mas não o bloqueio.

**É a mudança mais profunda das quatro**, porque muda o contrato: quem pergunta
passa a poder receber resposta parcial, e completação, navegação e realce precisam
lidar com isso — inclusive decidindo o que mostrar enquanto o índice não terminou.

**Critério:** a primeira consulta responde sem esperar o índice inteiro.

### Fase 3 — Os tetos saem

Com a indexação em segundo plano, demorar deixa de travar alguém, e os quatro
tetos perdem a razão de existir. Sai também a necessidade de avisar que truncou —
não haverá truncamento.

**Critério:** nenhum limite silencioso; um monorepo é indexado por inteiro, no
tempo que levar.

### Fase 4 — Índice incremental

Salvar um arquivo reindexa **aquele arquivo**, em vez de a classe nova só aparecer
na ativação seguinte. Trocar o JDK deixa de refazer os fontes do projeto.

**Critério:** uma classe criada agora participa da completação sem reiniciar nada.

## A medida provisória que não entra na ordem

Enquanto as fases 2 e 3 não existirem, o índice pode **relatar que truncou** — uma
mensagem na barra de estado dizendo que o projeto passou dos limites e a
completação está incompleta.

Isso não conserta: só para de mentir. Vale como curativo se as fases seguintes
demorarem, e some quando a 3 chegar. **Não é um passo do caminho** — é o que se faz
enquanto o caminho não anda.

## O que não muda

- **os tetos de memória por JAR** (`08-storage-and-memory`) continuam: eles
  protegem consumo, não tempo de bloqueio;
- **a decisão de indexar depois do primeiro quadro** continua certa e permanece.

## Verificação

Cada fase termina com `cargo test --workspace` e
`cargo clippy --workspace --all-targets -- -D warnings`.

E com **número medido**, como a ADR-015 fez: tempo de abertura de projeto para a
fase 1, tempo até a primeira resposta do provider para a 2. Sem medição, "ficou
mais rápido" é opinião — e foi medição que produziu os 2,17 s e o 1,6 s que
motivam esta especificação.
