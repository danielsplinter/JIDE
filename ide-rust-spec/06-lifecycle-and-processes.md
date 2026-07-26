# 06 — Ciclo de Vida e Processos

## Objetivo

Evitar que todos os componentes permaneçam carregados durante toda a sessão.

## Serviços

Cada serviço deverá possuir ciclo de vida explícito:

```rust
#[async_trait::async_trait]
pub trait ManagedService: Send + Sync {
    async fn start(&self) -> Result<(), ServiceError>;
    async fn suspend(&self) -> Result<(), ServiceError>;
    async fn resume(&self) -> Result<(), ServiceError>;
    async fn stop(&self) -> Result<(), ServiceError>;
    fn health(&self) -> ServiceHealth;
}
```

## Políticas de ativação

Exemplos:

- ativar Java ao abrir projeto Java;
- ativar parser ao abrir arquivo;
- ativar semântica após parsing;
- ativar debugger somente ao iniciar depuração;
- ativar Maven somente ao importar ou executar build;
- suspender indexação após ociosidade;
- descarregar provider quando nenhum workspace o utiliza.

## Process Supervisor

```rust
#[async_trait::async_trait]
pub trait ProcessSupervisor: Send + Sync {
    async fn spawn(
        &self,
        request: ProcessRequest,
    ) -> Result<ProcessHandle, ProcessError>;

    async fn terminate(
        &self,
        process_id: ProcessId,
    ) -> Result<(), ProcessError>;

    async fn status(
        &self,
        process_id: ProcessId,
    ) -> Result<ProcessStatus, ProcessError>;

    async fn execute(
        &self,
        request: ProcessRequest,
    ) -> Result<ProcessOutput, ProcessError>;
}
```

`ProcessRequest` inclui programa, argumentos, diretório de trabalho, timeout e
variáveis de ambiente. `execute` aguarda um processo finito e captura uma única
vez código de saída, `stdout` e `stderr`. A implementação nativa encerra a
espera ao exceder o timeout e nunca usa a área de renderização como fonte do
estado do processo.

Na Fase 5, `javac` e `java` são chamados em um worker assíncrono criado fora da
thread da UI. O resultado chega à aplicação por canal tipado e só então é
anexado ao terminal, evitando bloquear eventos de teclado, mouse ou pintura.

## Isolamento recomendado

Processo principal:

```text
IDE UI
Application Core
Document Model
Command Router
```

Processos auxiliares:

```text
Language Worker
Index Worker
Plugin Host
Build Worker
Debug Worker
```

O `Debug Worker` mantém a conexão com o alvo depurado e traduz suas mensagens em
eventos tipados. Ele só existe enquanto houver sessão ativa: conectar é
explícito, e desconectar — por pedido do usuário, por término do processo ou por
queda da rede — encerra o worker sem afetar a IDE nem o processo depurado.

## Comunicação

Preferir mensagens tipadas:

```rust
pub enum LanguageWorkerRequest {
    OpenDocument(OpenDocumentRequest),
    ChangeDocument(ChangeDocumentRequest),
    Completion(CompletionRequest),
    Diagnostics(DiagnosticsRequest),
    Shutdown,
}
```

Evitar compartilhamento de estruturas mutáveis entre processos.

## Cancelamento

Toda operação longa deve aceitar:

```rust
pub struct RequestContext {
    pub request_id: RequestId,
    pub cancellation: CancellationToken,
    pub deadline: Option<Instant>,
}
```

## Backpressure

O sistema deve evitar filas ilimitadas.

Exemplo:

```text
Usuário digita 20 caracteres rapidamente
    ↓
19 análises intermediárias são canceladas
    ↓
somente o snapshot mais recente é analisado
```

## Worker de linguagem da Fase 2

Cada provider ativo executa em uma thread dedicada, com runtime assíncrono
próprio, separado da thread da interface. A comunicação usa mensagens tipadas
para abrir, alterar e fechar documentos, consultar diagnósticos e encerrar.
Esse isolamento de execução é o limite da Fase 2; a migração para processo
auxiliar poderá manter os mesmos contratos de mensagens.

A fila entre o host e cada worker é limitada. Quando estiver cheia, o host
retorna erro de backpressure em vez de acumular memória sem limite. O número
de providers simultaneamente ativos também possui limite configurável.

Toda solicitação recebe `LanguageRequestContext`, com `RequestId` monotônico e
`CancellationToken` compartilhável. O cancelamento é verificado antes de
enfileirar e novamente no worker antes de iniciar a operação. Uma solicitação
cancelada não deve alcançar o provider.

## Terminal integrado

O terminal deve usar uma pseudoterminal persistente por aba e oferecer perfis
selecionáveis. No Windows, a pseudoterminal deve ser implementada sobre
ConPTY. Os perfis iniciais são:

```text
PowerShell
CMD
Git Bash (quando instalado)
```

Cada aba deve iniciar uma única instância interativa do shell no diretório do
workspace ativo e mantê-la viva até a aba ser encerrada. A interface deve
manter histórico limitado e permitir rolagem independente. Cada perfil deve
possuir uma aba, PTY, processo, fluxo de entrada e fluxo de saída próprios.
Entrada, histórico, saída e posição de rolagem não podem ser compartilhados
entre abas, e somente a sessão ativa deve ser renderizada.
A linha de entrada deve ficar no topo da área de conteúdo do terminal e mostrar
o caminho do workspace atual no prompt. O comando submetido, `stdout` e
`stderr` devem ser acrescentados abaixo dela em ordem cronológica.

