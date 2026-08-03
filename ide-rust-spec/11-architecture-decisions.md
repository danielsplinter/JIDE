# 11 — Decisões Arquiteturais

## ADR-001 — IDE nativa

**Decisão:** o processo principal será escrito em Rust e não dependerá de JVM.

**Motivo:** reduzir overhead permanente e controlar memória.

## ADR-002 — Toolchains externas

**Decisão:** compiladores, runtimes e interpretadores serão ferramentas externas configuráveis.

**Motivo:** evitar acoplamento entre IDE e runtime.

## ADR-003 — Providers substituíveis

**Decisão:** uma linguagem poderá possuir vários providers.

**Motivo:** permitir provider nativo, LSP, serviço remoto ou fallback.

## ADR-004 — Composição

**Decisão:** funcionalidades serão compostas por contratos pequenos.

**Motivo:** evitar classes monolíticas e hierarquias rígidas.

## ADR-005 — Isolamento

**Decisão:** providers e plugins pesados poderão executar fora do processo principal.

**Motivo:** resiliência e controle de recursos.

## ADR-006 — Análise incremental

**Decisão:** parsing, semântica e indexação devem trabalhar sobre snapshots e invalidação.

**Motivo:** desempenho em projetos grandes.

## ADR-007 — Núcleo independente de linguagem

**Decisão:** tipos específicos de Java não entram no core.

**Motivo:** viabilizar múltiplas linguagens.

## ADR-008 — APIs versionadas

**Decisão:** contratos de plugins terão versionamento explícito.

**Motivo:** evolução sem quebra silenciosa.

## ADR-009 — Servidores externos e neutros

**Decisão:** a integração com servidores e containers ocorrerá por processos,
arquivos, protocolos e APIs, e nenhum produto terá posição privilegiada. Tomcat,
Jetty, WildFly, WebSphere, Liberty, Quarkus, Spring Boot e qualquer outro
processo Java são alvos equivalentes.

**Motivo:** não acoplar o processo da IDE à JVM do servidor nem a arquitetura a
um fornecedor.

## ADR-010 — Memória como requisito arquitetural

**Decisão:** cada componente terá orçamento e métricas.

**Motivo:** baixo consumo não surge automaticamente por usar Rust.

## ADR-011 — Terminal persistente via PTY

**Decisão:** cada aba de terminal possuirá um shell interativo persistente
conectado a uma pseudoterminal; no Windows será usado ConPTY por meio de
`portable-pty`.

**Motivo:** delegar a interpretação integral dos comandos ao shell, preservar
estado entre comandos e suportar o comportamento esperado de terminais de IDE,
inclusive programas interativos, redimensionamento e saída assíncrona.

## ADR-012 — Depuração como forma de integração com servidores

**Decisão:** a integração com um processo em execução se dá conectando-se à sua
porta de depuração. O usuário inicia o servidor com depuração habilitada e
informa host e porta; a IDE registra breakpoints, para na linha, executa passo a
passo e inspeciona a pilha e as variáveis. Iniciar, parar, publicar artefato e
ler logs do produto não fazem parte desse caminho.

**Motivo:** é o único mecanismo que todo servidor, container e ferramenta Java
oferece do mesmo jeito. Suportar mais um servidor passa a custar zero linhas de
código — apenas host e porta —, enquanto integrações por produto exigiriam um
adapter, um formato de configuração e um ciclo de vida para cada um.

**Consequência:** operações específicas de produto ficam disponíveis apenas como
adapters opcionais posteriores, e nenhuma funcionalidade essencial pode depender
delas.

## ADR-013 — Interrupção do terminal: defeito conhecido, não resolvido

**Situação:** parar a aplicação escreve `0x03` na entrada do PTY, que é como um
terminal envia `Ctrl+C`. Isso **não interrompe** o processo em primeiro plano
com `portable-pty` 0.8.1 no Windows.

**Evidência:** um `ping -n 30` segue respondendo por mais de doze segundos depois
da interrupção, com `cmd` e com `powershell`, dentro e fora de sandbox, tanto com
`0x03` cru quanto seguido de `CR` ou `CRLF`. Com o Maven real, a aplicação sobe,
o stop não produz saída alguma — nem log de encerramento, nem a pergunta do lote
— e o comando seguinte é engolido pelo processo que continua rodando.

O caso decisivo é o `pause`, que continua com **qualquer** tecla: ele não é
dispensado pelo `0x03`. A entrada não chega ao processo filho nem como sinal nem
como tecla, embora comandos digitados e submetidos com quebra de linha cheguem
normalmente.

**Caminhos já descartados:**

- subir para `portable-pty` 0.9.0 — o terminal deixa de produzir qualquer saída;
- enviar a tecla em win32-input-mode (`ESC [ 67;46;3;1;8;1 _`), apesar de o
  pseudoconsole ser criado com `PSEUDOCONSOLE_WIN32_INPUT_MODE`.

**Caminho restante:** `GenerateConsoleCtrlEvent`, que exige Win32 direto e
esbarra no `unsafe_code = "forbid"` do workspace — decisão de arquitetura, não
detalhe de implementação.

**Consequência:** o botão de parar não interrompe a aplicação. Os dois testes que
cobrem o comportamento estão marcados como `ignored` apontando para esta decisão,
em vez de removidos: eles descrevem o comportamento correto e voltam a valer no
dia em que a interrupção funcionar.

