# 21 — O que muda fora da IDE

## Situação

São **dois** problemas, e convém não confundi-los, porque um deles eu já
confundi.

**1. A IDE aberta ignora o disco.** O índice é atualizado quando você grava pela
IDE, e quando ela abre. Entre uma coisa e outra, nada. Trocar de branch,
`mvn clean`, gerar código, editar num outro programa — a completação, o
Ctrl+clique e a busca por tipo continuam respondendo pelo código anterior, **sem
avisar**. É a família de defeito que a `19` chamou de a mais perigosa: quem usa
não distingue uma resposta velha de uma resposta certa.

**2. A conferência da abertura custa uma varredura.** A fase 4 da `20` compara
data e tamanho de cada fonte para achar o que mudou com a IDE fechada. São
**3,7 s** sobre 26.211 fontes no projeto de referência, e o tempo cresce com o
número de arquivos, não com o número de mudanças.

## A correção que motiva esta especificação

O fim da `20` diz que um observador de sistema de arquivos eliminaria a varredura
da abertura. **Não elimina**, e a frase foi corrigida lá.

Um observador só vê o que acontece enquanto ele está rodando. As mudanças que a
abertura precisa descobrir são exatamente as que aconteceram com a IDE
**fechada** — ninguém estava observando. A varredura existe para isso, e continua
existindo.

Os dois problemas são independentes, e cada fase resolve um.

## Fase 1 — O observador ✅

O sistema operacional avisa quando um arquivo muda, e o evento chega a
`reindex_file`, que já existe e custa **3,5 ms** por fonte.

### O filtro é o que já existe

`collect_workspace_paths` pula `.git`, `target`, `node_modules` e `.gradle`, e a
varredura só considera `.java` dentro das raízes de fonte. O observador usa
**esse mesmo teste**, extraído para um lugar só.

Isto não é economia de código, é correção. Dois filtros discordam um dia, e a
discordância tem forma conhecida: o arquivo muda, a varredura diz que interessa,
o observador diz que não, e o índice envelhece sem ninguém saber.

### O tempo de espera é o silêncio

Não se reage a evento; acumula-se e reage-se à calmaria:

1. chega evento → o caminho entra num conjunto, um relógio de ~300 ms reinicia;
2. chega outro → mesmo conjunto, e repetido some sozinho; relógio reinicia;
3. passaram 300 ms sem evento → o lote é reindexado.

Resolve os dois extremos com uma regra só. Gravar um arquivo dispara três ou
quatro eventos no Windows, e eles viram um. Um `mvn clean install` gera milhares
durante um minuto: como o relógio só dispara no silêncio, a reação vem **depois**
do build, uma vez, com a lista já sem repetição.

Mil arquivos alterados custam 3,5 s em segundo plano, sem ninguém esperando.

### Quantas pastas observar não é uma pergunta

No Windows, `ReadDirectoryChangesW` observa uma pasta **e toda a subárvore** com
um registro só. Observa-se a raiz do projeto, e acabou — um watch, seja o projeto
de dez arquivos ou de sessenta mil. O macOS é igual. O `notify` expõe isso como
`RecursiveMode::Recursive`.

A pergunta só existe no **Linux**, e a seção seguinte trata dela.

**Falhar em observar não pode quebrar nada.** É degradação, não erro: sem
observador, a IDE volta a ser o que é hoje.

### Como cada plataforma observa

A IDE roda hoje no Windows, mas a arquitetura não é de uma plataforma só — o
`06-lifecycle-and-processes` já trata processo e terminal por porta. O observador
segue o mesmo caminho: o `notify` resolve a diferença de API, e o que **não** se
resolve sozinho está abaixo.

| | Windows | macOS | Linux |
|---|---|---|---|
| API | `ReadDirectoryChangesW` | FSEvents | `inotify` |
| recursivo | sim | sim | **não** |
| registros para um monorepo | 1 | 1 | um por pasta |
| limite | tamanho do buffer | — | `max_user_watches` por usuário |
| pode perder evento | sim, buffer cheio | sim, na retomada | sim, fila cheia |

