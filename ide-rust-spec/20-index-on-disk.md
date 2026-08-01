# 20 — Índice no disco, memória como cache

## Situação

O índice Java é montado **do zero a cada abertura da IDE** e vive inteiro em
memória até ela fechar. Não há nada em disco: nenhuma serialização, nenhum cache.

Medido no projeto de referência — `camel-main`, 26.211 fontes:

| | |
|---|---|
| montar o índice, disco frio | **283 s** |
| montar o índice, disco quente | 42 s |
| memória do índice | **178 MB**, fixos enquanto a IDE viver |
| declarações | 339.664 |
| ocorrências de nomes | 2.741.995 |
| nomes distintos referenciados | 77.907 |
| tipos declarados | 30.745 |
| classes externas | 22.951 |

A especificação `19` tirou o bloqueio: nada disso está no caminho de ninguém, a
ativação volta em menos de 250 ms e a indexação acontece em segundo plano. O que
sobrou é **retrabalho** — os 283 s são pagos de novo a cada abertura, sobre um
projeto que não mudou.

## O que se ganha, e o que não se ganha

Convém dizer isto antes das fases, porque o motivo errado leva ao desenho errado.

**O prêmio é o tempo de abertura.** Persistir troca reconstruir por ler. Os 283 s
viram o tempo de mapear um arquivo, que é imediato.

**O prêmio secundário é a memória virar elástica.** Hoje os 178 MB são retidos
mesmo com a IDE parada. Com o índice mapeado, as páginas efetivamente lidas ficam
residentes e o sistema operacional recupera as frias quando outro programa precisa
de memória. Não é "menos memória" — é memória **emprestada em vez de retida**, o
que é melhor.

**O que não se ganha:** redução garantida do número. Um cache só economiza quando
o conjunto de trabalho é menor que o índice inteiro. Ver a ressalva sobre a ordem,
abaixo.

## O princípio

Toda a economia depende do **conjunto de trabalho** de cada consulta:

| consulta | o que toca | conjunto de trabalho |
|---|---|---|
| Ctrl+clique, definição | um nome | punhado de registros |
| referências, renomear | um nome | dezenas a milhares |
| completação | **todas** as 339.664 declarações, a cada tecla | tudo |
| busca por tipo (Ctrl+N) | **todas**, filtrando por trecho | tudo |

As duas primeiras são o caso em que o desenho brilha. As duas últimas, do jeito
que estão escritas hoje, tocam o índice inteiro — e aí não há política de cache
que ajude, porque não há nada a descartar.

Por isso a fase 3 existe: ela não é acessório do cache, é o que faz o cache valer
para **todas** as consultas em vez de duas de quatro.

## A ordem

### Fase 1 — O formato em disco ✅

Um arquivo, por projeto, com registros de **tamanho fixo** e as cadeias de texto
numa área à parte. Consultar é saltar e ler, nunca desserializar o todo.

O esboço:

| área | conteúdo |
|---|---|
| cabeçalho | assinatura, versão do formato, raiz do projeto, contagens e deslocamentos |
| textos | todos os nomes distintos, concatenados |
| tabela de nomes | `(deslocamento, tamanho)` por nome, **ordenada** |
| arquivos | caminho, data de modificação e tamanho de cada fonte indexado |
| declarações | `(nome, espécie, faixa, profundidade, arquivo, tipo)`, tamanho fixo |
| ocorrências | `(nome, arquivo, faixa)`, tamanho fixo, **agrupadas por nome** |
| tipos declarados | `(nome, arquivo)` |

Duas decisões que carregam o resto:

- **a tabela de nomes é ordenada.** Isso dá busca por nome exato em tempo
  logarítmico, e é o que faz Ctrl+clique achar 9.314 ocorrências em 0,9 ms sem
  tocar o resto do arquivo.

  *Correção registrada na fase 3:* escreveu-se aqui que essa tabela já era a
  resposta da fase 3. Não era — ela indexa **ocorrências**, e a completação
  pergunta por **declarações**, que são outra área. A fase 3 precisou ordenar
  também a área de símbolos. A ideia estava certa, o alvo estava errado.
- **os arquivos guardam data e tamanho.** É com isso que a fase 4 decide o que
  releu e o que não precisou.

A forma que o índice já tem em memória — `IndexedSymbol`, `Occurrence`, arquivos
por número — está a um passo disto. Ela nasceu para economizar memória na fase 3
da `19`, e serve aqui sem mudança de modelo.

