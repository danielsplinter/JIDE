# 05 — Integração Java Inicial

## Objetivo

Fornecer suporte Java sem usar uma JVM embutida na IDE.

A IDE deverá:

- analisar Java nativamente em Rust;
- ler `.class` e `.jar`;
- detectar JDKs instalados;
- permitir selecionar o JDK usado pelo projeto;
- aceitar JDKs distribuídos junto de servidores, como o SDK do WebSphere;
- conectar-se por depuração a qualquer processo Java em execução;
- iniciar `javac`, Maven, Gradle e ferramentas Java apenas sob demanda.

## Componentes

```text
JavaLanguageProvider
├── JavaSyntaxEngine
├── JavaSemanticEngine
├── JavaClassFileReader
├── JavaSymbolIndexer
├── JavaCompletionEngine
├── JavaDiagnosticsEngine
└── JavaRefactoringEngine

JavaToolchainProvider
├── OracleJdkDetector
├── OpenJdkDetector
└── BundledSdkDetector      // JDKs distribuídos com servidores

JavaCompilerAdapter
└── JavacProcessAdapter

JavaBuildAdapters
├── MavenAdapter
└── GradleAdapter

JavaRuntimeAdapters
└── JavaProcessAdapter

JavaDebugAdapter
└── JdwpAttachAdapter       // qualquer processo Java com depuração habilitada
```

## Configuração

```toml
[languages.java]
enabled = true
provider = "native-java"

[toolchains.java]
home = "C:/IBM/WebSphere/AppServer/java/8.0"
source = 8
target = 8

[[debug.targets]]
name = "app local"
host = "127.0.0.1"
port = 8000
```

O caminho do JDK acima é apenas um exemplo de instalação que acompanha um
servidor; qualquer JDK válido serve. Nenhum servidor específico aparece na
configuração.

### Interface de configuração

O menu `Configurações` abre uma janela com uma lista de páginas no painel
esquerdo. A primeira página, `Compilador e VM`, mostra no painel direito uma
combo com versão e caminho dos JDKs detectados. Essa tela substitui visualmente
o conteúdo apenas na área de seu painel: a IDE continua visível e escurecida ao
redor, sem permitir que texto do editor, Explorer ou terminal seja composto
sobre a janela. A implementação utiliza `ModalHost`, `ComboBox`, `Button` e
`MenuBar` fornecidos pela ERLibUi; a IDE mantém apenas o estado e as regras
específicas da toolchain Java.

Ao escolher um item da combo, esse JDK se torna a toolchain ativa. O botão
`Procurar...`, ao lado da combo, abre o seletor nativo de pastas e permite
apontar para outro JDK. A pasta só é aceita quando contém `bin/java`,
`bin/javac` e `bin/jar`; uma pasta inválida produz uma mensagem dentro da
janela. `Ctrl+Shift+J` abre diretamente essa mesma página de configuração.

## JDKs distribuídos com servidores

Alguns servidores trazem o próprio JDK, e ele deve ser aceito como qualquer
outro. O WebSphere é o caso mais comum no Windows corporativo, e por isso os
locais abaixo entram na varredura — como conveniência de detecção, não como
suporte a um produto:

```text
WAS_HOME/java
WAS_HOME/java/8.0
```

A regra vale para qualquer instalação: o que define um JDK é o conteúdo da pasta,
não o produto que a instalou.

Validação mínima:

```text
JAVA_HOME/bin/java
JAVA_HOME/bin/javac
JAVA_HOME/bin/jar
```

## Compatibilidade Java 8

O suporte inicial deverá incluir:

- classes;
- interfaces;
- enums;
- annotations;
- generics;
- lambdas;
- method references;
- streams;
- imports;
- inner classes;
- anonymous classes;
- try-with-resources;
- default methods;
- static interface methods.

## Implementação sintática da Fase 3

O crate `language-java` fornece `JavaLanguageProvider` com a gramática oficial
`tree-sitter-java`. O parser é nativo e não inicia nem incorpora uma JVM.

Cada documento aberto mantém texto, versão e árvore do Tree-sitter. Uma
`DocumentChange` é aplicada ao texto e à árvore anterior por `InputEdit`; o
parser recebe a árvore editada para reutilizar as regiões inalteradas.

O snapshot produzido contém:

- árvore sintática neutra com tipos de nó, intervalos, filhos e indicação de
  erro;
- outline hierárquico de classes, interfaces, enums, annotations,
  construtores, métodos e campos;
- spans de highlighting para keywords, tipos, funções, campos, variáveis,
  strings, números, comentários, annotations e operadores;
- imports comuns, estáticos e wildcard;
- diagnósticos para nós `ERROR` e tokens ausentes produzidos pela gramática.

O provider declara `SYNTAX | DIAGNOSTICS`, é registrado no composition root e
executa dentro do worker do Language Host. O editor envia abertura e alterações
incrementais, guarda somente snapshots da mesma versão do buffer e renderiza os
spans recebidos. A barra de status informa a quantidade atual de erros,
símbolos de outline e imports.

## Implementação semântica da Fase 4

O provider passa a declarar também `SEMANTICS`, `COMPLETION`, `DEFINITION` e
`REFERENCES`. Para cada fonte são construídos:

- tabela de classes, interfaces, enums, annotations, construtores, métodos,
  campos, parâmetros e variáveis locais;
- escopos aninhados para tipos, métodos, construtores, blocos, lambdas, laços e
  cláusulas `catch`;
- tipos declarados, dimensões de array e argumentos genéricos;
- mapa de referências por identificador.

Ao ativar, o provider cria um índice limitado das fontes Java, class files e
JARs do workspace. Fontes abertas substituem os resultados estáveis do índice
na resolução. Definições no mesmo arquivo e no escopo mais profundo têm
prioridade; depois são consultadas outras fontes do workspace.

`Ctrl+Click` usa `DefinitionRequest` e abre a localização retornada, **rolando o
editor até ela**. Sem isso, um destino no mesmo arquivo mas fora da área visível
movia o cursor e mais nada: a navegação parecia não funcionar justamente para
método, constante e variável, que quase sempre estão declarados no próprio
arquivo. Tipos pareciam funcionar só porque abriam outra aba.

Com `Ctrl` pressionado, o cursor vira uma mão sobre **tudo que pode levar a uma
definição** — tipo, método, campo, variável e anotação —, e não apenas sobre
tipos. Palavra-chave, literal, comentário e operador ficam de fora: nenhum
declara nada, e uma mão sobre cada palavra do arquivo não informa coisa alguma.

O cursor precisa concordar com o clique. Enquanto só tipo acendia a mão, o clique
navegava em método, campo e variável sem que nada na tela dissesse que era
possível, e a conclusão natural era que ali não funcionava.

Isso exige que o realce classifique também os **usos**, e não apenas as
declarações. Um identificador que não é declaração é uma referência a algo
declarado em outro lugar — a constante numa comparação, a variável passada como
argumento, o contador de um laço — e recebe o mesmo papel da declaração
correspondente. Ficam sem papel apenas os fragmentos de nome qualificado, o `org`
e o `springframework` de um import, que não nomeiam nada que se possa abrir.

A linha de destino fica **destacada** enquanto o cursor continuar nela, com a
mesma decoração que marca a linha em que a execução parou. O destaque não é
apagado por ninguém: ele vale enquanto o cursor estiver onde a navegação o pôs, e
o primeiro clique ou tecla o encerra sozinho. Assim nenhum caminho novo precisa
lembrar de limpá-lo. O índice
cobre **toda forma de declarar um tipo ou membro**, e não apenas classe: `record`,
`interface`, `enum`, constante de enumeração, tipo de anotação e seus elementos,
construtor — inclusive o compacto de um registro —, método, campo, parâmetro e
variável local.

`record` faltava, e a falha era silenciosa do jeito pior: navegar até uma classe
funcionava, até um registro não encontrava nada. Como registro é a forma comum de
declarar um DTO em Java moderno, o efeito prático era a navegação parecer quebrada
justamente nos tipos do próprio projeto.

`Ctrl+Space` solicita autocomplete, mostra até oito opções visíveis, permite
navegar com as setas, confirmar com Enter e cancelar com Escape.

## Implementação da toolchain da Fase 5

O crate `java-toolchain` detecta instalações pelos seguintes locais:

- `JAVA_HOME` e `JDK_HOME`;
- `.jdk` e `jdk` dentro do workspace;
- `WAS_HOME/java`;
- executável `java` encontrado no `PATH`;
- diretórios usuais de JDK no Windows, macOS e Linux.

Uma instalação só é aceita quando possui `bin/java`, `bin/javac` e `bin/jar`.
A versão é obtida com `java -version`. A primeira instalação válida é
selecionada automaticamente e `Ctrl+Shift+J` abre uma janela modal para escolher
explicitamente entre as instalações detectadas. A janela lista versão e caminho de
cada JDK, destaca a seleção atual e permite escolher por mouse ou pelas setas,
confirmar com Enter ou `Selecionar` e cancelar com Escape ou `Cancelar`. A barra
de status mostra a seleção confirmada.

O `ClasspathBuilder` elimina duplicatas e inclui o diretório de saída
`.er-ide/classes`, `target/classes`, diretórios `lib` e `libs`, JARs diretamente
contidos neles e saídas usuais do Gradle. Os caminhos são unidos com o separador
correto da plataforma.

O crate `java-javac-adapter` implementa os contratos de compilação, execução e
testes. `Ctrl+B` compila as fontes Java do workspace com `javac`, UTF-8,
`-source 8`, `-target 8` e o classpath calculado. `F5` compila e depois executa
com `java` a classe do arquivo Java ativo, incluindo seu package.
`Ctrl+Shift+T` compila e executa o arquivo Java ativo como classe de teste.
Cada processo recebe `JAVA_HOME` correspondente ao JDK selecionado.

Compilação e execução acontecem fora da thread da interface. Código de saída,
`stdout` e `stderr` retornam como dados tipados; a aplicação acrescenta a saída
ao terminal ativo e atualiza a barra de status. O adapter de testes sempre
compila antes de executar cada classe de teste solicitada.

## Class files e JARs

O crate `java-classfile` valida o magic number, lê versão, constant pool,
hierarquia, campos e métodos sem carregar bytecode em uma JVM. Entradas
desconhecidas ou truncadas retornam erro tipado.

JARs são lidos como ZIP e somente entradas `.class` de até 16 MiB são
consideradas. A ativação limita a varredura a 500 fontes, 64 JARs e 20.000
classes por JAR. Classes externas indexadas participam do autocomplete.

## Bibliotecas padrão

Para Java 8:

```text
JAVA_HOME/jre/lib/rt.jar
JAVA_HOME/jre/lib/*.jar
```

O analisador deve indexar APIs públicas e metadados necessários.

## Bibliotecas fornecidas pelo servidor

O classpath do projeto poderá incluir bibliotecas do servidor usado em produção,
qualquer que seja ele — `lib` do Tomcat, módulos do WildFly, `plugins`, `dev` e
`lib` do WebSphere. Esses diretórios são configuração do usuário, não
conhecimento embutido na IDE.

Não indexar indiscriminadamente todos os JARs. O adapter de projeto deve determinar quais bibliotecas realmente pertencem ao classpath.

## Maven

Estratégia:

1. interpretar o `pom.xml` básico nativamente;
2. executar Maven externo para obter o modelo efetivo quando necessário;
3. configurar `JAVA_HOME` com o JDK selecionado;
4. importar dependências e módulos;
5. acompanhar mudanças no POM.

## Gradle

Gradle deve ser tratado como ferramenta externa.

A IDE não deve tentar interpretar toda lógica Groovy ou Kotlin.

## Implementação de Maven e Gradle da Fase 6

O modelo de projeto é neutro e vive em `ide_project::model`: módulos,
coordenadas, escopos de dependência, raízes de código — inclusive geradas — e
diretórios de saída. `ide_project::build` define `BuildSystemAdapter` e o
registro que escolhe o primeiro adapter capaz de reconhecer a raiz do
workspace. Nenhum dos módulos conhece Java, Maven ou Gradle.

### Maven

`java-maven-adapter` detecta o projeto pelo `pom.xml` da raiz e interpreta o
modelo nativamente, sem iniciar processo algum:

- coordenadas próprias ou herdadas do `<parent>`;
- propriedades do POM e do pai, com interpolação `${...}` e as implícitas
  `project.groupId`, `project.artifactId` e `project.version`;
- `<modules>`, percorridos recursivamente com limites de profundidade e
  quantidade; módulos declarados que não existem no disco são ignorados;
