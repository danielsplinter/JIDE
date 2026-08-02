#![doc = "Detecção, classpath e adapters da toolchain Java."]
#![doc = ""]
#![doc = "A superfície é a que a raiz de composição usa, e nada além. Enquanto isto"]
#![doc = "era uma crate, tudo o que os módulos vizinhos precisavam tinha de ser"]
#![doc = "`pub` — e o que sobrava não aparecia como sobra."]

mod adapter;
mod classpath;
mod detection;

pub use adapter::JavaToolchainAdapter;
pub use classpath::ClasspathBuilder;
pub use detection::JavaToolchainProvider;