## ADR-014 — O realce sintático é convertido uma vez por revisão

**Situação:** o realce chega do provider em linha e coluna, e o editor endereça o
texto por caractere absoluto. A conversão acontecia **a cada quadro**, e cada
extremo de token era localizado percorrendo o arquivo desde a primeira linha.

**Evidência:** numa classe de cerca de 1.300 linhas e 212 KB, com uns 3.600
identificadores, são mais de 7.000 extremos a converter. Como cada um custa uma
varredura, o trabalho cresce com o quadrado do tamanho do arquivo — e se repetia a
cada rolagem, clique ou tecla que provocasse redesenho. O editor da biblioteca não
era o gargalo: o benchmark virtualizado pinta 100 mil linhas em microssegundos,
porque ali não há milhares de spans para percorrer.

**Decisão:** a conversão passa a acontecer uma vez por documento e revisão, e o
resultado fica guardado. Uma tabela com o início e o tamanho de cada linha,
montada numa única passagem, transforma cada extremo de token em consulta direta.
A pintura recebe o vetor já pronto **por empréstimo**, sem reconstruí-lo.

O cache é indexado por documento e descartado quando a aba fecha, respeitando o
anti-padrão de *cache sem limite* de `08-storage-and-memory`: ele é limitado pelo
que está aberto, não pelo que já foi aberto.

Na mesma linha, a sincronização de linguagens deixa de acontecer em clique comum.
Ela clona o texto de todas as abas na thread da interface; o que a justifica é o
**conjunto de documentos mudar** — uma aba aberta ou fechada —, e não o ponteiro
ter tocado o editor. Mover o cursor não muda documento nenhum.

A mesma regra vale na abertura. Ativar o provider indexa o JDK e os fontes do
projeto — mais de um segundo em compilação de depuração, medido sobre um projeto
real. Feito antes do primeiro quadro, isso deixava a janela **já visível** em
branco todo esse tempo. A primeira sincronização de linguagens passou a acontecer
depois do primeiro desenho: a IDE aparece montada, e o realce chega no quadro
seguinte.

**Consequência:** o custo do realce passa a ser proporcional ao arquivo, e só
quando ele muda. Em troca, quem produzir realce por outro caminho precisa invalidar
a entrada — a revisão do buffer é o que decide, e um realce novo com revisão antiga
é ignorado em vez de exibido fora de lugar.

## ADR-015 — Indexação Java: era síncrona, integral e com tetos silenciosos

**Situação:** a completação, a navegação e a busca por nome se apoiam num índice
montado uma vez, quando o provider Java é ativado. Ele tem duas metades: os nomes
das classes do JDK, lidos do diretório de cada `jmods/*.jmod` sem descompactar — os
membros vêm depois, sob demanda —, e os fontes do projeto, que são lidos, parseados
e analisados semanticamente para render símbolos, referências e o mapa de qual
arquivo declara cada tipo.

**Evidência:** medido sobre um projeto real de 121 arquivos, 92 deles `.java`, com
o JDK 17 e seus 71 `jmods`, a ativação leva cerca de **1,6 s em compilação de
depuração**. O peso está em parsear e analisar os fontes, não em ler os `jmods`.

**Decisão:** por ora a indexação continua assim, e a primeira sincronização de
linguagens acontece depois do primeiro quadro, para que a espera aconteça com a
IDE já desenhada.

### Pendências conhecidas — todas resolvidas

Eram três, e a especificação `19` as executou nesta ordem, com a varredura do
Explorer na frente:

- **Os tetos silenciosos saíram.** Eram 600 caminhos, 500 arquivos `.java`, 64
  jars e 24.000 classes do JDK. No projeto de referência, isso era **1,9%** do
  código: 30.745 tipos existem onde o teto mostrava uns 500. Hoje não há limite,
  e portanto não há truncamento a relatar.
- **A indexação é incremental.** Gravar um arquivo reindexa **aquele** arquivo, e
  a classe criada agora participa da completação sem reiniciar nada.
- **Ela é assíncrona.** O índice nasce vazio e é montado em segundo plano; ativar
  volta em menos de 250 ms, e quem precisa da resposta completa pede para
  esperar, com limite.

**O que tornou os tetos dispensáveis** foi a fase 2, não a coragem de removê-los:
enquanto a indexação bloqueava, o teto de 600 era **sintoma**, e tirá-lo antes
trocaria uma resposta errada em silêncio por uma IDE travada. A ordem importava.

**O que a remoção cobrou, e como coube:** indexar o projeto inteiro custava 927 MB.
Três mudanças no que o índice guarda o levaram a **178 MB** sem perder nada —
parâmetros e variáveis locais de outros arquivos saíram (nenhum consumidor os
queria), e o caminho do arquivo passou a ser guardado uma vez, em lugar de uma
por ocorrência e uma por declaração: eram 2,7 milhões e 340 mil cópias de trinta
mil nomes. Os números e o método estão na fase 3 da `19`.

A quarta pendência do mesmo tema, registrada em `08-storage-and-memory` — a
**árvore do Explorer varrida inteira ao abrir o projeto** — também saiu: a
varredura é rasa e por caminho, e a abertura foi de 3,22 s para 3 ms.

## ADR-016 — Análise por tecla: uma passada, sob demanda o resto