- `<dependencyManagement>` do POM e da cadeia de pais, usado para completar
  dependências declaradas sem versão;
- dependências com escopo, `optional` e `systemPath`;
- `<build>`: `sourceDirectory`, `testSourceDirectory`, `directory`,
  `outputDirectory` e `testOutputDirectory`.

Cada artefato é procurado no repositório local, no layout
`grupo/artefato/versão/artefato-versão.jar`. O repositório vem de `M2_REPO` ou
de `~/.m2/repository`. Artefatos ausentes não impedem a importação: a
dependência entra no modelo sem caminho resolvido.

Perfis ativos, heranças fora do workspace e plugins que alteram o build ficam
fora da interpretação nativa e continuam acessíveis executando o Maven externo.

### Gradle

`java-gradle-adapter` detecta o projeto por `settings.gradle(.kts)` ou
`build.gradle(.kts)`. A IDE não interpreta Groovy nem Kotlin: são extraídas
apenas as declarações literais que qualquer script expõe — `rootProject.name`,
`include` e dependências cuja coordenada é uma string. Declarações calculadas em
tempo de execução são ignoradas de propósito. Os artefatos são procurados no
cache de módulos do Gradle, cujo diretório de versão contém um nível de hash.

### Código gerado

`target/generated-sources`, `target/generated-test-sources` e
`build/generated/**/{main,test}` entram no modelo como raízes de código. Elas
participam da compilação e da análise como qualquer outra fonte, mas não são
escritas pelo usuário.

### Build e integração

O menu `Projeto` oferece `Compilar projeto` e `Reimportar projeto`;
`Ctrl+Shift+B` executa o build do sistema detectado — `compile` no Maven e
`classes` no Gradle. O wrapper versionado no projeto tem prioridade sobre o
executável do `PATH` e sobre `MAVEN_HOME`/`GRADLE_HOME`, e todo processo recebe
o `JAVA_HOME` do JDK selecionado na Fase 5. A execução acontece fora da thread
da interface e a saída tipada vai para o terminal ativo.

O classpath de `Ctrl+B`, `F5` e `Ctrl+Shift+T` passa a incluir as saídas dos
módulos e os artefatos das dependências importadas, e a compilação considera
apenas as fontes sob as raízes do projeto. Sem projeto importado, permanece a
varredura completa do workspace da Fase 5.

Alterações do manifesto feitas fora da IDE são percebidas e disparam
reimportação. Uma importação que falha — por exemplo, um POM salvo pela metade —
preserva o último modelo válido e informa o erro na barra de status.

## Servidores e containers Java

A IDE não é uma ferramenta de um servidor específico. Tomcat, Jetty, WildFly,
JBoss EAP, WebSphere, Liberty, Quarkus, Spring Boot e qualquer outro processo
Java — inclusive ferramentas como Flyway ou um job em lote — devem ser
igualmente suportados, sem que nenhum deles apareça no núcleo, nos contratos ou
no modelo de projeto.

### Depuração como integração primária

A integração acontece **exclusivamente pela porta de depuração**. A IDE não
para, não publica artefato e não instala nada; o ciclo de vida do que está
rodando continua com o usuário.

A única exceção é iniciar: para o projeto aberto, a IDE pode subir a aplicação
já com o agente ligado, porque isso é executar o código do próprio usuário, e
não administrar um servidor. Ela sobe pelo terminal integrado, com o comando
visível, e nunca sobe nada quando já existe algo escutando no alvo.

O requisito é apenas que o processo tenha sido iniciado com depuração
habilitada, o que na JVM significa um agente JDWP escutando em uma porta:

```text
-agentlib:jdwp=transport=dt_socket,server=y,suspend=n,address=*:8000
```

A partir daí a IDE:

- conecta-se ao host e à porta informados;
- registra os breakpoints dos arquivos abertos;
- recebe a parada quando a execução atinge um breakpoint;
- posiciona o editor na linha correspondente;
- executa o código linha a linha, entrando, passando por cima ou saindo do
  método;
- apresenta pilha de chamadas, quadros, variáveis locais e campos visíveis;
- avalia expressões no contexto do quadro selecionado;
- retoma a execução ou desconecta sem afetar o processo depurado.

