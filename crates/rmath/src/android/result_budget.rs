//! Admission checks before recursively formatting or copying an R result.
//! Counts occurrences, not unique objects: shared lists expand during export.
#![forbid(unsafe_code)]
use crate::sexp::{ffi::SEXPTYPE, object::Sexp};

pub(crate) fn admit(value: Sexp<'_>, limit: usize) -> Result<(), &'static str> {
    let mut remaining = limit;
    let mut pending = vec![(value, 0usize)];
    while let Some((value, depth)) = pending.pop() {
        if depth > 64 {
            return Err("result nesting exceeds export limit (64)");
        }
        let kind = value.typeof_();
        let len = usize::try_from(value.len()).unwrap_or(usize::MAX);
        // Includes per-element formatting/owned-value overhead. This is an
        // admission budget, deliberately more conservative than payload size.
        let cost = match kind {
            SEXPTYPE::CHARSXP => len.saturating_mul(8).saturating_add(128),
            SEXPTYPE::INTSXP
            | SEXPTYPE::REALSXP
            | SEXPTYPE::LGLSXP
            | SEXPTYPE::CPLXSXP
            | SEXPTYPE::RAWSXP
            | SEXPTYPE::STRSXP
            | SEXPTYPE::VECSXP
            | SEXPTYPE::EXPRSXP => len.saturating_mul(64).saturating_add(128),
            _ => 128,
        };
        remaining = remaining
            .checked_sub(cost)
            .ok_or("result exceeds export budget")?;
        if let Some(attributes) = value.attrib().filter(|v| !v.is_nil()) {
            pending.push((attributes, depth + 1));
        }
        match kind {
            SEXPTYPE::STRSXP => {
                for i in 0..value.len() {
                    let child = value
                        .try_string_elt(i)
                        .map_err(|_| "invalid string result")?;
                    pending.push((child, depth + 1));
                }
            }
            SEXPTYPE::VECSXP | SEXPTYPE::EXPRSXP => {
                for i in 0..value.len() {
                    let child = value.try_vector_elt(i).map_err(|_| "invalid list result")?;
                    pending.push((child, depth + 1));
                }
            }
            SEXPTYPE::LISTSXP | SEXPTYPE::LANGSXP | SEXPTYPE::DOTSXP => {
                for child in [value.car(), value.cdr(), value.tag()]
                    .into_iter()
                    .flatten()
                    .filter(|v| !v.is_nil())
                {
                    pending.push((child, depth + 1));
                }
            }
            SEXPTYPE::CLOSXP => {
                for child in [value.formals(), value.body()].into_iter().flatten() {
                    pending.push((child, depth + 1));
                }
            }
            SEXPTYPE::SYMSXP => {
                if let Some(name) = value.printname() {
                    pending.push((name, depth + 1));
                }
            }
            _ => {}
        }
    }
    Ok(())
}

pub(super) fn truncate(text: &mut String, limit: usize) {
    if text.len() <= limit {
        return;
    }
    let mut boundary = limit;
    while !text.is_char_boundary(boundary) {
        boundary -= 1;
    }
    text.truncate(boundary);
    text.push_str("\n[result output truncated by runtime limit]");
}
