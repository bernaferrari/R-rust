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
use std::ptr;

use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::ffi::*;
use crate::sexp::protect::*;

/// Build a memoization table for the Ansari distribution.
/// Returns a 3-level array: w[m+1][n+1] -> Vec<Vec<Option<Vec<f64>>>>
unsafe fn w_init(m: c_int, n: c_int) -> *mut *mut *mut f64 {
    use std::alloc::{Layout, alloc, dealloc};

    // Allocate array of (m+1) pointers to (n+1) pointers
    let ptr_size = std::mem::size_of::<*mut *mut f64>();
    let outer_layout = Layout::array::<*mut *mut f64>((m + 1) as usize)
        .unwrap_or_else(|_| std::alloc::handle_alloc_error(Layout::new::<*mut *mut f64>()));
    let outer = alloc(outer_layout) as *mut *mut *mut f64;
    if outer.is_null() {
        return ptr::null_mut();
    }
    ptr::write_bytes(outer, 0, (m + 1) as usize);

    for i in 0..=(m as usize) {
        let mid_layout = Layout::array::<*mut f64>((n + 1) as usize)
            .unwrap_or_else(|_| std::alloc::handle_alloc_error(Layout::new::<*mut f64>()));
        let mid = alloc(mid_layout) as *mut *mut f64;
        if mid.is_null() {
            // cleanup on failure - simplified
            return ptr::null_mut();
        }
        ptr::write_bytes(mid, 0, (n + 1) as usize);
        *outer.add(i) = mid;
    }

    outer
}

unsafe fn cansari(k: c_int, m: c_int, n: c_int, w: *mut *mut *mut f64) -> f64 {
    let l = (m + 1) * (m + 1) / 4;
    let u = l + m * n / 2;

    if k < l || k > u {
        return 0.0;
    }

    let w_m = *w.add(m as usize);
    if w_m.is_null() {
        return 0.0;
    }
    let w_mn = *w_m.add(n as usize);
    if w_mn.is_null() {
        // allocate the inner array
        let inner_layout = std::alloc::Layout::array::<f64>((u + 1) as usize)
            .unwrap_or_else(|_| std::alloc::handle_alloc_error(std::alloc::Layout::new::<f64>()));
        let inner = std::alloc::alloc(inner_layout) as *mut f64;
        if inner.is_null() {
            return 0.0;
        }
        for i in 0..=(u as usize) {
            *inner.add(i) = -1.0;
        }
        *w_m.add(n as usize) = inner;
    }

    let w_mn = *w_m.add(n as usize);
    if *w_mn.add(k as usize) < 0.0 {
        if m == 0 {
            *w_mn.add(k as usize) = if k == 0 { 1.0 } else { 0.0 };
        } else if n == 0 {
            *w_mn.add(k as usize) = if k == l { 1.0 } else { 0.0 };
        } else {
            let val = cansari(k, m, n - 1, w) + cansari(k - (m + n + 1) / 2, m - 1, n, w);
            *w_mn.add(k as usize) = val;
        }
    }

    *w_mn.add(k as usize)
}

unsafe fn pansari(len: c_int, q: *const f64, p: *mut f64, m: c_int, n: c_int) {
    let l = (m + 1) * (m + 1) / 4;
    let u = l + m * n / 2;
    let c_val = crate::nmath::special::choose::choose((m + n) as f64, m as f64);

    let w = w_init(m, n);

    for i in 0..(len as usize) {
        let qv = *q.add(i);
        let q_floor = qv.floor();
        if q_floor < l as f64 {
            *p.add(i) = 0.0;
        } else if q_floor > u as f64 {
            *p.add(i) = 1.0;
        } else {
            let mut sum = 0.0;
            let mut j = l;
            loop {
                sum += cansari(j, m, n, w);
                if j > q_floor as c_int {
                    break;
                }
                j += 1;
            }
            *p.add(i) = sum / c_val;
        }
    }
}

unsafe fn qansari(len: c_int, p: *const f64, q: *mut f64, m: c_int, n: c_int) {
    use crate::main::errors::Rf_error;

    let l = (m + 1) * (m + 1) / 4;
    let u = l + m * n / 2;
    let c_val = crate::nmath::special::choose::choose((m + n) as f64, m as f64);

    let w = w_init(m, n);

    for i in 0..(len as usize) {
        let xi = *p.add(i);
        if xi < 0.0 || xi > 1.0 {
            Rf_error(b"probabilities outside [0,1] in qansari()\0".as_ptr() as *const i8);
        }
        if xi == 0.0 {
            *q.add(i) = l as f64;
        } else if xi == 1.0 {
            *q.add(i) = u as f64;
        } else {
            let mut psum = 0.0;
            let mut qv: c_int = 0;
            loop {
                psum += cansari(qv, m, n, w) / c_val;
                if psum >= xi {
                    break;
                }
                qv += 1;
            }
            *q.add(i) = qv as f64;
        }
    }
}

unsafe fn coerceVector(x: SEXP, type_: c_int) -> SEXP {
    crate::main::coerce::coerceVector(x, type_)
}

pub unsafe fn pAnsari(q: SEXP, sm: SEXP, sn: SEXP) -> SEXP {
    let m = as_integer(sm);
    let n = as_integer(sn);
    let q = coerceVector(q, SEXPTYPE::REALSXP.0);
    Rf_protect(q);
    let len = LENGTH(q);
    let p = Rf_protect(Rf_allocVector(SEXPTYPE::REALSXP.0, len as c_int));
    pansari(len as c_int, REAL(q), REAL(p), m, n);
    Rf_unprotect(2);
    p
}

pub unsafe fn qAnsari(p: SEXP, sm: SEXP, sn: SEXP) -> SEXP {
    let m = as_integer(sm);
    let n = as_integer(sn);
    let p = coerceVector(p, SEXPTYPE::REALSXP.0);
    Rf_protect(p);
    let len = LENGTH(p);
    let q = Rf_protect(Rf_allocVector(SEXPTYPE::REALSXP.0, len as c_int));
    qansari(len as c_int, REAL(p), REAL(q), m, n);
    Rf_unprotect(2);
    q
}

/// Helper: asInteger (local to this module)
unsafe fn as_integer(x: SEXP) -> c_int {
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
