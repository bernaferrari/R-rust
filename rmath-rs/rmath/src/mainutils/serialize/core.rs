use super::*;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Initial hash table size for write reference tracking.
pub const HASHSIZE: c_int = 1009;

/// Default serialize version (2 or 3).
pub const R_DEFAULT_SERIALIZE_VERSION: c_int = 3;

/// R version (4, 5, 0) packed as integer.
pub const R_VERSION_450: c_int = (4 << 16) | (5 << 8);

/// R version (2, 3, 0) packed as integer.
pub const R_VERSION_230: c_int = (2 << 16) | (3 << 8);

/// R version (3, 5, 0) packed as integer.
pub const R_VERSION_350: c_int = (3 << 16) | (5 << 8);

/// Writer R version in packed form.
pub const R_VERSION: c_int = R_VERSION_450;

/// Chunk size for vector I/O.
pub const CHUNK_SIZE: usize = 512;

/// Maximum codeset name length.
pub const R_CODESET_MAX: c_int = 256;

/// Initial reference read table size.
pub const INITIAL_REFREAD_TABLE_SIZE: c_int = 128;

/// REFSXP type for reference markers.
pub const REFSXP: c_int = 255;

/// Reference index packing macros.
pub const PACK_REF_INDEX_BIT: c_int = 0x40000000;

/// Cache size for lazy-load databases.
pub const NC: usize = 100;

/// Limit for lazy-load file caching.
pub const LEN_LIMIT: usize = 10 * 1048576;

/// Maximum element size for serialization.
pub const MAXELTSIZE: usize = 131072;

// Administrative SXP values used in the serialization protocol.
pub const NILVALUE_SXP: c_int = 254;
pub const GLOBALENV_SXP: c_int = 253;
pub const UNBOUNDVALUE_SXP: c_int = 252;
pub const MISSINGARG_SXP: c_int = 251;
pub const BASENAMESPACE_SXP: c_int = 250;
pub const NAMESPACESXP: c_int = 249;
pub const PACKAGESXP: c_int = 248;
pub const PERSISTSXP: c_int = 247;
pub const EMPTYENV_SXP: c_int = 242;
pub const BASEENV_SXP: c_int = 241;
pub const ALTREP_SXP: c_int = 238;

// Flag packing masks.
pub const IS_OBJECT_BIT_MASK: c_int = 1 << 8;
pub const HAS_ATTR_BIT_MASK: c_int = 1 << 9;
pub const HAS_TAG_BIT_MASK: c_int = 1 << 10;
pub const ENCODE_LEVELS: c_int = 1 << 12;
pub const DECODE_TYPE_MASK: c_int = 0xFF;
pub const GROWABLE_MASK: c_int = 1 << 5;

/// Maximum packed reference index.
pub const MAX_PACKED_INDEX: c_int = c_int::MAX >> 8;

/// Serialization format types.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum R_pstream_format_t {
    R_pstream_any_format = 0,
    R_pstream_ascii_format = 1,
    R_pstream_binary_format = 2,
    R_pstream_xdr_format = 3,
    R_pstream_asciihex_format = 4,
}

/// Opaque pointer for stream data.
pub type R_pstream_data_t = *mut c_void;

/// Structure for the R output persistent stream.
#[repr(C)]
pub struct R_outpstream_st {
    pub data: R_pstream_data_t,
    pub type_: R_pstream_format_t,
    pub version: c_int,
    pub OutChar: Option<unsafe extern "C" fn(*mut R_outpstream_st, c_int)>,
    pub OutBytes: Option<unsafe extern "C" fn(*mut R_outpstream_st, *const c_void, c_int)>,
    pub OutPersistHookFunc: Option<unsafe extern "C" fn(SEXP, SEXP) -> SEXP>,
    pub OutPersistHookData: SEXP,
}

