//! Package metadata discovery for an embedded session.

use std::collections::BTreeMap;

/// Metadata for an installed R package visible to an embedded session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RPackageInfo {
    pub name: String,
    pub version: String,
    pub title: String,
    pub description: String,
    pub license: String,
    pub depends: String,
    pub imports: String,
    pub suggests: String,
    pub needs_compilation: bool,
    pub path: String,
    pub library_path: String,
}
pub(crate) fn package_info_from_path(
    fallback_name: &str,
    package_path: &std::path::Path,
    library_paths: &[String],
) -> Option<RPackageInfo> {
    let description = std::fs::read_to_string(package_path.join("DESCRIPTION")).ok()?;
    let fields = description_fields(&description);
    let name = fields
        .get("Package")
        .cloned()
        .unwrap_or_else(|| fallback_name.into());
    let version = fields.get("Version").cloned().unwrap_or_default();
    let title = fields.get("Title").cloned().unwrap_or_default();
    let description = fields.get("Description").cloned().unwrap_or_default();
    let license = fields.get("License").cloned().unwrap_or_default();
    let depends = fields.get("Depends").cloned().unwrap_or_default();
    let imports = fields.get("Imports").cloned().unwrap_or_default();
    let suggests = fields.get("Suggests").cloned().unwrap_or_default();
    let needs_compilation = fields.get("NeedsCompilation").is_some_and(|value| {
        value.eq_ignore_ascii_case("yes") || value.eq_ignore_ascii_case("true")
    });
    let package_path_string = package_path.to_string_lossy().into_owned();
    let library_path = library_paths
        .iter()
        .find(|library| {
            package_path
                .parent()
                .is_some_and(|parent| parent == std::path::Path::new(library.as_str()))
        })
        .cloned()
        .unwrap_or_else(|| {
            package_path
                .parent()
                .map(|path| path.to_string_lossy().into_owned())
                .unwrap_or_default()
        });
    Some(RPackageInfo {
        name,
        version,
        title,
        description,
        license,
        depends,
        imports,
        suggests,
        needs_compilation,
        path: package_path_string,
        library_path,
    })
}

fn description_fields(description: &str) -> BTreeMap<String, String> {
    let mut fields = BTreeMap::<String, String>::new();
    let mut current_key: Option<String> = None;

    for line in description.lines() {
        if line.trim().is_empty() {
            break;
        }

        if line.starts_with(' ') || line.starts_with('\t') {
            if let Some(key) = current_key.as_ref()
                && let Some(value) = fields.get_mut(key)
            {
                if !value.is_empty() {
                    value.push('\n');
                }
                value.push_str(line.trim());
            }
            continue;
        }

        let Some((key, value)) = line.split_once(':') else {
            current_key = None;
            continue;
        };
        let key = key.trim();
        if key.is_empty() || key.chars().any(char::is_whitespace) {
            current_key = None;
            continue;
        }
        let key = key.to_string();
        fields.insert(key.clone(), value.trim().to_string());
        current_key = Some(key);
    }

    fields
}
/// Return metadata for every package directory with a DESCRIPTION file in the
/// given library paths, deduplicated by package name and sorted by name.
pub(crate) fn installed_packages_from_library_paths(library_paths: &[String]) -> Vec<RPackageInfo> {
    let mut packages: Vec<RPackageInfo> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    for library_path in library_paths {
        let Ok(entries) = std::fs::read_dir(library_path) else {
            continue;
        };
        for entry in entries.filter_map(Result::ok) {
            let package_dir = entry.path();
            if !package_dir.is_dir() || !package_dir.join("DESCRIPTION").is_file() {
                continue;
            }
            let Some(package_name) = package_dir
                .file_name()
                .and_then(|name| name.to_str())
                .map(str::to_string)
            else {
                continue;
            };
            if seen.contains(&package_name) {
                continue;
            }
            if let Some(info) = package_info_from_path(&package_name, &package_dir, library_paths) {
                seen.insert(package_name);
                packages.push(info);
            }
        }
    }
    packages.sort_by(|left, right| left.name.cmp(&right.name));
    packages
}
