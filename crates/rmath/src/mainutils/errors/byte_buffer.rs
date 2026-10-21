#![forbid(unsafe_code)]
//! Fixed error-state bytes are copied before any runtime lend or constructor.
use crate::sexp::{
    encoding::{self, Mode, NulText, TextEncoding},
    ffi::SEXPTYPE,
    object::{Sexp, SexpError, SexpMut, SexpResult},
    owner::RuntimeAccess,
};

pub(super) struct Snapshot {
    bytes: [u8; super::BUFSIZE + 1],
    len: usize,
}
impl Snapshot {
    pub(super) fn stored(bytes: &[u8; super::BUFSIZE + 1]) -> Self {
        let len = bytes.iter().position(|&b| b == 0).unwrap_or(super::BUFSIZE);
        Self { bytes: *bytes, len }
    }

    pub(super) fn copy(mut byte: impl FnMut(usize) -> Option<u8>, utf8: bool) -> Self {
        let mut bytes = [0; super::BUFSIZE + 1];
        let mut len = 0;
        while len < super::BUFSIZE - 1 {
            let Some(value) = byte(len) else { break };
            if value == 0 {
                break;
            }
            bytes[len] = value;
            len += 1;
        }
        // GNU Rstrncpy always repairs the last sequence, even without clipping.
        // Earlier invalid bytes remain intact: UTF8 is self-synchronizing.
        if utf8 {
            let valid = encoding::utf8_tail_prefix_len(&bytes[..len]);
            bytes[valid..len].fill(0);
            len = valid;
        }
        Self { bytes, len }
    }

    pub(super) fn into_array(self) -> [u8; super::BUFSIZE + 1] {
        self.bytes
    }

    pub(super) fn value(&self, access: &RuntimeAccess) -> SexpResult<Sexp<'static>> {
        let domain = access.domain();
        let allocator = access.allocator(&domain)?;
        let mut value = SexpMut::try_from_checked(
            allocator.allocate(|a| Some(a.alloc_vector(SEXPTYPE::STRSXP, 1)))?,
        )?;
        let character = allocator.allocate(|a| {
            encoding::allocate_character(a, &self.bytes[..self.len], TextEncoding::Native)
        })?;
        value.try_set_string_elt(0, character)?;
        Ok(value.freeze())
    }
}

pub(super) fn argument(access: &RuntimeAccess, args: &Sexp<'_>) -> SexpResult<Snapshot> {
    access.require_active()?;
    let domain = access.domain();
    domain.link(args)?;
    let value = args.clone().try_pairlist_arg(0)?.into_owned()?;
    domain.link(&value)?;
    if value.typeof_() != SEXPTYPE::STRSXP || value.len() != 1 {
        return Err(SexpError::EvaluationFailed {
            message: "error message must be a character string".into(),
        });
    }
    let character = value.try_string_elt(0)?.into_owned()?;
    access.require_active()?;
    domain.link(&character)?;
    let text = NulText::new(&character, Mode::Bytes, args)?;
    let snapshot = Snapshot::copy(
        |i| text.byte(i),
        crate::mainutils::platform::locale_is_utf8(),
    );
    access.require_active()?;
    Ok(snapshot)
}