Isso vale identicamente para um servidor de aplicação, um container, um serviço
em contêiner Docker com a porta exposta ou uma máquina remota — a diferença é só
o host e a porta.

### Configuração

Alvos de depuração são configurados de forma neutra, sem tipo de servidor:

```toml
[[debug.targets]]
name = "app local"
host = "127.0.0.1"
port = 8000

[[debug.targets]]
name = "homologação"
host = "10.0.0.20"
port = 8787
```

O mapeamento entre as posições recebidas do alvo e os arquivos do workspace usa
as raízes de código do projeto importado na Fase 6, incluindo o código gerado.

### Implementação da Fase 7

`java-debug-adapter` é o único crate que conhece o protocolo. Ele cumprimenta o
alvo, negocia as larguras de identificador declaradas por ele e mantém um leitor
dedicado: respostas voltam a quem as pediu e eventos são entregues à sessão.
Respostas truncadas viram erro tipado, e perder a conexão falha o que estava
pendente em vez de deixar a interface esperando.

Para instalar um breakpoint, o arquivo é convertido no nome qualificado da
classe pela raiz de código que o contém, e o adapter observa também as classes
internas e anônimas do mesmo arquivo. As classes já carregadas recebem o
breakpoint na hora; as demais são instaladas quando o alvo as carrega. Se a
linha pedida não tem código executável — linha em branco, comentário, chave — o
breakpoint desce para a próxima linha executável e o usuário é avisado da linha
efetiva.

Ao parar, o adapter traduz a posição recebida em `Location` do domínio usando as
mesmas raízes de código do projeto importado na Fase 6, inclusive as geradas. A
pilha traz classe, método e linha de cada quadro; as variáveis vêm da tabela de
variáveis locais válida no ponto de execução, mais o `this` quando existe.

A inspeção aceita `this`, variáveis locais e cadeias de campos. Chamadas de
método e operadores são recusados de propósito: executar código no alvo altera o
estado do programa depurado, e isso precisa ser uma decisão explícita do
usuário.

### Botões de parar, executar e depurar

O canto direito da barra de menus tem três botões da ERLibUi — `Button::icon`
com `Icon::Stop`, `Icon::Play` e `Icon::Bug`. A IDE define papel, posição e
comando; o desenho do ícone, a cor pelo tema e o nome acessível são da
biblioteca.

O **play** sobe a aplicação do projeto no terminal integrado, sem depuração. A
mesma ação está em `Projeto → Executar aplicação`.

O **stop** interrompe a aplicação iniciada pela IDE enviando ao terminal a mesma
interrupção de um `Ctrl+C`, na aba em que ela foi iniciada. A IDE não encerra
processo por fora: quem decide o que fazer com o sinal é o programa, e um Spring
Boot faz seu desligamento gracioso normalmente. Uma sessão de depuração aberta é
desconectada antes. O ícone fica apagado enquanto não há aplicação iniciada pela
IDE, e a ação também está em `Projeto → Parar aplicação`.

### Parar e executar em seguida

Ferramentas de build no Windows são arquivos de lote — `mvn.cmd`, `gradlew.bat`.
Interromper um arquivo de lote faz o `cmd` perguntar se deve finalizá-lo e ficar
esperando a resposta, o que deixa o terminal travado; sem tratar isso, o próximo
comando viraria a resposta da pergunta em vez de executar.

O terminal resolve os dois lados:

- a pergunta é respondida com uma segunda interrupção, enviada quando ela
  aparece de fato. O momento não é previsível — a aplicação ainda encerra seus
  recursos antes, e no Spring Boot isso leva segundos —, então a espera é pela
  pergunta, não pelo relógio. Ela é reconhecida pela pontuação, um par entre
  parênteses separado por barra ao fim da linha, o que não depende do idioma do
  Windows;
- um comando pedido enquanto o terminal está ocupado é enfileirado e executado
  quando ele fica livre, em vez de se perder. Há um limite de espera, para que
  um terminal que não se resolva não engula o pedido em silêncio.

Com isso, clicar em parar e em executar em seguida reinicia a aplicação.

O **inseto** executa a ação completa com um clique, usando o alvo já
configurado:

1. se algo já escuta em host e porta, apenas conecta;
2. senão, monta o comando que sobe a aplicação com o agente de depuração,
   executa esse comando no terminal integrado e espera a porta abrir, tentando
   conectar por até dois minutos;
