//! Detecção e validação de instalações JDK.

use std::{
    collections::HashSet,
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

use async_trait::async_trait;
use ide_domain::LanguageId;
use ide_toolchain_api::{
    DetectionContext, ToolchainError, ToolchainId, ToolchainInstallation, ToolchainProvider,
    ToolchainValidation,
};

pub const JAVA_TOOLCHAIN_ID: &str = "java-jdk";

#[derive(Default)]
pub struct JavaToolchainProvider;

impl JavaToolchainProvider {
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    pub fn detect_from_candidates(
        candidates: impl IntoIterator<Item = PathBuf>,
    ) -> Vec<ToolchainInstallation> {
        let mut seen = HashSet::new();
        let mut installations = candidates
            .into_iter()
            .filter_map(|home| {
                let normalized = fs::canonicalize(&home).unwrap_or(home);
                if !seen.insert(normalized.clone()) || !is_jdk_home(&normalized) {
                    return None;
                }
                Some(ToolchainInstallation {
                    id: ToolchainId(format!("java:{}", normalized.to_string_lossy())),
                    version: java_version(&normalized),
                    home: normalized,
                })
            })
            .collect::<Vec<_>>();
        installations.sort_by(|left, right| {
            right
                .version
                .cmp(&left.version)
                .then(left.home.cmp(&right.home))
        });
        installations
    }

    pub fn installation_from_home(
        home: impl Into<PathBuf>,
    ) -> Result<ToolchainInstallation, ToolchainError> {
        Self::detect_from_candidates([home.into()])
            .into_iter()
            .next()
            .ok_or(ToolchainError::NotFound)
    }
}

#[async_trait]
impl ToolchainProvider for JavaToolchainProvider {
    fn toolchain_id(&self) -> ToolchainId {
        ToolchainId(JAVA_TOOLCHAIN_ID.to_owned())
    }

    fn supported_languages(&self) -> Vec<LanguageId> {
        vec![LanguageId("java".to_owned())]
    }

    async fn detect(
        &self,
        context: DetectionContext,
    ) -> Result<Vec<ToolchainInstallation>, ToolchainError> {
        Ok(Self::detect_from_candidates(detection_candidates(&context)))
    }

    async fn validate(
        &self,
        installation: &ToolchainInstallation,
    ) -> Result<ToolchainValidation, ToolchainError> {
        let mut details = Vec::new();
        for executable in ["java", "javac", "jar"] {
            let path = jdk_executable(&installation.home, executable);
            details.push(format!(
                "{}: {}",
                executable,
                if path.is_file() { "ok" } else { "missing" }
            ));
        }
        let valid = is_jdk_home(&installation.home);
        if let Some(version) = java_version(&installation.home) {
            details.push(format!("version: {version}"));
        }
        Ok(ToolchainValidation { valid, details })
    }

    async fn resolve_installation(
        &self,
        home: PathBuf,
    ) -> Result<ToolchainInstallation, ToolchainError> {
        Self::installation_from_home(home)
    }
}

#[must_use]
pub fn jdk_executable(home: &Path, executable: &str) -> PathBuf {
    let name = if cfg!(windows) {
        format!("{executable}.exe")
    } else {
        executable.to_owned()
    };
    home.join("bin").join(name)
}

fn is_jdk_home(home: &Path) -> bool {
    ["java", "javac", "jar"]
        .iter()
        .all(|executable| jdk_executable(home, executable).is_file())
}

fn detection_candidates(context: &DetectionContext) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    for variable in ["JAVA_HOME", "JDK_HOME"] {
        if let Some(home) = env::var_os(variable) {
            candidates.push(PathBuf::from(home));
        }
    }
    if let Some(workspace) = &context.workspace_root {
        candidates.push(workspace.join(".jdk"));
        candidates.push(workspace.join("jdk"));
    }
    if let Some(was_home) = env::var_os("WAS_HOME") {
        let home = PathBuf::from(was_home);
        candidates.push(home.join("java"));
        candidates.push(home.join("java").join("8.0"));
    }
    if let Some(path) = env::var_os("PATH") {
        for directory in env::split_paths(&path) {
            let java = directory.join(if cfg!(windows) { "java.exe" } else { "java" });
            if java.is_file()
                && let Some(home) = directory.parent()
            {
                candidates.push(home.to_path_buf());
            }
        }
    }
    for root in platform_jdk_roots() {
        collect_jdk_homes(&root, &mut candidates, 4);
    }
    candidates
}

fn platform_jdk_roots() -> Vec<PathBuf> {
    if cfg!(windows) {
        vec![
            PathBuf::from(r"C:\Program Files\Java"),
            PathBuf::from(r"C:\Program Files\Eclipse Adoptium"),
            PathBuf::from(r"C:\Program Files\Microsoft"),
            PathBuf::from(r"C:\Program Files\IBM"),
            PathBuf::from(r"C:\IBM\WebSphere\AppServer\java"),
        ]
    } else if cfg!(target_os = "macos") {
        vec![PathBuf::from("/Library/Java/JavaVirtualMachines")]
    } else {
        vec![PathBuf::from("/usr/lib/jvm")]
    }
}

fn collect_jdk_homes(root: &Path, output: &mut Vec<PathBuf>, depth: usize) {
    if depth == 0 || !root.is_dir() {
        return;
    }
    if is_jdk_home(root) {
        output.push(root.to_path_buf());
        return;
    }
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten().take(128) {
        let path = entry.path();
        if path.is_dir() {
            if path.ends_with("Contents/Home") {
                output.push(path);
            } else {
                collect_jdk_homes(&path, output, depth - 1);
            }
        }
    }
}

fn java_version(home: &Path) -> Option<String> {
    let output = Command::new(jdk_executable(home, "java"))
        .arg("-version")
        .output()
        .ok()?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    text.lines().next().map(str::trim).map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_jdk(name: &str) -> PathBuf {
        let root = env::temp_dir().join(format!("er-ide-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let bin = root.join("bin");
        assert!(fs::create_dir_all(&bin).is_ok());
        for executable in ["java", "javac", "jar"] {
            assert!(fs::write(jdk_executable(&root, executable), []).is_ok());
        }
        root
    }

    #[test]
    fn detects_valid_jdk_and_allows_manual_home() {
        let first = fake_jdk("jdk-8");
        let second = fake_jdk("jdk-17");
        let installations =
            JavaToolchainProvider::detect_from_candidates([first.clone(), second.clone()]);
        assert_eq!(installations.len(), 2);
        assert!(JavaToolchainProvider::installation_from_home(&second).is_ok());
        let _ = fs::remove_dir_all(first);
        let _ = fs::remove_dir_all(second);
    }
}