**Critério:** gravar e reler o índice devolve exatamente as mesmas respostas que
montá-lo do zero, para as quatro consultas. Um arquivo de versão diferente,
truncado ou corrompido é descartado e o índice é reconstruído — nunca lido pela
metade.

**Feita, e o número é o argumento inteiro desta especificação:**

| | |
|---|---|
| arquivo, no projeto de referência | **78 MB** |
| gravar | 1,05 s |
| **reler** | **262 ms** |
| reconstruir, que é o que ele substitui | 51 s com disco quente, 283 s frio |

Duzentas vezes.

**Gravar já está em uso; ler não.** A varredura em segundo plano grava o arquivo
ao terminar, e assim o formato se prova em uso real. Nada o lê ainda, e isso é
deliberado: ler exige antes saber o que mudou desde a gravação, e servir um
índice vencido é o defeito silencioso que a `19` combateu. O leitor existe, está
coberto por teste, e entra em uso na fase 4.

**O que a fase custou descobrir:**

- **os números das espécies de símbolo são escritos à mão.** Derivá-los da ordem
  do `enum` faria reordenar uma variante corromper, em silêncio, todo arquivo já
  gravado. Mexer nos números existentes obriga a mudar a versão do formato.
- **os genéricos não cabem em registro fixo.** `TypeDescriptor` tem uma lista de
  argumentos; ela virou uma área própria, alcançada por `(início, quantos)`,
  como as ocorrências.
- **gravar é num temporário e renomear.** Um desligamento no meio da escrita
  deixaria arquivo pela metade, e ler índice truncado é pior que não ter índice.
- **a ordem tem de ser estável.** As declarações saem de um `HashMap`, cuja ordem
  não é; sem ordená-las, dois arquivos gravados do mesmo índice sairiam
  diferentes, e nenhuma comparação futura valeria.

`what_goes_to_disk_answers_the_same_when_it_comes_back` compara as quatro
consultas antes e depois do disco — comparar as estruturas byte a byte diria
menos, porque o que precisa sobreviver ao arquivo é o que a IDE pergunta.
`a_file_that_does_not_serve_is_discarded` recusa assinatura errada, versão
futura e cinco pontos de corte, e confirma que o arquivo bom continua sendo
aceito — senão o teste passaria recusando tudo.

O módulo `index` virou diretório para receber o codec ao lado: `index/mod.rs`
continua dono da construção e da consulta, `index/file.rs` é o formato.

### Fase 2 — O mapeamento ✅ *(com uma ressalva)*

O arquivo é mapeado em memória, e não lido. O sistema operacional passa a fazer o
que um cache escrito à mão faria: mantém residente o que foi tocado e despeja o
frio sob pressão.

Duas razões para não escrever o cache:

- **não há política de despejo para acertar.** A do sistema operacional é melhor
  que a que escreveríamos, e não tem defeito nosso para depurar;
- **não há terceira cópia.** Um cache próprio criaria mais um lugar onde a mesma
  informação mora, e lugares que podem discordar são a origem do defeito
  silencioso que a `19` combateu.

Entra uma dependência para o mapeamento. É uma dependência de plataforma, do tipo
que o `02-architecture` admite atrás de uma porta: quem consulta o índice não sabe
se ele está mapeado, lido ou em memória.

**Critério:** abrir o projeto pela segunda vez não reconstrói o índice, e a
memória residente da IDE cresce com o uso em vez de nascer no tamanho do índice.

**Feita a primeira metade; a segunda esbarrou numa decisão de arquitetura.**

`memmap2::Mmap::map` é `unsafe`, e o workspace declara `unsafe_code = "forbid"`.
Isso não é detalhe de implementação — é a mesma decisão que deixou o Ctrl+C do
terminal sem funcionar, registrada na ADR-013. Não foi furada aqui, e o exame que
levou a mantê-la está na **ADR-023**, inclusive por que uma exceção pontual não
era possível: `forbid` não se desliga localmente.

O que se construiu no lugar: o arquivo é **lido para um vetor de bytes**, e as
consultas saem dos bytes, sem materializar estrutura nenhuma. Tudo abaixo do
carregamento trabalha sobre `&[u8]`, então **trocar leitura por mapeamento é uma
linha** no dia em que a decisão mudar.

O que se ganhou assim mesmo:

