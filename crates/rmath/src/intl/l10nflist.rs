#![warn(unused_unsafe)]
//! Gettext catalog paths for the standalone loader.

use std::path::{Component, Path, PathBuf};

pub(super) fn catalog_path(directory: &Path, locale: &str, domain: &str) -> Option<PathBuf> {
    fn single_component(value: &str) -> bool {
        let mut components = Path::new(value).components();
        !value.contains(['/', '\\', '\0'])
            && matches!(components.next(), Some(Component::Normal(_)))
            && components.next().is_none()
    }
    if !single_component(locale) || !single_component(domain) {
        return None;
    }
    Some(
        directory
            .join(locale)
            .join("LC_MESSAGES")
            .join(format!("{domain}.mo")),
    )
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_catalog_path_accepts_only_single_components() {
        assert_eq!(
            catalog_path(Path::new("locale"), "fr_CA.UTF-8", "R-base"),
            Some(
                Path::new("locale")
                    .join("fr_CA.UTF-8")
                    .join("LC_MESSAGES")
                    .join("R-base.mo")
            )
        );
        for locale in ["..", "fr/CA", "", "fr\\CA"] {
            assert!(catalog_path(Path::new("locale"), locale, "R-base").is_none());
        }
        for domain in ["/tmp/test", "../base", "base\0tail"] {
            assert!(catalog_path(Path::new("locale"), "fr_CA", domain).is_none());
        }
    }
}
