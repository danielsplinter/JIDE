# Especificação da IDE Nativa em Rust

## Objetivo

Construir uma IDE nativa em Rust, extensível e preparada para múltiplas linguagens, iniciando por Java.

A IDE não deve depender de uma JVM para executar sua interface, infraestrutura, editor, indexador ou núcleo de análise. Ferramentas externas de uma linguagem, como compiladores, runtimes, interpretadores, depuradores e gerenciadores de dependências, poderão ser conectadas por meio de adaptadores desacoplados.

## Princípios arquiteturais

1. Dependências devem apontar para abstrações.
2. Composição deve ser priorizada em relação à herança.
3. Cada integração externa deve ficar atrás de uma interface de contrato.
4. Linguagens devem ser ativáveis, desativáveis e substituíveis.
5. O núcleo da IDE não deve conhecer detalhes de Java, Python, Rust ou qualquer outra linguagem.
6. Plugins não devem possuir acesso irrestrito ao processo principal.
7. Serviços pesados devem poder executar em processos isolados.
8. O consumo de memória deve ser controlado por orçamentos e políticas explícitas.
9. A análise deve ser incremental.
10. Funcionalidades opcionais não devem ser inicializadas até serem necessárias.


## Tecnologia oficial do projeto

Todo o projeto será desenvolvido em **Rust**.

Isso inclui:

- núcleo da IDE;
- interface gráfica;
- editor de texto;
- gerenciamento de workspace;
- sistema de eventos;
- sistema de plugins;
- host de linguagens;
- analisadores sintáticos e semânticos nativos;
- indexadores;
- persistência;
- integração com ferramentas externas;
- infraestrutura compartilhada.

Não serão aceitas implementações do núcleo da IDE em outras linguagens.

Ferramentas externas (como JDK, Maven, Gradle, servidores de aplicação, containers, Python, Node.js etc.) poderão ser utilizadas apenas como dependências de execução para compilar, executar ou depurar projetos do usuário, nunca para implementar a IDE.

Nenhum servidor, container ou fornecedor deve ocupar posição privilegiada na arquitetura. A integração com processos em execução se dá pela porta de depuração, o que torna qualquer servidor Java um alvo equivalente.

Este é um requisito arquitetural obrigatório.


## Índice

- [01 — Visão do produto](01-product-vision.md)
- [02 — Arquitetura geral](02-architecture.md)
- [03 — Contratos centrais](03-core-contracts.md)
- [04 — Sistema de linguagens](04-language-system.md)
- [05 — Integração Java](05-java-integration.md)
- [06 — Ciclo de vida e processos](06-lifecycle-and-processes.md)
- [07 — Extensibilidade e plugins](07-plugins.md)
- [08 — Persistência, cache e memória](08-storage-and-memory.md)
- [09 — Estrutura inicial do workspace Rust](09-rust-workspace.md)
- [10 — Roadmap](10-roadmap.md)
- [11 — Decisões arquiteturais](11-architecture-decisions.md)
- [12 — Consolidação de crates e módulos](12-crate-consolidation.md) — **completa**: a fase 8 fez de Java uma crate por linguagem, o workspace caiu de 19 para 14 crates, e a próxima linguagem custa uma
- [13 — Desacoplamento da aplicação e da apresentação](13-application-ui-decoupling.md)
- [14 — Decomposição do `ide_shell`](14-ide-shell-decomposition.md)
- [15 — Adoção do runtime de eventos da ERLibUi](15-event-runtime-adoption.md)
- [16 — Um anfitrião só](16-single-host.md)
- [17 — Adoção do arranjo](17-layout-adoption.md)
- [18 — Um terminal de verdade](18-real-terminal.md) — fases 0 a 3 feitas; a **4 é pendência**: seleção lida da grade, busca na saída e links clicáveis
- [19 — Varredura e indexação: sair do bloqueio](19-indexing-and-scanning.md)
- [20 — Índice no disco, memória como cache](20-index-on-disk.md) — **completa**: abrir caiu de 251 s para 3,7 s, a memória de 178 para 103 MB, uma tecla lê 2 mil registros em vez de 340 mil e um fonte alterado custa 3,5 ms
- [21 — O que muda fora da IDE](21-file-watcher.md) — **completa**: o que muda no disco chega ao índice em ~700 ms sem ação do usuário, e a conferência da abertura caiu de 4,66 s para 0,70 s
- [22 — Git](22-git-integration.md) — **não iniciada**: a crate `ide-git`, uma só, com as capacidades em módulos e a implementação atrás de traits
- [23 — TypeScript](23-typescript.md) — fases 0 a 2 feitas: Java saiu do núcleo, a segunda linguagem existe e o projeto é lido do `tsconfig.json`; a **3 é a próxima**, com o analisador externo
- [24 — Angular](24-angular.md) — **não iniciada**: um framework não é uma linguagem, e entra pelas portas que a `23` já abriu; depende da fase 3 dela

