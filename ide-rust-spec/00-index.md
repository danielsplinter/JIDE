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
- [12 — Consolidação de crates e módulos](12-crate-consolidation.md)
- [13 — Desacoplamento da aplicação e da apresentação](13-application-ui-decoupling.md)

## Escopo inicial

A primeira versão deve oferecer:

- editor de texto nativo;
- gerenciamento de workspace;
- árvore de arquivos;
- comandos e atalhos;
- terminal integrado;
- registro de linguagens;
- ativação e desativação de suporte linguístico;
- parser Java;
- análise sintática Java;
- indexação de símbolos;
- navegação para definição;
- busca por referências;
- autocomplete inicial;
- execução de `javac`, Maven e Gradle como processos externos;
- configuração de um JDK externo;
- depuração remota de qualquer processo Java com depuração habilitada,
  independentemente do servidor ou container;
- logs e diagnósticos;
- arquitetura de plugins.

## Fora do escopo inicial

- equivalência imediata com IntelliJ IDEA Ultimate;
- reimplementação completa de Maven ou Gradle;
- compilador Java completo escrito em Rust;
- execução interna de bytecode Java;
- suporte avançado a todas as linguagens desde a primeira versão;
- marketplace público de plugins;
- colaboração em tempo real.
