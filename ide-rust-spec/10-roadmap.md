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

## Fase 2 — Language Host ✅ Concluída

Concluída em 25/07/2026. Validada com testes de contrato e ciclo de vida do
Language Host, testes completos do workspace e Clippy sem warnings.

- [x] registro;
- [x] capabilities;
- [x] ativação;
- [x] desativação;
- [x] seleção de provider;
- [x] worker isolado;
- [x] cancelamento.

### Critérios funcionais da Fase 2

- o registro valida metadata e compatibilidade da versão principal da API,
  normaliza extensões e rejeita providers duplicados;
- capabilities são usadas antes do roteamento, impedindo que uma operação seja
  enviada a um provider incompatível;
- providers são ativados somente na primeira solicitação e permanecem ativos
  para os documentos seguintes;
- um provider executa em worker com thread e runtime próprios, fora da thread
  da interface, por meio de uma fila limitada de mensagens tipadas;
- o número de providers ativos e o tamanho das filas possuem limites
  configuráveis;
- a seleção respeita provider principal e fallbacks ordenados; falha de
  ativação do principal aciona automaticamente o próximo provider;
- documentos abertos preservam a rota para mudanças, diagnósticos e fechamento;
- cada solicitação possui identificador monotônico e token de cancelamento
  verificado antes da fila e antes da execução;
- a desativação solicita `shutdown`, encerra o worker, remove rotas associadas e
  atualiza o estado para `Disabled`;
- a aplicação instancia o Language Host no composition root e atualiza nele o
  caminho sempre que o usuário abre outro projeto.

## Fase 3 — Java sintático ✅ Concluída

Concluída em 25/07/2026. Validada com exemplos das construções exigidas de
Java 8, testes de parsing incremental, extração sintática, integração com o
Language Host, renderização no editor, testes completos do workspace e Clippy
sem warnings.

- [x] gramática;
- [x] parser incremental;
- [x] syntax tree;
- [x] outline;
- [x] highlighting;
- [x] erros sintáticos;
- [x] imports.

### Critérios funcionais da Fase 3

- `language-java` usa `tree-sitter-java` e não inicia uma JVM;
- classes, interfaces, enums, annotations, generics, lambdas, method
  references, imports, inner/anonymous classes, try-with-resources e métodos
  default/static de interfaces são aceitos pela gramática;
- cada documento mantém a árvore anterior e aplica `InputEdit` antes do
  reparsing, preservando a versão do buffer;
- a árvore pública usa somente tipos neutros de `ide-domain`;
- o outline identifica tipos, construtores, métodos e campos de forma
  hierárquica e fica disponível à apresentação;
- o editor colore spans sintáticos entregues pelo provider, sem interpretar
  Java na camada de UI;
- nós inválidos e tokens ausentes geram diagnósticos com intervalo e origem;
- imports normais, estáticos e wildcard são extraídos de forma estruturada;
- abertura, alterações e fechamento de arquivos `.java` são sincronizados com
  o provider pelo Language Host;
- snapshots antigos não são usados para renderizar uma versão mais recente do
  buffer;
- a barra de status informa erros sintáticos, símbolos de outline e imports do
  documento Java ativo.

## Fase 4 — Java semântico ✅ Concluída

Concluída em 25/07/2026. Validada com testes de símbolos, escopos, tipos,
definições locais e entre fontes do workspace, referências, autocomplete,
class files Java 8, indexação de JAR, integração visual, testes completos do
workspace e Clippy sem warnings.

- [x] símbolos;
- [x] escopos;
- [x] tipos;
- [x] resolução;
- [x] class files;
- [x] jars;
- [x] navegação;
- [x] referências;
- [x] autocomplete.

O item de navegação desta fase deve conectar a infraestrutura genérica de
`Ctrl+Click` da Fase 1 ao `DefinitionRequest` do Language Host. O provider Java
resolverá o símbolo e retornará uma ou mais localizações; a aplicação abrirá a
localização escolhida usando `open_location`.

### Critérios funcionais da Fase 4

- snapshots semânticos usam contratos neutros e acompanham a versão do
  documento;
- símbolos distinguem tipos, construtores, métodos, campos, parâmetros e
  variáveis locais;
