# 22 — Git

## Situação

A IDE não sabe o que mudou no código. Quem edita aqui precisa sair para outro
programa — ou para o terminal integrado — para ver a diferença, escolher o que
entra no commit, trocar de branch ou desfazer uma alteração. O editor mostra o
arquivo; ninguém mostra o **trabalho**.

E o Git já entrou na especificação por outra porta. A `21` cita "trocar de
branch" como o exemplo de mudança externa que deixava o índice velho: um
`checkout` reescreve milhares de fontes de uma vez. O observador resolveu o lado
do índice. O que falta é o outro lado — a IDE participar da troca em vez de
apenas reagir a ela.

Há ainda um agravante que não existe em IDEs sem terminal: **a nossa tem um**. O
usuário vai rodar `git` lá dentro, e a IDE precisa não mentir sobre o estado do
repositório quando isso acontecer.

## A fronteira

A `12` fixou a regra, e ela decide esta especificação inteira:

> **crate** para uma fronteira de dependência, substituição, isolamento ou
> distribuição independente; **módulo** para responsabilidades que são
> compiladas, versionadas e alteradas como uma única unidade.

Git é **uma crate**, `ide-git`, e tudo dentro dela são módulos. Não há
`ide-git-api` mais `ide-git-cli`, e não há uma crate por capacidade.

O que separa a IDE da implementação do Git não é a fronteira de crate — é a
**privacidade de módulo**. `pub(crate)` já impede que qualquer coisa do interior
escape, e o compilador passa a garantir o que antes seria disciplina. Uma crate a
mais não acrescentaria encapsulamento nenhum aqui; acrescentaria arquivos.

### O que a IDE sabe, e o que ela não sabe

A IDE **sabe que existe Git**. Ela conhece branch, commit, stage, conflito — são
conceitos do domínio dela, aparecem nos menus e no vocabulário de quem usa.

A IDE **não sabe como o Git é falado**. Nunca vê processo, argumento de linha de
comando, `stderr`, formato de saída, `gix` ou `libgit2`. Chama trait, recebe tipo
de domínio, trata erro tipado.

Essa é a mesma linha que a `02` já desenha para Java: `Adapters` inclui "Java,
Git, Maven, depuração, filesystem", e a regra de dependência aponta para os
contratos. Git não é exceção — é o próximo caso da regra que já existe.

### As duas alternativas, e por que não

**Separar contrato de implementação em duas crates**, como a ERLibUi faz em
`ui-text-api`/`ui-text-cosmic`, resolve um problema que aqui não existe. Lá, o
contrato é consumido por muitas crates e nenhuma delas pode arrastar
`cosmic-text` junto. Aqui o consumidor é um — a aplicação — e a troca de backend
é interna: um `feature`, não uma dependência diferente no `Cargo.toml` de quem
consome.

**Esconder o Git atrás de um contrato genérico de versionamento**, para que a IDE
não saiba se é Git, Mercurial ou SVN, custa o modelo de domínio. *Index*,
*rebase*, *stash*, *cherry-pick* e *detached HEAD* não existem nos outros e não
sobrevivem inteiros à generalização — o que resta é um denominador comum que não
serve para desenhar tela nenhuma. É fragmentação prematura, exatamente o que a
`12` recusa.

E o caminho de volta é curto, o que é o que torna a decisão barata: no dia em que
um segundo sistema de versionamento entrar, os traits e os tipos de domínio
mudam de crate e `ide-git` passa a implementá-los. Nenhuma linha de lógica muda.
Ver a ADR-024.

## Estrutura

```text
ide-git/src/
├── lib.rs            superfície pública; é a única coisa que existe para fora
├── error.rs
├── model.rs          tipos do domínio compartilhados entre capacidades
├── repository.rs     ciclo de vida: descobrir, abrir, HEAD, configuração
├── working_tree.rs   estado dos arquivos e do índice
├── history.rs        commits e histórico
├── branches.rs       branches e referências
├── integration.rs    merge, rebase, cherry-pick e o estado entre eles
├── remotes.rs        sincronização com o remoto
├── tags.rs
├── events.rs         quando o repositório mudou, e quem avisa
├── adapters/         privado
│   └── cli/
└── infrastructure/   privado
```

Cada arquivo é uma **capacidade do domínio**, não um comando. Vira pasta quando
crescer, e não antes:

```text
integration/
├── mod.rs
├── service.rs
├── model.rs
├── conflict.rs
└── port.rs
```

