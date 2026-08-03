//! Plugins do analisador, contribuídos de fora.
//!
//! # Por que isto existe, e por que é genérico
//!
//! O `tsserver` carrega plugins no **próprio processo**, no mesmo grafo de
//! tipos. É como o suporte a um framework que vive dentro de TypeScript —
//! Angular, Vue — responde por arquivos que não são código: o plugin vê o
//! programa inteiro, e nós não precisamos ver nada.
//!
//! Esta crate não sabe de nenhum framework, e não vai saber. Ela recebe um
//! [`AnalyzerPlugin`] — nome de módulo, onde procurá-lo, o que dizer a ele — e
//! entrega os três ao processo. Quem sabe o que aquilo significa é quem
//! contribuiu.
//!
//! Não é ponto de extensão inventado para salvar o desenho: o `tsconfig.json`
//! já tem um `plugins` genérico, e é assim que o próprio VS Code os expõe. Ver a
//! seção "Quem depende de quem, e por quê nessa direção" da `24`.

use std::path::{Path, PathBuf};

/// Um plugin a carregar no analisador.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AnalyzerPlugin {
    /// O nome do módulo, como o `node_modules` o conhece.
    ///
    /// É por ele que o plugin é carregado e é por ele que se fala com ele
    /// depois — o `configurePlugin` endereça pelo nome.
    pub module: String,
    /// O diretório a partir do qual procurar o módulo.
    ///
    /// Pode ser a raiz do projeto, quando o projeto traz o pacote, ou um
    /// diretório nosso, quando não traz. Esta crate não decide qual: ela recebe
    /// o caminho pronto.
    pub probe_location: PathBuf,
    /// O que dizer ao plugin depois que ele sobe.
    ///
    /// Opaco de propósito. Cada plugin tem as suas chaves, e traduzi-las aqui
    /// seria conhecer o assunto deles.
    pub configuration: serde_json::Value,
}

/// Um arquivo que não é código por si, e o irmão que o ancora a um projeto.
///
/// # O problema que esta regra resolve
///
/// O `tsserver` só conhece projeto por arquivo de código. Um arquivo que não é
/// código — um template, uma folha de componente — aberto sozinho cai num
/// **projeto inferido**: sem `tsconfig`, sem o resto do programa, sem nada com
/// que responder. Medido: `projectInfo` sobre um template devolve
/// `/dev/null/inferredProject1*`, e toda pergunta sobre ele volta vazia.
///
/// O que resolve é dizer de qual projeto ele é, e quem sabe isso é o irmão de
/// mesmo nome. A regra é **dados**, e não um retorno de chamada, para que esta
/// crate possa aplicá-la sem saber de que assunto ela é.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompanionRule {
    /// A extensão do arquivo que não é código por si.
    pub extension: String,
    /// A extensão do irmão de mesmo nome que o ancora.
    pub anchor_extension: String,
}

impl CompanionRule {
    /// O irmão que ancora este caminho, se a regra se aplicar a ele.
    ///
    /// Troca a extensão e nada mais: o irmão é o de mesmo nome, na mesma pasta.
    /// Se ele não existe no disco, não há âncora — e um arquivo que não é
    /// template de ninguém não recebe nada, que é o critério da `24`.
    #[must_use]
    pub fn anchor_of(&self, path: &Path) -> Option<PathBuf> {
        let extensao = path.extension()?.to_str()?;
        if !extensao.eq_ignore_ascii_case(&self.extension) {
            return None;
        }
        let irmao = path.with_extension(&self.anchor_extension);
        irmao.is_file().then_some(irmao)
    }
}

/// Quem oferece plugins para um projeto.
///
/// A pergunta é feita **por projeto**, e não uma vez só, porque a resposta
/// depende do que há no disco: o mesmo binário abre um projeto que precisa do
/// plugin e outro que não, e não pode carregá-lo nos dois.
pub trait AnalyzerPluginSource: Send + Sync {
    /// O plugin que este contribuinte oferece a **este** projeto, se oferecer.
    ///
    /// `None` é a resposta normal, e não uma falha: a maioria dos projetos não
    /// é do assunto de quem responde.
    fn plugin_for(&self, workspace_root: &Path) -> Option<AnalyzerPlugin>;

    /// Os arquivos que este contribuinte faz o analisador responder.
    ///
    /// Não depende de projeto — a forma da regra é fixa, e só a carga do plugin
    /// é que varia. Precisa ser assim porque as extensões entram nos **metadados
    /// do provider**, que são anunciados uma vez, antes de haver projeto.
    fn companions(&self) -> Vec<CompanionRule> {
        Vec::new()
    }
}

