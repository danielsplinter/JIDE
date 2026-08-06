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

## Dependência da interface

**Tudo o que aparece na tela é desenhado por um componente da ERLibUi.** A IDE
compõe e dá significado; o traço é sempre da biblioteca, e o que falta nela é
pedido a ela. Um teste de arquitetura falha se a IDE desenhar uma primitiva
sequer — ver `01-product-vision` e a ADR-022.

A IDE usa a ERLibUi `0.1.0` como repositório irmão, sem caminhos absolutos:

```text
projetos/rust/
  ERLibUi/
  ide/
```

As dependências ficam centralizadas em `workspace.dependencies`; portanto mover
essa pasta conjunta para outro usuário ou unidade não exige editar manifests.

## Executar

```text
cargo run -p ide-app
```

## Gerar o executável

```text
cargo build --release -p ide-app
```

```text
cargo build --release -p ide-app --features producao
```

O binário sai em `target/release/ide-app.exe`. A tela de abertura está embutida
nele, então ele não depende de nenhum arquivo ao lado.

Os dois comandos diferem em uma coisa: **com `producao`, o Windows não anexa um
console ao processo**. Sem a feature — o padrão — o console fica, e é nele que o
`tracing` aparece; é o binário de quem está caçando defeito.

É uma feature, e não um perfil, porque perfil do Cargo não chega ao código: não
existe `cfg(profile = "release")`. Amarrar a decisão a `debug_assertions` faria o
console sumir de **todo** release, e é justamente num release que se precisa do
log quando algo só acontece lá.

**No binário de produção o log não tem para onde ir**: `stdout` e `stderr` ficam
sem destino, e o `tracing` segue escrevendo no vazio. Enquanto o `init_logging`
não souber gravar em arquivo, um defeito relatado sobre esse binário chega sem
rastro nenhum.

Na primeira execução, o Explorer carrega o diretório no qual a IDE foi iniciada.
Depois disso vale o último projeto aberto por `Arquivo → Projeto...`, gravado em
`%APPDATA%\er-ide\config.toml` no Windows e em `~/.config/er-ide/config.toml`
nas demais plataformas — `ER_IDE_CONFIG` aponta para outro arquivo quando
necessário. Se a pasta registrada não existir mais, a IDE abre o diretório atual
sem reclamar.

Clique em diretórios
para expandir ou recolher e em arquivos de texto para abri-los. Clique nas abas
para alternar documentos. Clique no editor para posicionar o cursor e digite
normalmente; `Backspace`, `Enter` e as quatro setas estão disponíveis — as
verticais movem entre linhas preservando a coluna, e param no fim de uma linha
mais curta em vez de num ponto que não existe.

Ao iniciar a IDE ou trocar de aba, o Explorer expande automaticamente todas as
pastas que levam ao arquivo ativo, seleciona esse arquivo na árvore e ajusta a
rolagem para mantê-lo visível.

`Enter` abre a linha nova já com a **indentação da linha anterior**, de modo que
escrever dentro de um bloco não exige reindentar a cada linha. A regra é do editor
do ERLibUi, e vale igual no editor da janela de inspeção do depurador.

Arraste para selecionar um trecho, dê **duplo clique** para selecionar a palavra
sob o ponteiro, ou use as setas com `Shift`; mover sem `Shift` desfaz a seleção.
As setas laterais com `Ctrl` **saltam de palavra em palavra** em vez de andar um
caractere, e com `Ctrl+Shift` o salto marca o caminho. O que é palavra é a mesma
regra do duplo clique, que vem do editor do ERLibUi.

Em arquivo com fim de linha CRLF, o cursor não para **entre o retorno e a quebra**:
o fim de linha é um lugar só, que as setas atravessam inteiro e onde o clique para
no fim do que se vê. Sem isso, digitar no fim de uma linha terminada em espaço
escrevia depois do fim de linha, e o texto recém digitado aparecia repetido sobre a
linha de baixo — o retorno no meio do trecho faz o shaper abrir outra linha.
**`Shift+clique`** marca do cursor até o ponto clicado, e outro `Shift+clique`
alarga a partir da mesma âncora — é o gesto de estender, não o de recomeçar.

