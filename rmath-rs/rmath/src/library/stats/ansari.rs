/*
 *  R : A Computer Language for Statistical Data Analysis
 *  Copyright (C) 1999-2024  The R Core Team
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
 *  Ported from r-source/src/library/stats/src/ansari.c
 *
 *  ansari.c
 *  Compute the exact distribution of the Ansari-Bradley test statistic.
 */

use std::os::raw::{c_double, c_int};
use std::slice;

use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::ffi::*;
use crate::sexp::protect::protect as protect_sexp;

type MemoTable = Vec<Vec<Option<Vec<f64>>>>;

fn w_init(m: c_int, n: c_int) -> MemoTable {
    vec![vec![None; (n + 1) as usize]; (m + 1) as usize]
}

fn cansari(k: c_int, m: c_int, n: c_int, w: &mut MemoTable) -> f64 {
    let l = (m + 1) * (m + 1) / 4;
    let u = l + m * n / 2;

    if k < l || k > u {
        return 0.0;
    }

    let m_idx = m as usize;
    let n_idx = n as usize;
    if w[m_idx][n_idx].is_none() {
        w[m_idx][n_idx] = Some(vec![-1.0; (u + 1) as usize]);
    }
    if w[m_idx][n_idx]
        .as_ref()
        .expect("memo table slot must exist")[k as usize]
        < 0.0
    {
        let val = if m == 0 {
            if k == 0 { 1.0 } else { 0.0 }
        } else if n == 0 {
            if k == l { 1.0 } else { 0.0 }
        } else {
            cansari(k, m, n - 1, w) + cansari(k - (m + n + 1) / 2, m - 1, n, w)
        };
        w[m_idx][n_idx]
            .as_mut()
            .expect("memo table slot must exist")[k as usize] = val;
    }

    w[m_idx][n_idx]
        .as_ref()
        .expect("memo table slot must exist")[k as usize]
}

fn pansari(len: c_int, q: &[f64], p: &mut [f64], m: c_int, n: c_int) {
    let l = (m + 1) * (m + 1) / 4;
    let u = l + m * n / 2;
    let c_val = crate::nmath::special::choose::choose((m + n) as f64, m as f64);

    let mut w = w_init(m, n);

    for (i, qv) in q.iter().copied().enumerate().take(len as usize) {
        let q_floor = qv.floor();
        if q_floor < l as f64 {
            p[i] = 0.0;
        } else if q_floor > u as f64 {
            p[i] = 1.0;
        } else {
            let mut sum = 0.0;
            let mut j = l;
            loop {
                sum += cansari(j, m, n, &mut w);
                if j > q_floor as c_int {
                    break;
                }
                j += 1;
            }
            p[i] = sum / c_val;
        }
    }
}

fn qansari(len: c_int, p: &[f64], q: &mut [f64], m: c_int, n: c_int) {
    use crate::main::errors::Rf_error;

    let l = (m + 1) * (m + 1) / 4;
    let u = l + m * n / 2;
    let c_val = crate::nmath::special::choose::choose((m + n) as f64, m as f64);

    let mut w = w_init(m, n);

    for (i, xi) in p.iter().copied().enumerate().take(len as usize) {
        if xi < 0.0 || xi > 1.0 {
            unsafe {
                Rf_error(
                    b"probabilities outside [0,1] in qansari()\0".as_ptr() as *const libc::c_char
                );
            }
        }
        if xi == 0.0 {
            q[i] = l as f64;
        } else if xi == 1.0 {
            q[i] = u as f64;
        } else {
            let mut psum = 0.0;
            let mut qv: c_int = 0;
            loop {
                psum += cansari(qv, m, n, &mut w) / c_val;
                if psum >= xi {
                    break;
                }
                qv += 1;
            }
            q[i] = qv as f64;
        }
    }
}

pub unsafe fn pAnsari(q: SEXP, sm: SEXP, sn: SEXP) -> SEXP {
    let m = as_integer(sm);
    let n = as_integer(sn);
    let q = unsafe { crate::main::coerce::coerceVector(q, SEXPTYPE::REALSXP.as_c_int()) };
    let _q_guard = protect_sexp(q);
    let len = unsafe { LENGTH(q) };
    let p = unsafe { Rf_allocVector(SEXPTYPE::REALSXP, len as c_int) };
    let _p_guard = protect_sexp(p);
    let q_slice = unsafe { slice::from_raw_parts(REAL(q), len as usize) };
    let p_slice = unsafe { slice::from_raw_parts_mut(REAL(p), len as usize) };
    pansari(len as c_int, q_slice, p_slice, m, n);
    p
}

pub unsafe fn qAnsari(p: SEXP, sm: SEXP, sn: SEXP) -> SEXP {
    let m = as_integer(sm);
    let n = as_integer(sn);
    let p = unsafe { crate::main::coerce::coerceVector(p, SEXPTYPE::REALSXP.as_c_int()) };
    let _p_guard = protect_sexp(p);
    let len = unsafe { LENGTH(p) };
    let q = unsafe { Rf_allocVector(SEXPTYPE::REALSXP, len as c_int) };
    let _q_guard = protect_sexp(q);
    let p_slice = unsafe { slice::from_raw_parts(REAL(p), len as usize) };
    let q_slice = unsafe { slice::from_raw_parts_mut(REAL(q), len as usize) };
    qansari(len as c_int, p_slice, q_slice, m, n);
    q
}

/// Helper: asInteger (local to this module)
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
