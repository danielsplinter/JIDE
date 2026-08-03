//! O descritor de plugin entregue ao analisador de TypeScript.
//!
//! É a única coisa que atravessa a fronteira entre as duas crates, e ela vai
//! num sentido só. Ver "Quem depende de quem, e por quê nessa direção" na `24`.

use std::path::{Path, PathBuf};

use language_typescript::{AnalyzerPlugin, AnalyzerPluginSource, CompanionRule};

use crate::project::{e_angular, subindo};

/// O nome do módulo, como o `node_modules` o conhece.
const MODULO: &str = "@angular/language-service";

/// Contribui o serviço de linguagem do Angular ao analisador de TypeScript.
///
/// # De onde vem o pacote
///
/// **Do projeto, quando o projeto o tem.** É a regra da ADR-028, e é a que dá a
/// resposta que bate com o build.
///
/// Quando não tem, do diretório que a IDE carrega. Medido: dos cinco projetos
/// Angular de referência, **um** tem o pacote. Sem a reserva, quatro deles
/// abririam o template sem nada — e a fase seria uma promessa que quase nunca se
/// cumpre.
///
/// A reserva é aceitável porque o **`tsserver` continua sendo o do projeto**, e
/// é ele quem decide se um tipo bate. O que varia é a versão de quem entende o
/// template, e a sintaxe de template é aditiva entre versões maiores. Verificado:
/// um serviço 21.2.17 respondeu por um projeto Angular 21.2.6 sem ressalva.
pub struct AngularAnalyzerPlugin {
    /// O diretório que a IDE carrega, com o pacote dentro de `node_modules`.
    ///
    /// `None` quando a IDE não carrega nenhum: aí só serve projeto que traz o
    /// seu, e os outros abrem o template como HTML puro — que é degradar, e não
    /// recusar.
    reserva: Option<PathBuf>,
}

impl AngularAnalyzerPlugin {
    /// Sem reserva: só serve projeto que traz o pacote.
    #[must_use]
    pub fn new() -> Self {
        Self { reserva: None }
    }

    /// Com a reserva que a IDE carrega.
    #[must_use]
    pub fn with_fallback(reserva: PathBuf) -> Self {
        Self {
            reserva: Some(reserva),
        }
    }

    /// De onde carregar o serviço para este projeto.
    fn origem(&self, workspace_root: &Path) -> Option<PathBuf> {
        if subindo(workspace_root, MODULO).is_some() {
            // O local de sondagem é a raiz, e não o caminho do pacote: é o
            // analisador quem resolve o módulo a partir dali.
            return Some(workspace_root.to_path_buf());
        }
        self.reserva
            .as_ref()
            .filter(|reserva| subindo(reserva, MODULO).is_some())
            .cloned()
    }
}

impl Default for AngularAnalyzerPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl AnalyzerPluginSource for AngularAnalyzerPlugin {
    fn plugin_for(&self, workspace_root: &Path) -> Option<AnalyzerPlugin> {
        // Projeto que não é Angular não carrega o plugin, e a razão é medida: o
        // plugin aproximadamente dobra o tempo de carga do projeto — 30 s viram
        // de 46 a 70 s no monorepo de referência. Cobrar isso de quem não tem
        // template seria cobrar por nada.
        if !e_angular(workspace_root) {
            return None;
        }
        Some(AnalyzerPlugin {
            module: MODULO.to_owned(),
            probe_location: self.origem(workspace_root)?,
            // As chaves são do plugin, e não nossas. `angularOnly: false` o põe
            // ao lado do TypeScript em vez de no lugar dele — sem isso, um `.ts`
            // deixaria de ser respondido, que é o defeito que o `ngserver`
            // carrega por ter esse valor fixo no código.
            configuration: serde_json::json!({
                "angularOnly": false,
                "includeCompletionsWithSnippetText": true,
            }),
        })
    }