**Situação:** cada tecla digitada sincroniza o documento com o provider de
linguagem e pede o realce novo. Medido na janela em execução, com `ERIDE_PERF=1`,
uma tecla custava **entre 390 e 460 ms em `release`** — tudo dentro dessa
sincronização, com o desenho do quadro em saudáveis 5 ms. Digitar ficava
travado: o caractere aparecia segundos depois.

**Evidência:** reproduzido por tamanho de arquivo, medindo o custo de uma tecla:

| linhas | antes | depois |
|---|---|---|
| 200 | 46 ms | 4,0 ms |
| 1000 | 1,48 s | 19,7 ms |
| 3000 | 12,5 s | 65 ms |

O crescimento era **quadrático**. Três causas, na ordem em que pesavam:

1. **Conversão de posição varrendo o arquivo.** O tree-sitter dá a posição de um
   nó em coluna de *bytes*; a IDE fala em coluna de *caracteres*. A conversão
   procurava a linha do nó com `lines().nth(row)`, que varre o texto desde o
   começo — e a análise converte a árvore inteira. Um `LineIndex`, montado uma
   vez por análise, troca a varredura por uma indexação.
2. **Semântica calculada sem ninguém pedir.** Símbolos, escopos e referências
   custavam quase tanto quanto o realce e não servem para desenhar: só
   completação, navegação e busca de usos os consultam. Agora a tecla apenas
   invalida, e quem pergunta paga — `Documents::ensure_semantics`.
3. **A árvore percorrida três vezes, criando um cursor por nó.** `Node::walk`
   aloca, e só andar por uma árvore de 81 mil nós custava 35 ms; realces,
   diagnósticos e outline andavam cada um por sua conta. `walk_tree` percorre com
   **um** cursor e `collect_analysis` colhe os três de uma passada.

**Decisão:** a análise por tecla fica com o que é preciso para desenhar — realce,
diagnósticos, outline e imports — em uma passada; o resto é sob demanda. O campo
`SyntaxSnapshot::tree`, uma cópia da árvore com uma `String` por nó, foi
**removido**, e o tipo `SyntaxNode` saiu junto de `ide-domain`: ninguém os lia, e
preencher o campo custava 48 mil alocações por tecla. Um contrato não pode obrigar
a pagar por resposta que ninguém pediu; se a árvore voltar a ser necessária, deve
nascer sob demanda, como a semântica. Os capítulos `03-core-contracts` e
`05-java-integration` acompanham a mudança.

**Consequência:** o custo da tecla passou a ser linear e cabe num quadro para
arquivos de tamanho normal. A pendência de a análise ser **síncrona**, da
ADR-015, continua de pé: num arquivo de 3000 linhas a tecla ainda gasta 65 ms no
laço da janela. Tirá-la do caminho da tecla é o próximo passo, e só então o
tamanho do arquivo deixa de aparecer na digitação. A medição que encontrou isto
fica disponível em `ERIDE_PERF=1`, que imprime o custo por evento e por quadro.

## ADR-017 — A digitação não espera pela análise

**Situação:** depois da ADR-016 a tecla caiu de 400 ms para 60–90 ms, medidos na
janela com `ERIDE_PERF=1`, e continuava tudo em `sync_languages`. Ainda dava para
sentir: a 60 ms por caractere, quem digita rápido chega na frente da IDE.

**Descoberta:** o provider de linguagem **sempre teve thread própria** — o
`ProviderWorker` sobe uma no `spawn` e recebe pedidos por canal, com `try_send`
que nunca bloqueia. O que punha a análise no meio da digitação era o
`pollster::block_on` do lado da aplicação, esperando a resposta que já vinha de
outra thread.

**Decisão:** a mudança de documento e o pedido de realce passam a ser **postados**
— `LanguageHost::post_change_document` e `post_syntax` devolvem o receptor sem
esperar. O `LanguageController` guarda os receptores pendentes e o relógio da
janela recolhe o que ficou pronto, instalando o realce quando ele chega. Abrir
documento continua esperando: é raro e o resto depende dele.

A fila do worker é ordenada, então o realce pedido depois de uma mudança fala do
texto **com** ela aplicada, e as consultas que ainda esperam — completação,
navegação, `Generate` — são processadas depois do que já foi postado, ou seja,
enxergam o texto atual.

**Consequência:** a tecla custa o que custa mexer no texto, e o realce aparece um
ou dois quadros depois; o editor já ignorava realce de revisão vencida, então
chegar tarde não desenha nada errado. Em troca:

- **Contrapressão é visível no código, não no relógio.** Se a fila do worker
  enche, a mudança não entra, e o registro de "o que o provider já tem" **não**
  avança — a sincronização seguinte recalcula a diferença do mesmo ponto e tenta
  outra vez, com um pedaço maior. Nada se perde e nada bloqueia.
- **Determinismo em teste exige espera explícita.** `NativeIde::settle_syntax`,
  só em `cfg(test)`, aguarda o pendente; na janela, esperar é justamente o que se
  quer evitar.

Com isto, o item "ela é síncrona" da ADR-015 fica resolvido para o caminho da
digitação. Continua de pé para a **ativação**: a primeira consulta a uma
linguagem ainda espera a indexação inteira.

### Pendência: os ~7 ms que sobraram na tecla

Medido na janela com `ERIDE_PERF=1` depois desta decisão, uma tecla custa **cerca
de 7 ms**, e ainda quase todos dentro de `sync_languages`. Não é mais a análise —
ela agora acontece na thread do provider e nada é esperado. O que resta é o
preparo do que se posta:

