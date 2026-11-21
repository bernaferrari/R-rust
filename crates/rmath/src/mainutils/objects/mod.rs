#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Port of R's src/main/objects.c -- S-style generic functions and class support.
//!
//! This module provides the core S3/S4 method dispatch infrastructure, including
//! UseMethod, NextMethod, standardGeneric, inherits, and related helpers.
//!
//! Original file: r-source/src/main/objects.c (1,879 lines)

pub(crate) use std::collections::{HashMap, HashSet};
pub(crate) use std::ffi::CString;
pub(crate) use std::os::raw::{c_char, c_int};
pub(crate) use std::ptr;

pub(crate) use crate::eval::attrib_core::isObject;
pub(crate) use crate::eval::eval::Rf_eval;
pub(crate) use crate::sexp::accessors::*;
pub(crate) use crate::sexp::attrib_core::{R_ClassSymbol, getAttrib};
pub(crate) use crate::sexp::constructors::*;
pub(crate) use crate::sexp::context::{R_GlobalContext, RCNTXT};
pub(crate) use crate::sexp::ffi::{FALSE, R_xlen_t, SEXP, SEXPTYPE, TRUE};
pub(crate) use crate::sexp::globals::*;
pub(crate) use crate::sexp::memory_ext::allocList;
pub(crate) use crate::sexp::protect::protect;
pub(crate) use crate::sexp::symbol::Rf_install;

/// Set attribute through the evaluator setter (GNU names/class/tsp rules).
pub(crate) fn setAttrib(x: SEXP, which: SEXP, value: SEXP) {
    // SAFETY: callers pass live, rooted nodes of the current dispatch; the
    // evaluator setter only rewrites `x`'s attribute chain.
    unsafe { crate::eval::attrib_core::setAttrib(x, which, value) }
}

/// GNU `R_data_class(x, FALSE)`: the explicit or implicit class vector.
pub(crate) fn R_data_class(x: SEXP) -> SEXP {
    // SAFETY: `x` is a live, rooted object of the current dispatch; the
    // evaluator helper only reads its attributes and allocates the result.
    unsafe { crate::eval::attrib_core::R_data_class(x) }
}

// ---------------------------------------------------------------------------
// Checked CHARSXP text and string constructors
// ---------------------------------------------------------------------------

/// Raw CHAR bytes of a CHARSXP (GNU `CHAR(x)`).
pub(crate) fn char_bytes(x: SEXP) -> Vec<u8> {
    crate::mainutils::printutils::charsxp_bytes(x)
}

/// CHAR text of a CHARSXP as an owned C string (up to its first NUL);
/// `None` for a null pointer, mirroring a null `CHAR(x)`.
pub(crate) fn char_cstring(x: SEXP) -> Option<std::ffi::CString> {
    if x.is_null() {
        return None;
    }
    let mut bytes = char_bytes(x);
    if let Some(nul) = bytes.iter().position(|&b| b == 0) {
        bytes.truncate(nul);
    }
    std::ffi::CString::new(bytes).ok()
}

/// Lossy UTF-8 text of a CHARSXP's CHAR bytes.
pub(crate) fn char_lossy(x: SEXP) -> String {
    String::from_utf8_lossy(&char_bytes(x)).into_owned()
}

/// UTF-8 text of a CHARSXP's CHAR bytes, empty when invalid.
pub(crate) fn char_utf8(x: SEXP) -> String {
    String::from_utf8(char_bytes(x)).unwrap_or_default()
}

/// Checked CHARSXP for `text` on the active session heap. Like
/// `Rf_mkChar(CString::new(text).unwrap_or_default())`, an interior NUL
/// yields the empty string.
pub(crate) fn mk_char(text: &str) -> SEXP {
    let text = if text.contains('\0') { "" } else { text };
    crate::mainutils::essentials::active_owner()
        .node_factory()
        .character(text)
        .map(|value| value.as_raw())
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
}

/// Checked STRSXP(1) for `text` (`Rf_mkString`); an interior NUL yields "".
pub(crate) fn mk_string(text: &str) -> SEXP {
    let text = if text.contains('\0') { "" } else { text };
    crate::mainutils::essentials::active_owner()
        .node_factory()
        .strings(&[text])
        .map(|value| value.as_raw())
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
}

mod apply_args;
pub(crate) mod class_name;
mod class_prims;
mod helpers;
mod nextmethod;
mod primitive_methods;
mod s4;
mod standard_generic;
mod state;
#[cfg(test)]
mod tests;
mod usemethod;

pub use self::apply_args::*;
pub use self::class_prims::*;
pub(crate) use self::helpers::*;
pub use self::nextmethod::*;
pub use self::primitive_methods::*;
pub use self::s4::*;
pub use self::standard_generic::*;
pub use self::state::*;
pub use self::usemethod::*;
