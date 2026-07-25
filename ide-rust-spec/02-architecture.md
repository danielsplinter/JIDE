# 02 — Arquitetura Geral

## Estilo arquitetural

A arquitetura deverá combinar:

- Hexagonal Architecture;
- Ports and Adapters;
- Component-Based Architecture;
- Event-Driven Architecture;
- Query-based incremental computation;
- isolamento por processos quando necessário.

## Camadas

```text
┌─────────────────────────────────────────────┐
│ Presentation                               │
│ editor, painéis, comandos, atalhos          │
├─────────────────────────────────────────────┤
│ Application                                │
│ casos de uso, orquestração, estado          │
├─────────────────────────────────────────────┤
│ Domain                                     │
│ documentos, workspace, símbolos, projetos  │
├─────────────────────────────────────────────┤
│ Ports                                      │
│ contratos para linguagens e ferramentas    │
├─────────────────────────────────────────────┤
│ Adapters                                   │
│ Java, Git, Maven, WebSphere, filesystem     │
├─────────────────────────────────────────────┤
│ Infrastructure                             │
│ processos, persistência, IPC, renderização  │
└─────────────────────────────────────────────┘
```

## Regra de dependência

```text
Presentation → Application → Domain ← Ports
                                      ↑
                                  Adapters
```

O domínio não deve depender de:

- `wgpu`;
- `winit`;
- Tree-sitter;
- Maven;
- Gradle;
- Java;
- WebSphere;
- banco de dados;
- sistema operacional.

## Componentes principais

### Core

Responsável por:

- modelo de documento;
- workspace;
- comandos;
- eventos;
- diagnósticos genéricos;
- posições e intervalos;
- edição de texto;
- ciclo de vida.

### Language Host

Responsável por:

- registrar providers;
- ativar providers;
- desativar providers;
- rotear requisições;
- controlar memória;
- controlar prioridades;
- fornecer isolamento.

### Toolchain Host

Responsável por:

- JDK;
- compiladores;
- interpretadores;
- runtimes;
- formatadores;
- linters;
- gerenciadores de build;
- depuradores.

### Project Model

Responsável por:

- módulos;
- dependências;
- source roots;
- test roots;
- recursos;
- classpath;
- configurações por linguagem;
- ferramentas associadas.

### Index Service

Responsável por:

- símbolos;
- referências;
- hierarquias;
- metadados;
- persistência;
- invalidação incremental.

### Process Supervisor

Responsável por:

- iniciar processos;
- capturar saída;
- aplicar timeout;
- cancelar execução;
- limitar recursos;
- detectar falhas;
- reiniciar serviços isolados.

## Regra de composição

Objetos devem receber dependências por composição.

Evitar:

```rust
trait JavaIde: Ide {
    fn compile_java(&self);
}
```

Preferir:

```rust
struct IdeApplication {
    language_host: Arc<dyn LanguageHost>,
    toolchain_host: Arc<dyn ToolchainHost>,
    project_service: Arc<dyn ProjectService>,
}
```

Java será um adapter registrado no `LanguageHost`, não uma subclasse da IDE.
