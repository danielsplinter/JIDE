#![doc = "Detecção, validação, seleção e classpath de toolchains Java externas."]

use std::{
    collections::HashSet,
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

use async_trait::async_trait;
use ide_domain::LanguageId;
use ide_toolchain_api::{
    Classpath, DetectionContext, ToolchainError, ToolchainId, ToolchainInstallation,
    ToolchainProvider, ToolchainValidation,
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
}

#[derive(Clone, Debug, Default)]
pub struct JavaToolchainSelection {
    installations: Vec<ToolchainInstallation>,
    selected: Option<ToolchainId>,
}

impl JavaToolchainSelection {
    #[must_use]
    pub fn new(installations: Vec<ToolchainInstallation>) -> Self {
        let selected = installations
            .first()
            .map(|installation| installation.id.clone());
        Self {
            installations,
            selected,
        }
    }

    pub fn select(&mut self, id: &ToolchainId) -> Result<(), ToolchainError> {
        if !self
            .installations
            .iter()
            .any(|installation| &installation.id == id)
        {
            return Err(ToolchainError::NotFound);
        }
        self.selected = Some(id.clone());
        Ok(())
    }

    pub fn add_and_select(&mut self, installation: ToolchainInstallation) {
        if let Some(existing) = self
            .installations
            .iter_mut()
            .find(|existing| existing.id == installation.id)
        {
            *existing = installation.clone();
        } else {
            self.installations.push(installation.clone());
        }
        self.selected = Some(installation.id);
    }

    #[must_use]
    pub fn selected(&self) -> Option<&ToolchainInstallation> {
        let selected = self.selected.as_ref()?;
        self.installations
            .iter()
            .find(|installation| &installation.id == selected)
    }

    #[must_use]
    pub fn installations(&self) -> &[ToolchainInstallation] {
        &self.installations
    }

    pub fn select_next(&mut self) -> Option<&ToolchainInstallation> {
        if self.installations.is_empty() {
            self.selected = None;
            return None;
        }
        let current = self.selected.as_ref().and_then(|selected| {
            self.installations
                .iter()
                .position(|installation| &installation.id == selected)
        });
        let next = current.map_or(0, |index| (index + 1) % self.installations.len());
        self.selected = Some(self.installations[next].id.clone());
        self.installations.get(next)
    }
}

#[derive(Clone, Debug, Default)]
pub struct ClasspathBuilder {
    entries: Vec<PathBuf>,
}

impl ClasspathBuilder {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn with_entry(mut self, entry: impl Into<PathBuf>) -> Self {
        let entry = entry.into();
        if entry.exists() && !self.entries.contains(&entry) {
            self.entries.push(entry);
        }
        self
    }

    #[must_use]
    pub fn workspace_defaults(mut self, root: &Path, output_directory: &Path) -> Self {
        self = self.with_entry(output_directory);
        for directory in [
            root.join("lib"),
            root.join("libs"),
            root.join("target").join("classes"),
            root.join("build").join("classes").join("java").join("main"),
        ] {
            self = self.with_entry(&directory);
            if let Ok(entries) = fs::read_dir(directory) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path
                        .extension()
                        .and_then(|extension| extension.to_str())
                        .is_some_and(|extension| extension.eq_ignore_ascii_case("jar"))
                    {
                        self = self.with_entry(path);
                    }
                }
            }
        }
        self
    }

    #[must_use]
    pub fn build(self) -> Classpath {
        Classpath {
            entries: self.entries,
        }
    }
}

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
    fn detects_valid_jdk_and_allows_explicit_selection() {
        let first = fake_jdk("jdk-8");
        let second = fake_jdk("jdk-17");
        let installations =
            JavaToolchainProvider::detect_from_candidates([first.clone(), second.clone()]);
        assert_eq!(installations.len(), 2);
        let mut selection = JavaToolchainSelection::new(installations);
        let second_id = ToolchainId(format!(
            "java:{}",
            fs::canonicalize(&second)
                .unwrap_or(second.clone())
                .to_string_lossy()
        ));
        assert!(selection.select(&second_id).is_ok());
        assert_eq!(
            selection.selected().map(|installation| &installation.id),
            Some(&second_id)
        );
        let _ = fs::remove_dir_all(first);
        let _ = fs::remove_dir_all(second);
    }

    #[test]
    fn classpath_deduplicates_existing_entries() {
        let root = env::temp_dir().join(format!("er-ide-classpath-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        assert!(fs::create_dir_all(&root).is_ok());
        let classpath = ClasspathBuilder::new()
            .with_entry(&root)
            .with_entry(&root)
            .build();
        assert_eq!(classpath.entries, vec![root.clone()]);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn manually_selected_jdk_is_validated_and_added() {
        let root = fake_jdk("manual");
        let installation = match JavaToolchainProvider::installation_from_home(&root) {
            Ok(installation) => installation,
            Err(error) => panic!("manual JDK was rejected: {error}"),
        };
        let mut selection = JavaToolchainSelection::default();
        selection.add_and_select(installation);
        let expected = match fs::canonicalize(&root) {
            Ok(path) => path,
            Err(error) => panic!("failed to canonicalize fake JDK: {error}"),
        };
        assert_eq!(
            selection.selected().map(|value| value.home.as_path()),
            Some(expected.as_path())
        );
        let _ = fs::remove_dir_all(root);
    }
}
