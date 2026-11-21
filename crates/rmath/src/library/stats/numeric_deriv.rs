//! GNU `stats/src/nls.c` `numeric_deriv`.
//!
//! Variables are duplicated into a child environment before the step, so the
//! caller's bindings are not mutated (PR#15849).

use crate::sexp::accessors::{CHAR, REAL, STRING_ELT, TYPEOF, XLENGTH};
use crate::sexp::accessors::{REAL_ELT, SET_REAL_ELT};
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
                b"'theta' should be of type character\0".as_ptr() as *const std::os::raw::c_char
            );
        }
        if TYPEOF(rho) != SEXPTYPE::ENVSXP {
            crate::main::errors::Rf_error(
                b"'rho' should be an environment\0".as_ptr() as *const std::os::raw::c_char
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
            let name = crate::library::stats::checked::char_string(STRING_ELT(theta, i));
            let cname = std::ffi::CString::new(name.as_str()).unwrap_or_default();
            let sym = crate::sexp::symbol::install_str(cname.to_str().unwrap_or_default());
            let found = R_findVar(sym, rho);
            if found == R_UnboundValue() || TYPEOF(found) != SEXPTYPE::REALSXP {
                crate::main::errors::Rf_error(
                    b"variable is not numeric\0".as_ptr() as *const std::os::raw::c_char
                );
            }
            let copy = crate::mainutils::seq::Rf_duplicate(found);
            let _c = protect(copy);
            defineVar(sym, copy, rho1);
            total += XLENGTH(copy);
            copies.push(copy);
        }
        let ans0 = crate::eval::eval::eval_inner(expr, rho1);
        let _a0 = protect(ans0);
        let ans = if TYPEOF(ans0) == SEXPTYPE::REALSXP {
            crate::mainutils::seq::Rf_duplicate(ans0)
        } else {
            crate::main::coerce::coerceVector(ans0, SEXPTYPE::REALSXP.as_c_int())
        };
        let _ans = protect(ans);
        let nans = XLENGTH(ans);
        let eps = crate::mainutils::coerce::asReal(eps_);
        let gradient = crate::library::stats::checked::alloc_matrix(
            SEXPTYPE::REALSXP.as_c_int(),
            nans as i32,
            total as i32,
        );
        let _g = protect(gradient);
        let mut start = 0i64;
        for i in 0..ntheta {
            let pars = copies[i as usize];
            let npar = XLENGTH(pars);
            let direction = REAL_ELT(dir, i as i32);
            for j in 0..npar {
                let orig = REAL_ELT(pars, j as i32);
                let xx = orig.abs();
                let delta = if xx == 0.0 { eps } else { xx * eps };
                SET_REAL_ELT(pars, j as i32, orig + direction * delta);
                let forward = evaluated_real(expr, rho1);
                if central {
                    SET_REAL_ELT(pars, j as i32, orig - direction * delta);
                    let backward = evaluated_real(expr, rho1);
                    for k in 0..nans {
                        SET_REAL_ELT(
                            gradient,
                            ((start + k) as usize) as i32,
                            direction
                                * (REAL_ELT(forward, k as i32) - REAL_ELT(backward, k as i32))
                                / (2.0 * delta),
                        );
                    }
                } else {
                    for k in 0..nans {
                        SET_REAL_ELT(
                            gradient,
                            ((start + k) as usize) as i32,
                            direction * (REAL_ELT(forward, k as i32) - REAL_ELT(ans, k as i32))
                                / delta,
                        );
                    }
                }
                SET_REAL_ELT(pars, j as i32, orig);
                start += nans;
            }
        }
        crate::sexp::attrib_core::setAttrib(
            ans,
            crate::sexp::symbol::install_str("gradient"),
            gradient,
        );
        ans
    }
}

fn evaluated_real(expr: SEXP, rho: SEXP) -> SEXP {
    let value = crate::eval::eval::eval_inner(expr, rho);
    let _v = protect(value);
    if TYPEOF(value) == SEXPTYPE::REALSXP {
        value
    } else {
        crate::main::coerce::coerceVector(value, SEXPTYPE::REALSXP.as_c_int())
    }
}
