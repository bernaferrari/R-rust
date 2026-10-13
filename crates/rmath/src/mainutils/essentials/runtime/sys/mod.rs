//! Sys.* functions, R.home, date/time coercion, timezone and locale.

#[allow(unused_imports)]
use std::collections::BTreeSet;
#[allow(unused_imports)]
use std::ffi::{CStr, CString};
#[allow(unused_imports)]
use std::os::raw::{c_char, c_int};
#[allow(unused_imports)]
use std::path::{Path, PathBuf};

use crate::mainutils::essentials::*;

#[allow(unused_imports)]
use crate::sexp::accessors::{
    ATTRIB, CADDR, CADR, CAR, CDR, CHAR, COMPLEX, FORMALS, FRAME, HASHTAB, INTEGER, INTEGER_ELT,
    LENGTH, LOGICAL, LOGICAL_ELT, PRINTNAME, RAW, REAL, REAL_ELT, SET_ENCLOS, SET_OBJECT,
    SET_STRING_ELT, SET_VECTOR_ELT, SETCAR, SETCDR, SETTAG, STRING_ELT, TAG, TYPEOF, VECTOR_ELT,
    XLENGTH,
};
#[allow(unused_imports)]
use crate::sexp::constructors::{
    Rf_ScalarInteger, Rf_ScalarLogical, Rf_ScalarReal, Rf_allocVector3, Rf_cons, Rf_mkChar,
    Rf_mkString,
};
#[allow(unused_imports)]
use crate::sexp::context::RError;
#[allow(unused_imports)]
use crate::sexp::ffi::{
    FALSE, NA_INTEGER, NA_LOGICAL, NA_REAL, R_xlen_t, Rcomplex, SEXP, SEXPTYPE, TRUE,
};
#[allow(unused_imports)]
use crate::sexp::globals::{R_MissingArg, R_NilValue};
#[allow(unused_imports)]
use crate::sexp::protect::protect;
#[allow(unused_imports)]
use crate::sexp::symbol::Rf_install;

// ---------------------------------------------------------------------------
// Locale plumbing (libc setlocale on native; fixed values on wasm32)
// ---------------------------------------------------------------------------

/// Locale category codes forwarded to libc `setlocale(3)` on native targets.
/// The wasm32 sandbox has no locale subsystem, so the glibc numbering is
/// used there as a stable private encoding (the only consumer is the
/// setlocale stub below, which ignores it).
#[cfg(not(target_arch = "wasm32"))]
use libc::{LC_ALL, LC_COLLATE, LC_CTYPE, LC_MESSAGES, LC_MONETARY, LC_NUMERIC, LC_TIME};

// Split from the original single-file module; pure code movement.
mod combine;
mod cut;
mod date;
mod difftime;
mod env;
mod iso;
mod locale;
mod mean_diff;
mod misc;
mod posixct;
mod strftime;
mod sysinfo;

pub use combine::*;
pub use cut::*;
pub use date::*;
pub use difftime::*;
pub use env::*;
pub use iso::*;
pub use locale::*;
pub use mean_diff::*;
pub use misc::*;
pub use posixct::*;
pub use strftime::*;
pub use sysinfo::*;
