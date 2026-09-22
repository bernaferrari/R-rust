//! GNU `stats/src/nls.c` `numeric_deriv`.
//!
//! Variables are duplicated into a child environment before the step, so the
//! caller's bindings are not mutated (PR#15849).

use crate::sexp::accessors::{CHAR, REAL, STRING_ELT, TYPEOF, XLENGTH};
use crate::sexp::envir::{R_findVar, defineVar};
use crate::sexp::ffi::{SEXP, SEXPTYPE};
use crate::sexp::globals::{R_NilValue, R_UnboundValue};
use crate::sexp::protect::protect;

pub unsafe extern "C-unwind" fn c_numeric_deriv(
    expr: SEXP,
    theta: SEXP,
    rho: SEXP,
    dir: SEXP,
    eps_: SEXP,
    centr: SEXP,
) -> SEXP {
    unsafe {
        if TYPEOF(theta) != SEXPTYPE::STRSXP {
            crate::main::errors::Rf_error(
                b"'theta' should be of type character\0".as_ptr() as *const std::os::raw::c_char,
            );
        }
        if TYPEOF(rho) != SEXPTYPE::ENVSXP {
            crate::main::errors::Rf_error(
                b"'rho' should be an environment\0".as_ptr() as *const std::os::raw::c_char,
            );
        }
        let dir = if TYPEOF(dir) == SEXPTYPE::REALSXP {
            dir
        } else {
            crate::main::coerce::coerceVector(dir, SEXPTYPE::REALSXP.as_c_int())
        };
        let _dir = protect(dir);
        if XLENGTH(dir) != XLENGTH(theta) {
            crate::main::errors::Rf_error(
                b"'dir' is not a numeric vector of the correct length\0".as_ptr()
                    as *const std::os::raw::c_char,
            );
        }
        let central = crate::mainutils::coerce::asLogical(centr) != 0;
        let rho1 = crate::sexp::memory_ext::NewEnvironment(R_NilValue(), rho, R_NilValue());
        let _rho1 = protect(rho1);
        let ntheta = XLENGTH(theta);
        let mut copies = Vec::with_capacity(ntheta as usize);
        let mut total = 0i64;
        for i in 0..ntheta {
            let name = std::ffi::CStr::from_ptr(CHAR(STRING_ELT(theta, i)))
                .to_string_lossy()
                .into_owned();
            let cname = std::ffi::CString::new(name.as_str()).unwrap_or_default();
            let sym = crate::sexp::symbol::Rf_install(cname.as_ptr());
            let found = R_findVar(sym, rho);
            if found == R_UnboundValue() || TYPEOF(found) != SEXPTYPE::REALSXP {
                crate::main::errors::Rf_error(
                    b"variable is not numeric\0".as_ptr() as *const std::os::raw::c_char,
                );
            }
            let copy = crate::mainutils::duplicate::duplicate(found);
            let _c = protect(copy);
            defineVar(sym, copy, rho1);
            total += XLENGTH(copy);
            copies.push(copy);
        }
        let ans0 = crate::eval::eval::Rf_eval(expr, rho1);
        let _a0 = protect(ans0);
        let ans = if TYPEOF(ans0) == SEXPTYPE::REALSXP {
            crate::mainutils::duplicate::duplicate(ans0)
        } else {
            crate::main::coerce::coerceVector(ans0, SEXPTYPE::REALSXP.as_c_int())
        };
        let _ans = protect(ans);
        let nans = XLENGTH(ans);
        let eps = crate::mainutils::coerce::asReal(eps_);
        let gradient = crate::mainutils::array::allocMatrix(
            SEXPTYPE::REALSXP.as_c_int(),
            nans as i32,
            total as i32,
        );
        let _g = protect(gradient);
        let mut start = 0i64;
        for i in 0..ntheta {
            let pars = copies[i as usize];
            let npar = XLENGTH(pars);
            let direction = *REAL(dir).add(i as usize);
            for j in 0..npar {
                let orig = *REAL(pars).add(j as usize);
                let xx = orig.abs();
                let delta = if xx == 0.0 { eps } else { xx * eps };
                *REAL(pars).add(j as usize) = orig + direction * delta;
                let forward = evaluated_real(expr, rho1);
                if central {
                    *REAL(pars).add(j as usize) = orig - direction * delta;
                    let backward = evaluated_real(expr, rho1);
                    for k in 0..nans {
                        *REAL(gradient).add((start + k) as usize) = direction
                            * (*REAL(forward).add(k as usize) - *REAL(backward).add(k as usize))
                            / (2.0 * delta);
                    }
                } else {
                    for k in 0..nans {
                        *REAL(gradient).add((start + k) as usize) = direction
                            * (*REAL(forward).add(k as usize) - *REAL(ans).add(k as usize))
                            / delta;
                    }
                }
                *REAL(pars).add(j as usize) = orig;
                start += nans;
            }
        }
        crate::sexp::attrib_core::setAttrib(
            ans,
            crate::sexp::symbol::Rf_install(c"gradient".as_ptr()),
            gradient,
        );
        ans
    }
}

unsafe fn evaluated_real(expr: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let value = crate::eval::eval::Rf_eval(expr, rho);
        let _v = protect(value);
        if TYPEOF(value) == SEXPTYPE::REALSXP {
            value
        } else {
            crate::main::coerce::coerceVector(value, SEXPTYPE::REALSXP.as_c_int())
        }
    }
}
