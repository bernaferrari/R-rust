//! Package path policy and DESCRIPTION lookup.

use super::*;
use crate::mainutils::browser_files;
use std::io;

pub(super) enum PackageText {
    Native(String),
    Browser(browser_files::FileText),
}

impl std::ops::Deref for PackageText {
    type Target = str;

    fn deref(&self) -> &str {
        match self {
            Self::Native(text) => text,
            Self::Browser(text) => text,
        }
    }
}

pub(super) fn read_package_text(path: &Path) -> io::Result<PackageText> {
    if browser_files::enabled() {
        let key = path.to_str().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "invalid browser package path")
        })?;
        browser_files::read_text_or_host(key).map(PackageText::Browser)
    } else {
        std::fs::read_to_string(path).map(PackageText::Native)
    }
}

pub(super) fn package_file_exists(path: &Path) -> bool {
    if browser_files::enabled() {
        path.to_str().is_some_and(browser_files::contains_current)
    } else {
        path.is_file()
    }
}

// Browser directories are prefixes over bounded, validated file keys.
pub(super) fn package_directory_entries(path: &Path) -> io::Result<Vec<PathBuf>> {
    if browser_files::enabled() {
        Ok(browser_files::list_current()
            .into_iter()
            .map(PathBuf::from)
            .filter(|entry| entry.parent() == Some(path))
            .collect())
    } else if path.is_dir() {
        Ok(std::fs::read_dir(path)?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .collect())
    } else {
        Ok(Vec::new())
    }
}

fn find_browser_package(package: &str, roots: &[PathBuf]) -> Option<PathBuf> {
    if !package.starts_with(|c: char| c.is_ascii_alphabetic())
        || !package.ends_with(|c: char| c.is_ascii_alphanumeric())
        || !package
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'.')
        || is_builtin_package_dependency(package)
    {
        return None;
    }
    roots.iter().map(|root| root.join(package)).find(|dir| {
        dir.join("DESCRIPTION").to_str().is_some_and(|key| {
            browser_files::BrowserFileStore::validate_path(key).is_ok()
                && browser_files::contains_current(key)
        })
    })
}

/// Try to find a package by name in this session's configured library paths.
pub(crate) fn find_package_path(package: &str) -> String {
    crate::sexp::instance::with_required_current_instance(|inst| unsafe {
        let paths = &(*inst).path_policy;
        let package_dir = if browser_files::enabled() {
            find_browser_package(package, paths.library_paths())
        } else {
            paths.find_package_path(package)
        };
        package_dir
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_default()
    })
}

/// Public package discovery includes the shipped datasets image. Internal
/// loader decisions continue to distinguish installed and portable storage.
pub(crate) fn public_package_path(package: &str) -> String {
    let path = find_package_path(package);
    if path.is_empty() && package == "datasets" {
        crate::library::datasets::PACKAGE_DIRECTORY.into()
    } else {
        path
    }
}

pub(crate) fn package_description_fields(
    package: &str,
) -> Result<BTreeMap<String, String>, String> {
    if package.is_empty() || package == "NA" {
        return Err("invalid package name".to_string());
    }

    let package_path = find_package_path(package);
    if package_path.is_empty() {
        if package == "datasets" {
            return Ok(description_fields(include_str!(
                "../../../library/datasets/assets/DESCRIPTION"
            )));
        }
        return Err(format!("there is no package called '{}'", package));
    }

    let description = Path::new(&package_path).join("DESCRIPTION");
    let content = read_package_text(&description)
        .map_err(|err| format!("could not read {}: {err}", description.display()))?;
    Ok(description_fields(&content))
}

pub(crate) unsafe fn load_package_namespace_by_name(package: &str) -> Result<SEXP, String> {
    unsafe {
        let package = package.strip_prefix("package:").unwrap_or(package);
        if package.is_empty() || package == "NA" {
            return Err("invalid package name".to_string());
        }
        // GNU getNamespace("base") / asNamespace("base") is .BaseNamespaceEnv,
        // not the search-path base environment and not library/base.
        if package == "base" {
            return Ok(crate::sexp::envir::R_BaseNamespace());
        }
        // GNU keeps loaded namespace identities when .libPaths() changes.
        // Existing closures and imports still refer to that original graph.
        if let Some(namespace) = cached_namespace_by_name(package) {
            return Ok(namespace);
        }

        #[cfg(feature = "renderplot-device")]
        if package == "grid" {
            return Ok(crate::mainutils::portable_grid::namespace());
        }

        let package_path = find_package_path(package);
        // The authenticated builtin datasets image owns the datasets
        // namespace even when a library tree is visible: its lazydata
        // promises are what `datasets::` must resolve (an installed
        // source tree only carries the empty lazydata NAMESPACE).
        if package == "datasets" {
            return crate::library::datasets::namespace().map(|namespace| namespace.as_raw());
        }
        if package_path.is_empty() {
            if package == "methods" {
                return crate::library::methods::portable::namespace()
                    .map(|namespace| namespace.as_raw());
            }
            if let Some(image) = crate::library::portable_package::image(package) {
                return image.namespace().map(|namespace| namespace.as_raw());
            }
            return Err(format!("there is no package called '{}'", package));
        }
        let package_dir = Path::new(&package_path);
        let description = package_dir.join("DESCRIPTION");
        check_package_compilation(package, &description)?;

        let mut loading = vec![package.to_string()];
        let (env, _) = load_package_namespace(package, package_dir, &mut loading)?;

        Ok(env)
    }
}