/// Os argumentos de linha de comando que carregam estes plugins.
///
/// Vazio quando não há plugin, e é o caso comum — um projeto sem framework não
/// paga nada por este mecanismo existir.
///
/// # Por que os nomes vão juntos e os caminhos também
///
/// O `tsserver` recebe `--globalPlugins` e `--pluginProbeLocations` como listas
/// separadas por vírgula, e não como pares. Ele procura **cada** módulo em
/// **cada** local, o que é frouxo e é o que ele oferece.
pub(crate) fn plugin_arguments(plugins: &[AnalyzerPlugin]) -> Vec<String> {
    if plugins.is_empty() {
        return Vec::new();
    }
    let modulos = plugins
        .iter()
        .map(|plugin| plugin.module.as_str())
        .collect::<Vec<_>>()
        .join(",");
    let locais = plugins
        .iter()
        .map(|plugin| plugin.probe_location.to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join(",");
    vec![
        "--globalPlugins".to_owned(),
        modulos,
        "--pluginProbeLocations".to_owned(),
        locais,
    ]
}

/// O pedido que entrega a configuração a um plugin já carregado.
///
/// Carregar não basta: um plugin sobe inerte e passa a servir depois de ser
/// configurado. É o que o cliente do editor faz, e sem isso ele fica lá sem
/// atender.
pub(crate) fn configure_arguments(plugin: &AnalyzerPlugin) -> serde_json::Value {
    serde_json::json!({
        "pluginName": plugin.module,
        "configuration": plugin.configuration,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plugin(module: &str, local: &str) -> AnalyzerPlugin {
        AnalyzerPlugin {
            module: module.to_owned(),
            probe_location: PathBuf::from(local),
            configuration: serde_json::json!({}),
        }
    }

    #[test]
    fn sem_plugin_nao_ha_argumento() {
        assert!(plugin_arguments(&[]).is_empty());
    }

    #[test]
    fn um_plugin_vira_nome_e_local() {
        assert_eq!(
            plugin_arguments(&[plugin("@algum/servico", "/projeto")]),
            vec![
                "--globalPlugins".to_owned(),
                "@algum/servico".to_owned(),
                "--pluginProbeLocations".to_owned(),
                "/projeto".to_owned(),
            ]
        );
    }

    /// Dois plugins não viram quatro argumentos: viram duas listas.
    #[test]
    fn dois_plugins_viram_duas_listas() {
        let argumentos = plugin_arguments(&[
            plugin("@um/servico", "/projeto"),
            plugin("@outro/servico", "/nosso"),
        ]);
        assert_eq!(argumentos.len(), 4, "{argumentos:?}");
        assert_eq!(argumentos[1], "@um/servico,@outro/servico");
        assert_eq!(argumentos[3], "/projeto,/nosso");
    }

    #[test]
    fn a_ancora_e_o_irmao_de_mesmo_nome() {
        let pasta = std::env::temp_dir().join("er-ide-teste-ancora");
        assert!(std::fs::create_dir_all(&pasta).is_ok());
        let irmao = pasta.join("hud.component.ts");
        assert!(std::fs::write(&irmao, "export class Hud {}").is_ok());
        let regra = CompanionRule {
            extension: "html".to_owned(),
            anchor_extension: "ts".to_owned(),
        };

        assert_eq!(
            regra.anchor_of(&pasta.join("hud.component.html")),
            Some(irmao)
        );
        let _ = std::fs::remove_dir_all(&pasta);
    }

    /// Sem irmão no disco não há âncora — e um `.html` que não é template de
    /// ninguém não recebe nada, que é o critério da `24`.
    #[test]
    fn sem_irmao_nao_ha_ancora() {
        let regra = CompanionRule {
            extension: "html".to_owned(),
            anchor_extension: "ts".to_owned(),
        };
        let sozinho = std::env::temp_dir().join("er-ide-nao-existe-mesmo.html");
        assert_eq!(regra.anchor_of(&sozinho), None);
    }

    /// Outra extensão não é assunto desta regra, mesmo que o irmão exista.
    #[test]
    fn outra_extensao_nao_e_assunto_da_regra() {
        let regra = CompanionRule {
            extension: "html".to_owned(),
            anchor_extension: "ts".to_owned(),
        };
        assert_eq!(regra.anchor_of(Path::new("/algum/lugar/codigo.ts")), None);
        assert_eq!(regra.anchor_of(Path::new("/algum/lugar/sem-extensao")), None);
    }

    /// A configuração vai como veio. Traduzi-la seria conhecer o assunto do
    /// plugin, que é justamente o que esta crate não faz.
    #[test]
    fn a_configuracao_atravessa_intacta() {
        let mut escolha = plugin("@algum/servico", "/projeto");
        escolha.configuration = serde_json::json!({ "chaveDeles": true, "numero": 3 });
        let pedido = configure_arguments(&escolha);
        assert_eq!(pedido["pluginName"], "@algum/servico");
        assert_eq!(pedido["configuration"]["chaveDeles"], true);
        assert_eq!(pedido["configuration"]["numero"], 3);
    }
}
