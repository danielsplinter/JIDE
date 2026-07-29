# 09 — Estrutura Inicial do Workspace Rust

## Estrutura

```text
ide/
├── Cargo.toml
├── crates/
│   ├── ide-app/
│   ├── ide-application/
│   ├── ide-core/
│   ├── ide-domain/
│   ├── ide-text/
│   ├── ide-ui/
│   ├── ide-workspace/
│   ├── ide-project/
│   ├── ide-language-api/
│   ├── ide-language-host/
│   ├── ide-toolchain-api/
│   ├── ide-process/
│   ├── ide-index-api/
│   ├── ide-index-store/
│   ├── ide-plugin-api/
│   ├── ide-plugin-host/
│   ├── ide-debug-api/
│   ├── language-java/
│   ├── java-parser/
│   ├── java-semantics/
│   ├── java-classfile/
│   ├── java-toolchain/
│   ├── java-javac-adapter/
│   ├── java-maven-adapter/
│   ├── java-gradle-adapter/
│   └── java-debug-adapter/
├── specs/
└── tests/
```

## Dependências permitidas

```text
ide-domain
    não depende de infraestrutura

ide-application
    depende de ide-domain e reúne comandos, eventos e, gradualmente, os casos
    de uso coordenados pela aplicação

ide-language-api
    depende de ide-domain

ide-language-host
    depende de ide-language-api e ide-domain

language-java
    depende de ide-language-api, ide-domain, java-classfile e da gramática
    tree-sitter-java

java-classfile
    depende somente de contratos próprios e do leitor ZIP

java-toolchain
    depende de ide-toolchain-api e ide-domain

java-javac-adapter
    depende de ide-toolchain-api, ide-process e java-toolchain

ide-project
    reúne o modelo neutro em ide_project::model e os contratos de build em
    ide_project::build; não depende de infraestrutura nem de linguagem

java-maven-adapter
java-gradle-adapter
    dependem de ide-project e ide-process

ide-debug-api
    depende de ide-domain; não conhece servidor, container nem protocolo

java-debug-adapter
    depende de ide-debug-api e ide-domain; é o único crate que conhece o
    protocolo de depuração da JVM

ide-app
    compõe Language Host, supervisor de processos, detecção de JDK, adapter
    javac e os adapters de build
```

## Composition Root

Somente `ide-app` deve montar implementações concretas.

```rust
pub fn build_application(config: AppConfig) -> Result<IdeApplication, AppError> {
    let process_supervisor = Arc::new(NativeProcessSupervisor::new());

    let index_store = Arc::new(RedbIndexStore::open(config.index_path)?);

    let language_registry = Arc::new(DefaultLanguageRegistry::new());

    let java_provider = Arc::new(JavaLanguageProvider::new(
        JavaSyntaxEngine::new(),
        JavaSemanticEngine::new(index_store.clone()),
    ));

    language_registry.register(java_provider)?;

    Ok(IdeApplication::new(
        language_registry,
        process_supervisor,
        index_store,
    ))
}
```

## Regra de visibilidade

Preferir:

```rust
pub(crate)
```

em vez de tornar tudo público.

A API pública deve ser pequena e intencional.

## Testes

Separar:

- unitários;
- contratos;
- integração;
- compatibilidade de plugins;
- snapshots;
- desempenho;
- memória;
- processos externos.