Como o `lib.rs` reexporta a superfície pública, essa promoção não quebra
consumidor nenhum. É o que torna começar pequeno seguro.

### O `lib.rs` é o contrato

```rust
mod adapters;        // privado: nenhum tipo concreto é nomeável de fora
mod infrastructure;  // privado

pub mod branches;
pub mod error;
pub mod events;
pub mod history;
pub mod integration;
pub mod model;
pub mod remotes;
pub mod repository;
pub mod tags;
pub mod working_tree;

pub use branches::{BranchService, BranchSummary};
pub use error::{GitError, GitResult};
pub use events::{RepositoryEvent, RepositoryEventSink};
pub use model::{BranchName, CommitId, FileState, RepositoryStatus, StatusEntry};
pub use repository::{Repository, RepositoryService};
pub use working_tree::{FileDiff, Hunk, WorkingTreeService};

/// Abre um repositório a partir de um caminho qualquer dentro dele.
///
/// É o **único** ponto de construção. Quem chama nunca nomeia o adapter, e
/// trocá-lo não altera esta assinatura.
pub async fn open(path: &Path) -> GitResult<Repository> { /* ... */ }

/// Se este caminho está dentro de um repositório, e onde ele começa.
pub fn discover(path: &Path) -> Option<PathBuf> { /* ... */ }
```

`Repository` é um agregado que entrega os serviços, e não um objeto que faz
tudo — cada consumidor pede só a capacidade que usa:

```rust
impl Repository {
    pub fn working_tree(&self) -> Arc<dyn WorkingTreeService>;
    pub fn history(&self) -> Arc<dyn HistoryService>;
    pub fn branches(&self) -> Arc<dyn BranchService>;
    pub fn integration(&self) -> Arc<dyn IntegrationService>;
    pub fn remotes(&self) -> Arc<dyn RemoteService>;
    pub fn tags(&self) -> Arc<dyn TagService>;
}
```

Não existe `GitService` com tudo dentro. O painel de alterações depende de
`WorkingTreeService` e não recompila quando `rebase` muda.

### Por capacidade, e não por comando

Um comando do Git não é uma fronteira arquitetural. `status`, `diff`, `add` e
`restore` são quatro comandos e um assunto só — o estado da árvore de trabalho e
do índice —, e mudam pela mesma razão. `merge`, `rebase` e `cherry-pick` também:
os três produzem conflito, os três param no meio, os três precisam de continuar,
abortar e pular. O que os une é mais forte do que o que os separa.

| módulo | responsabilidade |
|---|---|
| `repository` | ciclo de vida do repositório, HEAD, configuração |
| `working_tree` | estado dos arquivos e do índice |
| `history` | commits, log, blame |
| `branches` | branches e referências |
| `integration` | integração de históricos e o estado entre commits |
| `remotes` | sincronização remota |
| `tags` | tags |

**O caso que prova a regra é o `checkout`.** É um comando só e são duas
capacidades: trocar o conteúdo de um arquivo é `working_tree::discard`; trocar de
branch é `branches::switch`. Quem organizasse por comando teria de escolher um
lugar para os dois, e o lugar errado arrastaria o outro atrás. É o mesmo motivo
pelo qual não existe um `git_service`.

## O modelo de execução

Esta é a parte que não se retrofita. `fn status() -> Result<Status>` e
`async fn status(&self, cancel) -> Result<Status>` são arquiteturas diferentes,
e descobrir isso depois reescreve todas as assinaturas.

A `03` já fixou a regra: *contratos assíncronos devem aceitar cancelamento*, e
*operações longas devem informar progresso*. Git é I/O, e I/O caro: `status` num
repositório grande custa centenas de milissegundos, `fetch` depende de rede e
pode não terminar nunca.

```rust
#[async_trait::async_trait]
pub trait WorkingTreeService: Send + Sync {
    async fn status(&self, cancel: &CancellationToken) -> GitResult<RepositoryStatus>;

    async fn diff(
        &self,
        path: &Path,
        side: DiffSide,
        cancel: &CancellationToken,
    ) -> GitResult<FileDiff>;

    async fn stage(&self, paths: &[PathBuf]) -> GitResult<()>;
    async fn unstage(&self, paths: &[PathBuf]) -> GitResult<()>;

    /// Destrutivo: joga fora alteração não commitada, sem rede de recuperação.
    async fn discard(&self, paths: &[PathBuf]) -> GitResult<()>;
}
```