Se o arrasto passar da borda da área, **a vista acompanha** nas quatro direções,
e continua acompanhando **com o ponteiro parado** — basta segurar o botão além da
borda para a seleção seguir crescendo. Quanto mais longe da borda, mais rápido,
até um teto: o ponteiro no canto da tela acelera, mas não varre o arquivo de uma
vez. Sem isso a seleção pararia no que está à mostra, e marcar além da tela
exigiria soltar, rolar e recomeçar. Soltar o botão encerra o gesto — e perder o
foco da janela no meio dele também, para a vista não continuar andando sozinha.
Digitar ou apagar com um trecho marcado age sobre ele. `Ctrl+C` copia e `Ctrl+V`
cola, usando a área de transferência **do sistema** — o mesmo que o clique com o
botão direito sobre o editor oferece em Copiar e Colar. Sem seleção, Copiar
aparece desabilitado em vez de sumir do menu.

Com o cursor **dentro de uma classe, interface, enum ou anotação**, o mesmo menu
traz `Generate`, que abre um submenu com `Constructor`, `Getter`, `Setter` e
`Getter and Setter`. Fora de um tipo a opção não aparece: não haveria onde
escrever.

`Getter` e `Setter` abrem uma janela com **os campos que ainda não têm aquele
acessor** — o que já existe não é oferecido, porque escolhê-lo não escreveria
nada. Cada linha traz uma caixa de marcação. **OK** gera os marcados; **All** gera
todos, marcados ou não, para quando se quer a classe inteira sem marcar campo por
campo. `Esc` desiste.

Os métodos entram **na posição do cursor**, no começo da linha em que ele está —
a linha atual desce. Inserir no meio dela partiria um token. O ponto fica preso ao
corpo do tipo: com o cursor na linha da declaração da classe, ou depois da chave
que a fecha, o método sairia fora dela e nem compilaria.

Quem sabe Java é o provider: a convenção de nome — `getNome`, mas `isAtivo` para
`boolean` —, o tipo de retorno e a linha de inserção vêm dele prontos. A tela
mostra nomes e insere trechos, sem saber o que é um getter.

`Setter` e `Getter and Setter` usam **a mesma janela**: o que muda é o que a
linguagem escreve, não a tela. No par, um campo entra na lista quando falta
**algum** dos dois, e o texto gerado traz só o que falta — repetir o que a classe
já tem seria erro de compilação, não conveniência.

`Constructor` usa **a mesma janela**, com uma diferença: ela lista **todos** os
campos, porque a escolha não é sobre o que falta, e sim sobre quais entram por
parâmetro. Os campos marcados viram parâmetros e atribuições, na ordem em que
foram **declarados** — é assim que se lê a classe depois. **All** gera o construtor
com todos os campos. E **OK sem marcar nada gera um construtor sem parâmetros**:
não marcar é um pedido legítimo, e não a ausência de um.

Um construtor com a mesma lista de tipos de um que a classe já tem não é gerado —
a assinatura repetida não compilaria, e a barra de estado diz que ele já existe.
Como nos acessores, quem monta o texto é o provider: a tela decide **quais campos**
e entrega a lista.

Clicando com o botão direito **sobre um arquivo na árvore**, o menu oferece
`Renomear` — qualquer arquivo, não só código. Sobre uma pasta a opção não aparece:
pasta não tem tipo dentro nem referências por nome.

A janela mostra um campo com o nome atual e, abaixo, **todos os arquivos que serão
reescritos**, com a pasta e quantas ocorrências cada um tem — é o alcance da
mudança, visível antes de confirmar. `Enter` ou **OK** confirmam; `Esc` ou
**Cancelar** desistem.

