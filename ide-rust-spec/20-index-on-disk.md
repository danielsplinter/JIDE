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

### Fase 1 — O formato em disco

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
  logarítmico **e** faixa por prefixo — que é exatamente o que a fase 3 precisa.
  O formato da fase 1 já contém a resposta da fase 3; não é coincidência, é o
  motivo de a fase 1 vir primeiro.
- **os arquivos guardam data e tamanho.** É com isso que a fase 4 decide o que
  releu e o que não precisou.

A forma que o índice já tem em memória — `IndexedSymbol`, `Occurrence`, arquivos
por número — está a um passo disto. Ela nasceu para economizar memória na fase 3
da `19`, e serve aqui sem mudança de modelo.

**Critério:** gravar e reler o índice devolve exatamente as mesmas respostas que
montá-lo do zero, para as quatro consultas. Um arquivo de versão diferente,
truncado ou corrompido é descartado e o índice é reconstruído — nunca lido pela
metade.

### Fase 2 — O mapeamento

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

### Fase 3 — A completação vira busca

Hoje `completion` percorre **todas** as declarações a cada tecla, filtrando por
prefixo; `workspace_types` faz o mesmo filtrando por trecho. Com a tabela de nomes
ordenada, a primeira vira uma faixa: os nomes que começam com o prefixo são
contíguos.

`workspace_types` filtra por **trecho**, não por prefixo, e por isso não se
resolve com a mesma faixa. Ou ela passa a privilegiar o prefixo — que é o que a
ordenação atual do resultado já faz, e o que quem digita espera —, ou ganha uma
estrutura própria. **Decidir isso é parte da fase**, e não detalhe de
implementação.

**Critério:** digitar uma letra não toca o índice inteiro. Medido: número de
registros lidos por tecla, que hoje é 339.664.

Vale sozinha, mesmo que as fases 1 e 2 nunca existissem: varrer 340 mil registros
por tecla é caro em memória, em disco ou onde for.

### Fase 4 — A invalidação ao gravar

Gravar um arquivo já chama `reindex_file`, que refaz **aquele** arquivo. Com o
índice em disco, a mesma gravação precisa alcançar o disco também.

E na abertura: comparar data e tamanho de cada fonte com o que o índice gravado
registra, e reindexar **só a diferença**. Um projeto que não mudou abre sem
reindexar nada; um com dez arquivos alterados paga dez.

**Critério:** abrir um projeto inalterado não reindexa nada; alterar um arquivo
fora da IDE e abrir reindexa aquele arquivo e mais nenhum; gravar dentro da IDE
deixa o disco de acordo com a memória.

## Uma ressalva sobre a ordem

Esta é a ordem pedida, e ela tem uma janela ruim entre a fase 2 e a fase 3.

Com o índice mapeado (fase 2) mas a completação ainda varrendo tudo (fase 3), a
primeira tecla digitada toca o arquivo inteiro e o traz todo para a memória
residente. O ganho de elasticidade desaparece justamente no uso normal — e volta
só quando a fase 3 chega.

Não é defeito, é uma janela: quem parar entre a 2 e a 3 fica com o ganho de tempo
de abertura e sem o de memória. **Trocar a 3 pela 2** fecha essa janela e faz a
fase 2 render desde o primeiro dia. Fica registrado para quem executar decidir;
as duas ordens chegam ao mesmo lugar.

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
