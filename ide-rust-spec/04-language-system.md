# 04 — Sistema de Linguagens

## Conceito

Cada linguagem deve ser representada por um provider independente.

Exemplos:

```text
JavaLanguageProvider
RustLanguageProvider
PythonLanguageProvider
TypeScriptLanguageProvider
```

O núcleo conhece apenas:

```rust
Arc<dyn LanguageProvider>
```

## Registro

```rust
pub trait LanguageRegistry: Send + Sync {
    fn register(
        &self,
        provider: Arc<dyn LanguageProvider>,
    ) -> Result<(), RegistryError>;

    fn unregister(
        &self,
        provider_id: &ProviderId,
    ) -> Result<(), RegistryError>;

    fn providers_for_extension(
        &self,
        extension: &str,
    ) -> Vec<ProviderDescriptor>;

    fn active_provider(
        &self,
        language_id: &LanguageId,
    ) -> Option<ProviderDescriptor>;
}
```

## Ativação e desativação

Estado sugerido:

```rust
pub enum ProviderState {
    Registered,
    Disabled,
    Activating,
    Active,
    Suspended,
    Failed,
    ShuttingDown,
}
```

Fluxo:

```text
Registered
   ↓ enable
Activating
   ↓ success
Active
   ↓ idle policy
Suspended
   ↓ request
Active
   ↓ disable
ShuttingDown
   ↓
Disabled
```

## Seleção de provider

Uma linguagem pode possuir vários providers:

```text
Java
├── native-java-analyzer
├── jdtls-adapter
└── remote-java-service
```

Configuração:

```toml
[languages.java]
enabled = true
provider = "native-java-analyzer"
fallback_provider = "jdtls-adapter"
```

## Seleção de toolchain

Provider e toolchain são conceitos diferentes.

```text
Language Provider
    analisa o código

Toolchain
    compila e executa
```

Exemplo:

```toml
[languages.java]
provider = "native-java-analyzer"

[toolchains.java]
selected = "temurin-17"
```

## Capabilities

```rust
bitflags::bitflags! {
    pub struct LanguageCapabilities: u64 {
        const SYNTAX = 1 << 0;
        const SEMANTICS = 1 << 1;
        const COMPLETION = 1 << 2;
        const DIAGNOSTICS = 1 << 3;
        const DEFINITION = 1 << 4;
        const REFERENCES = 1 << 5;
        const RENAME = 1 << 6;
        const FORMAT = 1 << 7;
        const BUILD = 1 << 8;
        const RUN = 1 << 9;
        const DEBUG = 1 << 10;
    }
}
```

## Completação de membros

`COMPLETION` cobre dois pedidos diferentes no mesmo método. Sem contexto de
acesso, a resposta é o que o prefixo alcança no arquivo e no índice do workspace.
Depois de um caractere de gatilho da linguagem, a resposta são os **membros do
receptor** — e só eles: devolver palavras-chave e classes soltas ali seria falar de
outra coisa que não o objeto à esquerda do ponto.

Quem separa os dois casos é o provider, não o editor. Ele tem o texto e a posição,
então ele lê para trás e decide se há acesso a membro, qual é o receptor e qual é o
prefixo já digitado. O editor não conhece a sintaxe da linguagem, e o pedido
continua sendo um `CompletionRequest` sem campo novo.

A resolução do tipo do receptor tem duas fontes:

- a **declaração no arquivo aberto** — variável local, parâmetro ou campo —, que o
  índice semântico já registra em `type_descriptor`;
- o **próprio nome** tomado como tipo, quando não há declaração. É o caso do
  acesso estático, `Integer.` ou `Math.`.

Os membros também vêm de duas fontes somadas: o arquivo aberto, que responde pelo
tipo que ainda não foi compilado, e as classes compiladas alcançadas pelo índice —
biblioteca padrão do toolchain escolhido, dependências e o próprio projeto depois
de um build. A cadeia de superclasses é percorrida porque um membro herdado é tão
membro do objeto quanto o declarado. Só o que é público entra, sem construtor e sem
membro sintético.