`IdeShell::document_snapshots` monta um `DocumentSnapshot` por documento aberto,
e cada um **clona o texto inteiro**. Num arquivo grande, com algumas abas
abertas, isso é alguns megabytes copiados a cada caractere digitado, para que no
fim só um documento tenha mudado e só a diferença seja enviada.

Dois caminhos, nenhum tomado:

- **Só o documento que mudou.** A tecla altera um documento; os outros já foram
  sincronizados e continuam iguais. Montar o instantâneo apenas do ativo elimina
  a maior parte da cópia.
- **Texto emprestado até o ponto de envio.** O instantâneo poderia carregar uma
  referência, ou um texto compartilhado por `Arc<str>`, e só materializar o que a
  mudança realmente leva. Muda o tipo de domínio, então é a alternativa mais
  cara.

A 7 ms por tecla isso não aparece para quem digita, e por isso não foi mexido: a
correção certa é a primeira, e ela pode ser feita quando o número incomodar ou
quando arquivos maiores entrarem em jogo. Fica registrado com a medição para que
a próxima investigação não precise começar do zero.

## ADR-018 — O foco dos formulários vem do `ui-focus`

**Decisão:** as janelas da IDE param de guardar por conta própria qual campo tem
o foco e passam a usar o `FocusManager` do crate `ui-focus` da ERLibUi. A janela
de criar item o usa por inteiro — `Tab`, clique e entrega do par ganhar/perder —,
e a página de depuração das Configurações o usa para a contabilidade, já que ali
os campos são pintados e editados pela própria tela.

**Motivo:** era a mesma regra escrita duas vezes, de formas diferentes: um `bool
naming` numa janela, um `Option<WidgetId>` comparado com `DEBUG_HOST_ID` na
outra. Nenhuma das duas é errada; ter as duas é.

**Correção de rota.** A primeira versão desta ADR mandava usar um `FocusGroup`
criado em `ui-components` para este fim. Aquele tipo duplicava o `FocusManager`,
que já existia em outro crate e fazia mais — tem escopos. O `FocusGroup` foi
removido; o critério que esta ADR enuncia continua válido, mas ganhou uma
precondição: **antes de concluir que falta peça na biblioteca, é obrigatório
inventariar os crates de runtime dela** — `ui-tree`, `ui-events`, `ui-focus`,
`ui-commands`, `ui-layout-api` —, que a IDE não consome e por isso não aparecem
em lugar nenhum do código dela.

**Consequência:** uma janela nova com dois campos declara o percurso com
`register` e chama `focus_next`. A entrega de `FocusGained`/`FocusLost` continua
na IDE, marcada como temporária: ela é trabalho do anfitrião que falta à
biblioteca, e sai na fase 4 de `15-event-runtime-adoption`.

Fica ainda uma diferença entre as duas telas: a página de depuração edita o texto
por concatenação e desenha o próprio contorno de foco, em vez de deixar isso com
o `TextInput`. Unificá-la muda o que se vê — o cursor passaria a ser posicionado
pelo clique, e `Tab` a andar entre host e porta —, então é trabalho separado.

## ADR-019 — A IDE lê ações, e não texto de comando

**Decisão:** a IDE deixa de traduzir comandos em texto emitidos por componentes.
As abas passam a ser lidas por `WidgetAction::TabSelected`/`TabClosed`, e as
listas de escolha das Configurações por `ItemSelected`, distinguidas pelo
`widget_id` (ADR-017 da ERLibUi).

**Motivo:** a IDE montava um prefixo (`"toolchain.select."`, `"tool.select."`) e
o desmontava do outro lado da mesma tela, e a função `tab_command` existia só
para reverter o `format!` que o componente acabara de fazer. Duas listas na mesma
janela só se distinguiam porque alguém combinou dois rótulos distintos — quem
copiasse a linha de montagem sem trocar o prefixo teria duas escolhas gravando no
mesmo lugar, e nada acusaria.

**Consequência:** saem `TabCommand` e a tradução; a leitura passa a ser um `match`
que o compilador confere. `tab_action` continua existindo por um motivo diferente
do anterior: a janela entrega só o pressionar, e o componente espera pressionar e
soltar — é a soltura sintética que fica ali, não a tradução de texto.

## ADR-020 — As janelas pedem as áreas, e não as calculam

**Decisão:** as cinco janelas da IDE — criar item, renomear, gerar, configurações
e inspeção — passam a obter campos, legendas e a fileira de ações do `FormLayout`
da ERLibUi (ADR-018 de lá), em vez de escreverem as coordenadas.

**Motivo:** eram os mesmos números em cinco lugares, e já haviam divergido. A
janela de geração usava 100×36 com 12 pontos de folga onde as outras usavam 88×34
com 10; a diferença não vinha de decisão nenhuma, e ninguém a notaria, porque
telas que funcionam não são comparadas lado a lado.

**Consequência:** a janela de geração mudou 2 pontos — os botões desceram e se
aproximaram, e a lista dela ficou 2 pontos mais alta. É a única diferença visível
da mudança, e vale registrá-la: unificar medidas que divergiram significa escolher
uma, e quem escolhe deve dizer qual. O botão maior dela continua maior, por
`with_action_size` — o que se unificou foi a fileira, não o tamanho do rótulo.

## ADR-022 — A IDE não desenha

