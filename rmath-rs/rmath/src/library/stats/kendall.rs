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
use std::ptr;

use crate::main::coerce::{asInteger, coerceVector};
use crate::nmath::special::gamma::gammafn;
use crate::sexp::accessors::*;
use crate::sexp::constructors::Rf_allocVector;
use crate::sexp::ffi::{SEXP, SEXPTYPE};
use crate::sexp::protect::{Rf_protect, Rf_unprotect};

// ---------------------------------------------------------------------------
// ckendall: recursive computation of exact Kendall distribution
// ---------------------------------------------------------------------------

unsafe fn ckendall(k: c_int, n: c_int, w: *mut *mut c_double) -> c_double {
    let u = n * (n - 1) / 2;

    if k < 0 || k > u {
        return 0.0;
    }

    if (*w.add(n as usize)).is_null() {
        let layout = std::alloc::Layout::array::<c_double>((u + 1) as usize).expect("unwrap on None/Err");
        let ptr = std::alloc::alloc(layout) as *mut c_double;
        if ptr.is_null() {
            std::alloc::handle_alloc_error(layout);
        }
        *w.add(n as usize) = ptr;
        for i in 0..=(u as usize) {
            *ptr.add(i) = -1.0;
        }
    }

    let wn = *w.add(n as usize);
    if *wn.add(k as usize) < 0.0 {
        if n == 1 {
            *wn.add(k as usize) = if k == 0 { 1.0 } else { 0.0 };
        } else {
            let mut s: c_double = 0.0;
            let mut i: c_int = 0;
            while i < n {
                s += ckendall(k - i, n - 1, w);
                i += 1;
            }
            *wn.add(k as usize) = s;
        }
    }

    *wn.add(k as usize)
}

// ---------------------------------------------------------------------------
// pkendall: cumulative distribution function for Kendall's tau
// ---------------------------------------------------------------------------

unsafe fn pkendall(len: c_int, q: *const c_double, p: *mut c_double, n: c_int) {
    let layout = std::alloc::Layout::array::<*mut c_double>((n + 1) as usize).expect("unwrap on None/Err");
    let w = std::alloc::alloc(layout) as *mut *mut c_double;
    if w.is_null() {
        std::alloc::handle_alloc_error(layout);
    }
    for i in 0..=(n as usize) {
        *w.add(i) = ptr::null_mut();
    }

    for i in 0..len as usize {
        let qi = *q.add(i);
        let qv = (qi + 1e-7).floor() as c_int;

        if qv < 0 {
            *p.add(i) = 0.0;
        } else if qv > n * (n - 1) / 2 {
            *p.add(i) = 1.0;
        } else {
            let mut pv: c_double = 0.0;
            let mut j: c_int = 0;
            while j <= qv {
                pv += ckendall(j, n, w);
                j += 1;
            }
            *p.add(i) = pv / gammafn((n + 1) as f64);
        }
    }

    // Free allocated arrays
    for i in 0..=(n as usize) {
        let wn = *w.add(i);
        if !wn.is_null() {
            let u = i * (i - 1) / 2;
            let dealloc_layout = std::alloc::Layout::array::<c_double>((u + 1) as usize).expect("unwrap on None/Err");
            std::alloc::dealloc(wn as *mut u8, dealloc_layout);
        }
    }
    std::alloc::dealloc(w as *mut u8, layout);
}

// ---------------------------------------------------------------------------
// pKendall: SEXP interface
// ---------------------------------------------------------------------------

pub unsafe fn pKendall(q: SEXP, sn: SEXP) -> SEXP {
    let q = Rf_protect(coerceVector(q, SEXPTYPE::REALSXP.0));
    let len = LENGTH(q);
    let n = asInteger(sn);
    let p = Rf_protect(Rf_allocVector(SEXPTYPE::REALSXP.0, len));

    pkendall(len, REAL(q), REAL(p), n);

    Rf_unprotect(2);
    p
}
