# `@angular/language-service`, embarcado

Este diretório é uma cópia podada de `@angular/language-service`, versionada
para viajar dentro do executável da IDE. Ver a ADR-029.

| | |
| --- | --- |
| pacote | `@angular/language-service` |
| versão | **21.2.17** |
| licença | MIT — Copyright Google LLC |
| origem | `https://github.com/angular/angular`, `packages/language-service` |

## Por que ele está aqui

O plugin responde pelos templates `.html` **dentro do `tsserver` do projeto** —
sem segundo processo, e por isso a +385 MB em vez de +2,1 GB. Mas ele não está
na maioria dos projetos: dos cinco projetos Angular de referência, **um** o traz
em `node_modules`. Sem esta cópia, quatro deles abririam o template sem nada.

**O do projeto continua vencendo.** Esta cópia só é usada quando o projeto não
tem a sua, e o `tsserver` é sempre o do projeto — é ele quem decide se um tipo
bate, e a regra da ADR-028 fica de pé onde importa.

## O que foi removido, e por quê

O pacote publicado tem 14 MB; aqui são 4,1 MB.

| removido | motivo |
| --- | --- |
| o sourcemap embutido em `bundles/language-service.js` | 9 dos 13 MB do arquivo; serve a quem depura o próprio serviço |
| `*.d.ts`, `src/` | tipos para quem **compila** contra o pacote; nada os lê em execução |
| `api_bundle.js`, `private_bundle.js` e os `.map` soltos | pontos de entrada que o `tsserver` não alcança |

O que sobrou é exatamente a cadeia que o `tsserver` percorre:

```text
package.json  ->  index.js  ->  factory_bundle.js  ->  bundles/language-service.js
```

O cabeçalho de licença do bundle **não** foi tocado, que é o que a MIT exige.

## Como atualizar

```bash
npm pack @angular/language-service@<versão>
```

Copie os quatro arquivos acima, corte tudo a partir de
`//# sourceMappingURL=data:` no bundle, e rode o critério da fase 1 da `24`
contra um projeto que **não** traz o pacote:

```bash
ER_IDE_PROJETO_ANGULAR=<projeto> cargo test -p language-angular --test template -- --ignored
```

Atualize a versão nesta tabela. O teste `a_versao_embarcada_esta_declarada`
falha se ela e o `package.json` discordarem.
