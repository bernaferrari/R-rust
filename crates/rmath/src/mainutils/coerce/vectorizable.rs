//! GNU list and pairlist admission for vector coercion.
#![forbid(unsafe_code)]

use crate::sexp::{
    ffi::SEXPTYPE,
    object::{Sexp, SexpError, SexpResult},
    owner::RuntimeAccess,
};

fn failure(message: &str) -> SexpError {
    SexpError::EvaluationFailed {
        message: message.into(),
    }
}

/// GNU admits NULL and lists whose elements are vectors of length zero or one.
/// An expression vector or atomic vector is admissible as a child, but is not
/// itself a list for this predicate. ALTREP lengths are fixed at construction.
pub(super) fn check(value: Sexp<'static>, access: &RuntimeAccess) -> SexpResult<bool> {
    access.require_active()?;
    let value = access.domain().wrap(value.as_raw())?.into_owned()?;
    match value.typeof_() {
        SEXPTYPE::NILSXP => Ok(true),
        SEXPTYPE::VECSXP => {
            for index in 0..value.len() {
                // A provider can collect, reenter, close the owner, or unwind.
                // Never inspect its result or propagate its signal before the
                // original authority has been checked again.
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    value.try_vector_elt(index)?.into_owned()
                }));
                access.require_active()?;
                let child = match result {
                    Ok(child) => child?,
                    Err(payload) => std::panic::resume_unwind(payload),
                };
                if !scalar_vector(&child) {
                    return Ok(false);
                }
            }
            Ok(true)
        }
        SEXPTYPE::LISTSXP => {
            let mut cursor = value;
            let mut seen = std::collections::HashSet::new();
            while !cursor.is_nil() {
                if cursor.typeof_() != SEXPTYPE::LISTSXP {
                    return Err(failure("improper pairlist in vector coercion"));
                }
                let identity = cursor
                    .allocation()?
                    .link()
                    .ok_or(SexpError::StaleAllocation)?;
                seen.try_reserve(1)
                    .map_err(|_| failure("pairlist admission allocation failed"))?;
                if !seen.insert(identity) {
                    return Err(failure("cyclic pairlist in vector coercion"));
                }
                let child = cursor.try_car()?.into_owned()?;
                if !scalar_vector(&child) {
                    return Ok(false);
                }
                cursor = cursor.try_cdr()?.into_owned()?;
            }
            Ok(true)
        }
        _ => Ok(false),
    }
}

fn scalar_vector(value: &Sexp<'_>) -> bool {
    value.is_vector() && value.len() <= 1
}