Indexar a biblioteca padrão exige ler os nomes **pelo diretório do arquivo
compactado**, e não decodificando cada classe: um único módulo do JDK passa de seis
mil classes, e o índice precisa apenas do nome. Os membros de um tipo são lidos sob
demanda, um tipo por vez.

## Geração de membros

A IDE oferece `Generate` com `Constructor`, `Getter`, `Setter` e `Getter and
Setter`, e **não sabe** o que é nenhum deles: a tela mostra nomes de campos e
escreve o texto que recebe. Toda a convenção — `getNome` mas `isAtivo` para
`boolean`, tipo de retorno, indentação, ordem dos parâmetros, onde o trecho entra
— é da linguagem.

São duas operações, e a diferença entre elas é o que motiva a segunda:

- `accessor_plan(document_id, position, kind)` devolve os campos do tipo que
  contém a posição, cada um com o texto pronto do acessor, ou `None` quando ele
  já existe. Cada campo gera um trecho **independente**, então dá para entregar
  tudo de uma vez e deixar a tela escolher quais usar;
- `constructor_source(document_id, position, fields)` devolve **um** texto,
  montado a partir do conjunto escolhido. Um construtor não é a soma de trechos
  por campo, e a escolha muda a assinatura inteira: por isso ele é pedido depois
  da janela, com os campos marcados na mão. Lista vazia é um construtor sem
  parâmetros — uma resposta legítima, e não a ausência de resposta. `None` é o
  tipo já ter um construtor de mesma assinatura, caso em que escrever outro não
  compilaria.

A linha de inserção vem do plano, presa ao corpo do tipo: com o cursor na linha da
declaração, ou depois da chave que a fecha, o membro sairia fora da classe.

## Renomeação

O gesto parte da **árvore de arquivos**, e o alvo é o arquivo. Renomear
`Pedido.java` para `Compra.java` renomeia o arquivo, o tipo dentro dele e todas as
referências ao nome no projeto.

`references_to_name(name)` responde onde o nome aparece — no projeto inteiro,
inclusive em arquivos fechados, e por isso a pergunta é pelo **nome** e não por uma
posição num arquivo aberto. A rota até o provider é pela **extensão**: o arquivo
pode não estar aberto, que é o caso comum ao renomear pela árvore. Sem provider
para aquela extensão a resposta é vazia, e renomear vira só mover o arquivo — que
continua sendo uma resposta útil para um `.md` ou um `.properties`.

A escrita é repartida por quem tem o quê:

- **arquivos abertos** são reescritos pela tela, no buffer: a aba mantém cursor,
  desfazer e alterações não salvas, e gravar por cima delas perderia trabalho que
  o disco não tem;
- **arquivos fechados** e o `rename` do arquivo em si são da aplicação, que tem a
  porta do sistema de arquivos. `WorkspacePort::rename_path` falha se o destino já
  existir — sobrescrever apagaria o arquivo de outro tipo.

A gravação dos fechados é **tudo ou nada**: os conteúdos novos são calculados antes
de qualquer escrita, e uma falha no meio restaura os já gravados. Meio caminho é um
projeto que não compila com o usuário sem saber onde parou.

A troca em si — posições em linha e coluna viram bytes, do fim para o começo do
texto — vive em `ide_workspace::rewrite_occurrences`, num lugar só, porque a tela e
a aplicação precisam dela pelos dois lados.

### O índice não acompanha edição

`references_to_name` soma duas fontes, e elas não valem o mesmo. O índice do
workspace é montado na ativação e **não é incremental** (`ADR-015`): para um
arquivo aberto ele fala do texto de antes das edições, e depois de uma renomeação
ele ainda guarda o caminho antigo.

