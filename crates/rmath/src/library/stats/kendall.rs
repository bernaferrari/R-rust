/*
 *  R : A Computer Language for Statistical Data Analysis
 *  Copyright (C) 1999-2024   The R Core Team.
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

//! Kendall's rank correlation tau and its exact distribution in case of no ties
//! Port of r-source/src/library/stats/src/kendall.c

use std::os::raw::{c_double, c_int};
use std::slice;

use crate::main::coerce::{asInteger, coerceVector};
use crate::nmath::special::gamma::gammafn;
use crate::sexp::accessors::*;
use crate::sexp::constructors::Rf_allocVector;
use crate::sexp::ffi::{SEXP, SEXPTYPE};
use crate::sexp::protect::protect as protect_sexp;

// ---------------------------------------------------------------------------
// ckendall: recursive computation of exact Kendall distribution
// ---------------------------------------------------------------------------

type MemoTable = Vec<Option<Vec<c_double>>>;

fn ckendall(k: c_int, n: c_int, w: &mut MemoTable) -> c_double {
    let u = n * (n - 1) / 2;

    if k < 0 || k > u {
        return 0.0;
    }

    let n_idx = n as usize;
    if w[n_idx].is_none() {
        w[n_idx] = Some(vec![-1.0; (u + 1) as usize]);
    }

    if w[n_idx].as_ref().expect("memo table slot must exist")[k as usize] < 0.0 {
        if n == 1 {
            w[n_idx].as_mut().expect("memo table slot must exist")[k as usize] =
                if k == 0 { 1.0 } else { 0.0 };
        } else {
            let mut s: c_double = 0.0;
            for i in 0..n {
                s += ckendall(k - i, n - 1, w);
            }
            w[n_idx].as_mut().expect("memo table slot must exist")[k as usize] = s;
        }
    }

    w[n_idx].as_ref().expect("memo table slot must exist")[k as usize]
}

// ---------------------------------------------------------------------------
// pkendall: cumulative distribution function for Kendall's tau
// ---------------------------------------------------------------------------

fn pkendall(len: c_int, q: &[c_double], p: &mut [c_double], n: c_int) {
    let mut w: MemoTable = vec![None; (n + 1) as usize];

    for (i, qi) in q.iter().copied().enumerate().take(len as usize) {
        let qv = (qi + 1e-7).floor() as c_int;

        if qv < 0 {
            p[i] = 0.0;
        } else if qv > n * (n - 1) / 2 {
            p[i] = 1.0;
        } else {
            let mut pv: c_double = 0.0;
            for j in 0..=qv {
                pv += ckendall(j, n, &mut w);
            }
            p[i] = pv / gammafn((n + 1) as f64);
        }
    }
}

// ---------------------------------------------------------------------------
// pKendall: SEXP interface
// ---------------------------------------------------------------------------

pub unsafe fn pKendall(q: SEXP, sn: SEXP) -> SEXP {
    let q = unsafe { coerceVector(q, SEXPTYPE::REALSXP.as_c_int()) };
    let _q_guard = protect_sexp(q);
    let len = unsafe { LENGTH(q) };
    let n = unsafe { asInteger(sn) };
    let p = unsafe { Rf_allocVector(SEXPTYPE::REALSXP, len) };
    let _p_guard = protect_sexp(p);
    let q_slice = unsafe { slice::from_raw_parts(REAL(q), len as usize) };
    let p_slice = unsafe { slice::from_raw_parts_mut(REAL(p), len as usize) };

    pkendall(len, q_slice, p_slice, n);

    p
}