O diretório atual pertence ao processo de shell de cada sessão. Comandos `cd`,
`chdir`, `Set-Location`, aliases, variáveis de ambiente, funções e demais
recursos devem ser interpretados pelo próprio shell persistente. Uma mudança
válida aparece no prompt produzido pelo shell e afeta todos os comandos
posteriores daquela aba. Trocar de aba não pode copiar nem redefinir o estado
de outra sessão.

### Arquitetura PTY

- usar `portable-pty`, que utiliza ConPTY no Windows;
- manter `MasterPty`, escritor e processo filho durante toda a vida da aba;
- ler a saída continuamente em thread dedicada e entregá-la à UI sem bloquear;
- enviar o texto digitado diretamente ao fluxo de entrada do shell;
- nunca iniciar um novo processo para cada comando;
- nunca interpretar `cd` ou reescrever comandos na IDE;
- redimensionar visualmente o painel sem alterar as dimensões do PTY enquanto a
  saída for armazenada como scrollback linear;
- somente propagar dimensões ao PTY quando a camada de apresentação possuir uma
  grade VT capaz de aplicar redesenhos e movimentações de cursor sem convertê-los
  em novas linhas de histórico;
- limitar o scrollback para evitar crescimento de memória sem limite;
- encerrar o processo filho quando a aba/sessão for descartada;
- preservar sequências e comportamento necessários a programas interativos,
  normalizando somente controles de apresentação que a camada gráfica ainda
  não renderiza.

A saída deve representar linhas lógicas, não cada redesenho enviado pelo shell.
Sequências de reposicionamento de cursor não podem duplicar resultados no
histórico. Enquanto a camada gráfica não possuir uma grade VT completa, o
PSReadLine deve ser desativado silenciosamente nas sessões PowerShell para
evitar que seus redesenhos sejam persistidos como novas linhas.

O arraste vertical do painel altera exclusivamente o layout, o recorte e a
quantidade de linhas visíveis. Ele não pode produzir entrada ou saída no PTY,
reexecutar comandos, duplicar prompts ou acrescentar conteúdo ao histórico.

### Estado do shell e mudança de diretório

Cada aba deve manter, no mínimo:

```text
shell selecionado
diretório atual
entrada em edição
histórico de comandos
stdout e stderr
posição de rolagem
```

Exemplo obrigatório:

```text
C:\workspace\ide> cd crates
C:\workspace\ide\crates> ls
```

O segundo comando deve executar em `C:\workspace\ide\crates`. Não é permitido
iniciar cada comando novamente no diretório original do workspace, nem simular
mudanças de diretório no processo da IDE.

Formas mínimas aceitas:

```text
cd crates
cd ..
cd "pasta com espaços"
cd /d C:\outro\caminho
chdir crates
Set-Location crates
sl crates
cd ~
```

Regras:

- caminho relativo é resolvido a partir do diretório atual daquela aba;
- caminho absoluto substitui o diretório atual;
- `..` navega para o diretório pai;
- `~` usa o diretório do usuário;
- `cd /d` deve aceitar mudança de unidade no perfil CMD;
- caminhos entre aspas devem preservar espaços;
- caminho inexistente mantém o diretório anterior e produz uma linha de erro;
- caminhos internos do Windows, como os iniciados por `\\?\`, não devem ser
  exibidos ao usuário;
- PowerShell, CMD e Git Bash devem preservar seus diretórios
  independentemente.
A IDE não deve interpretar comandos: o texto digitado é entregue ao shell
persistente explicitamente selecionado pelo usuário. Isso inclui comandos
internos, aliases, variáveis, pipelines, programas interativos e alterações de
estado.

### Painel do terminal

O painel que contém as abas de terminal deve oferecer:

- botão de minimizar/restaurar no canto superior direito;
- ao minimizar, ocultar a área de conteúdo e manter visível um cabeçalho
  compacto que permita restaurar o painel;
- ao restaurar, recuperar a última altura utilizada pelo usuário;
- redimensionamento vertical pela borda superior;
- ao posicionar o ponteiro sobre a borda superior, exibir cursor de
  redimensionamento vertical;
- iniciar o redimensionamento ao pressionar o botão principal do mouse sobre a
  borda, continuar enquanto ele estiver pressionado e finalizar ao soltá-lo;
- arrastar a borda para cima aumenta a altura do painel e arrastá-la para baixo
  reduz a altura;
- aplicar alturas mínima e máxima para não ocultar completamente o editor nem
  permitir que o terminal ultrapasse a janela;
- preservar abas, processos, entradas, históricos e posições de rolagem ao
  minimizar, restaurar ou redimensionar;
- solicitar novo layout e nova renderização durante o arraste.

A área de saída deve permitir seleção visual de texto por clique e arraste,
inclusive no sentido inverso e entre linhas. A barra de rolagem deve responder
ao clique na trilha e ao arraste do indicador. A roda do mouse deve rolar o
conteúdo sob o ponteiro. Quando o usuário sair do final do histórico, novas
saídas não podem forçar o retorno automático ao fim; o acompanhamento
automático é retomado quando a posição voltar ao final.

O comportamento deve ser implementado pela camada de apresentação usando os
eventos e contratos do ERLibUi. A biblioteca de UI só deverá ser alterada se os
contratos existentes não permitirem captura de ponteiro, indicação de cursor ou
invalidação contínua durante o arraste.
