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

use std::ffi::CStr;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::raw::{c_char, c_double, c_int, c_void};
#[cfg(unix)]
use std::os::unix::ffi::OsStrExt;
use std::path::PathBuf;
use std::ptr;
use std::slice;

use crate::sexp::attrib_core::{R_NamesSymbol, setAttrib};
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
use crate::sexp::symbol::install_str;
#[cfg(test)]
use crate::sexp::symbol::Rf_install;

fn error(msg: &str) -> ! {
    std::panic::panic_any(crate::sexp::context::RError {
        message: msg.to_string(),
    });
}

#[inline]
fn require_arg(args: SEXP, index: usize) -> SEXP {
    let mut cur = args;
    for _ in 0..index {
        if cur.is_null() || TYPEOF(cur) == SEXPTYPE::NILSXP {
            {
                error("wrong number of arguments")
            };
        }
        cur = CDR(cur);
    }
    if cur.is_null() || TYPEOF(cur) == SEXPTYPE::NILSXP {
        {
            error("wrong number of arguments")
        };
    }
    CAR(cur)
}

#[inline]
fn optional_arg(args: SEXP, index: usize) -> SEXP {
    let mut cur = args;
    for _ in 0..index {
        if cur.is_null() || TYPEOF(cur) == SEXPTYPE::NILSXP {
            return R_NilValue();
        }
        cur = CDR(cur);
    }
    if cur.is_null() || TYPEOF(cur) == SEXPTYPE::NILSXP {
        return R_NilValue();
    }
    CAR(cur)
}

fn arg_by_name_or_position(args: SEXP, name: &str, position: usize) -> SEXP {
    {
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

fn arg_present_by_name_or_position(args: SEXP, name: &str, position: usize) -> bool {
    {
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

/// Audited boundary: serialize builtins only run inside evaluated R calls,
/// where the current runtime owner is live.
fn current_owner() -> crate::sexp::owner::OwnerToken<'static> {
    // SAFETY: serialize builtins only run inside evaluated R calls.
    unsafe { crate::sexp::owner::OwnerToken::current() }
        .unwrap_or_else(|e| error(&e.to_string()))
}

fn call_arg_tag_name(node: SEXP) -> Option<String> {
    let tag = TAG(node);
    if tag.is_null() || tag == R_NilValue() {
        return None;
    }
    let pname = PRINTNAME(tag);
    if pname.is_null() || pname == R_NaString() {
        return None;
    }
    let bytes = charsxp_as_utf8(pname);
    if bytes.is_empty() {
        return None;
    }
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

fn scalar_integer_value(value: SEXP) -> Option<c_int> {
    if value.is_null() || value == R_NilValue() || LENGTH(value) < 1 {
        return None;
    }
    let kind = TYPEOF(value);
    if kind == SEXPTYPE::INTSXP {
        Some(INTEGER_ELT(value, 0))
    } else if kind == SEXPTYPE::LGLSXP {
        Some(LOGICAL_ELT(value, 0))
    } else if kind == SEXPTYPE::REALSXP {
        Some(REAL_ELT(value, 0) as c_int)
    } else {
        None
    }
}

#[inline]
fn sexp_to_path(file: SEXP) -> PathBuf {
    if TYPEOF(file) != SEXPTYPE::STRSXP || LENGTH(file) <= 0 {
        {
            error("not a proper file name")
        };
    }
    let elt = STRING_ELT(file, 0);
    if elt.is_null() {
        {
            error("not a proper file name")
        };
    }
    let bytes = charsxp_as_utf8(elt);
    #[cfg(unix)]
    {
        PathBuf::from(std::ffi::OsStr::from_bytes(&bytes))
    }
    #[cfg(not(unix))]
    {
        PathBuf::from(String::from_utf8_lossy(&bytes).into_owned())
    }
}

#[inline]
fn raw_from_bytes(bytes: &[u8]) -> SEXP {
    let ans = Rf_allocVector3(SEXPTYPE::RAWSXP, bytes.len() as R_xlen_t);
    if !ans.is_null() {
        for (index, byte) in bytes.iter().enumerate() {
            SET_RAW_ELT(ans, index as c_int, *byte);
        }
    }
    ans
}

/// Copy a RAWSXP payload into owned native bytes with checked reads.
fn raw_to_bytes(raw: SEXP) -> Vec<u8> {
    let len = usize::try_from(XLENGTH(raw)).unwrap_or(0);
    let mut bytes = Vec::with_capacity(len);
    for index in 0..len {
        let index = c_int::try_from(index).unwrap_or_else(|_| error("read error"));
        bytes.push(RAW_ELT(raw, index));
    }
    bytes
}

/// Audited boundary: copy UTF-8 text into a fresh CHARSXP node.
fn mk_char(text: &str) -> SEXP {
    mk_chars(text.as_bytes())
}

/// Audited boundary: copy bytes into a fresh CHARSXP node.
fn mk_chars(bytes: &[u8]) -> SEXP {
    // SAFETY: `bytes` is a valid read-only buffer for the duration of the
    // call; Rf_mkCharLen copies exactly `len` bytes into the new node.
    unsafe { Rf_mkCharLen(bytes.as_ptr() as *const c_char, bytes.len() as c_int) }
}

/// Build a scalar STRSXP from UTF-8 text.
fn mk_string(text: &str) -> SEXP {
    let charsxp = mk_char(text);
    if charsxp.is_null() {
        return charsxp;
    }
    Rf_ScalarString(charsxp)
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
        // SAFETY: `err` is a caller-owned out parameter checked for null;
        // the write is a single flag store before returning.
        unsafe {
            *err = 1;
        }
    }
}

#[inline]
fn clear_lazy_load_cache() {
    with_required_current_instance(|instance| {
        // SAFETY: `instance` is the live current RInstance for this
        // strictly-local cache reset; no callback runs inside the borrow.
        unsafe {
            (*instance).serialize_state.used = 0;
            (*instance).serialize_state.cache_names = [ptr::null_mut(); NC];
            (*instance).serialize_state.cache_ptrs = [ptr::null_mut(); NC];
        }
    });
}

mod api;
mod ascii_numbers;
mod compress;
mod core;
mod lazyload;
mod membuf;
mod owned_input;
mod owned_output;
mod raw_codecs;
mod stream;
mod stream_values;
mod workspace;

pub(crate) use owned_output::compress_container;

pub use self::api::*;
pub use self::compress::*;
pub use self::core::*;
pub use self::lazyload::*;
pub use self::membuf::*;
pub(crate) use self::raw_codecs::DecodeError as LazyRecordDecodeError;
pub use self::stream::*;
#[cfg(test)]
mod tests;

#[cfg(test)]
mod logical_scalar_tests;

#[cfg(test)]
mod promise_tests;

#[cfg(test)]
mod ascii_numeric_tests;

#[cfg(test)]
mod ascii_stream_tests;
