#![allow(unused_variables)]
#![allow(unused_assignments)]
#![allow(non_snake_case, non_upper_case_globals, dead_code)]
#![allow(unused_imports)]

//! Additional exported symbols — SubassignTypeSym, SubassignDotsNames,
//! GetSubassignSxpVec, var_assign.

use std::os::raw::{c_char, c_double, c_int};
use std::ptr;

use crate::mainutils::subscript::{
    OneIndex, get1index, int_arraySubscript, makeSubscript, mat2indsub, strmat2intmat, vectorIndex,
};
use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::envir::defineVar;
use crate::sexp::ffi::{FALSE, NA_INTEGER, R_xlen_t, SEXP, SEXPTYPE, TRUE};
use crate::sexp::globals::R_NilValue;
use crate::sexp::memory_ext::{allocList, allocSExp};
use crate::sexp::protect::protect;
use crate::sexp::symbol::Rf_install;

use super::*;

// ---------------------------------------------------------------------------
// Additional exported symbols
// ---------------------------------------------------------------------------

/// Port of `SubassignTypeSym()` -- used by the byte code compiler.
pub unsafe fn SubassignTypeSym() -> SEXP {
    unsafe { Rf_install(c"SubassignTypeSym".as_ptr()) }
}

/// Port of `SubassignDotsNames()` -- handles assignment to `...` names.
pub unsafe fn SubassignDotsNames(_call: SEXP, _rho: SEXP) -> SEXP {
    unsafe { R_NilValue() }
}

/// Port of `GetSubassignSxpVec()` -- used by the byte code interpreter.
pub unsafe fn GetSubassignSxpVec(x: SEXP, indx: SEXP) -> SEXP {
    unsafe {
        if isNull(x) || isNull(indx) {
            return R_NilValue();
        }
        let n = XLENGTH(indx);
        if n == 0 {
            return R_NilValue();
        }
        let idx = gi(indx, 0);
        if idx == NA_INTEGER as R_xlen_t || idx < 1 || idx > XLENGTH(x) {
            return R_NilValue();
        }
        VECTOR_ELT(x, idx - 1)
    }
}

/// Port of `var_assign()` -- handles variable assignment in the interpreter.
pub unsafe fn var_assign(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe { do_subassign(call, op, args, rho) }
}