**Contexto.** Quando a dívida foi medida pela primeira vez, a IDE emitia **48**
primitivas cruas — retângulos e contornos desenhados à mão, ao lado de componentes
da biblioteca. Cada uma era uma cor fora do tema, uma medida que discordava do
texto real e um elemento invisível para o leitor de tela.

O número foi caindo à medida que a ERLibUi ganhava o que faltava: painel, ícones,
console, corte de texto medido. Chegou a 2, e ali empacou — uma divisória de 1 px
e um contorno de foco. Nenhuma das duas era teimosia: faltava borda **por lado** na
`Panel`, e faltava a página de depuração entregar o foco ao próprio campo em vez
de contorná-lo por fora.

**Decisão.** A IDE não emite primitiva. O teto virou **zero**, e o teste mudou de
nome: `the_ide_does_not_draw`. Os atalhos `raw_fill` e `raw_stroke` foram apagados
— enquanto existirem, desenhar à mão fica a uma linha de distância.

**Consequência.** O que falta na biblioteca é pedido a ela. Foi assim que saíram as
duas últimas: a `Panel` ganhou `with_borders(EdgeInsets)` e a divisória virou a
borda esquerda da superfície; o campo de depuração passou a receber `FocusGained`
e a desenhar o próprio foco — de quebra, com o cursor que antes não aparecia.

Zero é diferente de um número pequeno: não há mais o que negociar quadro a quadro.
A regra está escrita também do lado da biblioteca, em `01-product-vision`.

## ADR-021 — A IDE não estima largura de texto

**Decisão:** a mensagem da janela de inspeção passa a ser um `Label` com
`with_max_width` (ADR-019 da ERLibUi). Somem a função `clipped_message` e a
constante `INSPECTION_MESSAGE_CHAR_WIDTH`, que valia 6.6.

**Motivo:** o 6.6 era a largura média de um caractere, medida uma vez e escrita à
mão. Uma mensagem de letras estreitas ficava com folga sobrando; uma de letras
largas passava da borda — e a mensagem longa é a que explica por que a execução
falhou. A IDE nunca teve como medir texto: quem mede é o componente, e a medição
chega a ele pelo contexto de pintura.

**Consequência:** é a quarta lacuna da biblioteca encontrada pelo mesmo caminho —
a IDE resolvendo à mão o que a biblioteca deveria oferecer. Nas quatro, o sinal
foi o mesmo: um número mágico ou uma mecânica repetida. Vale como regra de
inspeção do que ainda resta.

## ADR-023 — O índice é lido, e não mapeado

**Decisão:** o índice em disco é carregado para um vetor de bytes, e as consultas
respondem a partir desses bytes. **Não** há mapeamento de memória, embora a fase 2
da `20` o tivesse pedido pelo nome.

**Motivo:** `memmap2::Mmap::map` é `unsafe`, e o workspace declara
`unsafe_code = "forbid"` em `[workspace.lints.rust]`. Mapear é `unsafe` por uma
razão concreta, não formal: os bytes lidos são páginas do arquivo, e um processo
que trunque esse arquivo com o mapeamento aberto faz as páginas sumirem debaixo
do programa. Não é erro tratável — é o processo morrendo ou lendo lixo. O risco
não é hipotético aqui: duas IDEs abertas na mesma raiz, uma regravando o índice
enquanto a outra o tem aberto, está listado entre os riscos da própria `20`.

**O que se examinou antes de decidir:**

- **`#[allow(unsafe_code)]` na função que mapeia** não existe. `forbid` é o nível
  que **não pode** ser desligado localmente; um `allow` interno é erro de
  compilação. A exceção obrigaria o `language-java` a deixar de herdar a regra do
  workspace e declarar uma sua, mais fraca — não é exceção pontual, é um crate
  inteiro saindo da garantia mais forte do projeto;
- **o que se ganharia** é a elasticidade: com mapeamento, o sistema operacional
  recupera as páginas frias sob pressão, e a IDE deixaria de reter os 103 MB do
  índice. Some também a leitura de 78 MB na abertura, que custa 34 ms — irrelevante
  perto dos 3,6 s que a conferência leva;
- **o que se perde** é a regra deixar de ser absoluta, que é de onde vem o valor
  dela.

**Consequência:** a memória do índice é **reduzida, não emprestada**. Foi de
178 MB para 103 MB, e esses 103 MB ficam retidos enquanto a IDE viver. O prêmio
secundário anunciado no começo da `20` — memória elástica — não foi entregue, e a
especificação diz isso onde antes prometia.

**O caminho de volta é curto, e isso é de propósito.** Tudo abaixo do
carregamento trabalha sobre `&[u8]`, não sobre o que produziu esses bytes: trocar
`fs::read` por `Mmap::map` é uma linha. No dia em que a decisão mudar, nada mais
muda.

**Precedente:** é o segundo caso em que a regra custa uma capacidade. O primeiro
está na ADR-013 — `GenerateConsoleCtrlEvent` exigiria Win32 direto, e o botão de
parar continua sem interromper a aplicação. Nos dois, o comportamento desejado
ficou escrito em vez de apagado, para voltar a valer se a decisão for revista.

## ADR-024 — Git é uma crate, e a IDE não sabe como ele fala com o Git

**Decisão:** o suporte a Git é **uma** crate, `ide-git`, com as capacidades como
módulos internos. Os adapters — hoje a linha de comando — são `mod` privados. A
IDE conhece os conceitos do Git e depende dos traits públicos; ela nunca vê
processo, argumento, `stderr` ou biblioteca concreta. Ver a `22`.

