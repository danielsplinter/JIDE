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

### As quatro coisas que a fase custou descobrir

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

**4. Uma pasta vazia pedia leitura para sempre — e isso travou a IDE.** Na
árvore, pasta vazia e pasta não lida têm a mesma forma. Responder a leitura de
uma pasta vazia não a tirava da lista de pendentes, e cada resposta fazia a
reconciliação da seleção pedir **todas** as outras de novo.

Com quarenta pastas expandidas o número medido foi: `request_expanded_directories`
chamada **1099 vezes**, **2490** leituras numa fila só, e um único evento
`Resized` levando **21,7 s** dentro de `dispatch_application_commands`. O laço de
eventos nunca voltava ao sistema — a janela abria branca e não respondia.

O que separa as duas formas é lembrar **o que já foi perguntado**: um conjunto de
caminhos pedidos no Explorer. Perguntar uma vez por pasta responde a pergunta de
vez, e uma pasta que veio vazia veio vazia mesmo. Recarregar o projeto limpa o
conjunto, porque aí o pedido é justamente ler tudo de novo.

**O diagnóstico custou mais que o conserto**, e por um motivo que vale registrar:
as primeiras tentativas foram palpite sobre o código, e o teste que as
acompanhava passava com e sem o defeito — logo não provava nada. O que resolveu
foi medir o processo: CPU acumulada (girando, não em impasse), carimbo de tempo
em cada etapa da partida (`initialize` terminava em 1,5 s), e então dentro do
evento até chegar na fila de comandos. `the_queue_of_directory_reads_settles`
guarda o resultado, e foi verificado que **falha sem o conserto**.

Nenhuma das quatro aparecia no plano: a fase foi escrita como "ler a pasta ao
expandir", e a leitura era a parte fácil.

### Fase 2 — Índice assíncrono ✅

O provider passa a responder **"ainda indexando"** e a completar em segundo plano.
Hoje a primeira consulta espera o índice inteiro; adiar a ativação para depois do
primeiro quadro tirou a janela em branco, mas não o bloqueio.

**É a mudança mais profunda das quatro**, porque muda o contrato: quem pergunta
passa a poder receber resposta parcial, e completação, navegação e realce precisam
lidar com isso — inclusive decidindo o que mostrar enquanto o índice não terminou.

**Critério:** a primeira consulta responde sem esperar o índice inteiro.

**Feito, e menor do que esta especificação supunha.** O contrato **já era**
assíncrono — `activate` e todos os métodos de `ActiveLanguage` são `async`. O
bloqueio não estava no contrato: estava dentro do `activate`, que montava o
`WorkspaceIndex` antes de devolver.

O índice virou `Arc<RwLock<WorkspaceIndex>>`, nasce **vazio** e é montado numa
linha de execução à parte. Os nove sítios que o leem passaram por um `index()`, e
enquanto ele não chega respondem nada — o que depende só do documento aberto
responde igual, porque nunca dependeu do índice.

`ActiveLanguage` ganhou **um** método, com padrão `true`:

```rust
async fn wait_until_indexed(&self, timeout: Duration) -> bool
```

Com limite, porque um índice que falhe não pode pendurar quem esperou. Quem não
chama trabalha com o que já existe, que é o caminho normal.

**A medição prévia acertou.** Contados antes: cinco testes consultavam o índice
logo após ativar. Exatamente **esses cinco** falharam, e nenhum outro — os demais
respondem pelo arquivo aberto. Eles passaram a esperar, e continuam afirmando o
que afirmavam.

Um teste novo guarda o que a fase entrega: `activation_returns_before_the_index_is_ready`
afirma que ativar leva menos de 250 ms. Antes, no mesmo projeto, levava ~1,5 s.
`language-java` foi de 29 para 30 testes.

### Fase 3 — Os tetos saem ✅

Com a indexação em segundo plano, demorar deixa de travar alguém, e os quatro
tetos perdem a razão de existir. Sai também a necessidade de avisar que truncou —
não haverá truncamento.

**Critério:** nenhum limite silencioso; um monorepo é indexado por inteiro, no
tempo que levar.

**Feita, na segunda tentativa.** Os quatro saíram: 600 caminhos, 500 fontes
`.java`, 64 jars e 24.000 classes do JDK. O que estava escondido, medido no
projeto de referência:

| | com teto | real |
|---|---|---|
| caminhos | 600 | **40.472** |
| fontes `.java` | 500 | **26.211** |
| tipos declarados | ~500 | **30.745** |
| classes externas | 24.000 | 22.951 |

A IDE indexava **1,9%** do projeto e não dizia nada. A completação não conhecia
98% do código, e quem usava não distinguia isso de um tipo que não existe — o
defeito que esta especificação chamou de o mais perigoso.

**A primeira tentativa foi revertida por engano.** A IDE abria branca e sem
responder, e eu atribuí à indexação sem teto. Era outra coisa: a cascata de
leituras do Explorer, descrita na fase 1. Vale registrar porque o erro tem forma
reconhecível — a suspeita caiu sobre a mudança mais recente, e não sobre a
medida.

#### O que a fase custou, e o que a fez caber

**1. Ceder entre arquivos.** A indexação é uma linha de execução só, e o que ela
não pode atrapalhar é a que desenha. Um `yield` a cada fonte é o que separa
"demora" de "trava".

