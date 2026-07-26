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
selected = "temurin-17"
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
└── conexão JDWP a um processo em execução
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

## Implementação da Fase 2

O crate `ide-language-host` implementa o registro central. No registro, o host:

- valida a versão principal de `ide-language-api`;
- rejeita identificadores vazios e providers duplicados;
- normaliza extensões sem ponto e sem distinção entre maiúsculas e minúsculas;
- armazena metadata, capabilities, estado e último erro de cada provider.

A ativação é preguiçosa: registrar um provider não cria seu runtime. A primeira
solicitação para uma extensão compatível muda o estado de `Registered` para
`Activating` e então para `Active`. Falha de ativação muda o estado para
`Failed` e faz o host tentar o próximo fallback configurado.

`ProviderSelection` define um provider principal e uma lista ordenada de
fallbacks por linguagem. O roteamento filtra primeiro por extensão e
capabilities; um provider sem a capability solicitada nunca recebe a operação.
Na ausência de configuração explícita, a ordem estável dos identificadores é
usada.

Cada documento aberto fica associado ao provider que aceitou sua abertura.
Mudanças, diagnósticos e fechamento são enviados ao mesmo worker até que o
documento seja fechado ou o provider seja desativado.

`disable` executa `shutdown`, encerra o worker, remove as rotas de documentos e
termina em `Disabled`. `enable` devolve um provider desativado ou falho para
`Registered`, permitindo uma nova ativação sob demanda.