    fn companions(&self) -> Vec<CompanionRule> {
        vec![CompanionRule {
            extension: "html".to_owned(),
            anchor_extension: "ts".to_owned(),
        }]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Pasta(PathBuf);

    impl Pasta {
        fn nova(nome: &str) -> Self {
            let caminho = std::env::temp_dir().join(format!("er-ide-plugin-{nome}"));
            let _ = std::fs::remove_dir_all(&caminho);
            assert!(std::fs::create_dir_all(&caminho).is_ok());
            Self(caminho)
        }

        fn com(&self, pacote: &str) -> &Self {
            let mut destino = self.0.join("node_modules");
            for parte in pacote.split('/') {
                destino.push(parte);
            }
            assert!(std::fs::create_dir_all(&destino).is_ok());
            self
        }
    }

    impl Drop for Pasta {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn projeto_que_nao_e_angular_nao_carrega_nada() {
        let pasta = Pasta::nova("sem-angular");
        pasta.com("typescript");
        assert_eq!(AngularAnalyzerPlugin::new().plugin_for(&pasta.0), None);
    }

    #[test]
    fn projeto_com_o_pacote_carrega_do_proprio_projeto() {
        let pasta = Pasta::nova("com-pacote");
        pasta.com("@angular/core").com("@angular/language-service");
        let escolha = AngularAnalyzerPlugin::new()
            .plugin_for(&pasta.0)
            .unwrap_or_else(|| panic!("o plugin deveria ser oferecido"));
        assert_eq!(escolha.module, MODULO);
        assert_eq!(escolha.probe_location, pasta.0);
    }

    /// O caso dos quatro em cinco: é Angular, mas não traz o serviço.
    #[test]
    fn projeto_sem_o_pacote_cai_na_reserva() {
        let projeto = Pasta::nova("sem-pacote");
        projeto.com("@angular/core");
        let reserva = Pasta::nova("reserva");
        reserva.com("@angular/language-service");

        assert_eq!(
            AngularAnalyzerPlugin::new().plugin_for(&projeto.0),
            None,
            "sem reserva não há de onde carregar"
        );
        let escolha = AngularAnalyzerPlugin::with_fallback(reserva.0.clone())
            .plugin_for(&projeto.0)
            .unwrap_or_else(|| panic!("com reserva o plugin deveria ser oferecido"));
        assert_eq!(escolha.probe_location, reserva.0);
    }

    /// O do projeto vence o nosso. É a ADR-028, e é o que faz a resposta bater
    /// com o build.
    #[test]
    fn o_do_projeto_vence_a_reserva() {
        let projeto = Pasta::nova("prefere-o-proprio");
        projeto.com("@angular/core").com("@angular/language-service");
        let reserva = Pasta::nova("reserva-ignorada");
        reserva.com("@angular/language-service");

        let escolha = AngularAnalyzerPlugin::with_fallback(reserva.0.clone())
            .plugin_for(&projeto.0)
            .unwrap_or_else(|| panic!("o plugin deveria ser oferecido"));
        assert_eq!(escolha.probe_location, projeto.0);
    }

    /// Reserva apontando para um diretório sem o pacote é o mesmo que não ter
    /// reserva — e não um caminho quebrado entregue ao processo.
    #[test]
    fn reserva_vazia_nao_e_reserva() {
        let projeto = Pasta::nova("reserva-vazia-projeto");
        projeto.com("@angular/core");
        let reserva = Pasta::nova("reserva-vazia");

        assert_eq!(
            AngularAnalyzerPlugin::with_fallback(reserva.0.clone()).plugin_for(&projeto.0),
            None
        );
    }

    #[test]
    fn o_template_e_ancorado_pelo_componente() {
        let regras = AngularAnalyzerPlugin::new().companions();
        assert_eq!(regras.len(), 1);
        assert_eq!(regras[0].extension, "html");
        assert_eq!(regras[0].anchor_extension, "ts");
    }

    /// `angularOnly: false` é o que mantém o `.ts` respondido. O `ngserver` o
    /// tem fixo em `true` e por isso não substitui o analisador; aqui é escolha,
    /// e a escolha é a outra.
    #[test]
    fn o_plugin_fica_ao_lado_do_typescript_e_nao_no_lugar() {
        let pasta = Pasta::nova("ao-lado");
        pasta.com("@angular/core").com("@angular/language-service");
        let escolha = AngularAnalyzerPlugin::new()
            .plugin_for(&pasta.0)
            .unwrap_or_else(|| panic!("o plugin deveria ser oferecido"));
        assert_eq!(escolha.configuration["angularOnly"], false);
    }
}