**Motivo:** o que separa a IDE da implementação não é a fronteira de crate, é a
privacidade de módulo. `pub(crate)` faz o compilador garantir o que seria
disciplina, e uma crate a mais não acrescentaria encapsulamento — acrescentaria
arquivos. A regra da `12` decide o resto: crate para fronteira de dependência,
substituição, isolamento ou distribuição; módulo para o que é compilado e
alterado como uma unidade só. Git é o segundo caso.

**O que se examinou antes de decidir:**

- **Separar contrato e implementação em duas crates**, como a ERLibUi faz em
  `ui-text-api`/`ui-text-cosmic`, resolve um problema que aqui não existe. Lá o
  contrato é consumido por muitas crates e nenhuma pode arrastar `cosmic-text`
  junto. Aqui o consumidor é um, e a troca de backend é um `feature` interno —
  não uma dependência diferente no `Cargo.toml` de quem consome;
- **Esconder o Git atrás de um contrato genérico de versionamento**, para que o
  núcleo não saiba se é Git, Mercurial ou SVN, custa o modelo de domínio.
  *Index*, *rebase*, *stash* e *detached HEAD* não existem nos outros e não
  sobrevivem inteiros à generalização; o que resta é um denominador comum com o
  qual não se desenha tela nenhuma. É a fragmentação prematura que a `12` recusa,
  paga hoje por um segundo sistema de versionamento que ninguém pediu;
- **Uma crate por capacidade** — branch, commit, merge — foi descartada pelo
  mesmo argumento: elas evoluem e são distribuídas juntas.

**Consequência:** a IDE **sabe** que existe Git, e isso é deliberado. Branch,
commit, stage e conflito aparecem nos menus e no vocabulário de quem usa; fingir
que não estão lá tornaria o código mais pobre que a tela. O que ela não sabe é
como esses conceitos viram chamadas.

**O caminho de volta é curto, e isso é de propósito.** No dia em que um segundo
sistema de versionamento entrar, os traits e os tipos de domínio mudam de crate e
`ide-git` passa a implementá-los. Nenhuma linha de lógica muda. Vale o mesmo
raciocínio da ADR-023: decisão barata é a que se revisa sem reescrever.

**A regra tem uma guarda, porque disciplina sozinha não segura.** Nenhuma crate
fora de `ide-git` pode mencionar `Command::new("git")`, `git2` ou `gix`, e um
teste falha no dia em que alguém tomar o atalho. Sem ela, o primeiro `push` com
pressa vira uma chamada direta no painel, e a fronteira passa a existir só no
documento.

## ADR-025 — O analisador de uma linguagem pode morar fora do processo, e nunca ser obrigatório

**Decisão:** o suporte a TypeScript tem **dois** providers. `typescript.syntax` é
nativo, sobre tree-sitter, e responde por realce, estrutura, símbolos e navegação
por nome. `typescript.service` fala com o `tsserver`, num processo Node, e
responde com tipo. O externo é o principal quando existe; o nativo é o chão, e
não sai. Ver a `23`.

**Motivo:** o sistema de tipos do TypeScript — condicionais, mapeados, literais de
template, estreitamento por fluxo — não é uma fase de trabalho, é um projeto do
tamanho da IDE. Reimplementá-lo não está no orçamento de nenhuma versão previsível,
e entregar meia verificação seria pior do que nenhuma: diagnóstico errado em código
certo é a resposta velha com cara de resposta nova, que a `21` já nomeou como a
família de defeito mais perigosa.

**A regra que isto parecia violar, e por que não viola.** A `00` diz que
ferramenta externa serve "para compilar, executar ou depurar projetos do usuário,
nunca para implementar a IDE", e analisar não está na lista. A `04`, por outro
lado, lista `jdtls-adapter` e `remote-java-service` como providers legítimos. Os
dois documentos discordavam desde que foram escritos, e ninguém tinha exercido o
caso.

A fronteira é o **núcleo**, e não a tecnologia. Interface, editor, host, índice,
modelo de projeto e infraestrutura são nossos e continuam sendo. Um
`LanguageProvider` é, pela ADR-003, substituível — e a `04` já previa provider
externo pelo nome.

**A consequência é a que dá dente à decisão:** nenhuma capacidade da IDE pode
depender de o usuário ter Node instalado. Sem ele, um projeto Angular abre,
destaca, navega e roda as tarefas que não precisam dele. É degradação, como a do
observador da `21` quando não consegue observar — a IDE volta a ser menos, e não
deixa de ser.

**Por isso o provider nativo não é provisório.** Ele não é o andaime que sai
quando o externo chegar; ele é o piso que fica embaixo. Um provider externo que se
tornasse a única resposta transformaria uma dependência opcional em requisito de
instalação, e aí sim a regra da `00` teria sido quebrada — não por onde o código
roda, mas por ter deixado de haver IDE sem ele.

## ADR-026 — A ferramenta escolhida é de uma seção, e não de um campo

**Decisão:** `ToolchainConfig` deixa de ter `jdk_home` e `maven_home` como campos
e passa a guardar escolhas por `LanguageId` e por papel — o principal e o
secundário que `SettingsSection` já declara —, com **padrão global e sobreposição
por raiz de workspace**. Os comandos de configuração passam a dizer de qual seção
falam. `classpath_entries` sai do `TaskExecutionContext` genérico.

