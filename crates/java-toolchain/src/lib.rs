#![doc = "Detecção, seleção, classpath e adapters da toolchain Java."]

pub mod adapter;
pub mod classpath;
pub mod detection;
pub mod selection;

pub use adapter::JavaToolchainAdapter;
pub use classpath::ClasspathBuilder;
pub use detection::{JAVA_TOOLCHAIN_ID, JavaToolchainProvider, jdk_executable};
pub use selection::JavaToolchainSelection;
