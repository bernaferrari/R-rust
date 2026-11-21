/*
 *  R : A Computer Language for Statistical Data Analysis
 *  bandwidth.c by W. N. Venables and B. D. Ripley  Copyright (C) 1994-2001
 *  Copyright (C) 2012-2025  The R Core Team
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
 *  Ported from r-source/src/library/stats/src/bandwidths.c
 */

use std::os::raw::{c_double, c_int};

use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::ffi::*;
use crate::sexp::object::SexpMut;
use crate::sexp::protect::protect as protect_sexp;

const DELMAX: c_double = 1000.0;

fn checked<T>(result: crate::sexp::object::SexpResult<T>) -> T {
    result.unwrap_or_else(|e| crate::mainutils::essentials::base_error(e.to_string()))
}

// SAFETY: the raw ABI caller retains the active runtime across dispatch; the
// returned token borrows the interpreter-held owner and does not outlive it.
fn current_owner() -> crate::sexp::owner::OwnerToken<'static> {
    crate::library::stats::checked::current_owner()
}

fn as_integer(owner: crate::sexp::owner::OwnerToken<'_>, x: SEXP) -> c_int {
    if x.is_null() {
        return NA_INTEGER;
    }
    let t = TYPEOF(x);
    if t == SEXPTYPE::INTSXP {
        return checked(owner.sexp(x)).integer_elt(0).unwrap_or(NA_INTEGER);
    }
    if t == SEXPTYPE::REALSXP {
        let v = checked(owner.sexp(x)).real_elt(0).unwrap_or(NA_REAL);
        if v.is_nan() || v < c_int::MIN as f64 || v > c_int::MAX as f64 {
            return NA_INTEGER;
        }
        return v as c_int;
    }
    if t == SEXPTYPE::LGLSXP {
        return checked(owner.sexp(x)).logical_elt(0).unwrap_or(NA_INTEGER);
    }
    NA_INTEGER
}

fn as_real(owner: crate::sexp::owner::OwnerToken<'_>, x: SEXP) -> c_double {
    if x.is_null() {
        return NA_REAL;
    }
    let t = TYPEOF(x);
    if t == SEXPTYPE::REALSXP {
        return checked(owner.sexp(x)).real_elt(0).unwrap_or(NA_REAL);
    }
    if t == SEXPTYPE::INTSXP {
        let v = checked(owner.sexp(x)).integer_elt(0).unwrap_or(NA_INTEGER);
        if v == NA_INTEGER {
            return NA_REAL;
        }
        return v as c_double;
    }
    NA_REAL
}

/// Copy a real payload into caller-owned storage via checked element reads.
fn real_vec(owner: crate::sexp::owner::OwnerToken<'_>, x: SEXP) -> Vec<c_double> {
    let value = checked(owner.sexp(x));
    (0..value.len())
        .map(|i| checked(value.try_real_elt(i)))
        .collect()
}

/// Copy an integer payload into caller-owned storage via checked element reads.
fn int_vec(owner: crate::sexp::owner::OwnerToken<'_>, x: SEXP) -> Vec<c_int> {
    let value = checked(owner.sexp(x));
    (0..value.len())
        .map(|i| checked(value.try_integer_elt(i)))
        .collect()
}

/// Write a buffer into a fresh protected real vector via checked element writes.
fn real_out(owner: crate::sexp::owner::OwnerToken<'_>, x: SEXP, values: &[c_double]) {
    let mut out = checked(SexpMut::try_from_checked(checked(owner.sexp(x))));
    for (i, &v) in values.iter().enumerate() {
        checked(out.try_set_real_elt(i as R_xlen_t, v));
    }
}

fn bw_ucv_impl(cnt: &[c_double], n: c_int, d: c_double, h: c_double) -> c_double {
    let mut sum = 0.0;
    for (i, &count) in cnt.iter().enumerate() {
        let mut delta = i as c_double * d / h;
        delta *= delta;
        if delta >= DELMAX {
            break;
        }
        let term = (-delta / 4.0).exp() - (8.0_f64).sqrt() * (-delta / 2.0).exp();
        sum += term * count;
    }
    (0.5 + sum / n as c_double) / (n as c_double * h * std::f64::consts::PI.sqrt())
}

