//! Essentials domain module `strings` — extracted verbatim from essentials.rs.

use super::*;
use std::ffi::CString;
use std::os::raw::c_int;
use std::path::PathBuf;

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
use crate::sexp::ffi::{FALSE, NA_INTEGER, NA_LOGICAL, NA_REAL, R_xlen_t, SEXP, SEXPTYPE, TRUE};
use crate::sexp::globals::R_NilValue;
use crate::sexp::protect::protect;
use crate::sexp::symbol::Rf_install;

mod bits_utf8;
mod encodings;
mod format;
mod gettext;
mod gsub_grep;
mod iconv_codec;
use crate::sexp::encoding::nul_diagnostic as iconv_diagnostic;
mod iconv_owned;
#[cfg(test)]
mod iconv_ownership_tests;
#[cfg(test)]
mod iconv_tests;
mod iconv_translit;
mod make_names;
mod misc;
mod nchar_substr;
mod nclass_bw;
mod pattern;
mod strsplit;
mod strtools;
mod substring_owned;
#[cfg(test)]
mod substring_ownership_tests;
#[cfg(test)]
mod tests;

pub use bits_utf8::*;
pub use encodings::*;
pub use format::*;
pub use gettext::*;
pub use gsub_grep::*;
pub use make_names::*;
pub use misc::*;
pub use nchar_substr::*;
pub use nclass_bw::*;
pub use pattern::*;
pub use strsplit::*;
pub use strtools::*;
