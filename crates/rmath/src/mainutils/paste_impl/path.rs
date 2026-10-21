#![forbid(unsafe_code)]
//! Both path entry shapes join owned components through the original runtime.
use super::owned_paste::{self, Bytes, Mode, Native, Value};
use crate::sexp::{
    encoding::TextEncoding,
    ffi::SEXPTYPE,
    memory::TransientReservation,
    object::{SexpError, SexpMut, SexpResult},
    owner::RuntimeAccess,
};

fn error(message: impl Into<String>) -> SexpError {
    SexpError::EvaluationFailed {
        message: message.into(),
    }
}
fn allocation() -> SexpError {
    SexpError::AllocationFailed {
        object: "file.path workspace",
    }
}
fn vector(access: &RuntimeAccess, kind: SEXPTYPE, len: i64) -> SexpResult<SexpMut<'static>> {
    let domain = access.domain();
    SexpMut::try_from_checked(
        access
            .allocator(&domain)?
            .allocate(|a| Some(a.alloc_vector(kind, len)))?,
    )
}
fn workspace(parent: &Value, bytes: usize) -> SexpResult<TransientReservation> {
    let node = parent.allocation()?;
    node.heap_identity()
        .reserve_payload_bytes(node, bytes)
        .ok_or_else(allocation)
}
fn input(access: &RuntimeAccess, inputs: &Value, index: i64) -> SexpResult<Value> {
    let value = inputs.try_vector_elt(index)?.into_owned()?;
    access.require_active()?;
    Ok(value)
}
fn selected(access: &RuntimeAccess, inputs: &Value, index: i64, row: i64) -> SexpResult<Value> {
    let value = input(access, inputs, index)?;
    let len = owned_paste::length(access, &value)?;
    if len == 0 {
        return Err(error("path component changed length to zero"));
    }
    owned_paste::element(access, &value, row % len)
}

/// GNU's internal entry receives the component list and separator vector.
pub(super) fn internal(
    access: &RuntimeAccess,
    native: &impl Native,
    args: Value,
    env: Value,
) -> SexpResult<Value> {
    let inputs = args.try_car()?.into_owned()?;
    let tail = args.try_cdr()?.into_owned()?;
    if tail.is_nil() || !tail.try_cdr()?.is_nil() {
        return Err(error("invalid number of arguments to .Internal(file.path)"));
    }
    let separator = tail.try_car()?.into_owned()?;
    evaluate(access, native, inputs, separator, args, env)
}