fn bw_bcv_impl(cnt: &[c_double], n: c_int, d: c_double, h: c_double) -> c_double {
    let mut sum = 0.0;
    for (i, &count) in cnt.iter().enumerate() {
        let mut delta = i as c_double * d / h;
        delta *= delta;
        if delta >= DELMAX {
            break;
        }
        let term = (-delta / 4.0).exp() * (delta * delta - 12.0 * delta + 12.0);
        sum += term * count;
    }
    (1.0 + sum / (32.0 * n as c_double)) / (2.0 * n as c_double * h * std::f64::consts::PI.sqrt())
}

fn bw_phi4_impl(cnt: &[c_double], n: c_int, d: c_double, h: c_double) -> c_double {
    let mut sum = 0.0;
    for (i, &count) in cnt.iter().enumerate() {
        let mut delta = i as c_double * d / h;
        delta *= delta;
        if delta >= DELMAX {
            break;
        }
        let term = (-delta / 2.0).exp() * (delta * delta - 6.0 * delta + 3.0);
        sum += term * count;
    }
    sum = 2.0 * sum + n as c_double * 3.0;
    sum / ((n as c_double) * (n as c_double - 1.0) * h.powi(5))
        * (2.0 * std::f64::consts::PI).sqrt().recip()
}

fn bw_phi6_impl(cnt: &[c_double], n: c_int, d: c_double, h: c_double) -> c_double {
    let mut sum = 0.0;
    for (i, &count) in cnt.iter().enumerate() {
        let mut delta = i as c_double * d / h;
        delta *= delta;
        if delta >= DELMAX {
            break;
        }
        let term = (-delta / 2.0).exp()
            * (delta * delta * delta - 15.0 * delta * delta + 45.0 * delta - 15.0);
        sum += term * count;
    }
    sum = 2.0 * sum - 15.0 * n as c_double;
    sum / ((n as c_double) * (n as c_double - 1.0) * h.powi(7))
        * (2.0 * std::f64::consts::PI).sqrt().recip()
}

pub fn bw_ucv(sn: SEXP, sd: SEXP, cnt: SEXP, sh: SEXP) -> SEXP {
    let owner = current_owner();
    let h = as_real(owner, sh);
    let d = as_real(owner, sd);
    let n = as_integer(owner, sn);
    let cnt = real_vec(owner, cnt);
    Rf_ScalarReal(bw_ucv_impl(&cnt, n, d, h))
}

pub fn bw_bcv(sn: SEXP, sd: SEXP, cnt: SEXP, sh: SEXP) -> SEXP {
    let owner = current_owner();
    let h = as_real(owner, sh);
    let d = as_real(owner, sd);
    let n = as_integer(owner, sn);
    let cnt = real_vec(owner, cnt);
    Rf_ScalarReal(bw_bcv_impl(&cnt, n, d, h))
}

pub fn bw_phi4(sn: SEXP, sd: SEXP, cnt: SEXP, sh: SEXP) -> SEXP {
    let owner = current_owner();
    let h = as_real(owner, sh);
    let d = as_real(owner, sd);
    let n = as_integer(owner, sn);
    let cnt = real_vec(owner, cnt);
    Rf_ScalarReal(bw_phi4_impl(&cnt, n, d, h))
}

pub fn bw_phi6(sn: SEXP, sd: SEXP, cnt: SEXP, sh: SEXP) -> SEXP {
    let owner = current_owner();
    let h = as_real(owner, sh);
    let d = as_real(owner, sd);
    let n = as_integer(owner, sn);
    let cnt = real_vec(owner, cnt);
    Rf_ScalarReal(bw_phi6_impl(&cnt, n, d, h))
}