## Escopo inicial, e o que dele já existe

A lista abaixo era a da primeira versão. Está mantida como escrita, com o estado
de cada item — um sumário que descreve só a intenção envelhece em silêncio, e
quem chega ao projeto não tem como saber o que está de pé.

| item | estado | onde |
|---|---|---|
| editor de texto nativo | ✅ | `ui-editor` na ERLibUi, `EditorPane` na IDE |
| gerenciamento de workspace | ✅ | `ide-workspace` |
| árvore de arquivos | ✅ | `explorer`, sobre a `TreeView` da biblioteca |
| comandos e atalhos | ✅ | `menus`, `ApplicationCommand`; o `ShortcutMap` da biblioteca ainda não é consumido |
| terminal integrado | ✅ | `ide-terminal`, com abas, seleção e o `Console` da biblioteca |
| registro de linguagens | ✅ | `ide-language-api`, `ide-language-host` |
| ativação e desativação de suporte linguístico | 🔶 | o host ativa por contribuição; desativar em tempo de execução não existe |
| parser Java | ✅ | `language-java`, sobre tree-sitter |
| análise sintática Java | ✅ | destaque, diagnósticos e estrutura numa passada |
| indexação de símbolos | ✅ | busca por tipo e por conteúdo |
| navegação para definição | ✅ | `definition` na API de linguagem |
| busca por referências | ✅ | `references_to_name`, usada também pela renomeação |
| autocomplete inicial | ✅ | lista de completação, inclusive por membro após o ponto |
| execução de `javac`, Maven e Gradle | ✅ | `java-maven-adapter`, `java-gradle-adapter`, `java-toolchain` |
| configuração de um JDK externo | ✅ | Configurações, com JDK e Maven persistidos juntos |
| depuração remota de qualquer processo Java | ✅ | `java-debug-adapter`: pontos de parada, quadros, variáveis, inspeção e passo |
| logs e diagnósticos | 🔶 | diagnósticos no editor e saída no terminal; não há registro estruturado |
| arquitetura de plugins | ⬜ | não existe; a extensibilidade hoje é por contribuição de linguagem |

Além do previsto, foram construídos: renomeação de arquivo e de tipo com reescrita
de referências, geração de acessores e construtor, criação de pacote, classe e
interface, e inspeção de objetos durante a depuração.

**O que a lista não capturava e virou trabalho próprio:** a interface da IDE
crescera reimplementando à mão o que a ERLibUi oferece. As especificações `14`,
`15` e `16` tratam disso — decompor o shell, adotar o runtime de eventos da
biblioteca, e reduzir a IDE ao que é dela: compor a tela e dizer o que cada gesto
significa no domínio.

## Fora do escopo inicial

- equivalência imediata com IntelliJ IDEA Ultimate;
- reimplementação completa de Maven ou Gradle;
- compilador Java completo escrito em Rust;
- execução interna de bytecode Java;
- suporte avançado a todas as linguagens desde a primeira versão;
- marketplace público de plugins;
- colaboração em tempo real.
