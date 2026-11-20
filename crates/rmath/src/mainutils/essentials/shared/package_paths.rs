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

pub(super) fn package_directory_entries(path: &Path) -> io::Result<Vec<PathBuf>> {
    if browser_files::enabled() {
        let key = path.strip_prefix("/").unwrap_or(path);
        Ok(browser_files::list_current()
            .into_iter()
            .map(PathBuf::from)
            .filter(|entry| entry.parent() == Some(key))
            .map(|entry| {
                if path.is_absolute() {
                    Path::new("/").join(entry)
                } else {
                    entry
                }
            })
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

pub(super) fn valid_package_name(package: &str) -> bool {
    package.starts_with(|c: char| c.is_ascii_alphabetic())
        && package.ends_with(|c: char| c.is_ascii_alphanumeric())
        && package
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'.')
}

fn find_browser_package(package: &str, roots: &[PathBuf]) -> Option<PathBuf> {
    if !valid_package_name(package) || is_builtin_package_dependency(package) {
        return None;
    }
    let anchor = browser_files::getcwd_current() != "/";
    roots
        .iter()
        .filter_map(|root| {
            let dir = root.join(package);
            let key = dir.join("DESCRIPTION");
            let key = key.to_str()?;
            if browser_files::BrowserFileStore::validate_path(key).is_err()
                || !browser_files::contains_current(&format!("/{key}"))
            {
                return None;
            }
            Some(if anchor {
                Path::new("/").join(dir)
            } else {
                dir
            })
        })
        .next()
}

fn find_package_in_libraries(package: &str, roots: &[PathBuf]) -> Option<PathBuf> {
    if browser_files::enabled() {
        return find_browser_package(package, roots);
    }
    roots
        .iter()
        .map(|library| library.join(package))
        .find(|path| path.join("DESCRIPTION").is_file())
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

pub(crate) fn namespace_version(namespace: SEXP) -> Option<String> {
    unsafe {
        if namespace == crate::sexp::envir::R_BaseNamespace() {
            return Some(crate::mainutils::compatibility_target::NUMERIC_VERSION.to_owned());
        }
        let _namespace = protect(namespace);
        let metadata = crate::sexp::envir::R_findVarInFrame(
            namespace,
            Rf_install(c".__NAMESPACE__.".as_ptr()),
        );
        if TYPEOF(metadata) != SEXPTYPE::ENVSXP {
            return None;
        }
        let _metadata = protect(metadata);
        let spec = crate::sexp::envir::R_findVarInFrame(metadata, Rf_install(c"spec".as_ptr()));
        if TYPEOF(spec) != SEXPTYPE::STRSXP || XLENGTH(spec) != 2 {
            return None;
        }
        let _spec = protect(spec);
        let version = elt_to_string(spec, 1);
        (!version.is_empty()).then_some(version)
    }
}

fn is_portable_namespace(package: &str) -> bool {
    package == "methods" || crate::library::portable_package::image(package).is_some()
}

fn is_builtin_namespace(package: &str) -> bool {
    package == "datasets" || (cfg!(feature = "renderplot-device") && package == "grid")
}

fn check_loaded_requirement(
    package: &str,
    namespace: SEXP,
    requirement: &NamespaceRequirement,
) -> Result<(), String> {
    {
        let version = namespace_version(namespace)
            .ok_or_else(|| format!("namespace ‘{package}’ has no version metadata"))?;
        requirement.check(package, &version, true)
    }
}

fn check_source_requirement(
    package: &str,
    directory: Option<&Path>,
    requirement: &NamespaceRequirement,
) -> Result<(), String> {
    let version = if is_builtin_namespace(package)
        || (directory.is_none() && is_portable_namespace(package))
    {
        crate::mainutils::compatibility_target::NUMERIC_VERSION.to_owned()
    } else {
        let directory =
            directory.ok_or_else(|| format!("there is no package called '{package}'"))?;
        read_package_text(&directory.join("DESCRIPTION"))
            .map_err(|e| format!("could not read DESCRIPTION for '{package}': {e}"))
            .and_then(|text| {
                description_fields(&text)
                    .remove("Version")
                    .ok_or_else(|| format!("namespace ‘{package}’ has no version metadata"))
            })?
    };
    requirement.check(package, &version, false)
}

/// Check a dependency before attaching packages or running namespace code.
pub(super) fn check_declared_dependency(
    package: &str,
    requirement: &NamespaceRequirement,
) -> Result<(), String> {
    if package == "R" {
        return requirement.check(
            package,
            crate::mainutils::compatibility_target::NUMERIC_VERSION,
            true,
        );
    }
    if let Some(namespace) = cached_namespace_by_name(package) {
        return  check_loaded_requirement(package, namespace, requirement) ;
    }
    let path = find_package_path(package);
    check_source_requirement(
        package,
        (!path.is_empty()).then(|| Path::new(&path)),
        requirement,
    )
}

pub(crate) fn load_package_namespace_by_name(package: &str) -> Result<SEXP, String> {
     load_package_namespace_checked(package, None, None) 
}

pub(crate) fn load_package_namespace_checked(
    package: &str,
    libraries: Option<&[PathBuf]>,
    requirement: Option<&NamespaceRequirement>,
) -> Result<SEXP, String> {
    {
        let package = package.strip_prefix("package:").unwrap_or(package);
        if package == "NA" || !valid_package_name(package) {
            return Err("invalid package name".to_string());
        }
        if let Some(namespace) = cached_namespace_by_name(package) {
            if let Some(requirement) = requirement {
                check_loaded_requirement(package, namespace, requirement)?;
            }
            return Ok(namespace);
        }

        let package_path = if let Some(roots) = libraries {
            find_package_in_libraries(package, roots)
        } else {
            let path = find_package_path(package);
            (!path.is_empty()).then(|| PathBuf::from(path))
        };
        if let Some(requirement) = requirement {
            check_source_requirement(package, package_path.as_deref(), requirement)?;
        }

        #[cfg(feature = "renderplot-device")]
        if package == "grid" {
            return Ok(crate::mainutils::portable_grid::namespace());
        }

        // The authenticated builtin datasets image owns the datasets
        // namespace even when a library tree is visible.
        if package == "datasets" {
            return crate::library::datasets::namespace().map(|namespace| namespace.as_raw());
        }
        let Some(package_dir) = package_path else {
            if package == "methods" {
                return crate::library::methods::portable::namespace()
                    .map(|namespace| namespace.as_raw());
            }
            if let Some(image) = crate::library::portable_package::image(package) {
                return image.namespace().map(|namespace| namespace.as_raw());
            }
            return Err(format!("there is no package called '{package}'"));
        };
        let mut loading = vec![package.to_string()];
        let (env, _) = load_package_namespace(package, &package_dir, &mut loading)?;

        Ok(env)
    }
}
