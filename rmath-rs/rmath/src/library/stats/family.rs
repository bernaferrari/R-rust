#![allow(
    dead_code,
    unused_imports,
    unused_variables,
    unused_assignments,
    non_camel_case_types,
    unsafe_op_in_unsafe_fn
)]

/*
 *  R : A Computer Language for Statistical Data Analysis
 *  Copyright (C) 2005-2025  The R Core Team
 *
 *  This program is free software; you can redistribute it and/or modify
 *  it under the terms of the GNU General Public License as published by
 *  the Free Software Foundation; either version 2 of the License, or
 *  (at your option) any later version.
 *
 *  This program is distributed in the hope that it will be useful,
 *  but WITHOUT ANY WARRANTY; without even the implied warranty of
 *  MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 *  GNU General Public License for more details.
 *
 *  You should have received a copy of the GNU General Public License
 *  along with this program; if not, a copy is available at
 *  https://www.R-project.org/Licenses/
 *
 *  Ported from r-source/src/library/stats/src/family.c
 */

use std::os::raw::{c_double, c_int};

use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::ffi::*;
use crate::sexp::protect::*;

const THRESH: c_double = 30.0;
const MTHRESH: c_double = -30.0;
const INVEPS: c_double = 1.0 / f64::EPSILON;

unsafe fn coerceVector(x: SEXP, type_: c_int) -> SEXP {
    crate::main::coerce::coerceVector(x, type_)
}

unsafe fn shallow_duplicate(x: SEXP) -> SEXP {
    crate::main::duplicate::shallow_duplicate(x)
}

/// Evaluate x/(1 - x). x must be in the range (0, 1).
#[inline]
unsafe fn x_d_omx(x: c_double) -> c_double {
    if x < 0.0 || x > 1.0 {
        crate::main::errors::Rf_error(b"Value out of range (0, 1)\0".as_ptr() as *const i8);
    }
    x / (1.0 - x)
}

/// Evaluate x/(1 + x).
#[inline]
unsafe fn x_d_opx(x: c_double) -> c_double {
    x / (1.0 + x)
}

pub unsafe fn logit_link(mu: SEXP) -> SEXP {
    let n = LENGTH(mu);
    if n == 0 || TYPEOF(mu) != SEXPTYPE::REALSXP {
        crate::main::errors::Rf_error(
            b"Argument must be a nonempty numeric vector\0".as_ptr() as *const i8
        );
    }
    let ans = Rf_protect(shallow_duplicate(mu));
    let rans = REAL(ans);
    let rmu = REAL(mu);

    for i in 0..(n as usize) {
        *rans.add(i) = x_d_omx(*rmu.add(i)).ln(); // log(x/(1-x))
    }
    Rf_unprotect(1);
    ans
}

pub unsafe fn logit_linkinv(eta: SEXP) -> SEXP {
    let n = LENGTH(eta);
    let mut nprot: c_int = 1;
    if n == 0
        || !(TYPEOF(eta) == SEXPTYPE::REALSXP
            || TYPEOF(eta) == SEXPTYPE::INTSXP
            || TYPEOF(eta) == SEXPTYPE::LGLSXP)
    {
        crate::main::errors::Rf_error(
            b"Argument must be a nonempty numeric vector\0".as_ptr() as *const i8
        );
    }
    let mut eta = eta;
    if TYPEOF(eta) != SEXPTYPE::REALSXP {
        eta = coerceVector(eta, SEXPTYPE::REALSXP.0);
        Rf_protect(eta);
        nprot += 1;
    }
    let ans = Rf_protect(shallow_duplicate(eta));
    let rans = REAL(ans);
    let reta = REAL(eta);

    for i in 0..(n as usize) {
        let etai = *reta.add(i);
        let tmp = if etai < MTHRESH {
            f64::EPSILON
        } else if etai > THRESH {
            INVEPS
        } else {
            etai.exp()
        };
        *rans.add(i) = x_d_opx(tmp);
    }
    Rf_unprotect(nprot);
    ans
}

