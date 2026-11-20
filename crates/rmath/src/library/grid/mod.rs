//! Grid package - grid graphics

mod clippath;
mod gpar;
mod grid;
mod just;
mod layout;
mod mask;
mod matrix;
mod path;
mod register;
pub mod state;
pub mod types;
mod typeset;
mod unit;
pub mod util;
mod viewport;

use std::os::raw::{c_char, c_double, c_int};

use crate::sexp::context::r_error;
use crate::sexp::ffi::{R_xlen_t, SEXP};
use crate::sexp::object::{Sexp, SexpMut};

// ---------------------------------------------------------------------------
// Checked SEXP access shared by the translated grid modules
// ---------------------------------------------------------------------------

/// Wrap a live raw object lent by translated grid code.
///
/// `Sexp::from_raw` rejects null, foreign, and reclaimed pointers before any
/// access; failures raise an R error instead of dereferencing bad memory.
#[inline]
pub(crate) fn checked(x: SEXP) -> Sexp<'static> {
    // SAFETY: translated grid entry points lend live, protected objects; the
    // checked wrap validates registration before lending a handle.
    unsafe { Sexp::from_raw(x) }.unwrap_or_else(|| r_error("invalid grid object"))
}

/// Checked `VECTOR_ELT`; wrong type or index raises an R error.
#[inline]
pub(crate) fn vector_elt(x: SEXP, index: R_xlen_t) -> SEXP {
    checked(x)
        .try_vector_elt(index)
        .map(|child| child.as_raw())
        .unwrap_or_else(|e| r_error(e.to_string()))
}

/// Checked `REAL(x)[i]`; wrong type or index raises an R error.
#[inline]
pub(crate) fn real_elt(x: SEXP, index: R_xlen_t) -> c_double {
    checked(x)
        .try_real_elt(index)
        .unwrap_or_else(|e| r_error(e.to_string()))
}

/// Checked `INTEGER(x)[i]`; wrong type or index raises an R error.
#[inline]
pub(crate) fn integer_elt(x: SEXP, index: R_xlen_t) -> c_int {
    checked(x)
        .try_integer_elt(index)
        .unwrap_or_else(|e| r_error(e.to_string()))
}

/// Checked `LOGICAL(x)[i]`; wrong type or index raises an R error.
#[inline]
pub(crate) fn logical_elt(x: SEXP, index: R_xlen_t) -> c_int {
    checked(x)
        .try_logical_elt(index)
        .unwrap_or_else(|e| r_error(e.to_string()))
}

/// Checked `STRING_ELT`; wrong type or index raises an R error.
#[inline]
pub(crate) fn string_elt(x: SEXP, index: R_xlen_t) -> SEXP {
    checked(x)
        .try_string_elt(index)
        .map(|child| child.as_raw())
        .unwrap_or_else(|e| r_error(e.to_string()))
}

/// Checked `CHAR()`; returns the sealed byte projection of a CHARSXP.
#[inline]
pub(crate) fn char_ptr(x: SEXP) -> *const c_char {
    checked(x)
        .try_data_ptr()
        .map(|p| p.cast::<c_char>().cast_const())
        .unwrap_or_else(|e| r_error(e.to_string()))
}

/// Checked `STRING_ELT` text copy; `None` for `NA_character_`.
#[inline]
pub(crate) fn string_value_elt(x: SEXP, index: R_xlen_t) -> Option<String> {
    checked(x)
        .try_string_value_elt(index)
        .unwrap_or_else(|e| r_error(e.to_string()))
}

/// Checked `SET_VECTOR_ELT` on a borrowed raw object.
#[inline]
pub(crate) fn set_vector_elt(x: SEXP, index: R_xlen_t, value: SEXP) {
    let parent = checked(x);
    // SAFETY: the guard lives only for this call; translated grid code lends
    // no Rust payload references into `x` that could overlap the write.
    let mut parent = unsafe { SexpMut::from_owned(parent) };
    parent
        .try_set_vector_elt(index, checked(value))
        .unwrap_or_else(|e| r_error(e.to_string()));
}

/// Checked `REAL(x)[i] = v` on a borrowed raw object.
#[inline]
pub(crate) fn set_real_elt(x: SEXP, index: R_xlen_t, value: c_double) {
    let parent = checked(x);
    // SAFETY: the guard lives only for this call; no Rust payload borrow
    // into `x` overlaps the write.
    let mut parent = unsafe { SexpMut::from_owned(parent) };
    parent
        .try_set_real_elt(index, value)
        .unwrap_or_else(|e| r_error(e.to_string()));
}

/// Checked `INTEGER(x)[i] = v` on a borrowed raw object.
#[inline]
pub(crate) fn set_integer_elt(x: SEXP, index: R_xlen_t, value: c_int) {
    let parent = checked(x);
    // SAFETY: the guard lives only for this call; no Rust payload borrow
    // into `x` overlaps the write.
    let mut parent = unsafe { SexpMut::from_owned(parent) };
    parent
        .try_set_integer_elt(index, value)
        .unwrap_or_else(|e| r_error(e.to_string()));
}

/// Checked `LOGICAL(x)[i] = v` on a borrowed raw object.
#[inline]
pub(crate) fn set_logical_elt(x: SEXP, index: R_xlen_t, value: c_int) {
    let parent = checked(x);
    // SAFETY: the guard lives only for this call; no Rust payload borrow
    // into `x` overlaps the write.
    let mut parent = unsafe { SexpMut::from_owned(parent) };
    parent
        .try_set_logical_elt(index, value)
        .unwrap_or_else(|e| r_error(e.to_string()));
}

/// Checked `SET_STRING_ELT` on a borrowed raw object.
#[inline]
pub(crate) fn set_string_elt(x: SEXP, index: R_xlen_t, value: SEXP) {
    let parent = checked(x);
    // SAFETY: the guard lives only for this call; no Rust payload borrow
    // into `x` overlaps the write.
    let mut parent = unsafe { SexpMut::from_owned(parent) };
    parent
        .try_set_string_elt(index, checked(value))
        .unwrap_or_else(|e| r_error(e.to_string()));
}

unsafe extern "C-unwind" fn c_pretty2(scale: SEXP, n: SEXP) -> SEXP {
    unsafe { grid::L_pretty2(scale, n) }
}

pub(crate) fn lookup(name: &str) -> Option<crate::mainutils::native_routines::NativeRoutine> {
    let bare = name.strip_prefix("C_").unwrap_or(name);
    match bare {
        "pretty2" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args2(c_pretty2),
        )),
        _ => None,
    }
}

pub unsafe fn install_call_symbols(env: SEXP) {
    unsafe {
        let cname = std::ffi::CString::new("C_pretty2").unwrap_or_default();
        crate::sexp::envir::defineVar(
            crate::sexp::symbol::Rf_install(cname.as_ptr()),
            crate::sexp::constructors::Rf_mkString(cname.as_ptr()),
            env,
        );
    }
}
