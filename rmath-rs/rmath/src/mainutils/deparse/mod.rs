#![allow(unused_variables)]
#![allow(unused_assignments)]
#![allow(non_snake_case, non_upper_case_globals, dead_code)]

//! Port of R's src/main/deparse.c (1,962 lines) — deparse, dput, dump.
//!
//! Deparsing has 3 layers:
//! - User interfaces: do_deparse(), do_dput(), do_dump() (should not be called
//!   from internal functions).
//! - The actual deparsing via deparse2() needs to be done twice (once to count
//!   lines, once to fill the string vector), unless nlines > 0.
//! - Printing to a file is handled by the calling routine.
//!
//! Current call paths:
//!   do_deparse() ------------> deparse1WithCutoff()
//!   do_dput() -> deparse1() -> deparse1WithCutoff()
//!   do_dump() -> deparse1() -> deparse1WithCutoff()
//!
//! Workhorse: deparse1WithCutoff() -> deparse2() -> deparse2buff()
//!
//! Porting status:
//! - Full implementation of deparse2buff, deparse2, print2buff, writeline,
//!   linebreak, printtab2buff, args2buff, vector2buff, vec2buff.
//! - deparse1WithCutoff, deparse1, deparse1w, deparse1line, deparse1s, deparse1m.
//! - do_deparse with argument extraction.
//! - Helper functions: curlyahead, needsparens, quotify, etc.
//! - Source reference deparsing kept as a stub (needs eval/methods).
//! - do_dput, do_dump kept as stubs (need connections infrastructure).

use std::os::raw::{c_char, c_int, c_uint};
use std::ptr;

use crate::eval::attrib_core::getAttrib;
use crate::mainutils::memory_main::{R_AllocStringBuffer, R_FreeStringBuffer, R_StringBuffer};
use crate::mainutils::names::{
    PP_ASSIGN as N_PP_ASSIGN, PP_ASSIGN2 as N_PP_ASSIGN2, PP_BINARY as N_PP_BINARY,
    PP_BINARY2 as N_PP_BINARY2, PP_BREAK as N_PP_BREAK, PP_CURLY as N_PP_CURLY,
    PP_DOLLAR as N_PP_DOLLAR, PP_FOR as N_PP_FOR, PP_FOREIGN as N_PP_FOREIGN,
    PP_FUNCALL as N_PP_FUNCALL, PP_FUNCTION as N_PP_FUNCTION, PP_IF as N_PP_IF,
    PP_NEXT as N_PP_NEXT, PP_PAREN as N_PP_PAREN, PP_REPEAT as N_PP_REPEAT,
    PP_RETURN as N_PP_RETURN, PP_SUBASS as N_PP_SUBASS, PP_SUBSET as N_PP_SUBSET,
    PP_UNARY as N_PP_UNARY, PP_WHILE as N_PP_WHILE, PPinfo, PREC_COMPARE as N_PREC_COMPARE,
    PREC_PERCENT as N_PREC_PERCENT, PREC_SIGN as N_PREC_SIGN, PREC_SUBSET as N_PREC_SUBSET,
    PREC_SUM as N_PREC_SUM,
};
use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::ffi::{ISNAN, NA_INTEGER, R_FINITE, R_xlen_t, Rbyte, Rcomplex, SEXP, SEXPTYPE};
use crate::sexp::globals::*;
use crate::sexp::protect::*;
use crate::sexp::symbol::R_BraceSymbol;
use crate::sexp::symbol::R_DotsSymbol;
use crate::sexp::symbol::Rf_install;

pub mod attrs;
pub mod buffer;
pub mod consts;
pub mod dispatch;
pub mod elements;
pub mod entry;
pub mod local_parse_data;
pub mod predicates;
pub mod srcref;

pub use attrs::*;
pub use buffer::*;
pub use consts::*;
pub use dispatch::*;
pub use elements::*;
pub use entry::*;
pub use local_parse_data::*;
pub use predicates::*;
pub use srcref::*;

#[cfg(test)]
mod tests;
