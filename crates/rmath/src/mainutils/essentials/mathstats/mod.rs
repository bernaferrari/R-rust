//! Essentials domain module `mathstats` — extracted verbatim from essentials.rs.

use super::*;
use std::ffi::{CStr, CString};
use std::os::raw::c_int;
use std::path::PathBuf;

#[allow(unused_imports)]
use crate::sexp::accessors::{
    ATTRIB, CADR, CAR, CDR, CHAR, COMPLEX, FORMALS, FRAME, HASHTAB, INTEGER, INTEGER_ELT, LENGTH,
    LOGICAL, LOGICAL_ELT, OBJECT, PRINTNAME, RAW, REAL, REAL_ELT, SET_ATTRIB, SET_ENCLOS,
    SET_OBJECT, SET_S4_OBJECT, SET_STRING_ELT, SET_VECTOR_ELT, SETCAR, SETCDR, SETTAG, STRING_ELT,
    TAG, TYPEOF, UNSET_S4_OBJECT, VECTOR_ELT, XLENGTH,
};
#[allow(unused_imports)]
use crate::sexp::constructors::{
    Rf_ScalarInteger, Rf_ScalarLogical, Rf_ScalarReal, Rf_allocVector3, Rf_cons, Rf_mkChar,
    Rf_mkString,
};
use crate::sexp::context::RError;
use crate::sexp::ffi::{ISNAN, NA_INTEGER, NA_REAL, R_xlen_t, Rcomplex, SEXP, SEXPTYPE, TRUE};
use crate::sexp::globals::{R_MissingArg, R_NilValue};
use crate::sexp::protect::protect;
use crate::sexp::symbol::Rf_install;

mod cluster;
mod covscale;
mod ecdf_step;
mod families;
mod files_regex;
mod glm_nls;
mod lm;
mod lm_diag;
mod matchenv;
mod math1;
mod math2;
mod misc_stats;
mod multivariate;
mod stat_tests_a;
mod stat_tests_b;

#[cfg(test)]
mod tests;

pub use cluster::*;
pub use covscale::*;
pub use ecdf_step::*;
pub use families::*;
pub use files_regex::*;
pub use glm_nls::*;
pub use lm::*;
pub use lm_diag::*;
pub use matchenv::*;
pub use math1::*;
pub use math2::*;
pub use misc_stats::*;
pub use multivariate::*;
pub use stat_tests_a::*;
pub use stat_tests_b::*;
