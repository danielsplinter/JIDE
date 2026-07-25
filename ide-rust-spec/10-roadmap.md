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

Concluída em 25/07/2026. Validada com testes do workspace, testes de interação
para abas/editor/Explorer, Clippy sem warnings e inicialização real da janela
com o renderer WGPU do ERLibUi.

- [x] janela;
- [x] renderização;
- [x] buffer;
- [x] abas;
- [x] árvore de arquivos;
- [x] busca;
- [x] comandos;
- [x] terminal.
- [x] barra de menu e abertura de projeto.

### Critérios funcionais da Fase 1

- editor e terminal possuem rolagem independente por roda do mouse e barra
  vertical proporcional ao conteúdo;
- a barra aceita clique na trilha e arraste do indicador, e o terminal permite
  selecionar visualmente texto por clique e arraste;
- novas saídas não anulam a rolagem manual enquanto o usuário consulta o
  histórico;
- a barra de menu oferece `Arquivo → Projeto...`, abre um seletor nativo de
  pasta e carrega todo o conteúdo permitido da pasta na árvore do Explorer;
- cancelar a seleção preserva o workspace atual; selecionar outra pasta
  substitui a raiz, o nome do projeto e as sessões de terminal;
- o Explorer recorta a árvore dentro do painel esquerdo e oferece rolagem
  horizontal interativa quando nomes ou níveis de indentação excedem a largura;
- o Explorer oferece rolagem vertical e sua borda direita pode ser arrastada
  horizontalmente; editor e terminal usam juntos a largura restante;
- cada aba do editor possui botão `x` que fecha somente o documento clicado;
- títulos longos das abas são abreviados e recortados antes do botão `x`, sem
  vazar para abas vizinhas;
- o terminal apresenta abas independentes para PowerShell, CMD e Git Bash;
- cada aba preserva isoladamente entrada, histórico, saída e posição de rolagem;
- ao alternar a aba, somente o conteúdo da sessão ativa é exibido;
- o prompt no topo mostra o caminho do workspace e os resultados são exibidos
  abaixo em ordem cronológica;
- cada aba mantém seu diretório atual; `cd`, `chdir` e `Set-Location` alteram o
  prompt e o diretório dos comandos seguintes;
- cada aba mantém um processo de shell interativo persistente conectado a uma
  PTY (ConPTY no Windows), sem criar um processo novo por comando;
- variáveis, aliases, funções e mudanças de diretório permanecem entre comandos
  porque são interpretados pelo próprio shell;
- a leitura da saída é assíncrona e programas de longa duração não bloqueiam a
  interface;
- o redimensionamento vertical do painel altera somente o layout e nunca
  multiplica conteúdo do terminal;
- o painel do terminal possui botão de minimizar/restaurar no canto superior
  direito e recupera a última altura ao ser restaurado;
- a borda superior do painel permite redimensionamento vertical por arraste,
  respeitando alturas mínima e máxima;
- o editor detecta `Ctrl+Click`, identifica o token sob o ponteiro e emite uma
  solicitação genérica de navegação;
- a aplicação oferece `open_location` para abrir arquivo e posicionar o cursor
  em uma linha e coluna, sem depender de uma linguagem específica;
- PowerShell e CMD são disponibilizados no Windows;
- Git Bash é disponibilizado quando uma instalação do Git for detectada;
- comandos são executados pelo shell selecionado no diretório do workspace;
- a saída combinada do terminal interativo é apresentada na ordem produzida
  pelo PTY;
- a execução do terminal não altera o shell do processo principal.

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

O item de navegação desta fase deve conectar a infraestrutura genérica de
`Ctrl+Click` da Fase 1 ao `DefinitionRequest` do Language Host. O provider Java
resolverá o símbolo e retornará uma ou mais localizações; a aplicação abrirá a
localização escolhida usando `open_location`.

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
