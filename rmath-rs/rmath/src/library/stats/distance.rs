/*
 *  R : A Computer Language for Statistical Data Analysis
 *  Copyright (C) 1998-2017  The R Core Team
 *  Copyright (C) 2002-2017  The R Foundation
 *  Copyright (C) 1995, 1996 Robert Gentleman and Ross Ihaka
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
 *  Ported from r-source/src/library/stats/src/distance.c
 */

use std::os::raw::{c_double, c_int};
use std::slice;

use crate::attrib_core::{R_DimSymbol, R_NamesSymbol, getAttrib, setAttrib};
use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::ffi::*;
use crate::sexp::protect::protect as protect_sexp;

const EUCLIDEAN: c_int = 1;
const MAXIMUM: c_int = 2;
const MANHATTAN: c_int = 3;
const CANBERRA: c_int = 4;
const BINARY: c_int = 5;
const MINKOWSKI: c_int = 6;

fn both_non_na(a: c_double, b: c_double) -> bool {
    !a.is_nan() && !b.is_nan()
}

fn both_finite(a: c_double, b: c_double) -> bool {
    a.is_finite() && b.is_finite()
}

fn R_euclidean(x: &[c_double], nr: c_int, nc: c_int, i1: c_int, i2: c_int) -> c_double {
    let mut count: c_int = 0;
    let mut dist: c_double = 0.0;
    let mut idx1 = i1 as usize;
    let mut idx2 = i2 as usize;

    for _ in 0..(nc as usize) {
        let a = x[idx1];
        let b = x[idx2];
        if both_non_na(a, b) {
            let dev = a - b;
            if !dev.is_nan() {
                dist += dev * dev;
                count += 1;
            }
        }
        idx1 += nr as usize;
        idx2 += nr as usize;
    }
    if count == 0 {
        return NA_REAL;
    }
    if count != nc {
        dist /= (count as c_double) / (nc as c_double);
    }
    dist.sqrt()
}

fn R_maximum(x: &[c_double], nr: c_int, nc: c_int, i1: c_int, i2: c_int) -> c_double {
    let mut count: c_int = 0;
    let mut dist: c_double = f64::NEG_INFINITY; // -DBL_MAX
    let mut idx1 = i1 as usize;
    let mut idx2 = i2 as usize;

    for _ in 0..(nc as usize) {
        let a = x[idx1];
        let b = x[idx2];
        if both_non_na(a, b) {
            let dev = (a - b).abs();
            if !dev.is_nan() {
                if dev > dist {
                    dist = dev;
                }
                count += 1;
            }
        }
        idx1 += nr as usize;
        idx2 += nr as usize;
    }
    if count == 0 {
        return NA_REAL;
    }
    dist
}

fn R_manhattan(x: &[c_double], nr: c_int, nc: c_int, i1: c_int, i2: c_int) -> c_double {
    let mut count: c_int = 0;
    let mut dist: c_double = 0.0;
    let mut idx1 = i1 as usize;
    let mut idx2 = i2 as usize;

    for _ in 0..(nc as usize) {
        let a = x[idx1];
        let b = x[idx2];
        if both_non_na(a, b) {
            let dev = (a - b).abs();
            if !dev.is_nan() {
                dist += dev;
                count += 1;
            }
        }
        idx1 += nr as usize;
        idx2 += nr as usize;
    }
    if count == 0 {
        return NA_REAL;
    }
    if count != nc {
        dist /= (count as c_double) / (nc as c_double);
    }
    dist
}

fn R_canberra(x: &[c_double], nr: c_int, nc: c_int, i1: c_int, i2: c_int) -> c_double {
    use crate::main::errors::Rf_warning;

    let mut count: c_int = 0;
    let mut dist: c_double = 0.0;
    let mut idx1 = i1 as usize;
    let mut idx2 = i2 as usize;

    for _ in 0..(nc as usize) {
        let a = x[idx1];
        let b = x[idx2];
        if both_non_na(a, b) {
            let sum_val = a.abs() + b.abs();
            let diff = (a - b).abs();
            if sum_val > f64::MIN_POSITIVE || diff > f64::MIN_POSITIVE {
                let mut dev = diff / sum_val;
                if !dev.is_nan() || (!diff.is_finite() && diff == sum_val) {
                    if !diff.is_finite() && diff == sum_val {
                        dev = 1.0;
                    }
                    dist += dev;
                    count += 1;
                }
            }
        }
        idx1 += nr as usize;
        idx2 += nr as usize;
    }
    if count == 0 {
        return NA_REAL;
    }
    if count != nc {
        dist /= (count as c_double) / (nc as c_double);
    }
    dist
}

