# 01 — Visão do Produto

## Proposta

A IDE será uma plataforma nativa em Rust para desenvolvimento em diversas linguagens.

## Navegação principal

A janela principal deve possuir uma barra de menu. O fluxo mínimo para carregar
um workspace é:

```text
Arquivo
  └─ Projeto...
       ↓
seletor nativo de pasta
       ↓
árvore do Explorer reconstruída com o conteúdo da pasta selecionada
```

Ao escolher uma pasta, ela se torna o workspace ativo. O painel esquerdo deve
mostrar recursivamente seus diretórios e arquivos, respeitando as exclusões
técnicas da IDE, como `.git` e `target`. O nome do projeto, os terminais e os
demais serviços associados ao workspace devem passar a usar a nova raiz.
Cancelar o seletor não altera o workspace atual.

O projeto escolhido é registrado na configuração do usuário e reaberto
automaticamente na próxima inicialização: quem abre a IDE pela segunda vez
espera continuar de onde parou, sem repetir a seleção de pasta. Um caminho que
não existe mais é ignorado em silêncio e a IDE abre normalmente.

As abas abertas voltam junto com o projeto, já com o conteúdo carregado e
**colorido**: quem reabre a IDE espera continuar de onde parou, e não repetir a
navegação pelo Explorer até os arquivos em que estava trabalhando. O realce é
pedido na própria inicialização, e não no primeiro evento de interface — senão o
código apareceria sem cor até o usuário clicar em algum lugar. Os detalhes do arquivo e das
regras estão em [08 — Persistência](08-storage-and-memory.md).

O menu `Configurações` abre uma janela modal com navegação em um painel
esquerdo e o conteúdo da opção selecionada no painel direito. A primeira opção
é `Compilador e VM`. A janela fica suspensa sobre a IDE: workspace, editor,
Explorer e terminais permanecem visíveis e levemente escurecidos ao redor, mas
seus textos nunca podem atravessar ou ser desenhados sobre o painel modal. O
conteúdo inferior permanece inativo até o fechamento. Barra principal, combo e
isolamento modal devem reutilizar respectivamente `MenuBar`, `ComboBox` e
`ModalHost` da ERLibUi, sem duplicar desenho ou hit testing na IDE.

O rodapé é a `StatusBar` da ERLibUi, e é dela também a altura que o restante do
layout desconta. A mensagem da última ação fica à esquerda; codificação, posição
do cursor e resumo do projeto ficam ancorados à direita, sempre no mesmo lugar.
Antes tudo era uma linha só, separada por marcadores, e cada mensagem longa
deslocava a posição do cursor para outro ponto da barra.

O Explorer é a `TreeView` da ERLibUi. Recuo, marcador de expansão, recorte,
virtualização, seleção e deslocamento horizontal pertencem ao componente; a IDE
entrega os arquivos e traduz o nó escolhido de volta para um caminho. A árvore
identifica nós por número e o Explorer os identifica por caminho — o caminho é o
que sobrevive a uma releitura do disco, então ele é a origem do número.

Todo o conteúdo da árvore deve ser recortado pelos limites do painel esquerdo.
Quando nomes ou níveis de indentação ultrapassarem a largura disponível, o
Explorer deve mostrar uma barra de rolagem horizontal com clique na trilha e
arraste do indicador. Texto da árvore nunca pode vazar sobre o editor.

Diferente das abas, a árvore não é remontada a cada quadro: remontar milhares de
nós custaria caro, e ela só muda quando o projeto ou a expansão mudam.

As quatro barras de rolagem da janela — editor, terminal e as duas do Explorer —
são a `Scrollbar` da ERLibUi. A IDE informa quanto conteúdo existe, quanto cabe e
onde está, e recebe de volta o deslocamento escolhido; o indicador, o arraste que
preserva o ponto da pegada e o clique na trilha pertencem ao componente. As
barras verticais contam linhas e a horizontal conta pontos de largura, o que para
o componente é a mesma aritmética.

O Explorer também deve possuir barra de rolagem vertical quando a árvore
ultrapassar a altura disponível. A borda direita do painel deve permitir
redimensionamento horizontal por clique, retenção e arraste. Editor e painel de
terminal ocupam sempre a largura restante e acompanham imediatamente essa
mudança de layout, sem enviar redimensionamentos ao PTY.

Na inicialização e em toda troca de aba, a `TreeView` deve acompanhar o documento
ativo: expandir todos os diretórios ancestrais, selecionar o arquivo
correspondente e ajustar a rolagem vertical para deixá-lo visível.