/// Structure for the R input persistent stream.
#[repr(C)]
pub struct R_inpstream_st {
    pub data: R_pstream_data_t,
    pub type_: R_pstream_format_t,
    pub InChar: Option<unsafe extern "C" fn(*mut R_inpstream_st) -> c_int>,
    pub InBytes: Option<unsafe extern "C" fn(*mut R_inpstream_st, *mut c_void, c_int)>,
    pub InPersistHookFunc: Option<unsafe extern "C" fn(SEXP, SEXP) -> SEXP>,
    pub InPersistHookData: SEXP,
    pub native_encoding: [c_char; R_CODESET_MAX as usize],
    pub nat2nat_obj: *mut c_void,
    pub nat2utf8_obj: *mut c_void,
}

pub type R_outpstream_t = *mut R_outpstream_st;
pub type R_inpstream_t = *mut R_inpstream_st;

// ---------------------------------------------------------------------------
// Per-session lazy-load/read state
// ---------------------------------------------------------------------------

pub struct SerializeRuntimeState {
    pub used: usize,
    pub cache_names: [*mut c_char; NC],
    pub cache_ptrs: [*mut c_char; NC],
    pub read_item_depth: c_int,
}

impl Default for SerializeRuntimeState {
    fn default() -> Self {
        Self {
            used: 0,
            cache_names: [ptr::null_mut(); NC],
            cache_ptrs: [ptr::null_mut(); NC],
            read_item_depth: 0,
        }
    }
}

pub fn increment_read_item_depth() {
    with_required_current_instance(|instance| {
        instance.serialize_state.read_item_depth += 1;
    });
}

pub fn decrement_read_item_depth() {
    with_required_current_instance(|instance| {
        if instance.serialize_state.read_item_depth > 0 {
            instance.serialize_state.read_item_depth -= 1;
        }
    });
}

#[cfg(test)]
pub fn read_item_depth_for_test() -> c_int {
    with_required_current_instance(|instance| instance.serialize_state.read_item_depth)
}

// ---------------------------------------------------------------------------
// Internal binary writer/reader (Vec<u8> based)
// ---------------------------------------------------------------------------

/// Internal serializer that writes to a Vec<u8>.
pub struct BinaryWriter {
    pub buf: Vec<u8>,
    pub ascii_body: bool,
    pub xdr_body: bool,
}

impl BinaryWriter {
    pub fn new() -> Self {
        BinaryWriter {
            buf: Vec::new(),
            ascii_body: false,
            xdr_body: false,
        }
    }

    pub fn set_ascii_body(&mut self, ascii_body: bool) {
        self.ascii_body = ascii_body;
    }

    pub fn set_xdr_body(&mut self, xdr_body: bool) {
        self.xdr_body = xdr_body;
    }

    pub fn write_i32(&mut self, val: i32) {
        if self.ascii_body {
            self.buf.extend_from_slice(val.to_string().as_bytes());
            self.buf.push(b'\n');
        } else if self.xdr_body {
            self.buf.extend_from_slice(&val.to_be_bytes());
        } else {
            self.buf.extend_from_slice(&val.to_ne_bytes());
        }
    }