Confirmando, três coisas acontecem juntas: o **arquivo** é renomeado, o **tipo**
dentro dele também — a declaração e os construtores, que sem isso ficariam com o
nome antigo e não compilariam — e **todas as referências espalhadas pelo projeto**
são reescritas, inclusive em arquivos fechados.

A gravação é **tudo ou nada**: os conteúdos novos são calculados antes de qualquer
escrita, e se uma falhar as já gravadas voltam ao que eram. Meio caminho seria um
projeto que não compila e ninguém sabendo onde parou. Um nome já ocupado por outro
arquivo faz a operação parar com aviso, em vez de sobrescrevê-lo.

Arquivos **abertos** são reescritos no editor, não no disco: a aba mantém cursor,
desfazer e alterações não salvas, e você grava como de costume. Os fechados são
gravados direto.

`Tab` indenta até a próxima parada de quatro colunas e `Shift+Tab` recolhe a
indentação da linha. **Com um trecho marcado, os dois deslocam todas as linhas do
bloco** e mantêm a seleção, para indentar vários níveis sem remarcar; recolher
para quando alguma linha já está na margem, preservando a relação entre elas. A
indentação usa espaços, porque o editor mede o texto por coluna de largura fixa e
um `\t` ocuparia uma coluna no texto e várias na tela.

`Ctrl+Z` desfaz, voltando **até dez ações** — cada alteração conta como uma, e a
mais antiga sai quando a décima primeira entra. O teto existe porque cada passo
guarda o texto inteiro.

`Ctrl+D` marca a **próxima ocorrência do trecho selecionado**, e repetir vai
somando ocorrências; a busca dá a volta no arquivo, então nada fica de fora por
estar acima do cursor. **Todas as ocorrências marcadas ficam destacadas**, numa
cor própria, diferente da seleção: elas não são seleção, e copiar ou `Tab` não
agem sobre elas. Com as marcas
de pé, **cada marca é um cursor**: o texto que já estava lá permanece, e cada
caractere digitado ou apagado é replicado em todas as ocorrências, letra a letra —
dá para ver a alteração acontecer nas várias posições antes de confirmar. Marcar
não substitui a palavra: mudar uma letra é mudar uma letra, e `Backspace` tira uma
de cada.

A alteração acontece **onde o cursor está** dentro do trecho, e não no fim dele:
quem põe o cursor no meio da palavra quer alterar ali. As setas movem esse ponto
em todas as ocorrências ao mesmo tempo e param nas bordas da marca — sair dela
acabaria com a correspondência entre os pontos. `Enter` confirma e solta as marcas; `Esc` desiste e
devolve o texto ao que era antes da primeira marca, de uma vez, sem gastar os
passos do desfazer.

`Ctrl+L` abre a **busca de tipo por nome**: uma janela larga com um campo em cima
e, abaixo, a lista do que foi encontrado — classes, interfaces, records e enums do
projeto. Cada linha traz o nome, o que ele é e o caminho **relativo à última
pasta `java`** da raiz de fontes — por exemplo, `br\com\exemplo\Pedido.java` —,
sem exibir o caminho absoluto da máquina. Dois tipos de mesmo nome em pacotes
diferentes são justamente o caso que a busca precisa desempatar. A janela nasce
mostrando o que existe, e cada letra digitada refina; quem começa com o que foi
digitado vem antes de quem só contém.

As setas andam na lista, `Enter` ou um clique abrem o tipo escolhido numa aba do
editor, posicionando o cursor na declaração, e `Esc` dispensa sem abrir nada. Só
entram tipos com arquivo no projeto: o resultado existe para ser aberto, e uma
classe dentro de um jar não teria onde abrir.

`Ctrl+Shift+L` abre **a mesma janela no modo de busca por conteúdo**. A consulta
percorre somente as `source_roots` informadas pelo modelo Maven ou Gradle,
inclusive quando o projeto possui vários módulos ou raízes `src/main/java` e
`src/test/java`; arquivos fora dessas raízes não aparecem. Antes de existir um
modelo importado, a IDE usa diretórios chamados `java` como fallback. A janela nasce
aguardando texto para não transformar cada linha vazia do projeto em resultado,
e a comparação ignora diferenças entre maiúsculas e minúsculas.

