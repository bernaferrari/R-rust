#![warn(unused_unsafe)]
//! Read a candidate gettext catalog. Runtime lookup does not yet load catalogs.

use std::path::Path;

use super::l10nflist::catalog_path;
use super::loadmsgcat::{Catalog, parse_catalog};

pub(super) fn read_catalog(directory: &Path, locale: &str, domain: &str) -> Option<Catalog> {
    let path = catalog_path(directory, locale, domain)?;
    parse_catalog(std::fs::read(path).ok()?)
}
