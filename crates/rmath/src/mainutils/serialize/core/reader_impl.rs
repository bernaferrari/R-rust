use super::*;

impl<'a> BinaryReader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        BinaryReader {
            data,
            pos: 0,
            ascii_body: false,
            xdr_body: false,
            item_depth: 0,
            bc_source_depth: 0,
            persist_hook: ptr::null_mut(),
            persist_hook_func: None,
            persist_hook_data: ptr::null_mut(),
            lazy_restore: None,
            persist_cache: std::collections::HashMap::new(),
            pull: None,
        }
    }

    /// Build a reader whose bytes arrive on demand through `fill(buf, need)`,
    /// which must grow `buf` to length `need` (or return false at end of
    /// input). The source is never read beyond what the decoder asks for.
    pub fn new_pull(fill: &'a mut dyn FnMut(&mut Vec<u8>, usize) -> bool) -> Self {
        BinaryReader {
            data: &[],
            pos: 0,
            ascii_body: false,
            xdr_body: false,
            item_depth: 0,
            bc_source_depth: 0,
            persist_hook: ptr::null_mut(),
            persist_hook_func: None,
            persist_hook_data: ptr::null_mut(),
            lazy_restore: None,
            persist_cache: std::collections::HashMap::new(),
            pull: Some(PullInput {
                buf: Vec::new(),
                fill,
            }),
        }
    }

    pub fn set_ascii_body(&mut self, ascii_body: bool) {
        self.ascii_body = ascii_body;
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

    pub fn remaining(&self) -> usize {
        match &self.pull {
            // A pull source has no known end; admission therefore runs
            // against the bytes actually buffered, and callers must have
            // pulled the declared payload's lower bound via `ensure()` first
            // (GNU reads until end of input, so no static length exists).
            Some(p) => p.buf.len().saturating_sub(self.pos),
            None => self.data.len().saturating_sub(self.pos),
        }
    }

    /// The bytes currently available to the decoder.
    pub(crate) fn src(&self) -> &[u8] {
        match &self.pull {
            Some(p) => &p.buf,
            None => self.data,
        }
    }

    /// Ensure `count` more bytes are readable at `pos`. Pull mode requests
    /// exactly the missing bytes from the source.
    pub(crate) fn ensure(&mut self, count: usize) -> bool {
        let Some(needed) = self.pos.checked_add(count) else {
            return false;
        };
        match self.pull.as_mut() {
            Some(p) => (p.fill)(&mut p.buf, needed),
            None => self.data.len() >= needed,
        }
    }

    /// Next byte without consuming it; pulls one byte in pull mode.
    pub(crate) fn peek_byte(&mut self) -> Option<u8> {
        if !self.ensure(1) {
            return None;
        }
        Some(self.src()[self.pos])
    }

    pub(crate) fn next_byte(&mut self) -> Option<u8> {
        let byte = self.peek_byte()?;
        self.pos += 1;
        Some(byte)
    }

    /// Pull mode only: bytes buffered but not consumed by the decode. The
    /// caller must return them to the source (e.g. connection pushback) so
    /// trailing data is not lost.
    pub fn take_leftover(&mut self) -> Vec<u8> {
        match self.pull.as_mut() {
            Some(p) => {
                let rest = p.buf[self.pos..].to_vec();
                p.buf.clear();
                self.pos = 0;
                rest
            }
            None => Vec::new(),
        }
    }

    pub fn read_i32(&mut self) -> Result<i32, String> {
        if self.ascii_body {
            let token = self.read_ascii_token()?;
            if token == "NA" {
                return Ok(crate::sexp::ffi::NA_INTEGER);
            }
            return token
                .parse::<i32>()
                .map_err(|_| format!("read error: invalid integer token '{token}'"));
        }
        if !self.ensure(4) {
            return Err("read error: not enough bytes for i32".into());
        }
        let bytes: [u8; 4] = self.src()[self.pos..self.pos + 4]
            .try_into()
            .unwrap_or([0; 4]);
        self.pos += 4;
        Ok(if self.xdr_body {
            i32::from_be_bytes(bytes)
        } else {
            i32::from_ne_bytes(bytes)
        })
    }

    pub fn read_f64(&mut self) -> Result<f64, String> {
        if self.ascii_body {
            let token = self.read_ascii_token()?;
            return super::ascii_numbers::parse_real(&token)
                .map_err(|_| format!("read error: invalid real token '{token}'"));
        }
        if !self.ensure(8) {
            return Err("read error: not enough bytes for f64".into());
        }
        let bytes: [u8; 8] = self.src()[self.pos..self.pos + 8]
            .try_into()
            .unwrap_or([0; 8]);
        self.pos += 8;
        Ok(if self.xdr_body {
            f64::from_be_bytes(bytes)
        } else {
            f64::from_ne_bytes(bytes)
        })
    }

    pub fn read_byte(&mut self) -> Result<u8, String> {
        if self.ascii_body {
            let token = self.read_ascii_token()?;
            return u8::from_str_radix(&token, 16)
                .map_err(|_| format!("read error: invalid byte token '{token}'"));
        }
        self.next_byte()
            .ok_or_else(|| "read error: not enough bytes for byte".to_string())
    }

    /// The returned slice borrows the reader's buffer; in pull mode any later
    /// read may reallocate it, so consume the slice before reading on.
    pub fn read_bytes(&mut self, len: usize) -> Result<&[u8], String> {
        if self.ascii_body {
            self.skip_ascii_whitespace();
        }
        let start = self.pos;
        if !self.ensure(len) {
            return Err(format!("read error: not enough bytes for {} bytes", len));
        }
        self.pos += len;
        if self.ascii_body && self.peek_byte() == Some(b'\n') {
            self.pos += 1;
        }
        Ok(&self.src()[start..start + len])
    }

    /// A vector cannot contain more elements than the remaining stream can
    /// encode. Check this before allocating its R payload, even without a budget.
    pub(crate) fn read_vector_length(
        &mut self,
        binary_element_bytes: usize,
    ) -> Result<i32, String> {
        let len = self.read_i32()?;
        // Pull-mode streams have no known end, so a declared length is only
        // admissible once its minimum encoding is actually buffered: the fill
        // callback stops at end of input without pre-sizing, so a short
        // malicious header is rejected here instead of driving a huge
        // payload allocation. Slice mode `ensure` is the same presence check
        // the admission below performs.
        if let Ok(count) = usize::try_from(len) {
            let minimum = if self.ascii_body {
                1
            } else {
                binary_element_bytes
            };
            let admissible = count
                .checked_mul(minimum)
                .map(|needed| self.ensure(needed))
                .unwrap_or(false);
            if !admissible {
                return Err("read error: truncated vector payload".to_string());
            }
        }
        vector_length_fits(len, self.remaining(), binary_element_bytes, self.ascii_body)
    }

    /// Connection object vectors are decoded incrementally before allocating
    /// their final payload. Reading a lower bound for every later child here
    /// would advance the connection past restoration callbacks in early children.
    pub(crate) fn read_object_vector_length(&mut self) -> Result<i32, String> {
        if self.pull.is_none() {
            return self.read_vector_length(4);
        }
        let len = self.read_i32()?;
        if len < 0 {
            return Err("read error: negative vector length".into());
        }
        Ok(len)
    }

    pub fn read_string_bytes(&mut self, len: usize) -> Result<Vec<u8>, String> {
        if !self.ascii_body {
            return self.read_bytes(len).map(|bytes| bytes.to_vec());
        }

        self.skip_ascii_whitespace();
        // Escapes consume at least one input byte per decoded byte. Pull-mode
        // streams must buffer that lower bound before any reservation; the
        // fill callback stops at end of input without pre-sizing. Never
        // reserve a stream-declared allocation before checking that bound.
        if !self.ensure(len) || len > self.remaining() {
            return Err("read error: truncated ASCII string".into());
        }
        let mut out = Vec::new();
        out.try_reserve_exact(len)
            .map_err(|_| "read error: ASCII string allocation failed".to_string())?;
        while out.len() < len {
            let byte = self
                .next_byte()
                .ok_or_else(|| "read error: unexpected end of ASCII string".to_string())?;
            if byte != b'\\' {
                out.push(byte);
                continue;
            }
            let escaped = self
                .next_byte()
                .ok_or_else(|| "read error: truncated ASCII string escape".to_string())?;
            match escaped {
                b'n' => out.push(b'\n'),
                b't' => out.push(b'\t'),
                b'v' => out.push(0x0b),
                b'b' => out.push(0x08),
                b'r' => out.push(b'\r'),
                b'f' => out.push(0x0c),
                b'a' => out.push(0x07),
                b'\\' => out.push(b'\\'),
                b'?' => out.push(b'?'),
                b'\'' => out.push(b'\''),
                b'"' => out.push(b'"'),
                b'0'..=b'7' => {
                    let mut value = (escaped - b'0') as u8;
                    let mut digits = 1usize;
                    while digits < 3 {
                        let Some(digit) = self.peek_byte() else {
                            break;
                        };
                        if !matches!(digit, b'0'..=b'7') {
                            break;
                        }
                        value = value.saturating_mul(8).saturating_add(digit - b'0');
                        self.pos += 1;
                        digits += 1;
                    }
                    out.push(value);
                }
                other => out.push(other),
            }
        }
        Ok(out)
    }

    pub fn read_ascii_token(&mut self) -> Result<String, String> {
        self.skip_ascii_whitespace();
        let start = self.pos;
        loop {
            if self.pos - start == 127 {
                return Err("read error: ASCII token exceeds 127 bytes".into());
            }
            match self.peek_byte() {
                Some(byte) if !byte.is_ascii_whitespace() => self.pos += 1,
                _ => break,
            }
        }
        if start == self.pos {
            return Err("read error: missing ASCII token".to_string());
        }
        let token = self.src()[start..self.pos].to_vec();
        // GNU InWord reads and discards the first terminating whitespace.
        // It does not leave that delimiter in stream pushback.
        if self
            .peek_byte()
            .is_some_and(|byte| byte.is_ascii_whitespace())
        {
            self.pos += 1;
        }
        std::str::from_utf8(&token)
            .map(|s| s.to_string())
            .map_err(|_| "read error: invalid ASCII token".to_string())
    }

    pub fn skip_ascii_whitespace(&mut self) {
        while self
            .peek_byte()
            .is_some_and(|byte| byte.is_ascii_whitespace())
        {
            self.pos += 1;
        }
    }
}