`Ctrl+L` abre a busca de tipos do projeto e `Ctrl+Shift+L` reutiliza o mesmo
`ModalHost`, campo e lista para buscar pelo conteúdo dos arquivos. No segundo
modo, a varredura fica estritamente limitada aos descendentes de diretórios
chamados `java`, funciona em projetos multimódulo e ignora diferenças entre
maiúsculas e minúsculas. A consulta vazia não varre o projeto.

Cada ocorrência textual apresenta caminho relativo à última pasta `java`, linha
e trecho. `Enter` ou clique abrem o arquivo na linha e coluna encontradas; setas,
roda e `Esc` mantêm o mesmo contrato da busca de tipos. O teto de resultados
impede que uma consulta ampla bloqueie a interface com uma lista sem utilidade.

Esse divisor e o que separa editor e terminal são `Splitter` da ERLibUi, com
limites em pontos — a largura mínima da barra lateral e a do editor, a altura
mínima do terminal e o espaço que o editor precisa manter. A área que aceita o
arraste é maior que a linha desenhada, e a linha se destaca quando o ponteiro se
aproxima, o que antes não acontecia: a borda parecia decoração.

Abas do editor e do painel de terminal são o `Tabs` da ERLibUi, com largura fixa
por aba. Cada aba de editor exibe um botão `x` próprio; clicar nesse botão fecha
somente o documento correspondente e ativa uma aba remanescente quando
necessário. Abas de terminal não fecham: um terminal pertence à janela enquanto
ela existir.

O título de cada aba permanece estritamente dentro de seus limites e reserva
espaço fixo para a faixa de ações. Nomes longos são abreviados com reticências e
também recortados graficamente; nunca podem invadir a aba vizinha ou cobrir seu
botão de fechamento. Um documento alterado e não gravado é sinalizado por um
ponto nessa mesma faixa, que dá lugar ao `x` quando o ponteiro está sobre a aba.

Nada disso é decidido aqui: a IDE monta as abas a partir dos documentos abertos e
traduz o comando que o componente emite. Como o widget é reconstruído a cada
quadro a partir dessa verdade, nenhuma abertura, gravação ou fechamento precisa
lembrar de sincronizar a barra de abas.

A seleção do editor é da IDE, que é dona do texto, mas a **regra do que é uma
palavra** é do componente: o duplo clique pergunta ao `CodeEditor` qual palavra
contém aquele deslocamento, em vez de a IDE reimplementar a definição e divergir
dela. A IDE guarda a seleção em bytes, o componente conta caracteres, e a
conversão fica nessa borda.

Copiar e colar falam com a área de transferência **do sistema**, pela porta
`ClipboardService` da ERLibUi e o adaptador `ui-clipboard-arboard`. Uma cópia que
não sai do processo não é cópia: quem copia da IDE espera colar no navegador e no
terminal. Ambiente sem área de transferência não impede a IDE de abrir — copiar e
colar ficam desligados e dizem isso na barra de estado.

A fonte de código é monoespaçada, e isso é uma decisão assumida da IDE, não um
detalhe de implementação: o editor localiza coluna, seleção e cursor por
contagem de colunas, como qualquer editor de código. Trocar por fonte
proporcional embaralharia o texto. A premissa é verificada por teste na
biblioteca; a largura concreta não é fixada, porque monoespaçadas diferentes têm
larguras diferentes e o editor mede a que estiver instalada.

A IDE não mede texto: ela entrega aos componentes o mecanismo de texto da
ERLibUi, e cada um pergunta a largura do que vai desenhar. Métrica de fonte é
conhecimento de interface, e mantê-la aqui faria cada aplicação da biblioteca
redescobrir a mesma coisa.

O código é desenhado pelo `CodeEditor` da ERLibUi. Calha, números de linha,
realce sintático, marcas de ponto de parada, linha em execução e cursor pertencem
ao componente; a IDE entrega o texto do documento ativo, o realce convertido do
analisador e as decorações, e recebe de volta a linha clicada na calha. As
métricas — altura da linha e largura da calha — vêm de lá, para que cursor, popup
de autocomplete e cliques usem os mesmos números que o desenho.

O texto continua sendo do `EditorSession`: o editor da biblioteca guarda uma cópia
para desenhar, reconstruída quando a revisão do documento muda. Por isso pintar
exige acesso mutável ao shell — deixar essa reconciliação para os manipuladores de
evento faria cada esquecimento virar um quadro desatualizado.

