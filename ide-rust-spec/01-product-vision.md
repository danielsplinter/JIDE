# 01 — Visão do Produto

## Proposta

A IDE será uma plataforma nativa em Rust para desenvolvimento em diversas linguagens.

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
