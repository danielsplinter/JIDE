# 09 — Estrutura Inicial do Workspace Rust

## Estrutura

```text
ide/
├── Cargo.toml
├── crates/
│   ├── ide-app/
│   ├── ide-core/
│   ├── ide-domain/
│   ├── ide-events/
│   ├── ide-text/
│   ├── ide-ui/
│   ├── ide-workspace/
│   ├── ide-project-model/
│   ├── ide-language-api/
│   ├── ide-language-host/
│   ├── ide-toolchain-api/
│   ├── ide-process/
│   ├── ide-index-api/
│   ├── ide-index-store/
│   ├── ide-plugin-api/
│   ├── ide-plugin-host/
│   ├── ide-debug-api/
│   ├── ide-build-api/
│   ├── language-java/
│   ├── java-parser/
│   ├── java-semantics/
│   ├── java-classfile/
│   ├── java-toolchain/
│   ├── java-javac-adapter/
│   ├── java-maven-adapter/
│   ├── java-gradle-adapter/
│   └── websphere-adapter/
├── specs/
└── tests/
```

## Dependências permitidas

```text
ide-domain
    não depende de infraestrutura

ide-language-api
    depende de ide-domain

language-java
    depende de ide-language-api

java-javac-adapter
    depende de ide-toolchain-api e ide-process

ide-app
    compõe implementações
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
