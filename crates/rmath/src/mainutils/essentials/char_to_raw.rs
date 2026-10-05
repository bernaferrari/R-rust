#![forbid(unsafe_code)]
//! Original encoded bytes, retained inputs, and admitted copy workspace.
use crate::sexp::{
    ffi::{NodeBody, SEXPTYPE},
    object::{Sexp, SexpError, SexpMut, SexpResult},
    owner::RuntimeAccess,
};
fn invalid_input() -> SexpError {
    SexpError::EvaluationFailed {
        message: "argument must be a character vector of length 1".to_owned(),
    }
}

pub(super) fn evaluate(
    access: &RuntimeAccess,
    arguments: Sexp<'static>,
) -> SexpResult<Sexp<'static>> {
    access.require_active()?;
    if arguments.is_nil() {
        return Err(invalid_input());
    }
    let input = arguments.try_car()?.into_owned()?;
    if input.typeof_() != SEXPTYPE::STRSXP || input.is_empty() {
        return Err(invalid_input());
    }
    // The provider can detach its container, collect, or revoke this runtime.
    // Keep its actual selected child before allocating any output.
    let character = input.try_string_elt(0)?.into_owned()?;
    access.require_active()?;
    if character.typeof_() != SEXPTYPE::CHARSXP {
        return Err(invalid_input());
    }
    let missing = character.is_na_string();
    let size = if missing {
        2
    } else {
        let NodeBody::Vector(vector) = character.header().body else {
            return Err(SexpError::MissingData {
                sexptype: SEXPTYPE::CHARSXP,
            });
        };
        usize::try_from(vector.length).map_err(|_| SexpError::AllocationFailed {
            object: "character byte count",
        })?
    };
    let byte_count =
        size.checked_mul(std::mem::size_of::<u8>())
            .ok_or(SexpError::AllocationFailed {
                object: "character copy size",
            })?;
    // The argument cell belongs to the original managed store even when the
    // selected character is an immutable singleton. This passive reservation
    // ends on every error/unwind; output payload bytes are charged separately.
    let node = arguments.allocation()?;
    let _workspace = node
        .heap_identity()
        .reserve_payload_bytes(node, byte_count)
        .ok_or(SexpError::AllocationFailed {
            object: "character copy workspace budget",
        })?;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(size)
        .map_err(|_| SexpError::AllocationFailed {
            object: "character byte copy",
        })?;
    if missing {
        bytes.extend_from_slice(b"NA");
    } else {
        // All payload inspection finishes before allocation/callbacks. Copy
        // bytes without UTF8 conversion, including declared byte encodings.
        let header = character.header();
        let payload = header.payload_lease().ok_or(SexpError::MissingData {
            sexptype: SEXPTYPE::CHARSXP,
        })?;
        if !payload.is_immutable() || payload.byte_elt(size) != Some(0) {
            return Err(SexpError::MissingData {
                sexptype: SEXPTYPE::CHARSXP,
            });
        }
        for index in 0..size {
            bytes.push(payload.byte_elt(index).ok_or(SexpError::MissingData {
                sexptype: SEXPTYPE::CHARSXP,
            })?);
        }
    }
    access.require_active()?;
    let domain = access.domain();
    let length = i64::try_from(size).map_err(|_| SexpError::AllocationFailed {
        object: "raw byte vector length",
    })?;
    let output = access
        .allocator(&domain)?
        .allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::RAWSXP, length)))?;
    let mut output = SexpMut::try_from_checked(output)?;
    for (index, byte) in bytes.into_iter().enumerate() {
        output.try_set_raw_elt(index as i64, byte)?;
    }
    access.require_active()?;
    Ok(output.freeze())
}