Leitura recebe token; escrita não. Cancelar uma leitura é jogar fora uma resposta
que ninguém quer mais — trocar de arquivo antes do `diff` chegar. Cancelar uma
escrita pela metade deixaria o repositório num estado que ninguém pediu, e o
`stage` de trinta arquivos é rápido demais para valer o risco.

**O `CancellationToken` já existe, e já mudou de lugar ✅.** Ele nasceu em
`ide-language-api`, e não pertencia àquela crate: cancelar não é assunto de
linguagem, e `ide-git` não deve depender do host de linguagens para conseguir um
booleano compartilhado. Está em `ide-domain`, com `ide-language-api`
reexportando-o para quem já o importava de lá.

Escrever um segundo seria o defeito que a `21` nomeou noutro contexto: duas
definições da mesma coisa discordam um dia, e a discordância aparece como
comportamento estranho e não como erro de compilação.

A mudança custou o que se esperava de um tipo bem contido — **três linhas**
mencionavam o nome, todas no arquivo que o definia. Todo o resto do projeto o usa
pelo campo `context.cancellation`, sem nunca nomeá-lo, e por isso não houve o que
mudar em `ide-language-host`, em `language-java` ou na aplicação. `cargo test
--workspace` e `cargo clippy --workspace --all-targets -- -D warnings` passam.

### Nada disso roda na thread da interface

O caminho já existe e é o das ferramentas: linha de execução própria, runtime
`current_thread` do tokio dentro dela, e o resultado voltando por canal para ser
consumido no laço da IDE. Git entra por ele, sem inventar um segundo mecanismo.

### Uma escrita por vez, leituras à vontade

O Git protege o índice com `.git/index.lock`, e quem tentar escrever com o
arquivo presente falha. Não é hipótese remota aqui: **o terminal integrado é um
segundo escritor**, e o usuário vai usá-lo.

`ide-git` serializa as **escritas por repositório** e deixa as leituras livres.
Isso resolve a nossa concorrência interna e não resolve a do terminal — para essa
não há solução, só tratamento: `RepositoryLocked` é um erro previsto, com
mensagem que diz o que está acontecendo, e não uma falha genérica.

## Os erros são do domínio

Se o adapter deixar `stderr` chegar à interface como `String`, a regra de que a
IDE não conhece a implementação já quebrou — e quebrou de um jeito que compila,
passa nos testes e só aparece na tela do usuário.

```rust
#[derive(Debug, thiserror::Error)]
pub enum GitError {
    #[error("não é um repositório Git")]
    NotARepository,

    #[error("o repositório está em uso por outro processo")]
    RepositoryLocked,

    #[error("há uma operação em curso: {0:?}")]
    OperationInProgress(PendingOperation),

    #[error("há conflitos a resolver")]
    Conflicted { paths: Vec<PathBuf> },

    #[error("há alterações não commitadas")]
    DirtyWorkingTree { paths: Vec<PathBuf> },

    #[error("o remoto pediu autenticação")]
    AuthenticationRequired { remote: RemoteName },

    #[error("o remoto recusou a atualização")]
    RemoteRejected { reason: RejectionReason },

    #[error("referência não encontrada: {0}")]
    ReferenceNotFound(String),

    #[error("a operação foi cancelada")]
    Cancelled,

    /// O Git falhou de um jeito que não temos como classificar.
    ///
    /// O texto original fica aqui, para o registro e para o relatório de
    /// defeito. **Não** é o que a interface mostra como explicação.
    #[error("falha na ferramenta Git")]
    Backend { detail: String },
}
```

A diferença que essa lista compra é concreta: `DirtyWorkingTree` vira um diálogo
que oferece *stash* e *descartar*; `AuthenticationRequired` vira um pedido de
credencial; `RemoteRejected` com `NonFastForward` vira a oferta de puxar antes de
empurrar. Uma `String` viraria as três coisas sendo a mesma caixa de texto
vermelha.

Traduzir é trabalho do adapter, e é onde ele ganha o sustento. `Backend` é a
válvula honesta: o que ainda não foi classificado não vira classificação
inventada.

## Credenciais

`fetch` e `push` precisam de credencial, e credencial precisa de gente. É uma
inversão: o domínio **pede**, e quem responde é o anfitrião.

```rust
pub enum CredentialRequest {
    UsernamePassword { remote: RemoteName, url: String },
    Passphrase { key_path: PathBuf },
}

#[async_trait::async_trait]
pub trait CredentialProvider: Send + Sync {
    async fn request(&self, request: CredentialRequest) -> Option<Credential>;
}
```

Três regras, e as três existem por um motivo que já mordeu outros projetos:

- **`ide-git` não guarda credencial.** Ele pede, usa e esquece. Persistir é
  decisão de quem hospeda, com o cofre da plataforma, e não desta crate;
- **credencial nunca entra em registro nem em mensagem de erro.** Nem no
  `Backend { detail }` — a URL de um `push` autenticado pode carregar segredo
  dentro, e o texto do Git a repete;
- **o adapter de linha de comando roda com `GIT_TERMINAL_PROMPT=0`.** Sem isso, o
  `git` tenta perguntar num terminal que não existe e o processo **fica pendurado
  para sempre**, sem erro, sem saída, sem timeout. É a falha mais chata de
  diagnosticar de toda esta especificação, e ela se previne com uma variável de
  ambiente.

## Eventos: quando o repositório mudou

A `21` estabeleceu qual é a família de defeito mais perigosa: **a resposta velha
que se parece com a resposta certa**. Vale igual aqui. Um painel que mostra o
branch anterior ou uma lista de alterações de dois minutos atrás não avisa que
está errado — ele só está errado.

O repositório muda por três caminhos, e os três precisam chegar ao mesmo lugar:

1. **a IDE mudou.** Depois de todo comando de escrita, o próprio `ide-git`
   publica o evento. É o caminho fácil e o único que existe nas fases iniciais;
2. **alguém de fora mudou.** Terminal integrado, outro programa, um script de
   build. Chega pelo observador;
3. **os arquivos mudaram sem o repositório mudar.** Editar e gravar altera o
   `status` sem tocar em `.git`.

```rust
pub enum RepositoryEvent {
    /// HEAD apontou para outro lugar: troca de branch, commit, reset.
    HeadChanged { head: Head },
    /// O índice ou a árvore de trabalho mudaram.
    WorkingTreeChanged,
    /// Referências mudaram: fetch, criação ou remoção de branch, tags.
    RefsChanged,
    /// Começou ou terminou merge, rebase ou cherry-pick.
    OperationChanged { operation: Option<PendingOperation> },
}
```

### O observador de hoje ignora exatamente o que o Git precisa

`caminho_ignorado`, em `language-java/src/index/mod.rs`, pula `.git` — e está
certo, porque o índice de símbolos não tem nada a fazer lá dentro. Só que é
precisamente `.git/HEAD`, `.git/index` e `.git/refs/` que dizem que o
repositório mudou.

São **filtros de consumidores diferentes**, e a lição da `21` continua valendo
com um ajuste: o erro não é ter dois filtros, é ter dois filtros para a *mesma*
pergunta. O que precisa ser um só é o **observador**; o que cada consumidor
considera interessante é dele.

Isso torna explícito um débito que a `21` já tinha anotado: o observador mora
dentro de `language-java`, e a árvore do Explorer foi listada lá como segundo
consumidor sem dono. O Git é o terceiro. Três consumidores é quando o observador
deixa de ser detalhe do indexador Java e vira infraestrutura — a fase 4 trata
disso.

**O tempo de espera é o mesmo, e pelo mesmo motivo.** Um `checkout` reescreve
milhares de arquivos; reagir a cada evento seria calcular `status` mil vezes.
Acumula-se e reage-se à calmaria, com os ~300 ms que a `21` já mediu.

## Um `status` só, lido por todos

`status` é caro e é pedido o tempo todo: a barra de estado quer o branch, o
Explorer quer marcar os arquivos alterados, a margem do editor quer as linhas
modificadas, o painel quer a lista inteira. Quatro consumidores perguntando de
forma independente seriam quatro varreduras do repositório por tecla digitada.

O `RepositoryStatus` é calculado **uma vez**, guardado, e invalidado por evento.
Os consumidores leem o que está guardado; eles não chamam o Git. O cache mora no
domínio e não no adapter — trocar de backend não pode trocar a política de
atualização, senão a IDE muda de comportamento ao mudar de implementação.

## Como a IDE fala com o módulo

Pelo barramento de comandos, como tudo o mais. A ADR-019 já fixou que a IDE lê
ações e não texto de comando, e o painel de Git não é diferente do de build:

```rust
pub enum ApplicationCommand {
    // ...
    Git(GitRequest),
}

pub enum GitRequest {
    Refresh,
    Stage(Vec<PathBuf>),
    Unstage(Vec<PathBuf>),
    Discard(Vec<PathBuf>),
    Commit { message: String, amend: bool },
    SwitchBranch(BranchName),
    CreateBranch { name: BranchName, from: Option<CommitId> },
    Fetch,
    Pull,
    Push { force: bool },
    ContinueOperation,
    AbortOperation,
}
```