- escopos representam tipos, executáveis, blocos, lambdas, laços e `catch`;
- tipos preservam nome, arrays e argumentos genéricos declarados;
- resolução prioriza o arquivo atual e o escopo mais profundo, com fallback
  para fontes indexadas do workspace;
- referências podem incluir ou excluir a declaração;
- o leitor de `.class` valida estrutura, versão e constant pool e expõe
  hierarquia, campos e métodos;
- o indexador de JAR aplica limites de quantidade e tamanho e não extrai
  arquivos no workspace;
- `Ctrl+Click` consulta `DefinitionRequest` e abre a localização retornada;
- enquanto `Ctrl` estiver pressionado sobre um tipo Java navegável, o cursor
  muda imediatamente para a mão apontando e volta ao cursor normal ao sair do
  tipo ou soltar `Ctrl`;
- `Ctrl+Space` exibe sugestões de símbolos, classes externas e keywords;
- setas selecionam a sugestão, Enter aplica e Escape fecha o popup;
- todas as operações passam pelo worker cancelável do Language Host.

## Fase 5 — Toolchain Java ✅ Concluída

Concluída em 25/07/2026. Validada com testes de detecção, seleção, classpath,
processos, compilação, execução e ordem compilar-antes-de-testar.

- [x] detecção de JDK;
- [x] seleção de JDK;
- [x] javac;
- [x] execução;
- [x] testes;
- [x] classpath.

Critérios concluídos:

- instalações são descobertas por variáveis de ambiente, workspace, PATH,
  WebSphere e locais usuais da plataforma;
- somente JDKs com `java`, `javac` e `jar` são aceitos;
- o menu `Configurações` abre uma janela com painel lateral e a página
  `Compilador e VM`;
- a página oferece uma combo dos JDKs detectados e `Procurar...` para validar e
  adicionar uma pasta de JDK informada pelo usuário;
- `Ctrl+Shift+J` abre diretamente a página `Compilador e VM`;
- `Ctrl+B` compila todas as fontes Java em `.er-ide/classes`;
- `F5` compila e executa a classe Java ativa;
- `Ctrl+Shift+T` compila e executa a classe Java ativa como teste;
- classpath é deduplicado e usa o separador nativo da plataforma;
- compilação, execução e testes retornam saída tipada sem bloquear a UI;
- `stdout` e `stderr` são apresentados no terminal ativo;
- testes automatizados cobrem detecção, seleção, classpath, processos,
  compilação, execução e ordem compilar-antes-de-testar.

## Fase 6 — Maven e Gradle ✅ Concluída

Concluída em 25/07/2026. Validada com testes de leitura de POM, importação de
projetos multi-módulo Maven e Gradle, execução dos wrappers, filtragem de fontes
pelo modelo importado, testes completos do workspace e Clippy sem warnings.

- [x] detecção;
- [x] importação;
- [x] módulos;
- [x] dependências;
- [x] build;
- [x] código gerado.

### Critérios funcionais da Fase 6

- `ide-project-model` define o modelo neutro de projeto — módulos, coordenadas,
  escopos, raízes de código e diretórios de saída — sem conhecer Maven, Gradle
  ou Java;
- `ide-build-api` define `BuildSystemAdapter` e o registro que escolhe o
  primeiro adapter capaz de reconhecer a raiz do workspace;
- a detecção acontece ao abrir a IDE e ao trocar de projeto, sem iniciar
  processo externo;
- `java-maven-adapter` interpreta o `pom.xml` nativamente: coordenadas herdadas
  do `<parent>`, propriedades com interpolação `${...}`, `<modules>`,
  `<dependencyManagement>`, dependências com escopo e `<build>`;
- módulos declarados que não existem no disco são ignorados, e a recursão
  respeita limites de profundidade e quantidade;
- dependências Maven são resolvidas no repositório local e dependências Gradle
  no cache de módulos; artefatos ausentes não impedem a importação;
- `java-gradle-adapter` trata o Gradle como ferramenta externa: extrai apenas
  `rootProject.name`, `include` e dependências com coordenada literal, e ignora
  o que depende de execução de Groovy ou Kotlin;
- raízes de código geradas — `target/generated-sources`,
  `target/generated-test-sources` e `build/generated/**/{main,test}` — entram no
  modelo e são compiladas como qualquer outra fonte;
- o classpath de `Ctrl+B`, `F5` e `Ctrl+Shift+T` recebe as saídas dos módulos e
  os artefatos das dependências importadas;
