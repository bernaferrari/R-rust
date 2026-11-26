use super::*;
use crate::mainutils::serialize::owned_input::InputBytes;

/// The reservation travels with the storage through final R allocation or a
/// stream callback. Only low-level fixtures use an unadmitted buffer.
pub struct SerializedBytes {
    plain: Vec<u8>,
    admitted: Option<InputBytes>,
}

impl std::ops::Deref for SerializedBytes {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        self.admitted.as_deref().unwrap_or(&self.plain)
    }
}

impl SerializedBytes {
    #[inline]
    fn extend_from_slice(&mut self, bytes: &[u8]) {
        if let Some(buffer) = &mut self.admitted {
            use std::io::Write;
            buffer.write_all(bytes).unwrap_or_else(|_| {
                crate::sexp::context::r_error("serialize exceeds session memory limit")
            });
        } else {
            self.plain.extend_from_slice(bytes);
        }
    }

    #[inline]
    fn push(&mut self, byte: u8) {
        self.extend_from_slice(&[byte]);
    }
}

/// Internal serializer that writes to a Vec<u8>.
pub struct BinaryWriter {
    buf: SerializedBytes,
    /// Serialization format version (2 or 3); ALTREP wiring is v3-only.
    pub version: c_int,
    pub ascii_body: bool,
    pub(crate) ascii_hex: bool,
    pub xdr_body: bool,
    pub(crate) item_depth: usize,
    pub(crate) strict_workspace: bool,
    pub(crate) persist_hook: SEXP,
}

impl BinaryWriter {
    pub fn new() -> Self {
        BinaryWriter {
            buf: SerializedBytes {
                plain: Vec::new(),
                admitted: None,
            },
            version: 3,
            ascii_body: false,
            ascii_hex: false,
            xdr_body: false,
            item_depth: 0,
            strict_workspace: false,
            persist_hook: ptr::null_mut(),
        }
    }

    pub(crate) fn admitted(owner: &crate::sexp::owner::OwnerToken<'_>) -> Self {
        let reservation = owner
            .with_arena(|a| a.reserve_workspace(0))
            .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()))
            .unwrap_or_else(|| {
                crate::sexp::context::r_error("serialize exceeds session memory limit")
            });
        let mut writer = Self::new();
        writer.buf.admitted = Some(InputBytes::new(reservation, 0).unwrap_or_else(|_| {
            crate::sexp::context::r_error("serialize exceeds session memory limit")
        }));
        writer
    }

    pub fn set_ascii_body(&mut self, ascii_body: bool) {
        self.ascii_body = ascii_body;
    }

    pub fn set_ascii_hex(&mut self, ascii_hex: bool) {
        self.ascii_hex = ascii_hex;
    }

    pub fn set_xdr_body(&mut self, xdr_body: bool) {
        self.xdr_body = xdr_body;
    }

    pub fn set_persist_hook(&mut self, hook: SEXP) {
        self.persist_hook = hook;
    }

    pub fn write_i32(&mut self, val: i32) {
        self.buf.extend_from_slice(&val.to_be_bytes());
    }

    pub fn write_f64(&mut self, val: f64) {
        self.buf.extend_from_slice(&val.to_be_bytes());
    }

    pub fn write_byte(&mut self, val: u8) {
        self.buf.push(val);
    }

    pub fn write_bytes(&mut self, data: &[u8]) {
        self.buf.extend_from_slice(data);
    }

    pub fn write_string_bytes(&mut self, bytes: &[u8]) {
        self.buf.extend_from_slice(bytes);
    }

    pub fn into_output(self) -> SerializedBytes {
        self.buf
    }

    #[cfg(test)]
    pub fn into_vec(self) -> Vec<u8> {
        assert!(
            self.buf.admitted.is_none(),
            "fixtures only; production retains admission"
        );
        self.buf.plain
    }
}
