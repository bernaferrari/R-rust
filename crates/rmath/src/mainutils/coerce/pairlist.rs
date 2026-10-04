//! Owning pairlist snapshots for string and list coercion.
#![forbid(unsafe_code)]

use crate::sexp::{
    SEXPTYPE,
    object::{Sexp, SexpError, SexpMut, SexpResult},
    owner::RuntimeAccess,
};

pub(super) trait Native {
    fn deparse(
        &mut self,
        access: &RuntimeAccess,
        value: &Sexp<'static>,
    ) -> SexpResult<Sexp<'static>>;
    fn names(
        &mut self,
        access: &RuntimeAccess,
        value: &Sexp<'static>,
        names: &Sexp<'static>,
    ) -> SexpResult<()>;
}

fn failure(message: &str) -> SexpError {
    SexpError::EvaluationFailed {
        message: message.into(),
    }
}

fn callback<T>(access: &RuntimeAccess, operation: impl FnOnce() -> SexpResult<T>) -> SexpResult<T> {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(operation));
    access.require_active()?;
    match result {
        Ok(result) => result,
        Err(payload) => std::panic::resume_unwind(payload),
    }
}

/// Every selected child and tag has an independent lease before allocation or
/// provider callbacks. No raw cell cursor survives those callbacks.
pub(super) fn coerce(
    value: Sexp<'static>,
    target: SEXPTYPE,
    access: &RuntimeAccess,
    native: &mut impl Native,
) -> SexpResult<Sexp<'static>> {
    access.require_active()?;
    let domain = access.domain();
    let value = domain.wrap(value.as_raw())?.into_owned()?;
    if !matches!(target, SEXPTYPE::STRSXP | SEXPTYPE::VECSXP) {
        return Err(failure("unsupported checked pairlist coercion"));
    }
    let language = value.typeof_() == SEXPTYPE::LANGSXP;
    let mut cursor = value;
    let mut selected = Vec::new();
    let mut seen = std::collections::HashSet::new();
    while !cursor.is_nil() {
        if cursor.typeof_() != SEXPTYPE::LISTSXP
            && !(selected.is_empty() && cursor.typeof_() == SEXPTYPE::LANGSXP)
        {
            return Err(failure("improper pairlist in vector coercion"));
        }
        let identity = cursor
            .allocation()?
            .link()
            .ok_or(SexpError::StaleAllocation)?;
        seen.try_reserve(1)
            .map_err(|_| failure("pairlist snapshot allocation failed"))?;
        if !seen.insert(identity) {
            return Err(failure("cyclic pairlist in vector coercion"));
        }
        let child = cursor.try_car()?.into_owned()?;
        let tag = cursor.try_tag()?;
        let name = if tag.is_nil() {
            None
        } else {
            if tag.typeof_() != SEXPTYPE::SYMSXP {
                return Err(failure("invalid pairlist tag in vector coercion"));
            }
            Some(tag.try_printname()?.into_owned()?)
        };
        selected
            .try_reserve(1)
            .map_err(|_| failure("pairlist snapshot allocation failed"))?;
        selected.push((child, name));
        cursor = cursor.try_cdr()?.into_owned()?;
    }
    let length = selected
        .len()
        .try_into()
        .map_err(|_| failure("pairlist is too long"))?;
    let allocator = access.allocator(&domain)?;
    let result = allocator.allocate(|arena| Some(arena.alloc_vector(target, length)))?;
    let mut result = SexpMut::try_from_checked(result)?;
    for (index, (child, _)) in selected.iter().enumerate() {
        if target == SEXPTYPE::VECSXP {
            result.try_set_vector_elt(index as _, child.clone())?;
        } else {
            let character = if language && index == 0 && child.typeof_() == SEXPTYPE::SYMSXP {
                child.try_printname()?.into_owned()?
            } else if child.typeof_() == SEXPTYPE::STRSXP && child.len() == 1 {
                callback(access, || child.try_string_elt(0)?.into_owned())?
            } else {
                let text = callback(access, || native.deparse(access, child))?;
                domain.link(&text)?;
                if text.typeof_() != SEXPTYPE::STRSXP || text.len() < 1 {
                    return Err(failure("invalid pairlist deparse result"));
                }
                callback(access, || text.try_string_elt(0)?.into_owned())?
            };
            result.try_set_string_elt(index as _, character)?;
        }
    }
    let result = result.freeze();
    if !(language && target == SEXPTYPE::STRSXP) && selected.iter().any(|(_, name)| name.is_some())
    {
        let names =
            allocator.allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::STRSXP, length)))?;
        let mut names = SexpMut::try_from_checked(names)?;
        let blank = allocator.character("")?;
        for (index, (_, name)) in selected.iter().enumerate() {
            names.try_set_string_elt(index as _, name.clone().unwrap_or_else(|| blank.clone()))?;
        }
        callback(access, || native.names(access, &result, &names.freeze()))?;
    }
    access.require_active()?;
    Ok(result)
}
