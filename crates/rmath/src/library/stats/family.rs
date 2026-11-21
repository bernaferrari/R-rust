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

fn coerceVector(x: SEXP, type_: SEXPTYPE) -> SEXP {
    crate::main::coerce::coerceVector(x, type_.into())
}

fn shallow_duplicate(x: SEXP) -> SEXP {
    crate::mainutils::seq::shallow_duplicate(x)
}

/// Evaluate x/(1 - x). x must be in the range (0, 1).
#[inline]
fn x_d_omx(x: c_double) -> c_double {
    if x < 0.0 || x > 1.0 {
        crate::main::errors::Rf_error_cstr(
            c"Value out of range (0, 1)"
        );
    }
    x / (1.0 - x)
}

/// Evaluate x/(1 + x).
#[inline]
fn x_d_opx(x: c_double) -> c_double {
    x / (1.0 + x)
}

pub fn logit_link(mu: SEXP) -> SEXP {
    let n = LENGTH(mu);
    if n == 0 || TYPEOF(mu) != SEXPTYPE::REALSXP {
        crate::main::errors::Rf_error_cstr(
            c"Argument must be a nonempty numeric vector"
        );
    }
    let ans = shallow_duplicate(mu);
    let _ans_guard = protect(ans);

    for i in 0..(n as usize) {
        SET_REAL_ELT(ans, i as i32, x_d_omx(REAL_ELT(mu, i as i32)).ln()); // log(x/(1-x))
    }
    ans
}

pub fn logit_linkinv(eta: SEXP) -> SEXP {
    let n = LENGTH(eta);
    if n == 0
        || !(TYPEOF(eta) == SEXPTYPE::REALSXP
            || TYPEOF(eta) == SEXPTYPE::INTSXP
            || TYPEOF(eta) == SEXPTYPE::LGLSXP)
    {
        crate::main::errors::Rf_error_cstr(
            c"Argument must be a nonempty numeric vector"
        );
    }
    let mut guards = Vec::with_capacity(2);
    let mut eta = eta;
    if TYPEOF(eta) != SEXPTYPE::REALSXP {
        eta = coerceVector(eta, SEXPTYPE::REALSXP);
        guards.push(protect(eta));
    }
    let ans = shallow_duplicate(eta);
    guards.push(protect(ans));

    for i in 0..(n as usize) {
        let etai = REAL_ELT(eta, i as i32);
        let tmp = if etai < MTHRESH {
            f64::EPSILON
        } else if etai > THRESH {
            INVEPS
        } else {
            etai.exp()
        };
        SET_REAL_ELT(ans, i as i32, x_d_opx(tmp));
    }
    ans
}

pub fn logit_mu_eta(eta: SEXP) -> SEXP {
    let n = LENGTH(eta);
    if n == 0
        || !(TYPEOF(eta) == SEXPTYPE::REALSXP
            || TYPEOF(eta) == SEXPTYPE::INTSXP
            || TYPEOF(eta) == SEXPTYPE::LGLSXP)
    {
        crate::main::errors::Rf_error_cstr(
            c"Argument must be a nonempty numeric vector"
        );
    }
    let mut guards = Vec::with_capacity(2);
    let mut eta = eta;
    if TYPEOF(eta) != SEXPTYPE::REALSXP {
        eta = coerceVector(eta, SEXPTYPE::REALSXP);
        guards.push(protect(eta));
    }
    let ans = shallow_duplicate(eta);
    guards.push(protect(ans));

    for i in 0..(n as usize) {
        let etai = REAL_ELT(eta, i as i32);
        let expE = etai.exp();
        let opexp = 1.0 + expE;
        SET_REAL_ELT(
            ans,
            i as i32,
            if etai > THRESH || etai < MTHRESH {
                f64::EPSILON
            } else {
                expE / (opexp * opexp)
            },
        );
    }
    ans
}

/// y * log(y/mu), returning 0 when y == 0.
#[inline]
fn y_log_y(y: c_double, mu: c_double) -> c_double {
    if y != 0.0 { y * (y / mu).ln() } else { 0.0 }
}

pub fn binomial_dev_resids(y: SEXP, mu: SEXP, wt: SEXP) -> SEXP {
    let n = LENGTH(y);
    let lmu = LENGTH(mu);
    let lwt = LENGTH(wt);
    let mut guards = Vec::with_capacity(4);

    let mut y = y;
    let mut mu = mu;
    let mut wt = wt;

    if TYPEOF(y) != SEXPTYPE::REALSXP {
        y = coerceVector(y, SEXPTYPE::REALSXP);
        guards.push(protect(y));
    }
    let ans = shallow_duplicate(y);
    guards.push(protect(ans));

    if TYPEOF(mu) != SEXPTYPE::REALSXP {
        mu = coerceVector(mu, SEXPTYPE::REALSXP);
        guards.push(protect(mu));
    }
    if TYPEOF(wt) != SEXPTYPE::REALSXP {
        wt = coerceVector(wt, SEXPTYPE::REALSXP);
        guards.push(protect(wt));
    }

    if lmu != n && lmu != 1 {
        crate::main::errors::Rf_error_cstr(
            c"argument mu must be a numeric vector of length 1 or matching length");
    }
    if lwt != n && lwt != 1 {
        crate::main::errors::Rf_error_cstr(
            c"argument wt must be a numeric vector of length 1 or matching length");
    }

    if lmu > 1 {
        for i in 0..(n as usize) {
            let mui = REAL_ELT(mu, i as i32);
            let yi = REAL_ELT(y, i as i32);
            let w = if lwt > 1 {
                REAL_ELT(wt, i as i32)
            } else {
                REAL_ELT(wt, 0)
            };
            SET_REAL_ELT(
                ans,
                i as i32,
                2.0 * w * (y_log_y(yi, mui) + y_log_y(1.0 - yi, 1.0 - mui)),
            );
        }
    } else {
        let mui = REAL_ELT(mu, 0);
        for i in 0..(n as usize) {
            let yi = REAL_ELT(y, i as i32);
            let w = if lwt > 1 {
                REAL_ELT(wt, i as i32)
            } else {
                REAL_ELT(wt, 0)
            };
            SET_REAL_ELT(
                ans,
                i as i32,
                2.0 * w * (y_log_y(yi, mui) + y_log_y(1.0 - yi, 1.0 - mui)),
            );
        }
    }

    ans
}

pub unsafe extern "C-unwind" fn c_logit_link(mu: SEXP) -> SEXP {
    logit_link(mu)
}

pub unsafe extern "C-unwind" fn c_logit_linkinv(eta: SEXP) -> SEXP {
    logit_linkinv(eta)
}

pub unsafe extern "C-unwind" fn c_logit_mu_eta(eta: SEXP) -> SEXP {
    logit_mu_eta(eta)
}

pub unsafe extern "C-unwind" fn c_binomial_dev_resids(y: SEXP, mu: SEXP, wt: SEXP) -> SEXP {
    binomial_dev_resids(y, mu, wt)
}
