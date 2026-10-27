use super::*;

#[cfg(test)]
#[path = "raw_codec_tests.rs"]
mod workspace_tests;

// Raw entry points retain only an original operation pin and checked native
// snapshots. All codec allocation and framing lives in the safe Rust module.
pub(super) unsafe fn raw_snapshot(
    owner: &crate::sexp::owner::OwnerToken<'_>,
    input: SEXP,
    mode: u8,
    encode: bool,
) -> super::owned_input::InputBytes {
    unsafe {
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
            RAW(input.as_raw());
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

unsafe fn raw_codec_operation(input: SEXP, mode: u8, decode_error: Option<*mut Rboolean>) -> SEXP {
    unsafe {
        let owner =
            crate::sexp::owner::OwnerToken::current().unwrap_or_else(|e| error(&e.to_string()));
        let _pin = owner.pin().unwrap_or_else(|e| error(&e.to_string()));
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let bytes = raw_snapshot(&owner, input, mode, decode_error.is_none());
            if let Some(invalid) = decode_error {
                match super::raw_codecs::decode(&owner, &bytes, mode) {
                    Ok(decoded) => raw_from_bytes(&decoded),
                    Err(super::raw_codecs::DecodeError::Resource(message)) => error(&message),
                    Err(super::raw_codecs::DecodeError::Invalid) => {
                        mark_decompress_error(invalid);
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

pub unsafe fn R_compress1(input: SEXP) -> SEXP {
    unsafe { raw_codec_operation(input, 1, None) }
}
pub unsafe fn R_compress2(input: SEXP) -> SEXP {
    unsafe { raw_codec_operation(input, 2, None) }
}
pub unsafe fn R_compress3(input: SEXP) -> SEXP {
    unsafe { raw_codec_operation(input, 3, None) }
}
pub unsafe fn R_decompress1(input: SEXP, error: *mut Rboolean) -> SEXP {
    unsafe { raw_codec_operation(input, 1, Some(error)) }
}
pub unsafe fn R_decompress2(input: SEXP, error: *mut Rboolean) -> SEXP {
    unsafe { raw_codec_operation(input, 2, Some(error)) }
}
pub unsafe fn R_decompress3(input: SEXP, error: *mut Rboolean) -> SEXP {
    unsafe { raw_codec_operation(input, 3, Some(error)) }
}

// ---------------------------------------------------------------------------
// R_snprintf utility
// ---------------------------------------------------------------------------

pub unsafe fn Rsnprintf(buf: *mut c_char, size: usize, _format: *const c_char) -> c_int {
    unsafe {
        if !buf.is_null() && size > 0 {
            *buf = 0;
        }
        0
    }
}

// ---------------------------------------------------------------------------
// InStringStream helper
// ---------------------------------------------------------------------------

#[repr(C)]
pub struct R_instring_stream_st {
    pub last: c_int,
    pub stream: R_inpstream_t,
}

pub type R_instring_stream_t = *mut R_instring_stream_st;

pub unsafe fn InitInStringStream(s: R_instring_stream_t, stream: R_inpstream_t) {
    unsafe {
        if !s.is_null() {
            (*s).last = 0;
            (*s).stream = stream;
        }
    }
}

pub unsafe fn GetChar(s: R_instring_stream_t) -> c_int {
    -1
}

pub unsafe fn UngetChar(s: R_instring_stream_t, c: c_int) {
    unsafe {
        if !s.is_null() {
            (*s).last = c;
        }
    }
}