**Motivo:** o formato antigo cresce com o número de linguagens, o que é a
definição de não ser neutro. Node exigiria um terceiro campo e npm um quarto, e
`UiAction::BrowseSecondaryTool` — que é genérico no contrato — chamava
`choose_maven_home` direto na aplicação, porque com uma seção só não havia
ambiguidade a resolver.

**Por que a escolha é por projeto, e não só por linguagem:** Angular 11 e Angular
15 exigem Node de faixas diferentes, e a CLI de cada um recusa a do outro — quem
tem os dois projetos não tem um Node que sirva aos dois. E o caso não nasceu com
TypeScript: um projeto em Java 8 e outro em Java 21 sempre tiveram o mesmo
problema, resolvido na mão até hoje. A sobreposição mora na configuração da IDE, e
**não** dentro do repositório, porque um caminho de instalação é específico da
máquina: escrevê-lo no projeto o tornaria inútil para qualquer outra pessoa, e
ainda criaria arquivo a ser comitado sem ninguém pedir.

**O que isto revela, e é o mais útil da decisão:** o contrato estava certo e a
fiação é que era curta. `SettingsSection::secondary_caption` já previa por escrito
"em Java é o Maven; em outra linguagem será outra coisa". A abstração foi bem
desenhada e nunca exercida, e **uma abstração com uma implementação só é uma
hipótese**. A segunda linguagem é o experimento.

**Consequência:** a guarda cresce junto, porque foi ela que deixou passar.
`neutral_crates_expose_no_language_specific_public_api` não incluía `ide-core` —
exatamente a crate onde `jdk_home` mora — e só examinava linhas de declaração de
item público, o que nunca alcançaria um campo de struct. Corrigida, ela **falha no
código de hoje**, e é assim que se sabe que passou a valer alguma coisa.

## ADR-027 — Quais arquivos compõem o projeto é o `tsconfig.json` quem diz

**Decisão:** em TypeScript, as raízes de fonte do `ProjectModel` são **importadas
do `tsconfig.json`**, e não deduzidas por convenção. O `BuildSystemAdapter` lê o
arquivo — com `extends`, `include` e `exclude` — e produz o modelo a partir dele.
Ninguém do nosso lado adivinha que o código está em `src/`. Ver a `23`.

**Motivo:** o `tsserver` faz descoberta própria, subindo do arquivo aberto até o
`tsconfig.json` mais próximo. Se o nosso modelo deduzisse as raízes por conta, a
IDE teria **duas definições de qual é o projeto**, e elas discordariam em casos
que não são raros: monorepo com vários `tsconfig`, `references`, testes excluídos
do build, `paths` remapeando módulos, arquivo fora do `rootDir` puxado por um
`import`.

A discordância seria silenciosa, que é a pior forma — o índice responde sobre um
arquivo que o analisador considera fora do projeto, a navegação leva a um lugar
sobre o qual a completação não sabe nada, e uma renomeação reescreve um arquivo
que o compilador nunca vê. É a família de defeito que a `21` chama de resposta
velha com cara de resposta certa.

**A origem única é o arquivo, e não um processo — e isso é a parte importante.**
Nós lemos o `tsconfig.json`; o `tsserver` lê o mesmo `tsconfig.json`. O modelo de
projeto **não** pergunta ao analisador, porque isso criaria uma dependência do
núcleo a uma ferramenta externa, exatamente o que a ADR-025 proíbe: sem Node, a
IDE deixaria de saber o que é o projeto.

**Consequência:** o nosso leitor é aproximado e o deles é exato. O formato aceita
comentários, `extends` encadeado e vírgula sobrando, e a lista efetiva exige
expandir globs — vamos errar em algum canto.

Mas errar contra a mesma fonte é um **defeito com forma conhecida e testável**,
enquanto duas definições diferentes seriam desacordo por desenho, que nenhum teste
apanha porque os dois lados estão certos. Daí sai a verificação: com o analisador
de pé, comparar a nossa lista de arquivos com a que ele reporta, e tratar
divergência como defeito nosso.

**Precedente:** vale para qualquer linguagem cujo projeto esteja declarado num
arquivo, e não numa convenção de diretório. Java é o caso oposto — `src/main/java`
é convenção do Maven, e o `pom.xml` a confirma —, e é por isso que a armadilha só
apareceu agora.

## ADR-028 — O analisador é o que o projeto fixa, e hoje isso é o `tsserver`

**Decisão:** a porta de análise de TypeScript admite mais de um adapter, e qual
deles sobe é resolvido na abertura, a partir do que está no `node_modules` do
projeto. O adapter construído agora é o **`tsserver`**, sobre Node, falando o
protocolo próprio dele por stdin/stdout. O `tsgo` — a porta nativa em Go, que fala
LSP — fica registrado como o segundo adapter, para quando fizer sentido. Ver a
`23` e a `24`.

**Motivo:** a versão do analisador não é escolha nossa. Cada versão do Angular
fixa uma faixa estreita de TypeScript — a 11 na casa do 4.1, a 15 do 4.9, a 19 do
5.6 —, e o `tsgo` é a porta do TypeScript do 5.8 em diante. Analisar um projeto
presos ao 4.1 com um 7.x daria respostas que não batem com o build, e "a versão do
TypeScript é do projeto" é regra da `23` justamente por isso.

O caso concreto que decidiu: há projetos em Angular 11 e em Angular 15 em uso.
Nenhum dos dois é atendível pelo `tsgo`, nem hoje nem quando ele amadurecer.

