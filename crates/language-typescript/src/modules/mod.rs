//! De que arquivo vem um nome importado.
//!
//! É a fase 2 da `25`, e a que decide se as fases 3 e 4 existem. Em Java, pacote
//! e classpath tornam um nome globalmente resolvível: `Pedido` é uma coisa só.
//! Em TypeScript quem decide o que um nome alcança é o **`import`** — o mesmo
//! nome em dois arquivos são duas coisas, e um nome pode não estar ao alcance.
//!
//! # Por que isto não mora no analisador
//!
//! Resolver módulo é conhecimento de **projeto**: depende do `tsconfig.json`, do
//! `baseUrl` e do `paths`. O analisador responde sobre texto, e a guarda de
//! arquitetura o mantém assim. Aqui se lê o que o analisador extraiu — a lista
//! de `import` e de reexportação, que é texto — e se decide para onde cada um
//! aponta, que é projeto.
//!
//! # As três formas, e a terceira é a cara
//!
//! | forma | exemplo |
//! | --- | --- |
//! | relativa | `./pedido`, `../modelo/pedido` |
//! | apelido do `paths` | `@spartacus/core` |
//! | barril | `./index` que reexporta de outro lugar |
//!
//! Medido no projeto de referência da `25`: **315 entradas em `paths`** e
//! **2 279 barris**. Sem as duas, quase todo `import` de um monorepo fica sem
//! destino.

use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

use crate::project::TsConfig;

mod instalada;

/// Quantos barris se atravessa antes de desistir.
///
/// Um barril que reexporta de outro é normal; uma cadeia de vinte é sinal de
/// ciclo que a detecção não pegou, ou de um projeto que não vale perseguir.
const PROFUNDIDADE: usize = 20;

/// As extensões tentadas quando o `import` não traz uma.
///
/// `.ts` antes de `.d.ts` de propósito: havendo os dois, o fonte é a verdade e a
/// declaração é o que sobrou do último build.
const EXTENSOES: [&str; 4] = ["ts", "tsx", "d.ts", "js"];

/// Resolve especificadores de módulo com as regras do projeto.
pub struct ModuleResolver {
    base_url: Option<PathBuf>,
    /// Apelidos já separados em prefixo e sufixo do `*`.
    apelidos: Vec<Apelido>,
    /// A pasta a partir da qual os destinos do `paths` são resolvidos.
    raiz_dos_apelidos: PathBuf,
}

struct Apelido {
    prefixo: String,
    sufixo: String,
    /// Se o padrão tem `*`. Sem ele, o apelido casa exato.
    curinga: bool,
    destinos: Vec<String>,
}

impl ModuleResolver {
    #[must_use]
    pub fn new(config: &TsConfig) -> Self {
        let apelidos = config
            .paths
            .iter()
            .map(|(padrao, destinos)| match padrao.split_once('*') {
                Some((prefixo, sufixo)) => Apelido {
                    prefixo: prefixo.to_owned(),
                    sufixo: sufixo.to_owned(),
                    curinga: true,
                    destinos: destinos.clone(),
                },
                None => Apelido {
                    prefixo: padrao.clone(),
                    sufixo: String::new(),
                    curinga: false,
                    destinos: destinos.clone(),
                },
            })
            .collect();
        Self {
            // Sem `baseUrl`, o `paths` do TypeScript moderno vale relativo ao
            // próprio `tsconfig.json`.
            raiz_dos_apelidos: config
                .base_url
                .clone()
                .unwrap_or_else(|| config.directory.clone()),
            base_url: config.base_url.clone(),
            apelidos,
        }
    }