    pub fn write_f64(&mut self, val: f64) {
        if self.ascii_body {
            self.buf.extend_from_slice(format!("{val:?}").as_bytes());
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

/// Internal deserializer that reads from a &[u8].
pub struct BinaryReader<'a> {
    pub data: &'a [u8],
    pub pos: usize,
    pub ascii_body: bool,
    pub xdr_body: bool,
}

impl<'a> BinaryReader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        BinaryReader {
            data,
            pos: 0,
            ascii_body: false,
            xdr_body: false,
        }
    }

    pub fn set_ascii_body(&mut self, ascii_body: bool) {
        self.ascii_body = ascii_body;
    }

    pub fn set_xdr_body(&mut self, xdr_body: bool) {
        self.xdr_body = xdr_body;
    }

    pub fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.pos)
    }

    pub fn read_i32(&mut self) -> Result<i32, String> {
        if self.ascii_body {
            let token = self.read_ascii_token()?;
            return token
                .parse::<i32>()
                .map_err(|_| format!("read error: invalid integer token '{token}'"));
        }
        if self.remaining() < 4 {
            return Err("read error: not enough bytes for i32".into());
        }
        let bytes: [u8; 4] = self.data[self.pos..self.pos + 4]
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
            return token
                .parse::<f64>()
                .map_err(|_| format!("read error: invalid real token '{token}'"));
        }
        if self.remaining() < 8 {
            return Err("read error: not enough bytes for f64".into());
        }
        let bytes: [u8; 8] = self.data[self.pos..self.pos + 8]
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
        if self.remaining() < 1 {
            return Err("read error: not enough bytes for byte".into());
        }
        let b = self.data[self.pos];
        self.pos += 1;
        Ok(b)
    }

    pub fn read_bytes(&mut self, len: usize) -> Result<&'a [u8], String> {
        if self.ascii_body {
            self.skip_ascii_whitespace();
        }
        if self.remaining() < len {
            return Err(format!("read error: not enough bytes for {} bytes", len));
        }
        let slice = &self.data[self.pos..self.pos + len];
        self.pos += len;
        if self.ascii_body && self.remaining() > 0 && self.data[self.pos] == b'\n' {
            self.pos += 1;
        }
        Ok(slice)
    }

    pub fn read_string_bytes(&mut self, len: usize) -> Result<Vec<u8>, String> {
        if !self.ascii_body {
            return self.read_bytes(len).map(|bytes| bytes.to_vec());
        }

        self.skip_ascii_whitespace();
        let mut out = Vec::with_capacity(len);
        while out.len() < len {
            if self.pos >= self.data.len() {
                return Err("read error: unexpected end of ASCII string".to_string());
            }
            let byte = self.data[self.pos];
            self.pos += 1;
            if byte != b'\\' {
                out.push(byte);
                continue;
            }
            if self.pos >= self.data.len() {
                return Err("read error: truncated ASCII string escape".to_string());
            }
            let escaped = self.data[self.pos];
            self.pos += 1;
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
                    while digits < 3
                        && self.pos < self.data.len()
                        && matches!(self.data[self.pos], b'0'..=b'7')
                    {
                        value = value
                            .saturating_mul(8)
                            .saturating_add(self.data[self.pos] - b'0');
                        self.pos += 1;
                        digits += 1;
                    }
                    out.push(value);
                }
                other => out.push(other),
            }
        }
        if self.remaining() > 0 && self.data[self.pos] == b'\n' {
            self.pos += 1;
        }
        Ok(out)
    }

    pub fn read_ascii_token(&mut self) -> Result<String, String> {
        self.skip_ascii_whitespace();
        let start = self.pos;
        while self.pos < self.data.len() && !self.data[self.pos].is_ascii_whitespace() {
            self.pos += 1;
        }
        if start == self.pos {
            return Err("read error: missing ASCII token".to_string());
        }
        std::str::from_utf8(&self.data[start..self.pos])
            .map(|s| s.to_string())
            .map_err(|_| "read error: invalid ASCII token".to_string())
    }

    pub fn skip_ascii_whitespace(&mut self) {
        while self.pos < self.data.len() && self.data[self.pos].is_ascii_whitespace() {
            self.pos += 1;
        }
    }
}

// ---------------------------------------------------------------------------
// Internal hash table for write reference tracking
// ---------------------------------------------------------------------------

/// A simple hash table mapping SEXP pointers to reference indices.
/// Stored as a vector of (pointer, index) pairs with linear probing.
pub struct WriteHashTable {
    pub buckets: Vec<Vec<(usize, i32)>>,
    pub count: i32,
}

impl WriteHashTable {
    pub fn new() -> Self {
        WriteHashTable {
            buckets: vec![Vec::new(); HASHSIZE as usize],
            count: 0,
        }
    }

