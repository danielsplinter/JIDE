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
