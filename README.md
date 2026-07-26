# IDE nativa em Rust

Implementação da IDE descrita em `ide-rust-spec`, usando o ERLibUi como biblioteca
de interface gráfica.

## Estado

As Fases 0 a 7 estão concluídas. A fundação estabelece os tipos de domínio,
contratos, eventos, configuração, logging e supervisão de processos. O editor
inclui shell nativo Winit/WGPU baseado no ERLibUi, buffer, abas, árvore de
arquivos, busca, comandos e terminal. O Language Host roteia providers em
workers canceláveis, e o suporte a Java cobre análise sintática e semântica,
navegação, referências, autocomplete, toolchain JDK, os build systems Maven e
Gradle e a depuração remota de processos em execução.

## Executar

```text
cargo run -p ide-app
```

O Explorer carrega o diretório no qual a IDE foi iniciada. Clique em diretórios
para expandir ou recolher e em arquivos de texto para abri-los. Clique nas abas
para alternar documentos. Clique no editor para posicionar o cursor e digite
normalmente; `Backspace`, `Enter` e as setas esquerda/direita estão disponíveis.
Pressione `F3` para abrir a busca e `Esc` para fechá-la.

Editor e terminal possuem rolagem independente. No cabeçalho do terminal,
selecione PowerShell, CMD ou Git Bash; este último aparece quando o Git for
detectado. Digite o comando e pressione `Enter` para executá-lo no diretório
do workspace. Cada aba de terminal mantém sua própria entrada, histórico,
saída e rolagem; o conteúdo das outras sessões fica oculto ao alternar.
Cada aba executa um shell interativo persistente por PTY (ConPTY no Windows).
O prompt fica no topo e a saída é acrescentada abaixo. Comandos `cd`, aliases,
variáveis e programas são interpretados pelo próprio PowerShell, CMD ou Git
Bash, portanto o estado permanece nos comandos seguintes daquela aba.

O painel do terminal pode ser minimizado e restaurado pelo botão no canto
superior direito. Sua altura é ajustável ao arrastar a borda superior; ao
restaurar, o painel recupera a última altura definida pelo usuário.

`Ctrl+Click` sobre um token Java resolve a definição pelo provider semântico e
abre a localização com `open_location`, posicionando o cursor na linha e coluna
retornadas. `Ctrl+Space` abre o autocomplete.

## Toolchain e build

`Ctrl+Shift+J` abre `Configurações → Compilador e VM` para escolher o JDK.
`Ctrl+B` compila as fontes Java com `javac`, `F5` compila e executa a classe
ativa e `Ctrl+Shift+T` a executa como teste.

Ao abrir um projeto, a IDE detecta Maven ou Gradle na raiz e importa módulos,
dependências e raízes de código — inclusive as geradas — lendo apenas os
manifestos, sem iniciar processo externo. A barra de status mostra o build
system, o nome do projeto e a quantidade de módulos e dependências, e o
classpath usado por `Ctrl+B`, `F5` e `Ctrl+Shift+T` passa a incluir as saídas
dos módulos e os artefatos resolvidos.

O menu `Projeto` oferece `Compilar projeto` e `Reimportar projeto`.
`Ctrl+Shift+B` executa o build do sistema detectado — `compile` no Maven,
`classes` no Gradle — usando o wrapper versionado no projeto quando existir e o
`JAVA_HOME` do JDK selecionado. A saída aparece no terminal ativo. Alterações
feitas no `pom.xml` ou nos scripts do Gradle fora da IDE disparam reimportação
automática.

## Depuração

A IDE se conecta a qualquer processo Java em execução com depuração habilitada —
Tomcat, Jetty, WildFly, WebSphere, Quarkus, Spring Boot, um contêiner com a
porta exposta ou uma ferramenta em lote. Ela não inicia nem para o servidor:
quem faz isso é você.

Suba o processo com o agente de depuração escutando:

```text
-agentlib:jdwp=transport=dt_socket,server=y,suspend=n,address=*:8000
```

Em `Depurar → Conectar...` informe host e porta e conecte. Clique na calha do
editor ou pressione `F9` para marcar um breakpoint. Quando a execução parar, o
arquivo é aberto, a linha fica destacada e o painel à direita mostra a pilha de
chamadas e as variáveis do quadro selecionado; clicar em um quadro navega até
sua linha.

`F8` continua, `F10` passa sobre a linha, `F11` entra no método e `Shift+F11`
sai dele. `Depurar → Desconectar` encerra a sessão e o processo segue rodando
normalmente. Inspecionar valores nunca executa código no alvo: variáveis,
`this` e campos são lidos, mas chamadas de método são recusadas.

## Verificação

```text
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```