O suporte a cada linguagem será fornecido por um módulo desacoplado, chamado neste documento de `Language Provider`.

Cada provider poderá oferecer, de forma independente:

- detecção de arquivos;
- parsing;
- análise sintática;
- análise semântica;
- indexação;
- autocomplete;
- diagnósticos;
- formatação;
- refatorações;
- compilação;
- execução;
- testes;
- depuração;
- integração com ferramentas externas.

## Diferencial

A IDE deve evitar o modelo no qual toda funcionalidade permanece carregada continuamente.

Exemplo:

```text
Projeto sem Java aberto
    ↓
Java Provider não é inicializado
    ↓
Parser, índices e ferramentas Java não consomem memória
```

Quando Java estiver ativo:

```text
Arquivo .java aberto
    ↓
Language Registry identifica Java
    ↓
Java Provider é ativado
    ↓
Somente serviços Java necessários são inicializados
```

## Objetivos não funcionais

### Desempenho

- inicialização rápida;
- interface responsiva;
- análise incremental;
- cancelamento de tarefas;
- processamento paralelo controlado;
- ausência de pausas globais de garbage collection;
- mínimo trabalho no thread da interface.

### Memória

- carregamento sob demanda;
- caches limitados;
- descarregamento de providers;
- índices persistidos em disco;
- uso de IDs compactos;
- compartilhamento de estruturas imutáveis;
- separação de processos para serviços pesados.

### Resiliência

- falha de um provider não deve derrubar a IDE;
- falha de um plugin deve ser isolada;
- índices corrompidos devem poder ser reconstruídos;
- processos externos devem possuir timeout e cancelamento;
- estados incompletos devem ser recuperáveis.

### Extensibilidade

- novas linguagens sem alterar o núcleo;
- novos compiladores sem alterar providers;
- novos depuradores por adapters;
- novos sistemas de build;
- novos formatos de projeto;
- novas integrações com servidores.

## Aparência

A IDE não define cores próprias. O tema vem da ERLibUi — `Theme::dark()`,
`Theme::light()` e `Theme::high_contrast()` — e é entregue aos componentes da
biblioteca pelo contexto de pintura, de modo que barra de menus, combos, campos e
modais acompanham o mesmo tema que o resto da janela.

Reimplementar aparência na IDE é o caminho errado: cada cor duplicada aqui é uma
cor que deixa de acompanhar o tema. Quando um componente da biblioteca não
atende, a correção pertence à biblioteca.

A regra é verificada por teste: nenhuma cor literal pode aparecer na camada de
apresentação. Quando falta uma cor, acrescenta-se um token ao tema da
biblioteca, e não uma exceção aqui.

## Executar a aplicação

A barra de menus tem, no canto direito, três botões que resolvem o trabalho do
dia a dia com um clique: um quadrado que **para** a aplicação, um triângulo que
a **executa** e um inseto que a **executa com depuração** e conecta o depurador.
Executar e depurar usam o mesmo comando, deduzido do projeto importado ou
declarado pelo usuário na configuração; a diferença é apenas o agente de
depuração.

A aplicação sobe no terminal integrado, com o comando visível, para que o
usuário acompanhe a saída e a interrompa como faria em qualquer terminal. Parar
envia a mesma interrupção de um `Ctrl+C`: a IDE não mata processo por fora, e o
programa encerra como encerraria no terminal. Interromper com uma sessão de
depuração aberta a desconecta antes, para o depurador não apontar para um
processo que está terminando.

A IDE não esconde o que executou nem gerencia o processo por trás do usuário. As
mesmas ações estão no menu `Projeto`, porque atalho de barra não pode ser o
único caminho para uma função.

## Neutralidade em relação a servidores

A IDE não é feita para um servidor de aplicação específico. Tomcat, Jetty,
WildFly, JBoss EAP, WebSphere, Liberty, Quarkus, Spring Boot e qualquer outro
processo Java — inclusive ferramentas como Flyway e jobs em lote — precisam
receber o mesmo tratamento.

A forma de integração escolhida é a **depuração**: o usuário inicia o processo
com depuração habilitada e informa host e porta; a IDE se conecta, para nos
breakpoints e executa o código linha a linha a partir dali. Como todo servidor
Java oferece esse mecanismo, suportar mais um não exige código novo.

Iniciar, parar e publicar artefatos continua sendo responsabilidade do usuário e
de suas ferramentas. Operações específicas de um produto, se existirem um dia,
serão adapters opcionais — nunca requisito para usar a IDE.
