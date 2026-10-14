use super::*;

/// Internal serializer that writes to a Vec<u8>.
pub struct BinaryWriter {
    pub buf: Vec<u8>,
    /// Serialization format version (2 or 3); ALTREP wiring is v3-only.
    pub version: c_int,
    pub ascii_body: bool,
    pub(crate) ascii_hex: bool,
    pub xdr_body: bool,
    pub(crate) item_depth: usize,
    pub(crate) persist_hook: SEXP,
    pub(crate) persist_hook_func: Option<unsafe extern "C" fn(SEXP, SEXP) -> SEXP>,
    pub(crate) persist_hook_data: SEXP,
}

impl BinaryWriter {
    pub fn new() -> Self {
        BinaryWriter {
            buf: Vec::new(),
            version: 3,
            ascii_body: false,
            ascii_hex: false,
            xdr_body: false,
            item_depth: 0,
            persist_hook: ptr::null_mut(),
            persist_hook_func: None,
            persist_hook_data: ptr::null_mut(),
        }
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

    pub fn set_c_persist_hook(
        &mut self,
        func: Option<unsafe extern "C" fn(SEXP, SEXP) -> SEXP>,
        data: SEXP,
    ) {
        self.persist_hook_func = func;
        self.persist_hook_data = data;
    }

    pub fn write_i32(&mut self, val: i32) {
        if self.ascii_body {
            self.buf
                .extend_from_slice(super::ascii_numbers::integer(val).as_bytes());
            self.buf.push(b'\n');
        } else if self.xdr_body {
            self.buf.extend_from_slice(&val.to_be_bytes());
        } else {
            self.buf.extend_from_slice(&val.to_ne_bytes());
        }
    }

    pub fn write_f64(&mut self, val: f64) {
        if self.ascii_body {
            self.buf
                .extend_from_slice(super::ascii_numbers::real(val, self.ascii_hex).as_bytes());
            self.buf.push(b'\n');
        } else if self.xdr_body {
            self.buf.extend_from_slice(&val.to_be_bytes());
        } else {
            self.buf.extend_from_slice(&val.to_ne_bytes());
        }
    }

    pub fn write_byte(&mut self, val: u8) {
        if self.ascii_body {
            self.buf.extend_from_slice(format!("{val:02x}").as_bytes());
            self.buf.push(b'\n');
        } else {
            self.buf.push(val);
        }
    }

    pub fn write_bytes(&mut self, data: &[u8]) {
        self.buf.extend_from_slice(data);
    }

    pub fn write_string_bytes(&mut self, s: *const c_char, len: i32) {
        if len > 0 && !s.is_null() {
            // SAFETY: Caller ensures s points to at least `len` valid bytes.
            let slice = unsafe { slice::from_raw_parts(s as *const u8, len as usize) };
            if self.ascii_body {
                for &byte in slice {
                    match byte {
                        b'\n' => self.buf.extend_from_slice(b"\\n"),
                        b'\t' => self.buf.extend_from_slice(b"\\t"),
                        0x0b => self.buf.extend_from_slice(b"\\v"),
                        0x08 => self.buf.extend_from_slice(b"\\b"),
                        b'\r' => self.buf.extend_from_slice(b"\\r"),
                        0x0c => self.buf.extend_from_slice(b"\\f"),
                        0x07 => self.buf.extend_from_slice(b"\\a"),
                        b'\\' => self.buf.extend_from_slice(b"\\\\"),
                        b'?' => self.buf.extend_from_slice(b"\\?"),
                        b'\'' => self.buf.extend_from_slice(b"\\'"),
                        b'"' => self.buf.extend_from_slice(b"\\\""),
                        0..=32 | 127..=255 => {
                            self.buf
                                .extend_from_slice(format!("\\{byte:03o}").as_bytes());
                        }
                        _ => self.buf.push(byte),
                    }
                }
                self.buf.push(b'\n');
            } else {
                self.buf.extend_from_slice(slice);
            }
        }
    }

    pub fn into_vec(self) -> Vec<u8> {
        self.buf
    }
}
