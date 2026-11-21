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

// ---------------------------------------------------------------------------
// Allocation wrappers for translated code running inside the active session
// ---------------------------------------------------------------------------

/// Allocate a vector in the active session; failures raise an R error.
#[inline]
pub(crate) fn alloc_vector(sexptype: crate::sexp::ffi::SEXPTYPE, len: c_int) -> SEXP {
    // SAFETY: grid entry points run inside the owning session; allocation
    // failures raise R errors rather than producing invalid objects.
    crate::sexp::constructors::Rf_allocVector(sexptype, len)
}

/// Allocate a one-element REALSXP holding `value`.
#[inline]
pub(crate) fn scalar_real(value: c_double) -> SEXP {
    // SAFETY: see `alloc_vector`.
    crate::sexp::constructors::Rf_ScalarReal(value)
}

/// Allocate a one-element INTSXP holding `value`.
#[inline]
pub(crate) fn scalar_integer(value: c_int) -> SEXP {
    // SAFETY: see `alloc_vector`.
    crate::sexp::constructors::Rf_ScalarInteger(value)
}

/// Allocate a one-element LGLSXP holding `value`.
#[inline]
pub(crate) fn scalar_logical(value: c_int) -> SEXP {
    // SAFETY: see `alloc_vector`.
    crate::sexp::constructors::Rf_ScalarLogical(value)
}

/// Allocate a one-element STRSXP from Rust text.
#[inline]
pub(crate) fn mk_string(text: &str) -> SEXP {
    let text = std::ffi::CString::new(text)
        .unwrap_or_else(|_| r_error("embedded NUL byte in grid string"));
    // SAFETY: see `alloc_vector`; `text` is a live NUL-terminated buffer.
    unsafe { crate::sexp::constructors::Rf_mkString(text.as_ptr()) }
}

/// Allocate a CHARSXP from Rust text.
#[inline]
pub(crate) fn mk_char(text: &str) -> SEXP {
    let text = std::ffi::CString::new(text)
        .unwrap_or_else(|_| r_error("embedded NUL byte in grid string"));
    // SAFETY: see `alloc_vector`; `text` is a live NUL-terminated buffer.
    unsafe { crate::sexp::constructors::Rf_mkChar(text.as_ptr()) }
}

/// Intern a symbol from Rust text.
#[inline]
pub(crate) fn install(text: &str) -> SEXP {
    let text = std::ffi::CString::new(text)
        .unwrap_or_else(|_| r_error("embedded NUL byte in grid symbol"));
    // SAFETY: `text` is a live NUL-terminated buffer for the call.
    unsafe { crate::sexp::symbol::Rf_install(text.as_ptr()) }
}

/// Protect a live object and return the RAII guard.
#[inline]
pub(crate) fn protect(x: SEXP) -> crate::sexp::protect::ProtectGuard<'static> {
    // `checked` proves `x` names a live registered object before the root
    // slot is claimed; foreign or reclaimed pointers raise an R error.
    let _ = checked(x);
    // SAFETY: `x` was just validated as a live registered node.
    crate::sexp::protect::protect(x)
}

// ---------------------------------------------------------------------------
// Bridge calls whose raw-pointer contracts are discharged locally
// ---------------------------------------------------------------------------

/// `getAttrib` on a checked, live object and symbol.
#[inline]
pub(crate) fn get_attrib(x: SEXP, which: SEXP) -> SEXP {
    let _ = checked(x);
    let _ = checked(which);
    // SAFETY: both operands were just validated as live registered objects.
    crate::attrib_core::getAttrib(x, which)
}

/// `Rf_inherits` on a checked, live object.
#[inline]
pub(crate) fn inherits(x: SEXP, name: &std::ffi::CStr) -> bool {
    let _ = checked(x);
    // SAFETY: `x` was just validated as a live registered object.
    let result = crate::mainutils::objects::inherits2(x, name.as_ptr());
    result != 0
}

/// `R_classgets` on checked, live objects.
#[inline]
pub(crate) fn class_gets(x: SEXP, class: SEXP) -> SEXP {
    let _ = checked(x);
    let _ = checked(class);
    // SAFETY: both operands were just validated as live registered objects.
    crate::attrib_core::R_classgets(x, class)
}

/// `Rf_duplicate` on a checked, live object.
#[inline]
pub(crate) fn duplicate(x: SEXP) -> SEXP {
    let _ = checked(x);
    // SAFETY: `x` was just validated as a live registered object.
    crate::main::duplicate::Rf_duplicate(x)
}

unsafe extern "C-unwind" fn c_pretty2(scale: SEXP, n: SEXP) -> SEXP {
    grid::L_pretty2(scale, n)
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
