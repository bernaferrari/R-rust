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

use crate::attrib_core::{R_DimSymbol, R_NamesSymbol, getAttrib, setAttrib};
use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::ffi::*;
use crate::sexp::protect::*;

const EUCLIDEAN: c_int = 1;
const MAXIMUM: c_int = 2;
const MANHATTAN: c_int = 3;
const CANBERRA: c_int = 4;
const BINARY: c_int = 5;
const MINKOWSKI: c_int = 6;

unsafe fn both_non_NA(a: c_double, b: c_double) -> bool {
    !ISNAN(a) && !ISNAN(b)
}

unsafe fn both_FINITE(a: c_double, b: c_double) -> bool {
    R_FINITE(a) && R_FINITE(b)
}

unsafe fn R_euclidean(x: *mut c_double, nr: c_int, nc: c_int, i1: c_int, i2: c_int) -> c_double {
    let mut count: c_int = 0;
    let mut dist: c_double = 0.0;
    let mut idx1 = i1 as usize;
    let mut idx2 = i2 as usize;

    for _ in 0..nc {
        if both_non_NA(*x.add(idx1), *x.add(idx2)) {
            let dev = *x.add(idx1) - *x.add(idx2);
            if !ISNAN(dev) {
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

unsafe fn R_maximum(x: *mut c_double, nr: c_int, nc: c_int, i1: c_int, i2: c_int) -> c_double {
    let mut count: c_int = 0;
    let mut dist: c_double = f64::NEG_INFINITY; // -DBL_MAX
    let mut idx1 = i1 as usize;
    let mut idx2 = i2 as usize;

    for _ in 0..nc {
        if both_non_NA(*x.add(idx1), *x.add(idx2)) {
            let dev = (*x.add(idx1) - *x.add(idx2)).abs();
            if !ISNAN(dev) {
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

unsafe fn R_manhattan(x: *mut c_double, nr: c_int, nc: c_int, i1: c_int, i2: c_int) -> c_double {
    let mut count: c_int = 0;
    let mut dist: c_double = 0.0;
    let mut idx1 = i1 as usize;
    let mut idx2 = i2 as usize;

    for _ in 0..nc {
        if both_non_NA(*x.add(idx1), *x.add(idx2)) {
            let dev = (*x.add(idx1) - *x.add(idx2)).abs();
            if !ISNAN(dev) {
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

unsafe fn R_canberra(x: *mut c_double, nr: c_int, nc: c_int, i1: c_int, i2: c_int) -> c_double {
    use crate::main::errors::Rf_warning;

    let mut count: c_int = 0;
    let mut dist: c_double = 0.0;
    let mut idx1 = i1 as usize;
    let mut idx2 = i2 as usize;

    for _ in 0..nc {
        if both_non_NA(*x.add(idx1), *x.add(idx2)) {
            let sum_val = (*x.add(idx1)).abs() + (*x.add(idx2)).abs();
            let diff = (*x.add(idx1) - *x.add(idx2)).abs();
            if sum_val > f64::MIN_POSITIVE || diff > f64::MIN_POSITIVE {
                let mut dev = diff / sum_val;
                if !ISNAN(dev) || (!R_FINITE(diff) && diff == sum_val) {
                    if !R_FINITE(diff) && diff == sum_val {
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

unsafe fn R_dist_binary(x: *mut c_double, nr: c_int, nc: c_int, i1: c_int, i2: c_int) -> c_double {
    use crate::main::errors::Rf_warning;

    let mut total: c_int = 0;
    let mut count: c_int = 0;
    let mut dist: c_int = 0;
    let mut idx1 = i1 as usize;
    let mut idx2 = i2 as usize;

    for _ in 0..nc {
        if both_non_NA(*x.add(idx1), *x.add(idx2)) {
            if !both_FINITE(*x.add(idx1), *x.add(idx2)) {
                Rf_warning(b"treating non-finite values as NA\0".as_ptr() as *const i8);
            } else {
                if *x.add(idx1) != 0.0 || *x.add(idx2) != 0.0 {
                    count += 1;
                    if !(*x.add(idx1) != 0.0 && *x.add(idx2) != 0.0) {
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

unsafe fn R_minkowski(
    x: *mut c_double,
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

    for _ in 0..nc {
        if both_non_NA(*x.add(idx1), *x.add(idx2)) {
            let dev = *x.add(idx1) - *x.add(idx2);
            if !ISNAN(dev) {
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

#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_distance(
    x: *mut c_double,
    nr: *mut c_int,
    nc: *mut c_int,
    d: *mut c_double,
    diag: *mut c_int,
    method: *mut c_int,
    p: *mut c_double,
) {
    let dc = if *diag != 0 { 0 } else { 1 };
    let mut ij: usize = 0;
    let nr_val = *nr;
    let nc_val = *nc;
    let method_val = *method;
    let p_val = *p;

    for j in 0..=(nr_val as usize) {
        let mut i = j + dc as usize;
        loop {
            if i >= nr_val as usize {
                break;
            }
            let val = match method_val {
                EUCLIDEAN => R_euclidean(x, nr_val, nc_val, i as c_int, j as c_int),
                MAXIMUM => R_maximum(x, nr_val, nc_val, i as c_int, j as c_int),
                MANHATTAN => R_manhattan(x, nr_val, nc_val, i as c_int, j as c_int),
                CANBERRA => R_canberra(x, nr_val, nc_val, i as c_int, j as c_int),
                BINARY => R_dist_binary(x, nr_val, nc_val, i as c_int, j as c_int),
                MINKOWSKI => R_minkowski(x, nr_val, nc_val, i as c_int, j as c_int, p_val),
                _ => NA_REAL,
            };
            *d.add(ij) = val;
            ij += 1;
            i += 1;
        }
    }
}

unsafe fn coerceVector(x: SEXP, type_: c_int) -> SEXP {
    crate::main::coerce::coerceVector(x, type_)
}

unsafe fn as_integer(x: SEXP) -> c_int {
    if x.is_null() {
        return NA_INTEGER;
    }
    let t = TYPEOF(x);
    if t == SEXPTYPE::INTSXP.0 {
        return *INTEGER(x);
    }
    if t == SEXPTYPE::REALSXP.0 {
        let v = *REAL(x);
        if v.is_nan() || v < c_int::MIN as f64 || v > c_int::MAX as f64 {
            return NA_INTEGER;
        }
        return v as c_int;
    }
    if t == SEXPTYPE::LGLSXP.0 {
        return *INTEGER(x);
    }
    NA_INTEGER
}

unsafe fn as_real(x: SEXP) -> c_double {
    if x.is_null() {
        return NA_REAL;
    }
    let t = TYPEOF(x);
    if t == SEXPTYPE::REALSXP.0 {
        return *REAL(x);
    }
    if t == SEXPTYPE::INTSXP.0 {
        let v = *INTEGER(x);
        if v == NA_INTEGER {
            return NA_REAL;
        }
        return v as c_double;
    }
    NA_REAL
}

unsafe fn nrows(x: SEXP) -> c_int {
    let d = getAttrib(x, R_DimSymbol());
    if d.is_null() {
        return LENGTH(x);
    }
    *INTEGER(d)
}

unsafe fn ncols(x: SEXP) -> c_int {
    let d = getAttrib(x, R_DimSymbol());
    if d.is_null() {
        return 1;
    }
    if LENGTH(d) >= 2 {
        return *INTEGER(d.add(1));
    }
    1
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn Cdist(x: SEXP, smethod: SEXP, attrs: SEXP, p: SEXP) -> SEXP {
    let nr = nrows(x);
    let nc = ncols(x);
    let method = as_integer(smethod);
    let diag: c_int = 0;
    let rp = as_real(p);
    let n_val = (nr as i64 * (nr as i64 - 1) / 2) as c_int;

    let ans = Rf_protect(Rf_allocVector(SEXPTYPE::REALSXP.0, n_val));
    let x = if TYPEOF(x) != SEXPTYPE::REALSXP.0 {
        coerceVector(x, SEXPTYPE::REALSXP.0)
    } else {
        x
    };
    Rf_protect(x);

    R_distance(
        REAL(x),
        &nr as *const c_int as *mut c_int,
        &nc as *const c_int as *mut c_int,
        REAL(ans),
        &diag as *const c_int as *mut c_int,
        &method as *const c_int as *mut c_int,
        &rp as *const c_double as *mut c_double,
    );

    /* tack on attributes */
    let names = getAttrib(attrs, R_NamesSymbol());
    for i in 0..(LENGTH(attrs) as usize) {
        let name_sexp = STRING_ELT(names, i as R_xlen_t);
        let name_cstr = CHAR(name_sexp);
        let sym = crate::sexp::symbol::Rf_install(name_cstr);
        setAttrib(ans, sym, VECTOR_ELT(attrs, i as R_xlen_t));
    }

    Rf_unprotect(2);
    ans
}
