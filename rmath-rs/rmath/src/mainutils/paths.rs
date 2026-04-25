//! Per-instance runtime path policy.
//!
//! R normally derives library and temporary paths from process state. The Rust
//! port keeps the resolved policy on `RInstance` so Android hosts can run
//! multiple isolated sessions with explicit app-private directories.

use std::collections::HashSet;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimePathPolicy {
    library_paths: Vec<PathBuf>,
    temp_dir: PathBuf,
    cache_dir: Option<PathBuf>,
}

impl RuntimePathPolicy {
    pub fn from_env() -> Self {
        let mut library_paths = Vec::new();
        extend_env_paths(&mut library_paths, "R_LIBS_USER");
        extend_env_paths(&mut library_paths, "R_LIBS_SITE");

        if let Some(r_home) = std::env::var_os("R_HOME").map(PathBuf::from) {
            library_paths.push(r_home.join("library"));
        }

        #[cfg(not(target_os = "android"))]
        {
            if library_paths.is_empty() {
                if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
                    library_paths.push(home.join(".R").join("library"));
                }
                library_paths.push(PathBuf::from("/usr/local/lib/R/site-library"));
                library_paths.push(PathBuf::from("/usr/lib/R/site-library"));
                library_paths.push(PathBuf::from("/usr/lib/R/library"));
            }
        }

        dedupe_paths(&mut library_paths);

        RuntimePathPolicy {
            library_paths,
            temp_dir: std::env::temp_dir(),
            cache_dir: None,
        }
    }

    pub fn for_android_app(
        app_files_dir: impl Into<PathBuf>,
        cache_dir: impl Into<PathBuf>,
        bundled_library_dir: Option<impl Into<PathBuf>>,
    ) -> std::io::Result<Self> {
        let app_files_dir = app_files_dir.into();
        let cache_dir = cache_dir.into();
        let user_library = app_files_dir.join("R").join("library");
        let temp_dir = cache_dir.join("Rtmp");

        std::fs::create_dir_all(&user_library)?;
        std::fs::create_dir_all(&temp_dir)?;

        let mut library_paths = vec![user_library];
        if let Some(path) = bundled_library_dir {
            library_paths.push(path.into());
        }
        dedupe_paths(&mut library_paths);

        Ok(RuntimePathPolicy {
            library_paths,
            temp_dir,
            cache_dir: Some(cache_dir),
        })
    }

    pub fn library_paths(&self) -> &[PathBuf] {
        &self.library_paths
    }

    pub fn set_library_paths<I, P>(&mut self, paths: I)
    where
        I: IntoIterator<Item = P>,
        P: Into<PathBuf>,
    {
        self.library_paths = paths.into_iter().map(Into::into).collect();
        dedupe_paths(&mut self.library_paths);
    }

    pub fn temp_dir(&self) -> &Path {
        &self.temp_dir
    }

    pub fn cache_dir(&self) -> Option<&Path> {
        self.cache_dir.as_deref()
    }

    pub fn find_package_path(&self, package: &str) -> Option<PathBuf> {
        self.library_paths.iter().find_map(|library| {
            let package_dir = library.join(package);
            if package_dir.join("DESCRIPTION").is_file() {
                Some(package_dir)
            } else {
                None
            }
        })
    }
}

impl Default for RuntimePathPolicy {
    fn default() -> Self {
        Self::from_env()
    }
}

fn extend_env_paths(paths: &mut Vec<PathBuf>, key: &str) {
    let Some(value) = std::env::var_os(key) else {
        return;
    };
    if value.is_empty() {
        return;
    }
    paths.extend(std::env::split_paths(&value));
}

fn dedupe_paths(paths: &mut Vec<PathBuf>) {
    let mut seen = HashSet::<OsString>::new();
    paths.retain(|path| !path.as_os_str().is_empty() && seen.insert(path.as_os_str().to_owned()));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn android_policy_uses_only_app_owned_paths() {
        let root = std::env::temp_dir().join(format!("rport-path-policy-{}", std::process::id()));
        let files = root.join("files");
        let cache = root.join("cache");
        let bundled = root.join("bundled-library");

        let policy = RuntimePathPolicy::for_android_app(&files, &cache, Some(bundled.clone()))
            .expect("policy should create app-owned directories");

        assert_eq!(
            policy.library_paths(),
            &[files.join("R").join("library"), bundled]
        );
        assert_eq!(policy.temp_dir(), cache.join("Rtmp").as_path());
        assert!(policy.temp_dir().is_dir());

        let _ = std::fs::remove_dir_all(root);
    }
}