pub unsafe fn logit_mu_eta(eta: SEXP) -> SEXP {
    let n = LENGTH(eta);
    let mut nprot: c_int = 1;
    if n == 0
        || !(TYPEOF(eta) == SEXPTYPE::REALSXP
            || TYPEOF(eta) == SEXPTYPE::INTSXP
            || TYPEOF(eta) == SEXPTYPE::LGLSXP)
    {
        crate::main::errors::Rf_error(
            b"Argument must be a nonempty numeric vector\0".as_ptr() as *const i8
        );
    }
    let mut eta = eta;
    if TYPEOF(eta) != SEXPTYPE::REALSXP {
        eta = coerceVector(eta, SEXPTYPE::REALSXP.0);
        Rf_protect(eta);
        nprot += 1;
    }
    let ans = Rf_protect(shallow_duplicate(eta));
    let rans = REAL(ans);
    let reta = REAL(eta);

    for i in 0..(n as usize) {
        let etai = *reta.add(i);
        let expE = etai.exp();
        let opexp = 1.0 + expE;
        *rans.add(i) = if etai > THRESH || etai < MTHRESH {
            f64::EPSILON
        } else {
            expE / (opexp * opexp)
        };
    }
    Rf_unprotect(nprot);
    ans
}

/// y * log(y/mu), returning 0 when y == 0.
#[inline]
unsafe fn y_log_y(y: c_double, mu: c_double) -> c_double {
    if y != 0.0 { y * (y / mu).ln() } else { 0.0 }
}

pub unsafe fn binomial_dev_resids(y: SEXP, mu: SEXP, wt: SEXP) -> SEXP {
    let n = LENGTH(y);
    let lmu = LENGTH(mu);
    let lwt = LENGTH(wt);
    let mut nprot: c_int = 1;

    let mut y = y;
    let mut mu = mu;
    let mut wt = wt;

    if TYPEOF(y) != SEXPTYPE::REALSXP {
        y = coerceVector(y, SEXPTYPE::REALSXP.0);
        Rf_protect(y);
        nprot += 1;
    }
    let ry = REAL(y);
    let ans = Rf_protect(shallow_duplicate(y));
    let rans = REAL(ans);

    if TYPEOF(mu) != SEXPTYPE::REALSXP {
        mu = coerceVector(mu, SEXPTYPE::REALSXP.0);
        Rf_protect(mu);
        nprot += 1;
    }
    if TYPEOF(wt) != SEXPTYPE::REALSXP {
        wt = coerceVector(wt, SEXPTYPE::REALSXP.0);
        Rf_protect(wt);
        nprot += 1;
    }
    let rmu = REAL(mu);
    let rwt = REAL(wt);

    if lmu != n && lmu != 1 {
        crate::main::errors::Rf_error(
            b"argument mu must be a numeric vector of length 1 or matching length\0".as_ptr()
                as *const i8,
        );
    }
    if lwt != n && lwt != 1 {
        crate::main::errors::Rf_error(
            b"argument wt must be a numeric vector of length 1 or matching length\0".as_ptr()
                as *const i8,
        );
    }

    if lmu > 1 {
        for i in 0..(n as usize) {
            let mui = *rmu.add(i);
            let yi = *ry.add(i);
            let w = if lwt > 1 { *rwt.add(i) } else { *rwt };
            *rans.add(i) = 2.0 * w * (y_log_y(yi, mui) + y_log_y(1.0 - yi, 1.0 - mui));
        }
    } else {
        let mui = *rmu;
        for i in 0..(n as usize) {
            let yi = *ry.add(i);
            let w = if lwt > 1 { *rwt.add(i) } else { *rwt };
            *rans.add(i) = 2.0 * w * (y_log_y(yi, mui) + y_log_y(1.0 - yi, 1.0 - mui));
        }
    }

    Rf_unprotect(nprot);
    ans
}
