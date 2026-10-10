//! The original pinned GNU methods namespace for runtimes without host R.
use crate::sexp::object::{Sexp, SexpResult};
mod bridge;
mod owned;

pub(crate) const DIRECTORY: &str = "<builtin:methods>";
pub(crate) const DATABASE: &str = "<builtin:methods>/R/methods.rdb";
const DATABASE_BYTES: &[u8] = include_bytes!("assets/methods.rdb");

pub(crate) fn namespace() -> Result<Sexp<'static>, String> {
    owned::namespace(bridge::base().map_err(|error| error.to_string())?)
}

pub(crate) fn attach() -> Result<(), String> {
    owned::attach(bridge::base().map_err(|error| error.to_string())?)
}

pub(crate) fn is_database(file: &Sexp<'_>) -> SexpResult<bool> {
    if file.typeof_() != crate::sexp::SEXPTYPE::STRSXP || file.header().sxpinfo.alt() || file.len() != 1 { return Ok(false); }
    file.try_string_elt(0)?.try_char_eq(DATABASE.as_bytes())
}

pub(crate) fn read_database(file: Sexp<'static>, key: Sexp<'static>) -> Result<Option<Sexp<'static>>, String> {
    owned::read_database(file, key)
}

pub(crate) fn fetch(key: Sexp<'static>, file: Sexp<'static>, compressed: Sexp<'static>, hook: Sexp<'static>) -> Result<Sexp<'static>, String> {
    owned::fetch(key, file, compressed, hook)
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod allocation_diagnostics;
