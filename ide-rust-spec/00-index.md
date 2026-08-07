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
- [12 — Consolidação de crates e módulos](12-crate-consolidation.md) — **completa**: a fase 8 fez de Java uma crate por linguagem, o workspace caiu de 19 para 14 crates, e a próxima linguagem custa uma. **Hoje são 20 crates**, e a diferença é o preço declarado: TypeScript, Angular, marcação e folhas de estilo entraram uma crate cada, como o desenho previa
- [13 — Desacoplamento da aplicação e da apresentação](13-application-ui-decoupling.md)
- [14 — Decomposição do `ide_shell`](14-ide-shell-decomposition.md)
- [15 — Adoção do runtime de eventos da ERLibUi](15-event-runtime-adoption.md)
- [16 — Um anfitrião só](16-single-host.md)
- [17 — Adoção do arranjo](17-layout-adoption.md)
- [18 — Um terminal de verdade](18-real-terminal.md) — fases 0 a 3 feitas; a **4 é pendência**: seleção lida da grade, busca na saída e links clicáveis
- [19 — Varredura e indexação: sair do bloqueio](19-indexing-and-scanning.md)
- [20 — Índice no disco, memória como cache](20-index-on-disk.md) — **completa**: abrir caiu de 251 s para 3,7 s, a memória de 178 para 103 MB, uma tecla lê 2 mil registros em vez de 340 mil e um fonte alterado custa 3,5 ms
- [21 — O que muda fora da IDE](21-file-watcher.md) — **completa**: o que muda no disco chega ao índice em ~700 ms sem ação do usuário, e a conferência da abertura caiu de 4,66 s para 0,70 s
- [22 — Git](22-git-integration.md) — **as cinco fases feitas**: a crate `ide-git`, uma só, com as capacidades em módulos e a implementação atrás de traits. A barra de estado mostra a branch e quantos arquivos mudaram; o gerenciador abre pelo terceiro botão da barra de atividades, com a árvore de referências à esquerda e a aba `status` empilhando preparados, alterados e não rastreados. Cada linha traz as ações daquele painel — e cada ação pede o retrato de novo, senão a lista fica velha —, a margem do editor mostra o que mudou desde o commit, e clicar num arquivo abre a comparação com o texto de então ao lado, como documento de memória e não como cópia no disco. **Não rastreado não tem "Descartar"**: seria apagar do disco o que não tem de onde voltar. A fase 4 tirou o observador de arquivos de dentro do `language-java` — ele virou a crate `ide-watch`, com um registro só no sistema e um filtro por consumidor —, e trouxe o remoto: `fetch`, `pull`, `push`, as branches remotas e a contagem à frente e atrás. Um `push` que precisa de senha **não fica pendurado**, mas também não pergunta: dizer o que aconteceu é o que está feito, e o `CredentialProvider` ponta a ponta não. A fase 3 trouxe trocar, criar e fundir branch, o `stash`, as tags, e a faixa do estado intermediário com **Continuar** e **Abortar** — porque a IDE não pode ficar presa num estado do qual não se sai; trocar de branch recarrega o Explorer, o editor e o índice de símbolos. Resolver conflito é edição de texto normal, que é como ele já está gravado no arquivo. A fase 2 fechou o ciclo: a caixa de mensagem com **Commit** e **Amend** embaixo dos três painéis, e a aba `history` com a tabela de cinco colunas e o grafo — cujas faixas a IDE calcula e a ERLibUi desenha, na `ComposedTable` e na `GraphCell` que entraram lá. Ela descreve também **o gerenciador**: o terceiro botão da barra de atividades abre uma janela com a árvore de `branches`, `tags`, `remotes` e `stashes` à esquerda, uma busca própria acima dela — a terceira da IDE, e independente das outras duas —, e à direita as abas `status`, com três painéis empilhados, e `history`, com a tabela de commits e o grafo. A tela pede três coisas à ERLibUi, e nenhuma delas é lógica de Git: `Icon::Branch`, o `ComposedTable` da `09` de lá, e a célula que desenha o grafo a partir das faixas que a IDE calcula
- [23 — TypeScript](23-typescript.md) — **fases 0 a 6 feitas**, menos dois níveis opcionais de folha de estilo; a 6 acrescentou a completação por nome com **auto-import**: escrever duas letras abre a lista, e escolher um tipo que o arquivo ainda não importa escreve o `import` junto — porque oferecer o nome sozinho seria sugerir código que não compila. A **7 é pendência**, e é a primeira decisão desta especificação que vai além do VS Code: sugerir um nome onde a sintaxe ainda não o espera, porque quem valida sintaxe é o compilador. Java saiu do núcleo, a segunda linguagem existe, o projeto é lido do `tsconfig.json`, o `tsserver` responde com tipo e o `.scss` completa pelo grafo de `@use`; depurar ficou **fora de escopo** — quem depura é o navegador
- [24 — Angular](24-angular.md) — **fase 1 feita**: o `.html` responde com tipo pelo plugin dentro do `tsserver` que já sobe, a +385 MB num processo em vez de +2,1 GB em dois; faltam criar as peças (2) e as tarefas da CLI (3); um framework não é uma linguagem, e entra pelas portas que a `23` já abriu
- [25 — Índice próprio de TypeScript](25-typescript-index.md) — **fases 0 a 9 feitas**; a **9 é a maior de todas**: os tipos das dependências instaladas passaram a ser alcançados pelo ponto — `this.http.` com `HttpClient` de `@angular/common/http` responde —, e a cobertura da aplicação **quase dobrou, de 28,5% para 53,6%**; a busca por nome continua sem `node_modules`, porque são duas perguntas e a fase 1 as tratava como uma. Custa ~9 ms por ponto na aplicação e ~44 ms no monorepo, e guardar os membros já extraídos por arquivo é o próximo passo. Antes disso: os `lib.*.d.ts` do próprio TypeScript entraram no índice — 1 525 tipos, cache de 458 KB relido em 21 ms contra 1,26 s de análise —, e `this.svc.`, `this.buscar().` e `this.nome.` com `nome: string` respondem sem o analisador. Medida com o instrumento corrigido, a cobertura do ponto é de **32,5% no monorepo e 28,5% na aplicação**; a fase 8 rendeu 3,3 pontos no monorepo e **zero** na aplicação, onde os tipos injetados vêm de `node_modules`. **O instrumento de medição errou três vezes** — amostra enviesada, estimativa dobrada, e pontos dentro de aspas contados como perguntas —, e por isso as porcentagens anteriores a esta não são comparáveis. O analisador externo só sobe quando o índice diz que não alcança; o `.` declarado responde 14% a 17% dos pontos de um projeto real e diz "não sei" no resto; a busca por tipo responde em 4 ms contra os 30 s do analisador externo, com +4 MB e sem `node_modules`; o analisador externo custa 1,9 GB e 30 s porque guarda o programa inteiro; um índice responde busca, navegação e o `.` **declarado** em dezenas de MB — e diz que não soube no resto, em vez de mentir
- [26 — A dívida que uma sessão longa deixou](26-divida-de-arquitetura.md) — **pendente, as três**: `native_ide.rs` chegou a 4 680 linhas sem teto que o vigie; o teto de `block_on` subiu duas vezes no mesmo dia; e o auto-import guarda no serviço a última lista respondida, que pode envelhecer. Nenhuma é defeito — são hipotecas, e a especificação registra **por que ficaram assim**, que é o que uma lista de tarefas perde
- [27 — Fechar o par, e indentar ao abrir linha](27-pares-e-indentacao.md) — **completa**: `(`, `{` e `[` fecham sozinhos, digitar o fechamento à mão passa por cima dele em vez de duplicar, o abridor envolve o trecho marcado, e apagá-lo leva o fechamento junto **só enquanto os dois estão na mesma linha** — depois do `Enter` ele deixou de ser eco de tecla e virou o fim de um bloco. O `Enter` dentro do par abre o bloco em três linhas, com o cursor no fim da linha em branco e o fechamento alinhado com quem o abriu, e o degrau é **lido do arquivo** — degraus que se repetem, e não a menor indentação, que seria o espaço do `*` de um comentário. As aspas, sendo pares **simétricos**, invertem a ordem das perguntas — primeiro passar por cima, depois abrir — e trazem duas defesas próprias: `don't` não abre string, e apagar leva só a aspa encostada. A **3 foi dispensada por quem usa a IDE**: não fechar dentro de texto nem de comentário custaria uma consulta ao realce e um caso que nunca acertaria — o realce chega uma revisão atrás —, e o fechamento sobrando dentro de uma string incomoda sem quebrar nada. **Completa**, portanto. Mora no `ide-ui` porque é lá que o texto vive — o `CodeEditor` da ERLibUi desenha, e a `08` dela já prevê a aplicação que é dona do texto
- [28 — A divisão do editor](28-divisao-do-editor.md) — **fases 1 e 2 feitas**: `Split direita` no menu da aba abre dois editores lado a lado sobre o **mesmo** documento, cada um com cursor, rolagem e faixa de abas próprios. A divisão parte a vista, e não o armazém: duas sessões fariam o mesmo arquivo virar dois documentos com duas revisões, e gravar escolheria em silêncio qual sobrevive. O foco segue o ponteiro; **quem recebe um arquivo aberto é o painel do último clique**, porque o caminho do mouse até o Explorer atravessa o painel do lado. A **3 é pendência**: divisão vertical, mais de duas divisões e arrastar aba entre lados
- [29 — O que a IDE segura sem precisar](29-memoria.md) — **pendente**: cinco itens de memória, do mais barato ao mais delicado. Três terminais sobem na abertura mesmo sem ninguém abrir o painel; o desfazer guarda o arquivo **inteiro** dez vezes, por painel; a suspensão por ociosidade nunca alcança quem tem arquivo aberto. Antes de qualquer um deles, **medir** — o medidor já está na tela, e a lista de hoje está ordenada por raciocínio

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