    /// O arquivo para onde um `import` aponta, visto de um arquivo.
    ///
    /// Devolve `None` quando não se acha o destino — um pacote que não está
    /// instalado, ou que não declara tipo nenhum. **Não é erro**: é o índice
    /// dizendo que não alcança, que é diferente de dizer que não existe.
    ///
    /// # A ordem, e por que o projeto vem primeiro
    ///
    /// Relativo, depois `paths`, depois `baseUrl`, e **só então**
    /// `node_modules`. É a ordem do próprio TypeScript, e ela importa aqui pelo
    /// mesmo motivo que lá: um apelido do `paths` costuma apontar para o código
    /// do projeto que substitui um pacote publicado — num monorepo,
    /// `@empresa/core` é a pasta ao lado, e não o que está instalado. Procurar
    /// em `node_modules` antes responderia com a versão publicada enquanto quem
    /// edita olha para a local.
    ///
    /// # `node_modules` entrou na fase 9, e não estava aqui antes
    ///
    /// A fase 1 o deixou de fora do **índice**, para a busca por nome não encher
    /// de tipos que ninguém escreve — e isso continua valendo. O que estava
    /// junto e não devia é a **resolução**: sem ela a IDE só sabe o que o projeto
    /// declara, e ela é usada em projetos diferentes, onde o que se injeta vem do
    /// framework.
    #[must_use]
    pub fn resolve(&self, de: &Path, especificador: &str) -> Option<PathBuf> {
        if especificador.starts_with('.') {
            let pasta = de.parent()?;
            return arquivo_em(&pasta.join(especificador));
        }
        for apelido in &self.apelidos {
            if let Some(caminho) = self.tentar_apelido(apelido, especificador) {
                return Some(caminho);
            }
        }
        // Sem apelido, `baseUrl` ainda permite importar por caminho absoluto
        // dentro do projeto — `app/servicos/pedido` em vez de `../../servicos`.
        if let Some(base) = self.base_url.as_ref()
            && let Some(caminho) = arquivo_em(&base.join(especificador))
        {
            return Some(caminho);
        }
        instalada::resolver(de, especificador)
    }

    fn tentar_apelido(&self, apelido: &Apelido, especificador: &str) -> Option<PathBuf> {
        let miolo = if apelido.curinga {
            let resto = especificador.strip_prefix(&apelido.prefixo)?;
            if !resto.ends_with(&apelido.sufixo) {
                return None;
            }
            resto.get(..resto.len() - apelido.sufixo.len())?
        } else {
            if especificador != apelido.prefixo {
                return None;
            }
            ""
        };
        // Os destinos são tentados **em ordem**: é a regra do TypeScript, e o
        // primeiro que existir vence.
        apelido.destinos.iter().find_map(|destino| {
            let destino = destino.replace('*', miolo);
            arquivo_em(&self.raiz_dos_apelidos.join(destino))
        })
    }
}

/// O arquivo que um caminho sem extensão designa.
///
/// A ordem é a do TypeScript: o caminho como arquivo, depois como pasta com
/// `index`. Sem isto, `./modelo` não acha `modelo/index.ts`, que é como quase
/// todo barril é escrito.
fn arquivo_em(caminho: &Path) -> Option<PathBuf> {
    // O caminho tal como veio, quando o `import` traz a extensão.
    if caminho.is_file() {
        return Some(normalizado(caminho));
    }
    for extensao in EXTENSOES {
        if let Some(candidato) = com_extensao(caminho, extensao)
            && candidato.is_file()
        {
            return Some(normalizado(&candidato));
        }
    }
    for extensao in EXTENSOES {
        if let Some(candidato) = com_extensao(&caminho.join("index"), extensao)
            && candidato.is_file()
        {
            return Some(normalizado(&candidato));
        }
    }
    // `public_api.ts` não é convenção do TypeScript, é do Angular — e é o que os
    // pacotes gerados pelo `ng-packagr` apontam. Sem ele, um `import` de
    // biblioteca do próprio monorepo fica sem destino.
    for nome in ["public_api", "public-api"] {
        for extensao in EXTENSOES {
            if let Some(candidato) = com_extensao(&caminho.join(nome), extensao)
                && candidato.is_file()
            {
                return Some(normalizado(&candidato));
            }
        }
    }
    None
}

/// O caminho com a extensão **acrescentada**, e não trocada.
///
/// # Isto foi um defeito, e a comparação com o analisador o achou
///
/// `Path::with_extension` **substitui** o que vier depois do último ponto. Num
/// especificador como `./require-logged-in.commands` — ou `./pedido.service`, ou
/// `./pedido.model`, que são o idioma do Angular — ele produzia
/// `require-logged-in.ts`, um arquivo que não existe, e a importação ficava sem
/// destino.
///
/// O nome do módulo em TypeScript pode ter quantos pontos quiser; a extensão é
/// só o que o disco tem a mais.
fn com_extensao(caminho: &Path, extensao: &str) -> Option<PathBuf> {
    let texto = caminho.as_os_str().to_str()?;
    Some(PathBuf::from(format!("{texto}.{extensao}")))
}

