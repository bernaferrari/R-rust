/*
 *  R : A Computer Language for Statistical Data Analysis
 *  Copyright (C) 2023   Torsten Hothorn
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
 */

/**
    Exact Distribution of Two-Sample Permutation Tests
    Streitberg and Rohmel Shift Algorithm

    Ported from r-source/src/library/stats/src/permdist.c
*/
use std::os::raw::{c_double, c_int};

use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::ffi::*;
use crate::sexp::protect::*;

unsafe fn imin2(x: c_int, y: c_int) -> c_int {
    if x < y { x } else { y }
}

pub unsafe fn dpermdist2(x: SEXP, m: SEXP) -> SEXP {
    use crate::main::errors::R_CheckUserInterrupt;
    use crate::main::errors::Rf_error;
    use crate::sexp::ffi::R_FINITE;

    let n = LENGTH(x);
    let iscore_b = INTEGER(x);

    /* optimization according to Streitberg and Rohmel */
    let sum_a = *INTEGER(m);
    let mut sum_b: c_int = 0;
    let start = (n - sum_a) as usize;
    let end = n as usize;
    for idx in start..end {
        sum_b += *iscore_b.add(idx);
    }

    /* initialize H in Algorithm 'Verteilung 3' */
    let sum_bp1 = (sum_b + 1) as usize;
    let h_len = (sum_a as usize + 1) * sum_bp1;
    let mut dH = vec![0.0f64; h_len];
    let dH = dH.as_mut_ptr();

    /* start the shift algorithm with H[0,0] = 1 */
    *dH = 1.0;
    let mut ic: c_int = 10000;
    let mut s_b: c_int = 0;
    for k in 0..(n as usize) {
        let s_a = (k as c_int) + 1;
        s_b += *iscore_b.add(k);
        let min_b = imin2(sum_b, s_b) as usize;
        let i_max = imin2(sum_a, s_a) as usize;
        let mut i = i_max;
        loop {
            let idx = i * sum_bp1;
            let score_k = *iscore_b.add(k) as usize;
            let idx2 = (i - 1) * sum_bp1 - score_k;
            let mut j = min_b;
            loop {
                if j < score_k {
                    break;
                }
                ic -= 1;
                if ic == 0 {
                    R_CheckUserInterrupt();
                    ic = 10000;
                }
                *dH.add(idx + j) += *dH.add(idx2 + j);
                if j == score_k {
                    break;
                }
                j -= 1;
            }
            if i <= 1 {
                break;
            }
            i -= 1;
        }
    }

    let ret = Rf_protect(Rf_allocVector(SEXPTYPE::REALSXP, sum_b));
    let dret = REAL(ret);
    let row_start = (sum_a as usize) * sum_bp1 + 1;
    let mut msum = 0.0;
    for j in 0..(sum_b as usize) {
        if !R_FINITE(*dH.add(row_start + j)) {
            Rf_error(b"overflow error; cannot compute exact distribution\0".as_ptr() as *const libc::c_char);
        }
        *dret.add(j) = *dH.add(row_start + j);
        msum += *dret.add(j);
    }
    if !R_FINITE(msum) || msum == 0.0 {
        Rf_error(b"overflow error; cannot compute exact distribution\0".as_ptr() as *const libc::c_char);
    }
    for j in 0..(sum_b as usize) {
        *dret.add(j) /= msum;
    }

    Rf_unprotect(1);
    ret
}

pub unsafe fn dpermdist1(x: SEXP) -> SEXP {
    use crate::main::errors::R_CheckUserInterrupt;
    use crate::main::errors::Rf_error;
    use crate::sexp::ffi::R_FINITE;

    let n = LENGTH(x);
    let iscores = INTEGER(x);

    let mut sum_a: c_int = 0;
    for i in 0..(n as usize) {
        sum_a += *iscores.add(i);
    }

    let ret = Rf_protect(Rf_allocVector(SEXPTYPE::REALSXP, sum_a + 1));
    let dH = REAL(ret);
    for i in 0..=(sum_a as usize) {
        *dH.add(i) = 0.0;
    }

    *dH = 1.0;
    let mut ic: c_int = 10000;
    let mut s_a: c_int = 0;
    for k in 0..(n as usize) {
        s_a += *iscores.add(k);
        let score_k = *iscores.add(k) as usize;
        let mut i = s_a as usize;
        loop {
            if i < score_k {
                break;
            }
            ic -= 1;
            if ic == 0 {
                R_CheckUserInterrupt();
                ic = 10000;
            }
            *dH.add(i) += *dH.add(i - score_k);
            if i == score_k {
                break;
            }
            i -= 1;
        }
    }

    let mut msum = 0.0;
    for i in 0..=(sum_a as usize) {
        if !R_FINITE(*dH.add(i)) {
            Rf_error(b"overflow error: cannot compute exact distribution\0".as_ptr() as *const libc::c_char);
        }
        msum += *dH.add(i);
    }
    if !R_FINITE(msum) || msum == 0.0 {
        Rf_error(b"overflow error: cannot compute exact distribution\0".as_ptr() as *const libc::c_char);
    }

    for i in 0..=(sum_a as usize) {
        *dH.add(i) /= msum;
    }

    Rf_unprotect(1);
    ret
}
