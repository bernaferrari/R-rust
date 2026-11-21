/*
 *  R : A Computer Language for Statistical Data Analysis
 *  Copyright (C) 2020-2022 The R Core Team.
 *
 *  Ported to Rust for the crates project.
 *  Based on R's r-source/src/library/tools/src/pdscan.c
 *
 *  This program is free software; you can redistribute it and/or modify
 *  it under the terms of the GNU General Public License as published by
 *  the Free Software Foundation; either version 2 of the License, or
 *  (at your option) any later version.
 */

use crate::sexp::{
    encoding::{self, Mode, TextEncoding},
    ffi::{SEXP, SEXPTYPE},
    object::{Sexp, SexpError, SexpMut, SexpResult},
    owner::{OwnerToken, RuntimeAccess, with_runtime},
};

fn names(bytes: &[u8]) -> impl Iterator<Item = &[u8]> {
    bytes
        .split(|byte| !(byte.is_ascii_alphanumeric() || *byte == b'.'))
        .filter_map(|span| {
            let start = span.iter().position(u8::is_ascii_alphabetic)?;
            let name = &span[start..];
            (name != b"R").then_some(name)
        })
}

fn scan(access: &RuntimeAccess, input: &Sexp<'static>) -> SexpResult<Sexp<'static>> {
    if input.typeof_() != SEXPTYPE::STRSXP {
        return Err(SexpError::EvaluationFailed {
            message: "non-character argument".into(),
        });
    }
    let domain = access.domain();
    let allocator = access.allocator(&domain)?;
    let vector = |kind, length| {
        SexpMut::try_from_checked(
            allocator.allocate(|arena| Some(arena.alloc_vector(kind, length)))?,
        )
    };
    let length = input.len();
    access.require_active()?;
    let scan_part = |index| {
        let value = input.try_string_elt(index)?.into_owned()?;
        access.require_active()?;
        if value.is_na_string() {
            return Ok(vector(SEXPTYPE::STRSXP, 0)?.freeze());
        }
        let data = encoding::translate(&value, Mode::Bytes, input)?;
        let count = names(&data.data).count() as i64;
        let mut part = vector(SEXPTYPE::STRSXP, count)?;
        for (offset, name) in names(&data.data).enumerate() {
            // The package-name grammar only returns ASCII bytes.
            let token = allocator.allocate(|arena| {
                encoding::allocate_character(arena, name, TextEncoding::Native)
            })?;
            part.try_set_string_elt(offset as i64, token)?;
        }
        Ok(part.freeze())
    };
    match length {
        0 => return Ok(vector(SEXPTYPE::STRSXP, 0)?.freeze()),
        1 => return scan_part(0),
        _ => {}
    }

    // Resolve each element once. Keep the scanned vectors rooted while a
    // later ALTREP element or an allocation can call back into this runtime.
    let mut pieces = vector(SEXPTYPE::VECSXP, length)?;
    let mut total = 0i64;
    for index in 0..length {
        let part = scan_part(index)?;
        total = total
            .checked_add(part.len())
            .ok_or(SexpError::AllocationFailed {
                object: "package dependency names",
            })?;
        pieces.try_set_vector_elt(index, part)?;
    }
    let pieces = pieces.freeze();
    let mut result = vector(SEXPTYPE::STRSXP, total)?;
    let mut offset = 0;
    for index in 0..pieces.len() {
        let part = pieces.try_vector_elt(index)?;
        for item in 0..part.len() {
            result.try_set_string_elt(offset, part.try_string_elt(item)?.into_owned()?)?;
            offset += 1;
        }
    }
    access.require_active()?;
    Ok(result.freeze())
}

pub(super) unsafe extern "C-unwind" fn package_dependencies_scan(input: SEXP) -> SEXP {
    let result = (|| {
        OwnerToken::with_current(|owner| {
            let value = owner.sexp(input)?.into_owned()?;
            let runtime = owner.weak_owner().ok_or(SexpError::RootUnavailable)?;
            with_runtime(&runtime, |access| scan(access, &value))?
        })?
    })();
    result
        .unwrap_or_else(|error| match error {
            SexpError::EvaluationFailed { message } => crate::sexp::context::r_error(message),
            other => crate::sexp::context::r_error(other.to_string()),
        })
        .as_raw()
}
