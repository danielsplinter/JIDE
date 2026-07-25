# 10 — Roadmap

## Fase 0 — Fundação ✅ Concluída

Concluída em 24/07/2026. Validada com `cargo test --workspace` e
`cargo clippy --workspace --all-targets -- -D warnings`.

- [x] workspace Cargo;
- [x] contratos;
- [x] eventos;
- [x] configuração;
- [x] logging;
- [x] process supervisor;
- [x] testes de arquitetura.

## Fase 1 — Editor ✅ Concluída

Concluída em 25/07/2026. Validada com testes do workspace, Clippy sem warnings
e inicialização real da janela com o renderer WGPU do ERLibUi.

- [x] janela;
- [x] renderização;
- [x] buffer;
- [x] abas;
- [x] árvore de arquivos;
- [x] busca;
- [x] comandos;
- [x] terminal.

## Fase 2 — Language Host

- [ ] registro;
- [ ] capabilities;
- [ ] ativação;
- [ ] desativação;
- [ ] seleção de provider;
- [ ] worker isolado;
- [ ] cancelamento.

## Fase 3 — Java sintático

- [ ] gramática;
- [ ] parser incremental;
- [ ] syntax tree;
- [ ] outline;
- [ ] highlighting;
- [ ] erros sintáticos;
- [ ] imports.

## Fase 4 — Java semântico

- [ ] símbolos;
- [ ] escopos;
- [ ] tipos;
- [ ] resolução;
- [ ] class files;
- [ ] jars;
- [ ] navegação;
- [ ] referências;
- [ ] autocomplete.

## Fase 5 — Toolchain Java

- [ ] detecção de JDK;
- [ ] seleção de JDK;
- [ ] javac;
- [ ] execução;
- [ ] testes;
- [ ] classpath.

## Fase 6 — Maven e Gradle

- [ ] detecção;
- [ ] importação;
- [ ] módulos;
- [ ] dependências;
- [ ] build;
- [ ] código gerado.

## Fase 7 — WebSphere

- [ ] detecção;
- [ ] perfis;
- [ ] servidores;
- [ ] deploy;
- [ ] logs;
- [ ] debug remoto;
- [ ] wsadmin.

## Fase 8 — Plugins

- [ ] manifesto;
- [ ] permissões;
- [ ] WASM;
- [ ] processo isolado;
- [ ] API versionada.

## Fase 9 — Segunda linguagem

Escolher uma linguagem com modelo diferente de Java para validar a arquitetura.

Sugestões:

- Python, para interpretar/runtime;
- Rust, para integração com Cargo;
- TypeScript, para projetos frontend.

A segunda linguagem deve ser adicionada sem alterar contratos centrais.
