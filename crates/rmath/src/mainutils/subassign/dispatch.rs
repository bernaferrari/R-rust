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
        use crate::sexp::memory_ext::R_mkEVPROMISE;
        use crate::sexp::object::SessionNodeFactory;
        use crate::sexp::symbol::R_DotsSymbol;

        let factory = SessionNodeFactory::new(
            crate::sexp::owner::OwnerToken::current()
                .unwrap_or_else(|error| crate::sexp::context::r_error(&error.to_string())),
        );
        let args_owner = factory
            .wrap(args)
            .unwrap_or_else(|error| crate::sexp::context::r_error(&error.to_string()));
        let rho_owner = factory
            .wrap(rho)
            .unwrap_or_else(|error| crate::sexp::context::r_error(&error.to_string()));
        let mut args_work = args_owner.clone();

        if !args_owner.is_nil() && CAR(args) != R_DotsSymbol() {
            let expression = args_owner
                .try_car()
                .unwrap_or_else(|error| crate::sexp::context::r_error(&error.to_string()));
            let tail = args_owner
                .try_cdr()
                .unwrap_or_else(|error| crate::sexp::context::r_error(&error.to_string()));
            let x = factory
                .wrap(Rf_eval(expression.as_raw(), rho_owner.as_raw()))
                .unwrap_or_else(|error| crate::sexp::context::r_error(&error.to_string()));
            if !isObject(x.as_raw()) {
                let rest = evalListKeepMissing(tail, rho_owner.clone());
                if !ans.is_null() {
                    let nil = factory.nil();
                    let evaluated = factory
                        .allocate(|arena| Some(arena.cons(x.as_raw(), rest.as_raw(), nil.as_raw())))
                        .unwrap_or_else(|error| crate::sexp::context::r_error(&error.to_string()));
                    *ans = evaluated.as_raw();
                }
                return 0;
            }
            let promise = factory
                .wrap(R_mkEVPROMISE(expression.as_raw(), x.as_raw()))
                .unwrap_or_else(|error| crate::sexp::context::r_error(&error.to_string()));
            let nil = factory.nil();
            args_work = factory
                .allocate(|arena| Some(arena.cons(promise.as_raw(), tail.as_raw(), nil.as_raw())))
                .unwrap_or_else(|error| crate::sexp::context::r_error(&error.to_string()));
        }

        // The owned prefix retains its promise and evaluated value throughout
        // method lookup, fallback argument evaluation and method application.
        DispatchOrEval(
            call,
            op,
            generic,
            args_work.as_raw(),
            rho_owner.as_raw(),
            ans,
            0,
            0,
        )
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
///
/// GNU uses `R_CurrentExpression`. During `xx[[]] <- pi` that is the source
/// assignment, which `applydefine` stores on `eval_state.current_expr`.
pub(crate) unsafe fn errorMissingSubscript(x: SEXP, call: SEXP) {
    unsafe {
        let instance = crate::sexp::instance::with_required_current_instance(|instance| instance);
        let source = (*instance).eval_state.current_expr.owned();
        let call = source.as_ref().map_or(call, |source| source.as_raw());
        crate::mainutils::errors::R_MissingSubscriptError(x, call);
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