**2. Guardar só o que outro arquivo pode nomear.** O índice acumulava **todo**
símbolo de todo fonte, parâmetros e variáveis locais inclusive. Nenhum consumidor
os queria: quem pergunta pelo arquivo em que está recebe a semântica dele, com
locais; o índice responde pelo resto do projeto, e ali um local de outro arquivo
não é destino de navegação nem sugestão de completação. Guardá-los era memória
paga por resposta errada.

**3. O arquivo guardado uma vez, não por ocorrência.** Este foi o número grande.
São **2.741.995** ocorrências de nomes no projeto, e cada uma carregava uma cópia
do caminho do arquivo — caminhos longos, num monorepo. Trocar o `PathBuf` de cada
ocorrência por um número, com a lista de arquivos ao lado, não muda o que o índice
sabe: `references_to` devolve `Location` como antes, e o formato compacto existe
só dentro do índice.

**4. E o mesmo nas declarações.** Sobravam **339.664** delas, cada uma repetindo o
seu caminho. `IndexedSymbol` é o `SemanticSymbol` sem o `Location`, e o índice
materializa **só o que a consulta acerta** — que é o ponto: a completação passa
por todas as declarações a cada tecla, e ali ler trinta mil caminhos para montar
uma lista que não usa caminho nenhum trocaria memória por lentidão. Por isso
`symbols()` devolve a forma compacta, e `location_of`/`materialize` são pedidos à
parte.

| | pico de memória |
|---|---|
| como estava | **927 MB** |
| sem locais e parâmetros | 822 MB |
| ocorrências com o arquivo por número | 266 MB |
| declarações também | **178 MB** |

Cinco vezes menos, com as contagens idênticas nos dois extremos: 339.664
declarações, 2.741.995 ocorrências, 30.745 tipos, 22.951 classes externas. Não é
menos índice — é o mesmo índice sem repetir nome de arquivo.

#### Os números da fase

Índice completo do projeto de referência, em release: **283 s** com o disco frio,
**42 s** com ele quente. E a IDE inteira, em debug, aberta no mesmo projeto:

| | 10 s | 60 s | 180 s | 240 s |
|---|---|---|---|---|
| responde | sim | sim | sim | sim |
| memória | 433 MB | 488 MB | 572 MB | 576 MB |

A CPU deixa de subir entre 180 s e 240 s: é a indexação terminando. A memória
estabiliza em 576 MB — o que a IDE já usava, uns 430 MB, mais o índice. Os dois
números medem coisas diferentes e convém não confundi-los: os 178 MB são o índice
sozinho, num processo que não tem janela, fontes nem terminal. Ninguém
espera por nada disso, e é esse o ponto da ordem escolhida: a fase 2 tirou a
espera do caminho, e só por isso a 3 pôde tirar os limites.

`indexing_a_large_project_costs_what_the_spec_says`, marcado `#[ignore]`, guarda
esses números e permite refazê-los.

### Fase 4 — Índice incremental ✅

Salvar um arquivo reindexa **aquele arquivo**, em vez de a classe nova só aparecer
na ativação seguinte. Trocar o JDK deixa de refazer os fontes do projeto.

**Critério:** uma classe criada agora participa da completação sem reiniciar nada.

**Feito no índice, e provado.** `WorkspaceIndex::reindex_file` tira o que um
arquivo declarava — símbolos, referências e a declaração do tipo — e o lê de novo;
se ele sumiu do disco, só tira. A indexação de um fonte virou `index_source`,
extraída da varredura, então o caminho é o mesmo para o primeiro e para o
milésimo arquivo.

O contrato ganhou o aviso, com padrão vazio para quem não tem índice:

```rust
async fn file_changed(&self, path: &Path) -> Result<(), LanguageError>
```

O teste `a_file_saved_now_joins_the_index` cobre as duas metades: a classe criada
entra na busca por tipo, e a apagada sai — sem levar as outras junto.

### O gatilho, ligado

`save_document` avisa o `LanguageHost` depois de gravar, e o host avisa **todas** as
linguagens ativas — não só a do documento: quem grava um `.java` pode estar com um
`.xml` aberto, e cada linguagem decide se o arquivo lhe interessa. O padrão do
contrato é ignorar.

O caminho passou a existir nas três camadas: `WorkerRequest::FileChanged`,
`LanguageHost::file_changed`, e a chamada no `native_ide`.

Junto veio `wait_until_indexed` no host, pela mesma via. Ele não serve ao uso
normal — serve a quem precisa da resposta completa, e um dia a um indicador de
"indexando" na barra de estado.

**Um teste de integração caiu no caminho**, e pelo motivo certo:
`navigation_finds_definitions_declared_in_other_files` afirmava navegação pelo
projeto inteiro logo após ativar. Ele passou a esperar — é o mesmo ajuste dos cinco
testes da fase 2, agora no nível da aplicação.

**327 testes na IDE.**

## A medida provisória que não chegou a ser precisa

Enquanto as fases 2 e 3 não existissem, o índice podia **relatar que truncou** —
uma mensagem na barra de estado dizendo que o projeto passou dos limites. Isso
não consertava: só parava de mentir.

Não foi preciso. As fases 2 e 3 chegaram antes, e sem teto não há o que relatar.
Fica registrado como o curativo que se faz quando o caminho não anda — e como
lembrete de que ele **não era um passo do caminho**.

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
