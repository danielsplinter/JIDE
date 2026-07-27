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
