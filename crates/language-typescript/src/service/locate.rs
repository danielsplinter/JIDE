//! Onde estão o Node e o analisador **deste** projeto.
//!
//! São duas dependências separadas, e confundi-las leva a erro de diagnóstico:
//! o Node executa JavaScript, e o `tsserver` vem com o pacote `typescript` do
//! projeto. Faltando qualquer um dos dois, o provider externo não sobe e o
//! nativo responde — degradar, e não recusar. Ver a fase 3c da `23`.

use std::path::{Path, PathBuf};

use crate::toolchain::node_executable;

/// O que é preciso para subir o analisador.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Location {
    pub(crate) node: PathBuf,
    pub(crate) tsserver: PathBuf,
}

/// Por que o analisador externo não pode subir.
///
/// Cada motivo tem um texto próprio porque a saída é diferente: sem Node, quem
/// usa instala o Node; sem `node_modules`, quem usa roda `npm install`. Um
/// "falhou" só mandaria adivinhar.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Missing {
    Node,
    TypeScriptPackage,
}

impl std::fmt::Display for Missing {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Node => formatter.write_str(
                "Node não foi encontrado — aponte a instalação em Configurações ou instale-o",
            ),
            Self::TypeScriptPackage => formatter.write_str(
                "o projeto não tem o pacote typescript em node_modules — rode `npm install`",
            ),
        }
    }
}

/// Procura o analisador que **o projeto** fixa.
///
/// Nunca o nosso: a versão do TypeScript é a do projeto, e usar outra daria
/// respostas que não batem com o build. É a regra da `23`, e ela é o motivo de
/// não haver um `typescript` embutido na IDE para servir de reserva.
pub(crate) fn locate(workspace_root: &Path, node_home: Option<&Path>) -> Result<Location, Missing> {
    let node = node_home
        .map(node_executable)
        .filter(|caminho| caminho.is_file())
        .or_else(|| ide_process::find_in_path(NODE_EXECUTABLE))
        .ok_or(Missing::Node)?;
    let tsserver = tsserver_in(workspace_root).ok_or(Missing::TypeScriptPackage)?;
    Ok(Location { node, tsserver })
}

/// O `tsserver.js` do projeto, ou de um pacote acima dele.
///
/// Subir a árvore é o que faz funcionar em monorepo, onde o `node_modules` fica
/// na raiz e os pacotes ficam abaixo. Parar na primeira ocorrência é o mesmo que
/// o Node faz para resolver módulo.
pub fn tsserver_in(workspace_root: &Path) -> Option<PathBuf> {
    let mut atual = Some(workspace_root);
    while let Some(diretorio) = atual {
        let candidato = diretorio
            .join("node_modules")
            .join("typescript")
            .join("lib")
            .join("tsserver.js");
        if candidato.is_file() {
            return Some(candidato);
        }
        atual = diretorio.parent();
    }
    None
}

#[cfg(windows)]
const NODE_EXECUTABLE: &str = "node.exe";
#[cfg(not(windows))]
const NODE_EXECUTABLE: &str = "node";
