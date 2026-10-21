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
    pub(super) fn empty() -> Self {
        Self {
            bytes: [0; super::BUFSIZE + 1],
            len: 0,
        }
    }

    /// Copy native bytes before any error-state lend. This performs no repair.
    pub(super) fn raw(mut byte: impl FnMut(usize) -> Option<u8>) -> Self {
        let mut snapshot = Self::empty();
        while snapshot.len < super::BUFSIZE {
            let Some(value) = byte(snapshot.len) else {
                break;
            };
            if value == 0 {
                break;
            }
            snapshot.bytes[snapshot.len] = value;
            snapshot.len += 1;
        }
        snapshot
    }

    pub(super) fn as_bytes(&self) -> &[u8] {
        &self.bytes[..self.len]
    }

    pub(super) fn len(&self) -> usize {
        self.len
    }

    pub(super) fn clipped(mut self, limit: usize, utf8: bool) -> Self {
        if self.len > limit {
            let mut length = limit;
            if utf8 {
                length = encoding::utf8_tail_prefix_len(&self.bytes[..length]);
            }
            self.bytes[length..self.len].fill(0);
            self.len = length;
        }
        self
    }

    /// GNU ERRBUFCAT skips a complete fragment that would not fit.
    pub(super) fn append(&mut self, bytes: &[u8]) {
        if self.len + bytes.len() < super::BUFSIZE {
            self.append_bounded(bytes);
        }
    }

    pub(super) fn append_bounded(&mut self, bytes: &[u8]) {
        let count = bytes.len().min(super::BUFSIZE - self.len);
        self.bytes[self.len..self.len + count].copy_from_slice(&bytes[..count]);
        self.len += count;
    }

    pub(super) fn normalized(self, utf8: bool) -> Self {
        Self::copy(|i| self.bytes.get(i).copied(), utf8)
    }

    pub(super) fn host_text(bytes: &[u8]) -> String {
        let mut text = String::new();
        crate::sexp::output::visit_capture_text(bytes, |part| {
            text.push_str(part);
            true
        });
        text
    }

    pub(super) fn bare_host_text(&self) -> String {
        let mut payload = self.as_bytes();
        if let Some(rest) = payload.strip_prefix(b"Error in ") {
            if let Some(index) = rest.windows(3).position(|part| part == b" : ") {
                if index + 3 < rest.len() {
                    payload = &rest[index + 3..];
                }
            }
        } else if let Some(rest) = payload.strip_prefix(b"Error: ") {
            if !rest.is_empty() {
                payload = rest;
            }
        }
        Self::host_text(payload)
    }

    /// GNU errors.c layout, before the optional Calls fragment. All inputs
    /// are owned snapshots or Rust text; no runtime data loan crosses a call.
    pub(super) fn render_error(
        body: &Self,
        call: Option<&Self>,
        location: &[u8],
        utf8: bool,
    ) -> Self {
        let mut output = Self::empty();
        if let Some(call) = call {
            if b"Error in ".len() + b"\n  ".len() + body.len() < super::BUFSIZE {
                output.append(b"Error in ");
                output.append(call.as_bytes());
                output.append(location);
                output.append(b" : ");
                let first = body
                    .as_bytes()
                    .iter()
                    .position(|&b| b == b'\n')
                    .unwrap_or(body.len());
                let width = |bytes: &[u8]| -> i64 {
                    if !utf8 {
                        return bytes.len() as i64;
                    }
                    let Ok(text) = std::str::from_utf8(bytes) else {
                        return -1;
                    };
                    let count = text.chars().count();
                    if count >= 2000 {
                        count as i64
                    } else {
                        text.chars()
                            .map(crate::mainutils::unicode_width::columns)
                            .sum::<usize>() as i64
                    }
                };
                if 14 + width(call.as_bytes()) + width(&body.as_bytes()[..first])
                    > super::LONGWARN as i64
                {
                    output.append(b"\n  ");
                }
                output.append(body.as_bytes());
            } else {
                output.append(b"Error: ");
                output.append(body.as_bytes());
            }
        } else {
            output.append(b"Error: ");
            output.append(body.as_bytes());
            output.append(location);
        }
        if output.len() > super::BUFSIZE - 1 {
            let end = (output.len() + 1).min(super::BUFSIZE + 1 - 4);
            output = output.clipped(end - 1, utf8);
            output.append_bounded(b"...\n");
        } else if output.as_bytes().last() != Some(&b'\n') {
            output.append_bounded(b"\n");
        }
        output
    }

    pub(super) fn append_trace(&mut self, trace: &[u8]) {
        if !trace.is_empty() && trace.len() + self.len() + b"Calls:".len() + 2 < super::BUFSIZE + 1
        {
            self.append_bounded(b"Calls: ");
            self.append_bounded(trace);
            self.append_bounded(b"\n");
        }
    }

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