Cada ocorrência mostra o caminho relativo à última pasta `java`, o número da
linha e um trecho do conteúdo. Setas, roda, clique, `Enter` e `Esc` têm exatamente
o mesmo comportamento da busca de tipos; abrir uma ocorrência posiciona o cursor
na linha e na coluna encontradas.

`Ctrl+S` grava a aba ativa, o mesmo que `Arquivo → Salvar`.
Pressione `F3` para abrir a busca e `Esc` para fechá-la.

No Explorer, as cadeias de pacote Java aparecem comprimidas numa linha só —
`br.com.exemplo.endpoints` em vez de quatro níveis —, como no IntelliJ. Os
diretórios intermediários existem porque o Java exige que o diretório espelhe o
pacote, e não carregam informação nenhuma.

O **clique com o botão direito** sobre um item do Explorer abre um menu com o que
faz sentido naquele alvo. Dentro de uma raiz de fontes Java as opções são novo
pacote, nova classe e nova interface; fora dela, nova pasta. As três ações Java
abrem a mesma janela, com dois campos: o pacote — já preenchido com o do item
clicado — e o nome. `Enter` cria o pacote e, quando há nome, o tipo dentro dele,
com a declaração `package` deduzida do caminho. `Tab` alterna os campos e `Esc`
fecha sem criar.

Linhas de código não quebram. Quando alguma passa da largura visível, aparece uma
**barra de rolagem horizontal** rente à borda de baixo do editor; ela some quando
não há o que rolar, para não disputar aquele ponto com a borda do terminal.
Digitar no fim de uma linha comprida traz o cursor de volta à vista sozinho — mas
só quando o cursor se move: rolar com a barra não o move, e por isso a vista fica
onde foi posta em vez de saltar de volta no quadro seguinte.

A rolagem vertical do editor é **contínua**: o deslocamento é em pixels e aceita
fração de linha, então um touchpad ou um mouse de precisão desliza o texto em vez
de saltar linha a linha. A roda comum continua andando três linhas por passo.

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

Digitar `.` depois de uma variável, parâmetro, campo ou nome de tipo abre a lista
com os **membros públicos** daquele tipo. Cada letra digitada em seguida refaz o
filtro, então a lista acompanha o nome sendo escrito; apagar a alarga de volta, e
um caractere que não faz parte de um nome — um parêntese, um espaço — a fecha,
assim como `Esc` e um clique em qualquer lugar fora dela. Clicar **dentro** escolhe
a linha, como `Enter`. As setas navegam e `Enter` ou `Tab` aceitam. `Ctrl+Space` abre a
lista a qualquer momento, mas não é mais preciso repetir a cada letra. O tipo vem da declaração no arquivo
aberto, e os membros vêm de três origens somadas: o próprio arquivo, que responde
pelo tipo ainda não compilado; os **demais fontes do projeto**, porque uma classe
por arquivo é a regra em Java e a classe usada quase nunca está no arquivo aberto;
e as classes compiladas — o JDK, as dependências em jar e o projeto depois de um
build. A cadeia de superclasses é percorrida nas compiladas, então `toString()`
aparece como qualquer membro declarado.

O fonte tem precedência sobre o `.class` do último build: ele é a classe como ela
está agora. O arquivo do tipo é lido e analisado na hora da consulta, e não
mantido analisado — guardar todos os fontes do projeto custaria memória
proporcional ao projeto para responder sobre um tipo de cada vez.

Membros `private` não aparecem, porque não se alcançam pelo ponto. Nas classes
compiladas isso vem dos modificadores nos próprios bytes; no fonte, da árvore
sintática, já que ele não passou por compilador nenhum.

Quais caracteres disparam a lista é a linguagem quem informa, não o editor: o
provider declara os seus, e em Java é o ponto.

