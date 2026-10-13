//! Package path policy and DESCRIPTION lookup.

use super::*;

/// Try to find a package by name in this session's configured library paths.
pub(crate) fn find_package_path(package: &str) -> String {
    crate::sexp::instance::with_required_current_instance(|inst| unsafe {
        (*inst)
            .path_policy
            .find_package_path(package)
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
    let content = std::fs::read_to_string(&description)
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
        if package_needs_compilation(&description)? && !is_builtin_package_dependency(package) {
            return Err(format!(
                "package '{}' declares NeedsCompilation: yes; this pure-R Android runtime does not load compiled package code",
                package
            ));
        }

        let mut loading = vec![package.to_string()];
        let (env, _) = load_package_namespace(package, package_dir, &mut loading)?;

        Ok(env)
    }
}
