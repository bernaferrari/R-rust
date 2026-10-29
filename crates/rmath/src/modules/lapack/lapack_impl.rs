/*
 *  R : A Computer Language for Statistical Data Analysis
 *  Copyright (C) 2001--2025  The R Core Team.
 *
 *  Ported to Rust from R's src/modules/lapack/Lapack.c
 *
 *  Interface routines for LAPACK, callable from R via .Internal.
 */

use std::ffi::CStr;
use std::os::raw::{c_char, c_int};
use std::ptr;

use crate::attrib_core::{
    R_ClassSymbol, R_DimNamesSymbol, R_DimSymbol, R_NamesSymbol, getAttrib, setAttrib,
};

use crate::main::coerce::{asInteger, asLogical, asReal, coerceVector};
use crate::main::errors::Rf_error;
use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::ffi::*;
use crate::sexp::globals::R_NilValue;
use crate::sexp::instance::with_current_instance;
use crate::sexp::memory::with_arena_in;
use crate::sexp::memory_ext::R_alloc;
use crate::sexp::protect::*;

use super::backend;
use super::backend::{
    La_norm_type, La_rcond_type, La_valid_uplo, LapRcomplex, fort_char, fort_str, unscramble_into,
};
use super::workspace_size;

// Local SEXPTYPE constants (as c_int for coerceVector etc.)
const REALSXP_C: c_int = 14;
const INTSXP_C: c_int = 13;
const CPLXSXP_C: c_int = 15;
const STRSXP_C: c_int = 16;
const LGLSXP_C: c_int = 10;
const VECSXP_C: c_int = 19;

mod chol;
mod cond;
mod eigen;
mod misc;
mod qr;
mod solve;
/// La_svd - real singular value decomposition.
///
mod svd;
pub use chol::*;
pub use cond::*;
pub use eigen::*;
pub use misc::*;
pub use qr::*;
pub use solve::*;
pub use svd::*;
