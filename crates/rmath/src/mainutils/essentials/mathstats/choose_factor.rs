#![forbid(unsafe_code)]
//! Class inspection retains the selected class through provider callbacks.
use crate::sexp::{
    ffi::SEXPTYPE,
    object::{Sexp, SexpError, SexpResult},
    owner::RuntimeAccess,
};

pub(super) fn has_factor(access: &RuntimeAccess, a: &Sexp<'_>, b: &Sexp<'_>) -> SexpResult<bool> {
    access.require_active()?;
    for input in [a, b] {
        if input.typeof_() != SEXPTYPE::INTSXP {
            continue;
        }
        let mut attributes = input.try_attrib()?.into_owned()?;
        validate_attributes(&attributes)?;
        while !attributes.is_nil() {
            if attributes.try_tag_name_eq(b"class")? {
                let classes = attributes.try_car()?.into_owned()?;
                if classes.typeof_() != SEXPTYPE::STRSXP {
                    break;
                }
                let length = classes.len();
                for index in 0..length {
                    // No input or character payload view crosses this callback.
                    // Its selected child has its own root before inspection.
                    let name = classes.try_string_elt(index)?.into_owned()?;
                    access.require_active()?;
                    if name.try_char_eq(b"factor")? {
                        return Ok(true);
                    }
                }
                break;
            }
            attributes = attributes.try_cdr()?.into_owned()?;
        }
    }
    access.require_active()?;
    Ok(false)
}

// No provider can run during this metadata-only preflight. Two rooted cursors
// reject malformed tails and cycles without input-size scratch storage.
fn next_attribute(value: &Sexp<'_>) -> SexpResult<Sexp<'static>> {
    if value.is_nil() {
        return value.clone().into_owned();
    }
    if value.typeof_() != SEXPTYPE::LISTSXP {
        return Err(SexpError::InvalidAttributes {
            reason: "malformed choose attribute list",
        });
    }
    let next = value.try_cdr()?.into_owned()?;
    if !next.is_nil() && next.typeof_() != SEXPTYPE::LISTSXP {
        return Err(SexpError::InvalidAttributes {
            reason: "malformed choose attribute list",
        });
    }
    Ok(next)
}
fn validate_attributes(head: &Sexp<'_>) -> SexpResult<()> {
    let mut slow = head.clone().into_owned()?;
    let mut fast = head.clone().into_owned()?;
    loop {
        fast = next_attribute(&fast)?;
        if fast.is_nil() {
            return Ok(());
        }
        fast = next_attribute(&fast)?;
        slow = next_attribute(&slow)?;
        if fast.is_nil() {
            return Ok(());
        }
        if fast == slow {
            return Err(SexpError::InvalidAttributes {
                reason: "cyclic choose attribute list",
            });
        }
    }
}