| | reconstruindo | do arquivo |
|---|---|---|
| abrir o projeto | **251 s** | **3,65 s** |
| memória | 178 MB | **103 MB** |

E o que as consultas custam, já respondendo do arquivo:

| | |
|---|---|
| carregar | 34 ms |
| conferir se ainda vale | 3,6 s |
| Ctrl+clique — 9.314 ocorrências de `CamelContext` | **1,2 ms** |
| percorrer as 339.664 declarações por prefixo | 12,6 ms |

O 1,2 ms é a tabela ordenada da fase 1 fazendo o que ela prometia: busca binária
pelo nome, e só as ocorrências dele são tocadas.

**O que ficou de fora, e é preciso dizer** — ver ADR-023: a **elasticidade**. Esses 103 MB são
memória nossa, retida enquanto a IDE viver; com mapeamento, o sistema operacional
recuperaria as páginas frias sob pressão. O ganho de 178 para 103 MB é real, mas
é redução, não empréstimo — que era o prêmio secundário anunciado no começo desta
especificação.

**Conferir custa 3,6 dos 3,65 segundos.** É uma varredura de diretório comparando
data e tamanho de cada fonte. Qualquer diferença — arquivo novo, apagado ou
alterado — reprova o arquivo **inteiro** e o índice é reconstruído. É grosso de
propósito: reconstruir é caro, servir resposta vencida é errado, e reindexar só a
diferença é exatamente o que a fase 4 traz.

**Duas origens convivem**, e é isso que permite a fase 4 existir sem outro
formato: o arquivo responde por quase tudo, a memória guarda o que foi reindexado
desde o carregamento, e um fonte refeito na memória **some** do arquivo. Sem esse
apagamento a IDE responderia as duas versões — o defeito silencioso de novo.
`what_the_memory_redid_hides_what_the_file_says` é quem guarda isso.

**Limite conhecido:** a conferência não cobre os jars. Uma dependência trocada sem
mexer em fonte nenhum passa despercebida, e o índice segue com as classes antigas.

### Fase 3 — A completação vira busca ✅

Hoje `completion` percorre **todas** as declarações a cada tecla, filtrando por
prefixo; `workspace_types` faz o mesmo filtrando por trecho. Com a tabela de nomes
ordenada, a primeira vira uma faixa: os nomes que começam com o prefixo são
contíguos.

`workspace_types` filtra por **trecho**, não por prefixo, e por isso não se
resolve com a mesma faixa. Ou ela passa a privilegiar o prefixo — que é o que a
ordenação atual do resultado já faz, e o que quem digita espera —, ou ganha uma
estrutura própria. **Decidir isso é parte da fase**, e não detalhe de
implementação. *(Resolvido sem escolher nenhuma das duas: ver abaixo.)*

**Critério:** digitar uma letra não toca o índice inteiro. Medido: número de
registros lidos por tecla, que hoje é 339.664.

Vale sozinha, mesmo que as fases 1 e 2 nunca existissem: varrer 340 mil registros
por tecla é caro em memória, em disco ou onde for.

**Feita.** A área de símbolos passou a ser gravada **ordenada pelo nome em
minúsculas**, e as declarações de um prefixo ficaram contíguas: acha-se a
primeira por busca binária e anda-se enquanto o prefixo valer. Nada endereça um
símbolo por posição, então reordenar não quebrou nada.

Minúsculas porque é como a busca por tipo compara. A faixa sensível a maiúsculas,
que a completação usa, é um **subconjunto** dela — filtrar de novo custa nada,
porque a faixa já é pequena.

**Medido no projeto de referência**, com as 339.664 declarações:

| digitado | a faixa lê | em | a varredura lia |
|---|---|---|---|
| `C` | 46.818 | 3,0 ms | 339.664 em 8,9 ms |
| `Ca` | 4.308 | 0,40 ms | 339.664 em 9,1 ms |
| `Cam` | 2.388 | 0,19 ms | 339.664 em 8,8 ms |
| `Camel` | 2.340 | 0,20 ms | 339.664 em 8,8 ms |
| `CamelContext` | 1.039 | 0,073 ms | 339.664 em 8,3 ms |

Da terceira letra em diante são cerca de **140 vezes menos registros** e
**45 vezes menos tempo**. A primeira letra é a pior, e ainda assim lê 14% do
índice em vez de 100%.

