//! Virtual file store for builtin package namespaces.
//!
//! Builtin namespaces (datasets and the portable package images) report
//! `<builtin:<pkg>>` as their installation path, but `packageDescription()`
//! and friends resolve `<pkgpath>/DESCRIPTION` through the ordinary file
//! layer (`file.exists`, connections, `read.dcf`). Serve those well-known
//! metadata files from the compiled-in assets so GNU-visible DESCRIPTION
//! access works for every builtin package without a real directory.

/// Bytes for a `<builtin:...>`/`<builtin>/...` path, when the named asset
/// exists in a builtin package image.
pub(crate) fn file_bytes(path: &str) -> Option<&'static [u8]> {
    let relative = relative_path(path)?;
    let (package, rest) = relative.split_once('/')?;
    match package {
        "datasets" => crate::library::datasets::file_bytes(&format!("<builtin>/datasets/{rest}")),
        _ => portable_package_bytes(&package, rest),
    }
}

/// True when `path` names an existing builtin package asset (or the package
/// directory itself).
pub(crate) fn path_exists(path: &str) -> bool {
    let Some(relative) = relative_path(path) else {
        return false;
    };
    if let Some((package, rest)) = relative.split_once('/') {
        if package == "datasets" {
            // Reuse the curated datasets directory/file table.
            return crate::library::datasets::path_exists(&format!("<builtin>/datasets/{rest}"));
        }
        // Portable images serve only their metadata files.
        return rest == "DESCRIPTION" && known_package(&package);
    }
    known_package(&relative)
}

fn known_package(package: &str) -> bool {
    package == "compiler"
        || package == "datasets"
        || crate::library::portable_package::image(package).is_some()
}

/// Strip either builtin prefix form: `<builtin:<pkg>>/rest` or
/// `<builtin>/<pkg>/rest`, leaving `<pkg>/rest`.
fn relative_path(path: &str) -> Option<String> {
    if let Some(tail) = path.strip_prefix("<builtin:") {
        let (package, rest) = tail.split_once('>')?;
        let rest = rest.strip_prefix('/')?;
        Some(format!("{package}/{rest}"))
    } else {
        path.strip_prefix("<builtin>/").map(str::to_string)
    }
}
fn portable_package_bytes(package: &str, rest: &str) -> Option<&'static [u8]> {
    if crate::library::portable_package::image(package).is_none() && package != "compiler" {
        return None;
    }
    match (package, rest) {
        ("compiler", "DESCRIPTION") => Some(include_bytes!("compiler_assets/DESCRIPTION")),
        ("methods", "DESCRIPTION") => Some(include_bytes!("methods/portable/assets/DESCRIPTION")),
        ("grDevices", "DESCRIPTION") => Some(include_bytes!(
            "portable_package/assets/grDevices/DESCRIPTION"
        )),
        ("graphics", "DESCRIPTION") => Some(include_bytes!(
            "portable_package/assets/graphics/DESCRIPTION"
        )),
        ("stats", "DESCRIPTION") => {
            Some(include_bytes!("portable_package/assets/stats/DESCRIPTION"))
        }
        ("stats4", "DESCRIPTION") => {
            Some(include_bytes!("portable_package/assets/stats4/DESCRIPTION"))
        }
        ("utils", "DESCRIPTION") => {
            Some(include_bytes!("portable_package/assets/utils/DESCRIPTION"))
        }
        ("tools", "DESCRIPTION") => {
            Some(include_bytes!("portable_package/assets/tools/DESCRIPTION"))
        }
        _ => None,
    }
}
