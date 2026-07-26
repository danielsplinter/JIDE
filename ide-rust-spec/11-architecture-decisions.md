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
