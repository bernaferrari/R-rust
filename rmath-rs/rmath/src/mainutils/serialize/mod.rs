#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Port of R's src/main/serialize.c -- R object serialization/unserialization.
//!
//! Implements R serialization formats supporting basic atomic types
//! (NILSXP, LGLSXP, INTSXP, REALSXP, CPLXSXP, STRSXP, RAWSXP),
//! generic lists (VECSXP), dotted pairs (LISTSXP), symbols (SYMSXP),
//! closures (CLOSXP), and attributes.
//!
//! The format follows R's serialization protocol structure: format header,
//! version info, then recursive WriteItem/ReadItem. ASCII, native-endian
//! binary, and XDR binary headers/bodies are supported.

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
use crate::sexp::instance::with_required_current_instance;
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
unsafe fn optional_arg(args: SEXP, index: usize) -> SEXP {
    let mut cur = args;
    for _ in 0..index {
        if cur.is_null() || unsafe { TYPEOF(cur) } == SEXPTYPE::NILSXP {
            return unsafe { R_NilValue() };
        }
        cur = unsafe { CDR(cur) };
    }
    if cur.is_null() || unsafe { TYPEOF(cur) } == SEXPTYPE::NILSXP {
        return unsafe { R_NilValue() };
    }
    unsafe { CAR(cur) }
}

unsafe fn arg_by_name_or_position(args: SEXP, name: &str, position: usize) -> SEXP {
    unsafe {
        let mut cur = args;
        while !cur.is_null() && TYPEOF(cur) != SEXPTYPE::NILSXP {
            if call_arg_tag_name(cur).as_deref() == Some(name) {
                return CAR(cur);
            }
            cur = CDR(cur);
        }

        let mut cur = args;
        let mut untagged = 0usize;
        while !cur.is_null() && TYPEOF(cur) != SEXPTYPE::NILSXP {
            if call_arg_tag_name(cur).is_none() {
                if untagged == position {
                    return CAR(cur);
                }
                untagged += 1;
            }
            cur = CDR(cur);
        }
        R_NilValue()
    }
}

unsafe fn arg_present_by_name_or_position(args: SEXP, name: &str, position: usize) -> bool {
    unsafe {
        let mut cur = args;
        while !cur.is_null() && TYPEOF(cur) != SEXPTYPE::NILSXP {
            if call_arg_tag_name(cur).as_deref() == Some(name) {
                return true;
            }
            cur = CDR(cur);
        }

        let mut cur = args;
        let mut untagged = 0usize;
        while !cur.is_null() && TYPEOF(cur) != SEXPTYPE::NILSXP {
            if call_arg_tag_name(cur).is_none() {
                if untagged == position {
                    return true;
                }
                untagged += 1;
            }
            cur = CDR(cur);
        }
        false
    }
}

unsafe fn call_arg_tag_name(node: SEXP) -> Option<String> {
    unsafe {
        let tag = TAG(node);
        if tag.is_null() || tag == R_NilValue() {
            return None;
        }
        let pname = PRINTNAME(tag);
        if pname.is_null() || pname == R_NaString() {
            return None;
        }
        let chars = CHAR(pname);
        if chars.is_null() {
            return None;
        }
        Some(CStr::from_ptr(chars).to_string_lossy().into_owned())
    }
}

unsafe fn scalar_integer_value(value: SEXP) -> Option<c_int> {
    unsafe {
        if value.is_null() || value == R_NilValue() || LENGTH(value) < 1 {
            return None;
        }
        let kind = TYPEOF(value);
        if kind == SEXPTYPE::INTSXP || kind == SEXPTYPE::LGLSXP {
            Some(*INTEGER(value))
        } else if kind == SEXPTYPE::REALSXP {
            Some(*REAL(value) as c_int)
        } else {
            None
        }
    }
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
    with_required_current_instance(|instance| {
        instance.serialize_state.used = 0;
        instance.serialize_state.cache_names = [ptr::null_mut(); NC];
        instance.serialize_state.cache_ptrs = [ptr::null_mut(); NC];
    });
}

mod api;
mod compress;
mod core;
mod lazyload;
mod membuf;
mod stream;

pub use self::api::*;
pub use self::compress::*;
pub use self::core::*;
pub use self::lazyload::*;
pub use self::membuf::*;
pub use self::stream::*;
#[cfg(test)]
mod tests;
