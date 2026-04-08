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
 *  Copyright (C) 1997-2017   The R Core Team.
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
 *  Ported from r-source/src/library/stats/src/line.c
 */

use std::os::raw::{c_double, c_int};

use crate::attrib_core::{R_NamesSymbol, setAttrib};
use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::ffi::*;
use crate::sexp::globals::*;
use crate::sexp::protect::*;

unsafe fn il(n: c_int, x: c_double) -> c_int {
    ((n as c_double - 1.0) * x).floor() as c_int
}

unsafe fn iu(n: c_int, x: c_double) -> c_int {
    ((n as c_double - 1.0) * x).ceil() as c_int
}

unsafe fn R_rsort(x: *mut c_double, n: c_int) {
    crate::main::sort::R_rsort(x, n);
}

unsafe fn line(
    x: *const c_double,
    y: *const c_double,
    z: *mut c_double,
    w: *mut c_double,
    n: c_int,
    iter: c_int,
    coef: *mut c_double,
) {
    // Copy x -> z (for sorting) and y -> w
    for i in 0..(n as usize) {
        *z.add(i) = *x.add(i);
        *w.add(i) = *y.add(i);
    }
    R_rsort(z, n); /* z = ordered abscissae */

    // x1 := quantile(x, 1/3)
    let tmp1 = *z.add(il(n, 1.0 / 3.0) as usize);
    let tmp2 = *z.add(iu(n, 1.0 / 3.0) as usize);
    let x1 = 0.5 * (tmp1 + tmp2);

    // x2 := quantile(x, 2/3)
    let tmp1 = *z.add(il(n, 2.0 / 3.0) as usize);
    let tmp2 = *z.add(iu(n, 2.0 / 3.0) as usize);
    let x2 = 0.5 * (tmp1 + tmp2);

    // xb := x_L := Median(x[i]; x[i] <= quantile(x, 1/3))
    let mut k: c_int = 0;
    for i in 0..(n as usize) {
        if *x.add(i) <= x1 {
            *z.add(k as usize) = *x.add(i);
            k += 1;
        }
    }
    R_rsort(z, k);
    let xb = 0.5 * (*z.add(il(k, 0.5) as usize) + *z.add(iu(k, 0.5) as usize));

    // xt := x_R := Median(x[i]; x[i] >= quantile(x, 2/3))
    k = 0;
    for i in 0..(n as usize) {
        if *x.add(i) >= x2 {
            *z.add(k as usize) = *x.add(i);
            k += 1;
        }
    }
    R_rsort(z, k);
    let xt = 0.5 * (*z.add(il(k, 0.5) as usize) + *z.add(iu(k, 0.5) as usize));

    let mut slope: c_double = 0.0;
    // "Polishing" iterations
    for _j in 1..=(iter as usize) {
        // yb := Median(y[i]; x[i] <= quantile(x, 1/3))
        k = 0;
        for i in 0..(n as usize) {
            if *x.add(i) <= x1 {
                *z.add(k as usize) = *w.add(i);
                k += 1;
            }
        }
        R_rsort(z, k);
        let yb = 0.5 * (*z.add(il(k, 0.5) as usize) + *z.add(iu(k, 0.5) as usize));

        // yt := Median(y[i]; x[i] >= quantile(x, 2/3))
        k = 0;
        for i in 0..(n as usize) {
            if *x.add(i) >= x2 {
                *z.add(k as usize) = *w.add(i);
                k += 1;
            }
        }
        R_rsort(z, k);
        let yt = 0.5 * (*z.add(il(k, 0.5) as usize) + *z.add(iu(k, 0.5) as usize));

        slope += (yt - yb) / (xt - xb);
        for i in 0..(n as usize) {
            *w.add(i) = *y.add(i) - slope * *x.add(i);
        }
    }

    // intercept := median of residuals
    R_rsort(w, n);
    let yint = 0.5 * (*w.add(il(n, 0.5) as usize) + *w.add(iu(n, 0.5) as usize));

    for i in 0..(n as usize) {
        *w.add(i) = yint + slope * *x.add(i);
        *z.add(i) = *y.add(i) - *w.add(i);
    }
    *coef.add(0) = yint;
    *coef.add(1) = slope;
}

pub unsafe fn tukeyline0(
    x: *mut c_double,
    y: *mut c_double,
    z: *mut c_double,
    w: *mut c_double,
    n: *mut c_int,
    coef: *mut c_double,
) {
    line(x, y, z, w, *n, 1, coef);
}

unsafe fn asInteger(x: SEXP) -> c_int {
    crate::main::coerce::asInteger(x)
}

pub unsafe fn tukeyline(x: SEXP, y: SEXP, iter: SEXP, call: SEXP) -> SEXP {
    use crate::main::errors::Rf_error;

    let n = LENGTH(x);
    if n < 2 {
        Rf_error(b"insufficient observations\0".as_ptr() as *const i8);
    }

    let ans = Rf_protect(Rf_allocVector(SEXPTYPE::VECSXP.0, 4));
    let nm = Rf_allocVector(SEXPTYPE::STRSXP.0, 4);
    setAttrib(ans, R_NamesSymbol(), nm);
    SET_STRING_ELT(nm, 0, Rf_mkChar(b"call\0".as_ptr() as *const i8));
    SET_STRING_ELT(nm, 1, Rf_mkChar(b"coefficients\0".as_ptr() as *const i8));
    SET_STRING_ELT(nm, 2, Rf_mkChar(b"residuals\0".as_ptr() as *const i8));
    SET_STRING_ELT(nm, 3, Rf_mkChar(b"fitted.values\0".as_ptr() as *const i8));
    SET_VECTOR_ELT(ans, 0, call);

    let coef = Rf_allocVector(SEXPTYPE::REALSXP.0, 2);
    SET_VECTOR_ELT(ans, 1, coef);
    let res = Rf_allocVector(SEXPTYPE::REALSXP.0, n);
    SET_VECTOR_ELT(ans, 2, res);
    let fit = Rf_allocVector(SEXPTYPE::REALSXP.0, n);
    SET_VECTOR_ELT(ans, 3, fit);

    line(
        REAL(x),
        REAL(y),
        REAL(res),
        REAL(fit),
        n as c_int,
        asInteger(iter),
        REAL(coef),
    );

    Rf_unprotect(1);
    ans
}
