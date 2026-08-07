# 26 — A dívida que uma sessão longa deixou

## Situação

Uma sessão de dez commits mexeu em nove crates e acrescentou capacidade de
verdade — os tipos do TypeScript no índice, as dependências instaladas, o
segundo elo da cadeia, a completação por nome com auto-import. Todas as guardas
de arquitetura continuam passando.

**E guarda que passa não quer dizer arquitetura intacta.** Quer dizer que
ninguém cruzou uma linha que alguém já tinha desenhado. O que esta especificação
registra é o que cresceu **onde não havia linha nenhuma**, e por isso cresceu
sem ninguém precisar autorizar.

Ela é curta de propósito, e nasce **pendente**: nenhum dos itens é uma correção
de defeito. São hipotecas. As três primeiras vieram daquela sessão; a quarta foi
acrescentada depois, no dia em que um defeito de verdade foi corrigido pela
metade — e a metade que ficou é dívida pelo mesmo motivo das outras.

## O que não se perdeu, e é a metade que importa

Antes das dívidas, o que a sessão **não** custou — porque uma lista só de dívidas
mente por omissão:

- **o contrato cresceu neutro.** `TextEdit` e `completion_edits` nasceram
  falando de "o que mais muda no arquivo", e não de `import` de TypeScript: Java
  tem a mesma necessidade, e a `04` continua valendo;
- **o `analyzer` continua sem alcançar projeto.** A fase 7 da `25` nasceu
  partida em dois módulos — `analyzer::stdlib` para o texto, `project::stdlib`
  para o disco e o `tsconfig` — porque a fronteira exigiu, e não porque ficou
  bonito;
- **as 18 crates continuam 18**, e nenhuma dependência nova entrou no grafo
  protegido;
- **a guarda do `block_on` nasceu nesta sessão**, e já pegou duas adições no dia
  em que foram escritas. Ela é o oposto de dívida: é linha nova onde não havia.

## Dívida 1 — `native_ide.rs`, 4 680 linhas 🟨

É o objeto-deus deste código. As `14` e `15` já registraram que ele deveria ser
decomposto, e nesta sessão ele cresceu de novo: a completação assíncrona, as
trocas de texto do auto-import, o `text_typed`, os diagnósticos.

**Não havia teto para ele.** As `12` e `15` puseram teto em fachadas — hoje 35
linhas na raiz da `ide-ui`, 18 na do `language-java`, 11 na do host — e nenhum no
arquivo que mais cresce. Ele cresceu em silêncio até o teto abaixo.

*E há um sinal de que o tamanho já cobra.* O tratamento da tecla vivia dentro do
`match` do evento de janela, e **nenhum teste o alcançava**: montar um evento
desses exige o winit inteiro. O resultado foi um defeito relatado três vezes,
com as duas pontas já sondadas e respondendo, enquanto o que faltava testar era
o pedaço que não dava para testar. Foi extraí-lo que resolveu.

**O primeiro passo é um teto no número de hoje**, como o das primitivas de
desenho da `15`. Ele não conserta nada; ele torna o crescimento visível e
deliberado. É pequeno, e é o único desta lista que cabe em meia hora.

**Critério:** uma guarda com o teto de linhas de `native_ide.rs`, e o número
descendo — não subindo — a cada decomposição.

**O primeiro passo foi dado.** O teto existe, em 5 389 linhas — o arquivo tinha
4 680 quando esta dívida foi escrita, e cresceu 709 desde então **sem que nada
avisasse**, que é exatamente o que a guarda vem impedir. O mesmo teto foi posto
em `ide_shell/tests.rs`, com 7 269 linhas, pelo mesmo motivo.

**O de `tests.rs` acabou, e não por ter sido respeitado.** Ele subiu seis vezes —
7 269, 7 315, 7 333, 7 405, 7 434, 7 499… até 8 519 —, e nas duas últimas o
próprio comentário do teto já dizia que ele não subiria de novo sem o arquivo ser
partido. Foi partido: os 220 testes viraram dez arquivos por assunto — o
Explorer, o texto, a divisão, as buscas, a completação, o terminal, o Git, a
depuração, as janelas e a moldura —, com os ajudantes num `mod.rs` que todos
alcançam.