**A busca por tipo não perdeu nada, e essa era a decisão em aberto.** A
especificação deixava escolher entre privilegiar o prefixo — perdendo a busca por
trecho — ou construir outra estrutura. Nenhuma das duas foi preciso: a ordenação
**já** punha quem começa com o digitado antes de quem só contém, e o resultado é
truncado no limite. Então encher o limite pela faixa dá o **mesmo resultado** de
varrer tudo, e só quando a faixa não enche é que se percorre o índice atrás de
quem apenas contém — o que é uma busca pedida, não uma tecla digitada. Num
monorepo, três letras já passam do limite.

**O que a fase custou descobrir:** a promessa da fase 1 estava no alvo errado. A
tabela ordenada que ela criou indexa ocorrências, e serve a Ctrl+clique; a
completação pergunta por declarações, que vivem noutra área e não estavam
ordenadas. O formato foi para a versão 3.

`a_prefix_range_answers_the_same_without_walking_everything` guarda as duas
metades — se a faixa respondesse menos seria rápida e errada, se percorresse tudo
seria certa e não teria mudado nada. Foi verificado que ele **falha** com a faixa
deslocada em uma posição.

### Fase 4 — A invalidação ao gravar ✅

Gravar um arquivo já chama `reindex_file`, que refaz **aquele** arquivo. Com o
índice em disco, a mesma gravação precisa alcançar o disco também.

E na abertura: comparar data e tamanho de cada fonte com o que o índice gravado
registra, e reindexar **só a diferença**. Um projeto que não mudou abre sem
reindexar nada; um com dez arquivos alterados paga dez.

**Critério:** abrir um projeto inalterado não reindexa nada; alterar um arquivo
fora da IDE e abrir reindexa aquele arquivo e mais nenhum; gravar dentro da IDE
deixa o disco de acordo com a memória.

**Feita.** A conferência da fase 2 respondia sim ou não; agora ela devolve a
**lista** do que mudou — novo, alterado ou apagado. Cada um passa por
`reindex_file`, que já sabia tirar do carregado o que aquele arquivo dizia e pôr
no lugar o que ele diz agora. Depois o índice volta ao disco, senão a próxima
abertura recalcularia a mesma diferença e ela só cresceria.

**Medido no projeto de referência:**

| | |
|---|---|
| carregar o arquivo | 31 ms |
| calcular a diferença (26.211 fontes) | 3,68 s |
| reconciliar **um** fonte alterado | **3,5 ms** |
| regravar o índice | 3,35 s, fora do caminho de espera |

E o que isso muda em quem abre o projeto:

| | antes da fase 4 | agora |
|---|---|---|
| projeto inalterado | 3,7 s | 3,7 s |
| um fonte alterado fora da IDE | **41 s** — reconstruía tudo | **3,7 s** |

Os 3,5 ms são o número da fase: um fonte alterado custa um fonte.

**A regravação sai do caminho de quem espera.** Ela leva 3,35 s, e o índice já
está pronto antes dela. Publicar vem primeiro, avisar quem espera vem em seguida,
e a volta ao disco acontece depois — não é da conta de quem abriu o projeto.

**Gravar dentro da IDE** continua reindexando só o arquivo, como desde a fase 4
da `19`. O disco não é reescrito a cada gravação, e não precisa ser: a próxima
abertura vê exatamente aqueles arquivos na diferença e paga 3,5 ms por cada um.
Reescrever 78 MB por gravação custaria mais do que economiza.

**O defeito que a fase revelou, e que não era dela.** Um teste caiu:
`the_indexed_jdk_is_the_one_the_ide_points_at`. O arquivo do índice era
identificado só pela raiz do projeto, mas o **conteúdo** dele depende do JDK
escolhido — trocar de JDK e reabrir serviria as classes do JDK anterior. Resposta
errada em silêncio, de novo. O JDK entrou na identidade do arquivo: cada um
guarda o seu, e voltar ao anterior reaproveita o que ele já tinha. Foi verificado
que o teste **volta a falhar** sem isso.

Só apareceu agora porque, até a fase 2, ativar sempre reconstruía — não havia
arquivo velho para servir. É o tipo de defeito que persistência cria, e a
especificação já o nomeava entre os riscos: *lugares que podem discordar*.

`the_difference_sees_what_changed_and_only_that` cobre os três casos — novo,
alterado, apagado. `opening_rereads_what_changed_and_nothing_else` cobre as três
metades do critério, inclusive que depois da regravação o arquivo volta a
descrever o projeto e o que ele responde é o que a reconciliação produziu.

