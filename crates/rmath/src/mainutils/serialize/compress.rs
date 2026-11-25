use super::*;

#[cfg(test)]
#[path = "raw_codec_tests.rs"]
mod workspace_tests;

// Raw entry points retain only an original operation pin and checked native
// snapshots. All codec allocation and framing lives in the safe Rust module.
pub(super) fn raw_snapshot(
    owner: &crate::sexp::owner::OwnerToken<'_>,
    input: SEXP,
    mode: u8,
    encode: bool,
) -> super::owned_input::InputBytes {
    {
        let input = owner
            .sexp(input)
            .and_then(|v| v.into_owned())
            .unwrap_or_else(|e| error(&e.to_string()));
        if input.typeof_() != SEXPTYPE::RAWSXP {
            error(&format!(
                "R_{}compress{mode} requires a raw vector",
                if encode { "" } else { "de" }
            ));
        }
        if ALTREP(input.as_raw()) != 0 {
            input
                .materialize_compact_payload_for_raw()
                .unwrap_or_else(|e| error(&e.to_string()));
        }
        owner
            .require_active()
            .unwrap_or_else(|e| error(&e.to_string()));
        let length =
            usize::try_from(input.len()).unwrap_or_else(|_| error("invalid raw vector length"));
        if encode && length > u32::MAX as usize {
            error("raw vector too large to compress");
        }
        let mut bytes = super::owned_input::InputBytes::new(
            super::workspace::reserve(owner, length).unwrap_or_else(|e| error(&e)),
            length,
        )
        .unwrap_or_else(|e| error(&e.to_string()));
        if length == 0 {
            return bytes;
        }
        let header = input.header();
        let lease = header
            .payload_lease()
            .unwrap_or_else(|| error("invalid raw vector payload"));
        let mut chunk = [0u8; 8192];
        for start in (0..length).step_by(chunk.len()) {
            let count = (length - start).min(chunk.len());
            for (i, byte) in chunk[..count].iter_mut().enumerate() {
                *byte = lease
                    .byte_elt(start + i)
                    .unwrap_or_else(|| error("invalid raw vector payload"));
            }
            bytes
                .write_all(&chunk[..count])
                .unwrap_or_else(|e| error(&e.to_string()));
        }
        bytes
    }
}

/// Encode (`decode == false`) or decode `input`. When decoding, an invalid
/// payload sets `*error_flag` (if present) and returns `NULL`.
fn raw_codec_operation(
    input: SEXP,
    mode: u8,
    decode: bool,
    error_flag: Option<&mut Rboolean>,
) -> SEXP {
    {
        let owner = current_owner();
        let _pin = owner.pin().unwrap_or_else(|e| error(&e.to_string()));
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let bytes = raw_snapshot(&owner, input, mode, !decode);
            if decode {
                match super::raw_codecs::decode(&owner, &bytes, mode) {
                    Ok(decoded) => raw_from_bytes(&decoded),
                    Err(super::raw_codecs::DecodeError::Resource(message)) => error(&message),
                    Err(super::raw_codecs::DecodeError::Invalid) => {
                        if let Some(flag) = error_flag {
                            *flag = 1;
                        }
                        R_NilValue()
                    }
                }
            } else {
                let encoded =
                    super::raw_codecs::encode(&owner, &bytes, mode).unwrap_or_else(|e| error(&e));
                raw_from_bytes(&encoded)
            }
        }));
        owner
            .require_active()
            .unwrap_or_else(|e| error(&e.to_string()));
        match outcome {
            Ok(value) => value,
            Err(payload) => std::panic::resume_unwind(payload),
        }
    }
}

pub fn R_compress1(input: SEXP) -> SEXP {
    raw_codec_operation(input, 1, false, None)
}
pub fn R_compress2(input: SEXP) -> SEXP {
    raw_codec_operation(input, 2, false, None)
}
pub fn R_compress3(input: SEXP) -> SEXP {
    raw_codec_operation(input, 3, false, None)
}
/// # Safety
///
/// `error` must be null or valid for a write of one `Rboolean`.
pub unsafe fn R_decompress1(input: SEXP, error: *mut Rboolean) -> SEXP {
    // SAFETY: the caller guarantees `error` is null or a writable flag.
    raw_codec_operation(input, 1, true, unsafe { error.as_mut() })
}
/// # Safety
///
/// `error` must be null or valid for a write of one `Rboolean`.
pub unsafe fn R_decompress2(input: SEXP, error: *mut Rboolean) -> SEXP {
    // SAFETY: the caller guarantees `error` is null or a writable flag.
    raw_codec_operation(input, 2, true, unsafe { error.as_mut() })
}
/// # Safety
///
/// `error` must be null or valid for a write of one `Rboolean`.
pub unsafe fn R_decompress3(input: SEXP, error: *mut Rboolean) -> SEXP {
    // SAFETY: the caller guarantees `error` is null or a writable flag.
    raw_codec_operation(input, 3, true, unsafe { error.as_mut() })
}

// ---------------------------------------------------------------------------
// R_snprintf utility
// ---------------------------------------------------------------------------