/// The evaluator's public entry receives dots and the named fsep argument.
pub(super) fn public(
    access: &RuntimeAccess,
    native: &impl Native,
    args: Value,
    env: Value,
) -> SexpResult<Value> {
    let mut count = 0i64;
    let mut separator = None;
    let mut current = args.clone();
    while !current.is_nil() {
        if current.try_tag_name_eq(b"fsep")? {
            if separator.is_some() {
                return Err(error(
                    "formal argument \"fsep\" matched by multiple actual arguments",
                ));
            }
            separator = Some(current.try_car()?.into_owned()?);
        } else {
            count = count.checked_add(1).ok_or_else(allocation)?;
        }
        current = current.try_cdr()?.into_owned()?;
    }
    if count == 0 {
        return Ok(vector(access, SEXPTYPE::STRSXP, 0)?.freeze());
    }
    let mut inputs = vector(access, SEXPTYPE::VECSXP, count)?;
    let mut current = args.clone();
    let mut index = 0;
    while !current.is_nil() {
        if !current.try_tag_name_eq(b"fsep")? {
            inputs.try_set_vector_elt(index, current.try_car()?.into_owned()?)?;
            index += 1;
        }
        current = current.try_cdr()?.into_owned()?;
    }
    let separator = if let Some(separator) = separator {
        separator
    } else {
        let mut result = vector(access, SEXPTYPE::STRSXP, 1)?;
        result.try_set_string_elt(
            0,
            owned_paste::character(access, b"/", TextEncoding::Native)?,
        )?;
        result.freeze()
    };
    evaluate(access, native, inputs.freeze(), separator, args, env)
}
fn evaluate(
    access: &RuntimeAccess,
    native: &impl Native,
    inputs: Value,
    separator: Value,
    parent: Value,
    env: Value,
) -> SexpResult<Value> {
    if !matches!(inputs.typeof_(), SEXPTYPE::VECSXP | SEXPTYPE::EXPRSXP) {
        return Err(error("invalid first argument"));
    }
    let count = owned_paste::length(access, &inputs)?;
    if count == 0 {
        return Ok(vector(access, SEXPTYPE::STRSXP, 0)?.freeze());
    }
    // GNU validates/selects separately and uses the raw separator bytes. In
    // particular a declared Latin1 fsep is not converted to UTF8 here.
    owned_paste::separator(access, &separator, "invalid separator")?;
    let separator = owned_paste::element(access, &separator, 0)?;
    let separator = native.bytes(&separator, Mode::Bytes, &parent)?;
    let mut max_len = 0;
    for j in 0..count {
        let value = input(access, &inputs, j)?;
        let value = if value.typeof_() != SEXPTYPE::STRSXP {
            let value = native.coerce(&value, &env)?;
            access.require_active()?;
            if value.typeof_() != SEXPTYPE::STRSXP {
                return Err(error("non-string argument to .Internal(file.path)"));
            }
            SexpMut::try_from_checked(inputs.clone())?.try_set_vector_elt(j, value.clone())?;
            value
        } else {
            value
        };
        let len = owned_paste::length(access, &value)?;
        if len == 0 {
            return Ok(vector(access, SEXPTYPE::STRSXP, 0)?.freeze());
        }
        max_len = max_len.max(len);
    }
    // Byte declarations are forbidden for every component, even one that is
    // not selected in a particular recycled row. Keep GNU's traversal order.
    for j in 0..count {
        let value = input(access, &inputs, j)?;
        for i in 0..owned_paste::length(access, &value)? {
            let character = owned_paste::element(access, &value, i)?;
            if native.flags(&character)?.bytes {
                return Err(error("strings with \"bytes\" encoding are not allowed"));
            }
        }
    }
    let mut result = vector(access, SEXPTYPE::STRSXP, max_len)?;
    let count_usize = usize::try_from(count).map_err(|_| allocation())?;
    for row in 0..max_len {
        let mut utf8 = crate::mainutils::platform::locale_is_utf8();
        if !utf8 {
            for j in 0..count {
                let character = selected(access, &inputs, j, row)?;
                // Supported native profiles are C and UTF8. A declared Latin1
                // component requires UTF8 in C, just as GNU paste.c specifies.
                if native.flags(&character)?.known {
                    utf8 = true;
                    break;
                }
            }
        }
        let mode = if utf8 { Mode::Utf8Path } else { Mode::Native };
        let mut width = separator
            .data
            .len()
            .checked_mul(count_usize - 1)
            .ok_or_else(allocation)?;
        for j in 0..count {
            let character = selected(access, &inputs, j, row)?;
            width = width
                .checked_add(native.bytes(&character, mode, &parent)?.data.len())
                .ok_or_else(allocation)?;
        }
        if width > i32::MAX as usize {
            return Err(error("result would exceed 2^31-1 bytes"));
        }
        let _parts_charge = workspace(
            &parent,
            count_usize
                .checked_mul(std::mem::size_of::<Bytes>())
                .ok_or_else(allocation)?,
        )?;
        let mut parts = Vec::new();
        parts
            .try_reserve_exact(count_usize)
            .map_err(|_| allocation())?;
        for j in 0..count {
            let character = selected(access, &inputs, j, row)?;
            parts.push(native.bytes(&character, mode, &parent)?);
        }
        // Recompute admission from the actual copy-pass pieces. Providers can
        // change a later component after the width pass; never grow uncharged.
        let joined = owned_paste::join(&parent, &parts, &separator)?;
        let encoding = if utf8 {
            TextEncoding::Utf8
        } else {
            TextEncoding::Native
        };
        result.try_set_string_elt(row, owned_paste::character(access, &joined.data, encoding)?)?;
        access.require_active()?;
    }
    access.require_active()?;
    Ok(result.freeze())
}