O painel emite `GitRequest`; ele não conhece `WorkingTreeService`. O resultado
volta como `IdeEvent`, e a tela se redesenha a partir do estado — não a partir do
retorno da chamada.

**Operação destrutiva pede confirmação, e a confirmação é da aplicação.**
`Discard`, `Push { force: true }` e reset perdem trabalho de forma irreversível.
`ide-git` executa o que lhe pedem sem perguntar — perguntar é da camada que tem
usuário na frente.

## O gerenciador

Até aqui esta especificação descreve o que a IDE **sabe** do Git. Esta seção
descreve o que ela **mostra**: uma tela só, com o trabalho inteiro dentro, em vez
de um pedaço em cada canto da janela.

Ela é desenho de tela, e por isso obedece à regra que já está em "o que não
muda": **a IDE não desenha e não arranja**. Tudo aqui é composição de componentes
da ERLibUi, e o que faltar de componente é pedido lá — não desenhado aqui.

### Onde ele começa: o terceiro botão da barra de atividades

A faixa estreita da esquerda tem dois botões, a lupa e o que recolhe o Explorer,
montados a cada quadro a partir do estado. O do Git é o terceiro, e não é
diferente deles em nada: um `Button::icon` com comando `activity.git`.

**O ícone vem da biblioteca.** `Icon` é um enum fechado na ERLibUi — `Play`,
`Stop`, `Bug`, `Search`, `Panels`, `ChevronUp`, `ChevronDown` —, e um ícone é
desenho. Entra lá um `Icon::Branch`, ao lado dos outros; não entra aqui uma
imagem que a aplicação carregue, porque isso seria a IDE decidindo traço.

### A janela

Clicar abre a janela do gerenciador. Ela é uma **camada sobreposta**: declarada
por último, cobrindo o conteúdo e recebendo o gesto antes dele — o mecanismo é o
da `09` da ERLibUi, o mesmo que o diálogo de gerar código e a inspeção do
depurador já usam pelo `ModalHost`.

Começar pelo `ModalHost` é o barato: ele já traz a camada, o véu, o painel
centrado e o `Esc` que fecha. **E há uma pergunta que pode derrubá-lo**, que fica
registrada agora em vez de aparecer como surpresa na fase 1: a diferença de um
arquivo abre **no editor**, e o editor está atrás do véu. Ou o gerenciador dá
lugar quando a diferença abre, ou ele deixa de ser modal. A escolha se faz quando
a fase 1 existir e alguém tiver as duas coisas na frente; o conteúdo da janela não
muda por causa dela, e é isso que torna adiar barato.

Dentro, a janela é uma divisão horizontal: navegação à esquerda, trabalho à
direita, com a divisa arrastável — `SplitPane`, como a divisão do editor da `28`.

### O painel da esquerda: quatro nós

Quatro nós no mesmo nível, e o que cada um abre:

| nó | o que lista |
|---|---|
| `branches` | as branches locais |
| `tags` | as tags |
| `remotes` | as branches remotas, por remoto |
| `stashes` | os stashes guardados |

**Isto é uma árvore, e não uma lista.** O pedido dizia lista composta, e a
diferença não é de nome: `ComposedList` não tem nó, não tem filho e não tem
expansão. O que está descrito — quatro raízes que abrem e fecham, com itens
dentro — é a `ComposedTreeView`, e o motivo de a distinção importar está no
`set_roots` dela: expansão e seleção são preservadas **por identidade de nó**.
Um `fetch` refaz a lista de referências; sem isso, ele fecharia as branches que
estavam abertas na tela toda vez que chegasse.

Composta, e não a `TreeView` simples, porque a linha carrega mais que um rótulo:
a branch atual leva marca, uma branch local mostra quantos commits está à frente
e atrás do upstream, e as ações de cada linha são células. Quem monta as células
é a IDE — é o padrão da árvore do Explorer, e é o mesmo aqui.

**A hierarquia dentro de cada nó é rasa na primeira versão.** `feature/x` e
`feature/y` aparecem como dois itens, e não como uma pasta `feature` com dois
filhos. Agrupar por `/` é conveniência, e conveniência se acrescenta depois sem
mudar nada do que está aqui.

### A busca das branches

Acima da árvore, uma caixa de busca **no mesmo padrão da do editor** — a mesma
função de desenho, com a folga e a largura deste lugar.

