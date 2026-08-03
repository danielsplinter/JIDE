//! O que faz um projeto ser Angular.

use std::path::{Path, PathBuf};

/// O pacote cuja presença define o assunto.
///
/// Não é `@angular/cli` nem `angular.json`: uma biblioteca publicada não tem
/// nenhum dos dois e ainda assim tem componentes com template. O que todo
/// projeto Angular tem instalado é o núcleo.
const NUCLEO: &str = "@angular/core";

/// Este projeto é Angular?
///
/// A resposta vem do disco, e não de configuração: quem abre a IDE não deve ter
/// de declarar o óbvio, e uma configuração que discordasse do `node_modules`
/// daria respostas de uma versão que não é a que roda.
#[must_use]
pub fn e_angular(workspace_root: &Path) -> bool {
    subindo(workspace_root, NUCLEO).is_some()
}

/// Onde está um pacote, subindo a árvore a partir da raiz.
///
/// Subir é o que faz funcionar em monorepo, onde o `node_modules` fica na raiz e
/// os pacotes ficam abaixo. É o mesmo caminho que o Node faz para resolver
/// módulo, e o mesmo que `tsserver_in` já faz para achar o analisador.
pub(crate) fn subindo(workspace_root: &Path, pacote: &str) -> Option<PathBuf> {
    let mut atual = Some(workspace_root);
    while let Some(diretorio) = atual {
        let mut candidato = diretorio.join("node_modules");
        for parte in pacote.split('/') {
            candidato.push(parte);
        }
        if candidato.is_dir() {
            return Some(candidato);
        }
        atual = diretorio.parent();
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Pasta(PathBuf);

    impl Pasta {
        fn nova(nome: &str) -> Self {
            let caminho = std::env::temp_dir().join(format!("er-ide-angular-{nome}"));
            let _ = std::fs::remove_dir_all(&caminho);
            assert!(std::fs::create_dir_all(&caminho).is_ok());
            Self(caminho)
        }

        fn pacote(&self, relativo: &str, nome: &str) -> &Self {
            let mut destino = self.0.join(relativo).join("node_modules");
            for parte in nome.split('/') {
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
    fn um_projeto_com_o_nucleo_e_angular() {
        let pasta = Pasta::nova("com-nucleo");
        pasta.pacote(".", "@angular/core");
        assert!(e_angular(&pasta.0));
    }

    #[test]
    fn um_projeto_de_typescript_sem_angular_nao_e() {
        let pasta = Pasta::nova("sem-nucleo");
        pasta.pacote(".", "typescript");
        assert!(!e_angular(&pasta.0));
    }

    /// Num monorepo o `node_modules` fica na raiz, e o pacote que se abre fica
    /// abaixo. Parar no primeiro nível diria que ele não é Angular.
    #[test]
    fn em_monorepo_o_nucleo_acima_conta() {
        let pasta = Pasta::nova("monorepo");
        pasta.pacote(".", "@angular/core");
        let abaixo = pasta.0.join("projects").join("uma-lib");
        assert!(std::fs::create_dir_all(&abaixo).is_ok());
        assert!(e_angular(&abaixo));
    }

    /// Sem `node_modules` nenhum, subir a árvore não pode achar coisa alheia
    /// nem entrar em laço.
    #[test]
    fn sem_pacote_nenhum_a_subida_termina() {
        let pasta = Pasta::nova("vazio");
        assert!(!e_angular(&pasta.0));
    }
}
