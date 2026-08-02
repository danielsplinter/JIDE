# 08 — Persistência, Cache e Memória

## Objetivo

Controlar memória explicitamente e evitar crescimento ilimitado.

## Categorias

### Estado essencial em memória

- documentos abertos;
- seleções;
- árvore sintática atual;
- símbolos usados no contexto;
- estado da UI.

### Estado persistível

- índices;
- metadados de dependências;
- cache de class files;
- histórico de projetos;
- resultados de análise estáveis.

### Estado descartável

- autocomplete anterior;
- diagnósticos obsoletos;
- árvores de arquivos fechados;
- resultados de busca antigos;
- previews.

### Limites do índice Java inicial

- no máximo 600 arquivos candidatos são visitados por ativação;
- no máximo 500 fontes Java são analisadas;
- no máximo 64 JARs são abertos;
- no máximo 20.000 class files são indexados por JAR;
- uma entrada `.class` maior que 16 MiB é ignorada;
- diretórios `.git`, `target`, `node_modules` e `.gradle` não são percorridos.

### Pendência: a árvore do Explorer não tem teto

A varredura que monta a árvore de arquivos — `ide-workspace`, `tree::scan` —
percorre o projeto **inteiro**, recursivamente e de uma vez, ao abrir, e guarda um
nó por arquivo em memória. Ela não tem nenhum dos limites acima, e roda de forma
**síncrona no laço da janela**: enquanto não termina, a IDE não desenha.

Medido em `release`, sobre um diretório com 56.339 arquivos e cache de disco
quente: **2,17 s**. O tempo acompanha a quantidade de arquivos, não o tamanho
deles — um projeto Java grande costuma pesar em JARs e saídas de build, e não em
código.

Dois agravantes, os dois pequenos de corrigir:

- **A lista de diretórios ignorados diverge da do índice.** Aqui só `.git` e
  `target` são pulados; o índice também pula `node_modules` e `.gradle`. Num
  projeto Gradle, o Explorer desce por `.gradle/` e `build/` inteiros — justamente
  onde estão os milhares de arquivos que ninguém quer ver na árvore. Alinhar as
  duas listas é a correção de maior efeito pelo menor risco.
- **Nada é preguiçoso.** A árvore é materializada inteira mesmo com todos os nós
  fechados. Varrer um nível por vez, ao expandir, tiraria a espera e a memória —
  e é como o painel já se comporta visualmente.

Enquanto isso não mudar, abrir um projeto muito grande custa alguns segundos de
janela parada e memória proporcional ao número de arquivos. Digitar não é
afetado: o custo por tecla depende dos documentos abertos, não do tamanho do
projeto.

## Configuração do usuário

A IDE mantém um arquivo de configuração por usuário, fora de qualquer projeto:

```text
Windows   %APPDATA%\er-ide\config.toml
Linux     $XDG_CONFIG_HOME/er-ide/config.toml, ou ~/.config/er-ide/config.toml
macOS     ~/.config/er-ide/config.toml
```

A variável `ER_IDE_CONFIG` aponta diretamente para outro arquivo e tem
prioridade sobre o local padrão.

```toml
event_capacity = 1024

[workspace]
last_path = "C:/Users/exemplo/projetos/minha-app"
open_documents = [
    "C:/Users/exemplo/projetos/minha-app/src/main/java/App.java",
    "C:/Users/exemplo/projetos/minha-app/pom.xml",
]
active_document = "C:/Users/exemplo/projetos/minha-app/pom.xml"

[run]
# Opcional: como subir a aplicação. Vazio significa deduzir do projeto.
# `{agent}` recebe o agente de depuração quando a execução é com depuração e
# desaparece quando é sem; `{host}` e `{port}` seguem a mesma regra.
command = "./gradlew bootRun \"-Dorg.gradle.jvmargs={agent}\""

[debug]
host = "127.0.0.1"
port = 8000
```

### Último projeto

Abrir um projeto por `Arquivo → Projeto...` grava o caminho em
`workspace.last_path`. Na inicialização seguinte, esse projeto é reaberto
automaticamente, com árvore, terminais, toolchain e importação do build system
apontando para ele.

Regras:

- o último projeto tem prioridade sobre o diretório em que a IDE foi executada;
- um caminho que não existe mais — pasta renomeada, removida ou em um disco
  desconectado — é ignorado, e a IDE abre o diretório atual sem falhar;
- o registro é preservado mesmo quando o caminho está indisponível, para que a
  reabertura volte a funcionar quando o disco for reconectado;
- configuração ilegível não impede a inicialização: vale o padrão, e o motivo é
  registrado no log;
- falha ao gravar não interrompe o trabalho; o projeto continua aberto e o
  usuário é avisado na barra de status;
- a IDE nunca grava configuração do usuário dentro do projeto.

### Abas abertas

As abas abertas são registradas em `workspace.open_documents`, na ordem em que
aparecem, e a que está em foco em `workspace.active_document`. Na inicialização
seguinte elas voltam com o projeto, já com o conteúdo carregado: quem reabre a
IDE espera continuar de onde parou, e não repetir a navegação pelo Explorer.