/// Tira os `.` e `..` do caminho, sem tocar no disco.
///
/// `canonicalize` resolveria também os vínculos simbólicos e devolveria o
/// prefixo estendido do Windows — que não casa com o caminho que o resto da IDE
/// carrega, e transformaria comparação de caminho em defeito silencioso.
fn normalizado(caminho: &Path) -> PathBuf {
    let mut partes: Vec<std::ffi::OsString> = Vec::new();
    for parte in caminho.components() {
        match parte {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                partes.pop();
            }
            outro => partes.push(outro.as_os_str().to_os_string()),
        }
    }
    partes.iter().collect()
}

/// De onde vem um nome, seguindo os barris até a declaração.
///
/// `exportacoes` responde, para um arquivo, o que ele reexporta e de onde — é o
/// que o analisador extrai do texto. Aqui se decide **para onde** cada uma
/// aponta e se anda até quem declara de verdade.
///
/// # O ciclo não é hipótese
///
/// Barris que se reexportam em círculo existem em monorepo grande, e sem a marca
/// de visitado esta função não terminaria. O limite de profundidade é a segunda
/// rede: uma cadeia longa demais é sinal de que a resposta não vale a espera.
pub fn declarante(
    resolver: &ModuleResolver,
    arquivo: &Path,
    nome: &str,
    exportacoes: &dyn Fn(&Path) -> Vec<Reexportacao>,
    declara: &dyn Fn(&Path, &str) -> bool,
) -> Option<PathBuf> {
    let mut visitados = HashSet::new();
    let mut fila = vec![(arquivo.to_path_buf(), nome.to_owned(), 0usize)];
    while let Some((atual, procurado, profundidade)) = fila.pop() {
        if profundidade > PROFUNDIDADE || !visitados.insert((atual.clone(), procurado.clone())) {
            continue;
        }
        if declara(&atual, &procurado) {
            return Some(atual);
        }
        for reexportacao in exportacoes(&atual) {
            // Uma reexportação nominal só interessa se for **deste** nome.
            if let Some(exportado) = reexportacao.nome.as_deref()
                && exportado != procurado
            {
                continue;
            }
            // `export { A as B } from './a'` muda o nome no meio do caminho: de
            // `B` para cá, `A` para lá. Seguir procurando `B` no destino não
            // acharia nada, e pareceria limite do índice em vez de renomeação.
            let adiante = reexportacao
                .origem
                .clone()
                .unwrap_or_else(|| procurado.clone());
            if let Some(destino) = resolver.resolve(&atual, &reexportacao.de) {
                fila.push((destino, adiante, profundidade + 1));
            }
        }
    }
    None
}