A lista também aparece **no editor da janela de inspeção do depurador**. Ali não
há arquivo, então a pergunta muda de forma: em vez de uma posição num documento,
vai o nome do tipo. O tipo do receptor sai do que está parado — `m` é uma variável
de quadro, não existe em fonte nenhum —, e um nome que o depurador não conhece é
lido como nome de tipo. **Quem responde pelos membros é sempre o índice do
projeto**, de modo que uma classe que não participa do código depurado é tão
conhecida quanto as outras.

Ainda fora do alcance: encadeamento (`pedido.getCliente().`), substituição de
genéricos, `this` e `super` como receptores, e a cadeia de superclasses entre
tipos do próprio projeto.

## Toolchain e build

`Ctrl+Shift+J` abre `Configurações → Compilador e VM`. Ali há **duas escolhas**:
o JDK e, logo abaixo, o **Maven** — cada uma com sua lista e seu `Procurar...`,
que aceita apontar a pasta da instalação à mão. A lista traz o que foi encontrado
em `MAVEN_HOME`, `M2_HOME`, no `PATH` e nos diretórios convencionais, com a versão
lida do nome do jar do núcleo — sem executar o Maven só para preencher uma lista.

**Salvar grava as duas no arquivo de configuração do usuário**, o mesmo do projeto
aberto, e a próxima inicialização as carrega. Antes disso o JDK era redetectado a
cada início e a escolha se perdia: quem decidia era a ordem em que a máquina
respondia. Um caminho que deixou de existir é ignorado na leitura, e a IDE volta a
detectar em vez de recusar-se a abrir.

O Maven escolhido vale acima do `PATH` e das variáveis de ambiente, mas **abaixo
do wrapper do projeto**: `mvnw` existe para fixar a versão com que aquele projeto
compila, e a preferência do usuário não passa por cima disso sem avisar. A
janela de Configurações só aplica ao **Salvar**; `Cancelar` e `Esc` descartam o
que foi mexido e restauram o que estava valendo na abertura.

O JDK escolhido ali é o que a completação usa para conhecer a biblioteca padrão —
trocá-lo reindexa o provider, que havia indexado a instalação anterior. Enquanto
nenhuma instalação for escolhida, vale o `JAVA_HOME`.
`Ctrl+B` compila as fontes Java com `javac`, `F5` compila e executa a classe
ativa e `Ctrl+Shift+T` a executa como teste.

Ao abrir um projeto, a IDE detecta Maven ou Gradle na raiz e importa módulos,
dependências e raízes de código — inclusive as geradas — lendo apenas os
manifestos, sem iniciar processo externo. A barra de status mostra o build
system, o nome do projeto e a quantidade de módulos e dependências, e o
classpath usado por `Ctrl+B`, `F5` e `Ctrl+Shift+T` passa a incluir as saídas
dos módulos e os artefatos resolvidos.

O menu `Projeto` oferece `Compilar projeto`, `Reimportar projeto` e
`Executar aplicação`. `Ctrl+Shift+B` executa o build do sistema detectado —
`compile` no Maven, `classes` no Gradle — usando o wrapper versionado no projeto
quando existir e o `JAVA_HOME` do JDK selecionado. A saída aparece no terminal
ativo. Alterações feitas no `pom.xml` ou nos scripts do Gradle fora da IDE
disparam reimportação automática.

## Executar e depurar

O canto direito da barra de menus tem três botões:

- **quadrado de stop** — interrompe a aplicação iniciada pela IDE, com a mesma
  interrupção de um `Ctrl+C` na aba de terminal em que ela subiu; fica apagado
  quando não há nada em execução. O mesmo que `Projeto → Parar aplicação`;
- **triângulo de play** — sobe a aplicação do projeto no terminal integrado, sem
  depuração; o mesmo que `Projeto → Executar aplicação`;
- **inseto** — sobe a aplicação já com o agente de depuração e conecta o
  depurador assim que a porta abre. Se já existe algo escutando no alvo
  configurado, ele apenas conecta, sem subir uma segunda instância.

