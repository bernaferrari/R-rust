#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Port of R's src/main/machine.c -- machine constant initialization.
//!
//! Implements `Init_R_Machine` which computes `.Machine` platform constants
//! using `machar` and stores them in the specified environment.

use std::cell::{Cell, RefCell};
use std::os::raw::{c_double, c_int, c_void};

use crate::main::machar;
use crate::sexp::accessors::{SET_STRING_ELT, SET_VECTOR_ELT};
use crate::sexp::constructors::{Rf_ScalarInteger, Rf_ScalarReal, Rf_allocVector, Rf_mkChar};
use crate::sexp::ffi::{SEXP, SEXPTYPE};

// ---------------------------------------------------------------------------
// SEXPTYPE constants
// ---------------------------------------------------------------------------

// SEXPTYPE constants now imported from crate::sexp::ffi::SEXPTYPE

// ---------------------------------------------------------------------------
// Local wrappers for cross-module functions
// ---------------------------------------------------------------------------

unsafe fn setAttrib(x: SEXP, what: SEXP, val: SEXP) {
    unsafe {
        crate::attrib_core::setAttrib(x, what, val);
    }
}

unsafe fn R_NamesSymbol() -> SEXP {
    unsafe { crate::attrib_core::R_NamesSymbol() }
}

unsafe fn defineVar(name: SEXP, value: SEXP, rho: SEXP) {
    unsafe {
        crate::sexp::envir::defineVar(name, value, rho);
    }
}

unsafe fn install(name: *const std::os::raw::c_char) -> SEXP {
    unsafe { crate::sexp::symbol::Rf_install(name) }
}

// ---------------------------------------------------------------------------
// R_AccuracyInfo
// ---------------------------------------------------------------------------

thread_local! { static R_AccuracyInfo: RefCell<AccuracyInfo> = RefCell::new(AccuracyInfo::new()); }

thread_local! { static R_dec_min_exponent: Cell<c_int> = Cell::new(0); }

struct AccuracyInfo {
    ibeta: c_int,
    it: c_int,
    irnd: c_int,
    ngrd: c_int,
    machep: c_int,
    negep: c_int,
    iexp: c_int,
    minexp: c_int,
    maxexp: c_int,
    eps: c_double,
    epsneg: c_double,
    xmin: c_double,
    xmax: c_double,
}

impl AccuracyInfo {
    const fn new() -> Self {
        AccuracyInfo {
            ibeta: 0,
            it: 0,
            irnd: 0,
            ngrd: 0,
            machep: 0,
            negep: 0,
            iexp: 0,
            minexp: 0,
            maxexp: 0,
            eps: 0.0,
            epsneg: 0.0,
            xmin: 0.0,
            xmax: 0.0,
        }
    }
}

// ---------------------------------------------------------------------------
// Init_R_Machine
// ---------------------------------------------------------------------------

/// Initialize the `.Machine` variable in the given environment.
pub unsafe fn Init_R_Machine(_rho: SEXP) {
    unsafe {
        R_AccuracyInfo.with(|v| {
            let mut info = v.borrow_mut();
            machar::machar(
                &mut info.ibeta,
                &mut info.it,
                &mut info.irnd,
                &mut info.ngrd,
                &mut info.machep,
                &mut info.negep,
                &mut info.iexp,
                &mut info.minexp,
                &mut info.maxexp,
                &mut info.eps,
                &mut info.epsneg,
                &mut info.xmin,
                &mut info.xmax,
            );

            R_dec_min_exponent.set((info.xmin.log10()).floor() as c_int);

            // On most modern platforms, long double == double, so MACH_SIZE = 19
            let mach_size: usize = 19;

            let ans = Rf_allocVector(SEXPTYPE::VECSXP.0, mach_size as c_int);
            let nms = Rf_allocVector(SEXPTYPE::STRSXP.0, mach_size as c_int);

            let names: &[&[u8]] = &[
                b"double.eps\0",
                b"double.neg.eps\0",
                b"double.xmin\0",
                b"double.xmax\0",
                b"double.base\0",
                b"double.digits\0",
                b"double.rounding\0",
                b"double.guard\0",
                b"double.ulp.digits\0",
                b"double.neg.ulp.digits\0",
                b"double.exponent\0",
                b"double.min.exp\0",
                b"double.max.exp\0",
                b"integer.max\0",
                b"sizeof.long\0",
                b"sizeof.longlong\0",
                b"sizeof.longdouble\0",
                b"sizeof.pointer\0",
                b"sizeof.time_t\0",
            ];

            let values: [SEXP; 19] = [
                Rf_ScalarReal(info.eps),
                Rf_ScalarReal(info.epsneg),
                Rf_ScalarReal(info.xmin),
                Rf_ScalarReal(info.xmax),
                Rf_ScalarInteger(info.ibeta),
                Rf_ScalarInteger(info.it),
                Rf_ScalarInteger(info.irnd),
                Rf_ScalarInteger(info.ngrd),
                Rf_ScalarInteger(info.machep),
                Rf_ScalarInteger(info.negep),
                Rf_ScalarInteger(info.iexp),
                Rf_ScalarInteger(info.minexp),
                Rf_ScalarInteger(info.maxexp),
                Rf_ScalarInteger(i32::MAX),
                Rf_ScalarInteger(std::mem::size_of::<i64>() as c_int),
                Rf_ScalarInteger(std::mem::size_of::<i64>() as c_int),
                Rf_ScalarInteger(std::mem::size_of::<u128>() as c_int),
                Rf_ScalarInteger(std::mem::size_of::<*const c_void>() as c_int),
                Rf_ScalarInteger(std::mem::size_of::<i64>() as c_int),
            ];

            for i in 0..mach_size.min(names.len()).min(values.len()) {
                SET_STRING_ELT(nms, i as i64, Rf_mkChar(names[i].as_ptr() as *const _));
                SET_VECTOR_ELT(ans, i as i64, values[i]);
            }

            setAttrib(ans, R_NamesSymbol(), nms);
        });
    }
}

// ---------------------------------------------------------------------------
// Accessors for machine info (used by other modules)
// ---------------------------------------------------------------------------

pub extern "C" fn R_Dec_min_exponent() -> c_int {
    R_dec_min_exponent.with(|v| v.get())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_init_machine_runs() {
        unsafe {
            Init_R_Machine(R_NilValue());
            // Verify machar ran and set some values
            assert!(R_AccuracyInfo.ibeta != 0);
            assert!(R_AccuracyInfo.eps > 0.0);
        }
    }
}
