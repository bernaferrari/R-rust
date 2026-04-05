#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Port of R's src/main/serialize.c -- R object serialization/unserialization.
//!
//! Implements a binary serialization format supporting basic atomic types
//! (NILSXP, LGLSXP, INTSXP, REALSXP, CPLXSXP, STRSXP, RAWSXP),
//! generic lists (VECSXP), dotted pairs (LISTSXP), symbols (SYMSXP),
//! closures (CLOSXP), and attributes.
//!
//! The format uses native-endian encoding (no XDR) for simplicity, but
//! follows R's serialization protocol structure: format header, version
//! info, then recursive WriteItem/ReadItem.

use std::os::raw::{c_char, c_double, c_int, c_void};
use std::ptr;
use std::slice;

use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::ffi::{R_size_t, R_xlen_t, Rboolean, Rbyte, Rcomplex, SEXP, SEXPTYPE};
use crate::sexp::globals::R_NilValue;
use crate::sexp::memory_ext::allocSExp;
use crate::sexp::protect::*;
use crate::sexp::symbol::Rf_install;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Initial hash table size for write reference tracking.
const HASHSIZE: c_int = 1009;

/// Default serialize version (2 or 3).
const R_DEFAULT_SERIALIZE_VERSION: c_int = 3;

/// R's current version (placeholder).
const R_VERSION: c_int = 4;

/// R version (4, 5, 0) packed as integer.
const R_VERSION_450: c_int = (4 << 16) | (5 << 8) | 0;

/// R version (2, 3, 0) packed as integer.
const R_VERSION_230: c_int = (2 << 16) | (3 << 8) | 0;

/// R version (3, 5, 0) packed as integer.
const R_VERSION_350: c_int = (3 << 16) | (5 << 8) | 0;

/// Chunk size for vector I/O.
const CHUNK_SIZE: usize = 512;

/// Maximum codeset name length.
const R_CODESET_MAX: c_int = 256;

/// Initial reference read table size.
const INITIAL_REFREAD_TABLE_SIZE: c_int = 128;

/// REFSXP type for reference markers.
const REFSXP: c_int = 255;

/// Reference index packing macros.
const PACK_REF_INDEX_BIT: c_int = 0x40000000;

/// Cache size for lazy-load databases.
const NC: usize = 100;

/// Limit for lazy-load file caching.
const LEN_LIMIT: usize = 10 * 1048576;

/// Maximum element size for serialization.
const MAXELTSIZE: usize = 131072;

// Administrative SXP values used in the serialization protocol.
const NILVALUE_SXP: c_int = 254;
const GLOBALENV_SXP: c_int = 253;
const UNBOUNDVALUE_SXP: c_int = 252;
const MISSINGARG_SXP: c_int = 251;
const BASENAMESPACE_SXP: c_int = 250;
const NAMESPACESXP: c_int = 249;
const PACKAGESXP: c_int = 248;
const PERSISTSXP: c_int = 247;
const EMPTYENV_SXP: c_int = 242;
const BASEENV_SXP: c_int = 241;
const ALTREP_SXP: c_int = 238;

// Flag packing masks.
const IS_OBJECT_BIT_MASK: c_int = 1 << 8;
const HAS_ATTR_BIT_MASK: c_int = 1 << 9;
const HAS_TAG_BIT_MASK: c_int = 1 << 10;
const ENCODE_LEVELS: c_int = 1 << 12;
const DECODE_TYPE_MASK: c_int = 0xFF;

/// Maximum packed reference index.
const MAX_PACKED_INDEX: c_int = c_int::MAX >> 8;

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
// Global state for lazy-load database cache
// ---------------------------------------------------------------------------

static mut USED: usize = 0;

static mut CACHE_NAMES: [*mut c_char; NC] = [ptr::null_mut(); NC];

static mut CACHE_PTRS: [*mut c_char; NC] = [ptr::null_mut(); NC];

/// Global tracking for read recursion depth.
static mut R_ReadItemDepth: c_int = 0;

// ---------------------------------------------------------------------------
// Internal binary writer/reader (Vec<u8> based)
// ---------------------------------------------------------------------------

/// Internal serializer that writes to a Vec<u8>.
struct BinaryWriter {
    buf: Vec<u8>,
}

impl BinaryWriter {
    fn new() -> Self {
        BinaryWriter { buf: Vec::new() }
    }

    fn write_i32(&mut self, val: i32) {
        self.buf.extend_from_slice(&val.to_ne_bytes());
    }

    fn write_f64(&mut self, val: f64) {
        self.buf.extend_from_slice(&val.to_ne_bytes());
    }

    fn write_byte(&mut self, val: u8) {
        self.buf.push(val);
    }

    fn write_bytes(&mut self, data: &[u8]) {
        self.buf.extend_from_slice(data);
    }

    fn write_string_bytes(&mut self, s: *const c_char, len: i32) {
        if len > 0 && !s.is_null() {
            // SAFETY: Caller ensures s points to at least `len` valid bytes.
            let slice = unsafe { slice::from_raw_parts(s as *const u8, len as usize) };
            self.buf.extend_from_slice(slice);
        }
    }

    fn into_vec(self) -> Vec<u8> {
        self.buf
    }
}

/// Internal deserializer that reads from a &[u8].
struct BinaryReader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> BinaryReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        BinaryReader { data, pos: 0 }
    }

    fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.pos)
    }

    fn read_i32(&mut self) -> Result<i32, String> {
        if self.remaining() < 4 {
            return Err("read error: not enough bytes for i32".into());
        }
        let bytes: [u8; 4] = self.data[self.pos..self.pos + 4].try_into().unwrap();
        self.pos += 4;
        Ok(i32::from_ne_bytes(bytes))
    }

    fn read_f64(&mut self) -> Result<f64, String> {
        if self.remaining() < 8 {
            return Err("read error: not enough bytes for f64".into());
        }
        let bytes: [u8; 8] = self.data[self.pos..self.pos + 8].try_into().unwrap();
        self.pos += 8;
        Ok(f64::from_ne_bytes(bytes))
    }

    fn read_byte(&mut self) -> Result<u8, String> {
        if self.remaining() < 1 {
            return Err("read error: not enough bytes for byte".into());
        }
        let b = self.data[self.pos];
        self.pos += 1;
        Ok(b)
    }

    fn read_bytes(&mut self, len: usize) -> Result<&'a [u8], String> {
        if self.remaining() < len {
            return Err(format!("read error: not enough bytes for {} bytes", len));
        }
        let slice = &self.data[self.pos..self.pos + len];
        self.pos += len;
        Ok(slice)
    }
}

// ---------------------------------------------------------------------------
// Internal hash table for write reference tracking
// ---------------------------------------------------------------------------

/// A simple hash table mapping SEXP pointers to reference indices.
/// Stored as a vector of (pointer, index) pairs with linear probing.
struct WriteHashTable {
    buckets: Vec<Vec<(usize, i32)>>,
    count: i32,
}

impl WriteHashTable {
    fn new() -> Self {
        WriteHashTable {
            buckets: vec![Vec::new(); HASHSIZE as usize],
            count: 0,
        }
    }

    fn add(&mut self, obj: SEXP) {
        let key = obj as usize;
        let pos = (key >> 2) % (HASHSIZE as usize);
        self.count += 1;
        self.buckets[pos].push((key, self.count));
    }

