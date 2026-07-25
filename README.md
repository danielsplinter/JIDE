# IDE nativa em Rust

Implementação da IDE descrita em `ide-rust-spec`, usando o ERLibUi como biblioteca
de interface gráfica.

## Estado

As Fases 0 e 1 estão concluídas. A fundação estabelece os tipos de domínio,
contratos, eventos, configuração, logging e supervisão de processos. O editor
inclui shell nativo Winit/WGPU baseado no ERLibUi, buffer, abas, árvore de
arquivos, busca, comandos e terminal.

## Executar

```text
cargo run -p ide-app
```

Pressione `F3` para abrir ou fechar a busca demonstrativa e `Esc` para fechá-la.

## Verificação

```text
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```
# JIDE