**Linux é onde a pergunta existe.** O `inotify` registra pasta por pasta, contra
um limite por usuário que costuma ser 8.192 — um monorepo passa disso sem
esforço. A ordem de recuo, do melhor para o pior:

1. observar a raiz inteira, se couber no limite;
2. observar só as **raízes de fonte**, que é onde os `.java` vivem e onde o
   índice olha;
3. não observar, e ficar com a varredura da abertura.

Quem decide não é a configuração: é a tentativa. Registrar falha com erro
conhecido, e o passo seguinte é tentado. Nada disso pede número escolhido à mão,
que envelheceria em silêncio quando o projeto crescesse.

**macOS tem uma janela própria.** O FSEvents já entrega eventos agrupados, com
uma latência que se escolhe ao criar o fluxo. São **dois** tempos de espera
somados — o dele e o nosso de 300 ms. Escolher latência baixa lá e deixar o nosso
fazer o agrupamento mantém o comportamento igual nas três plataformas, em vez de
duas somas diferentes.

**Todas as três podem perder evento.** O buffer do Windows enche, a fila do
`inotify` estoura, o FSEvents entrega um aviso de que houve mudanças demais.
Nenhuma plataforma promete que você viu tudo.

Isso não é detalhe de implementação, é o que decide o desenho: **a varredura
completa continua sendo a rede**. Perder evento leva a IDE de volta ao
comportamento de hoje — índice velho até a próxima abertura — e não a um estado
inventado. Quando a biblioteca avisar que perdeu, a resposta é uma varredura,
que já existe e já é a mesma da abertura.

### O que difere além da API

Estas três não aparecem no Windows e apareceriam no primeiro dia de Linux ou
macOS. Ficam registradas para não serem descobertas por defeito:

- **Maiúsculas.** O Linux distingue `Pedido.java` de `pedido.java`; o macOS, por
  padrão, não distingue mas preserva o que foi escrito. O índice guarda caminhos
  como texto e os compara assim. Comparar caminho vindo de evento com caminho
  gravado tem de usar a mesma regra da plataforma, senão um arquivo alterado
  parece um arquivo novo.
- **Ligações simbólicas.** No macOS, `/tmp` é ligação para `/private/tmp`, e o
  FSEvents relata o caminho real. Se o índice gravou o caminho por onde a
  varredura passou e o evento traz o outro, os dois não se encontram. Os dois
  lados precisam ser reduzidos à mesma forma.
- **Gravar renomeando.** Vários editores gravam num temporário e renomeiam por
  cima — é o que a própria `20` faz com o arquivo do índice. Isso chega como
  criar mais renomear, e não como alterar. Como a reação acontece sobre o lote
  depois do silêncio, e o que se faz com cada caminho é **relê-lo do disco**, a
  forma do evento não importa: importa o caminho.

### Sobre a dependência

Entra o `notify`, ou equivalente — ele cobre as três plataformas e ainda traz
um observador por **sondagem**, que percorre a árvore de tempos em tempos. Esse é
o mesmo recuo que já temos pela varredura, e serve onde nada mais funciona.

Vale registrar por que ele passa onde o mapeamento não passou: `unsafe_code = "forbid"` proíbe **nós** escrevermos
`unsafe`, não proíbe depender de bibliotecas que o usem por dentro. O mapeamento
travou porque o `unsafe { Mmap::map(...) }` ficaria no nosso código; um observador
expõe interface segura. Não é exceção à regra — é a regra, lida direito. Ver
ADR-023.

**Critério:** alterar um `.java` fora da IDE, com ela aberta, faz a completação e
a navegação responderem pelo texto novo sem nenhuma ação do usuário. Um build
completo não trava a IDE nem dispara mil reindexações separadas. E **não observar
responde como hoje**: sem observador — por limite, por falha ou por plataforma —
a IDE continua correta, com o índice envelhecendo até a próxima abertura.