pub fn bw_den(nbin: SEXP, sx: SEXP) -> SEXP {
    use crate::main::errors::{Rf_error, Rf_error_cstr};

    let owner = current_owner();
    let nb = as_integer(owner, nbin);
    let n = LENGTH(sx);
    let x = real_vec(owner, sx);

    let mut xmin = f64::INFINITY;
    let mut xmax = f64::NEG_INFINITY;
    for (i, &value) in x.iter().enumerate() {
        if !R_FINITE(value) {
            let msg =
                std::ffi::CString::new(format!("non-finite x[{}] in bandwidth calculation", i + 1))
                    .unwrap();

            Rf_error_cstr(&msg);
        }
        if value < xmin {
            xmin = value;
        }
        if value > xmax {
            xmax = value;
        }
    }
    let mut rang = (xmax - xmin) * 1.01;
    if rang == 0.0 {
        Rf_error_cstr(
            c"data are constant in bandwidth calculation"
        );
    }
    let dd = rang / nb as c_double;

    let ans = Rf_allocVector(SEXPTYPE::VECSXP, 2);
    let _ans_guard = protect_sexp(ans);
    let sc = Rf_allocVector(SEXPTYPE::REALSXP, nb as c_int);
    let _sc_guard = protect_sexp(sc);

    SET_VECTOR_ELT(ans, 0, Rf_ScalarReal(dd));
    SET_VECTOR_ELT(ans, 1, sc);

    let mut cnt = vec![0.0; nb as usize];

    // Could have a small range very far from 0.
    if xmin / dd < c_int::MIN as c_double || xmax / dd > c_int::MAX as c_double {
        for i in 1..(n as usize) {
            let ii = ((x[i] - xmin) / dd) as c_int;
            for j in 0..i {
                let jj = ((x[j] - xmin) / dd) as c_int;
                let diff = (ii - jj).abs() as usize;
                cnt[diff] += 1.0;
            }
        }
    } else {
        // preserve previous behaviour
        for i in 1..(n as usize) {
            let ii = (x[i] / dd) as c_int;
            for j in 0..i {
                let jj = (x[j] / dd) as c_int;
                let diff = (ii - jj).abs() as usize;
                cnt[diff] += 1.0;
            }
        }
    }

    real_out(owner, sc, &cnt);
    ans
}

pub fn bw_den_binned(sx: SEXP) -> SEXP {
    let owner = current_owner();
    let nb = LENGTH(sx);
    let x = int_vec(owner, sx);

    let ans = Rf_allocVector(SEXPTYPE::REALSXP, nb as c_int);
    let _ans_guard = protect_sexp(ans);
    let mut cnt = vec![0.0; nb as usize];

    for ii in 0..(nb as usize) {
        let w = x[ii] as c_double; // avoid int overflows below
        cnt[0] += w * (w - 1.0); // don't count distances to self
        for jj in 0..ii {
            let diff = ii - jj;
            cnt[diff] += w * x[jj] as c_double;
        }
    }
    cnt[0] *= 0.5; // counts in the same bin got double-counted

    real_out(owner, ans, &cnt);
    ans
}

// SAFETY: registered .Call routine; the interpreter holds the runtime lock
// across dispatch and the callee owns argument validation.
pub unsafe extern "C-unwind" fn c_bw_den(nbin: SEXP, sx: SEXP) -> SEXP {
    bw_den(nbin, sx)
}

// SAFETY: registered .Call routine; the interpreter holds the runtime lock
// across dispatch and the callee owns argument validation.
pub unsafe extern "C-unwind" fn c_bw_den_binned(sx: SEXP) -> SEXP {
    bw_den_binned(sx)
}

// SAFETY: registered .Call routine; the interpreter holds the runtime lock
// across dispatch and the callee owns argument validation.
pub unsafe extern "C-unwind" fn c_bw_ucv(sn: SEXP, sd: SEXP, cnt: SEXP, sh: SEXP) -> SEXP {
    bw_ucv(sn, sd, cnt, sh)
}

// SAFETY: registered .Call routine; the interpreter holds the runtime lock
// across dispatch and the callee owns argument validation.
pub unsafe extern "C-unwind" fn c_bw_bcv(sn: SEXP, sd: SEXP, cnt: SEXP, sh: SEXP) -> SEXP {
    bw_bcv(sn, sd, cnt, sh)
}

// SAFETY: registered .Call routine; the interpreter holds the runtime lock
// across dispatch and the callee owns argument validation.
pub unsafe extern "C-unwind" fn c_bw_phi4(sn: SEXP, sd: SEXP, cnt: SEXP, sh: SEXP) -> SEXP {
    bw_phi4(sn, sd, cnt, sh)
}

// SAFETY: registered .Call routine; the interpreter holds the runtime lock
// across dispatch and the callee owns argument validation.
pub unsafe extern "C-unwind" fn c_bw_phi6(sn: SEXP, sd: SEXP, cnt: SEXP, sh: SEXP) -> SEXP {
    bw_phi6(sn, sd, cnt, sh)
}
