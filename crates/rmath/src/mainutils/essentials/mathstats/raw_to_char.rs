#![forbid(unsafe_code)]
//! Original raw input roots, admitted bulk copies and native character tags.
use crate::sexp::{
    diagnostic::EvaluationError,
    encoding::{self, TextEncoding},
    ffi::{NA_LOGICAL, SEXPTYPE},
    memory::WorkspaceReservation,
    object::{Sexp, SexpError, SexpMut, SexpResult},
    owner::RuntimeAccess,
};
fn error(message: &'static str) -> SexpError {
    SexpError::EvaluationFailed {
        message: message.into(),
    }
}
fn allocation() -> SexpError {
    SexpError::AllocationFailed {
        object: "rawToChar workspace",
    }
}

struct Bytes {
    data: Vec<u8>,
    // Physical copy storage drops before its original interpreter's ticket.
    _admission: Option<WorkspaceReservation>,
}
fn copy(input: &Sexp<'_>, len: usize, access: &RuntimeAccess) -> SexpResult<Bytes> {
    let ledger = input.allocation()?.heap_identity().byte_ledger();
    let mut admission = if len == 0 {
        None
    } else {
        Some(ledger.reserve(len).ok_or_else(allocation)?)
    };
    let mut data = Vec::new();
    data.try_reserve_exact(len).map_err(|_| allocation())?;
    if let Some(ticket) = &mut admission {
        ticket.try_resize(data.capacity()).ok_or_else(allocation)?;
    }
    for i in 0..len {
        data.push(input.try_raw_elt(i as i64)?);
        access.require_active()?;
    }
    Ok(Bytes {
        data,
        _admission: admission,
    })
}
fn vector(access: &RuntimeAccess, len: i64) -> SexpResult<SexpMut<'static>> {
    let domain = access.domain();
    SexpMut::try_from_checked(
        access
            .allocator(&domain)?
            .allocate(|a| Some(a.alloc_vector(SEXPTYPE::STRSXP, len)))?,
    )
}
fn character(access: &RuntimeAccess, data: &[u8]) -> SexpResult<Sexp<'static>> {
    let domain = access.domain();
    access
        .allocator(&domain)?
        .allocate(|a| encoding::allocate_character(a, data, TextEncoding::Native))
}

pub(super) fn evaluate(
    access: &RuntimeAccess,
    arguments: Sexp<'static>,
    logical: impl FnOnce(&Sexp<'_>) -> SexpResult<i32>,
) -> Result<Sexp<'static>, EvaluationError> {
    let input = arguments.try_car()?.into_owned()?;
    if input.typeof_() != SEXPTYPE::RAWSXP {
        return Err(error("argument 'x' must be a raw vector").into());
    }
    access.domain().link(&input)?;
    // Select both arguments before coercion can notify or reenter user code.
    let rest = arguments.try_cdr()?.into_owned()?;
    let flag = if rest.is_nil() {
        access.domain().logical(false)
    } else {
        rest.try_car()?.into_owned()?
    };
    let multiple = logical(&flag)?;
    access.require_active()?;
    if multiple == NA_LOGICAL {
        return Err(error("argument 'multiple' must be TRUE or FALSE").into());
    }
    if multiple != 0 {
        let len = input.len();
        let mut out = vector(access, len)?;
        for i in 0..len {
            // Only a copied stack byte crosses a constructor callback. The
            // original raw vector stays rooted until all elements publish.
            let byte = [input.try_raw_elt(i)?];
            access.require_active()?;
            let data = &byte[..usize::from(byte[0] != 0)];
            out.try_set_string_elt(i, character(access, data)?)?;
        }
        return Ok(out.freeze());
    }
    let mut len = usize::try_from(input.len()).map_err(|_| allocation())?;
    while len != 0 {
        let last = input.try_raw_elt((len - 1) as i64)?;
        access.require_active()?;
        if last != 0 {
            break;
        }
        len -= 1;
    }
    let bytes = copy(&input, len, access)?;
    if bytes.data.contains(&0) {
        let limit = access.with_native(|original| {
            let limit = crate::mainutils::errors::diagnostic_message_limit();
            original.require_active()?;
            Ok(limit)
        })?;
        let diagnostic = encoding::nul_diagnostic::format_bounded(
            &arguments,
            &bytes.data,
            TextEncoding::Native,
            crate::mainutils::platform::locale_is_utf8(),
            limit,
        )?;
        return Err(EvaluationError::Diagnostic(
            diagnostic.into_owned(access, &arguments)?,
        ));
    }
    let mut out = vector(access, 1)?;
    out.try_set_string_elt(0, character(access, &bytes.data)?)?;
    Ok(out.freeze())
}

#[cfg(test)]
#[path = "raw_to_char_workspace_tests.rs"]
mod tests;
