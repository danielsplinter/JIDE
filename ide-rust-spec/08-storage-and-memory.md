# 08 — Persistência, Cache e Memória

## Objetivo

Controlar memória explicitamente e evitar crescimento ilimitado.

## Categorias

### Estado essencial em memória

- documentos abertos;
- seleções;
- árvore sintática atual;
- símbolos usados no contexto;
- estado da UI.

### Estado persistível

- índices;
- metadados de dependências;
- cache de class files;
- histórico de projetos;
- resultados de análise estáveis.

### Estado descartável

- autocomplete anterior;
- diagnósticos obsoletos;
- árvores de arquivos fechados;
- resultados de busca antigos;
- previews.

### Limites do índice Java inicial

- no máximo 600 arquivos candidatos são visitados por ativação;
- no máximo 500 fontes Java são analisadas;
- no máximo 64 JARs são abertos;
- no máximo 20.000 class files são indexados por JAR;
- uma entrada `.class` maior que 16 MiB é ignorada;
- diretórios `.git`, `target`, `node_modules` e `.gradle` não são percorridos.

## Orçamento

```rust
pub struct MemoryBudget {
    pub syntax_bytes: usize,
    pub semantic_bytes: usize,
    pub index_cache_bytes: usize,
    pub ui_cache_bytes: usize,
    pub plugin_bytes: usize,
}
```

## Política de cache

```rust
pub trait CachePolicy: Send + Sync {
    fn should_retain(&self, entry: &CacheEntry) -> bool;
    fn eviction_priority(&self, entry: &CacheEntry) -> u64;
}
```

## Estruturas recomendadas

- interning de strings;
- arenas;
- IDs numéricos;
- árvores imutáveis;
- snapshots compartilhados;
- LRU;
- memory mapping;
- compactação de índices;
- paginação de resultados.

## Anti-padrões

Evitar:

- armazenar caminhos repetidos como `String`;
- duplicar AST e CST sem necessidade;
- guardar todos os arquivos parseados;
- cache sem limite;
- cópias profundas;
- ciclos de `Arc`;
- eventos contendo documentos inteiros;
- carregar todos os plugins na inicialização.

## Métricas

A IDE deve exibir:

```text
Uso total
Uso por provider
Uso por workspace
Uso dos índices
Uso dos plugins
Processos externos
Caches descartáveis
```

## Limites

Um provider que exceder o orçamento poderá:

1. receber solicitação de limpeza;
2. suspender caches;
3. descarregar arquivos inativos;
4. reiniciar em processo isolado.
