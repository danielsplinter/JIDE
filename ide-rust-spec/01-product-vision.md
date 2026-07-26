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
não existe mais é ignorado em silêncio e a IDE abre normalmente. Os detalhes do
arquivo e das regras estão em [08 — Persistência](08-storage-and-memory.md).

O menu `Configurações` abre uma janela modal com navegação em um painel
esquerdo e o conteúdo da opção selecionada no painel direito. A primeira opção
é `Compilador e VM`. A janela fica suspensa sobre a IDE: workspace, editor,
Explorer e terminais permanecem visíveis e levemente escurecidos ao redor, mas
seus textos nunca podem atravessar ou ser desenhados sobre o painel modal. O
conteúdo inferior permanece inativo até o fechamento. Barra principal, combo e
isolamento modal devem reutilizar respectivamente `MenuBar`, `ComboBox` e
`ModalHost` da ERLibUi, sem duplicar desenho ou hit testing na IDE.

Todo o conteúdo da árvore deve ser recortado pelos limites do painel esquerdo.
Quando nomes ou níveis de indentação ultrapassarem a largura disponível, o
Explorer deve mostrar uma barra de rolagem horizontal com clique na trilha e
arraste do indicador. Texto da árvore nunca pode vazar sobre o editor.

O Explorer também deve possuir barra de rolagem vertical quando a árvore
ultrapassar a altura disponível. A borda direita do painel deve permitir
redimensionamento horizontal por clique, retenção e arraste. Editor e painel de
terminal ocupam sempre a largura restante e acompanham imediatamente essa
mudança de layout, sem enviar redimensionamentos ao PTY.

Cada aba de editor deve exibir um botão `x` próprio. Clicar nesse botão fecha
somente o documento correspondente e ativa uma aba remanescente quando
necessário.

O título de cada aba deve permanecer estritamente dentro de seus limites e
reservar espaço fixo para o botão `x`. Nomes longos são abreviados com
reticências e também recortados graficamente; nunca podem invadir a aba vizinha
ou cobrir seu botão de fechamento.

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
