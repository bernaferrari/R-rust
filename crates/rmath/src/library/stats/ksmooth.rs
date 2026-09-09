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
//!
//! Core smoother uses borrowed slices; SEXP / Fortran entry points remain
//! `unsafe` and are locked with `deny(unsafe_op_in_unsafe_fn)`.

#![deny(unsafe_op_in_unsafe_fn)]

use std::ffi::CString;
use std::os::raw::{c_double, c_int};
use std::slice;

use crate::attrib_core::{R_NamesSymbol, setAttrib};
use crate::main::coerce::{asInteger, asReal, coerceVector};
use crate::main::errors::Rf_error;
use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::ffi::{NA_REAL, SEXP, SEXPTYPE};
use crate::sexp::protect::protect;

// ---------------------------------------------------------------------------
// Local helpers
// ---------------------------------------------------------------------------

unsafe fn error(msg: &str) {
    unsafe {
        let c_msg = CString::new(msg).unwrap_or_default();
        Rf_error(c_msg.as_ptr());
    }
}

unsafe fn mkChar(s: &str) -> SEXP {
    unsafe {
        let c_str = CString::new(s).unwrap_or_default();
        Rf_mkChar(c_str.as_ptr())
    }
}

// ---------------------------------------------------------------------------
// dokern: kernel function (pure; no unsafe)
// ---------------------------------------------------------------------------

fn dokern(x: c_double, kern: c_int) -> c_double {
    if kern == 1 {
        return 1.0;
    }
    if kern == 2 {
        return (-0.5 * x * x).exp();
    }
    0.0
}

// ---------------------------------------------------------------------------
// BDRksmooth: BDR kernel smoothing (safe slice facade)
// ---------------------------------------------------------------------------

fn BDRksmooth(
    x: &[c_double],
    y: &[c_double],
    xp: &[c_double],
    yp: &mut [c_double],
    kern: c_int,
    mut bw: c_double,
) {
    debug_assert_eq!(x.len(), y.len());
    debug_assert_eq!(xp.len(), yp.len());

    yp.fill(NA_REAL);

    let n = x.len();
    let np = xp.len();
    let mut imin: usize = 0;
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

    if np == 0 || n == 0 {
        return;
    }

    while imin < n && x[imin] < xp[0] - cutoff {
        imin += 1;
    }

    for j in 0..np {
        let mut num: c_double = 0.0;
        let mut den: c_double = 0.0;
        let x0 = xp[j];

        let mut i = imin;
        while i < n {
            if x[i] < x0 - cutoff {
                imin = i;
            } else {
                if x[i] > x0 + cutoff {
                    break;
                }
                let w = dokern((x[i] - x0).abs() / bw, kern);
                num += w * y[i];
                den += w;
            }
            i += 1;
        }

        if den > 0.0 {
            yp[j] = num / den;
        } else {
            yp[j] = NA_REAL;
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

        let x = coerceVector(x, SEXPTYPE::REALSXP.as_c_int());
        let _x_guard = protect(x);
        let y = coerceVector(y, SEXPTYPE::REALSXP.as_c_int());
        let _y_guard = protect(y);
        let xp = coerceVector(xp, SEXPTYPE::REALSXP.as_c_int());
        let _xp_guard = protect(xp);

        let nx = XLENGTH(x) as usize;
        if XLENGTH(y) != nx as crate::sexp::ffi::R_xlen_t {
            Rf_error(b"'x' and 'y' lengths differ\0".as_ptr() as *const std::os::raw::c_char);
        }
        let np = XLENGTH(xp) as usize;
        let yp = Rf_allocVector3(SEXPTYPE::REALSXP, np as crate::sexp::ffi::R_xlen_t);
        let _yp_guard = protect(yp);

        let x_slice = if nx == 0 {
            &[][..]
        } else {
            slice::from_raw_parts(REAL(x), nx)
        };
        let y_slice = if nx == 0 {
            &[][..]
        } else {
            slice::from_raw_parts(REAL(y), nx)
        };
        let xp_slice = if np == 0 {
            &[][..]
        } else {
            slice::from_raw_parts(REAL(xp), np)
        };
        let yp_slice = if np == 0 {
            &mut [][..]
        } else {
            slice::from_raw_parts_mut(REAL(yp), np)
        };

        BDRksmooth(x_slice, y_slice, xp_slice, yp_slice, krn, bw);

        let ans = Rf_allocVector(SEXPTYPE::VECSXP, 2);
        let _ans_guard = protect(ans);
        SET_VECTOR_ELT(ans, 0, xp);
        SET_VECTOR_ELT(ans, 1, yp);

        let nm = Rf_allocVector(SEXPTYPE::STRSXP, 2);
        setAttrib(ans, R_NamesSymbol(), nm);
        SET_STRING_ELT(nm, 0, mkChar("x"));
        SET_STRING_ELT(nm, 1, mkChar("y"));

        ans
    }
}

#[cfg(test)]
mod tests {
    use super::BDRksmooth;

    #[test]
    fn empty_input_initializes_predictions_to_na() {
        let mut yp = [0.0, 0.0];
        BDRksmooth(&[], &[], &[1.0, 2.0], &mut yp, 1, 1.0);
        assert!(yp.iter().all(|value| value.is_nan()));
    }
}
