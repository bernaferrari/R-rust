use super::*;

/// Internal deserializer that reads from a &[u8].
pub struct BinaryReader<'a> {
    pub data: &'a [u8],
    pub pos: usize,
    pub ascii_body: bool,
    pub xdr_body: bool,
    pub(crate) item_depth: usize,
    pub(crate) bc_source_depth: usize,
    pub(crate) persist_hook: SEXP,
    pub(crate) persist_hook_func: Option<unsafe extern "C" fn(SEXP, SEXP) -> SEXP>,

    pub(crate) persist_hook_data: SEXP,
    pub(crate) lazy_restore: Option<super::api::LazyLoadRestore>,
    pub(crate) persist_cache: std::collections::HashMap<String, SEXP>,
    /// Pull-mode byte source: bytes are fetched on demand, exactly as the
    /// decoder requests them (GNU `ConnReadChar` data flow).
    pub(crate) pull: Option<PullInput<'a>>,
}

/// Owning buffer plus a fill callback that grows `buf` to a requested
/// absolute length by reading exactly the missing bytes from a source.
pub(crate) struct PullInput<'a> {
    pub(crate) buf: Vec<u8>,
    pub(crate) fill: &'a mut dyn FnMut(&mut Vec<u8>, usize) -> bool,
}

/// Why a declared vector length is not admitted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum VectorLengthReject {
    Negative,
    ZeroWidth,
    Truncated,
}

/// Admit a declared vector length only when the remaining bytes can hold it.
///
/// `element_bytes == 0` in the binary body is [`VectorLengthReject::ZeroWidth`],
/// not a division. ASCII bodies use a minimum of one byte per declared element.
pub(crate) fn vector_length_decision(
    len: i32,
    remaining: usize,
    element_bytes: usize,
    ascii: bool,
) -> Result<i32, VectorLengthReject> {
    let count = usize::try_from(len).map_err(|_| VectorLengthReject::Negative)?;
    let minimum = if ascii { 1 } else { element_bytes };
    if minimum == 0 {
        return Err(VectorLengthReject::ZeroWidth);
    }
    if count > remaining / minimum {
        return Err(VectorLengthReject::Truncated);
    }
    Ok(len)
}

pub(crate) fn vector_length_fits(
    len: i32,
    remaining: usize,
    element_bytes: usize,
    ascii: bool,
) -> Result<i32, String> {
    vector_length_decision(len, remaining, element_bytes, ascii).map_err(|reject| match reject {
        VectorLengthReject::Negative => "read error: negative vector length".to_string(),
        VectorLengthReject::ZeroWidth => "read error: zero vector element width".to_string(),
        VectorLengthReject::Truncated => "read error: truncated vector payload".to_string(),
    })
}

#[cfg(test)]
mod vector_length_fits_tests {
    use super::vector_length_fits;

    #[test]
    fn zero_element_width_is_an_error() {
        let err = vector_length_fits(1, 8, 0, false).unwrap_err();
        assert!(err.contains("zero vector element width"));
    }

    #[test]
    fn extremes_match_the_admission_rule() {
        assert!(
            vector_length_fits(i32::MIN, 0, 1, false)
                .unwrap_err()
                .contains("negative")
        );
        assert_eq!(vector_length_fits(0, 0, 4, false).unwrap(), 0);
        assert!(vector_length_fits(i32::MAX, 4, 4, false).is_err());
    }
}
mod pull_admission_tests {
    #[allow(unused_imports)]
    use super::BinaryReader;

    #[test]
    fn pull_mode_short_stream_huge_declared_length_is_truncated() {
        // A 4-byte header declaring INT32_MAX doubles followed by end of
        // input must be rejected without attempting the payload allocation.
        let mut served = false;
        let mut fill = move |buf: &mut Vec<u8>, need: usize| -> bool {
            if served {
                return false;
            }
            buf.extend_from_slice(&i32::MAX.to_ne_bytes());
            served = true;
            buf.len() >= need
        };
        let mut reader = BinaryReader::new_pull(&mut fill);
        let rejected = reader
            .read_vector_length(8)
            .expect_err("huge declared length must be rejected");
        assert!(rejected.contains("truncated vector payload"), "{rejected}");
    }

    #[test]
    fn pull_mode_declared_length_with_buffered_payload_is_admitted() {
        let mut bytes: Vec<u8> = Vec::new();
        bytes.extend_from_slice(&3_i32.to_ne_bytes());
        bytes.extend_from_slice(&[0u8; 24]);
        let mut cursor = 0usize;
        let mut fill = move |buf: &mut Vec<u8>, need: usize| -> bool {
            while buf.len() < need && cursor < bytes.len() {
                buf.push(bytes[cursor]);
                cursor += 1;
            }
            buf.len() >= need
        };
        let mut reader = BinaryReader::new_pull(&mut fill);
        assert_eq!(reader.read_vector_length(8).unwrap(), 3);
    }
}

#[cfg(kani)]
mod vector_length_kani {
    use super::{VectorLengthReject, vector_length_decision};

    #[kani::proof]
    fn vector_length_fits_rejects() {
        let len: i32 = kani::any();
        let remaining: usize = kani::any();
        kani::assume(remaining <= 64);

        let element_bytes: usize = kani::any();
        kani::assume(matches!(element_bytes, 0 | 1 | 4 | 8 | 16));
        let ascii: bool = kani::any();
        let result = vector_length_decision(len, remaining, element_bytes, ascii);
        let minimum = if ascii { 1 } else { element_bytes };
        if len < 0 {
            assert_eq!(result, Err(VectorLengthReject::Negative));
        } else if minimum == 0 {
            assert_eq!(result, Err(VectorLengthReject::ZeroWidth));
        } else {
            let count = len as usize;
            assert_eq!(result.is_ok(), count <= remaining / minimum);
        }
        kani::cover(result.is_ok(), "accepted");
        kani::cover(result.is_err() && len >= 0, "rejected nonnegative");
    }
}
