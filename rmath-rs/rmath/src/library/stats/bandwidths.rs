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
use std::slice;

use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::ffi::*;
use crate::sexp::protect::protect as protect_sexp;

const DELMAX: c_double = 1000.0;

fn as_integer(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() {
            return NA_INTEGER;
        }
        let t = TYPEOF(x);
        if t == SEXPTYPE::INTSXP {
            return *INTEGER(x);
        }
        if t == SEXPTYPE::REALSXP {
            let v = *REAL(x);
            if v.is_nan() || v < c_int::MIN as f64 || v > c_int::MAX as f64 {
                return NA_INTEGER;
            }
            return v as c_int;
        }
        if t == SEXPTYPE::LGLSXP {
            return *INTEGER(x);
        }
        NA_INTEGER
    }
}

fn as_real(x: SEXP) -> c_double {
    unsafe {
        if x.is_null() {
            return NA_REAL;
        }
        let t = TYPEOF(x);
        if t == SEXPTYPE::REALSXP {
            return *REAL(x);
        }
        if t == SEXPTYPE::INTSXP {
            let v = *INTEGER(x);
            if v == NA_INTEGER {
                return NA_REAL;
            }
            return v as c_double;
        }
        NA_REAL
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

pub unsafe fn bw_ucv(sn: SEXP, sd: SEXP, cnt: SEXP, sh: SEXP) -> SEXP {
    let h = as_real(sh);
    let d = as_real(sd);
    let n = as_integer(sn);
    let nbin = unsafe { LENGTH(cnt) };
    let cnt = unsafe { slice::from_raw_parts(REAL(cnt), nbin as usize) };
    unsafe { Rf_ScalarReal(bw_ucv_impl(cnt, n, d, h)) }
}

pub unsafe fn bw_bcv(sn: SEXP, sd: SEXP, cnt: SEXP, sh: SEXP) -> SEXP {
    let h = as_real(sh);
    let d = as_real(sd);
    let n = as_integer(sn);
    let nbin = unsafe { LENGTH(cnt) };
    let cnt = unsafe { slice::from_raw_parts(REAL(cnt), nbin as usize) };
    unsafe { Rf_ScalarReal(bw_bcv_impl(cnt, n, d, h)) }
}

pub unsafe fn bw_phi4(sn: SEXP, sd: SEXP, cnt: SEXP, sh: SEXP) -> SEXP {
    let h = as_real(sh);
    let d = as_real(sd);
    let n = as_integer(sn);
    let nbin = unsafe { LENGTH(cnt) };
    let cnt = unsafe { slice::from_raw_parts(REAL(cnt), nbin as usize) };
    unsafe { Rf_ScalarReal(bw_phi4_impl(cnt, n, d, h)) }
}

pub unsafe fn bw_phi6(sn: SEXP, sd: SEXP, cnt: SEXP, sh: SEXP) -> SEXP {
    let h = as_real(sh);
    let d = as_real(sd);
    let n = as_integer(sn);
    let nbin = unsafe { LENGTH(cnt) };
    let cnt = unsafe { slice::from_raw_parts(REAL(cnt), nbin as usize) };
    unsafe { Rf_ScalarReal(bw_phi6_impl(cnt, n, d, h)) }
}

pub unsafe fn bw_den(nbin: SEXP, sx: SEXP) -> SEXP {
    use crate::main::errors::Rf_error;

    let nb = as_integer(nbin);
    let n = unsafe { LENGTH(sx) };
    let x = unsafe { slice::from_raw_parts(REAL(sx), n as usize) };

    let mut xmin = f64::INFINITY;
    let mut xmax = f64::NEG_INFINITY;
    for &value in x {
        if !R_FINITE(value) {
            unsafe {
                Rf_error(
                    b"non-finite x[%d] in bandwidth calculation\0".as_ptr() as *const libc::c_char
                );
            }
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
        unsafe {
            Rf_error(
                b"data are constant in bandwidth calculation\0".as_ptr() as *const libc::c_char
            );
        }
    }
    let dd = rang / nb as c_double;

    let ans = unsafe { Rf_allocVector(SEXPTYPE::VECSXP, 2) };
    let _ans_guard = protect_sexp(ans);
    let sc = unsafe { Rf_allocVector(SEXPTYPE::REALSXP, nb as c_int) };
    let _sc_guard = protect_sexp(sc);
    unsafe {
        SET_VECTOR_ELT(ans, 0, Rf_ScalarReal(dd));
        SET_VECTOR_ELT(ans, 1, sc);
    }
    let cnt = unsafe { slice::from_raw_parts_mut(REAL(sc), nb as usize) };
    cnt.fill(0.0);

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

    ans
}

pub unsafe fn bw_den_binned(sx: SEXP) -> SEXP {
    let nb = unsafe { LENGTH(sx) };
    let x = unsafe { slice::from_raw_parts(INTEGER(sx), nb as usize) };

    let ans = unsafe { Rf_allocVector(SEXPTYPE::REALSXP, nb as c_int) };
    let _ans_guard = protect_sexp(ans);
    let cnt = unsafe { slice::from_raw_parts_mut(REAL(ans), nb as usize) };
    cnt.fill(0.0);

    for ii in 0..(nb as usize) {
        let w = x[ii] as c_double; // avoid int overflows below
        cnt[0] += w * (w - 1.0); // don't count distances to self
        for jj in 0..ii {
            let diff = ii - jj;
            cnt[diff] += w * x[jj] as c_double;
        }
    }
    cnt[0] *= 0.5; // counts in the same bin got double-counted

    ans
}
