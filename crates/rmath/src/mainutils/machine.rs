#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Port of R's src/main/machine.c -- machine constant initialization.
//!
//! Implements `Init_R_Machine` which computes `.Machine` platform constants
//! using `machar` and stores them in the specified environment.

use std::os::raw::{c_double, c_int, c_void};

use crate::mainutils::machar;
use crate::sexp::accessors::{SET_STRING_ELT, SET_VECTOR_ELT};
use crate::sexp::constructors::{Rf_ScalarInteger, Rf_ScalarReal, Rf_allocVector, Rf_mkChar};
use crate::sexp::envir::Environment;
use crate::sexp::ffi::SEXP;
use crate::sexp::object::Sexp;
use crate::sexp::protect::protect;

// ---------------------------------------------------------------------------
// SEXPTYPE constants
// ---------------------------------------------------------------------------

const VECSXP_VAL: c_int = 19;
const STRSXP_VAL: c_int = 16;

// ---------------------------------------------------------------------------
// Local wrappers for cross-module functions
// ---------------------------------------------------------------------------

unsafe fn setAttrib(x: SEXP, what: SEXP, val: SEXP) {
    unsafe {
        crate::eval::attrib_core::setAttrib(x, what, val);
    }
}

unsafe fn R_NamesSymbol() -> SEXP {
    unsafe { crate::eval::attrib_core::R_NamesSymbol() }
}

unsafe fn install(name: *const std::os::raw::c_char) -> SEXP {
    unsafe { crate::sexp::symbol::Rf_install(name) }
}

// ---------------------------------------------------------------------------
// R_AccuracyInfo
// ---------------------------------------------------------------------------

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

fn compute_accuracy_info() -> AccuracyInfo {
    let mut info = AccuracyInfo::new();
    unsafe {
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
    }
    info
}

fn define_machine_binding(rho: SEXP, ans: SEXP) {
    let (Some(rho), Some(ans)) = (Sexp::from_raw(rho), Sexp::from_raw(ans)) else {
        return;
    };
    let Ok(env) = Environment::new(rho) else {
        return;
    };
    let Some(machine_symbol) = Sexp::from_raw(unsafe { install(c".Machine".as_ptr()) }) else {
        return;
    };
    let _ = env.define(machine_symbol, ans);
}

// ---------------------------------------------------------------------------
// Init_R_Machine
// ---------------------------------------------------------------------------

/// Initialize the `.Machine` variable in the given environment.
pub unsafe fn Init_R_Machine(_rho: SEXP) {
    unsafe {
        let info = compute_accuracy_info();

        // On most modern platforms, long double == double, so MACH_SIZE = 19
        let mach_size: usize = 19;

        let ans = Rf_allocVector(VECSXP_VAL, mach_size as c_int);
        let _ans_guard = protect(ans);
        let nms = Rf_allocVector(STRSXP_VAL, mach_size as c_int);
        let _nms_guard = protect(nms);

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

        for (i, name) in names.iter().enumerate() {
            SET_STRING_ELT(nms, i as i64, Rf_mkChar(name.as_ptr() as *const _));
        }

        // Install each freshly allocated scalar immediately into the rooted
        // list. Keeping all of them only in a Rust array until later would
        // leave the earlier values unrooted across subsequent allocations.
        SET_VECTOR_ELT(ans, 0, Rf_ScalarReal(info.eps));
        SET_VECTOR_ELT(ans, 1, Rf_ScalarReal(info.epsneg));
        SET_VECTOR_ELT(ans, 2, Rf_ScalarReal(info.xmin));
        SET_VECTOR_ELT(ans, 3, Rf_ScalarReal(info.xmax));
        let integer_values = [
            info.ibeta,
            info.it,
            info.irnd,
            info.ngrd,
            info.machep,
            info.negep,
            info.iexp,
            info.minexp,
            info.maxexp,
            i32::MAX,
            std::mem::size_of::<i64>() as c_int,
            std::mem::size_of::<i64>() as c_int,
            std::mem::size_of::<u128>() as c_int,
            std::mem::size_of::<*const c_void>() as c_int,
            std::mem::size_of::<i64>() as c_int,
        ];
        for (i, value) in integer_values.into_iter().enumerate() {
            SET_VECTOR_ELT(ans, (i + 4) as i64, Rf_ScalarInteger(value));
        }

        setAttrib(ans, R_NamesSymbol(), nms);
        define_machine_binding(_rho, ans);
    }
}

// ---------------------------------------------------------------------------
// Accessors for machine info (used by other modules)
// ---------------------------------------------------------------------------

/// Get the smallest decimal exponent.
pub extern "C" fn R_Dec_min_exponent() -> c_int {
    (compute_accuracy_info().xmin.log10()).floor() as c_int
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use crate::sexp::globals::*;
    use crate::sexp::{envir::Environment, memory};

    use super::*;

    #[test]
    fn test_init_machine_runs() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            Init_R_Machine(R_NilValue());
            // Verify machar ran and set some values
            let info = compute_accuracy_info();
            assert!(info.ibeta != 0);
            assert!(info.eps > 0.0);
            assert!(R_Dec_min_exponent() < 0);
        }
    }

    #[test]
    fn test_init_machine_defines_machine_binding() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let env_raw =
                memory::with_arena(|arena| arena.alloc_node(crate::sexp::ffi::SEXPTYPE::ENVSXP));
            Init_R_Machine(env_raw);

            let env = Environment::new(Sexp::from_raw(env_raw).expect("environment"))
                .expect("environment facade");
            let machine_symbol =
                Sexp::from_raw(install(c".Machine".as_ptr())).expect(".Machine symbol");
            let machine = env
                .find_in_frame(machine_symbol)
                .expect(".Machine lookup")
                .expect(".Machine binding");

            assert_eq!(machine.clone().len(), 19);

            let names = Sexp::from_raw(crate::eval::attrib_core::getAttrib(
                machine.as_raw(),
                crate::eval::attrib_core::R_NamesSymbol(),
            ))
            .expect("names attribute");
            assert_eq!(names.clone().len(), 19);
            assert_eq!(
                names
                    .string_elt(0)
                    .and_then(|name| name.as_str())
                    .expect("first name"),
                "double.eps"
            );
        }
    }
}