- a compilação passa a considerar apenas as fontes sob as raízes do projeto;
  sem projeto importado, permanece a varredura completa do workspace;
- o menu `Projeto` oferece `Compilar projeto` e `Reimportar projeto`, e
  `Ctrl+Shift+B` executa o build do sistema detectado — `compile` no Maven e
  `classes` no Gradle;
- o build usa o wrapper versionado no projeto quando existir, senão o executável
  do `PATH` ou de `MAVEN_HOME`/`GRADLE_HOME`, e recebe o `JAVA_HOME` do JDK
  selecionado;
- a execução ocorre fora da thread da interface e a saída tipada é apresentada
  no terminal ativo;
- alterações do manifesto feitas fora da IDE são detectadas e disparam
  reimportação; uma importação que falha preserva o último modelo válido;
- a barra de status mostra build system, nome do projeto, quantidade de módulos
  e de dependências.

## Fase 7 — Depuração remota ✅ Concluída

Concluída em 25/07/2026. Validada com testes de protocolo, um alvo simulado que
fala JDWP em socket real — cobrindo handshake, breakpoint, parada, pilha,
variáveis e passo a passo —, testes de interface para calha, painel e página de
conexão, testes completos do workspace e Clippy sem warnings.

A integração com servidores acontece pela porta de depuração, e não por
ferramentas de um produto. Qualquer processo Java iniciado com depuração
habilitada — Tomcat, Jetty, WildFly, WebSphere, Liberty, Quarkus, Spring Boot,
um contêiner Docker com a porta exposta, uma migração Flyway, um job em lote —
é um alvo válido, e todos são atendidos pelo mesmo caminho.

A IDE não inicia, não para e não publica nada: quem controla o servidor é o
usuário.

- [x] conexão a host e porta de depuração;
- [x] breakpoints por arquivo e linha;
- [x] parada no breakpoint com o editor posicionado na linha;
- [x] execução linha a linha, entrando, passando por cima e saindo do método;
- [x] pilha de chamadas e seleção de quadro;
- [x] variáveis locais e campos do quadro selecionado;
- [x] avaliação de expressões no contexto do quadro;
- [x] retomada, pausa e desconexão sem afetar o processo depurado;
- [x] mapeamento das posições recebidas para as raízes de código do projeto;
- [x] resiliência: queda da conexão encerra a sessão sem derrubar a IDE.

### Critérios funcionais da Fase 7

- `ide-debug-api` define alvo, breakpoints, quadros, variáveis, eventos e a
  sessão, sem nomear servidor, container ou protocolo;
- `java-debug-adapter` implementa o protocolo da JVM e é o único crate que o
  conhece: handshake, larguras de identificador negociadas com o alvo, e leitura
  de respostas truncadas como erro tipado, nunca como pânico;
- a conexão exige apenas host e porta; suportar mais um servidor não custa
  código novo;
- breakpoints são instalados nas classes já carregadas e reinstalados quando a
  classe é carregada depois, inclusive em classes internas e anônimas;
- uma linha sem código executável move o breakpoint para a próxima linha
  executável e informa isso ao usuário;
- arquivos fora das raízes de código do projeto produzem breakpoint não
  verificado com motivo, em vez de falha;
- clique na calha e `F9` alternam o breakpoint da linha;
- `F8` continua, `F10` passa sobre, `F11` entra e `Shift+F11` sai do método;
- ao parar, o editor abre o arquivo, posiciona o cursor e destaca a linha;
- o painel lateral mostra estado, pilha de chamadas e variáveis do quadro
  selecionado, e escolher um quadro navega até sua linha;
- inspecionar valores nunca invoca métodos no alvo;
- a sessão executa em thread própria com runtime isolado; a janela nunca espera
  pelo alvo;
- perder a conexão encerra a sessão, avisa na barra de status e mantém a IDE
  funcionando;
- nenhuma biblioteca do alvo é carregada no processo da IDE.

### Fora da Fase 7

Detecção de instalação, perfis, deploy, leitura de logs e scripts próprios de
cada produto — `wsadmin`, `catalina`, CLI do WildFly — ficam para adapters
opcionais posteriores, atrás dos mesmos contratos genéricos. Nenhuma
funcionalidade essencial da IDE pode depender deles.

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