/// Uma reexportação encontrada num arquivo.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Reexportacao {
    /// O nome reexportado, ou `None` para `export * from`.
    pub nome: Option<String>,
    /// Como o módulo de origem o declara, quando a reexportação renomeia.
    pub origem: Option<String>,
    /// O especificador do módulo de onde ele vem.
    pub de: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn projeto(nome: &str) -> PathBuf {
        let raiz = std::env::temp_dir().join(format!("er-ts-mod-{nome}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&raiz);
        assert!(std::fs::create_dir_all(&raiz).is_ok());
        raiz
    }

    fn escrever(caminho: &Path, conteudo: &str) {
        if let Some(pasta) = caminho.parent() {
            assert!(std::fs::create_dir_all(pasta).is_ok());
        }
        assert!(std::fs::write(caminho, conteudo).is_ok());
    }

    fn config(raiz: &Path, paths: Vec<(String, Vec<String>)>) -> TsConfig {
        let mut config = TsConfig::default();
        config.directory = raiz.to_path_buf();
        config.base_url = Some(raiz.to_path_buf());
        config.paths = paths;
        config
    }

    /// Um `import` relativo acha o arquivo ao lado.
    #[test]
    fn a_relative_import_finds_the_file_next_to_it() {
        let raiz = projeto("relativo");
        escrever(&raiz.join("src/pedido.ts"), "export class Pedido {}");
        escrever(&raiz.join("src/uso.ts"), "import { Pedido } from './pedido';");
        let resolver = ModuleResolver::new(&config(&raiz, Vec::new()));

        assert_eq!(
            resolver.resolve(&raiz.join("src/uso.ts"), "./pedido"),
            Some(raiz.join("src/pedido.ts"))
        );
        let _ = std::fs::remove_dir_all(&raiz);
    }

    /// Um caminho que sobe pastas é resolvido sem tocar no disco duas vezes.
    #[test]
    fn a_relative_import_that_climbs_is_normalised() {
        let raiz = projeto("subindo");
        escrever(&raiz.join("src/modelo/pedido.ts"), "export class Pedido {}");
        escrever(&raiz.join("src/pagina/uso.ts"), "");
        let resolver = ModuleResolver::new(&config(&raiz, Vec::new()));

        let achado = resolver.resolve(&raiz.join("src/pagina/uso.ts"), "../modelo/pedido");
        assert_eq!(achado, Some(raiz.join("src/modelo/pedido.ts")));
        let _ = std::fs::remove_dir_all(&raiz);
    }

    /// Uma pasta é o `index` dentro dela.
    #[test]
    fn a_folder_means_its_index() {
        let raiz = projeto("barril");
        escrever(&raiz.join("src/modelo/index.ts"), "export * from './pedido';");
        let resolver = ModuleResolver::new(&config(&raiz, Vec::new()));

        assert_eq!(
            resolver.resolve(&raiz.join("src/uso.ts"), "./modelo"),
            Some(raiz.join("src/modelo/index.ts"))
        );
        let _ = std::fs::remove_dir_all(&raiz);
    }

    /// Uma pasta também pode ser o `public_api`, que é o que o Angular gera.
    #[test]
    fn a_folder_can_also_mean_its_public_api() {
        let raiz = projeto("public-api");
        escrever(&raiz.join("libs/core/public_api.ts"), "export * from './a';");
        let resolver = ModuleResolver::new(&config(&raiz, Vec::new()));

        assert_eq!(
            resolver.resolve(&raiz.join("src/uso.ts"), "../libs/core"),
            Some(raiz.join("libs/core/public_api.ts"))
        );
        let _ = std::fs::remove_dir_all(&raiz);
    }

    /// O apelido do `paths` acha o que não existe como pasta.
    ///
    /// `@spartacus/core` não é caminho nenhum no disco — é o `paths` que o
    /// aponta. Sem isto, quase todo `import` de um monorepo fica sem destino.
    #[test]
    fn an_alias_finds_what_is_not_a_folder() {
        let raiz = projeto("apelido");
        escrever(&raiz.join("core-libs/core/public_api.ts"), "export class A {}");
        let resolver = ModuleResolver::new(&config(
            &raiz,
            vec![(
                "@loja/core".to_owned(),
                vec!["core-libs/core/public_api".to_owned()],
            )],
        ));

        assert_eq!(
            resolver.resolve(&raiz.join("src/uso.ts"), "@loja/core"),
            Some(raiz.join("core-libs/core/public_api.ts"))
        );
        let _ = std::fs::remove_dir_all(&raiz);
    }

    /// O apelido com `*` põe o que casou no lugar do `*` do destino.
    #[test]
    fn a_wildcard_alias_substitutes_what_it_matched() {
        let raiz = projeto("curinga");
        escrever(&raiz.join("libs/cart/public_api.ts"), "export class Cart {}");
        let resolver = ModuleResolver::new(&config(
            &raiz,
            vec![("@loja/*".to_owned(), vec!["libs/*/public_api".to_owned()])],
        ));

        assert_eq!(
            resolver.resolve(&raiz.join("src/uso.ts"), "@loja/cart"),
            Some(raiz.join("libs/cart/public_api.ts"))
        );
        // E o que não casa o prefixo não é resolvido pelo apelido.
        assert_eq!(resolver.resolve(&raiz.join("src/uso.ts"), "@outra/cart"), None);
        let _ = std::fs::remove_dir_all(&raiz);
    }

    /// Destinos são tentados em ordem, e o primeiro que existir vence.
    #[test]
    fn alias_targets_are_tried_in_order() {
        let raiz = projeto("ordem");
        escrever(&raiz.join("segundo/core.ts"), "export class A {}");
        let resolver = ModuleResolver::new(&config(
            &raiz,
            vec![(
                "@loja/core".to_owned(),
                vec!["primeiro/core".to_owned(), "segundo/core".to_owned()],
            )],
        ));

        assert_eq!(
            resolver.resolve(&raiz.join("src/uso.ts"), "@loja/core"),
            Some(raiz.join("segundo/core.ts")),
            "o primeiro destino não existe, e o segundo vence"
        );
        let _ = std::fs::remove_dir_all(&raiz);
    }

    /// Um módulo com ponto no nome não perde o pedaço depois do ponto.
    ///
    /// **Foi um defeito de verdade**, achado pela comparação com o analisador:
    /// `with_extension` substitui o que vem depois do último ponto, e
    /// `./pedido.service` virava `pedido.ts` — um arquivo que não existe. E
    /// `.service`, `.model`, `.commands` são o idioma do Angular.
    #[test]
    fn a_module_name_with_dots_keeps_them() {
        let raiz = projeto("com-pontos");
        escrever(&raiz.join("src/pedido.service.ts"), "export class PedidoService {}");
        escrever(&raiz.join("src/uso.ts"), "");
        let resolver = ModuleResolver::new(&config(&raiz, Vec::new()));

        assert_eq!(
            resolver.resolve(&raiz.join("src/uso.ts"), "./pedido.service"),
            Some(raiz.join("src/pedido.service.ts")),
            "o `.service` faz parte do nome do módulo, e não é extensão"
        );
        let _ = std::fs::remove_dir_all(&raiz);
    }

    /// Um `import` que já traz a extensão é aceito como está.
    #[test]
    fn an_import_that_carries_its_extension_is_taken_as_is() {
        let raiz = projeto("com-extensao");
        escrever(&raiz.join("src/pedido.ts"), "export class Pedido {}");
        escrever(&raiz.join("src/uso.ts"), "");
        let resolver = ModuleResolver::new(&config(&raiz, Vec::new()));

        assert_eq!(
            resolver.resolve(&raiz.join("src/uso.ts"), "./pedido.ts"),
            Some(raiz.join("src/pedido.ts"))
        );
        let _ = std::fs::remove_dir_all(&raiz);
    }

    /// Uma dependência **que não está instalada** não é resolvida.
    ///
    /// Dizer `None` é dizer "não alcanço", que é diferente de "não existe" — e é
    /// a distinção que a `25` faz questão de manter.
    ///
    /// *Este teste cobrava outra coisa até a fase 9: que **nenhuma** dependência
    /// fosse resolvida, instalada ou não. Era a fase 1 misturando duas
    /// perguntas — o que entra na busca por nome e o que responde depois do
    /// ponto. A primeira continua sem `node_modules`; a segunda passou a
    /// entrar.*
    #[test]
    fn a_dependency_that_is_not_installed_is_not_resolved() {
        let raiz = projeto("dependencia");
        escrever(&raiz.join("src/uso.ts"), "");
        let resolver = ModuleResolver::new(&config(&raiz, Vec::new()));

        assert_eq!(resolver.resolve(&raiz.join("src/uso.ts"), "@angular/core"), None);
        let _ = std::fs::remove_dir_all(&raiz);
    }

    /// **O projeto vence a dependência instalada.**
    ///
    /// Num monorepo, um apelido do `paths` aponta para o código local que
    /// substitui um pacote publicado — e o pacote também está em
    /// `node_modules`, porque alguma outra dependência o trouxe. Responder com
    /// o instalado mostraria a versão publicada a quem está editando a local:
    /// resposta plausível, e errada.
    #[test]
    fn the_project_outranks_the_installed_package() {
        let raiz = projeto("precedencia");
        escrever(&raiz.join("src/uso.ts"), "");
        escrever(&raiz.join("libs/core/index.ts"), "export class Local {}\n");
        escrever(
            &raiz.join("node_modules/@empresa/core/package.json"),
            "{\"types\": \"./index.d.ts\"}",
        );
        escrever(
            &raiz.join("node_modules/@empresa/core/index.d.ts"),
            "export declare class Publicado {}\n",
        );
        let resolver = ModuleResolver::new(&config(
            &raiz,
            vec![("@empresa/core".to_owned(), vec!["libs/core/index.ts".to_owned()])],
        ));

        let achado = resolver.resolve(&raiz.join("src/uso.ts"), "@empresa/core");
        assert_eq!(
            achado.as_deref().and_then(Path::file_name),
            Some("index.ts".as_ref()),
            "o apelido do projeto manda: {achado:?}"
        );
        let _ = std::fs::remove_dir_all(&raiz);
    }

    /// **E a dependência instalada é alcançada quando o projeto não a
    /// substitui.**
    ///
    /// É o que faltava para a IDE não saber só o que o projeto declara. Medido
    /// na fase 8: numa aplicação Angular, 24 dos 51 elos de cadeia sem resposta
    /// esbarravam exatamente aqui.
    #[test]
    fn an_installed_dependency_is_reached() {
        let raiz = projeto("instalada");
        escrever(&raiz.join("src/uso.ts"), "");
        escrever(
            &raiz.join("node_modules/@angular/forms/package.json"),
            "{\"typings\": \"./types/forms.d.ts\"}",
        );
        escrever(
            &raiz.join("node_modules/@angular/forms/types/forms.d.ts"),
            "export declare class FormBuilder {}\n",
        );
        let resolver = ModuleResolver::new(&config(&raiz, Vec::new()));

        let achado = resolver.resolve(&raiz.join("src/uso.ts"), "@angular/forms");
        assert_eq!(
            achado.as_deref().and_then(Path::file_name),
            Some("forms.d.ts".as_ref()),
            "veio: {achado:?}"
        );
        let _ = std::fs::remove_dir_all(&raiz);
    }

    /// O barril é atravessado até quem declara.
    #[test]
    fn the_barrel_is_walked_until_the_declaration() {
        let raiz = projeto("atravessar");
        escrever(&raiz.join("src/modelo/pedido.ts"), "export class Pedido {}");
        escrever(&raiz.join("src/modelo/index.ts"), "export * from './pedido';");
        escrever(&raiz.join("src/index.ts"), "export * from './modelo';");
        let resolver = ModuleResolver::new(&config(&raiz, Vec::new()));

        let pedido = raiz.join("src/modelo/pedido.ts");
        let exportacoes = |arquivo: &Path| {
            let texto = std::fs::read_to_string(arquivo).unwrap_or_default();
            texto
                .lines()
                .filter_map(|linha| {
                    let de = linha.split_once("from '")?.1.split_once('\'')?.0;
                    Some(Reexportacao {
                        nome: None,
                        origem: None,
                        de: de.to_owned(),
                    })
                })
                .collect()
        };
        let declara = |arquivo: &Path, nome: &str| arquivo == pedido && nome == "Pedido";

        assert_eq!(
            declarante(
                &resolver,
                &raiz.join("src/index.ts"),
                "Pedido",
                &exportacoes,
                &declara,
            ),
            Some(pedido.clone()),
            "dois barris de distância, e ele chega"
        );
        let _ = std::fs::remove_dir_all(&raiz);
    }

    /// Barris em círculo não travam a busca.
    ///
    /// Existem em monorepo grande, e sem a marca de visitado esta função não
    /// terminaria — a IDE ficaria pendurada num arquivo que ninguém suspeita.
    #[test]
    fn barrels_in_a_circle_do_not_hang() {
        let raiz = projeto("ciclo");
        escrever(&raiz.join("src/a.ts"), "export * from './b';");
        escrever(&raiz.join("src/b.ts"), "export * from './a';");
        let resolver = ModuleResolver::new(&config(&raiz, Vec::new()));

        let exportacoes = |arquivo: &Path| {
            let texto = std::fs::read_to_string(arquivo).unwrap_or_default();
            texto
                .lines()
                .filter_map(|linha| {
                    let de = linha.split_once("from '")?.1.split_once('\'')?.0;
                    Some(Reexportacao {
                        nome: None,
                        origem: None,
                        de: de.to_owned(),
                    })
                })
                .collect()
        };
        let nunca = |_: &Path, _: &str| false;

        assert_eq!(
            declarante(&resolver, &raiz.join("src/a.ts"), "Pedido", &exportacoes, &nunca),
            None,
            "o círculo termina, e termina dizendo que não achou"
        );
        let _ = std::fs::remove_dir_all(&raiz);
    }

    /// Uma reexportação nominal só serve ao nome dela.
    ///
    /// `export { A } from './a'` não diz nada sobre `B`, e segui-la para
    /// procurar `B` seria andar por onde a resposta não pode estar.
    #[test]
    fn a_named_re_export_only_serves_its_own_name() {
        let raiz = projeto("nominal");
        escrever(&raiz.join("src/a.ts"), "export class A {}");
        escrever(&raiz.join("src/index.ts"), "export { A } from './a';");
        let resolver = ModuleResolver::new(&config(&raiz, Vec::new()));

        let exportacoes = |_: &Path| {
            vec![Reexportacao {
                nome: Some("A".to_owned()),
                origem: Some("A".to_owned()),
                de: "./a".to_owned(),
            }]
        };
        let declara = |arquivo: &Path, nome: &str| arquivo == raiz.join("src/a.ts") && nome == "A";

        assert_eq!(
            declarante(&resolver, &raiz.join("src/index.ts"), "A", &exportacoes, &declara),
            Some(raiz.join("src/a.ts"))
        );
        assert_eq!(
            declarante(&resolver, &raiz.join("src/index.ts"), "B", &exportacoes, &declara),
            None,
            "a reexportação de `A` não é caminho para `B`"
        );
        let _ = std::fs::remove_dir_all(&raiz);
    }
}