    pub fn add(&mut self, obj: SEXP) {
        let key = obj as usize;
        let pos = (key >> 2) % (HASHSIZE as usize);
        self.count += 1;
        self.buckets[pos].push((key, self.count));
    }

    pub fn get(&self, item: SEXP) -> i32 {
        let key = item as usize;
        let pos = (key >> 2) % (HASHSIZE as usize);
        for &(k, v) in &self.buckets[pos] {
            if k == key {
                return v;
            }
        }
        0
    }
}

// ---------------------------------------------------------------------------
// Internal read reference table
// ---------------------------------------------------------------------------

/// A growable array for tracking deserialized reference objects.
pub struct ReadRefTable {
    pub entries: Vec<SEXP>,
}

impl ReadRefTable {
    pub fn new() -> Self {
        ReadRefTable {
            entries: Vec::with_capacity(INITIAL_REFREAD_TABLE_SIZE as usize),
        }
    }

    pub fn add(&mut self, value: SEXP) {
        self.entries.push(value);
    }

    pub fn get(&self, index: i32) -> Result<SEXP, String> {
        let i = (index - 1) as usize;
        if i >= self.entries.len() {
            Err("reference index out of range".into())
        } else {
            Ok(self.entries[i])
        }
    }
}

// ---------------------------------------------------------------------------
// Flag packing/unpacking
// ---------------------------------------------------------------------------

pub unsafe fn PackFlags(
    type_: c_int,
    levs: c_int,
    isobj: c_int,
    hasattr: c_int,
    hastag: c_int,
) -> c_int {
    let mut levs = levs;
    match type_ {
        kind if kind == SEXPTYPE::LGLSXP.as_c_int()
            || kind == SEXPTYPE::INTSXP.as_c_int()
            || kind == SEXPTYPE::REALSXP.as_c_int()
            || kind == SEXPTYPE::CPLXSXP.as_c_int()
            || kind == SEXPTYPE::STRSXP.as_c_int()
            || kind == SEXPTYPE::VECSXP.as_c_int()
            || kind == SEXPTYPE::EXPRSXP.as_c_int()
            || kind == SEXPTYPE::RAWSXP.as_c_int() =>
        {
            levs &= !GROWABLE_MASK;
        }
        _ => {}
    }
    let mut val = type_ | (levs << 12);
    if isobj != 0 {
        val |= IS_OBJECT_BIT_MASK;
    }
    if hasattr != 0 {
        val |= HAS_ATTR_BIT_MASK;
    }
    if hastag != 0 {
        val |= HAS_TAG_BIT_MASK;
    }
    val
}

pub unsafe fn UnpackFlags(
    flags: c_int,
    ptype: *mut c_int,
    plevs: *mut c_int,
    pisobj: *mut c_int,
    phasattr: *mut c_int,
    phastag: *mut c_int,
) {
    unsafe {
        if !ptype.is_null() {
            *ptype = flags & DECODE_TYPE_MASK;
        }
        if !plevs.is_null() {
            *plevs = flags >> 12;
        }
        if !pisobj.is_null() {
            *pisobj = if flags & IS_OBJECT_BIT_MASK != 0 {
                1
            } else {
                0
            };
        }
        if !phasattr.is_null() {
            *phasattr = if flags & HAS_ATTR_BIT_MASK != 0 { 1 } else { 0 };
        }
        if !phastag.is_null() {
            *phastag = if flags & HAS_TAG_BIT_MASK != 0 { 1 } else { 0 };
        }
    }
}

// ---------------------------------------------------------------------------
// Reference index packing/unpacking
// ---------------------------------------------------------------------------

pub unsafe fn OutRefIndex(writer: &mut BinaryWriter, i: c_int) {
    if i > MAX_PACKED_INDEX {
        writer.write_i32(REFSXP);
        writer.write_i32(i);
    } else {
        writer.write_i32((i << 8) | REFSXP);
    }
}