**Ela é a terceira caixa, e é independente das outras duas.** Isso não é detalhe
de conforto: a busca do arquivo e a da saída do terminal nasceram dividindo um
estado só, com o alvo escolhido pelo foco de quem apertou `Ctrl+F`, e o resultado
foi uma impedindo a outra de abrir. Custou uma correção inteira. A do gerenciador
nasce com o texto dela, o par de nós dela e o foco dela; nenhuma das três olha
para o estado das outras.

Ela **filtra o que já está carregado** e não pergunta nada ao Git. Texto vazio
mostra tudo; texto não vazio esconde o que não casa, e esconde o nó que ficou sem
filho — um `tags` aberto e vazio depois de digitar diz que não há tag nenhuma, o
que é mentira.

### O lado direito: duas abas

`Tabs` da biblioteca, com `status` e `history`.

#### A aba `history`: uma tabela

Cinco colunas: **Nó**, **Description**, **Date**, **Author**, **Hash**.

A coluna `Nó` é o grafo — o ponto do commit e os traços que o ligam aos pais. Ela
divide a mesma regra do ícone: **a IDE calcula, a biblioteca desenha**. Qual
faixa cada commit ocupa, e de onde para onde vai cada traço, sai do histórico, e
isso é conta e não desenho; o traço na tela é da ERLibUi, como célula que a IDE
põe na coluna. Sem essa divisão, o grafo seria a primeira coisa que a IDE
desenharia por conta própria.

As outras quatro são texto. `Hash` aparece abreviada, e o que se copia é a
inteira — quem copia hash vai colar num comando.

**A tabela é virtualizada e o histórico vem por páginas.** Um repositório de
verdade tem dezenas de milhares de commits, e carregar o `log` inteiro para
mostrar quarenta linhas é o oposto do que a `19` e a `20` fizeram no índice.

**Critério:** o histórico de um repositório grande abre no mesmo tempo que o de
um pequeno, e rolar até o fim não trava.

#### A aba `status`: três painéis empilhados

Empilhados na vertical, com as divisas arrastáveis — dois `SplitPane` verticais,
que é como três painéis se empilham com o que a biblioteca tem.

Os três, e o que separa um do outro:

1. **preparados** — o que está no índice e entra no próximo commit;
2. **alterados** — o que mudou na árvore de trabalho e não está preparado;
3. **não rastreados** — o que o Git ainda não conhece.

*A divisão dos três é a do `RepositoryStatus`, e não uma invenção da tela*: é
exatamente a distinção que `--porcelain=v2` devolve, e é a que decide o que
`stage` e `discard` fazem em cada linha. Se ela estiver errada, está errada antes
da tela.

Cada painel é uma **lista composta**: a linha tem o estado, o caminho e as ações
daquele lugar — preparar, despreparar, descartar —, e ação em linha é célula.

Clicar num arquivo mostra a diferença dele. Onde, é a pergunta da janela que
ficou registrada acima.

### O que o gerenciador não faz

Ele não fala com o Git. Emite `GitRequest` pelo barramento, como o painel de
build, e lê o `RepositoryStatus` que já está guardado — a mesma leitura que a
barra de estado e o Explorer fazem. Quatro telas perguntando por conta própria
seriam quatro varreduras do repositório, e é o que a seção "um `status` só" já
recusou.

E ele não confirma nada por si: `Discard` e `Push { force: true }` perdem
trabalho, e a confirmação é da aplicação — o gerenciador é aplicação, e por isso é
ele quem pergunta.

### O que a ERLibUi precisa ganhar

Três coisas, e nenhuma delas é lógica de Git:

- **`Icon::Branch`**, no enum de ícones;
- **`ComposedTable`**, que não existe. A lista composta e a árvore composta já
  existem, e nenhuma das duas é tabela: coluna é acordo **entre linhas**, e as
  duas resolvem largura dentro de cada linha. Sem cabeçalho e sem largura de
  tabela, o `Date` de uma linha não fica embaixo do `Date` da outra. A `09` da
  ERLibUi descreve o componente;
- **a célula do grafo**, que desenha ponto e traço a partir das faixas que a IDE
  calcula.

O padrão dos três é o dos componentes que já existem: a biblioteca posiciona,
desenha e encaminha o gesto; **quem decide o que vai dentro de cada célula é a
IDE**.

### Em que fase cada pedaço entra