Por isso o documento aberto **vence** o índice: dele vem tudo sobre os arquivos
abertos, e do índice só o que não está aberto. Sem esse corte a mesma ocorrência
aparecia duas vezes, uma em posição vencida, e a janela listava um arquivo com o
nome antigo.

Renomear também **reativa** o provider, o que refaz o índice inteiro. É caro — da
ordem de um segundo e meio num projeto médio — mas renomear é raro, e sem isso a
renomeação seguinte partiria de caminhos que não existem mais. Um índice
incremental tornaria as duas coisas desnecessárias.

## Estratégia de fallback

Se uma capability não estiver disponível no provider principal, o host pode consultar outro adapter.

Exemplo:

```text
Native Java Provider
├── parsing
├── symbols
├── autocomplete
└── sem debugger

Java Debug Adapter
└── conexão JDWP a um processo em execução
```

O usuário percebe um único suporte Java, mas internamente as capacidades são compostas.

## Composição de capacidades

```rust
pub struct LanguageRuntime {
    syntax: Arc<dyn SyntaxEngine>,
    semantics: Option<Arc<dyn SemanticEngine>>,
    index: Arc<dyn SymbolIndex>,
    formatter: Option<Arc<dyn FormatterAdapter>>,
    compiler: Option<Arc<dyn CompilerAdapter>>,
    runtime: Option<Arc<dyn RuntimeAdapter>>,
    debugger: Option<Arc<dyn DebugAdapter>>,
}
```

Não criar uma classe monolítica que implemente tudo.

## Implementação da Fase 2

O crate `ide-language-host` implementa o registro central. No registro, o host:

- valida a versão principal de `ide-language-api`;
- rejeita identificadores vazios e providers duplicados;
- normaliza extensões sem ponto e sem distinção entre maiúsculas e minúsculas;
- armazena metadata, capabilities, estado e último erro de cada provider.

A ativação é preguiçosa: registrar um provider não cria seu runtime. A primeira
solicitação para uma extensão compatível muda o estado de `Registered` para
`Activating` e então para `Active`. Falha de ativação muda o estado para
`Failed` e faz o host tentar o próximo fallback configurado.

`ProviderSelection` define um provider principal e uma lista ordenada de
fallbacks por linguagem. O roteamento filtra primeiro por extensão e
capabilities; um provider sem a capability solicitada nunca recebe a operação.
Na ausência de configuração explícita, a ordem estável dos identificadores é
usada.

Cada documento aberto fica associado ao provider que aceitou sua abertura.
Mudanças, diagnósticos e fechamento são enviados ao mesmo worker até que o
documento seja fechado ou o provider seja desativado.

`disable` executa `shutdown`, encerra o worker, remove as rotas de documentos e
termina em `Disabled`. `enable` devolve um provider desativado ou falho para
`Registered`, permitindo uma nova ativação sob demanda.

## Pedidos sem espera

O worker de cada provider vive em thread própria e recebe pedidos por canal, com
envio que **nunca bloqueia**: fila cheia devolve `Backpressure` em vez de esperar.

Duas operações do caminho da digitação têm forma postada — `post_change_document`
e `post_syntax` —, que enfileiram e devolvem o receptor sem aguardar a resposta.
Quem digita segue com a tecla, e quem chamou recolhe o resultado quando o provider
terminar. A fila é ordenada, então o realce pedido depois de uma mudança fala do
texto **com** ela aplicada, e as consultas que ainda esperam — completação,
navegação, geração de acessores — são processadas depois do que já foi postado,
ou seja, enxergam o texto atual.

Quando o envio falha por contrapressão, a mudança **não** entrou na fila, e quem
mantém o registro de "o que o provider já tem" não deve avançá-lo: a sincronização
seguinte recalcula a diferença do mesmo ponto e tenta outra vez, com um pedaço
maior. Nada se perde e nada bloqueia. Ver `ADR-017`.
