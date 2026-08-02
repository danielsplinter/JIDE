//! Descoberta das instalações do Maven disponíveis na máquina.
//!
//! O adaptador de build sempre soube **achar** um `mvn` para executar; o que
//! faltava era oferecer a escolha ao usuário. São coisas diferentes: achar
//! qualquer um serve para rodar, escolher exige mostrar quais existem e qual é
//! qual.

use std::path::{Path, PathBuf};

use ide_process::find_in_path;

/// Uma instalação do Maven encontrada na máquina.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MavenInstallation {
    /// Diretório raiz, aquele que contém `bin` e `lib`.
    pub home: PathBuf,
    /// Versão, quando pôde ser lida sem executar nada.
    pub version: Option<String>,
}

impl MavenInstallation {
    /// Como a instalação aparece numa lista de escolha.
    #[must_use]
    pub fn label(&self) -> String {
        match &self.version {
            Some(version) => format!("Maven {version} — {}", self.home.display()),
            None => self.home.display().to_string(),
        }
    }
}

/// Instalações encontradas, sem repetição e em ordem estável.
///
/// A ordem é a da procura — variáveis de ambiente, `PATH` e os diretórios
/// convencionais —, porque ela reflete o que a máquina já usaria: o primeiro da
/// lista é o que rodaria hoje sem escolha nenhuma.
#[must_use]
pub fn detect_installations() -> Vec<MavenInstallation> {
    let mut encontradas = Vec::new();
    let mut candidatos: Vec<PathBuf> = Vec::new();
    for variavel in ["MAVEN_HOME", "M2_HOME"] {
        if let Some(valor) = std::env::var_os(variavel) {
            candidatos.push(PathBuf::from(valor));
        }
    }
    // O `mvn` do `PATH` aponta para `<home>/bin/mvn`: a casa é a avó dele.
    if let Some(executavel) = find_in_path(executable_name())
        && let Some(home) = executavel.parent().and_then(Path::parent)
    {
        candidatos.push(home.to_path_buf());
    }
    candidatos.extend(conventional_directories());

    for candidato in candidatos {
        let Some(instalacao) = installation_from_home(&candidato) else {
            continue;
        };
        if !encontradas
            .iter()
            .any(|outra: &MavenInstallation| outra.home == instalacao.home)
        {
            encontradas.push(instalacao);
        }
    }
    encontradas
}

/// Lê uma instalação a partir do diretório, se houver uma ali.
///
/// O que caracteriza a casa do Maven é o executável em `bin`: um diretório
/// escolhido à mão que não o tenha não é uma instalação, e aceitar assim mesmo
/// só adiaria o erro para a hora de compilar.
#[must_use]
pub fn installation_from_home(home: &Path) -> Option<MavenInstallation> {
    let raiz = if home.join("bin").join(executable_name()).is_file() {
        home.to_path_buf()
    } else if home.file_name().is_some_and(|nome| nome == "bin")
        && home.join(executable_name()).is_file()
    {
        // Quem escolhe pelo seletor costuma parar em `bin`; subir um nível é o
        // que ele queria dizer.
        home.parent()?.to_path_buf()
    } else {
        return None;
    };
    let version = version_from_lib(&raiz);
    Some(MavenInstallation {
        home: raiz,
        version,
    })
}

/// Versão lida pelo nome do jar do núcleo, sem executar o Maven.
///
/// `mvn -v` daria a resposta exata, mas custa um processo e uma JVM só para
/// preencher uma lista. O nome de `lib/maven-core-<versão>.jar` diz o mesmo.
fn version_from_lib(home: &Path) -> Option<String> {
    let entradas = std::fs::read_dir(home.join("lib")).ok()?;
    for entrada in entradas.flatten() {
        let nome = entrada.file_name();
        let nome = nome.to_str()?;
        if let Some(resto) = nome.strip_prefix("maven-core-")
            && let Some(versao) = resto.strip_suffix(".jar")
        {
            return Some(versao.to_owned());
        }
    }
    None
}

const fn executable_name() -> &'static str {
    if cfg!(windows) { "mvn.cmd" } else { "mvn" }
}

/// Diretórios em que uma instalação costuma estar, por plataforma.
fn conventional_directories() -> Vec<PathBuf> {
    let mut caminhos = Vec::new();
    if cfg!(windows) {
        for base in ["C:\\Program Files\\Apache\\Maven", "C:\\apache-maven"] {
            caminhos.extend(subdirectories(Path::new(base)));
            caminhos.push(PathBuf::from(base));
        }
    } else {
        for base in ["/usr/share/maven", "/opt/maven", "/usr/local/maven"] {
            caminhos.extend(subdirectories(Path::new(base)));
            caminhos.push(PathBuf::from(base));
        }
    }
    caminhos
}

fn subdirectories(base: &Path) -> Vec<PathBuf> {
    let Ok(entradas) = std::fs::read_dir(base) else {
        return Vec::new();
    };
    entradas
        .flatten()
        .map(|entrada| entrada.path())
        .filter(|caminho| caminho.is_dir())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Um diretório vira instalação quando tem o executável; senão, não vira.
    #[test]
    fn a_home_is_an_installation_only_when_it_has_the_executable() {
        let raiz = std::env::temp_dir().join(format!("er-maven-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&raiz);
        assert!(std::fs::create_dir_all(raiz.join("bin")).is_ok());
        assert!(std::fs::create_dir_all(raiz.join("lib")).is_ok());

        assert!(
            installation_from_home(&raiz).is_none(),
            "sem o executável não é instalação"
        );

        assert!(std::fs::write(raiz.join("bin").join(executable_name()), "").is_ok());
        assert!(std::fs::write(raiz.join("lib").join("maven-core-3.9.6.jar"), "").is_ok());
        let Some(instalacao) = installation_from_home(&raiz) else {
            panic!("com o executável precisa ser instalação");
        };
        assert_eq!(instalacao.home, raiz);
        assert_eq!(instalacao.version.as_deref(), Some("3.9.6"));
        assert!(instalacao.label().starts_with("Maven 3.9.6"));

        // Escolher a pasta `bin` no seletor vale como escolher a casa.
        let Some(pelo_bin) = installation_from_home(&raiz.join("bin")) else {
            panic!("escolher `bin` precisa subir um nível");
        };
        assert_eq!(pelo_bin.home, raiz);

        let _ = std::fs::remove_dir_all(&raiz);
    }
}