| pedaço | fase | por quê |
|---|---|---|
| botão, janela, divisão, nó `branches` | 0 | é leitura pura, e prova a tela com o menor código atrás |
| aba `status`, os três painéis, as ações de linha | 1 | é o `working_tree` inteiro, que é o que a fase 1 entrega |
| aba `history`, a tabela, o grafo | 2 | precisa do `history` e do `ComposedTable` |
| nós `tags` e `stashes` | 3 | aparecem antes, vazios; a capacidade chega aqui |
| nó `remotes`, à frente e atrás | 4 | é `fetch`, e sem ele não há o que contar |

**Os nós aparecem desde o começo, mesmo sem ter o que mostrar.** Um nó que só
existe depois que a capacidade chega faz a tela mudar de forma a cada fase; um nó
vazio diz o que a IDE ainda não sabe fazer, e é honesto.

**Isto muda uma decisão anterior desta especificação.** O `stash` estava listado
em "o que fica de fora" como candidato à versão seguinte; com um nó dele na tela,
ele passa a ser fase 3. A lista lá embaixo foi corrigida.

## Fases

### Fase 0 — A crate existe, e ela responde `status`

Crate, `model`, `error`, os traits de `repository` e `working_tree`, o adapter de
linha de comando, e o suficiente para a barra de estado mostrar o branch e a
contagem de alterações. Nada de escrita.

É pouco de propósito: é a fase que prova que a fronteira se sustenta, com o menor
código possível atrás dela.

**Critério:** abrir um projeto versionado mostra o branch correto e o número de
arquivos alterados. Abrir um não versionado não mostra nada e não falha. E o
tempo do primeiro `status` está medido, no projeto de referência de 26 mil
arquivos.

### Fase 1 — Ver e escolher o que muda

`working_tree` inteiro: `diff`, `stage`, `unstage`, `discard`. O painel de
alterações, a diferença lado a lado reusando o editor, e a marcação na margem.

A granularidade é **por arquivo**. Preparar por hunk ou por linha é o que a
`integration` fará com conflito mais tarde, e não vale segurar esta fase.

**Critério:** dá para ver o que mudou num arquivo, preparar parte dos arquivos e
descartar outro, sem sair da IDE e sem que a lista fique velha depois de cada
ação.

### Fase 2 — Commitar

`history`: `commit`, `amend`, `log`. A caixa de mensagem, e o histórico como
lista.

**Critério:** um ciclo completo de trabalho — editar, ver, preparar, commitar —
acontece dentro da IDE.

### Fase 3 — Branches e integração

`branches` e `integration` juntos, porque separá-los daria uma fase que só cria
branch e não sabe fundir. Trocar, criar, fundir; e o estado intermediário: lista
de conflitos, continuar, abortar.

Resolver conflito **no editor** — os três blocos, escolher um lado — é o item
mais caro desta especificação. Se pesar, a fase entrega a *detecção* do conflito
e a lista, e a resolução acontece como edição de texto normal, que é como o
conflito já está gravado no arquivo.

**Critério:** trocar de branch pela IDE atualiza o editor, o Explorer e o índice
de símbolos. Um merge com conflito mostra quais arquivos, e a IDE não fica presa
num estado do qual não se sai.

### Fase 4 — O observador vira infraestrutura, e o remoto entra

Duas coisas na mesma fase porque a segunda depende da primeira: `fetch` muda
referências sem tocar em arquivo nenhum, e sem observar `.git` a IDE não teria
como saber.

O observador sai de `language-java` para um lugar próprio, com consumidores
registrados: o índice Java, o Git e — se couber — a árvore do Explorer, que a
`21` deixou anotada. Cada um com o seu filtro; o registro no sistema operacional
é um só.

Depois disso, `remotes`: `fetch`, `pull`, `push`, credenciais, e a contagem de
commits à frente e atrás.

**Critério:** rodar `git checkout` no terminal integrado atualiza a IDE inteira
sem ação do usuário. E um `push` que precisa de senha pergunta, em vez de ficar
pendurado.

## O que fica de fora, e por quê

- ~~**`stash`**~~ **entrou.** Ele era o candidato natural à versão seguinte, e o
  gerenciador lhe deu um nó na tela: ficar de fora significaria um nó que não
  abre. É fase 3, ao lado do diálogo de "há alterações não commitadas" que já o
  oferecia;
- **o gerenciador como painel encaixado.** Ele nasce como janela sobreposta.
  Virar painel do arranjo — ao lado do Explorer, ou embaixo com o terminal — é
  mudança de onde ele mora, e não do que ele tem dentro;
- **preparar por hunk ou por linha.** É o que separa uma integração boa de uma
  suficiente, e é caro. Fica anotado como o primeiro item depois das fases;
