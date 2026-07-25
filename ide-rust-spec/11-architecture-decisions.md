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

## ADR-009 — Ferramentas WebSphere externas

**Decisão:** integração com WebSphere ocorrerá por processos, arquivos, protocolos e APIs.

**Motivo:** não acoplar o processo da IDE à JVM do servidor.

## ADR-010 — Memória como requisito arquitetural

**Decisão:** cada componente terá orçamento e métricas.

**Motivo:** baixo consumo não surge automaticamente por usar Rust.