**E o que guarda o resultado mudou de forma.** No lugar do número há uma regra:
nenhum arquivo daquela pasta passa de 1 400 linhas. Um teto que só sobe registra
o que foi feito; um limite por arquivo só se respeita cortando de novo. É a
diferença que a dívida 2 desta mesma especificação vem cobrando do teto de
`block_on`, e aqui ela já está paga.

A dívida continua aberta: o teto não decompõe nada. Ele só faz o crescimento
parar de ser silencioso, e o número agora só pode descer sem alguém assinar
embaixo.

## Dívida 2 — o teto de `block_on` subiu cinco vezes, e nunca desceu 🟥

De 27 para 28, e de 28 para 29. As duas com justificativa escrita, e as duas por
decisão de quem estava escrevendo o código — que é exatamente o desenho da
guarda: ela obriga alguém a olhar, e não decide por ninguém.

**Duas subidas no mesmo dia é o padrão de como um teto vira formalidade.** Um
número que só sobe deixa de medir; ele vira um registro do que foi feito, e não
um limite do que se pode fazer.

Não há conserto óbvio, e é por isso que está aqui em vez de ser feito: a guarda
grosseira foi escolhida **de propósito**, depois de a precisa medir 21, 0 e 4 em
três tentativas. Trocar a grosseira por uma que erra calada seria pior.

**O que vale observar:** se o número subir uma terceira vez sem uma queda no
meio, a guarda deixou de funcionar e o problema passa a ser dela, e não de quem
a levanta.

### Aconteceu

Conferido contra o código: **o teto está em 32**. Depois das duas subidas que
originaram esta dívida, ele subiu mais três vezes — 30 pelos crachás do Explorer,
31 pelo renderizador da tela de abertura, 32 pela detecção do sistema de build —
e **nunca desceu**. Todas com justificativa escrita; nenhuma revertida.

A condição que este texto definiu como "a guarda deixou de funcionar" foi
cruzada, e o problema é dela: um número que só sobe registra o que foi feito, e
não limita o que se pode fazer.

Há um agravante que não existia quando isto foi escrito: a subida de 31 para 32
foi a **primeira que não está em thread própria**. A justificativa é real — a
detecção só procura manifesto em disco —, mas a guarda não distingue as duas
coisas, e era a distinção que ela existia para provocar.

**O que fazer não está decidido.** A opção que parece honesta é a guarda deixar
de contar e passar a **exigir**: cada `block_on` carrega ao lado uma marca
dizendo em que thread ele roda, e a guarda recusa os sem marca. Ela para de
medir um número que só cresce e passa a cobrar a frase que hoje é voluntária.
Isso é trabalho de verdade, e por isso está registrado em vez de feito.

## Dívida 3 — o serviço guarda estado que pode envelhecer ⬜

O auto-import precisa devolver ao analisador a identificação da entrada que ele
ofereceu — o `data` opaco. Guardá-la no `CompletionItem` obrigaria vinte e três
construções em cinco crates a carregar um campo que só interessa a quem faz a
pergunta seguinte, e por isso ela ficou num mapa dentro do serviço: **a última
lista respondida, por documento**.

O acoplamento é novo. A escolha confia em que ela venha logo depois do pedido —
o que é verdade no uso normal, porque é a mesma lista que está na tela. Não é
verdade se algo se meter no meio.

**O que aconteceria de errado:** a escolha responderia sobre a lista de antes, e
escreveria o `import` do que não foi escolhido. É silencioso, e é o formato de
defeito que esta arquitetura mais persegue.

**A defesa honesta é uma chave**, e não a confiança: guardar junto o que
identifica o pedido — documento e posição — e recusar a resposta quando ela não
casar, como a completação já faz com a resposta vencida. É pequeno, e não foi
feito porque apareceu no fim de uma sessão longa.

**Critério:** um teste em que a escolha chega depois de outra lista ter sido
pedida, e nada é escrito.

**Conferido contra o código, e continua valendo.** O mapa segue lá —
`origens: Mutex<HashMap<DocumentId, Origens>>` —, e a posição do pedido passou a
ser guardada junto, como esta dívida pedia. **Mas a recusa não existe**: quem
responde lê a linha e a coluna guardadas e usa as duas, sem compará-las com o
pedido que está atendendo — nem teria como, porque a assinatura recebe só o
documento e o rótulo. Metade da defesa está de pé; a que importa, não.