### Feita

**Medido:** uma rajada de 400 arquivos escritos em 283 ms — 200 fontes mais 200
em `target/`, que a indexação ignora. O índice ficou em dia **686 ms depois do
início da rajada**: os 300 ms de silêncio mais a releitura dos 200 fontes. Uma
reação, nada perdido, e o barulho da pasta ignorada não entrou.

O filtro saiu para `fonte_java` e `caminho_ignorado`, no `index`, e é **o mesmo**
que a varredura aplica. `the_watcher_and_the_scan_agree_on_what_matters` afirma
isso caminho a caminho, inclusive os que não interessam.

`what_changes_on_disk_reaches_the_index_by_itself` cobre o critério nas duas
direções — a classe criada fora da IDE entra sozinha, a apagada sai — e foi
verificado que ele **falha** com o observador desligado.

**Três decisões que o código tomou e que vale registrar:**

- **o observador nasce depois do índice.** Um evento chegando durante a varredura
  reindexaria contra um índice que ainda vai ser substituído inteiro, e o
  trabalho se perderia. Vale para as duas partidas: a que constrói e a que
  carrega do arquivo.
- **falhar em observar é silencioso.** `Observador::iniciar` devolve `None` e
  ninguém trata erro nenhum — porque não há erro: a IDE volta a ser o que era. A
  ordem de recuo do Linux está no código, na mesma função.
- **perder evento cai na varredura.** Quando a biblioteca avisa que perdeu, a
  resposta é `diferenca` mais reconciliação — as mesmas da abertura. Perder não
  vira índice inventado.

**O que a fase custou:** um guarda de arquitetura, que limita a fachada do
`language-java` a 12 linhas. Ela foi para 13 por causa de uma linha de `mod`, que
é exatamente o que a fachada deve ter. O teto subiu com a razão escrita ao lado.

**E o observador não mora mais aqui.** Este texto descreve onde ele nasceu, e
nascer dentro do índice de Java estava certo: quando nasceu, era dele. A fase 4
da `22` o tirou de lá — a crate `ide-watch`, com consumidores registrados —
porque o débito que **esta** especificação anotou se realizou: a árvore do
Explorer era o segundo consumidor sem dono, o Git virou o terceiro, e três é
quando um observador deixa de ser detalhe de um indexador.

O que mudou de lugar foi o registro e a espera pelo silêncio; o que não mudou é
o resto desta seção. As regras continuam sendo as mesmas, e os 300 ms também.

## Fase 2 — A varredura em paralelo ✅

Os segundos da conferência são perguntas independentes ao sistema de arquivos,
feitas uma a uma numa linha de execução só. Distribuí-las é o que há de mais
simples: `std::thread::scope`, sem dependência nova, sem mudar o que a
conferência responde.

Vem depois do observador de propósito. O observador tapa um buraco de
**correção**; este é tempo, e tempo que ninguém está esperando de olho na tela —
a IDE responde durante a conferência desde a fase 2 da `20`.

**Critério:** a conferência responde o mesmo, em fração do tempo. Medido no
projeto de referência, contra os 3,7 s de hoje.

### Feita, e a medição mudou o plano

Este texto dizia que os segundos eram "26 mil perguntas ao sistema de arquivos".
Medindo antes de mexer, não eram:

| | |
|---|---|
| caminhar pelos diretórios | **4,18 s** |
| 26.211 consultas de data e tamanho | 0,76 s |
| filtrar | 0,03 s |

O caro é **abrir diretório**, não perguntar pelo arquivo — cinco vezes mais. O
plano apontava para o lado errado, e teria comprado 20% do que comprou.

A caminhada virou uma fila de diretórios com tantos trabalhadores quantos a
máquina tem. Depois dela, as consultas de metadados também foram distribuídas,
porque aí sim elas passaram a ser a maior parte do que sobrou.

