#![doc = "Tudo o que é exclusivo de Java: análise, toolchain, build e depuração."]
#![doc = ""]
#![doc = "Uma crate por linguagem, com as capacidades em módulos — fase 8 da `12`."]
#![doc = "Esta fachada é a superfície inteira: o que não está aqui não é alcançável"]
#![doc = "de fora, e a raiz de composição é a única consumidora."]

mod analyzer;
mod build;
mod debug;
mod toolchain;

pub use analyzer::{JAVA_LANGUAGE_ID, JAVA_PROVIDER_ID, JavaLanguageProvider};
pub use build::{
    GRADLE_BUILD_SYSTEM_ID, GradleAdapter, MAVEN_BUILD_SYSTEM_ID, MavenAdapter, MavenInstallation,
    detect_maven_installations, maven_installation_from_home,
};
pub use debug::JavaDebugAdapter;
pub use toolchain::{ClasspathBuilder, JavaToolchainAdapter, JavaToolchainProvider};