## Dívida 4 — duas respostas para "onde o projeto começa" ⬜

Uma pasta que contém **uma única pasta** é a forma de quem clona um repositório
dentro de uma pasta de mesmo nome: abre-se `projetos/java/camel-main`, e o
`pom.xml` está em `camel-main/camel-main`.

Hoje a IDE responde a essa situação em **lugares que não concordam**:

- a árvore do Explorer **desce** a cadeia de pastas únicas e mostra o conteúdo
  em vez da porta seguinte (`scan_path_until_content`);
- a **detecção do sistema de build** também desce, desde o commit que voltou a
  reconhecer `camel-main` como Java — é dela que sai a linguagem do menu de
  recentes;
- a **importação do projeto** *não* desce. Ela procura manifesto só na raiz
  aberta, não acha, e **desiste em silêncio**.

O resultado é uma IDE que diz "este é um projeto Java" no menu e ao mesmo tempo
não tem projeto nenhum importado: sem sumário, sem raízes de fontes declaradas,
sem compilar nem executar pelo menu Projeto. Nada disso avisa — a importação sem
detecção retorna sem mensagem, porque "esta pasta não é um projeto" é resposta
legítima na maioria das vezes.

**Por que não foi corrigido junto.** Descer na detecção é barato: ela responde
uma pergunta só, e errar custa uma etiqueta errada num menu. Descer na
importação muda **qual pasta é a raiz do workspace** — e a raiz decide as raízes
de fontes, o diretório do terminal, o escopo da busca, o processo de execução, a
ferramenta que vale por projeto e o que é gravado na configuração. É decisão de
produto, e não correção: pode ser que a IDE deva adotar a pasta de dentro como
raiz, que deva perguntar, ou que deva abrir a de dentro desde o começo.

**O risco de deixar assim** é o formato de defeito que esta arquitetura mais
persegue: duas fontes para a mesma verdade, divergindo em silêncio. Quem
trabalha num projeto assim vê o Explorer certo e o menu Projeto morto, sem nada
ligando uma coisa à outra.

**Critério:** uma resposta só para "onde este projeto começa", usada pela
árvore, pela detecção e pela importação — e um teste que abra uma pasta que
apenas embrulha o projeto e afirme que ele foi **importado**, e não apenas
reconhecido.

## O que foi conferido, e quando

Esta seção existe porque uma especificação de dívida **envelhece contra o
código**, e uma dívida que descreve o que já mudou é pior do que dívida nenhuma:
ela é lida como verdade.

Conferência de 06/08/2026, item a item:

| | estado | o que se achou |
|---|---|---|
| Dívida 1 | 🟨 | O texto dizia "não há teto"; havia deixado de ser verdade no dia anterior, e os números das fachadas citados estavam velhos — 31 e 10 contra 35 e 11 reais. Corrigido acima. |
| Dívida 2 | 🟥 | O teto está em 32, e não em 29. Subiu três vezes além do que este texto registrava, **sem nenhuma queda** — a condição que ele mesmo definiu como fracasso da guarda. |
| Dívida 3 | ⬜ | Continua inteira. A posição foi acrescentada ao que se guarda; a recusa quando ela não casa, não. |
| Dívida 4 | ⬜ | Continua inteira. `import_project` chama `detect(root)` direto, sem descer a cadeia de pastas únicas. |
| "as 18 crates continuam 18" | ✅ | Verdade: são 18, e nenhuma aresta nova entrou no grafo protegido. |

## Por que isto é uma especificação, e não uma lista de tarefas

Porque as quatro têm **motivo**, e motivo é o que uma lista perde. A dívida 1
tem uma história que a explica; a 2 é uma guarda que pode estar se gastando; a 3
é uma troca deliberada entre acoplar em cinco crates e acoplar em duas chamadas;
a 4 é uma correção que parou no ponto em que deixaria de ser correção e passaria
a ser decisão de produto.

Quem chegar aqui daqui a seis meses vai reencontrar as quatro, e a pergunta que
importa não é "o que falta" — é **"por que ficou assim"**. É isso que está
escrito.