| | |
|---|---|
| conferência, antes | **4,66 s** |
| só a caminhada em paralelo | 1,32 s |
| e as consultas também | **0,70 s** |

Seis vezes e meia, e o número é estável — três execuções entre 692 e 724 ms.

**O resultado passou a sair ordenado.** Custa poucos milissegundos e compra
determinismo: antes a ordem era a que o sistema de arquivos entregasse, e dela
dependia qual arquivo ganha quando dois declaram o mesmo nome simples. Com
trabalhadores em paralelo isso deixaria de ser arbitrário para ser instável, que
é pior. Ordenar resolve os dois, e é melhor do que era antes da fase.

**A contagem de quem está abrindo é o que termina o laço.** Fila vazia não quer
dizer que acabou: pode haver alguém abrindo um diretório que ainda vai produzir
subpastas. Só quando a fila está vazia **e** ninguém está abrindo é que acabou
para todos — e é aí que os trabalhadores acordam uns aos outros para sair.

**Uma armadilha de medição, que vale registrar.** A instrumentação que separava
caminhada de metadados rodava antes da conferência e esquentava o cache do
sistema de arquivos para ela. Os números soltos oscilaram de 487 ms a 2,9 s entre
execuções enquanto a conferência inteira ficava em 0,7 s — comparar as duas seria
comparar coisas diferentes. A instrumentação saiu; o que ficou é a conferência
inteira, medida do mesmo jeito nas três vezes.

## O que fica de fora, e por quê

**O diário de alterações do NTFS** (USN Journal) resolveria o problema 2 de vez:
o volume registra toda mudança com um número de sequência, e a abertura
perguntaria "o que mudou desde o número tal?", recebendo uma lista proporcional
às mudanças e não ao número de arquivos. Mas é Win32 direto, e o `unsafe` ficaria
no nosso código — o mesmo muro da ADR-023 e da ADR-013. Fica registrado como o
caminho que existe e não está aberto.

## Riscos

- **Enxurrada de eventos.** Um build gera milhares de mudanças em pastas que já
  ignoramos. O filtro corta antes de o caminho entrar no conjunto, e o tempo de
  espera cuida do resto — mas o custo de *receber* os eventos existe, e é do
  sistema operacional. Se incomodar, o recuo é não observar as pastas ignoradas.
- **Reindexação em dobro.** Gravar pela IDE já chama `file_changed`; o observador
  verá a mesma gravação. É idempotente, e o conjunto de espera absorve — mas vale
  saber que acontece.
- **Renomear e mover** chegam como apagar mais criar, e às vezes fora de ordem.
  Como a reação é sobre o lote inteiro depois do silêncio, a ordem dentro dele
  não importa.
- **Uma plataforma que não seja a de desenvolvimento envelhece sem aviso.** O
  Linux e o macOS entram no desenho e não no teste diário — o que está escrito
  aqui é análise, e vira medição no dia em que alguém rodar lá.
- **A árvore do Explorer não é assunto desta especificação.** Ela também não sabe
  de mudanças externas, e o mesmo observador poderia servi-la. Fica anotado, e
  fora do escopo: são consumidores diferentes do mesmo evento.

## O que não muda

- **a varredura da abertura continua existindo.** O observador não a substitui, e
  a fase 2 apenas a barateia;
- **`reindex_file` continua sendo quem reindexa.** O observador só decide quando
  chamá-lo;
- **falhar em observar degrada, não quebra.**

## Verificação

Cada fase termina com `cargo test --workspace` e
`cargo clippy --workspace --all-targets -- -D warnings`.

E com número medido, como a `19` e a `20` fizeram:

| fase | o que medir |
|---|---|
| 1 | tempo entre gravar fora da IDE e a resposta mudar; eventos absorvidos num build; quantos registros o Linux precisou e se coube no limite |
| 2 | tempo da conferência, contra os 3,7 s de hoje — **feito: 4,66 s → 0,70 s** |