pub fn InRefIndex(flags: c_int, reader: &mut BinaryReader) -> Result<c_int, String> {
    let i = flags >> 8;
    if i == 0 { reader.read_i32() } else { Ok(i) }
}

// ---------------------------------------------------------------------------
// Version decoding
// ---------------------------------------------------------------------------

pub unsafe fn DecodeVersion(packed: c_int, v: *mut c_int, p: *mut c_int, s: *mut c_int) {
    unsafe {
        if !v.is_null() {
            *v = packed / 65536;
        }
        let mut rem = packed % 65536;
        if !p.is_null() {
            *p = rem / 256;
        }
        rem %= 256;
        if !s.is_null() {
            *s = rem;
        }
    }
}

// ---------------------------------------------------------------------------
// SaveSpecialHook -- detect special singleton values
// ---------------------------------------------------------------------------

pub unsafe fn SaveSpecialHook(item: SEXP) -> c_int {
    unsafe {
        if item.is_null() {
            return NILVALUE_SXP;
        }
        if TYPEOF(item) == SEXPTYPE::NILSXP {
            return NILVALUE_SXP;
        }
        if item == R_GlobalEnv() {
            return GLOBALENV_SXP;
        }
        if item == R_UnboundValue() {
            return UNBOUNDVALUE_SXP;
        }
        if item == R_MissingArg() {
            return MISSINGARG_SXP;
        }
        if item == R_EmptyEnv() {
            return EMPTYENV_SXP;
        }
        if item == R_BaseEnv() {
            return BASEENV_SXP;
        }
        0
    }
}

// ---------------------------------------------------------------------------
// Internal WriteItem (recursive, writes to BinaryWriter)
// ---------------------------------------------------------------------------