- **submódulos, worktrees e LFS.** Cada um é um modelo próprio de repositório
  dentro do repositório, e nenhum é necessário para o ciclo de trabalho básico;
- **rebase interativo, `bisect`, `reflog`.** Ferramentas de quem já sabe o que
  quer, e que o terminal integrado já atende;
- **assinatura GPG.** Ela funciona sozinha se o `git` do usuário estiver
  configurado, desde que o adapter não atrapalhe — e não atrapalhar é o único
  requisito desta versão;
- **pull requests, issues, revisão.** Isso é GitHub ou GitLab, não é Git. É uma
  integração com serviço de rede, com autenticação própria e ciclo de vida
  próprio, e não pertence a esta crate.

## Riscos

- **Ler saída feita para gente.** `git status` sem argumento muda de formato
  entre versões e traduz para o idioma do sistema. O adapter usa
  `--porcelain=v2 -z` com `LC_ALL=C`, e nunca o formato humano. O `-z` também
  resolve nome de arquivo com acento e com espaço, que o Git citaria e escaparia
  no formato normal — e nomes em português os têm;
- **Caminho que não é UTF-8.** O Git entrega bytes; `PathBuf` no Windows não é
  UTF-8 e no Linux nem precisa ser. Converter com perda faria um arquivo alterado
  parecer um arquivo desconhecido. É a mesma armadilha de maiúsculas e ligações
  simbólicas que a `21` registrou;
- **Fim de linha.** `core.autocrlf` no Windows faz arquivos aparecerem como
  alterados sem ninguém ter mudado nada. Não é defeito nosso e vai ser relatado
  como defeito nosso;
- **Disputa pelo `index.lock` com o terminal integrado.** Tratada, não
  eliminada;
- **Repositório grande.** `status` cresce com o número de arquivos rastreados. Se
  incomodar, o recuo é o que o próprio Git oferece — `core.fsmonitor` e
  `untracked-cache` —, e a decisão fica para quando houver medição;
- **`git` ausente do `PATH`.** Enquanto o adapter for o de linha de comando, isso
  é uma dependência externa como o JDK. Degrada: a IDE abre e trabalha, sem
  painel de Git;
- **Operação destrutiva sem confirmação.** O erro custa trabalho do usuário e não
  tem desfazer. É risco de interface, e a interface é quem o carrega.

## O que não muda

- **o domínio nunca vê a implementação.** Nenhum tipo de `adapters` atravessa o
  `lib.rs`, e nenhum `stderr` chega à tela como explicação;
- **a varredura da abertura continua sendo a rede.** Perder evento do observador
  leva ao comportamento de hoje, e não a um estado inventado — vale para o Git
  como já valia para o índice;
- **o observador é um só.** Ele muda de lugar na fase 4; ele não vira dois;
- **a IDE não desenha e não arranja.** O painel de Git é composição de
  componentes da ERLibUi, como todo o resto (ADR-020 e ADR-022).

## Verificação

Cada fase termina com `cargo test --workspace` e
`cargo clippy --workspace --all-targets -- -D warnings`.

Três coisas específicas desta especificação:

**Os testes rodam contra repositório de verdade**, criado numa pasta temporária
por `git init`, com commits feitos pelo próprio teste. Repositório falso testaria
o nosso `mock`, e é justamente a tradução da saída real do Git que precisa ser
verificada.

**Os testes do domínio não nomeiam o adapter.** Eles são escritos contra os
traits, e é isso que os torna a prova da regra 8: no dia em que um segundo
adapter existir, a mesma bateria roda contra ele sem uma linha mudada. Enquanto
houver um só, eles ainda valem — como guarda contra o modelo de domínio ser
desenhado a partir do formato de saída da linha de comando, que é como esse tipo
de abstração costuma vazar.

**Uma guarda de arquitetura**, no formato que o projeto já usa: nenhuma crate
fora de `ide-git` menciona `Command::new("git")`, `git2` ou `gix`. É um teste, e
ele falha no dia em que alguém tomar o atalho.

E número medido, como a `19`, a `20` e a `21` fizeram:

| fase | o que medir |
|---|---|
| 0 | tempo do `status` no projeto de referência, repositório limpo e sujo |
| 1 | tempo do `diff` de um arquivo; tempo entre gravar e a margem mudar |
| 3 | tempo do `checkout` de branch até editor, Explorer e índice em dia |
| 2 | tempo até a primeira página do histórico aparecer, no de referência |
| 4 | atraso entre `git` no terminal integrado e a IDE refletir |
