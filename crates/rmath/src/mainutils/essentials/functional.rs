//! Essentials domain module `functional` — extracted verbatim from essentials.rs.

use super::*;
use std::collections::BTreeMap;
use std::ffi::{CStr, CString};
use std::os::raw::c_int;

#[allow(unused_imports)]
use crate::sexp::accessors::{
    ATTRIB, CADR, CAR, CDR, CHAR, COMPLEX, FORMALS, FRAME, HASHTAB, INTEGER, INTEGER_ELT, LENGTH,
    LOGICAL, LOGICAL_ELT, PRINTNAME, RAW, REAL, REAL_ELT, SET_ENCLOS, SET_OBJECT, SET_STRING_ELT,
    SET_VECTOR_ELT, SETCAR, SETCDR, SETTAG, STRING_ELT, TAG, TYPEOF, VECTOR_ELT, XLENGTH,
};
#[allow(unused_imports)]
use crate::sexp::constructors::{
    Rf_ScalarInteger, Rf_ScalarLogical, Rf_ScalarReal, Rf_allocVector3, Rf_cons, Rf_lang3,
    Rf_mkChar, Rf_mkString,
};
use crate::sexp::ffi::{
    FALSE, ISNAN, NA_INTEGER, NA_LOGICAL, NA_REAL, R_NA_BIT_PATTERN, R_xlen_t, Rcomplex, SEXP,
    SEXPTYPE, TRUE,
};
use crate::sexp::globals::{R_MissingArg, R_NilValue};
use crate::sexp::protect::protect;
use crate::sexp::symbol::Rf_install;

mod apply;
mod apply_matrix;
mod data_frame;
mod dispatch;
mod do_call;
mod graphics;
mod higher_order;
mod lists;
mod merge;
mod split;
mod sweep;
mod unlist;

#[allow(unused_imports)]
pub use self::apply::*;

#[allow(unused_imports)]
pub use self::do_call::*;

#[allow(unused_imports)]
pub use self::dispatch::*;

#[allow(unused_imports)]
pub use self::apply_matrix::*;

#[allow(unused_imports)]
pub use self::sweep::*;

#[allow(unused_imports)]
pub use self::lists::*;

#[allow(unused_imports)]
pub use self::merge::*;

#[allow(unused_imports)]
pub use self::data_frame::*;

#[allow(unused_imports)]
pub use self::unlist::*;

#[allow(unused_imports)]
pub use self::higher_order::*;

#[allow(unused_imports)]
pub use self::split::*;

#[allow(unused_imports)]
pub use self::graphics::*;