3. se não houver comando confiável para o projeto, não inventa nenhum: informa
   na barra de status e deixa a decisão com o usuário.

Os dois botões montam o comando da mesma forma, nesta ordem: `run.command` na
configuração do usuário, onde `{agent}`, `{host}` e `{port}` são substituídos —
`{agent}` desaparece na execução sem depuração —; ou a dedução a partir do
projeto importado. A dedução cobre o que a IDE consegue afirmar com segurança —
hoje, Maven com o plugin do Spring Boot, usando o wrapper quando existir. Para os
demais casos vale a configuração explícita, porque só o usuário sabe como sua
aplicação sobe.

### Executar não é testar

Executar a aplicação não compila as fontes de teste. O `spring-boot:run` encadeia
a fase `test-compile`, então um teste que não compila — por uma dependência que
mudou de módulo, por exemplo — impediria de subir a aplicação, o que não é o que
se espera de um botão de executar. Por isso o comando deduzido desliga a
compilação de testes.

Rodar os testes continua sendo uma ação própria, com `Ctrl+Shift+T`.

### Argumentos no terminal

O comando é executado no terminal integrado, então precisa ser válido para o
shell da aba. No Windows isso tem uma consequência concreta: o PowerShell parte
um argumento `-Dchave.com.pontos=valor` no primeiro ponto e a ferramenta receberia
dois argumentos sem sentido. Todo argumento `-D` produzido pela IDE vai entre
aspas.

O ícone fica com a cor de destaque enquanto há sessão conectada.

### Configuração

### Estado visível do breakpoint

A calha tem fundo próprio e uma borda que a separa do código, porque é ela — e
só ela — que alterna breakpoints ao clique; sem contraste não há como saber onde
clicar. `F9` alterna o breakpoint da linha do cursor.

O marcador mostra o estado real:

- **contorno** — a linha está marcada, mas o alvo ainda não confirmou: não há
  sessão, ou a classe ainda não foi carregada;
- **cheio** — o alvo instalou o breakpoint e a execução vai parar ali.

A barra de status acompanha, informando quantos breakpoints estão ativos e
quantos aguardam a classe carregar.

Breakpoints marcados antes de existir sessão não se perdem: eles são guardados e
registrados quando a conexão acontece. Marcar antes de conectar é o fluxo normal,
já que a aplicação leva tempo para subir.

### Configuração

Na interface, a página `Depuração` das configurações pede host e porta, que são
gravados na configuração do usuário e reaproveitados pelo botão nas próximas
execuções. O clique na calha e `F9` alternam breakpoints; `F8`, `F10`, `F11` e `Shift+F11` controlam
a execução. Um painel à direita do editor mostra estado, pilha e variáveis, e
escolher um quadro navega até sua linha. A sessão vive em thread própria: nem a
parada, nem o passo, nem a queda da conexão bloqueiam a janela.

Pilha e variáveis são `ListView` da ERLibUi, com a altura de linha reduzida que a
lista oferece. A IDE não desenha linha, seleção nem recorte, e não decide qual
linha foi clicada: entrega o ponteiro à lista e reage à escolha dela — clique
fora das linhas não é escolha de quadro nenhum. Em troca, a pilha ganhou rolagem,
foco e nó de acessibilidade que o desenho manual não tinha.

### Adapters específicos de servidor

Detecção de instalação, perfis, deploy, leitura de logs e scripts próprios —
`wsadmin` no WebSphere, `catalina` no Tomcat, CLI do WildFly — são convenientes,
mas **não** fazem parte do caminho principal. Quando existirem, serão adapters
opcionais, ativados sob demanda, atrás dos mesmos contratos genéricos, e nenhuma
funcionalidade essencial pode depender deles.

## Segurança

- nunca compartilhar memória com a JVM do alvo depurado;
- nunca carregar bibliotecas do servidor no processo principal;
- a sessão de depuração roda em worker isolado e a queda da conexão não derruba
  a IDE;
- executar comandos por adapter;
- validar caminhos;
- escapar argumentos;
- tratar host, porta e credenciais de depuração como configuração do usuário,
  nunca embutidos no produto;
- registrar comandos executados sem expor segredos.