Regras:

- as abas pertencem ao projeto em que foram abertas. Abrir outro projeto
  descarta o registro do anterior, para que voltar ao primeiro não traga
  arquivos que não são dele;
- um arquivo apagado ou renomeado no meio-tempo é ignorado em silêncio, pela
  mesma razão que um projeto inexistente não impede a IDE de abrir;
- documentos criados em memória, sem arquivo por trás, não são registrados:
  seriam abas impossíveis de reabrir;
- o registro acompanha **qualquer** mudança do conjunto — abrir, fechar ou trocar
  de aba. Reabrir uma aba que o usuário fechou é tão errado quanto perder uma que
  ele deixou aberta;
- a comparação é feita a cada quadro, e não sinalizada em cada ponto que abre ou
  fecha uma aba: comparar alguns caminhos é barato, e assim nenhum caminho novo
  pode esquecer de avisar;
- falha ao gravar não interrompe o trabalho, como no registro do projeto.

### Execução e depuração

`run.command` descreve como a aplicação do projeto sobe. É opcional e tem
prioridade sobre qualquer dedução feita a partir do projeto importado. O mesmo
comando serve às duas execuções: com depuração, `{agent}` recebe o argumento do
agente; sem depuração, o marcador simplesmente desaparece.

`debug.host` e `debug.port` guardam o último alvo usado, para que o botão de
depurar funcione com um clique nas execuções seguintes.

## Orçamento

```rust
pub struct MemoryBudget {
    pub syntax_bytes: usize,
    pub semantic_bytes: usize,
    pub index_cache_bytes: usize,
    pub ui_cache_bytes: usize,
    pub plugin_bytes: usize,
    /// Teto somado dos analisadores que rodam fora do processo.
    ///
    /// Não é memória nossa e é problema nosso. Ver "São dois números".
    pub external_analyzer_bytes: usize,
}
```

### São dois números, e ignorar o segundo é mentir para si mesmo

Um analisador externo — o de TypeScript da `23` é o primeiro — roda em processo
próprio, com espaço de endereçamento próprio. O que ele aloca não entra no nosso
heap, não fragmenta a nossa memória, e um estouro lá não derruba a IDE.

Contabilmente separado; fisicamente, a mesma RAM. Se a soma passar do que a
máquina tem, quem parece lento é a IDE, porque é ela que está na frente de quem
usa. **"Não é o meu processo" é verdade contábil que não significa nada para
quem está com a máquina paginando.**

No Windows há um detalhe que fecha o argumento: o Gerenciador de Tarefas agrupa
processos filhos sob o pai e **soma no total exibido**. O analisador aparece como
memória da IDE para quem olha, esteja ou não no nosso heap.

Por isso:

- o orçamento tem **dois números** — o do processo da IDE e o da máquina, somando
  os analisadores. Um orçamento que ignora um processo de centenas de megabytes
  que nós mesmos criamos não é orçamento, é recorte conveniente;
- a **política de degradação dispara pelo total**. Passou do teto, o analisador
  externo cai e o provider nativo assume — o objetivo é a máquina continuar
  utilizável, e não a nossa linha do Gerenciador de Tarefas continuar bonita;
- o teto do processo externo é **imposto por nós**, e não sofrido: um runtime que
  aceite limite de heap o recebe na linha de comando. É literalmente o que esta
  seção chama de orçamento explícito, aplicado a um processo em vez de a uma
  estrutura.

Em compensação, essa memória tem uma propriedade que a nossa não tem: **ela é
recuperável**. Matar e reabrir o analisador devolve tudo, e o provider nativo
responde enquanto ele não volta. A ADR-023 registra que o índice em disco ficou
"reduzido, não emprestado" — os 103 MB dele ficam retidos enquanto a IDE viver.
A elasticidade que não se conseguiu lá aparece aqui de graça, por uma decisão
tomada para outro motivo.

## Política de cache

```rust
pub trait CachePolicy: Send + Sync {
    fn should_retain(&self, entry: &CacheEntry) -> bool;
    fn eviction_priority(&self, entry: &CacheEntry) -> u64;
}
```

## Estruturas recomendadas

- interning de strings;
- arenas;
- IDs numéricos;
- árvores imutáveis;
- snapshots compartilhados;
- LRU;
- memory mapping;
- compactação de índices;
- paginação de resultados.

## Anti-padrões

Evitar:

- armazenar caminhos repetidos como `String`;
- duplicar AST e CST sem necessidade;
- guardar todos os arquivos parseados;
- cache sem limite;
- cópias profundas;
- ciclos de `Arc`;
- eventos contendo documentos inteiros;
- carregar todos os plugins na inicialização.

## Métricas

A IDE deve exibir:

```text
Uso total
Uso por provider
Uso por workspace
Uso dos índices
Uso dos plugins
Processos externos
Caches descartáveis
```

## Limites

Um provider que exceder o orçamento poderá:

1. receber solicitação de limpeza;
2. suspender caches;
3. descarregar arquivos inativos;
4. reiniciar em processo isolado.
