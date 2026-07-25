# 04 — Sistema de Linguagens

## Conceito

Cada linguagem deve ser representada por um provider independente.

Exemplos:

```text
JavaLanguageProvider
RustLanguageProvider
PythonLanguageProvider
TypeScriptLanguageProvider
```

O núcleo conhece apenas:

```rust
Arc<dyn LanguageProvider>
```

## Registro

```rust
pub trait LanguageRegistry: Send + Sync {
    fn register(
        &self,
        provider: Arc<dyn LanguageProvider>,
    ) -> Result<(), RegistryError>;

    fn unregister(
        &self,
        provider_id: &ProviderId,
    ) -> Result<(), RegistryError>;

    fn providers_for_extension(
        &self,
        extension: &str,
    ) -> Vec<ProviderDescriptor>;

    fn active_provider(
        &self,
        language_id: &LanguageId,
    ) -> Option<ProviderDescriptor>;
}
```

## Ativação e desativação

Estado sugerido:

```rust
pub enum ProviderState {
    Registered,
    Disabled,
    Activating,
    Active,
    Suspended,
    Failed,
    ShuttingDown,
}
```

Fluxo:

```text
Registered
   ↓ enable
Activating
   ↓ success
Active
   ↓ idle policy
Suspended
   ↓ request
Active
   ↓ disable
ShuttingDown
   ↓
Disabled
```

## Seleção de provider

Uma linguagem pode possuir vários providers:

```text
Java
├── native-java-analyzer
├── jdtls-adapter
└── remote-java-service
```

Configuração:

```toml
[languages.java]
enabled = true
provider = "native-java-analyzer"
fallback_provider = "jdtls-adapter"
```

## Seleção de toolchain

Provider e toolchain são conceitos diferentes.

```text
Language Provider
    analisa o código

Toolchain
    compila e executa
```

Exemplo:

```toml
[languages.java]
provider = "native-java-analyzer"

[toolchains.java]
selected = "ibm-java-8-websphere"
```

## Capabilities

```rust
bitflags::bitflags! {
    pub struct LanguageCapabilities: u64 {
        const SYNTAX = 1 << 0;
        const SEMANTICS = 1 << 1;
        const COMPLETION = 1 << 2;
        const DIAGNOSTICS = 1 << 3;
        const DEFINITION = 1 << 4;
        const REFERENCES = 1 << 5;
        const RENAME = 1 << 6;
        const FORMAT = 1 << 7;
        const BUILD = 1 << 8;
        const RUN = 1 << 9;
        const DEBUG = 1 << 10;
    }
}
```

## Estratégia de fallback

Se uma capability não estiver disponível no provider principal, o host pode consultar outro adapter.

Exemplo:

```text
Native Java Provider
├── parsing
├── symbols
├── autocomplete
└── sem debugger

Java Debug Adapter
└── debugger JDWP
```

O usuário percebe um único suporte Java, mas internamente as capacidades são compostas.

## Composição de capacidades

```rust
pub struct LanguageRuntime {
    syntax: Arc<dyn SyntaxEngine>,
    semantics: Option<Arc<dyn SemanticEngine>>,
    index: Arc<dyn SymbolIndex>,
    formatter: Option<Arc<dyn FormatterAdapter>>,
    compiler: Option<Arc<dyn CompilerAdapter>>,
    runtime: Option<Arc<dyn RuntimeAdapter>>,
    debugger: Option<Arc<dyn DebugAdapter>>,
}
```

Não criar uma classe monolítica que implemente tudo.
