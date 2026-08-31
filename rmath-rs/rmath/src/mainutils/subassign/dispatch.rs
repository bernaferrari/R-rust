#![allow(unused_variables)]
#![allow(unused_assignments)]
#![allow(non_snake_case, non_upper_case_globals, dead_code)]
#![allow(unused_imports)]

//! Dispatch and error paths for `[<-` — R_DispatchOrEvalSP,
//! errorNotSubsettable, errorMissingSubscript, errorOutOfBoundsSEXP.

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
// R_DispatchOrEvalSP
// ---------------------------------------------------------------------------

/// Port of `R_DispatchOrEvalSP()` -- fast-path dispatch/eval for `[<-` and friends.
/// Mirrors subset.c: evaluate first arg, skip dispatch when not an object,
/// otherwise EVPROMISE + `DispatchOrEval`.
pub(crate) unsafe fn R_DispatchOrEvalSP(
    call: SEXP,
    op: SEXP,
    generic: *const c_char,
    args: SEXP,
    rho: SEXP,
    ans: *mut SEXP,
) -> c_int {
    unsafe {
        use crate::eval::dispatch::{DispatchOrEval, evalListKeepMissing};
        use crate::eval::eval::Rf_eval;
        use crate::sexp::memory_ext::{CONS_NR, R_mkEVPROMISE};
        use crate::sexp::symbol::R_DotsSymbol;

        let mut prom: SEXP = ptr::null_mut();
        let mut args_work = args;

        if args != R_NilValue() && CAR(args) != R_DotsSymbol() {
            let x = Rf_eval(CAR(args), rho);
            let _px = protect(x);
            if !isObject(x) {
                let rest = evalListKeepMissing(CDR(args), rho);
                let _pr = protect(rest);
                if !ans.is_null() {
                    *ans = CONS_NR(x, rest);
                }
                return 0;
            }
            prom = R_mkEVPROMISE(CAR(args), x);
            args_work = CONS_NR(prom, CDR(args));
        }

        let _pa = protect(args_work);
        let disp = DispatchOrEval(call, op, generic, args_work, rho, ans, 0, 0);
        let _ = prom;
        disp
    }
}

// ---------------------------------------------------------------------------
// Error helpers
// ---------------------------------------------------------------------------

/// Port of `errorNotSubsettable()` -- signals an error for non-subsettable types.
pub(crate) unsafe fn errorNotSubsettable(x: SEXP) {
    unsafe {
        let t = TYPEOF(x);
        let type_name = crate::mainutils::util_main::type2char(t);
        let s = std::ffi::CStr::from_ptr(type_name).to_string_lossy();
        let msg = format!("object of type '{}' is not subsettable", s);
        let cmsg = std::ffi::CString::new(msg).unwrap_or_default();
        crate::mainutils::errors::Rf_error1(
            b"invalid subscript\0".as_ptr() as *const core::ffi::c_char,
            cmsg.as_ptr(),
        );
        unreachable!()
    }
}

/// Port of `errorMissingSubscript()` -- signals an error for missing subscripts.
pub(crate) unsafe fn errorMissingSubscript(x: SEXP) {
    unsafe {
        let t = TYPEOF(x);
        let type_name = crate::mainutils::util_main::type2char(t);
        let s = std::ffi::CStr::from_ptr(type_name).to_string_lossy();
        let msg = format!("object of type '{}' is missing a subscript", s);
        let cmsg = std::ffi::CString::new(msg).unwrap_or_default();
        crate::mainutils::errors::Rf_error1(
            b"invalid subscript\0".as_ptr() as *const core::ffi::c_char,
            cmsg.as_ptr(),
        );
        unreachable!()
    }
}

/// Port of `errorOutOfBoundsSEXP()` -- signals an out-of-bounds error for [[<-.
pub(crate) unsafe fn errorOutOfBoundsSEXP(x: SEXP, subscript: c_int, _sindex: SEXP) {
    unsafe {
        let t = TYPEOF(x);
        let type_name = crate::mainutils::util_main::type2char(t);
        let s = std::ffi::CStr::from_ptr(type_name).to_string_lossy();
        let msg = format!("subscript out of bounds: type '{}' index {}", s, subscript);
        let cmsg = std::ffi::CString::new(msg).unwrap_or_default();
        crate::mainutils::errors::Rf_error1(
            b"subscript out of bounds\0".as_ptr() as *const core::ffi::c_char,
            cmsg.as_ptr(),
        );
        unreachable!()
    }
}