**O que se examinou antes de decidir:**

- **Padronizar no `tsgo`** era atraente por três razões — cerca de metade da
  memória da versão em JavaScript nos números publicados pela Microsoft, LSP
  nativo em vez de protocolo próprio, e binário autocontido, que dispensaria o
  runtime de Node e afrouxaria a tensão da ADR-025. Nenhuma delas sobrevive ao
  parágrafo acima: um analisador que não atende os projetos existentes não é uma
  otimização, é uma regressão;
- **Angular no `tsgo`** não existe e não é iminente. O próprio Angular declara que
  o suporte está em prototipagem e exige mudanças arquiteturais grandes, porque a
  integração deles com a API do compilador de TypeScript é das mais profundas que
  existem;
- **Escrever a nossa engine de tipos** é a ADR-025, e continua recusado.

**Consequência:** o Node permanece requisito do caminho externo, e a tensão da
ADR-025 **não** se dissolve — ela fica inteira, resolvida como já estava: o
provider nativo é o chão, e sem Node a IDE degrada em vez de parar.

**O que salva o custo de suportar as duas pontas:** o protocolo do `tsserver` é
estável ao longo dessas versões. Um adapter só atende TypeScript 4.1 e 5.6 sem
ramificar por versão — o que muda entre projetos é qual arquivo se executa, não
como se conversa com ele. É por isso que "o analisador é o do projeto" custa
pouco, e é o que torna a decisão sustentável.

**Verificado, e não mais suposto.** Isto era argumento — "o protocolo é estável" —
enquanto todo teste rodava contra o 5.x, e ficou registrado como pendência por
isso. Um projeto fixado em **TypeScript 4.1** passou a ser exercido pelo caminho
que mais mudaria se a aposta estivesse errada: localizar, subir, completar, buscar
tipo e mudar por intervalo. Nenhuma ramificação por versão foi necessária, e
nenhuma existe no código.

O teste confere duas coisas que um teste ingênuo deixaria passar: que o pacote
instalado é **mesmo** o 4.1 — `npm install` de uma versão indisponível resolveria
para outra, e o teste passaria falando do 5.x de novo — e que o analisador
executado veio **de dentro do projeto**, e não de um `node_modules` numa pasta
acima, que é precisamente a confusão que esta ADR existe para evitar.

**O caminho de volta existe e está nomeado.** No dia em que os projetos migrarem e
o Angular estiver portado, entra o segundo adapter atrás da mesma porta, e o
primeiro fica para quem ficou para trás. É a composição de capacidades da `04`,
um nível abaixo de onde ela foi escrita.

## ADR-029 — O template é respondido pelo plugin dentro do `tsserver` que já sobe

**Decisão:** o suporte a `.html` de Angular será feito carregando o
`@angular/language-service` como **plugin do `tsserver` do projeto**, e
perguntando sobre o template com **`projectFileName`** nomeando o `tsconfig` do
componente irmão. O pacote do plugin **viaja com a IDE** — 14 MB, apontados por
`--pluginProbeLocations` —, e é usado quando o projeto não o tem. **Nenhum
processo a mais.** Ver a fase 1 da `24`.

**Motivo:** é o único arranjo medido que responde pelo template sem dobrar a
memória nem exigir um parser nosso.

| arranjo | pico | processos Node | o que envelhece conosco |
| --- | --- | --- | --- |
| `tsserver` sozinho, hoje | 1906 MB | 1 | nada |
| **`tsserver` + plugin** | **~2290 MB** | **1** | nada |
| `tsserver` + `ngserver` (VS Code) | ~4,1 GB | 2 | nada |
| parser próprio (IntelliJ) | — | 1 | **um parser por revisão de sintaxe** |

**+385 MB no processo que já existe**, contra +2,1 GB num segundo. O `ngserver`
não serviria de substituto de qualquer forma: `angularOnly: true` é fixo no
código dele, e uma completude após `this.` num `.ts` devolve zero itens.

**A correção é um campo, e isso foi isolado.** A sondagem anterior concluiu que o
`tsserver` não encaminha perguntas sobre `.html` ao plugin. Está errado: o `.html`
cai num **projeto inferido**, e o plugin é consultado sobre um template órfão.
Nomeando o projeto configurado na pergunta, ele responde 20 membros reais do tipo.
Nem `extraFileExtensions` nem a dança de abrir e fechar o componente — ambos
presentes no `ngserver` — são necessários; ligando e desligando cada peça, só
`projectFileName` importa.

**A regra da 028 fica de pé onde importa.** O `tsserver` continua sendo o do
projeto, e é ele quem decide se um tipo bate. O que a IDE passa a carregar é o
plugin, e só quando o projeto não o tem — um dos cinco projetos locais o tem. Um
language-service 21.2.17 nosso serviu um projeto Angular 21.2.6 sem ressalva.

**O que ela custa em tempo:** o plugin aproximadamente dobra a carga do projeto —
30 s viram 46 a 70 s no `spartacus-develop`. Ele não deve subir em projeto sem
Angular, e a `24` diz como reconhecer um.

**O que ela não resolve:** realce e estrutura do `.html` continuam sendo do
provider nativo, e `@if`, `@for` e `@defer` continuam sendo texto comum até a
seção "Linguagem dentro de linguagem" ser feita. O plugin responde tipo, e não
sintaxe.
