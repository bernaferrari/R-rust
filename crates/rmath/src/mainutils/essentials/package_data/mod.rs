//! Owning serialized package data admission and publication.
use crate::sexp::{
    object::{Sexp, SexpError, SexpResult},
    owner::{OwnerToken, RuntimeAccess},
};
use std::path::Path;

mod owned;

/// Load an immutable workspace into a checked original-owner environment.
/// All bindings own their values before any publication callback can execute.
pub(crate) fn load_bytes(bytes: &[u8], environment: Sexp<'static>) -> Result<Vec<String>, String> {
    owned::load_bytes(bytes, environment)
}

pub(super) unsafe fn load_file(
    path: &Path,
    environment: crate::sexp::SEXP,
) -> Result<Vec<String>, String> {
    // SAFETY: translated package entry owns and activates the original runtime.
    let owner = unsafe { OwnerToken::current() }.map_err(|e| e.to_string())?;
    let environment = owner
        .sexp(environment)
        .and_then(Sexp::into_owned)
        .map_err(|e| e.to_string())?;
    owned::load_file(path, environment)
}

fn decode(access: &RuntimeAccess, bytes: &[u8]) -> SexpResult<Sexp<'static>> {
    let domain = access.domain();
    let allocator = access.allocator(&domain)?;
    let length = bytes
        .len()
        .try_into()
        .map_err(|_| SexpError::AllocationFailed {
            object: "serialized package data",
        })?;
    let raw = allocator
        .allocate(|arena| Some(arena.alloc_vector(crate::sexp::SEXPTYPE::RAWSXP, length)))?
        .into_owned()?;
    let mut raw = crate::sexp::SexpMut::try_from_checked(raw)?;
    for (index, value) in bytes.iter().copied().enumerate() {
        raw.try_set_raw_elt(index as _, value)?;
    }
    let raw = raw.freeze();
    access.with_native(|owner| {
        // SAFETY: the private raw input is owned and inaccessible to R callbacks.
        // No payload/arena loan spans decoding; the operation pin and activation
        // retain the original runtime and restore dispatch on every exit.
        unsafe {
            crate::sexp::session::with_instance_active(owner.as_ptr(), || {
                let value =
                    crate::mainutils::serialize::R_unserialize(raw.as_raw(), domain.nil().as_raw());
                domain.wrap(value)?.into_owned()
            })
        }
    })
}

fn publish(
    access: &RuntimeAccess,
    environment: &Sexp<'static>,
    symbol: &Sexp<'static>,
    value: &Sexp<'static>,
) -> SexpResult<()> {
    access.with_arena(|_| ())?;
    access.with_native(|owner| {
        // SAFETY: every operand and the complete pending workspace are owning
        // checked values; no loan spans the active binding callback.
        unsafe {
            crate::sexp::session::with_instance_active(owner.as_ptr(), || {
                crate::sexp::envir::defineVar(
                    symbol.as_raw(),
                    value.as_raw(),
                    environment.as_raw(),
                );
                Ok(())
            })
        }
    })
}

#[cfg(test)]
mod tests;