    fn get(&self, item: SEXP) -> i32 {
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
struct ReadRefTable {
    entries: Vec<SEXP>,
}

impl ReadRefTable {
    fn new() -> Self {
        ReadRefTable {
            entries: Vec::with_capacity(INITIAL_REFREAD_TABLE_SIZE as usize),
        }
    }

    fn add(&mut self, value: SEXP) {
        self.entries.push(value);
    }

    fn get(&self, index: i32) -> Result<SEXP, String> {
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

unsafe fn PackFlags(
    type_: c_int,
    levs: c_int,
    isobj: c_int,
    hasattr: c_int,
    hastag: c_int,
) -> c_int {
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

unsafe fn UnpackFlags(
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

unsafe fn OutRefIndex(writer: &mut BinaryWriter, i: c_int) {
    if i > MAX_PACKED_INDEX {
        writer.write_i32(REFSXP);
        writer.write_i32(i);
    } else {
        writer.write_i32((i << 8) | REFSXP);
    }
}

fn InRefIndex(flags: c_int, reader: &mut BinaryReader) -> Result<c_int, String> {
    let i = flags >> 8;
    if i == 0 { reader.read_i32() } else { Ok(i) }
}

// ---------------------------------------------------------------------------
// Version decoding
// ---------------------------------------------------------------------------

unsafe fn DecodeVersion(packed: c_int, v: *mut c_int, p: *mut c_int, s: *mut c_int) {
    unsafe {
        if !v.is_null() {
            *v = packed / 65536;
        }
        let mut rem = packed % 65536;
        if !p.is_null() {
            *p = rem / 256;
        }
        rem = rem % 256;
        if !s.is_null() {
            *s = rem;
        }
    }
}

// ---------------------------------------------------------------------------
// SaveSpecialHook -- detect special singleton values
// ---------------------------------------------------------------------------

unsafe fn SaveSpecialHook(item: SEXP) -> c_int {
    unsafe {
        if item.is_null() {
            return NILVALUE_SXP;
        }
        if TYPEOF(item) == SEXPTYPE::NILSXP.0 {
            return NILVALUE_SXP;
        }
        0
    }
}

// ---------------------------------------------------------------------------
// Internal WriteItem (recursive, writes to BinaryWriter)
// ---------------------------------------------------------------------------

unsafe fn WriteItemInternal(s: SEXP, ref_table: &mut WriteHashTable, writer: &mut BinaryWriter) {
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
        if stype == SEXPTYPE::SYMSXP.0 {
            ref_table.add(s);
            writer.write_i32(SEXPTYPE::SYMSXP.0);
            let pname = PRINTNAME(s);
            WriteItemInternal(pname, ref_table, writer);
            return;
        }

        // Handle LISTSXP
        if stype == SEXPTYPE::LISTSXP.0 {
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
        if stype == SEXPTYPE::LANGSXP.0 {
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
        if stype == SEXPTYPE::CLOSXP.0 {
            let hasattr = if ATTRIB(s).is_null() { 0 } else { 1 };
            let flags = PackFlags(stype, LEVELS(s), OBJECT(s), hasattr, 1); // hastag=1 for closures
            writer.write_i32(flags);
            if hasattr != 0 {
                WriteItemInternal(ATTRIB(s), ref_table, writer);
            }
            // Write CLOENV, FORMALS, BODY
            WriteItemInternal(CDR(s), ref_table, writer); // CLOENV stored in CDR for our simplified model
            WriteItemInternal(TAG(s), ref_table, writer); // FORMALS stored in TAG
            WriteItemInternal(CAR(s), ref_table, writer); // BODY stored in CAR
            return;
        }

        // Handle CHARSXP
        if stype == SEXPTYPE::CHARSXP.0 {
            let levs = LEVELS(s);
            let flags = PackFlags(stype, levs, 0, 0, 0);
            writer.write_i32(flags);
            let len = LENGTH(s);
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

        if stype == SEXPTYPE::LGLSXP.0 || stype == SEXPTYPE::INTSXP.0 {
            writer.write_i32(flags);
            let len = XLENGTH(s);
            writer.write_i32(len as c_int);
            let int_data = INTEGER(s);
            for i in 0..len as isize {
                writer.write_i32(*int_data.offset(i));
            }
        } else if stype == SEXPTYPE::REALSXP.0 {
            writer.write_i32(flags);
            let len = XLENGTH(s);
            writer.write_i32(len as c_int);
            let real_data = REAL(s);
            for i in 0..len as isize {
                writer.write_f64(*real_data.offset(i));
            }
        } else if stype == SEXPTYPE::CPLXSXP.0 {
            writer.write_i32(flags);
            let len = XLENGTH(s);
            writer.write_i32(len as c_int);
            let cpx_data = COMPLEX(s);
            for i in 0..len as isize {
                let c = *cpx_data.offset(i);
                writer.write_f64(c.r);
                writer.write_f64(c.i);
            }
        } else if stype == SEXPTYPE::STRSXP.0 {
            writer.write_i32(flags);
            let len = XLENGTH(s);
            writer.write_i32(len as c_int);
            for i in 0..len {
                let elt = STRING_ELT(s, i as R_xlen_t);
                WriteItemInternal(elt, ref_table, writer);
            }
        } else if stype == SEXPTYPE::RAWSXP.0 {
            writer.write_i32(flags);
            let len = XLENGTH(s);
            writer.write_i32(len as c_int);
            let raw_data = RAW(s);
            for i in 0..len as isize {
                writer.write_byte(*raw_data.offset(i));
            }
        } else if stype == SEXPTYPE::VECSXP.0 || stype == SEXPTYPE::EXPRSXP.0 {
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

unsafe fn ReadItemInternal(
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
        } else if stype == REFSXP {
            let idx = InRefIndex(flags, reader)?;
            ref_table.get(idx)
        } else if stype == SEXPTYPE::SYMSXP.0 {
            let pname = ReadItemInternal(reader, ref_table)?;
            let sym = Rf_install(CHAR(pname));
            ref_table.add(sym);
            Ok(sym)
        } else if stype == SEXPTYPE::LISTSXP.0 || stype == SEXPTYPE::LANGSXP.0 {
            let s = allocSExp(SEXPTYPE(stype));
            Rf_protect(s);
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
            Rf_unprotect(1);
            Ok(s)
        } else if stype == SEXPTYPE::CLOSXP.0 {
            let s = allocSExp(SEXPTYPE::CLOSXP);
            Rf_protect(s);
            if hasattr != 0 {
                let attr = ReadItemInternal(reader, ref_table)?;
                SET_ATTRIB(s, attr);
            }
            // Read CLOENV, FORMALS, BODY
            let cloenv = ReadItemInternal(reader, ref_table)?;
            SETCDR(s, cloenv); // CLOENV in CDR
            let formals = ReadItemInternal(reader, ref_table)?;
            SETTAG(s, formals); // FORMALS in TAG
            let body = ReadItemInternal(reader, ref_table)?;
            SETCAR(s, body); // BODY in CAR
            SETLEVELS(s, levs);
            if isobj != 0 {
                SET_OBJECT(s, 1);
            }
            Rf_unprotect(1);
            Ok(s)
        } else if stype == SEXPTYPE::CHARSXP.0 {
            let len = reader.read_i32()?;
            if len < 0 {
                // NA string - create an empty CHARSXP
                let s = Rf_mkCharLen(b"\0" as *const u8 as *const c_char, 0);
                Ok(s)
            } else if len == 0 {
                let s = Rf_mkCharLen(b"\0" as *const u8 as *const c_char, 0);
                Ok(s)
            } else {
                let bytes = reader.read_bytes(len as usize)?;
                let s = Rf_mkCharLen(bytes.as_ptr() as *const c_char, len);
                Ok(s)
            }
        } else if stype == SEXPTYPE::LGLSXP.0 || stype == SEXPTYPE::INTSXP.0 {
            let len = reader.read_i32()?;
            let s = Rf_allocVector3(stype, len as R_xlen_t);
            Rf_protect(s);
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
            Rf_unprotect(1);
            Ok(s)
        } else if stype == SEXPTYPE::REALSXP.0 {
            let len = reader.read_i32()?;
            let s = Rf_allocVector3(stype, len as R_xlen_t);
            Rf_protect(s);
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
            Rf_unprotect(1);
            Ok(s)
        } else if stype == SEXPTYPE::CPLXSXP.0 {
            let len = reader.read_i32()?;
            let s = Rf_allocVector3(stype, len as R_xlen_t);
            Rf_protect(s);
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
            Rf_unprotect(1);
            Ok(s)
        } else if stype == SEXPTYPE::STRSXP.0 {
            let len = reader.read_i32()?;
            let s = Rf_allocVector3(SEXPTYPE::STRSXP.0, len as R_xlen_t);
            Rf_protect(s);
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
            Rf_unprotect(1);
            Ok(s)
        } else if stype == SEXPTYPE::RAWSXP.0 {
            let len = reader.read_i32()?;
            let s = Rf_allocVector3(SEXPTYPE::RAWSXP.0, len as R_xlen_t);
            Rf_protect(s);
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
            Rf_unprotect(1);
            Ok(s)
        } else if stype == SEXPTYPE::VECSXP.0 || stype == SEXPTYPE::EXPRSXP.0 {
            let len = reader.read_i32()?;
            let s = Rf_allocVector3(stype, len as R_xlen_t);
            Rf_protect(s);
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
            Rf_unprotect(1);
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

// ---------------------------------------------------------------------------
// R_Serialize -- serialize an R object to a stream (C API)
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_Serialize(s: SEXP, stream: R_outpstream_t) {
    unsafe {
        if stream.is_null() {
            return;
        }
        let out_ref = &mut *stream;
        // Use OutFormat
        match out_ref.type_ {
            R_pstream_format_t::R_pstream_binary_format => {
                if let Some(out_bytes) = out_ref.OutBytes {
                    out_bytes(stream, b"B\n".as_ptr() as *const c_void, 2);
                }
            }
            R_pstream_format_t::R_pstream_xdr_format => {
                if let Some(out_bytes) = out_ref.OutBytes {
                    out_bytes(stream, b"X\n".as_ptr() as *const c_void, 2);
                }
            }
            R_pstream_format_t::R_pstream_ascii_format
            | R_pstream_format_t::R_pstream_asciihex_format => {
                if let Some(out_bytes) = out_ref.OutBytes {
                    out_bytes(stream, b"A\n".as_ptr() as *const c_void, 2);
                }
            }
            _ => {}
        }

        // Write version info (version 3)
        let version = out_ref.version;
        let write_i32_to_stream = |val: i32, stream: R_outpstream_t| {
            let bytes = val.to_ne_bytes();
            if let Some(out_bytes) = (*stream).OutBytes {
                out_bytes(stream, bytes.as_ptr() as *const c_void, 4);
            }
        };

        write_i32_to_stream(3, stream); // version
        write_i32_to_stream(R_VERSION, stream); // writer version
        write_i32_to_stream(R_VERSION_350, stream); // min reader version

        // Write native encoding (empty string for our purposes)
        write_i32_to_stream(0, stream); // encoding name length = 0

        // Write the object using a temporary buffer, then stream it out
        let mut writer = BinaryWriter::new();
        let mut ref_table = WriteHashTable::new();
        WriteItemInternal(s, &mut ref_table, &mut writer);

        // Stream out the serialized bytes
        let data = writer.into_vec();
        if !data.is_empty() {
            if let Some(out_bytes) = (*stream).OutBytes {
                // Write in chunks to avoid c_int overflow
                let mut offset = 0usize;
                while offset < data.len() {
                    let chunk_len = std::cmp::min(CHUNK_SIZE, data.len() - offset);
                    let chunk_len_int = chunk_len as c_int;
                    if chunk_len_int < 0 {
                        break;
                    }
                    out_bytes(
                        stream,
                        data[offset..].as_ptr() as *const c_void,
                        chunk_len_int,
                    );
                    offset += chunk_len;
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// R_Unserialize -- unserialize an R object from a stream (C API)
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_Unserialize(stream: R_inpstream_t) -> SEXP {
    unsafe {
        if stream.is_null() {
            return R_NilValue();
        }

        // Read format header
        // For the C stream API, we would need InChar/InBytes to build a buffer.
        // This requires the stream to have InBytes set. We read all data into a
        // Vec<u8> and then use our internal reader.
        // Since we don't know the total length from a stream, we return nil
        // and rely on the R_serialize/R_unserialize memory-based paths instead.
        R_NilValue()
    }
}

// ---------------------------------------------------------------------------
// R_SerializeInfo
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_SerializeInfo(stream: R_inpstream_t) -> SEXP {
    unsafe { R_NilValue() }
}

// ---------------------------------------------------------------------------
// R_ReadItem / R_WriteItem (C stream API)
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_ReadItem(stream: R_inpstream_t) -> SEXP {
    unsafe { R_NilValue() }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_WriteItem(s: SEXP, stream: R_outpstream_t) {
    unsafe {
        if stream.is_null() {
            return;
        }
        let mut writer = BinaryWriter::new();
        let mut ref_table = WriteHashTable::new();
        WriteItemInternal(s, &mut ref_table, &mut writer);
        let data = writer.into_vec();
        if !data.is_empty() {
            if let Some(out_bytes) = (*stream).OutBytes {
                let mut offset = 0usize;
                while offset < data.len() {
                    let chunk_len = std::cmp::min(CHUNK_SIZE, data.len() - offset);
                    let chunk_len_int = chunk_len as c_int;
                    if chunk_len_int < 0 {
                        break;
                    }
                    out_bytes(
                        stream,
                        data[offset..].as_ptr() as *const c_void,
                        chunk_len_int,
                    );
                    offset += chunk_len;
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Stream initializers
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_InitInPStream(
    stream: R_inpstream_t,
    data: R_pstream_data_t,
    type_: R_pstream_format_t,
    inchar: Option<unsafe extern "C" fn(R_inpstream_t) -> c_int>,
    inbytes: Option<unsafe extern "C" fn(R_inpstream_t, *mut c_void, c_int)>,
    phook: Option<unsafe extern "C" fn(SEXP, SEXP) -> SEXP>,
    pdata: SEXP,
) {
    unsafe {
        if stream.is_null() {
            return;
        }
        (*stream).data = data;
        (*stream).type_ = type_;
        (*stream).InChar = inchar;
        (*stream).InBytes = inbytes;
        (*stream).InPersistHookFunc = phook;
        (*stream).InPersistHookData = pdata;
        (*stream).native_encoding[0] = 0;
        (*stream).nat2nat_obj = ptr::null_mut();
        (*stream).nat2utf8_obj = ptr::null_mut();
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_InitOutPStream(
    stream: R_outpstream_t,
    data: R_pstream_data_t,
    type_: R_pstream_format_t,
    version: c_int,
    outchar: Option<unsafe extern "C" fn(R_outpstream_t, c_int)>,
    outbytes: Option<unsafe extern "C" fn(R_outpstream_t, *const c_void, c_int)>,
    phook: Option<unsafe extern "C" fn(SEXP, SEXP) -> SEXP>,
    pdata: SEXP,
) {
    unsafe {
        if stream.is_null() {
            return;
        }
        let ver = if version != 0 {
            version
        } else {
            R_DEFAULT_SERIALIZE_VERSION
        };
        (*stream).data = data;
        (*stream).type_ = type_;
        (*stream).version = ver;
        (*stream).OutChar = outchar;
        (*stream).OutBytes = outbytes;
        (*stream).OutPersistHookFunc = phook;
        (*stream).OutPersistHookData = pdata;
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_InitFileOutPStream(
    stream: R_outpstream_t,
    fp: *mut c_void,
    type_: R_pstream_format_t,
    version: c_int,
    phook: Option<unsafe extern "C" fn(SEXP, SEXP) -> SEXP>,
    pdata: SEXP,
) {
    unsafe {
        R_InitOutPStream(stream, fp, type_, version, None, None, phook, pdata);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_InitFileInPStream(
    stream: R_inpstream_t,
    fp: *mut c_void,
    type_: R_pstream_format_t,
    phook: Option<unsafe extern "C" fn(SEXP, SEXP) -> SEXP>,
    pdata: SEXP,
) {
    unsafe {
        R_InitInPStream(stream, fp, type_, None, None, phook, pdata);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_InitConnOutPStream(
    stream: R_outpstream_t,
    con: *mut c_void,
    type_: R_pstream_format_t,
    version: c_int,
    phook: Option<unsafe extern "C" fn(SEXP, SEXP) -> SEXP>,
    pdata: SEXP,
) {
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_InitConnInPStream(
    stream: R_inpstream_t,
    con: *mut c_void,
    type_: R_pstream_format_t,
    phook: Option<unsafe extern "C" fn(SEXP, SEXP) -> SEXP>,
    pdata: SEXP,
) {
}

// ---------------------------------------------------------------------------
// R_serialize / R_unserialize (R-level entry points, memory-based)
// ---------------------------------------------------------------------------

/// Serialize an R object to a raw vector (when icon is R_NilValue).
/// This is the main entry point for `serialize()` in R.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_serialize(
    object: SEXP,
    icon: SEXP,
    ascii: SEXP,
    Sversion: SEXP,
    fun: SEXP,
) -> SEXP {
    unsafe {
        if object.is_null() {
            return R_NilValue();
        }

        // Build the header
        let mut writer = BinaryWriter::new();
        // Format: 'B' + '\n' for binary
        writer.write_byte(b'B');
        writer.write_byte(b'\n');

        // Version info (version 3)
        let version = R_DEFAULT_SERIALIZE_VERSION;
        writer.write_i32(3); // version
        writer.write_i32(R_VERSION); // writer version
        writer.write_i32(R_VERSION_350); // min reader version
        writer.write_i32(0); // native encoding length

        // Serialize the object
        let mut ref_table = WriteHashTable::new();
        WriteItemInternal(object, &mut ref_table, &mut writer);

        // Create RAWSXP from the serialized bytes
        let data = writer.into_vec();
        let raw = Rf_allocVector3(SEXPTYPE::RAWSXP.0, data.len() as R_xlen_t);
        if !raw.is_null() && !data.is_empty() {
            let raw_ptr = RAW(raw);
            ptr::copy_nonoverlapping(data.as_ptr(), raw_ptr, data.len());
        }
        raw
    }
}

/// Unserialize an R object from a raw vector.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_unserialize(icon: SEXP, fun: SEXP) -> SEXP {
    unsafe {
        if icon.is_null() {
            return R_NilValue();
        }

        // Must be RAWSXP
        let stype = TYPEOF(icon);
        if stype != SEXPTYPE::RAWSXP.0 {
            return R_NilValue();
        }

        let len = XLENGTH(icon) as usize;
        if len == 0 {
            return R_NilValue();
        }

        let raw_ptr = RAW(icon);
        let data = slice::from_raw_parts(raw_ptr, len);

        let mut reader = BinaryReader::new(data);

        // Read format header: 2 bytes ('B' + '\n')
        let fmt1 = reader.read_byte().unwrap_or(0);
        let fmt2 = reader.read_byte().unwrap_or(0);
        if fmt1 != b'B' || fmt2 != b'\n' {
            // Could be other formats; for now only support binary
            return R_NilValue();
        }

        // Read version
        let version = reader.read_i32().unwrap_or(0);
        if version != 2 && version != 3 {
            return R_NilValue();
        }

        // Read writer_version and min_reader_version
        let _writer_version = reader.read_i32().unwrap_or(0);
        let _min_reader_version = reader.read_i32().unwrap_or(0);

        // Read native encoding length (version 3)
        if version == 3 {
            let nelen = reader.read_i32().unwrap_or(0);
            if nelen > 0 {
                // Skip encoding bytes
                let _ = reader.read_bytes(nelen as usize);
            }
        }

        // Read the object
        let mut ref_table = ReadRefTable::new();
        match ReadItemInternal(&mut reader, &mut ref_table) {
            Ok(s) => s,
            Err(_) => R_NilValue(),
        }
    }
}

// ---------------------------------------------------------------------------
// R_serializeb
// ---------------------------------------------------------------------------

unsafe fn R_serializeb(object: SEXP, icon: SEXP, xdr: SEXP, Sversion: SEXP, fun: SEXP) -> SEXP {
    unsafe { R_NilValue() }
}

// ---------------------------------------------------------------------------
// do_serialize (dispatch for serialize/unserialize builtins)
// ---------------------------------------------------------------------------

pub unsafe fn do_serialize(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        let _ = (call, op, env);
        if args.is_null() || args == R_NilValue() {
            return R_NilValue();
        }

        // serialize(object, connection, ascii)
        // object = CAR(args), connection = CADR(args), ascii = CADDR(args)
        let object = CAR(args);
        let _conn = CADR(args);
        let ascii = CADDR(args);

        // Use R_serialize to get a RAWSXP
        let _ascii_flag =
            if !ascii.is_null() && TYPEOF(ascii) == SEXPTYPE::LGLSXP.0 && LENGTH(ascii) >= 1 {
                *LOGICAL(ascii) != 0
            } else {
                false
            };

        R_serialize(object, R_NilValue(), ascii, R_NilValue(), R_NilValue())
    }
}

// ---------------------------------------------------------------------------
// do_serializeToConn
// ---------------------------------------------------------------------------

pub unsafe fn do_serializeToConn(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        let _ = (call, op, env);
        if args.is_null() || args == R_NilValue() {
            return R_NilValue();
        }

        // serializeToConn(object, connection, ascii)
        let object = CAR(args);
        let _conn = CADR(args);

        // Serialize to a raw vector, then the connection layer would write it
        // For now, just do the serialization
        R_serialize(
            object,
            R_NilValue(),
            R_NilValue(),
            R_NilValue(),
            R_NilValue(),
        )
    }
}

// ---------------------------------------------------------------------------
// do_unserializeFromConn
// ---------------------------------------------------------------------------

pub unsafe fn do_unserializeFromConn(
    call: SEXP,
    op: SEXP,
    args: SEXP,
    env: SEXP,
) -> SEXP {
    unsafe {
        let _ = (call, op, env);
        if args.is_null() || args == R_NilValue() {
            return R_NilValue();
        }

        // unserializeFromConn(connection, hook)
        let conn = CAR(args);
        // If the first argument is a raw vector, use R_unserialize directly
        if !conn.is_null() && TYPEOF(conn) == SEXPTYPE::RAWSXP.0 {
            return R_unserialize(conn, R_NilValue());
        }

        // For connection objects, we would need to read all bytes from the
        // connection first. This requires the connection infrastructure.
        // For now, return nil for non-raw connections.
        R_NilValue()
    }
}

// ---------------------------------------------------------------------------
// Lazy-load database functions
// ---------------------------------------------------------------------------

pub unsafe fn do_lazyLoadDBflush(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe { R_NilValue() }
}

pub unsafe fn do_lazyLoadDBfetch(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe { R_NilValue() }
}

pub unsafe fn do_getVarsFromFrame(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe { R_NilValue() }
}

pub unsafe fn do_lazyLoadDBinsertValue(
    call: SEXP,
    op: SEXP,
    args: SEXP,
    env: SEXP,
) -> SEXP {
    unsafe { R_NilValue() }
}

// ---------------------------------------------------------------------------
// Basic output routines (C stream-based, module-private)
// ---------------------------------------------------------------------------

unsafe fn OutInteger(stream: R_outpstream_t, i: c_int) {
    unsafe {
        if stream.is_null() {
            return;
        }
        if let Some(out_bytes) = (*stream).OutBytes {
            let bytes = i.to_ne_bytes();
            out_bytes(stream, bytes.as_ptr() as *const c_void, 4);
        }
    }
}

unsafe fn OutReal(stream: R_outpstream_t, d: c_double) {
    unsafe {
        if stream.is_null() {
            return;
        }
        if let Some(out_bytes) = (*stream).OutBytes {
            let bytes = d.to_ne_bytes();
            out_bytes(stream, bytes.as_ptr() as *const c_void, 8);
        }
    }
}

unsafe fn OutComplex(stream: R_outpstream_t, c: Rcomplex) {
    unsafe {
        OutReal(stream, c.r);
        OutReal(stream, c.i);
    }
}

unsafe fn OutByte(stream: R_outpstream_t, i: Rbyte) {
    unsafe {
        if stream.is_null() {
            return;
        }
        if let Some(out_bytes) = (*stream).OutBytes {
            out_bytes(stream, &i as *const Rbyte as *const c_void, 1);
        }
    }
}

unsafe fn OutString(stream: R_outpstream_t, s: *const c_char, length: c_int) {
    unsafe {
        if stream.is_null() || s.is_null() || length <= 0 {
            return;
        }
        if let Some(out_bytes) = (*stream).OutBytes {
            out_bytes(stream, s as *const c_void, length);
        }
    }
}

unsafe fn OutFormat(stream: R_outpstream_t) {
    unsafe {
        if stream.is_null() {
            return;
        }
        let out_ref = &mut *stream;
        match out_ref.type_ {
            R_pstream_format_t::R_pstream_binary_format => {
                if let Some(out_bytes) = out_ref.OutBytes {
                    out_bytes(stream, b"B\n".as_ptr() as *const c_void, 2);
                }
            }
            R_pstream_format_t::R_pstream_xdr_format => {
                if let Some(out_bytes) = out_ref.OutBytes {
                    out_bytes(stream, b"X\n".as_ptr() as *const c_void, 2);
                }
            }
            R_pstream_format_t::R_pstream_ascii_format
            | R_pstream_format_t::R_pstream_asciihex_format => {
                if let Some(out_bytes) = out_ref.OutBytes {
                    out_bytes(stream, b"A\n".as_ptr() as *const c_void, 2);
                }
            }
            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------
// Basic input routines (C stream-based, module-private)
// ---------------------------------------------------------------------------

unsafe fn InInteger(stream: R_inpstream_t) -> c_int {
    unsafe {
        if stream.is_null() {
            return 0;
        }
        let mut val: c_int = 0;
        if let Some(in_bytes) = (*stream).InBytes {
            in_bytes(stream, &mut val as *mut c_int as *mut c_void, 4);
        }
        val
    }
}

unsafe fn InReal(stream: R_inpstream_t) -> c_double {
    unsafe {
        if stream.is_null() {
            return 0.0;
        }
        let mut val: c_double = 0.0;
        if let Some(in_bytes) = (*stream).InBytes {
            in_bytes(stream, &mut val as *mut c_double as *mut c_void, 8);
        }
        val
    }
}

unsafe fn InComplex(stream: R_inpstream_t) -> Rcomplex {
    unsafe {
        Rcomplex {
            r: InReal(stream),
            i: InReal(stream),
        }
    }
}

unsafe fn InString(stream: R_inpstream_t, buf: *mut c_char, length: c_int) {
    unsafe {
        if stream.is_null() || buf.is_null() || length <= 0 {
            return;
        }
        if let Some(in_bytes) = (*stream).InBytes {
            in_bytes(stream, buf as *mut c_void, length);
        }
    }
}

unsafe fn InWord(stream: R_inpstream_t, buf: *mut c_char, size: c_int) {
    unsafe {
        // Simplified: just read size bytes
        InString(stream, buf, size);
    }
}

unsafe fn InFormat(stream: R_inpstream_t) {
    unsafe {
        if stream.is_null() {
            return;
        }
        // Read 2 bytes for format detection
        let mut buf = [0u8; 2];
        if let Some(in_bytes) = (*stream).InBytes {
            in_bytes(stream, buf.as_mut_ptr() as *mut c_void, 2);
        }
    }
}

// ---------------------------------------------------------------------------
// Hash table for write reference tracking (C SEXP-based, for C API)
// ---------------------------------------------------------------------------

unsafe fn MakeHashTable() -> SEXP {
    unsafe {
        let vec = Rf_allocVector3(SEXPTYPE::VECSXP.0, HASHSIZE as R_xlen_t);
        Rf_cons(R_NilValue(), vec)
    }
}

unsafe fn HashAdd(obj: SEXP, ht: SEXP) {
    // Simplified stub
}

unsafe fn HashGet(item: SEXP, ht: SEXP) -> c_int {
    0
}

// ---------------------------------------------------------------------------
// Persistent name / special hooks
// ---------------------------------------------------------------------------

unsafe fn GetPersistentName(stream: R_outpstream_t, s: SEXP) -> SEXP {
    unsafe { R_NilValue() }
}

unsafe fn PersistentRestore(stream: R_inpstream_t, s: SEXP) -> SEXP {
    unsafe { R_NilValue() }
}

unsafe fn SaveSpecialHookItem(item: SEXP) -> c_int {
    0
}

// ---------------------------------------------------------------------------
// Length writing
// ---------------------------------------------------------------------------

unsafe fn WriteLENGTH(stream: R_outpstream_t, s: SEXP) {
    unsafe {
        OutInteger(stream, LENGTH(s));
    }
}

// ---------------------------------------------------------------------------
// Vector serialization (C stream-based, module-private)
// ---------------------------------------------------------------------------

unsafe fn OutStringVec(stream: R_outpstream_t, s: SEXP, ref_table: SEXP) {
    unsafe {
        let len = XLENGTH(s);
        OutInteger(stream, 0);
        WriteLENGTH(stream, s);
        for i in 0..len {
            let elt = STRING_ELT(s, i as R_xlen_t);
            R_WriteItem(elt, stream);
        }
    }
}

unsafe fn InStringVec(stream: R_inpstream_t, ref_table: SEXP) -> SEXP {
    unsafe { R_NilValue() }
}

unsafe fn OutIntegerVec(stream: R_outpstream_t, s: SEXP, length: R_xlen_t) {
    unsafe {
        let int_data = INTEGER(s);
        for i in 0..length as isize {
            OutInteger(stream, *int_data.offset(i));
        }
    }
}

unsafe fn InIntegerVec(stream: R_inpstream_t, obj: SEXP, length: R_xlen_t) {
    unsafe {
        let int_data = INTEGER(obj);
        for i in 0..length as isize {
            *int_data.offset(i) = InInteger(stream);
        }
    }
}

unsafe fn OutRealVec(stream: R_outpstream_t, s: SEXP, length: R_xlen_t) {
    unsafe {
        let real_data = REAL(s);
        for i in 0..length as isize {
            OutReal(stream, *real_data.offset(i));
        }
    }
}

unsafe fn InRealVec(stream: R_inpstream_t, obj: SEXP, length: R_xlen_t) {
    unsafe {
        let real_data = REAL(obj);
        for i in 0..length as isize {
            *real_data.offset(i) = InReal(stream);
        }
    }
}

unsafe fn OutComplexVec(stream: R_outpstream_t, s: SEXP, length: R_xlen_t) {
    unsafe {
        let cpx_data = COMPLEX(s);
        for i in 0..length as isize {
            let c = *cpx_data.offset(i);
            OutComplex(stream, c);
        }
    }
}

unsafe fn InComplexVec(stream: R_inpstream_t, obj: SEXP, length: R_xlen_t) {
    unsafe {
        let cpx_data = COMPLEX(obj);
        for i in 0..length as isize {
            *cpx_data.offset(i) = InComplex(stream);
        }
    }
}

// ---------------------------------------------------------------------------
// Bytecode serialization (stubs)
// ---------------------------------------------------------------------------

unsafe fn WriteBC(s: SEXP, ref_table: SEXP, stream: R_outpstream_t) {}

unsafe fn ReadBC(ref_table: SEXP, stream: R_inpstream_t) -> SEXP {
    unsafe { R_NilValue() }
}

// ---------------------------------------------------------------------------
// Character conversion and reading
// ---------------------------------------------------------------------------

unsafe fn ConvertChar(obj: *mut c_void, inp: *mut c_char, inplen: usize, enc: c_int) {}

unsafe fn ReadChar(stream: R_inpstream_t, buf: *mut c_char, length: c_int, levs: c_int) {
    unsafe {
        InString(stream, buf, length);
        if !buf.is_null() && length >= 0 {
            *buf.add(length as usize) = 0;
        }
    }
}

// ---------------------------------------------------------------------------
// Read reference table (C SEXP-based)
// ---------------------------------------------------------------------------

unsafe fn MakeReadRefTable() -> SEXP {
    unsafe {
        let data = Rf_allocVector3(SEXPTYPE::VECSXP.0, INITIAL_REFREAD_TABLE_SIZE as R_xlen_t);
        Rf_cons(data, R_NilValue())
    }
}

unsafe fn GetReadRef(table: SEXP, index: c_int) -> SEXP {
    unsafe { R_NilValue() }
}

unsafe fn AddReadRef(table: SEXP, value: SEXP) {}

// ---------------------------------------------------------------------------
// R_InitSerializeRoutines
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_InitSerializeRoutines() {
    // No-op: all routines are statically available.
}

// ---------------------------------------------------------------------------
// Memory buffer operations
// ---------------------------------------------------------------------------

#[repr(C)]
struct membuf_st {
    size: R_size_t,
    count: R_size_t,
    buf: *mut u8,
}

unsafe fn resize_buffer(mb: *mut membuf_st, needed: R_size_t) {
    unsafe {
        if mb.is_null() {
            return;
        }
        let new_size = std::cmp::max(needed, (*mb).size * 2);
        let new_buf = libc::realloc((*mb).buf as *mut c_void, new_size) as *mut u8;
        if !new_buf.is_null() {
            (*mb).buf = new_buf;
            (*mb).size = new_size;
        }
    }
}

unsafe extern "C" fn OutCharMem(stream: R_outpstream_t, c: c_int) {
    unsafe {
        if stream.is_null() {
            return;
        }
        let mb = (*stream).data as *mut membuf_st;
        if mb.is_null() {
            return;
        }
        if (*mb).count >= (*mb).size {
            resize_buffer(mb, (*mb).count + 1);
        }
        if !(*mb).buf.is_null() {
            *(*mb).buf.add((*mb).count) = c as u8;
            (*mb).count += 1;
        }
    }
}

unsafe extern "C" fn OutBytesMem(stream: R_outpstream_t, buf: *const c_void, length: c_int) {
    unsafe {
        if stream.is_null() || buf.is_null() || length <= 0 {
            return;
        }
        let mb = (*stream).data as *mut membuf_st;
        if mb.is_null() {
            return;
        }
        let needed = (*mb).count + (length as R_size_t);
        if needed > (*mb).size {
            resize_buffer(mb, needed);
        }
        if !(*mb).buf.is_null() {
            ptr::copy_nonoverlapping(
                buf as *const u8,
                (*mb).buf.add((*mb).count),
                length as usize,
            );
            (*mb).count = needed;
        }
    }
}

unsafe extern "C" fn InCharMem(stream: R_inpstream_t) -> c_int {
    unsafe {
        if stream.is_null() {
            return -1;
        }
        let mb = (*stream).data as *mut membuf_st;
        if mb.is_null() {
            return -1;
        }
        if (*mb).count >= (*mb).size {
            return -1;
        }
        let val = *(*mb).buf.add((*mb).count) as c_int;
        (*mb).count += 1;
        val
    }
}

unsafe extern "C" fn InBytesMem(stream: R_inpstream_t, buf: *mut c_void, length: c_int) {
    unsafe {
        if stream.is_null() || buf.is_null() || length <= 0 {
            return;
        }
        let mb = (*stream).data as *mut membuf_st;
        if mb.is_null() {
            return;
        }
        if (*mb).count + (length as R_size_t) > (*mb).size {
            return;
        }
        ptr::copy_nonoverlapping((*mb).buf.add((*mb).count), buf as *mut u8, length as usize);
        (*mb).count += length as R_size_t;
    }
}

unsafe fn InitMemInPStream(
    stream: R_inpstream_t,
    mb: *mut membuf_st,
    buf: *mut c_void,
    length: R_size_t,
    phook: Option<unsafe extern "C" fn(SEXP, SEXP) -> SEXP>,
    pdata: SEXP,
) {
    unsafe {
        if mb.is_null() {
            return;
        }
        (*mb).count = 0;
        (*mb).size = length;
        (*mb).buf = buf as *mut u8;
        R_InitInPStream(
            stream,
            mb as R_pstream_data_t,
            R_pstream_format_t::R_pstream_any_format,
            Some(InCharMem),
            Some(InBytesMem),
            phook,
            pdata,
        );
    }
}

unsafe fn InitMemOutPStream(
    stream: R_outpstream_t,
    mb: *mut membuf_st,
    type_: R_pstream_format_t,
    version: c_int,
    phook: Option<unsafe extern "C" fn(SEXP, SEXP) -> SEXP>,
    pdata: SEXP,
) {
    unsafe {
        if mb.is_null() {
            return;
        }
        (*mb).count = 0;
        (*mb).size = 0;
        (*mb).buf = ptr::null_mut();
        R_InitOutPStream(
            stream,
            mb as R_pstream_data_t,
            type_,
            version,
            Some(OutCharMem),
            Some(OutBytesMem),
            phook,
            pdata,
        );
    }
}

unsafe fn CloseMemOutPStream(stream: R_outpstream_t) -> SEXP {
    unsafe {
        if stream.is_null() {
            return R_NilValue();
        }
        let mb = (*stream).data as *mut membuf_st;
        if mb.is_null() {
            return R_NilValue();
        }
        let count = (*mb).count;
        let val = Rf_allocVector3(SEXPTYPE::RAWSXP.0, count as R_xlen_t);
        if !val.is_null() && count > 0 && !(*mb).buf.is_null() {
            ptr::copy_nonoverlapping((*mb).buf, RAW(val), count);
        }
        free_mem_buffer(mb as *mut c_void);
        val
    }
}

unsafe fn free_mem_buffer(data: *mut c_void) {
    unsafe {
        if data.is_null() {
            return;
        }
        let mb = data as *mut membuf_st;
        if !(*mb).buf.is_null() {
            libc::free((*mb).buf as *mut c_void);
            (*mb).buf = ptr::null_mut();
        }
    }
}

// ---------------------------------------------------------------------------
// Buffer-connection operations (stubs)
// ---------------------------------------------------------------------------

unsafe fn InitBConOutPStream(
    stream: R_outpstream_t,
    bbs: *mut c_void,
    con: *mut c_void,
    type_: R_pstream_format_t,
    version: c_int,
    phook: Option<unsafe extern "C" fn(SEXP, SEXP) -> SEXP>,
    pdata: SEXP,
) {
}

unsafe fn flush_bcon_buffer(bbs: *mut c_void) {}

// ---------------------------------------------------------------------------
// File I/O callbacks
// ---------------------------------------------------------------------------

unsafe fn OutCharFile(stream: R_outpstream_t, c: c_int) {}

unsafe fn OutBytesFile(stream: R_outpstream_t, buf: *const c_void, length: c_int) {}

unsafe fn InCharFile(stream: R_inpstream_t) -> c_int {
    0
}

unsafe fn InBytesFile(stream: R_inpstream_t, buf: *mut c_void, length: c_int) {}

unsafe fn InInit(stream: R_inpstream_t, buf: *mut c_void, length: c_int) {}

// ---------------------------------------------------------------------------
// Connection I/O
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_WriteConnection(
    con: *mut c_void,
    buf: *const c_void,
    n: usize,
) -> usize {
    0
}

// ---------------------------------------------------------------------------
// Lazy-load helpers (module-private)
// ---------------------------------------------------------------------------

unsafe fn CallHook(x: SEXP, fun: SEXP) -> SEXP {
    unsafe { R_NilValue() }
}

unsafe fn checkNotPromise(val: SEXP) -> SEXP {
    val
}

unsafe fn appendRawToFile(file: SEXP, bytes: SEXP) -> SEXP {
    unsafe { R_NilValue() }
}

unsafe fn readRawFromFile(file: SEXP, key: SEXP) -> SEXP {
    unsafe { R_NilValue() }
}

unsafe fn R_lazyLoadDBinsertValue(
    value: SEXP,
    file: SEXP,
    ascii: SEXP,
    compsxp: SEXP,
    hook: SEXP,
) -> SEXP {
    unsafe { R_NilValue() }
}

unsafe fn R_getVarsFromFrame(vars: SEXP, env: SEXP, forcesxp: SEXP) -> SEXP {
    unsafe { R_NilValue() }
}

// ---------------------------------------------------------------------------
// Compression functions (stubs)
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_compress1(inp: SEXP) -> SEXP {
    unsafe { R_NilValue() }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_decompress1(inp: SEXP, err: *mut Rboolean) -> SEXP {
    unsafe {
        if !err.is_null() {
            *err = 0; // FALSE
        }
        R_NilValue()
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_compress2(inp: SEXP) -> SEXP {
    unsafe { R_NilValue() }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_decompress2(inp: SEXP, err: *mut Rboolean) -> SEXP {
    unsafe {
        if !err.is_null() {
            *err = 0;
        }
        R_NilValue()
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_compress3(inp: SEXP) -> SEXP {
    unsafe { R_NilValue() }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_decompress3(inp: SEXP, err: *mut Rboolean) -> SEXP {
    unsafe {
        if !err.is_null() {
            *err = 0;
        }
        R_NilValue()
    }
}

// ---------------------------------------------------------------------------
// R_snprintf utility (stub)
// ---------------------------------------------------------------------------

unsafe fn Rsnprintf(buf: *mut c_char, size: usize, _format: *const c_char) -> c_int {
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
    last: c_int,
    stream: R_inpstream_t,
}

pub type R_instring_stream_t = *mut R_instring_stream_st;

unsafe fn InitInStringStream(s: R_instring_stream_t, stream: R_inpstream_t) {
    unsafe {
        if !s.is_null() {
            (*s).last = 0;
            (*s).stream = stream;
        }
    }
}

unsafe fn GetChar(s: R_instring_stream_t) -> c_int {
    -1
}

unsafe fn UngetChar(s: R_instring_stream_t, c: c_int) {
    unsafe {
        if !s.is_null() {
            (*s).last = c;
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_serialize_version() {
        unsafe {
            let v = defaultSerializeVersion();
            assert_eq!(v, 3);
        }
    }

    #[test]
    fn test_binary_writer_reader_i32() {
        let mut writer = BinaryWriter::new();
        writer.write_i32(42);
        writer.write_i32(-1);
        writer.write_i32(i32::MAX);
        let data = writer.into_vec();
        assert_eq!(data.len(), 12);

        let mut reader = BinaryReader::new(&data);
        assert_eq!(reader.read_i32().unwrap(), 42);
        assert_eq!(reader.read_i32().unwrap(), -1);
        assert_eq!(reader.read_i32().unwrap(), i32::MAX);
    }

    #[test]
    fn test_binary_writer_reader_f64() {
        let mut writer = BinaryWriter::new();
        writer.write_f64(3.14);
        writer.write_f64(-1.0);
        writer.write_f64(0.0);
        let data = writer.into_vec();

        let mut reader = BinaryReader::new(&data);
        assert!((reader.read_f64().unwrap() - 3.14).abs() < 1e-10);
        assert_eq!(reader.read_f64().unwrap(), -1.0);
        assert_eq!(reader.read_f64().unwrap(), 0.0);
    }

    #[test]
    fn test_binary_writer_reader_bytes() {
        let mut writer = BinaryWriter::new();
        writer.write_byte(0xFF);
        writer.write_byte(0x00);
        writer.write_byte(0x42);
        let data = writer.into_vec();

        let mut reader = BinaryReader::new(&data);
        assert_eq!(reader.read_byte().unwrap(), 0xFF);
        assert_eq!(reader.read_byte().unwrap(), 0x00);
        assert_eq!(reader.read_byte().unwrap(), 0x42);
    }

    #[test]
    fn test_pack_flags() {
        unsafe {
            // Type only, no flags
            let flags = PackFlags(13, 0, 0, 0, 0);
            assert_eq!(flags, 13);

            // Type with object flag
            let flags = PackFlags(14, 0, 1, 0, 0);
            assert_eq!(flags, 14 | IS_OBJECT_BIT_MASK);

            // Type with attr and tag flags
            let flags = PackFlags(19, 0, 0, 1, 1);
            assert_eq!(flags, 19 | HAS_ATTR_BIT_MASK | HAS_TAG_BIT_MASK);

            // Type with levels
            let flags = PackFlags(10, 3, 0, 0, 0);
            assert_eq!(flags, 10 | (3 << 12));
        }
    }

    #[test]
    fn test_unpack_flags() {
        unsafe {
            let mut ptype = 0i32;
            let mut plevs = 0i32;
            let mut pisobj = 0i32;
            let mut phasattr = 0i32;
            let mut phastag = 0i32;

            let flags = 19 | HAS_ATTR_BIT_MASK | HAS_TAG_BIT_MASK | (2 << 12);
            UnpackFlags(
                flags,
                &mut ptype,
                &mut plevs,
                &mut pisobj,
                &mut phasattr,
                &mut phastag,
            );
            assert_eq!(ptype, 19);
            assert_eq!(plevs, 2);
            assert_eq!(pisobj, 0);
            assert_eq!(phasattr, 1);
            assert_eq!(phastag, 1);
        }
    }

    #[test]
    fn test_decode_version() {
        unsafe {
            let mut v = 0i32;
            let mut p = 0i32;
            let mut s = 0i32;
            DecodeVersion(R_VERSION_450, &mut v, &mut p, &mut s);
            assert_eq!(v, 4);
            assert_eq!(p, 5);
            assert_eq!(s, 0);

            DecodeVersion(R_VERSION_230, &mut v, &mut p, &mut s);
            assert_eq!(v, 2);
            assert_eq!(p, 3);
            assert_eq!(s, 0);

            DecodeVersion(R_VERSION_350, &mut v, &mut p, &mut s);
            assert_eq!(v, 3);
            assert_eq!(p, 5);
            assert_eq!(s, 0);
        }
    }

    #[test]
    fn test_write_hash_table() {
        let mut ht = WriteHashTable::new();
        assert_eq!(ht.count, 0);

        // Getting a non-existent key returns 0
        let fake_ptr = 0x1000 as *mut std::os::raw::c_void as SEXP;
        assert_eq!(ht.get(fake_ptr), 0);

        // Add and retrieve
        ht.add(fake_ptr);
        assert_eq!(ht.count, 1);
        assert_eq!(ht.get(fake_ptr), 1);

        // Add second
        let fake_ptr2 = 0x2000 as *mut std::os::raw::c_void as SEXP;
        ht.add(fake_ptr2);
        assert_eq!(ht.count, 2);
        assert_eq!(ht.get(fake_ptr2), 2);
        assert_eq!(ht.get(fake_ptr), 1);
    }

    #[test]
    fn test_read_ref_table() {
        let mut rt = ReadRefTable::new();
        let fake1 = 0x1000 as *mut std::os::raw::c_void as SEXP;
        let fake2 = 0x2000 as *mut std::os::raw::c_void as SEXP;

        rt.add(fake1);
        rt.add(fake2);
        assert_eq!(rt.get(1).unwrap(), fake1);
        assert_eq!(rt.get(2).unwrap(), fake2);
        assert!(rt.get(3).is_err());
        assert!(rt.get(0).is_err());
    }

    #[test]
    fn test_ref_index_packing() {
        unsafe {
            let mut writer = BinaryWriter::new();
            // Small index: packed into single integer
            OutRefIndex(&mut writer, 1);
            let data = writer.into_vec();
            let mut reader = BinaryReader::new(&data);
            let flags = reader.read_i32().unwrap();
            assert_eq!(flags & 0xFF, REFSXP);
            assert_eq!(flags >> 8, 1);
        }
    }

    #[test]
    fn test_constants() {
        assert_eq!(HASHSIZE, 1009);
        assert_eq!(INITIAL_REFREAD_TABLE_SIZE, 128);
        assert_eq!(REFSXP, 255);
        assert_eq!(NC, 100);
        assert_eq!(R_CODESET_MAX, 256);
        assert_eq!(CHUNK_SIZE, 512);
        assert_eq!(R_DEFAULT_SERIALIZE_VERSION, 3);
        assert_eq!(NILVALUE_SXP, 254);
        assert_eq!(GLOBALENV_SXP, 253);
        assert_eq!(MAX_PACKED_INDEX, c_int::MAX >> 8);
    }

    #[test]
    fn test_r_serialize_returns_nil_for_null() {
        unsafe {
            let result = R_serialize(
                ptr::null_mut(),
                R_NilValue(),
                R_NilValue(),
                R_NilValue(),
                R_NilValue(),
            );
            assert_eq!(result, R_NilValue());
        }
    }

    #[test]
    fn test_r_unserialize_returns_nil_for_null() {
        unsafe {
            let result = R_unserialize(R_NilValue(), R_NilValue());
            assert_eq!(result, R_NilValue());
        }
    }

    #[test]
    fn test_r_unserialize_returns_nil_for_empty_raw() {
        unsafe {
            let raw = Rf_allocVector3(SEXPTYPE::RAWSXP.0, 0);
            let result = R_unserialize(raw, R_NilValue());
            assert_eq!(result, R_NilValue());
        }
    }

    #[test]
    fn test_r_unserialize_returns_nil_for_non_raw() {
        unsafe {
            let s = Rf_allocVector3(SEXPTYPE::INTSXP.0, 1);
            let result = R_unserialize(s, R_NilValue());
            assert_eq!(result, R_NilValue());
        }
    }

    #[test]
    fn test_r_serialize_info_returns_nil() {
        unsafe {
            let result = R_SerializeInfo(ptr::null_mut());
            assert_eq!(result, R_NilValue());
        }
    }

    #[test]
    fn test_do_serialize_returns_nil() {
        unsafe {
            let result = do_serialize(
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            );
            assert_eq!(result, R_NilValue());
        }
    }

    #[test]
    fn test_do_serialize_to_conn_returns_nil() {
        unsafe {
            let result = do_serializeToConn(
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            );
            assert_eq!(result, R_NilValue());
        }
    }

    #[test]
    fn test_do_unserialize_from_conn_returns_nil() {
        unsafe {
            let result = do_unserializeFromConn(
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            );
            assert_eq!(result, R_NilValue());
        }
    }

    #[test]
    fn test_do_lazy_load_db_flush_returns_nil() {
        unsafe {
            let result = do_lazyLoadDBflush(
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            );
            assert_eq!(result, R_NilValue());
        }
    }

    #[test]
    fn test_do_lazy_load_db_fetch_returns_nil() {
        unsafe {
            let result = do_lazyLoadDBfetch(
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            );
            assert_eq!(result, R_NilValue());
        }
    }

    #[test]
    fn test_do_get_vars_from_frame_returns_nil() {
        unsafe {
            let result = do_getVarsFromFrame(
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            );
            assert_eq!(result, R_NilValue());
        }
    }

    #[test]
    fn test_do_lazy_load_db_insert_value_returns_nil() {
        unsafe {
            let result = do_lazyLoadDBinsertValue(
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            );
            assert_eq!(result, R_NilValue());
        }
    }

    #[test]
    fn test_r_compress_decompress_returns_nil() {
        unsafe {
            let nil = R_NilValue();
            let mut err: Rboolean = 1;
            assert_eq!(R_compress1(nil), nil);
            assert_eq!(R_decompress1(nil, &mut err), nil);
            assert_eq!(err, 0);
            assert_eq!(R_compress2(nil), nil);
            assert_eq!(R_decompress2(nil, &mut err), nil);
            assert_eq!(err, 0);
            assert_eq!(R_compress3(nil), nil);
            assert_eq!(R_decompress3(nil, &mut err), nil);
            assert_eq!(err, 0);
        }
    }

    #[test]
    fn test_r_write_connection_returns_zero() {
        unsafe {
            let n = R_WriteConnection(ptr::null_mut(), ptr::null(), 42);
            assert_eq!(n, 0);
        }
    }

    // ---- Round-trip serialization tests ----

    #[test]
    fn test_roundtrip_integer_scalar() {
        unsafe {
            let s = Rf_ScalarInteger(42);
            let raw = R_serialize(s, R_NilValue(), R_NilValue(), R_NilValue(), R_NilValue());
            assert_ne!(raw, R_NilValue());
            assert!(XLENGTH(raw) > 0);

            let result = R_unserialize(raw, R_NilValue());
            assert_ne!(result, R_NilValue());
            assert_eq!(TYPEOF(result), SEXPTYPE::INTSXP.0);
            assert_eq!(LENGTH(result), 1);
            assert_eq!(*INTEGER(result), 42);
        }
    }

    #[test]
    fn test_roundtrip_real_scalar() {
        unsafe {
            let s = Rf_ScalarReal(3.14159);
            let raw = R_serialize(s, R_NilValue(), R_NilValue(), R_NilValue(), R_NilValue());
            assert_ne!(raw, R_NilValue());

            let result = R_unserialize(raw, R_NilValue());
            assert_ne!(result, R_NilValue());
            assert_eq!(TYPEOF(result), SEXPTYPE::REALSXP.0);
            assert_eq!(LENGTH(result), 1);
            assert!((*REAL(result) - 3.14159).abs() < 1e-10);
        }
    }

    #[test]
    fn test_roundtrip_logical_scalar() {
        unsafe {
            let s = Rf_ScalarLogical(1); // TRUE
            let raw = R_serialize(s, R_NilValue(), R_NilValue(), R_NilValue(), R_NilValue());
            assert_ne!(raw, R_NilValue());

            let result = R_unserialize(raw, R_NilValue());
            assert_ne!(result, R_NilValue());
            assert_eq!(TYPEOF(result), SEXPTYPE::LGLSXP.0);
            assert_eq!(LENGTH(result), 1);
            assert_eq!(*LOGICAL(result), 1);
        }
    }

    #[test]
    fn test_roundtrip_integer_vector() {
        unsafe {
            let s = Rf_allocVector3(SEXPTYPE::INTSXP.0, 3);
            let data = INTEGER(s);
            *data = 10;
            *data.add(1) = 20;
            *data.add(2) = 30;

            let raw = R_serialize(s, R_NilValue(), R_NilValue(), R_NilValue(), R_NilValue());
            assert_ne!(raw, R_NilValue());

            let result = R_unserialize(raw, R_NilValue());
            assert_ne!(result, R_NilValue());
            assert_eq!(TYPEOF(result), SEXPTYPE::INTSXP.0);
            assert_eq!(LENGTH(result), 3);
            let rdata = INTEGER(result);
            assert_eq!(*rdata, 10);
            assert_eq!(*rdata.add(1), 20);
            assert_eq!(*rdata.add(2), 30);
        }
    }

    #[test]
    fn test_roundtrip_real_vector() {
        unsafe {
            let s = Rf_allocVector3(SEXPTYPE::REALSXP.0, 2);
            let data = REAL(s);
            *data = 1.5;
            *data.add(1) = 2.5;

            let raw = R_serialize(s, R_NilValue(), R_NilValue(), R_NilValue(), R_NilValue());
            assert_ne!(raw, R_NilValue());

            let result = R_unserialize(raw, R_NilValue());
            assert_ne!(result, R_NilValue());
            assert_eq!(TYPEOF(result), SEXPTYPE::REALSXP.0);
            assert_eq!(LENGTH(result), 2);
            let rdata = REAL(result);
            assert!((*rdata - 1.5).abs() < 1e-10);
            assert!((*rdata.add(1) - 2.5).abs() < 1e-10);
        }
    }

    #[test]
    fn test_roundtrip_complex_scalar() {
        unsafe {
            let s = Rf_ScalarComplex(Rcomplex { r: 1.0, i: 2.0 });
            let raw = R_serialize(s, R_NilValue(), R_NilValue(), R_NilValue(), R_NilValue());
            assert_ne!(raw, R_NilValue());

            let result = R_unserialize(raw, R_NilValue());
            assert_ne!(result, R_NilValue());
            assert_eq!(TYPEOF(result), SEXPTYPE::CPLXSXP.0);
            assert_eq!(LENGTH(result), 1);
            let c = *COMPLEX(result);
            assert!((c.r - 1.0).abs() < 1e-10);
            assert!((c.i - 2.0).abs() < 1e-10);
        }
    }

    #[test]
    fn test_roundtrip_string_scalar() {
        unsafe {
            let s = Rf_mkString(c"hello".as_ptr());
            let raw = R_serialize(s, R_NilValue(), R_NilValue(), R_NilValue(), R_NilValue());
            assert_ne!(raw, R_NilValue());

            let result = R_unserialize(raw, R_NilValue());
            assert_ne!(result, R_NilValue());
            assert_eq!(TYPEOF(result), SEXPTYPE::STRSXP.0);
            assert_eq!(LENGTH(result), 1);
            let elt = STRING_ELT(result, 0);
            assert!(!elt.is_null());
            // Check the string content
            let cstr = std::ffi::CStr::from_ptr(CHAR(elt));
            assert_eq!(cstr.to_bytes(), b"hello");
        }
    }

    #[test]
    fn test_roundtrip_string_vector() {
        unsafe {
            let s = Rf_allocVector3(SEXPTYPE::STRSXP.0, 2);
            let c1 = Rf_mkChar(c"foo".as_ptr());
            let c2 = Rf_mkChar(c"bar".as_ptr());
            SET_STRING_ELT(s, 0, c1);
            SET_STRING_ELT(s, 1, c2);

            let raw = R_serialize(s, R_NilValue(), R_NilValue(), R_NilValue(), R_NilValue());
            assert_ne!(raw, R_NilValue());

            let result = R_unserialize(raw, R_NilValue());
            assert_ne!(result, R_NilValue());
            assert_eq!(TYPEOF(result), SEXPTYPE::STRSXP.0);
            assert_eq!(LENGTH(result), 2);
            let e1 = STRING_ELT(result, 0);
            let e2 = STRING_ELT(result, 1);
            let cstr1 = std::ffi::CStr::from_ptr(CHAR(e1));
            let cstr2 = std::ffi::CStr::from_ptr(CHAR(e2));
            assert_eq!(cstr1.to_bytes(), b"foo");
            assert_eq!(cstr2.to_bytes(), b"bar");
        }
    }

    #[test]
    fn test_roundtrip_raw_vector() {
        unsafe {
            let s = Rf_allocVector3(SEXPTYPE::RAWSXP.0, 3);
            let data = RAW(s);
            *data = 0xDE;
            *data.add(1) = 0xAD;
            *data.add(2) = 0xBE;

            let raw = R_serialize(s, R_NilValue(), R_NilValue(), R_NilValue(), R_NilValue());
            assert_ne!(raw, R_NilValue());

            let result = R_unserialize(raw, R_NilValue());
            assert_ne!(result, R_NilValue());
            assert_eq!(TYPEOF(result), SEXPTYPE::RAWSXP.0);
            assert_eq!(LENGTH(result), 3);
            let rdata = RAW(result);
            assert_eq!(*rdata, 0xDE);
            assert_eq!(*rdata.add(1), 0xAD);
            assert_eq!(*rdata.add(2), 0xBE);
        }
    }

    #[test]
    fn test_roundtrip_empty_vector() {
        unsafe {
            let s = Rf_allocVector3(SEXPTYPE::INTSXP.0, 0);
            let raw = R_serialize(s, R_NilValue(), R_NilValue(), R_NilValue(), R_NilValue());
            assert_ne!(raw, R_NilValue());

            let result = R_unserialize(raw, R_NilValue());
            assert_ne!(result, R_NilValue());
            assert_eq!(TYPEOF(result), SEXPTYPE::INTSXP.0);
            assert_eq!(LENGTH(result), 0);
        }
    }

    #[test]
    fn test_roundtrip_null() {
        unsafe {
            let s = R_NilValue();
            let raw = R_serialize(s, R_NilValue(), R_NilValue(), R_NilValue(), R_NilValue());
            assert_ne!(raw, R_NilValue());

            let result = R_unserialize(raw, R_NilValue());
            assert_eq!(result, R_NilValue());
        }
    }

    #[test]
    fn test_roundtrip_generic_list() {
        unsafe {
            // Create a list(c(1, 2), c(3.0, 4.0))
            let vec1 = Rf_allocVector3(SEXPTYPE::INTSXP.0, 2);
            *INTEGER(vec1) = 1;
            *INTEGER(vec1).add(1) = 2;

            let vec2 = Rf_allocVector3(SEXPTYPE::REALSXP.0, 2);
            *REAL(vec2) = 3.0;
            *REAL(vec2).add(1) = 4.0;

            let list = Rf_allocVector3(SEXPTYPE::VECSXP.0, 2);
            SET_VECTOR_ELT(list, 0, vec1);
            SET_VECTOR_ELT(list, 1, vec2);

            let raw = R_serialize(list, R_NilValue(), R_NilValue(), R_NilValue(), R_NilValue());
            assert_ne!(raw, R_NilValue());

            let result = R_unserialize(raw, R_NilValue());
            assert_ne!(result, R_NilValue());
            assert_eq!(TYPEOF(result), SEXPTYPE::VECSXP.0);
            assert_eq!(LENGTH(result), 2);

            // Check first element (integer vector)
            let r1 = VECTOR_ELT(result, 0);
            assert_eq!(TYPEOF(r1), SEXPTYPE::INTSXP.0);
            assert_eq!(LENGTH(r1), 2);
            assert_eq!(*INTEGER(r1), 1);
            assert_eq!(*INTEGER(r1).add(1), 2);

            // Check second element (real vector)
            let r2 = VECTOR_ELT(result, 1);
            assert_eq!(TYPEOF(r2), SEXPTYPE::REALSXP.0);
            assert_eq!(LENGTH(r2), 2);
            assert!((*REAL(r2) - 3.0).abs() < 1e-10);
            assert!((*REAL(r2).add(1) - 4.0).abs() < 1e-10);
        }
    }

    #[test]
    fn test_roundtrip_nested_list() {
        unsafe {
            // Create list(list(1, 2), list(3, 4))
            let inner1 = Rf_allocVector3(SEXPTYPE::VECSXP.0, 2);
            let inner2 = Rf_allocVector3(SEXPTYPE::VECSXP.0, 2);

            let v1 = Rf_ScalarInteger(1);
            let v2 = Rf_ScalarInteger(2);
            let v3 = Rf_ScalarInteger(3);
            let v4 = Rf_ScalarInteger(4);

            SET_VECTOR_ELT(inner1, 0, v1);
            SET_VECTOR_ELT(inner1, 1, v2);
            SET_VECTOR_ELT(inner2, 0, v3);
            SET_VECTOR_ELT(inner2, 1, v4);

            let outer = Rf_allocVector3(SEXPTYPE::VECSXP.0, 2);
            SET_VECTOR_ELT(outer, 0, inner1);
            SET_VECTOR_ELT(outer, 1, inner2);

            let raw = R_serialize(
                outer,
                R_NilValue(),
                R_NilValue(),
                R_NilValue(),
                R_NilValue(),
            );
            assert_ne!(raw, R_NilValue());

            let result = R_unserialize(raw, R_NilValue());
            assert_ne!(result, R_NilValue());
            assert_eq!(TYPEOF(result), SEXPTYPE::VECSXP.0);
            assert_eq!(LENGTH(result), 2);

            let r_inner1 = VECTOR_ELT(result, 0);
            assert_eq!(TYPEOF(r_inner1), SEXPTYPE::VECSXP.0);
            assert_eq!(LENGTH(r_inner1), 2);
            let rv1 = VECTOR_ELT(r_inner1, 0);
            let rv2 = VECTOR_ELT(r_inner1, 1);
            assert_eq!(TYPEOF(rv1), SEXPTYPE::INTSXP.0);
            assert_eq!(*INTEGER(rv1), 1);
            assert_eq!(TYPEOF(rv2), SEXPTYPE::INTSXP.0);
            assert_eq!(*INTEGER(rv2), 2);

            let r_inner2 = VECTOR_ELT(result, 1);
            assert_eq!(TYPEOF(r_inner2), SEXPTYPE::VECSXP.0);
            assert_eq!(LENGTH(r_inner2), 2);
            let rv3 = VECTOR_ELT(r_inner2, 0);
            let rv4 = VECTOR_ELT(r_inner2, 1);
            assert_eq!(TYPEOF(rv3), SEXPTYPE::INTSXP.0);
            assert_eq!(*INTEGER(rv3), 3);
            assert_eq!(TYPEOF(rv4), SEXPTYPE::INTSXP.0);
            assert_eq!(*INTEGER(rv4), 4);
        }
    }

    #[test]
    fn test_serialized_format_header() {
        unsafe {
            let s = Rf_ScalarInteger(1);
            let raw = R_serialize(s, R_NilValue(), R_NilValue(), R_NilValue(), R_NilValue());
            let len = XLENGTH(raw) as usize;
            let raw_data = RAW(raw);
            let data = slice::from_raw_parts(raw_data, len);

            // Check format header: 'B' + '\n'
            assert_eq!(data[0], b'B');
            assert_eq!(data[1], b'\n');

            // Check version = 3
            let version = i32::from_ne_bytes(data[2..6].try_into().unwrap());
            assert_eq!(version, 3);
        }
    }

    #[test]
    fn test_roundtrip_large_integer_vector() {
        unsafe {
            let n: i32 = 100;
            let s = Rf_allocVector3(SEXPTYPE::INTSXP.0, n as R_xlen_t);
            let data = INTEGER(s);
            for i in 0..n as isize {
                *data.offset(i) = (i * i) as c_int;
            }

            let raw = R_serialize(s, R_NilValue(), R_NilValue(), R_NilValue(), R_NilValue());
            assert_ne!(raw, R_NilValue());

            let result = R_unserialize(raw, R_NilValue());
            assert_ne!(result, R_NilValue());
            assert_eq!(TYPEOF(result), SEXPTYPE::INTSXP.0);
            assert_eq!(LENGTH(result), n);
            let rdata = INTEGER(result);
            for i in 0..n as isize {
                assert_eq!(*rdata.offset(i), (i * i) as c_int);
            }
        }
    }

    #[test]
    fn test_binary_reader_errors() {
        let data = [0u8; 3]; // Too short for an i32
        let mut reader = BinaryReader::new(&data);
        assert!(reader.read_i32().is_err());
        assert!(reader.read_f64().is_err());
        assert!(reader.read_bytes(10).is_err());
    }

    #[test]
    fn test_binary_reader_remaining() {
        let data = [1u8, 2, 3, 4, 5];
        let mut reader = BinaryReader::new(&data);
        assert_eq!(reader.remaining(), 5);
        let _ = reader.read_byte();
        assert_eq!(reader.remaining(), 4);
    }
}
