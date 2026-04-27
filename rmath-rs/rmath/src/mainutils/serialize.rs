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

use std::cell::Cell;
use std::ffi::{CStr, CString};
use std::fs::{File, OpenOptions};
use std::io::{BufReader, Cursor, Read, Seek, SeekFrom, Write};
use std::os::raw::{c_char, c_double, c_int, c_void};
#[cfg(unix)]
use std::os::unix::ffi::OsStrExt;
use std::path::PathBuf;
use std::ptr;
use std::slice;

use bzip2::read::BzDecoder;
use bzip2::write::BzEncoder;
use flate2::Compression;
use flate2::read::ZlibDecoder;
use flate2::write::ZlibEncoder;

use crate::eval::attrib_core::{R_NamesSymbol, setAttrib};
use crate::eval::eval::Rf_eval;
use crate::mainutils::coerce::{asInteger, asLogical};
use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::envir::R_findVarInFrame;
use crate::sexp::ffi::{R_size_t, R_xlen_t, Rboolean, Rbyte, Rcomplex, SEXP, SEXPTYPE};
use crate::sexp::globals::{
    R_BaseEnv, R_EmptyEnv, R_GlobalEnv, R_MissingArg, R_NaString, R_NilValue, R_UnboundValue,
};
use crate::sexp::memory_ext::allocSExp;
use crate::sexp::protect::*;
use crate::sexp::symbol::Rf_install;

unsafe fn error(msg: &str) -> ! {
    std::panic::panic_any(crate::sexp::context::RError {
        message: msg.to_string(),
    });
}

#[inline]
unsafe fn require_arg(args: SEXP, index: usize) -> SEXP {
    let mut cur = args;
    for _ in 0..index {
        if cur.is_null() || unsafe { TYPEOF(cur) } == SEXPTYPE::NILSXP {
            unsafe { error("wrong number of arguments") };
        }
        cur = unsafe { CDR(cur) };
    }
    if cur.is_null() || unsafe { TYPEOF(cur) } == SEXPTYPE::NILSXP {
        unsafe { error("wrong number of arguments") };
    }
    unsafe { CAR(cur) }
}

#[inline]
unsafe fn sexp_to_path(file: SEXP) -> PathBuf {
    if unsafe { TYPEOF(file) } != SEXPTYPE::STRSXP || unsafe { LENGTH(file) } <= 0 {
        unsafe { error("not a proper file name") };
    }
    let elt = unsafe { STRING_ELT(file, 0) };
    let cpath = unsafe { CHAR(elt) };
    if cpath.is_null() {
        unsafe { error("not a proper file name") };
    }
    let cstr = unsafe { CStr::from_ptr(cpath) };
    #[cfg(unix)]
    {
        PathBuf::from(std::ffi::OsStr::from_bytes(cstr.to_bytes()))
    }
    #[cfg(not(unix))]
    {
        PathBuf::from(cstr.to_string_lossy().into_owned())
    }
}

#[inline]
unsafe fn raw_from_bytes(bytes: &[u8]) -> SEXP {
    let ans = unsafe { Rf_allocVector3(SEXPTYPE::RAWSXP, bytes.len() as R_xlen_t) };
    if !ans.is_null() && !bytes.is_empty() {
        unsafe {
            ptr::copy_nonoverlapping(bytes.as_ptr(), RAW(ans), bytes.len());
        }
    }
    ans
}

#[inline]
fn swapped_len_bytes(len: usize) -> [u8; 4] {
    (len as u32).swap_bytes().to_ne_bytes()
}

#[inline]
fn parse_swapped_len_prefix(data: &[u8]) -> Option<usize> {
    if data.len() < 4 {
        return None;
    }
    let mut bytes = [0u8; 4];
    bytes.copy_from_slice(&data[..4]);
    Some(u32::from_ne_bytes(bytes).swap_bytes() as usize)
}

#[inline]
fn mark_decompress_error(err: *mut Rboolean) {
    if !err.is_null() {
        unsafe {
            *err = 1;
        }
    }
}

#[inline]
fn build_compressed_blob(source_len: usize, marker: Option<u8>, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(4 + marker.map_or(0, |_| 1) + payload.len());
    out.extend_from_slice(&swapped_len_bytes(source_len));
    if let Some(tag) = marker {
        out.push(tag);
    }
    out.extend_from_slice(payload);
    out
}

fn zlib_compress(input: &[u8]) -> Result<Vec<u8>, std::io::Error> {
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(input)?;
    encoder.finish()
}

fn zlib_decompress_exact(input: &[u8], expected_len: usize) -> Result<Vec<u8>, std::io::Error> {
    let mut decoder = ZlibDecoder::new(input);
    let mut out = Vec::with_capacity(expected_len);
    decoder.read_to_end(&mut out)?;
    if out.len() != expected_len {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "zlib decompressed length mismatch",
        ));
    }
    Ok(out)
}

fn bzip2_compress(input: &[u8]) -> Result<Vec<u8>, std::io::Error> {
    let mut encoder = BzEncoder::new(Vec::new(), bzip2::Compression::best());
    encoder.write_all(input)?;
    encoder.finish()
}

fn bzip2_decompress_exact(input: &[u8], expected_len: usize) -> Result<Vec<u8>, std::io::Error> {
    let mut decoder = BzDecoder::new(input);
    let mut out = Vec::with_capacity(expected_len);
    decoder.read_to_end(&mut out)?;
    if out.len() != expected_len {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "bzip2 decompressed length mismatch",
        ));
    }
    Ok(out)
}

fn lzma2_raw_encode(input: &[u8], _out_cap: usize) -> Result<Vec<u8>, std::io::Error> {
    let mut compressed = Vec::new();
    let mut reader = BufReader::new(Cursor::new(input));
    lzma_rs::lzma2_compress(&mut reader, &mut compressed).map_err(|e| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("lzma2 compress: {e}"),
        )
    })?;
    Ok(compressed)
}

fn lzma2_raw_decode(input: &[u8], expected_len: usize) -> Result<Vec<u8>, std::io::Error> {
    let mut decompressed = Vec::with_capacity(expected_len);
    let mut reader = BufReader::new(input);
    lzma_rs::lzma2_decompress(&mut reader, &mut decompressed).map_err(|e| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("lzma2 decompress: {e}"),
        )
    })?;
    if decompressed.len() != expected_len {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "lzma2 decompressed length mismatch",
        ));
    }
    Ok(decompressed)
}

#[inline]
unsafe fn clear_lazy_load_cache() {
    USED.with(|used| used.set(0));
    CACHE_NAMES.with(|names| names.set([ptr::null_mut(); NC]));
    CACHE_PTRS.with(|ptrs| ptrs.set([ptr::null_mut(); NC]));
}

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Initial hash table size for write reference tracking.
const HASHSIZE: c_int = 1009;

/// Default serialize version (2 or 3).
const R_DEFAULT_SERIALIZE_VERSION: c_int = 3;

/// R version (4, 5, 0) packed as integer.
const R_VERSION_450: c_int = (4 << 16) | (5 << 8);

/// R version (2, 3, 0) packed as integer.
const R_VERSION_230: c_int = (2 << 16) | (3 << 8);

/// R version (3, 5, 0) packed as integer.
const R_VERSION_350: c_int = (3 << 16) | (5 << 8);

/// Writer R version in packed form.
const R_VERSION: c_int = R_VERSION_450;

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

thread_local! { static USED: Cell<usize> = Cell::new(0); }

thread_local! { static CACHE_NAMES: Cell<[*mut c_char; NC]> = Cell::new([ptr::null_mut(); NC]); }

thread_local! { static CACHE_PTRS: Cell<[*mut c_char; NC]> = Cell::new([ptr::null_mut(); NC]); }