Os dois usam o mesmo comando. Hoje a IDE deduz sozinha projetos Maven com o
plugin do Spring Boot; para os demais, defina `command` na seção `[run]` do
`config.toml`, onde `{agent}` recebe o agente de depuração quando a execução é
com depuração e desaparece quando é sem:

```toml
[run]
command = "./gradlew bootRun \"-Dorg.gradle.jvmargs={agent}\""
```

A aplicação roda no terminal integrado, com o comando visível — `Ctrl+C` encerra,
como em qualquer terminal.

Também dá para conectar a um processo que você mesmo subiu, em qualquer lugar:
Tomcat, Jetty, WildFly, WebSphere, Quarkus, um contêiner com a porta exposta ou
uma ferramenta em lote. Basta que ele tenha o agente escutando:

```text
-agentlib:jdwp=transport=dt_socket,server=y,suspend=n,address=*:8000
```

Em `Depurar → Conectar...` informe host e porta e conecte; o alvo fica gravado e
passa a ser o que o botão usa. Para marcar um breakpoint, clique na **calha** — a faixa escura à esquerda do
código, onde ficam os números de linha — ou ponha o cursor na linha e pressione
`F9`. O marcador aparece como contorno enquanto o alvo não confirmou, e fica
cheio quando o breakpoint está instalado e vai parar a execução; a barra de
status informa quantos estão ativos. Quando a execução parar, o
arquivo é aberto, a linha fica destacada e o painel à direita mostra a pilha de
chamadas e as variáveis do quadro selecionado; clicar em um quadro navega até
sua linha.

`F8` continua, `F10` passa sobre a linha, `F11` entra no método e `Shift+F11`
sai dele. `Depurar → Desconectar` encerra a sessão e o processo segue rodando
normalmente.

Com a sessão de pé, selecionar uma variável no editor e clicar com o **botão
direito** oferece **Inspecionar**, que avalia o trecho marcado no quadro
selecionado e abre uma janela com dois painéis: à esquerda, uma **árvore** com o
valor pedido e os campos dentro dele; à direita, o nome, o tipo e o valor completo
do nó destacado.

O painel direito tem, abaixo do detalhe, um **editor de código**: escreva uma ou
mais instruções e clique em **Executar** — ou `Ctrl+Enter` — para avaliá-las no
quadro atual.

O `;` e a quebra de linha separam instruções, e elas vão ao alvo **uma de cada
vez, na ordem escrita**: cada uma roda dentro do processo depurado e muda o que a
seguinte vai encontrar. A primeira que falhar interrompe as demais, e a mensagem
diz em qual parou — as seguintes esperavam um estado que não chegou a existir. O
que rodou antes da falha teve efeito, e a árvore é relida para mostrá-lo. Um `;`
dentro de aspas não separa nada.

**A árvore continua sendo o objeto inspecionado**: o que ela mostra são os valores
relidos ao fim da execução, com os níveis abertos ainda abertos. O retorno aparece
numa linha própria acima dos botões, dentro da janela — ela cobre a barra de
estado.

É a diferença entre olhar o resultado e olhar o efeito. `pedido.pagar()` devolve
`void`; trocar a árvore por isso apagaria justamente o objeto que se queria ver
mudar.

O botão fica apagado quando não há sessão de depuração de pé. A árvore continua
mostrando o que foi lido enquanto a execução estava parada, e sem esse aviso o
clique pareceria não fazer nada.

Duas coisas podem ser escritas ali. Um **caminho** — `pedido.cliente.nome` — é
lido, sem executar nada no alvo. Uma **chamada de método** — `m.setId(4L)` — roda
de verdade dentro do processo depurado, na thread que está parada e só nela: se o
método altera o objeto, a alteração vale a partir dali. Exceção lançada lá dentro
volta como mensagem, em vez de virar um retorno vazio.

