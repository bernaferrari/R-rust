#![forbid(unsafe_code)]
//! Internal authority for the existing `.Options` snapshot, including locks.
use crate::sexp::object::{SessionNodeFactory, Sexp, SexpMut};

type Result<T> = std::result::Result<T, String>;
fn checked<T>(value: crate::sexp::object::SexpResult<T>) -> Result<T> {
    value.map_err(|error| error.to_string())
}

/// Read the current frame after construction callbacks have finished. Updating
/// this one existing cell preserves the binding lock and shared base frame.
/// Creating a missing binding remains the ordinary bootstrap adapter's job.
pub(super) fn replace_existing(
    factory: &SessionNodeFactory<'_>,
    base: &Sexp<'_>,
    symbol: &Sexp<'_>,
    options: &Sexp<'_>,
) -> Result<bool> {
    checked(factory.require_active())?;
    let domain = factory.domain();
    for value in [base, symbol, options] {
        checked(domain.link(value))?;
    }
    if !base.is_environment() || !symbol.is_symbol() {
        return Err("invalid internal options binding".into());
    }
    let mut cell = checked(checked(base.try_frame())?.into_owned())?;
    let mut seen = std::collections::HashSet::new();
    while !cell.is_nil() {
        let identity = checked(domain.link(&cell))?;
        seen.try_reserve(1)
            .map_err(|_| "cannot inspect options binding")?;
        if !seen.insert(identity) {
            return Err("cyclic options binding frame".into());
        }
        let tag = checked(cell.try_tag())?;
        checked(domain.link(&tag))?;
        if tag == *symbol {
            checked(factory.require_active())?;
            checked(checked(SexpMut::try_from_checked(cell))?.try_set_pairlist_car(options))?;
            return Ok(true);
        }
        cell = checked(checked(cell.try_cdr())?.into_owned())?;
    }
    Ok(false)
}
