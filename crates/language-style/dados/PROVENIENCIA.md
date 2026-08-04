# `propriedades-css.txt`

A lista de propriedades CSS que a completação oferece. É o nível 2 da fase 5 da
`23`.

| | |
| --- | --- |
| origem | `mdn-data`, `css/properties.json` |
| versão | **2.27.1** |
| licença | **CC0-1.0** — domínio público, sem exigência de atribuição |
| repositório | `https://github.com/mdn/data` |

## O que entrou, e o que não

O `properties.json` tem 651 entradas. Ficaram **523**.

| deixado de fora | quantas | por quê |
| --- | --- | --- |
| `status: nonstandard` | 107 | não é CSS, é o que um motor fez sozinho |
| `status: obsolete` | 15 | `grid-gap`, `clip`, `page-break-*` — oferecer é ensinar errado |
| começando com `-` | 100 | prefixo de fornecedor, e o `--*` das propriedades personalizadas |

**`status: experimental` entrou**, e é escolha: são 39, e incluem
`anchor-name`, `animation-timeline` e `field-sizing` — coisas que já se escrevem
hoje. Recusá-las por não serem *baseline* seria a IDE discordando do que o
navegador aceita.

São 8,1 KB. O `properties.json` inteiro tem 336 KB, dos quais a maior parte é a
sintaxe dos valores — que só o nível 3 usaria.

## Como atualizar

```bash
npm pack mdn-data@<versão>
```

Regenere com o mesmo critério — `status` em `standard` ou `experimental`, e nome
sem `-` inicial —, um nome por linha, ordenado, com quebra `\n`. Atualize a
versão na tabela acima.

O teste `a_lista_tem_o_tamanho_que_a_procedencia_declara` falha se a contagem e
este documento discordarem.