## Uma ressalva sobre a ordem — vencida

Esta era a ordem pedida, e ela tinha uma janela ruim entre a fase 2 e a fase 3.
A janela existiu e fechou: as duas fases foram feitas em seguida. Fica o
raciocínio, que vale para a próxima vez que uma ordem for escolhida.

Com o índice mapeado (fase 2) mas a completação ainda varrendo tudo (fase 3), a
primeira tecla digitada toca o arquivo inteiro e o traz todo para a memória
residente. O ganho de elasticidade desaparece justamente no uso normal — e volta
só quando a fase 3 chega.

Não é defeito, é uma janela: quem parar entre a 2 e a 3 fica com o ganho de tempo
de abertura e sem o de memória. **Trocar a 3 pela 2** fecha essa janela e faz a
fase 2 render desde o primeiro dia. Fica registrado para quem executar decidir;
as duas ordens chegam ao mesmo lugar.

## O que a `20` entregou, no total

| | antes | depois |
|---|---|---|
| abrir um projeto inalterado | 251 s | **3,7 s** |
| abrir com um fonte alterado | 251 s | **3,7 s** |
| memória do índice | 178 MB | **103 MB** |
| registros lidos por tecla | 339.664 | **1.039 a 4.308** |
| Ctrl+clique (9.314 ocorrências) | — | **0,9 ms** |

**O que ficou por fazer**, e está registrado onde importa:

- **a memória não é elástica** — ver ADR-023. Trocar leitura por mapeamento é uma
  linha, e depende de rever o `unsafe_code = "forbid"`;
- **os 3,68 s da diferença** são uma varredura de 26 mil arquivos comparando data
  e tamanho. É o que sobrou de caro na abertura. *(Escreveu-se aqui que um
  observador de sistema de arquivos o eliminaria. **Não elimina** — ele só vê o
  que acontece enquanto está rodando, e a abertura precisa descobrir o que mudou
  com a IDE fechada. O que barateia é paralelizar a varredura; ver a `21`.)*
- **os jars não entram na conferência.** Uma dependência trocada sem mexer em
  fonte nenhum passa despercebida.

## Riscos

- **Três lugares com a mesma informação.** Disco, mapeamento e o índice vivo do
  processo. É a origem clássica de "a IDE insiste que essa classe existe" — o
  defeito silencioso que a `19` chamou de o mais perigoso. A fase 4 é o que o
  impede, e por isso ela é fase e não detalhe.
- **Arquivo corrompido ou de outra versão.** Descartar e reconstruir é a única
  resposta segura. Ler pela metade é pior que não ler.
- **Dois processos no mesmo projeto.** Duas IDEs abertas na mesma raiz gravando o
  mesmo arquivo. O mínimo é detectar e um deles trabalhar só em memória.
- **O tempo não sai, muda de lugar.** A primeira abertura de um projeto continua
  custando os 283 s. O que some é a segunda em diante.

## O que não muda

- **a indexação continua em segundo plano** e a ativação continua voltando em
  menos de 250 ms — as fases 2 e 3 da `19` continuam valendo;
- **o índice continua sem teto**: nenhum limite silencioso;
- **quem consulta não muda.** As quatro consultas mantêm a assinatura; onde os
  dados moram é assunto do índice.

## De onde isto vem

O `08-storage-and-memory` já recomendava, entre as estruturas: interning de
cadeias, IDs numéricos, mapeamento de memória, compactação de índices e LRU — e
listava "armazenar caminhos repetidos como `String`" entre os anti-padrões. A
fase 3 da `19` executou a parte de memória disso, e caiu de 927 MB para 178 MB
tirando exatamente os caminhos repetidos. Esta especificação executa o resto.

## Verificação

Cada fase termina com `cargo test --workspace` e
`cargo clippy --workspace --all-targets -- -D warnings`.

E com **número medido**, como a `19` fez:

| fase | o que medir |
|---|---|
| 1 | tamanho do arquivo; as quatro consultas respondem igual |
| 2 | tempo da segunda abertura; memória residente ao longo do uso |
| 3 | registros lidos por tecla |
| 4 | tempo de abrir um projeto inalterado |

Sem medição, "ficou mais rápido" é opinião — e foi medição que produziu os 283 s
e os 178 MB que motivam esta especificação.