fn R_dist_binary(x: &[c_double], nr: c_int, nc: c_int, i1: c_int, i2: c_int) -> c_double {
    use crate::main::errors::Rf_warning;

    let mut total: c_int = 0;
    let mut count: c_int = 0;
    let mut dist: c_int = 0;
    let mut idx1 = i1 as usize;
    let mut idx2 = i2 as usize;

    for _ in 0..(nc as usize) {
        let a = x[idx1];
        let b = x[idx2];
        if both_non_na(a, b) {
            if !both_finite(a, b) {
                unsafe {
                    Rf_warning(
                        b"treating non-finite values as NA\0".as_ptr() as *const libc::c_char
                    );
                }
            } else {
                if a != 0.0 || b != 0.0 {
                    count += 1;
                    if !(a != 0.0 && b != 0.0) {
                        dist += 1;
                    }
                }
                total += 1;
            }
        }
        idx1 += nr as usize;
        idx2 += nr as usize;
    }

    if total == 0 {
        return NA_REAL;
    }
    if count == 0 {
        return 0.0;
    }
    dist as c_double / count as c_double
}

fn R_minkowski(
    x: &[c_double],
    nr: c_int,
    nc: c_int,
    i1: c_int,
    i2: c_int,
    p: c_double,
) -> c_double {
    let mut count: c_int = 0;
    let mut dist: c_double = 0.0;
    let mut idx1 = i1 as usize;
    let mut idx2 = i2 as usize;

    for _ in 0..(nc as usize) {
        let a = x[idx1];
        let b = x[idx2];
        if both_non_na(a, b) {
            let dev = a - b;
            if !dev.is_nan() {
                dist += dev.abs().powf(p);
                count += 1;
            }
        }
        idx1 += nr as usize;
        idx2 += nr as usize;
    }
    if count == 0 {
        return NA_REAL;
    }
    if count != nc {
        dist /= (count as c_double) / (nc as c_double);
    }
    dist.powf(1.0 / p)
}

fn R_distance(
    x: &[c_double],
    nr: c_int,
    nc: c_int,
    d: &mut [c_double],
    diag: c_int,
    method: c_int,
    p: c_double,
) {
    let dc = if diag != 0 { 0 } else { 1 };
    let mut ij: usize = 0;

    for j in 0..=(nr as usize) {
        let mut i = j + dc as usize;
        loop {
            if i >= nr as usize {
                break;
            }
            let val = match method {
                EUCLIDEAN => R_euclidean(x, nr, nc, i as c_int, j as c_int),
                MAXIMUM => R_maximum(x, nr, nc, i as c_int, j as c_int),
                MANHATTAN => R_manhattan(x, nr, nc, i as c_int, j as c_int),
                CANBERRA => R_canberra(x, nr, nc, i as c_int, j as c_int),
                BINARY => R_dist_binary(x, nr, nc, i as c_int, j as c_int),
                MINKOWSKI => R_minkowski(x, nr, nc, i as c_int, j as c_int, p),
                _ => NA_REAL,
            };
            d[ij] = val;
            ij += 1;
            i += 1;
        }
    }
}

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

fn nrows(x: SEXP) -> c_int {
    unsafe {
        let d = getAttrib(x, R_DimSymbol());
        if d.is_null() {
            return LENGTH(x);
        }
        *INTEGER(d)
    }
}

fn ncols(x: SEXP) -> c_int {
    unsafe {
        let d = getAttrib(x, R_DimSymbol());
        if d.is_null() {
            return 1;
        }
        if LENGTH(d) >= 2 {
            return *INTEGER(d.add(1));
        }
        1
    }
}

pub unsafe fn Cdist(x: SEXP, smethod: SEXP, attrs: SEXP, p: SEXP) -> SEXP {
    let nr = nrows(x);
    let nc = ncols(x);
    let method = as_integer(smethod);
    let diag: c_int = 0;
    let rp = as_real(p);
    let n_val = (nr as i64 * (nr as i64 - 1) / 2) as c_int;

    let ans = unsafe { Rf_allocVector(SEXPTYPE::REALSXP, n_val) };
    let _ans_guard = protect_sexp(ans);
    let x = unsafe {
        if TYPEOF(x) != SEXPTYPE::REALSXP {
            crate::main::coerce::coerceVector(x, SEXPTYPE::REALSXP.as_c_int())
        } else {
            x
        }
    };
    let _x_guard = protect_sexp(x);
    let x_len = unsafe { LENGTH(x) };
    let x_slice = unsafe { slice::from_raw_parts(REAL(x), x_len as usize) };
    let ans_slice = unsafe { slice::from_raw_parts_mut(REAL(ans), n_val as usize) };

    R_distance(x_slice, nr, nc, ans_slice, diag, method, rp);

    /* tack on attributes */
    let names = unsafe { getAttrib(attrs, R_NamesSymbol()) };
    for i in 0..(unsafe { LENGTH(attrs) } as usize) {
        let name_sexp = unsafe { STRING_ELT(names, i as R_xlen_t) };
        let name_cstr = unsafe { CHAR(name_sexp) };
        let sym = unsafe { crate::sexp::symbol::Rf_install(name_cstr) };
        unsafe { setAttrib(ans, sym, VECTOR_ELT(attrs, i as R_xlen_t)) };
    }

    ans
}