pub unsafe fn WriteItemInternal(
    s: SEXP,
    ref_table: &mut WriteHashTable,
    writer: &mut BinaryWriter,
) {
    unsafe {
        // Check for special singletons
        let special = SaveSpecialHook(s);
        if special != 0 {
            writer.write_i32(special);
            return;
        }

        // Check for already-seen reference (symbols, environments, etc.)
        let ref_idx = ref_table.get(s);
        if ref_idx != 0 {
            OutRefIndex(writer, ref_idx);
            return;
        }

        let stype = TYPEOF(s);

        // Handle SYMSXP
        if stype == SEXPTYPE::SYMSXP {
            ref_table.add(s);
            writer.write_i32(SEXPTYPE::SYMSXP.as_c_int());
            let pname = PRINTNAME(s);
            WriteItemInternal(pname, ref_table, writer);
            return;
        }

        // Handle LISTSXP
        if stype == SEXPTYPE::LISTSXP {
            let hastag = if TAG(s).is_null() { 0 } else { 1 };
            let hasattr = if ATTRIB(s).is_null() { 0 } else { 1 };
            let flags = PackFlags(stype, LEVELS(s), OBJECT(s), hasattr, hastag);
            writer.write_i32(flags);
            if hasattr != 0 {
                WriteItemInternal(ATTRIB(s), ref_table, writer);
            }
            if hastag != 0 {
                WriteItemInternal(TAG(s), ref_table, writer);
            }
            WriteItemInternal(CAR(s), ref_table, writer);
            WriteItemInternal(CDR(s), ref_table, writer);
            return;
        }

        // Handle LANGSXP
        if stype == SEXPTYPE::LANGSXP {
            let hastag = if TAG(s).is_null() { 0 } else { 1 };
            let hasattr = if ATTRIB(s).is_null() { 0 } else { 1 };
            let flags = PackFlags(stype, LEVELS(s), OBJECT(s), hasattr, hastag);
            writer.write_i32(flags);
            if hasattr != 0 {
                WriteItemInternal(ATTRIB(s), ref_table, writer);
            }
            if hastag != 0 {
                WriteItemInternal(TAG(s), ref_table, writer);
            }
            WriteItemInternal(CAR(s), ref_table, writer);
            WriteItemInternal(CDR(s), ref_table, writer);
            return;
        }

        // Handle CLOSXP
        if stype == SEXPTYPE::CLOSXP {
            let hasattr = if ATTRIB(s).is_null() { 0 } else { 1 };
            let flags = PackFlags(stype, LEVELS(s), OBJECT(s), hasattr, 1); // hastag=1 for closures
            writer.write_i32(flags);
            if hasattr != 0 {
                WriteItemInternal(ATTRIB(s), ref_table, writer);
            }
            WriteItemInternal(CLOENV(s), ref_table, writer);
            WriteItemInternal(FORMALS(s), ref_table, writer);
            WriteItemInternal(BODY(s), ref_table, writer);
            return;
        }

        // Handle CHARSXP
        if stype == SEXPTYPE::CHARSXP {
            let levs = LEVELS(s);
            let flags = PackFlags(stype, levs, 0, 0, 0);
            writer.write_i32(flags);
            let len = if s == R_NaString() { -1 } else { LENGTH(s) };
            writer.write_i32(len);
            if len > 0 {
                let char_data = CHAR(s);
                writer.write_string_bytes(char_data, len);
            }
            return;
        }

        // For atomic/vector types, compute flags with attributes
        let hasattr = if ATTRIB(s).is_null() { 0 } else { 1 };
        let flags = PackFlags(stype, LEVELS(s), OBJECT(s), hasattr, 0);

        if stype == SEXPTYPE::LGLSXP || stype == SEXPTYPE::INTSXP {
            writer.write_i32(flags);
            let len = XLENGTH(s);
            writer.write_i32(len as c_int);
            let int_data = INTEGER(s);
            for i in 0..len as isize {
                writer.write_i32(*int_data.offset(i));
            }
        } else if stype == SEXPTYPE::REALSXP {
            writer.write_i32(flags);
            let len = XLENGTH(s);
            writer.write_i32(len as c_int);
            let real_data = REAL(s);
            for i in 0..len as isize {
                writer.write_f64(*real_data.offset(i));
            }
        } else if stype == SEXPTYPE::CPLXSXP {
            writer.write_i32(flags);
            let len = XLENGTH(s);
            writer.write_i32(len as c_int);
            let cpx_data = COMPLEX(s);
            for i in 0..len as isize {
                let c = *cpx_data.offset(i);
                writer.write_f64(c.r);
                writer.write_f64(c.i);
            }
        } else if stype == SEXPTYPE::STRSXP {
            writer.write_i32(flags);
            let len = XLENGTH(s);
            writer.write_i32(len as c_int);
            for i in 0..len {
                let elt = STRING_ELT(s, i as R_xlen_t);
                WriteItemInternal(elt, ref_table, writer);
            }
        } else if stype == SEXPTYPE::RAWSXP {
            writer.write_i32(flags);
            let len = XLENGTH(s);
            writer.write_i32(len as c_int);
            let raw_data = RAW(s);
            for i in 0..len as isize {
                writer.write_byte(*raw_data.offset(i));
            }
        } else if stype == SEXPTYPE::VECSXP || stype == SEXPTYPE::EXPRSXP {
            writer.write_i32(flags);
            let len = XLENGTH(s);
            writer.write_i32(len as c_int);
            for i in 0..len {
                let elt = VECTOR_ELT(s, i as R_xlen_t);
                WriteItemInternal(elt, ref_table, writer);
            }
        } else {
            // Unknown type: write flags + nothing (best effort)
            writer.write_i32(flags);
        }

        // Write attributes at the end for non-CHARSXP types
        if hasattr != 0 {
            WriteItemInternal(ATTRIB(s), ref_table, writer);
        }
    }
}

// ---------------------------------------------------------------------------
// Internal ReadItem (recursive, reads from BinaryReader)
// ---------------------------------------------------------------------------

