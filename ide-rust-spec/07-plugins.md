# 07 — Extensibilidade e Plugins

## Princípio

Plugins não devem alterar diretamente o núcleo da IDE.

Eles devem contribuir por contratos versionados.

## Tipos de extensão

- language provider;
- parser;
- analyzer;
- formatter;
- linter;
- compiler;
- runtime;
- build system;
- debugger;
- project importer;
- server integration;
- painel;
- comando;
- tema.

## Manifesto

```toml
[plugin]
id = "com.example.java"
name = "Java Support"
version = "0.1.0"
api_version = "1"

[capabilities]
languages = ["java"]
commands = ["java.build", "java.run"]

[permissions]
filesystem = ["workspace-read"]
process = ["java", "javac", "mvn"]
network = false
```

## Modelo de execução

### Plugins WebAssembly

Usar para:

- regras;
- comandos simples;
- transformações;
- formatadores;
- integrações controladas.

### Plugins em processo isolado

Usar para:

- análises pesadas;
- depuradores;
- toolchains;
- integrações com servidores;
- serviços nativos.

## Permissões

Cada plugin deve declarar:

- acesso a arquivos;
- execução de processos;
- rede;
- secrets;
- terminal;
- UI;
- workspace.

## Versionamento

```rust
pub struct PluginApiVersion {
    pub major: u16,
    pub minor: u16,
}
```

Alterações incompatíveis incrementam a versão principal.

## Regra

O plugin não deve receber referências diretas a estruturas internas.

Ele deve receber DTOs e handles estáveis.
