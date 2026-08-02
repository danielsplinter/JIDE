#![doc = "Integração com os sistemas de build de Java."]

mod gradle;
mod maven;

pub use gradle::{GRADLE_BUILD_SYSTEM_ID, GradleAdapter};
pub use maven::{MAVEN_BUILD_SYSTEM_ID, MavenAdapter, MavenInstallation};

// Qualificados no nome porque saíram de uma crate cujo nome já dizia "maven".
// Sem isso, `language_java::detect_installations` não diz de quê.
pub use maven::detect_installations as detect_maven_installations;
pub use maven::installation_from_home as maven_installation_from_home;