pub unsafe fn ReadItemInternal(
    reader: &mut BinaryReader,
    ref_table: &mut ReadRefTable,
) -> Result<SEXP, String> {
    unsafe {
        let flags = reader.read_i32()?;
        let mut stype: c_int = 0;
        let mut levs: c_int = 0;
        let mut isobj: c_int = 0;
        let mut hasattr: c_int = 0;
        let mut hastag: c_int = 0;
        UnpackFlags(
            flags,
            &mut stype,
            &mut levs,
            &mut isobj,
            &mut hasattr,
            &mut hastag,
        );

        if stype == NILVALUE_SXP {
            Ok(R_NilValue())
        } else if stype == GLOBALENV_SXP {
            Ok(R_GlobalEnv())
        } else if stype == UNBOUNDVALUE_SXP {
            Ok(R_UnboundValue())
        } else if stype == MISSINGARG_SXP {
            Ok(R_MissingArg())
        } else if stype == EMPTYENV_SXP {
            Ok(R_EmptyEnv())
        } else if stype == BASEENV_SXP {
            Ok(R_BaseEnv())
        } else if stype == REFSXP {
            let idx = InRefIndex(flags, reader)?;
            ref_table.get(idx)
        } else if stype == SEXPTYPE::SYMSXP {
            let pname = ReadItemInternal(reader, ref_table)?;
            let sym = Rf_install(CHAR(pname));
            ref_table.add(sym);
            Ok(sym)
        } else if stype == SEXPTYPE::LISTSXP || stype == SEXPTYPE::LANGSXP {
            let s = allocSExp(SEXPTYPE(stype));
            let _s_guard = protect(s);
            if hasattr != 0 {
                let attr = ReadItemInternal(reader, ref_table)?;
                SET_ATTRIB(s, attr);
            }
            if hastag != 0 {
                let tag = ReadItemInternal(reader, ref_table)?;
                SETTAG(s, tag);
            }
            let car = ReadItemInternal(reader, ref_table)?;
            SETCAR(s, car);
            let cdr = ReadItemInternal(reader, ref_table)?;
            SETCDR(s, cdr);
            SETLEVELS(s, levs);
            if isobj != 0 {
                SET_OBJECT(s, 1);
            }
            Ok(s)
        } else if stype == SEXPTYPE::CLOSXP {
            let s = allocSExp(SEXPTYPE::CLOSXP);
            let _s_guard = protect(s);
            if hasattr != 0 {
                let attr = ReadItemInternal(reader, ref_table)?;
                SET_ATTRIB(s, attr);
            }
            let cloenv = ReadItemInternal(reader, ref_table)?;
            SET_CLOENV(s, cloenv);
            let formals = ReadItemInternal(reader, ref_table)?;
            SET_FORMALS(s, formals);
            let body = ReadItemInternal(reader, ref_table)?;
            SET_BODY(s, body);
            SETLEVELS(s, levs);
            if isobj != 0 {
                SET_OBJECT(s, 1);
            }
            Ok(s)
        } else if stype == SEXPTYPE::CHARSXP {
            let len = reader.read_i32()?;
            if len < 0 {
                Ok(R_NaString())
            } else if len == 0 {
                let s = Rf_mkCharLen(b"\0" as *const u8 as *const c_char, 0);
                Ok(s)
            } else {
                let bytes = reader.read_string_bytes(len as usize)?;
                let s = Rf_mkCharLen(bytes.as_ptr() as *const c_char, len);
                Ok(s)
            }
        } else if stype == SEXPTYPE::LGLSXP || stype == SEXPTYPE::INTSXP {
            let len = reader.read_i32()?;
            let s = Rf_allocVector3(stype, len as R_xlen_t);
            let _s_guard = protect(s);
            let int_data = INTEGER(s);
            for i in 0..len as isize {
                *int_data.offset(i) = reader.read_i32()?;
            }
            SETLEVELS(s, levs);
            if isobj != 0 {
                SET_OBJECT(s, 1);
            }
            if hasattr != 0 {
                let attr = ReadItemInternal(reader, ref_table)?;
                SET_ATTRIB(s, attr);
            }
            Ok(s)
        } else if stype == SEXPTYPE::REALSXP {
            let len = reader.read_i32()?;
            let s = Rf_allocVector3(stype, len as R_xlen_t);
            let _s_guard = protect(s);
            let real_data = REAL(s);
            for i in 0..len as isize {
                *real_data.offset(i) = reader.read_f64()?;
            }
            SETLEVELS(s, levs);
            if isobj != 0 {
                SET_OBJECT(s, 1);
            }
            if hasattr != 0 {
                let attr = ReadItemInternal(reader, ref_table)?;
                SET_ATTRIB(s, attr);
            }
            Ok(s)
        } else if stype == SEXPTYPE::CPLXSXP {
            let len = reader.read_i32()?;
            let s = Rf_allocVector3(stype, len as R_xlen_t);
            let _s_guard = protect(s);
            let cpx_data = COMPLEX(s);
            for i in 0..len as isize {
                let r = reader.read_f64()?;
                let im = reader.read_f64()?;
                *cpx_data.offset(i) = Rcomplex { r, i: im };
            }
            SETLEVELS(s, levs);
            if isobj != 0 {
                SET_OBJECT(s, 1);
            }
            if hasattr != 0 {
                let attr = ReadItemInternal(reader, ref_table)?;
                SET_ATTRIB(s, attr);
            }
            Ok(s)
        } else if stype == SEXPTYPE::STRSXP {
            let len = reader.read_i32()?;
            let s = Rf_allocVector3(SEXPTYPE::STRSXP, len as R_xlen_t);
            let _s_guard = protect(s);
            for i in 0..len {
                let elt = ReadItemInternal(reader, ref_table)?;
                SET_STRING_ELT(s, i as R_xlen_t, elt);
            }
            SETLEVELS(s, levs);
            if isobj != 0 {
                SET_OBJECT(s, 1);
            }
            if hasattr != 0 {
                let attr = ReadItemInternal(reader, ref_table)?;
                SET_ATTRIB(s, attr);
            }
            Ok(s)
        } else if stype == SEXPTYPE::RAWSXP {
            let len = reader.read_i32()?;
            let s = Rf_allocVector3(SEXPTYPE::RAWSXP, len as R_xlen_t);
            let _s_guard = protect(s);
            let raw_data = RAW(s);
            for i in 0..len as isize {
                *raw_data.offset(i) = reader.read_byte()?;
            }
            SETLEVELS(s, levs);
            if isobj != 0 {
                SET_OBJECT(s, 1);
            }
            if hasattr != 0 {
                let attr = ReadItemInternal(reader, ref_table)?;
                SET_ATTRIB(s, attr);
            }
            Ok(s)
        } else if stype == SEXPTYPE::VECSXP || stype == SEXPTYPE::EXPRSXP {
            let len = reader.read_i32()?;
            let s = Rf_allocVector3(stype, len as R_xlen_t);
            let _s_guard = protect(s);
            for i in 0..len {
                let elt = ReadItemInternal(reader, ref_table)?;
                SET_VECTOR_ELT(s, i as R_xlen_t, elt);
            }
            SETLEVELS(s, levs);
            if isobj != 0 {
                SET_OBJECT(s, 1);
            }
            if hasattr != 0 {
                let attr = ReadItemInternal(reader, ref_table)?;
                SET_ATTRIB(s, attr);
            }
            Ok(s)
        } else {
            Err(format!("ReadItem: unknown type {}", stype))
        }
    }
}

// ---------------------------------------------------------------------------
// defaultSerializeVersion
// ---------------------------------------------------------------------------

pub unsafe fn defaultSerializeVersion() -> c_int {
    R_DEFAULT_SERIALIZE_VERSION
}