thread_local! { static R_ReadItemDepth: Cell<c_int> = Cell::new(0); }

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
        let bytes: [u8; 4] = self.data[self.pos..self.pos + 4]
            .try_into()
            .unwrap_or([0; 4]);
        self.pos += 4;
        Ok(i32::from_ne_bytes(bytes))
    }

    fn read_f64(&mut self) -> Result<f64, String> {
        if self.remaining() < 8 {
            return Err("read error: not enough bytes for f64".into());
        }
        let bytes: [u8; 8] = self.data[self.pos..self.pos + 8]
            .try_into()
            .unwrap_or([0; 8]);
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
        rem %= 256;
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
            // Write CLOENV, FORMALS, BODY
            WriteItemInternal(CDR(s), ref_table, writer); // CLOENV stored in CDR for our simplified model
            WriteItemInternal(TAG(s), ref_table, writer); // FORMALS stored in TAG
            WriteItemInternal(CAR(s), ref_table, writer); // BODY stored in CAR
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
            Ok(s)
        } else if stype == SEXPTYPE::CHARSXP {
            let len = reader.read_i32()?;
            if len < 0 {
                Ok(R_NaString())
            } else if len == 0 {
                let s = Rf_mkCharLen(b"\0" as *const u8 as *const c_char, 0);
                Ok(s)
            } else {
                let bytes = reader.read_bytes(len as usize)?;
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

// ---------------------------------------------------------------------------
// R_Serialize -- serialize an R object to a stream (C API)
// ---------------------------------------------------------------------------

pub unsafe fn R_Serialize(s: SEXP, stream: R_outpstream_t) {
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
            _ => {} // intentionally unhandled: unknown serialization format
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
        if !data.is_empty()
            && let Some(out_bytes) = (*stream).OutBytes
        {
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

// ---------------------------------------------------------------------------
// R_Unserialize -- unserialize an R object from a stream (C API)
// ---------------------------------------------------------------------------

pub unsafe fn R_Unserialize(stream: R_inpstream_t) -> SEXP {
    unsafe {
        if stream.is_null() {
            error("read error");
        }
        let bytes = read_stream_bytes_via_inchar(stream);
        if bytes.is_empty() {
            error("read error");
        }
        let raw = raw_from_bytes(&bytes);
        R_unserialize(raw, R_NilValue())
    }
}

// ---------------------------------------------------------------------------
// R_SerializeInfo
// ---------------------------------------------------------------------------

pub unsafe fn R_SerializeInfo(stream: R_inpstream_t) -> SEXP {
    unsafe {
        if stream.is_null() {
            error("read error");
        }
        InFormat(stream);

        let version = InInteger(stream);
        let anslen = if version == 3 { 5 } else { 4 };
        let writer_version = InInteger(stream);
        let min_reader_version = InInteger(stream);

        let ans = Rf_allocVector3(SEXPTYPE::VECSXP, anslen as R_xlen_t);
        let names = Rf_allocVector3(SEXPTYPE::STRSXP, anslen as R_xlen_t);
        let _ans_guard = protect(ans);
        let _names_guard = protect(names);

        SET_STRING_ELT(names, 0, Rf_mkChar(c"version".as_ptr()));
        SET_VECTOR_ELT(ans, 0, Rf_ScalarInteger(version));

        SET_STRING_ELT(names, 1, Rf_mkChar(c"writer_version".as_ptr()));
        let mut vv = 0;
        let mut vp = 0;
        let mut vs = 0;
        DecodeVersion(writer_version, &mut vv, &mut vp, &mut vs);
        let writer_s = format!("{vv}.{vp}.{vs}");
        let writer_c = CString::new(writer_s).unwrap_or_default();
        SET_VECTOR_ELT(ans, 1, Rf_mkString(writer_c.as_ptr()));

        SET_STRING_ELT(names, 2, Rf_mkChar(c"min_reader_version".as_ptr()));
        if min_reader_version < 0 {
            SET_VECTOR_ELT(ans, 2, Rf_ScalarString(R_NaString()));
        } else {
            DecodeVersion(min_reader_version, &mut vv, &mut vp, &mut vs);
            let min_reader_s = format!("{vv}.{vp}.{vs}");
            let min_reader_c = CString::new(min_reader_s).unwrap_or_default();
            SET_VECTOR_ELT(ans, 2, Rf_mkString(min_reader_c.as_ptr()));
        }

        SET_STRING_ELT(names, 3, Rf_mkChar(c"format".as_ptr()));
        match (*stream).type_ {
            R_pstream_format_t::R_pstream_ascii_format
            | R_pstream_format_t::R_pstream_asciihex_format => {
                SET_VECTOR_ELT(ans, 3, Rf_mkString(c"ascii".as_ptr()));
            }
            R_pstream_format_t::R_pstream_binary_format => {
                SET_VECTOR_ELT(ans, 3, Rf_mkString(c"binary".as_ptr()));
            }
            R_pstream_format_t::R_pstream_xdr_format => {
                SET_VECTOR_ELT(ans, 3, Rf_mkString(c"xdr".as_ptr()));
            }
            _ => error("unknown input format"),
        }

        if version == 3 {
            SET_STRING_ELT(names, 4, Rf_mkChar(c"native_encoding".as_ptr()));
            let nelen = InInteger(stream);
            if !(0..=R_CODESET_MAX).contains(&nelen) {
                error("invalid length of encoding name");
            }
            if nelen == 0 {
                SET_VECTOR_ELT(ans, 4, Rf_mkString(c"".as_ptr()));
            } else {
                let mut bytes = vec![0u8; nelen as usize];
                InString(stream, bytes.as_mut_ptr() as *mut c_char, nelen);
                let enc_ch = Rf_mkCharLen(bytes.as_ptr() as *const c_char, nelen);
                SET_VECTOR_ELT(ans, 4, Rf_ScalarString(enc_ch));
            }
        }

        setAttrib(ans, R_NamesSymbol(), names);
        ans
    }
}

// ---------------------------------------------------------------------------
// R_ReadItem / R_WriteItem (C stream API)
// ---------------------------------------------------------------------------

pub unsafe fn R_ReadItem(stream: R_inpstream_t) -> SEXP {
    unsafe {
        if stream.is_null() {
            error("read error");
        }
        let bytes = read_stream_bytes_via_inchar(stream);
        if bytes.is_empty() {
            error("read error");
        }
        let mut reader = BinaryReader::new(&bytes);
        let mut ref_table = ReadRefTable::new();
        match ReadItemInternal(&mut reader, &mut ref_table) {
            Ok(v) => v,
            Err(_) => error("read error"),
        }
    }
}

pub unsafe fn R_WriteItem(s: SEXP, stream: R_outpstream_t) {
    unsafe {
        if stream.is_null() {
            return;
        }
        let mut writer = BinaryWriter::new();
        let mut ref_table = WriteHashTable::new();
        WriteItemInternal(s, &mut ref_table, &mut writer);
        let data = writer.into_vec();
        if !data.is_empty()
            && let Some(out_bytes) = (*stream).OutBytes
        {
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

unsafe fn read_stream_bytes_via_inchar(stream: R_inpstream_t) -> Vec<u8> {
    unsafe {
        if stream.is_null() {
            return Vec::new();
        }
        let Some(in_char) = (*stream).InChar else {
            return Vec::new();
        };
        let mut out = Vec::new();
        loop {
            let ch = in_char(stream);
            if ch < 0 {
                break;
            }
            out.push(ch as u8);
        }
        out
    }
}

// ---------------------------------------------------------------------------
// Stream initializers
// ---------------------------------------------------------------------------

pub unsafe fn R_InitInPStream(
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

pub unsafe fn R_InitOutPStream(
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

pub unsafe fn R_InitFileOutPStream(
    stream: R_outpstream_t,
    fp: *mut c_void,
    type_: R_pstream_format_t,
    version: c_int,
    phook: Option<unsafe extern "C" fn(SEXP, SEXP) -> SEXP>,
    pdata: SEXP,
) {
    unsafe {
        R_InitOutPStream(
            stream,
            fp,
            type_,
            version,
            Some(OutCharFile),
            Some(OutBytesFile),
            phook,
            pdata,
        );
    }
}

pub unsafe fn R_InitFileInPStream(
    stream: R_inpstream_t,
    fp: *mut c_void,
    type_: R_pstream_format_t,
    phook: Option<unsafe extern "C" fn(SEXP, SEXP) -> SEXP>,
    pdata: SEXP,
) {
    unsafe {
        R_InitInPStream(
            stream,
            fp,
            type_,
            Some(InCharFile),
            Some(InBytesFile),
            phook,
            pdata,
        );
    }
}

pub unsafe fn R_InitConnOutPStream(
    stream: R_outpstream_t,
    con: *mut c_void,
    type_: R_pstream_format_t,
    version: c_int,
    phook: Option<unsafe extern "C" fn(SEXP, SEXP) -> SEXP>,
    pdata: SEXP,
) {
    unsafe {
        R_InitOutPStream(
            stream,
            con,
            type_,
            version,
            Some(OutCharFile),
            Some(OutBytesFile),
            phook,
            pdata,
        );
    }
}

pub unsafe fn R_InitConnInPStream(
    stream: R_inpstream_t,
    con: *mut c_void,
    type_: R_pstream_format_t,
    phook: Option<unsafe extern "C" fn(SEXP, SEXP) -> SEXP>,
    pdata: SEXP,
) {
    unsafe {
        R_InitInPStream(
            stream,
            con,
            type_,
            Some(InCharFile),
            Some(InBytesFile),
            phook,
            pdata,
        );
    }
}

// ---------------------------------------------------------------------------
// R_serialize / R_unserialize (R-level entry points, memory-based)
// ---------------------------------------------------------------------------

/// Serialize an R object to a raw vector (when icon is R_NilValue).
/// This is the main entry point for `serialize()` in R.
pub unsafe fn R_serialize(
    object: SEXP,
    icon: SEXP,
    ascii: SEXP,
    Sversion: SEXP,
    fun: SEXP,
) -> SEXP {
    unsafe {
        if object.is_null() {
            error("read error");
        }

        let version = if Sversion == R_NilValue() {
            defaultSerializeVersion()
        } else {
            let version = asInteger(Sversion);
            if version <= 0 {
                error("bad version value");
            }
            version
        };

        // Build the header
        let mut writer = BinaryWriter::new();
        // Format: 'B' + '\n' for binary
        writer.write_byte(b'B');
        writer.write_byte(b'\n');

        // Version info
        writer.write_i32(version); // version
        writer.write_i32(R_VERSION); // writer version
        writer.write_i32(R_VERSION_350); // min reader version
        if version == 3 {
            writer.write_i32(0); // native encoding length
        }

        // Serialize the object
        let mut ref_table = WriteHashTable::new();
        WriteItemInternal(object, &mut ref_table, &mut writer);

        // Create RAWSXP from the serialized bytes
        let data = writer.into_vec();
        let raw = Rf_allocVector3(SEXPTYPE::RAWSXP, data.len() as R_xlen_t);
        if !raw.is_null() && !data.is_empty() {
            let raw_ptr = RAW(raw);
            ptr::copy_nonoverlapping(data.as_ptr(), raw_ptr, data.len());
        }
        raw
    }
}

/// Unserialize an R object from a raw vector.
pub unsafe fn R_unserialize(icon: SEXP, fun: SEXP) -> SEXP {
    unsafe {
        if icon.is_null() {
            error("read error");
        }

        // Must be RAWSXP
        let stype = TYPEOF(icon);
        if stype != SEXPTYPE::RAWSXP {
            error("not a proper raw vector");
        }

        let len = XLENGTH(icon) as usize;
        if len == 0 {
            error("read error");
        }

        let raw_ptr = RAW(icon);
        let data = slice::from_raw_parts(raw_ptr, len);

        let mut reader = BinaryReader::new(data);

        // Read format header: 2 bytes ('B' + '\n')
        let fmt1 = reader.read_byte().unwrap_or(0);
        let fmt2 = reader.read_byte().unwrap_or(0);
        if fmt1 != b'B' || fmt2 != b'\n' {
            error("unknown input format");
        }

        // Read version
        let version = reader.read_i32().unwrap_or(0);
        if version != 2 && version != 3 {
            error("version not supported");
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
            Err(_) => error("read error"),
        }
    }
}

// ---------------------------------------------------------------------------
// R_serializeb
// ---------------------------------------------------------------------------

unsafe fn R_serializeb(object: SEXP, icon: SEXP, xdr: SEXP, Sversion: SEXP, fun: SEXP) -> SEXP {
    unsafe { R_serialize(object, icon, xdr, Sversion, fun) }
}

// ---------------------------------------------------------------------------
// do_serialize (dispatch for serialize/unserialize builtins)
// ---------------------------------------------------------------------------

pub unsafe fn do_serialize(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        let _ = (call, op, env);
        if args.is_null() || args == R_NilValue() {
            error("wrong number of arguments");
        }

        // serialize(object, connection, ascii)
        // object = CAR(args), connection = CADR(args), ascii = CADDR(args)
        let object = CAR(args);
        let _conn = CADR(args);
        let ascii = CADDR(args);

        // Use R_serialize to get a RAWSXP
        let _ascii_flag =
            if !ascii.is_null() && TYPEOF(ascii) == SEXPTYPE::LGLSXP && LENGTH(ascii) >= 1 {
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
            error("wrong number of arguments");
        }

        // serializeToConn(object, connection, ascii)
        let object = CAR(args);
        let _conn = CADR(args);

        // Serialize to a raw vector, then the connection layer would write it
        //  do the serialization
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

pub unsafe fn do_unserializeFromConn(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        let _ = (call, op, env);
        if args.is_null() || args == R_NilValue() {
            error("wrong number of arguments");
        }

        // unserializeFromConn(connection, hook)
        let conn = CAR(args);
        // If the first argument is a raw vector, use R_unserialize directly
        if !conn.is_null() && TYPEOF(conn) == SEXPTYPE::RAWSXP {
            return R_unserialize(conn, R_NilValue());
        }

        error("connection not open for reading");
    }
}

// ---------------------------------------------------------------------------
// Lazy-load database functions
// ---------------------------------------------------------------------------

pub unsafe fn do_lazyLoadDBflush(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        let _ = (call, op, env);
        let file = require_arg(args, 0);
        let _ = sexp_to_path(file);
        clear_lazy_load_cache();
        R_NilValue()
    }
}

pub unsafe fn do_lazyLoadDBfetch(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        let _ = (call, op, env);
        let key = require_arg(args, 0);
        let file = require_arg(args, 1);
        let compsxp = require_arg(args, 2);
        let hook = require_arg(args, 3);
        let compressed = asInteger(compsxp);

        let mut err: Rboolean = 0;
        let mut raw = readRawFromFile(file, key);
        let mut raw_guard = protect(raw);
        if compressed == 3 {
            let next = R_decompress3(raw, &mut err);
            raw = next;
            raw_guard = protect(raw);
        } else if compressed == 2 {
            let next = R_decompress2(raw, &mut err);
            raw = next;
            raw_guard = protect(raw);
        } else if compressed != 0 {
            let next = R_decompress1(raw, &mut err);
            raw = next;
            raw_guard = protect(raw);
        }

        if err != 0 {
            let file_name = sexp_to_path(file);
            error(&format!(
                "lazy-load database '{}' is corrupt",
                file_name.display()
            ));
        }

        let mut val = R_unserialize(raw, hook);
        let mut val_guard = protect(val);
        if TYPEOF(val) == SEXPTYPE::PROMSXP {
            val = Rf_eval(val, R_GlobalEnv());
            val_guard = protect(val);
            if !val.is_null() {
                SET_NAMED(val, 2);
            }
        }
        let _ = (raw_guard, val_guard);
        val
    }
}

pub unsafe fn do_getVarsFromFrame(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        let _ = (call, op, env);
        let vars = require_arg(args, 0);
        let env = require_arg(args, 1);
        let forcesxp = require_arg(args, 2);
        R_getVarsFromFrame(vars, env, forcesxp)
    }
}

pub unsafe fn do_lazyLoadDBinsertValue(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        let _ = (call, op, env);
        let value = require_arg(args, 0);
        let file = require_arg(args, 1);
        let ascii = require_arg(args, 2);
        let compsxp = require_arg(args, 3);
        let hook = require_arg(args, 4);
        R_lazyLoadDBinsertValue(value, file, ascii, compsxp, hook)
    }
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
            _ => {} // intentionally unhandled: unknown serialization format
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
        let mut buf = [0u8; 2];
        if let Some(in_bytes) = (*stream).InBytes {
            in_bytes(stream, buf.as_mut_ptr() as *mut c_void, 2);
        } else if let Some(in_char) = (*stream).InChar {
            for b in &mut buf {
                let ch = in_char(stream);
                if ch < 0 {
                    error("read error");
                }
                *b = ch as u8;
            }
        } else {
            error("read error");
        }

        let mut need_third = false;
        let detected = match buf[0] {
            b'A' => R_pstream_format_t::R_pstream_ascii_format,
            b'B' => R_pstream_format_t::R_pstream_binary_format,
            b'X' => R_pstream_format_t::R_pstream_xdr_format,
            b'\n' if buf[1] == b'A' => {
                need_third = true;
                R_pstream_format_t::R_pstream_ascii_format
            }
            _ => error("unknown input format"),
        };

        // Keep the stream position consistent with the C newline hack.
        if need_third {
            let mut one = 0u8;
            if let Some(in_bytes) = (*stream).InBytes {
                in_bytes(stream, &mut one as *mut u8 as *mut c_void, 1);
            } else if let Some(in_char) = (*stream).InChar {
                if in_char(stream) < 0 {
                    error("read error");
                }
            } else {
                error("read error");
            }
        }

        if (*stream).type_ == R_pstream_format_t::R_pstream_any_format {
            (*stream).type_ = detected;
        } else if (*stream).type_ != detected {
            error("input format does not match specified format");
        }
    }
}

// ---------------------------------------------------------------------------
// Hash table for write reference tracking (C SEXP-based, for C API)
// ---------------------------------------------------------------------------

unsafe fn MakeHashTable() -> SEXP {
    unsafe {
        let vec = Rf_allocVector3(SEXPTYPE::VECSXP, HASHSIZE as R_xlen_t);
        Rf_cons(Rf_ScalarInteger(0), vec)
    }
}

unsafe fn HashAdd(obj: SEXP, ht: SEXP) {
    unsafe {
        if ht.is_null() || TYPEOF(ht) != SEXPTYPE::LISTSXP {
            return;
        }
        let buckets = CDR(ht);
        if buckets.is_null() || TYPEOF(buckets) != SEXPTYPE::VECSXP {
            return;
        }
        let bucket_count = XLENGTH(buckets) as usize;
        if bucket_count == 0 {
            return;
        }
        let key = obj as usize;
        let pos = ((key >> 2) % bucket_count) as R_xlen_t;
        let current_count = asInteger(CAR(ht));
        let next_count = current_count.saturating_add(1);
        SETCAR(ht, Rf_ScalarInteger(next_count));

        let entry = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
        SET_VECTOR_ELT(entry, 0, obj);
        SET_VECTOR_ELT(entry, 1, Rf_ScalarInteger(next_count));
        let bucket = VECTOR_ELT(buckets, pos);
        let new_bucket = Rf_cons(entry, bucket);
        SET_VECTOR_ELT(buckets, pos, new_bucket);
    }
}

unsafe fn HashGet(item: SEXP, ht: SEXP) -> c_int {
    unsafe {
        if ht.is_null() || TYPEOF(ht) != SEXPTYPE::LISTSXP {
            return 0;
        }
        let buckets = CDR(ht);
        if buckets.is_null() || TYPEOF(buckets) != SEXPTYPE::VECSXP {
            return 0;
        }
        let bucket_count = XLENGTH(buckets) as usize;
        if bucket_count == 0 {
            return 0;
        }
        let key = item as usize;
        let pos = ((key >> 2) % bucket_count) as R_xlen_t;
        let mut node = VECTOR_ELT(buckets, pos);
        while !node.is_null() && TYPEOF(node) == SEXPTYPE::LISTSXP {
            let entry = CAR(node);
            if !entry.is_null() && TYPEOF(entry) == SEXPTYPE::VECSXP && XLENGTH(entry) >= 2 {
                let stored = VECTOR_ELT(entry, 0);
                if stored == item {
                    return asInteger(VECTOR_ELT(entry, 1));
                }
            }
            node = CDR(node);
        }
        0
    }
}

// ---------------------------------------------------------------------------
// Persistent name / special hooks
// ---------------------------------------------------------------------------

unsafe fn GetPersistentName(stream: R_outpstream_t, s: SEXP) -> SEXP {
    unsafe {
        if stream.is_null() {
            error("read error");
        }
        let Some(hook) = (*stream).OutPersistHookFunc else {
            return R_NilValue();
        };
        let res = hook(s, (*stream).OutPersistHookData);
        if res.is_null() || res == R_NilValue() {
            return R_NilValue();
        }
        if TYPEOF(res) != SEXPTYPE::STRSXP {
            error("persistent hook must return a character vector");
        }
        res
    }
}

unsafe fn PersistentRestore(stream: R_inpstream_t, s: SEXP) -> SEXP {
    unsafe {
        if stream.is_null() {
            error("read error");
        }
        let Some(hook) = (*stream).InPersistHookFunc else {
            error("read error");
        };
        hook(s, (*stream).InPersistHookData)
    }
}

unsafe fn SaveSpecialHookItem(item: SEXP) -> c_int {
    unsafe { SaveSpecialHook(item) }
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
    unsafe {
        if InInteger(stream) != 0 {
            error("names in persistent strings are not supported yet");
        }
        let len = InInteger(stream);
        if len < 0 {
            error("read error");
        }
        let s = Rf_allocVector3(SEXPTYPE::STRSXP, len as R_xlen_t);
        let _s_guard = protect(s);
        R_ReadItemDepth.with(|d| d.set(d.get() + 1));

        let local_ref_table = if ref_table.is_null() {
            MakeReadRefTable()
        } else {
            ref_table
        };

        for i in 0..len {
            let flags = InInteger(stream);
            let mut stype = 0;
            UnpackFlags(
                flags,
                &mut stype,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            );

            let elt = if stype == REFSXP {
                let idx = if (flags >> 8) == 0 {
                    InInteger(stream)
                } else {
                    flags >> 8
                };
                GetReadRef(local_ref_table, idx)
            } else {
                let charsxp_type: c_int = SEXPTYPE::CHARSXP.into();
                if stype != charsxp_type {
                    error("read error");
                }
                let slen = InInteger(stream);
                let val = if slen < 0 {
                    R_NaString()
                } else if slen == 0 {
                    Rf_mkCharLen(c"".as_ptr(), 0)
                } else {
                    let mut bytes = vec![0u8; slen as usize];
                    InString(stream, bytes.as_mut_ptr() as *mut c_char, slen);
                    Rf_mkCharLen(bytes.as_ptr() as *const c_char, slen)
                };
                AddReadRef(local_ref_table, val);
                val
            };
            SET_STRING_ELT(s, i as R_xlen_t, elt);
        }

        R_ReadItemDepth.with(|d| d.set(d.get().saturating_sub(1)));
        s
    }
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
// Bytecode serialization helpers
// ---------------------------------------------------------------------------

unsafe fn WriteBC(s: SEXP, ref_table: SEXP, stream: R_outpstream_t) {
    unsafe {
        let _ = ref_table;
        if stream.is_null() {
            return;
        }
        let raw = R_serialize(s, R_NilValue(), R_NilValue(), R_NilValue(), R_NilValue());
        let len = XLENGTH(raw);
        if len < 0 || len > c_int::MAX as R_xlen_t {
            error("write failed");
        }
        OutInteger(stream, len as c_int);
        if len > 0 {
            if let Some(out_bytes) = (*stream).OutBytes {
                out_bytes(stream, RAW(raw) as *const c_void, len as c_int);
            } else if let Some(out_char) = (*stream).OutChar {
                for i in 0..len as usize {
                    out_char(stream, *RAW(raw).add(i) as c_int);
                }
            } else {
                error("write failed");
            }
        }
    }
}

unsafe fn ReadBC(ref_table: SEXP, stream: R_inpstream_t) -> SEXP {
    unsafe {
        let _ = ref_table;
        if stream.is_null() {
            error("read error");
        }
        let len = InInteger(stream);
        if len < 0 {
            error("read error");
        }
        let raw = Rf_allocVector3(SEXPTYPE::RAWSXP, len as R_xlen_t);
        if len > 0 {
            if let Some(in_bytes) = (*stream).InBytes {
                in_bytes(stream, RAW(raw) as *mut c_void, len);
            } else if let Some(in_char) = (*stream).InChar {
                for i in 0..len as usize {
                    let ch = in_char(stream);
                    if ch < 0 {
                        error("read error");
                    }
                    *RAW(raw).add(i) = ch as Rbyte;
                }
            } else {
                error("read error");
            }
        }
        R_unserialize(raw, R_NilValue())
    }
}

// ---------------------------------------------------------------------------
// Character conversion and reading
// ---------------------------------------------------------------------------

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
        let data = Rf_allocVector3(SEXPTYPE::VECSXP, INITIAL_REFREAD_TABLE_SIZE as R_xlen_t);
        Rf_cons(data, Rf_ScalarInteger(0))
    }
}

unsafe fn GetReadRef(table: SEXP, index: c_int) -> SEXP {
    unsafe {
        if table.is_null() || TYPEOF(table) != SEXPTYPE::LISTSXP || index <= 0 {
            error("reference index out of range");
        }
        let data = CAR(table);
        if data.is_null() || TYPEOF(data) != SEXPTYPE::VECSXP {
            error("reference index out of range");
        }
        let used = asInteger(CDR(table));
        if index > used {
            error("reference index out of range");
        }
        VECTOR_ELT(data, (index - 1) as R_xlen_t)
    }
}

unsafe fn AddReadRef(table: SEXP, value: SEXP) {
    unsafe {
        if table.is_null() || TYPEOF(table) != SEXPTYPE::LISTSXP {
            return;
        }
        let mut data = CAR(table);
        if data.is_null() || TYPEOF(data) != SEXPTYPE::VECSXP {
            return;
        }
        let mut used = asInteger(CDR(table));
        if used < 0 {
            used = 0;
        }

        if (used as R_xlen_t) >= XLENGTH(data) {
            let old_len = XLENGTH(data);
            let new_len = std::cmp::max(1, old_len * 2);
            let grown = Rf_allocVector3(SEXPTYPE::VECSXP, new_len);
            for i in 0..old_len {
                SET_VECTOR_ELT(grown, i, VECTOR_ELT(data, i));
            }
            SETCAR(table, grown);
            data = grown;
        }

        SET_VECTOR_ELT(data, used as R_xlen_t, value);
        SETCDR(table, Rf_ScalarInteger(used.saturating_add(1)));
    }
}

// ---------------------------------------------------------------------------
// R_InitSerializeRoutines
// ---------------------------------------------------------------------------

pub unsafe fn R_InitSerializeRoutines() {
    // all routines are statically available.
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
            error("cannot allocate buffer");
        }
        let mb = (*stream).data as *mut membuf_st;
        if mb.is_null() {
            error("cannot allocate buffer");
        }
        let count = (*mb).count;
        let val = Rf_allocVector3(SEXPTYPE::RAWSXP, count as R_xlen_t);
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
// Buffer-connection operations
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
    unsafe {
        if !bbs.is_null() {
            R_InitOutPStream(
                stream,
                bbs as R_pstream_data_t,
                type_,
                version,
                Some(OutCharMem),
                Some(OutBytesMem),
                phook,
                pdata,
            );
        } else {
            R_InitOutPStream(
                stream,
                con as R_pstream_data_t,
                type_,
                version,
                Some(OutCharFile),
                Some(OutBytesFile),
                phook,
                pdata,
            );
        }
    }
}

unsafe fn flush_bcon_buffer(bbs: *mut c_void) {
    unsafe {
        if !bbs.is_null() {
            let fp = bbs as *mut libc::FILE;
            libc::fflush(fp);
        }
    }
}

// ---------------------------------------------------------------------------
// File I/O callbacks
// ---------------------------------------------------------------------------

unsafe extern "C" fn OutCharFile(stream: R_outpstream_t, c: c_int) {
    unsafe {
        if stream.is_null() {
            return;
        }
        let fp = (*stream).data as *mut libc::FILE;
        if fp.is_null() {
            return;
        }
        libc::fputc(c, fp);
    }
}

unsafe extern "C" fn OutBytesFile(stream: R_outpstream_t, buf: *const c_void, length: c_int) {
    unsafe {
        if stream.is_null() || buf.is_null() || length <= 0 {
            return;
        }
        let fp = (*stream).data as *mut libc::FILE;
        if fp.is_null() {
            error("write failed");
        }
        let wrote = libc::fwrite(buf, 1, length as usize, fp);
        if wrote != length as usize {
            error("write failed");
        }
    }
}

unsafe extern "C" fn InCharFile(stream: R_inpstream_t) -> c_int {
    unsafe {
        if stream.is_null() {
            return -1;
        }
        let fp = (*stream).data as *mut libc::FILE;
        if fp.is_null() {
            return -1;
        }
        libc::fgetc(fp)
    }
}

unsafe extern "C" fn InBytesFile(stream: R_inpstream_t, buf: *mut c_void, length: c_int) {
    unsafe {
        if stream.is_null() || buf.is_null() || length <= 0 {
            return;
        }
        let fp = (*stream).data as *mut libc::FILE;
        if fp.is_null() {
            error("read error");
        }
        let read_n = libc::fread(buf, 1, length as usize, fp);
        if read_n != length as usize {
            error("read error");
        }
    }
}

unsafe fn InInit(stream: R_inpstream_t, buf: *mut c_void, length: c_int) {
    unsafe {
        if !stream.is_null() && !buf.is_null() && length > 0 {
            InBytesFile(stream, buf, length);
        }
    }
}

// ---------------------------------------------------------------------------
// Connection I/O
// ---------------------------------------------------------------------------

pub unsafe fn R_WriteConnection(con: *mut c_void, buf: *const c_void, n: usize) -> usize {
    unsafe {
        if con.is_null() || buf.is_null() || n == 0 {
            return 0;
        }
        let fp = con as *mut libc::FILE;
        if fp.is_null() {
            return 0;
        }
        libc::fwrite(buf, 1, n, fp)
    }
}

// ---------------------------------------------------------------------------
// Lazy-load helpers (module-private)
// ---------------------------------------------------------------------------

unsafe fn CallHook(x: SEXP, fun: SEXP) -> SEXP {
    unsafe {
        let call = Rf_cons(fun, Rf_cons(x, R_NilValue()));
        Rf_eval(call, R_GlobalEnv())
    }
}

unsafe fn checkNotPromise(val: SEXP) -> SEXP {
    unsafe {
        if TYPEOF(val) == SEXPTYPE::PROMSXP {
            error("cannot return a promise (PROMSXP) object");
        }
        val
    }
}

unsafe fn appendRawToFile(file: SEXP, bytes: SEXP) -> SEXP {
    unsafe {
        let path = sexp_to_path(file);
        if TYPEOF(bytes) != SEXPTYPE::RAWSXP {
            error("not a proper raw vector");
        }
        let len = XLENGTH(bytes) as usize;
        if len > i32::MAX as usize {
            error("write failed");
        }
        let mut fp = match OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&path)
        {
            Ok(file) => file,
            Err(err) => error(&format!("cannot open file '{}': {}", path.display(), err)),
        };
        let pos = match fp.seek(SeekFrom::End(0)) {
            Ok(pos) => pos,
            Err(_) => error("write failed"),
        };
        let data = slice::from_raw_parts(RAW(bytes), len);
        if fp.write_all(data).is_err() || fp.flush().is_err() {
            error("write failed");
        }
        if pos > i32::MAX as u64 {
            error("write failed");
        }
        let key = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
        if key.is_null() {
            error("write failed");
        }
        *INTEGER(key) = pos as c_int;
        *INTEGER(key).add(1) = len as c_int;
        key
    }
}

unsafe fn readRawFromFile(file: SEXP, key: SEXP) -> SEXP {
    unsafe {
        let path = sexp_to_path(file);
        if TYPEOF(key) != SEXPTYPE::INTSXP || LENGTH(key) != 2 {
            error("bad offset/length argument");
        }
        let offset = *INTEGER(key);
        let len = *INTEGER(key).add(1);
        if offset < 0 || len < 0 {
            error("bad offset/length argument");
        }

        let mut fp = match File::open(&path) {
            Ok(file) => file,
            Err(err) => error(&format!("cannot open file '{}': {}", path.display(), err)),
        };
        let filelen = match fp.seek(SeekFrom::End(0)) {
            Ok(pos) => pos,
            Err(_) => error("read failed"),
        };
        let offset_u64 = offset as u64;
        let len_u64 = len as u64;
        if offset_u64 > filelen || len_u64 > filelen.saturating_sub(offset_u64) {
            error("read failed");
        }
        if fp.seek(SeekFrom::Start(offset_u64)).is_err() {
            error("read failed");
        }
        let mut buf = vec![0u8; len as usize];
        if len > 0 && fp.read_exact(&mut buf).is_err() {
            error("read failed");
        }
        raw_from_bytes(&buf)
    }
}

unsafe fn R_lazyLoadDBinsertValue(
    value: SEXP,
    file: SEXP,
    ascii: SEXP,
    compsxp: SEXP,
    hook: SEXP,
) -> SEXP {
    unsafe {
        let mut data = R_serialize(value, R_NilValue(), ascii, R_NilValue(), hook);
        let compress = asInteger(compsxp);
        if compress == 3 {
            data = R_compress3(data);
        } else if compress == 2 {
            data = R_compress2(data);
        } else if compress != 0 {
            data = R_compress1(data);
        }
        appendRawToFile(file, data)
    }
}

unsafe fn R_getVarsFromFrame(vars: SEXP, env: SEXP, forcesxp: SEXP) -> SEXP {
    unsafe {
        if TYPEOF(env) == SEXPTYPE::NILSXP {
            error("use of NULL environment is defunct");
        }
        if TYPEOF(env) != SEXPTYPE::ENVSXP {
            error("bad environment");
        }
        if TYPEOF(vars) != SEXPTYPE::STRSXP {
            error("bad variable names");
        }

        let force = asLogical(forcesxp);
        let len = LENGTH(vars);
        let val = Rf_allocVector3(SEXPTYPE::VECSXP, len as R_xlen_t);
        let _val_guard = protect(val);
        for i in 0..len {
            let name = STRING_ELT(vars, i as R_xlen_t);
            if name.is_null() {
                error("bad variable names");
            }
            let sym = Rf_install(CHAR(name));
            let mut tmp = R_findVarInFrame(env, sym);
            if tmp == R_UnboundValue() {
                error(&format!(
                    "object '{}' not found",
                    CStr::from_ptr(CHAR(name)).to_string_lossy()
                ));
            }
            if force != 0 && TYPEOF(tmp) == SEXPTYPE::PROMSXP {
                tmp = Rf_eval(tmp, R_GlobalEnv());
                if !tmp.is_null() {
                    SET_NAMED(tmp, 2);
                }
            }
            SET_VECTOR_ELT(val, i as R_xlen_t, tmp);
        }
        setAttrib(val, R_NamesSymbol(), vars);
        val
    }
}

// ---------------------------------------------------------------------------
// Compression functions
// ---------------------------------------------------------------------------

pub unsafe fn R_compress1(inp: SEXP) -> SEXP {
    unsafe {
        if TYPEOF(inp) != SEXPTYPE::RAWSXP {
            error("R_compress1 requires a raw vector");
        }
        let inlen = XLENGTH(inp) as usize;
        if inlen > u32::MAX as usize {
            error("raw vector too large to compress");
        }
        let input = slice::from_raw_parts(RAW(inp), inlen);
        let payload = match zlib_compress(input) {
            Ok(data) => data,
            Err(err) => error(&format!("internal error in R_compress1: {err}")),
        };
        raw_from_bytes(&build_compressed_blob(inlen, None, &payload))
    }
}

pub unsafe fn R_decompress1(inp: SEXP, err: *mut Rboolean) -> SEXP {
    unsafe {
        if TYPEOF(inp) != SEXPTYPE::RAWSXP {
            error("R_decompress1 requires a raw vector");
        }
        let inlen = XLENGTH(inp) as usize;
        if inlen < 4 {
            mark_decompress_error(err);
            return R_NilValue();
        }
        let data = slice::from_raw_parts(RAW(inp), inlen);
        let outlen = match parse_swapped_len_prefix(data) {
            Some(v) => v,
            None => {
                mark_decompress_error(err);
                return R_NilValue();
            }
        };
        match zlib_decompress_exact(&data[4..], outlen) {
            Ok(decoded) => raw_from_bytes(&decoded),
            Err(_) => {
                mark_decompress_error(err);
                R_NilValue()
            }
        }
    }
}

pub unsafe fn R_compress2(inp: SEXP) -> SEXP {
    unsafe {
        if TYPEOF(inp) != SEXPTYPE::RAWSXP {
            error("R_compress2 requires a raw vector");
        }
        let inlen = XLENGTH(inp) as usize;
        if inlen > u32::MAX as usize {
            error("raw vector too large to compress");
        }
        let input = slice::from_raw_parts(RAW(inp), inlen);
        let (marker, payload) = match bzip2_compress(input) {
            Ok(compressed) if compressed.len() <= inlen => (b'2', compressed),
            Ok(_) => (b'0', input.to_vec()),
            Err(err) => error(&format!("internal error in R_compress2: {err}")),
        };
        raw_from_bytes(&build_compressed_blob(inlen, Some(marker), &payload))
    }
}

pub unsafe fn R_decompress2(inp: SEXP, err: *mut Rboolean) -> SEXP {
    unsafe {
        if TYPEOF(inp) != SEXPTYPE::RAWSXP {
            error("R_decompress2 requires a raw vector");
        }
        let inlen = XLENGTH(inp) as usize;
        if inlen < 5 {
            mark_decompress_error(err);
            return R_NilValue();
        }
        let data = slice::from_raw_parts(RAW(inp), inlen);
        let outlen = match parse_swapped_len_prefix(data) {
            Some(v) => v,
            None => {
                mark_decompress_error(err);
                return R_NilValue();
            }
        };
        let decoded = match data[4] {
            b'2' => bzip2_decompress_exact(&data[5..], outlen),
            b'1' => zlib_decompress_exact(&data[5..], outlen),
            b'0' => {
                if data.len() < 5 + outlen {
                    mark_decompress_error(err);
                    return R_NilValue();
                }
                Ok(data[5..5 + outlen].to_vec())
            }
            _ => {
                mark_decompress_error(err);
                return R_NilValue();
            }
        };
        match decoded {
            Ok(out) => raw_from_bytes(&out),
            Err(_) => {
                mark_decompress_error(err);
                R_NilValue()
            }
        }
    }
}

pub unsafe fn R_compress3(inp: SEXP) -> SEXP {
    unsafe {
        if TYPEOF(inp) != SEXPTYPE::RAWSXP {
            error("R_compress3 requires a raw vector");
        }
        let inlen = XLENGTH(inp) as usize;
        if inlen > u32::MAX as usize {
            error("raw vector too large to compress");
        }
        let input = slice::from_raw_parts(RAW(inp), inlen);
        let (marker, payload) = match lzma2_raw_encode(input, inlen + 5) {
            Ok(compressed) => (b'Z', compressed),
            Err(_) => (b'0', input.to_vec()),
        };
        raw_from_bytes(&build_compressed_blob(inlen, Some(marker), &payload))
    }
}

pub unsafe fn R_decompress3(inp: SEXP, err: *mut Rboolean) -> SEXP {
    unsafe {
        if TYPEOF(inp) != SEXPTYPE::RAWSXP {
            error("R_decompress3 requires a raw vector");
        }
        let inlen = XLENGTH(inp) as usize;
        if inlen < 5 {
            mark_decompress_error(err);
            return R_NilValue();
        }
        let data = slice::from_raw_parts(RAW(inp), inlen);
        let outlen = match parse_swapped_len_prefix(data) {
            Some(v) => v,
            None => {
                mark_decompress_error(err);
                return R_NilValue();
            }
        };
        let decoded = match data[4] {
            b'Z' => lzma2_raw_decode(&data[5..], outlen).map_err(|_| ()),
            b'2' => bzip2_decompress_exact(&data[5..], outlen).map_err(|_| ()),
            b'1' => zlib_decompress_exact(&data[5..], outlen).map_err(|_| ()),
            b'0' => {
                if data.len() < 5 + outlen {
                    mark_decompress_error(err);
                    return R_NilValue();
                }
                Ok(data[5..5 + outlen].to_vec())
            }
            _ => {
                mark_decompress_error(err);
                return R_NilValue();
            }
        };
        match decoded {
            Ok(out) => raw_from_bytes(&out),
            Err(_) => {
                mark_decompress_error(err);
                R_NilValue()
            }
        }
    }
}

// ---------------------------------------------------------------------------
// R_snprintf utility
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
pub(crate) struct R_instring_stream_st {
    last: c_int,
    stream: R_inpstream_t,
}

pub(crate) type R_instring_stream_t = *mut R_instring_stream_st;

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
    use crate::sexp::envir::R_NewHashedEnv;
    use crate::sexp::envir::defineVar;
    use crate::sexp::globals::{
        R_BaseEnv, R_EmptyEnv, R_GlobalEnv, R_MissingArg, R_NaString, R_UnboundValue,
    };
    use std::ffi::CString;
    use std::mem;
    use std::os::raw::c_void;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn must<T, E: std::fmt::Debug>(r: Result<T, E>) -> T {
        match r {
            Ok(v) => v,
            Err(e) => panic!("test failed: {e:?}"),
        }
    }

    fn make_raw(bytes: &[u8]) -> SEXP {
        let raw = unsafe { Rf_allocVector3(SEXPTYPE::RAWSXP, bytes.len() as R_xlen_t) };
        if !raw.is_null() && !bytes.is_empty() {
            unsafe { ptr::copy_nonoverlapping(bytes.as_ptr(), RAW(raw), bytes.len()) };
        }
        raw
    }

    fn make_string_scalar(value: &str) -> SEXP {
        let c_value = CString::new(value).unwrap_or_default();
        unsafe { Rf_mkString(c_value.as_ptr()) }
    }

    fn make_string_vector(value: &str) -> SEXP {
        let vec = unsafe { Rf_allocVector3(SEXPTYPE::STRSXP, 1) };
        unsafe {
            SET_STRING_ELT(
                vec,
                0,
                Rf_mkChar(CString::new(value).unwrap_or_default().as_ptr()),
            );
        }
        vec
    }

    fn make_na_string_vector() -> SEXP {
        let vec = unsafe { Rf_allocVector3(SEXPTYPE::STRSXP, 1) };
        unsafe { SET_STRING_ELT(vec, 0, R_NaString()) };
        vec
    }

    fn make_args(items: &[SEXP]) -> SEXP {
        let mut tail = unsafe { R_NilValue() };
        for item in items.iter().rev().copied() {
            tail = unsafe { Rf_cons(item, tail) };
        }
        tail
    }

    fn temp_path(stem: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default();
        path.push(format!(
            "rport-serialize-{stem}-{}-{nanos}.bin",
            std::process::id()
        ));
        path
    }

    #[test]
    fn test_default_serialize_version() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let v = defaultSerializeVersion();
            assert_eq!(v, 3);
        }
    }

    #[test]
    fn test_binary_writer_reader_i32() {
        let _session = crate::sexp::session::RSession::new();
        let mut writer = BinaryWriter::new();
        writer.write_i32(42);
        writer.write_i32(-1);
        writer.write_i32(i32::MAX);
        let data = writer.into_vec();
        assert_eq!(data.len(), 12);

        let mut reader = BinaryReader::new(&data);
        assert_eq!(must(reader.read_i32()), 42);
        assert_eq!(must(reader.read_i32()), -1);
        assert_eq!(must(reader.read_i32()), i32::MAX);
    }

    #[test]
    fn test_binary_writer_reader_f64() {
        let _session = crate::sexp::session::RSession::new();
        let mut writer = BinaryWriter::new();
        writer.write_f64(3.14);
        writer.write_f64(-1.0);
        writer.write_f64(0.0);
        let data = writer.into_vec();

        let mut reader = BinaryReader::new(&data);
        assert!((must(reader.read_f64()) - 3.14).abs() < 1e-10);
        assert_eq!(must(reader.read_f64()), -1.0);
        assert_eq!(must(reader.read_f64()), 0.0);
    }

    #[test]
    fn test_binary_writer_reader_bytes() {
        let _session = crate::sexp::session::RSession::new();
        let mut writer = BinaryWriter::new();
        writer.write_byte(0xFF);
        writer.write_byte(0x00);
        writer.write_byte(0x42);
        let data = writer.into_vec();

        let mut reader = BinaryReader::new(&data);
        assert_eq!(must(reader.read_byte()), 0xFF);
        assert_eq!(must(reader.read_byte()), 0x00);
        assert_eq!(must(reader.read_byte()), 0x42);
    }

    #[test]
    fn test_pack_flags() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            // Type only, no flags
            let flags = PackFlags(13, 0, 0, 0, 0);
            assert_eq!(flags, 13);

            // Type with object flag
            let flags = PackFlags(14, 0, 1, 0, 0);
            assert_eq!(flags, 0b1110 | IS_OBJECT_BIT_MASK);

            // Type with attr and tag flags
            let flags = PackFlags(19, 0, 0, 1, 1);
            assert_eq!(flags, 0b10011 | HAS_ATTR_BIT_MASK | HAS_TAG_BIT_MASK);

            // Type with levels
            let flags = PackFlags(10, 3, 0, 0, 0);
            assert_eq!(flags, 0b1010 | (3 << 12));
        }
    }

    #[test]
    fn test_unpack_flags() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let mut ptype = 0i32;
            let mut plevs = 0i32;
            let mut pisobj = 0i32;
            let mut phasattr = 0i32;
            let mut phastag = 0i32;

            let flags = 0b10011 | HAS_ATTR_BIT_MASK | HAS_TAG_BIT_MASK | (2 << 12);
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
        let _session = crate::sexp::session::RSession::new();
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
        let _session = crate::sexp::session::RSession::new();
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
        let _session = crate::sexp::session::RSession::new();
        let mut rt = ReadRefTable::new();
        let fake1 = 0x1000 as *mut std::os::raw::c_void as SEXP;
        let fake2 = 0x2000 as *mut std::os::raw::c_void as SEXP;

        rt.add(fake1);
        rt.add(fake2);
        assert_eq!(must(rt.get(1)), fake1);
        assert_eq!(must(rt.get(2)), fake2);
        assert!(rt.get(3).is_err());
        assert!(rt.get(0).is_err());
    }

    #[test]
    fn test_c_read_ref_table_helpers() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let table = MakeReadRefTable();
            let a = Rf_ScalarInteger(11);
            let b = Rf_ScalarReal(2.5);
            AddReadRef(table, a);
            AddReadRef(table, b);
            assert_eq!(GetReadRef(table, 1), a);
            assert_eq!(GetReadRef(table, 2), b);
        }
    }

    #[test]
    fn test_c_hash_table_helpers() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let ht = MakeHashTable();
            let a = Rf_ScalarInteger(1);
            let b = Rf_ScalarInteger(2);
            assert_eq!(HashGet(a, ht), 0);
            HashAdd(a, ht);
            HashAdd(b, ht);
            assert!(HashGet(a, ht) > 0);
            assert!(HashGet(b, ht) > 0);
        }
    }

    #[test]
    fn test_writebc_readbc_round_trip() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let mut out_stream: R_outpstream_st = mem::zeroed();
            let mut out_buf = membuf_st {
                size: 0,
                count: 0,
                buf: ptr::null_mut(),
            };
            InitMemOutPStream(
                &mut out_stream,
                &mut out_buf,
                R_pstream_format_t::R_pstream_binary_format,
                3,
                None,
                R_NilValue(),
            );
            let value = Rf_ScalarInteger(77);
            WriteBC(value, ptr::null_mut(), &mut out_stream);
            let raw = CloseMemOutPStream(&mut out_stream);

            let mut in_stream: R_inpstream_st = mem::zeroed();
            let mut in_buf = membuf_st {
                size: 0,
                count: 0,
                buf: ptr::null_mut(),
            };
            InitMemInPStream(
                &mut in_stream,
                &mut in_buf,
                RAW(raw) as *mut c_void,
                XLENGTH(raw) as R_size_t,
                None,
                R_NilValue(),
            );
            let got = ReadBC(ptr::null_mut(), &mut in_stream);
            assert_eq!(TYPEOF(got), SEXPTYPE::INTSXP);
            assert_eq!(LENGTH(got), 1);
            assert_eq!(*INTEGER(got), 77);
        }
    }

    #[test]
    fn test_conn_stream_file_callbacks() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let fp = libc::tmpfile();
            assert!(!fp.is_null());

            let mut out_stream: R_outpstream_st = mem::zeroed();
            R_InitConnOutPStream(
                &mut out_stream,
                fp as *mut c_void,
                R_pstream_format_t::R_pstream_binary_format,
                3,
                None,
                R_NilValue(),
            );
            assert!(out_stream.OutBytes.is_some());
            let bytes = [1u8, 2, 3, 4];
            out_stream.OutBytes.unwrap()(
                &mut out_stream,
                bytes.as_ptr() as *const c_void,
                bytes.len() as c_int,
            );
            libc::fflush(fp);
            libc::rewind(fp);

            let mut in_stream: R_inpstream_st = mem::zeroed();
            R_InitConnInPStream(
                &mut in_stream,
                fp as *mut c_void,
                R_pstream_format_t::R_pstream_binary_format,
                None,
                R_NilValue(),
            );
            assert!(in_stream.InBytes.is_some());
            let mut got = [0u8; 4];
            in_stream.InBytes.unwrap()(
                &mut in_stream,
                got.as_mut_ptr() as *mut c_void,
                got.len() as c_int,
            );
            assert_eq!(got, bytes);

            libc::fclose(fp);
        }
    }

    #[test]
    fn test_r_write_connection_file_round_trip() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let fp = libc::tmpfile();
            assert!(!fp.is_null());
            let bytes = b"hello";
            let wrote = R_WriteConnection(
                fp as *mut c_void,
                bytes.as_ptr() as *const c_void,
                bytes.len(),
            );
            assert_eq!(wrote, bytes.len());
            libc::fflush(fp);
            libc::fseek(fp, 0, libc::SEEK_END);
            let size = libc::ftell(fp);
            assert_eq!(size, bytes.len() as libc::c_long);
            libc::fclose(fp);
        }
    }

    #[test]
    fn test_instringvec_round_trip() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let src = Rf_allocVector3(SEXPTYPE::STRSXP, 3);
            SET_STRING_ELT(src, 0, Rf_mkChar(c"foo".as_ptr()));
            SET_STRING_ELT(src, 1, R_NaString());
            SET_STRING_ELT(src, 2, Rf_mkChar(c"bar".as_ptr()));

            let mut out_stream: R_outpstream_st = mem::zeroed();
            let mut out_buf = membuf_st {
                size: 0,
                count: 0,
                buf: ptr::null_mut(),
            };
            InitMemOutPStream(
                &mut out_stream,
                &mut out_buf,
                R_pstream_format_t::R_pstream_binary_format,
                3,
                None,
                R_NilValue(),
            );
            let write_ref_table = MakeHashTable();
            OutStringVec(&mut out_stream, src, write_ref_table);
            let raw = CloseMemOutPStream(&mut out_stream);

            let mut in_stream: R_inpstream_st = mem::zeroed();
            let mut in_buf = membuf_st {
                size: 0,
                count: 0,
                buf: ptr::null_mut(),
            };
            InitMemInPStream(
                &mut in_stream,
                &mut in_buf,
                RAW(raw) as *mut c_void,
                XLENGTH(raw) as R_size_t,
                None,
                R_NilValue(),
            );
            in_stream.type_ = R_pstream_format_t::R_pstream_binary_format;

            let read_ref_table = MakeReadRefTable();
            let got = InStringVec(&mut in_stream, read_ref_table);
            assert_eq!(TYPEOF(got), SEXPTYPE::STRSXP);
            assert_eq!(LENGTH(got), 3);
            let g0 = std::ffi::CStr::from_ptr(CHAR(STRING_ELT(got, 0)));
            let g2 = std::ffi::CStr::from_ptr(CHAR(STRING_ELT(got, 2)));
            assert_eq!(g0.to_bytes(), b"foo");
            assert_eq!(STRING_ELT(got, 1), R_NaString());
            assert_eq!(g2.to_bytes(), b"bar");
        }
    }

    #[test]
    fn test_ref_index_packing() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let mut writer = BinaryWriter::new();
            // Small index: packed into single integer
            OutRefIndex(&mut writer, 1);
            let data = writer.into_vec();
            let mut reader = BinaryReader::new(&data);
            let flags = must(reader.read_i32());
            assert_eq!(flags & 0xFF, REFSXP);
            assert_eq!(flags >> 8, 1);
        }
    }

    #[test]
    fn test_constants() {
        let _session = crate::sexp::session::RSession::new();
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
    #[should_panic]
    fn test_r_serialize_returns_nil_for_null() {
        let _session = crate::sexp::session::RSession::new();
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
    #[should_panic]
    fn test_r_unserialize_returns_nil_for_null() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let result = R_unserialize(R_NilValue(), R_NilValue());
            assert_eq!(result, R_NilValue());
        }
    }

    #[test]
    #[should_panic]
    fn test_r_unserialize_returns_nil_for_empty_raw() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let raw = Rf_allocVector3(SEXPTYPE::RAWSXP, 0);
            let result = R_unserialize(raw, R_NilValue());
            assert_eq!(result, R_NilValue());
        }
    }

    #[test]
    #[should_panic]
    fn test_r_unserialize_returns_nil_for_non_raw() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let s = Rf_allocVector3(SEXPTYPE::INTSXP, 1);
            let result = R_unserialize(s, R_NilValue());
            assert_eq!(result, R_NilValue());
        }
    }

    #[test]
    #[should_panic]
    fn test_r_serialize_info_returns_nil() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let result = R_SerializeInfo(ptr::null_mut());
            assert_eq!(result, R_NilValue());
        }
    }

    #[test]
    fn test_r_serialize_info_reports_header() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let value = Rf_ScalarInteger(123);
            let raw = R_serialize(
                value,
                R_NilValue(),
                R_NilValue(),
                R_NilValue(),
                R_NilValue(),
            );
            assert_eq!(TYPEOF(raw), SEXPTYPE::RAWSXP);

            let mut in_stream: R_inpstream_st = mem::zeroed();
            let mut in_buf = membuf_st {
                size: 0,
                count: 0,
                buf: ptr::null_mut(),
            };
            InitMemInPStream(
                &mut in_stream,
                &mut in_buf,
                RAW(raw) as *mut c_void,
                XLENGTH(raw) as R_size_t,
                None,
                R_NilValue(),
            );

            let info = R_SerializeInfo(&mut in_stream);
            assert_eq!(TYPEOF(info), SEXPTYPE::VECSXP);
            assert_eq!(LENGTH(info), 5);

            let names = crate::eval::attrib_core::getAttrib(info, R_NamesSymbol());
            assert_eq!(TYPEOF(names), SEXPTYPE::STRSXP);
            assert_eq!(LENGTH(names), 5);
            let n0 = std::ffi::CStr::from_ptr(CHAR(STRING_ELT(names, 0)));
            let n1 = std::ffi::CStr::from_ptr(CHAR(STRING_ELT(names, 1)));
            let n2 = std::ffi::CStr::from_ptr(CHAR(STRING_ELT(names, 2)));
            let n3 = std::ffi::CStr::from_ptr(CHAR(STRING_ELT(names, 3)));
            let n4 = std::ffi::CStr::from_ptr(CHAR(STRING_ELT(names, 4)));
            assert_eq!(n0.to_bytes(), b"version");
            assert_eq!(n1.to_bytes(), b"writer_version");
            assert_eq!(n2.to_bytes(), b"min_reader_version");
            assert_eq!(n3.to_bytes(), b"format");
            assert_eq!(n4.to_bytes(), b"native_encoding");

            let version = VECTOR_ELT(info, 0);
            assert_eq!(TYPEOF(version), SEXPTYPE::INTSXP);
            assert_eq!(*INTEGER(version), 3);

            let writer = VECTOR_ELT(info, 1);
            assert_eq!(TYPEOF(writer), SEXPTYPE::STRSXP);
            let writer_s = std::ffi::CStr::from_ptr(CHAR(STRING_ELT(writer, 0)));
            assert!(writer_s.to_bytes().contains(&b'.'));

            let min_reader = VECTOR_ELT(info, 2);
            assert_eq!(TYPEOF(min_reader), SEXPTYPE::STRSXP);
            let min_reader_s = std::ffi::CStr::from_ptr(CHAR(STRING_ELT(min_reader, 0)));
            assert!(min_reader_s.to_bytes().contains(&b'.'));

            let fmt = VECTOR_ELT(info, 3);
            let fmt_s = std::ffi::CStr::from_ptr(CHAR(STRING_ELT(fmt, 0)));
            assert_eq!(fmt_s.to_bytes(), b"binary");

            let enc = VECTOR_ELT(info, 4);
            let enc_s = std::ffi::CStr::from_ptr(CHAR(STRING_ELT(enc, 0)));
            assert_eq!(enc_s.to_bytes(), b"");
        }
    }

    #[test]
    fn test_r_serialize_honors_requested_version_2() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let value = Rf_ScalarInteger(123);
            let version = Rf_ScalarInteger(2);
            let raw = R_serialize(value, R_NilValue(), R_NilValue(), version, R_NilValue());
            assert_eq!(TYPEOF(raw), SEXPTYPE::RAWSXP);

            let mut in_stream: R_inpstream_st = mem::zeroed();
            let mut in_buf = membuf_st {
                size: 0,
                count: 0,
                buf: ptr::null_mut(),
            };
            InitMemInPStream(
                &mut in_stream,
                &mut in_buf,
                RAW(raw) as *mut c_void,
                XLENGTH(raw) as R_size_t,
                None,
                R_NilValue(),
            );

            let info = R_SerializeInfo(&mut in_stream);
            assert_eq!(TYPEOF(info), SEXPTYPE::VECSXP);
            assert_eq!(LENGTH(info), 4);
            assert_eq!(*INTEGER(VECTOR_ELT(info, 0)), 2);

            let names = crate::eval::attrib_core::getAttrib(info, R_NamesSymbol());
            assert_eq!(TYPEOF(names), SEXPTYPE::STRSXP);
            assert_eq!(LENGTH(names), 4);
            let n0 = std::ffi::CStr::from_ptr(CHAR(STRING_ELT(names, 0)));
            let n1 = std::ffi::CStr::from_ptr(CHAR(STRING_ELT(names, 1)));
            let n2 = std::ffi::CStr::from_ptr(CHAR(STRING_ELT(names, 2)));
            let n3 = std::ffi::CStr::from_ptr(CHAR(STRING_ELT(names, 3)));
            assert_eq!(n0.to_bytes(), b"version");
            assert_eq!(n1.to_bytes(), b"writer_version");
            assert_eq!(n2.to_bytes(), b"min_reader_version");
            assert_eq!(n3.to_bytes(), b"format");
        }
    }

    #[test]
    #[should_panic]
    fn test_do_serialize_returns_nil() {
        let _session = crate::sexp::session::RSession::new();
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
    #[should_panic]
    fn test_do_serialize_to_conn_returns_nil() {
        let _session = crate::sexp::session::RSession::new();
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
    #[should_panic]
    fn test_do_unserialize_from_conn_returns_nil() {
        let _session = crate::sexp::session::RSession::new();
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
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let path = temp_path("flush");
            let path_str = path.to_str().unwrap().to_owned();
            let file = make_string_scalar(&path_str);
            let args = make_args(&[file]);
            let result =
                do_lazyLoadDBflush(ptr::null_mut(), ptr::null_mut(), args, ptr::null_mut());
            assert_eq!(result, R_NilValue());
        }
    }

    #[test]
    fn test_do_lazy_load_db_insert_and_fetch_round_trip() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let path = temp_path("lazyload");
            let path_str = path.to_str().unwrap().to_owned();
            let file = make_string_scalar(&path_str);
            let value = Rf_ScalarInteger(123);
            let insert_args = make_args(&[
                value,
                file,
                Rf_ScalarLogical(0),
                Rf_ScalarInteger(3),
                R_NilValue(),
            ]);

            let key = do_lazyLoadDBinsertValue(
                ptr::null_mut(),
                ptr::null_mut(),
                insert_args,
                ptr::null_mut(),
            );
            assert_eq!(TYPEOF(key), SEXPTYPE::INTSXP);
            assert_eq!(LENGTH(key), 2);

            let fetch_args = make_args(&[
                key,
                make_string_scalar(&path_str),
                Rf_ScalarInteger(3),
                R_NilValue(),
            ]);
            let fetched = do_lazyLoadDBfetch(
                ptr::null_mut(),
                ptr::null_mut(),
                fetch_args,
                ptr::null_mut(),
            );
            assert_eq!(TYPEOF(fetched), SEXPTYPE::INTSXP);
            assert_eq!(LENGTH(fetched), 1);
            assert_eq!(*INTEGER(fetched), 123);
            let _ = std::fs::remove_file(path);
        }
    }

    #[test]
    fn test_do_get_vars_from_frame_returns_values() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let env = R_NewHashedEnv(R_BaseEnv(), 29);
            let sym = Rf_install(c"x".as_ptr());
            let val = Rf_ScalarInteger(42);
            defineVar(sym, val, env);

            let vars = make_string_vector("x");
            let args = make_args(&[vars, env, Rf_ScalarLogical(0)]);
            let result =
                do_getVarsFromFrame(ptr::null_mut(), ptr::null_mut(), args, ptr::null_mut());
            assert_eq!(TYPEOF(result), SEXPTYPE::VECSXP);
            assert_eq!(LENGTH(result), 1);
            let elt = VECTOR_ELT(result, 0);
            assert_eq!(TYPEOF(elt), SEXPTYPE::INTSXP);
            assert_eq!(*INTEGER(elt), 42);
        }
    }

    #[test]
    fn test_r_compress_decompress_round_trip() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let raw = make_raw(&[0, 1, 2, 3, 4, 5, 6, 7]);
            let mut err: Rboolean = 0;

            let c1 = R_compress1(raw);
            let c1_data = slice::from_raw_parts(RAW(c1), XLENGTH(c1) as usize);
            assert_eq!(parse_swapped_len_prefix(c1_data), Some(8));
            let d1 = R_decompress1(c1, &mut err);
            assert_eq!(err, 0);
            assert_eq!(TYPEOF(d1), SEXPTYPE::RAWSXP);
            assert_eq!(slice::from_raw_parts(RAW(d1), 8), &[0, 1, 2, 3, 4, 5, 6, 7]);

            err = 0;
            let c2 = R_compress2(raw);
            let c2_data = slice::from_raw_parts(RAW(c2), XLENGTH(c2) as usize);
            assert_eq!(parse_swapped_len_prefix(c2_data), Some(8));
            assert!(c2_data.len() >= 5);
            assert!(matches!(c2_data[4], b'2' | b'0'));
            let d2 = R_decompress2(c2, &mut err);
            assert_eq!(err, 0);
            assert_eq!(TYPEOF(d2), SEXPTYPE::RAWSXP);
            assert_eq!(slice::from_raw_parts(RAW(d2), 8), &[0, 1, 2, 3, 4, 5, 6, 7]);

            err = 0;
            let c3 = R_compress3(raw);
            let c3_data = slice::from_raw_parts(RAW(c3), XLENGTH(c3) as usize);
            assert_eq!(parse_swapped_len_prefix(c3_data), Some(8));
            assert!(c3_data.len() >= 5);
            assert!(matches!(c3_data[4], b'Z' | b'0'));
            let d3 = R_decompress3(c3, &mut err);
            assert_eq!(err, 0);
            assert_eq!(TYPEOF(d3), SEXPTYPE::RAWSXP);
            assert_eq!(slice::from_raw_parts(RAW(d3), 8), &[0, 1, 2, 3, 4, 5, 6, 7]);
        }
    }

    #[test]
    fn test_r_decompress_rejects_truncated_input() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let raw = make_raw(&[1, 2, 3, 4]);
            let mut err: Rboolean = 0;
            assert_eq!(R_decompress1(raw, &mut err), R_NilValue());
            assert_eq!(err, 1);
            err = 0;
            assert_eq!(R_decompress2(raw, &mut err), R_NilValue());
            assert_eq!(err, 1);
            err = 0;
            assert_eq!(R_decompress3(raw, &mut err), R_NilValue());
            assert_eq!(err, 1);
        }
    }

    #[test]
    fn test_r_decompress2_3_support_legacy_markers() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let expected = [10, 20, 30, 40, 50, 60, 70, 80];
            let raw = make_raw(&expected);
            let c1 = R_compress1(raw);
            let c1_data = slice::from_raw_parts(RAW(c1), XLENGTH(c1) as usize);

            let mut marker1 = Vec::with_capacity(c1_data.len() + 1);
            marker1.extend_from_slice(&c1_data[..4]);
            marker1.push(b'1');
            marker1.extend_from_slice(&c1_data[4..]);
            let marker1_raw = make_raw(&marker1);

            let mut marker0 = Vec::with_capacity(expected.len() + 5);
            marker0.extend_from_slice(&swapped_len_bytes(expected.len()));
            marker0.push(b'0');
            marker0.extend_from_slice(&expected);
            let marker0_raw = make_raw(&marker0);

            let mut err: Rboolean = 0;
            let d21 = R_decompress2(marker1_raw, &mut err);
            assert_eq!(err, 0);
            assert_eq!(slice::from_raw_parts(RAW(d21), expected.len()), &expected);

            err = 0;
            let d31 = R_decompress3(marker1_raw, &mut err);
            assert_eq!(err, 0);
            assert_eq!(slice::from_raw_parts(RAW(d31), expected.len()), &expected);

            err = 0;
            let d20 = R_decompress2(marker0_raw, &mut err);
            assert_eq!(err, 0);
            assert_eq!(slice::from_raw_parts(RAW(d20), expected.len()), &expected);

            err = 0;
            let d30 = R_decompress3(marker0_raw, &mut err);
            assert_eq!(err, 0);
            assert_eq!(slice::from_raw_parts(RAW(d30), expected.len()), &expected);
        }
    }

    #[test]
    fn test_r_decompress2_3_reject_unknown_marker() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let mut blob = Vec::new();
            blob.extend_from_slice(&swapped_len_bytes(0));
            blob.push(b'X');
            let raw = make_raw(&blob);
            let mut err: Rboolean = 0;
            assert_eq!(R_decompress2(raw, &mut err), R_NilValue());
            assert_eq!(err, 1);
            err = 0;
            assert_eq!(R_decompress3(raw, &mut err), R_NilValue());
            assert_eq!(err, 1);
        }
    }

    #[test]
    fn test_r_write_connection_returns_zero() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let n = R_WriteConnection(ptr::null_mut(), ptr::null(), 42);
            assert_eq!(n, 0);
        }
    }

    // ---- Round-trip serialization tests ----

    #[test]
    fn test_roundtrip_integer_scalar() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let s = Rf_ScalarInteger(42);
            let raw = R_serialize(s, R_NilValue(), R_NilValue(), R_NilValue(), R_NilValue());
            assert_ne!(raw, R_NilValue());
            assert!(XLENGTH(raw) > 0);

            let result = R_unserialize(raw, R_NilValue());
            assert_ne!(result, R_NilValue());
            assert_eq!(TYPEOF(result), SEXPTYPE::INTSXP);
            assert_eq!(LENGTH(result), 1);
            assert_eq!(*INTEGER(result), 42);
        }
    }

    #[test]
    fn test_roundtrip_real_scalar() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let s = Rf_ScalarReal(3.14159);
            let raw = R_serialize(s, R_NilValue(), R_NilValue(), R_NilValue(), R_NilValue());
            assert_ne!(raw, R_NilValue());

            let result = R_unserialize(raw, R_NilValue());
            assert_ne!(result, R_NilValue());
            assert_eq!(TYPEOF(result), SEXPTYPE::REALSXP);
            assert_eq!(LENGTH(result), 1);
            assert!((*REAL(result) - 3.14159).abs() < 1e-10);
        }
    }

    #[test]
    fn test_roundtrip_logical_scalar() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let s = Rf_ScalarLogical(1); // TRUE
            let raw = R_serialize(s, R_NilValue(), R_NilValue(), R_NilValue(), R_NilValue());
            assert_ne!(raw, R_NilValue());

            let result = R_unserialize(raw, R_NilValue());
            assert_ne!(result, R_NilValue());
            assert_eq!(TYPEOF(result), SEXPTYPE::LGLSXP);
            assert_eq!(LENGTH(result), 1);
            assert_eq!(*LOGICAL(result), 1);
        }
    }

    #[test]
    fn test_roundtrip_integer_vector() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let s = Rf_allocVector3(SEXPTYPE::INTSXP, 3);
            let data = INTEGER(s);
            *data = 10;
            *data.add(1) = 20;
            *data.add(2) = 30;

            let raw = R_serialize(s, R_NilValue(), R_NilValue(), R_NilValue(), R_NilValue());
            assert_ne!(raw, R_NilValue());

            let result = R_unserialize(raw, R_NilValue());
            assert_ne!(result, R_NilValue());
            assert_eq!(TYPEOF(result), SEXPTYPE::INTSXP);
            assert_eq!(LENGTH(result), 3);
            let rdata = INTEGER(result);
            assert_eq!(*rdata, 10);
            assert_eq!(*rdata.add(1), 20);
            assert_eq!(*rdata.add(2), 30);
        }
    }

    #[test]
    fn test_roundtrip_real_vector() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let s = Rf_allocVector3(SEXPTYPE::REALSXP, 2);
            let data = REAL(s);
            *data = 1.5;
            *data.add(1) = 2.5;

            let raw = R_serialize(s, R_NilValue(), R_NilValue(), R_NilValue(), R_NilValue());
            assert_ne!(raw, R_NilValue());

            let result = R_unserialize(raw, R_NilValue());
            assert_ne!(result, R_NilValue());
            assert_eq!(TYPEOF(result), SEXPTYPE::REALSXP);
            assert_eq!(LENGTH(result), 2);
            let rdata = REAL(result);
            assert!((*rdata - 1.5).abs() < 1e-10);
            assert!((*rdata.add(1) - 2.5).abs() < 1e-10);
        }
    }

    #[test]
    fn test_roundtrip_complex_scalar() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let s = Rf_ScalarComplex(Rcomplex { r: 1.0, i: 2.0 });
            let raw = R_serialize(s, R_NilValue(), R_NilValue(), R_NilValue(), R_NilValue());
            assert_ne!(raw, R_NilValue());

            let result = R_unserialize(raw, R_NilValue());
            assert_ne!(result, R_NilValue());
            assert_eq!(TYPEOF(result), SEXPTYPE::CPLXSXP);
            assert_eq!(LENGTH(result), 1);
            let c = *COMPLEX(result);
            assert!((c.r - 1.0).abs() < 1e-10);
            assert!((c.i - 2.0).abs() < 1e-10);
        }
    }

    #[test]
    fn test_roundtrip_string_scalar() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let s = Rf_mkString(c"hello".as_ptr());
            let raw = R_serialize(s, R_NilValue(), R_NilValue(), R_NilValue(), R_NilValue());
            assert_ne!(raw, R_NilValue());

            let result = R_unserialize(raw, R_NilValue());
            assert_ne!(result, R_NilValue());
            assert_eq!(TYPEOF(result), SEXPTYPE::STRSXP);
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
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let s = Rf_allocVector3(SEXPTYPE::STRSXP, 2);
            let c1 = Rf_mkChar(c"foo".as_ptr());
            let c2 = Rf_mkChar(c"bar".as_ptr());
            SET_STRING_ELT(s, 0, c1);
            SET_STRING_ELT(s, 1, c2);

            let raw = R_serialize(s, R_NilValue(), R_NilValue(), R_NilValue(), R_NilValue());
            assert_ne!(raw, R_NilValue());

            let result = R_unserialize(raw, R_NilValue());
            assert_ne!(result, R_NilValue());
            assert_eq!(TYPEOF(result), SEXPTYPE::STRSXP);
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
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let s = Rf_allocVector3(SEXPTYPE::RAWSXP, 3);
            let data = RAW(s);
            *data = 0xDE;
            *data.add(1) = 0xAD;
            *data.add(2) = 0xBE;

            let raw = R_serialize(s, R_NilValue(), R_NilValue(), R_NilValue(), R_NilValue());
            assert_ne!(raw, R_NilValue());

            let result = R_unserialize(raw, R_NilValue());
            assert_ne!(result, R_NilValue());
            assert_eq!(TYPEOF(result), SEXPTYPE::RAWSXP);
            assert_eq!(LENGTH(result), 3);
            let rdata = RAW(result);
            assert_eq!(*rdata, 0xDE);
            assert_eq!(*rdata.add(1), 0xAD);
            assert_eq!(*rdata.add(2), 0xBE);
        }
    }

    #[test]
    fn test_roundtrip_empty_vector() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let s = Rf_allocVector3(SEXPTYPE::INTSXP, 0);
            let raw = R_serialize(s, R_NilValue(), R_NilValue(), R_NilValue(), R_NilValue());
            assert_ne!(raw, R_NilValue());

            let result = R_unserialize(raw, R_NilValue());
            assert_ne!(result, R_NilValue());
            assert_eq!(TYPEOF(result), SEXPTYPE::INTSXP);
            assert_eq!(LENGTH(result), 0);
        }
    }

    #[test]
    fn test_roundtrip_null() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let s = R_NilValue();
            let raw = R_serialize(s, R_NilValue(), R_NilValue(), R_NilValue(), R_NilValue());
            assert_ne!(raw, R_NilValue());

            let result = R_unserialize(raw, R_NilValue());
            assert_eq!(result, R_NilValue());
        }
    }

    #[test]
    fn test_roundtrip_na_string() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let s = make_na_string_vector();
            let raw = R_serialize(s, R_NilValue(), R_NilValue(), R_NilValue(), R_NilValue());
            assert_ne!(raw, R_NilValue());

            let result = R_unserialize(raw, R_NilValue());
            assert_ne!(result, R_NilValue());
            assert_eq!(TYPEOF(result), SEXPTYPE::STRSXP);
            assert_eq!(LENGTH(result), 1);
            assert_eq!(STRING_ELT(result, 0), R_NaString());
        }
    }

    #[test]
    fn test_roundtrip_special_singletons() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let mut values = vec![R_NilValue(), R_UnboundValue(), R_MissingArg()];
            for env in [R_GlobalEnv(), R_BaseEnv(), R_EmptyEnv()] {
                if !env.is_null() {
                    values.push(env);
                }
            }
            for value in values {
                let raw = R_serialize(
                    value,
                    R_NilValue(),
                    R_NilValue(),
                    R_NilValue(),
                    R_NilValue(),
                );
                assert_ne!(raw, R_NilValue());

                let result = R_unserialize(raw, R_NilValue());
                assert_eq!(result, value);
            }
        }
    }

    #[test]
    fn test_roundtrip_generic_list() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            // Create a list(c(1, 2), c(3.0, 4.0))
            let vec1 = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
            *INTEGER(vec1) = 1;
            *INTEGER(vec1).add(1) = 2;

            let vec2 = Rf_allocVector3(SEXPTYPE::REALSXP, 2);
            *REAL(vec2) = 3.0;
            *REAL(vec2).add(1) = 4.0;

            let list = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
            SET_VECTOR_ELT(list, 0, vec1);
            SET_VECTOR_ELT(list, 1, vec2);

            let raw = R_serialize(list, R_NilValue(), R_NilValue(), R_NilValue(), R_NilValue());
            assert_ne!(raw, R_NilValue());

            let result = R_unserialize(raw, R_NilValue());
            assert_ne!(result, R_NilValue());
            assert_eq!(TYPEOF(result), SEXPTYPE::VECSXP);
            assert_eq!(LENGTH(result), 2);

            // Check first element (integer vector)
            let r1 = VECTOR_ELT(result, 0);
            assert_eq!(TYPEOF(r1), SEXPTYPE::INTSXP);
            assert_eq!(LENGTH(r1), 2);
            assert_eq!(*INTEGER(r1), 1);
            assert_eq!(*INTEGER(r1).add(1), 2);

            // Check second element (real vector)
            let r2 = VECTOR_ELT(result, 1);
            assert_eq!(TYPEOF(r2), SEXPTYPE::REALSXP);
            assert_eq!(LENGTH(r2), 2);
            assert!((*REAL(r2) - 3.0).abs() < 1e-10);
            assert!((*REAL(r2).add(1) - 4.0).abs() < 1e-10);
        }
    }

    #[test]
    fn test_roundtrip_nested_list() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            // Create list(list(1, 2), list(3, 4))
            let inner1 = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
            let inner2 = Rf_allocVector3(SEXPTYPE::VECSXP, 2);

            let v1 = Rf_ScalarInteger(1);
            let v2 = Rf_ScalarInteger(2);
            let v3 = Rf_ScalarInteger(3);
            let v4 = Rf_ScalarInteger(4);

            SET_VECTOR_ELT(inner1, 0, v1);
            SET_VECTOR_ELT(inner1, 1, v2);
            SET_VECTOR_ELT(inner2, 0, v3);
            SET_VECTOR_ELT(inner2, 1, v4);

            let outer = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
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
            assert_eq!(TYPEOF(result), SEXPTYPE::VECSXP);
            assert_eq!(LENGTH(result), 2);

            let r_inner1 = VECTOR_ELT(result, 0);
            assert_eq!(TYPEOF(r_inner1), SEXPTYPE::VECSXP);
            assert_eq!(LENGTH(r_inner1), 2);
            let rv1 = VECTOR_ELT(r_inner1, 0);
            let rv2 = VECTOR_ELT(r_inner1, 1);
            assert_eq!(TYPEOF(rv1), SEXPTYPE::INTSXP);
            assert_eq!(*INTEGER(rv1), 1);
            assert_eq!(TYPEOF(rv2), SEXPTYPE::INTSXP);
            assert_eq!(*INTEGER(rv2), 2);

            let r_inner2 = VECTOR_ELT(result, 1);
            assert_eq!(TYPEOF(r_inner2), SEXPTYPE::VECSXP);
            assert_eq!(LENGTH(r_inner2), 2);
            let rv3 = VECTOR_ELT(r_inner2, 0);
            let rv4 = VECTOR_ELT(r_inner2, 1);
            assert_eq!(TYPEOF(rv3), SEXPTYPE::INTSXP);
            assert_eq!(*INTEGER(rv3), 3);
            assert_eq!(TYPEOF(rv4), SEXPTYPE::INTSXP);
            assert_eq!(*INTEGER(rv4), 4);
        }
    }

    #[test]
    fn test_serialized_format_header() {
        let _session = crate::sexp::session::RSession::new();
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
            let version = i32::from_ne_bytes(data[2..6].try_into().unwrap_or([0; 4]));
            assert_eq!(version, 3);
        }
    }

    #[test]
    fn test_roundtrip_large_integer_vector() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let n: i32 = 100;
            let s = Rf_allocVector3(SEXPTYPE::INTSXP, n as R_xlen_t);
            let data = INTEGER(s);
            for i in 0..n as isize {
                *data.offset(i) = (i * i) as c_int;
            }

            let raw = R_serialize(s, R_NilValue(), R_NilValue(), R_NilValue(), R_NilValue());
            assert_ne!(raw, R_NilValue());

            let result = R_unserialize(raw, R_NilValue());
            assert_ne!(result, R_NilValue());
            assert_eq!(TYPEOF(result), SEXPTYPE::INTSXP);
            assert_eq!(LENGTH(result), n);
            let rdata = INTEGER(result);
            for i in 0..n as isize {
                assert_eq!(*rdata.offset(i), (i * i) as c_int);
            }
        }
    }

    #[test]
    fn test_binary_reader_errors() {
        let _session = crate::sexp::session::RSession::new();
        let data = [0u8; 3]; // Too short for an i32
        let mut reader = BinaryReader::new(&data);
        assert!(reader.read_i32().is_err());
        assert!(reader.read_f64().is_err());
        assert!(reader.read_bytes(10).is_err());
    }

    #[test]
    fn test_binary_reader_remaining() {
        let _session = crate::sexp::session::RSession::new();
        let data = [1u8, 2, 3, 4, 5];
        let mut reader = BinaryReader::new(&data);
        assert_eq!(reader.remaining(), 5);
        let _ = reader.read_byte();
        assert_eq!(reader.remaining(), 4);
    }
}