Os argumentos são literais, com o sufixo decidindo o tipo como em Java: `4L`,
`2.5`, `true`, `false`, `null` e texto entre aspas. O literal é **ajustado ao tipo
que o parâmetro declara** antes de sair: um número que vai para um
`java.lang.Long` é embrulhado com `Long.valueOf` dentro do alvo, e um texto vira
uma `String` criada lá. Ambos custam uma chamada a mais, feita na mesma thread
parada.

Esse ajuste não é conforto, é o que mantém o processo de pé: o alvo **não confere
o tipo do argumento** antes de repassá-lo à chamada, então um `long` enviado onde
se espera uma referência seria lido como endereço de objeto e derrubaria a
aplicação depurada. Por isso o que não couber é recusado aqui, com o motivo na
tela, em vez de enviado.

O método é escolhido pelo nome, subindo a hierarquia. Entre sobrecargas de mesma
quantidade de argumentos vence a que exige menos conversão — `4L` prefere
`setId(long)` a `setId(Long)` —, e uma cujos tipos os literais não alcançam é
descartada em vez de chamada.

Depois de uma chamada, a árvore e as variáveis continuam legíveis. Executar um
método invalida os identificadores de quadro da thread no protocolo, e a sessão
refaz a correspondência pela profundidade da pilha — sem isso, a primeira leitura
após um `Executar` falharia dizendo que o quadro não existe.

Esse editor é o mesmo painel da janela principal, e edita do mesmo jeito: arraste
e duplo clique selecionam, `Shift` com as setas marca, `Tab` desloca o bloco, o
ponto abre a lista de membros, e `Ctrl+C`/`Ctrl+V` usam a área de transferência do
sistema — com a janela aberta,
copiar e colar agem no que se vê, não no documento atrás dela.

O que está desligado por configuração são os comportamentos que não fazem sentido
ali: não há arquivo para salvar, definição para navegar nem linha onde parar a
execução. O painel é um componente
com área própria, então qualquer tela pode abri-lo escolhendo o que ligar.

A árvore abre já com o primeiro nível à mostra. Clicar em um campo que também é
objeto revela os campos dele, um nível por vez — pedir tudo de uma vez percorreria
o grafo inteiro, que pode ser fundo e cíclico. Clicar de novo fecha. `Esc` ou
`Fechar` dispensam a janela, e um valor simples aparece sozinho, sem triângulo,
porque não há o que abrir.

O item só aparece durante a depuração — fora dela não há quadro que dê valor ao
nome — e fica desabilitado sem seleção. O quadro é o que estiver escolhido no
painel: o mesmo nome vale coisas diferentes em quadros diferentes.

## Verificação

```text
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

### Medindo o custo por tecla e por quadro

Com `ERIDE_PERF=1` a janela imprime, para cada evento acima de 4 ms, quanto ele
custou e onde — no tratamento do evento, no realce ou na completação — e o custo
de cada quadro separado em pintura, GPU e espera do vsync:

```text
$env:ERIDE_PERF=1; cargo run -p ide-app --release
```

Foi o que localizou a digitação travada: a tecla gastava mais de 400 ms na análise
do documento enquanto o quadro custava 5 ms. O que se descobriu ali está na
ADR-016 e na ADR-017, e a medição fica de pé para a próxima suspeita — sem a
variável, o custo é uma leitura de `OnceLock`.

A análise da linguagem **não está mais no caminho da tecla**: a mudança e o pedido
de realce são postados ao provider, que sempre teve thread própria, e o realce é
recolhido pelo relógio da janela quando fica pronto — um ou dois quadros depois. O
editor ignora realce de revisão vencida, então chegar tarde não desenha nada
errado, e a linha `tecla` do log passa a medir apenas mexer no texto.

Sobram aí **cerca de 7 ms por tecla**, e eles não são análise: é o instantâneo de
cada documento aberto, que clona o texto inteiro para que no fim só a diferença de
um deles seja enviada. Está **pendente**, com o caminho de correção anotado na
ADR-017 — montar o instantâneo apenas do documento que mudou.
