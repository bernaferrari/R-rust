/*
 *  R : A Computer Language for Statistical Data Analysis
 *  Copyright (C) 1998-2016	The R Foundation
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

//! Kernel smoothing
//! Port of r-source/src/library/stats/src/ksmooth.c

use std::ffi::CString;
use std::os::raw::{c_char, c_double, c_int};
use std::ptr;

use crate::attrib_core::{R_NamesSymbol, setAttrib};
use crate::main::coerce::{asInteger, asReal, coerceVector};
use crate::main::errors::Rf_error;
use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::ffi::{NA_REAL, R_xlen_t, SEXP, SEXPTYPE};
use crate::sexp::globals::R_NilValue;
use crate::sexp::protect::{Rf_protect, Rf_unprotect};

// ---------------------------------------------------------------------------
// Local helpers
// ---------------------------------------------------------------------------

unsafe fn error(msg: &str) {
    unsafe {
        let c_msg = CString::new(msg).unwrap_or_default();
        Rf_error(c_msg.as_ptr());
    }
}

unsafe fn Rprintf(fmt: &str) {
    unsafe {
        print!("{}", fmt);
    }
}

unsafe fn mkChar(s: &str) -> SEXP {
    unsafe {
        let c_str = CString::new(s).unwrap_or_default();
        Rf_mkChar(c_str.as_ptr())
    }
}

// ---------------------------------------------------------------------------
// dokern: kernel function
// ---------------------------------------------------------------------------

unsafe fn dokern(x: c_double, kern: c_int) -> c_double {
    unsafe {
        if kern == 1 {
            return 1.0;
        }
        if kern == 2 {
            return (-0.5 * x * x).exp();
        }
        0.0
    }
}

// ---------------------------------------------------------------------------
// BDRksmooth: BDR kernel smoothing
// ---------------------------------------------------------------------------

unsafe fn BDRksmooth(
    x: *const c_double,
    y: *const c_double,
    n: R_xlen_t,
    xp: *const c_double,
    yp: *mut c_double,
    np: R_xlen_t,
    kern: c_int,
    mut bw: c_double,
) {
    unsafe {
        let mut imin: R_xlen_t = 0;
        let mut cutoff: c_double = 0.0;

        // bandwidth is in units of half inter-quartile range.
        if kern == 1 {
            bw *= 0.5;
            cutoff = bw;
        }
        if kern == 2 {
            bw *= 0.3706506;
            cutoff = 4.0 * bw;
        }

        while *x.add(imin as usize) < *xp.add(0) - cutoff && imin < n {
            imin += 1;
        }

        for j in 0..np as usize {
            let mut num: c_double = 0.0;
            let mut den: c_double = 0.0;
            let x0 = *xp.add(j);

            let mut i = imin;
            while i < n {
                if *x.add(i as usize) < x0 - cutoff {
                    imin = i;
                } else {
                    if *x.add(i as usize) > x0 + cutoff {
                        break;
                    }
                    let w = dokern((*x.add(i as usize) - x0).abs() / bw, kern);
                    num += w * *y.add(i as usize);
                    den += w;
                }
                i += 1;
            }

            if den > 0.0 {
                *yp.add(j) = num / den;
            } else {
                *yp.add(j) = NA_REAL;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Fortran-callable stubs (called from ppr.f)
// ---------------------------------------------------------------------------

/// Called only from spline() in ./ppr.f
pub unsafe fn bdrsplerr_() {
    unsafe {
        error("only 2500 rows are allowed for sm.method=\"spline\"");
    }
}

pub unsafe fn splineprt_(
    df: *mut c_double,
    gcvpen: *mut c_double,
    ismethod: *mut c_int,
    lambda: *mut c_double,
    edf: *mut c_double,
) {
    unsafe {
        println!(
            "spline(df={:.3}, g.pen={:.6}, ismeth.={:+2}) -> (lambda, edf) = ({:.7}, {:.2})",
            *df, *gcvpen, *ismethod, *lambda, *edf
        );
    }
}

/// Called only from smooth(..., trace=TRUE) in ./ppr.f
pub unsafe fn smoothprt_(
    span: *mut c_double,
    iper: *mut c_int,
    var: *mut c_double,
    cvar: *mut c_double,
) {
    unsafe {
        println!(
            "smooth(span={}, iper={:+2}) -> (var, cvar) = ({}, {})",
            *span, *iper, *var, *cvar
        );
    }
}

// ---------------------------------------------------------------------------
// ksmooth: SEXP interface for kernel smoothing
// ---------------------------------------------------------------------------

pub unsafe fn ksmooth(x: SEXP, y: SEXP, xp: SEXP, skrn: SEXP, sbw: SEXP) -> SEXP {
    unsafe {
        let krn = asInteger(skrn);
        let bw = asReal(sbw);

        let x = Rf_protect(coerceVector(x, SEXPTYPE::REALSXP.as_c_int()));
        let y = Rf_protect(coerceVector(y, SEXPTYPE::REALSXP.as_c_int()));
        let xp = Rf_protect(coerceVector(xp, SEXPTYPE::REALSXP.as_c_int()));

        let nx = XLENGTH(x);
        let np = XLENGTH(xp);
        let yp = Rf_protect(Rf_allocVector(SEXPTYPE::REALSXP, np as c_int));

        BDRksmooth(REAL(x), REAL(y), nx, REAL(xp), REAL(yp), np, krn, bw);

        let ans = Rf_protect(Rf_allocVector(SEXPTYPE::VECSXP, 2));
        SET_VECTOR_ELT(ans, 0, xp);
        SET_VECTOR_ELT(ans, 1, yp);

        let nm = Rf_allocVector(SEXPTYPE::STRSXP, 2);
        setAttrib(ans, R_NamesSymbol(), nm);
        SET_STRING_ELT(nm, 0, mkChar("x"));
        SET_STRING_ELT(nm, 1, mkChar("y"));

        Rf_unprotect(5);
        ans
    }
}
