/* --- Isotonic regression ---
 * code simplified from VR_mds_fn() which is part of MASS.c,
 * Copyright (C) 1995  Brian Ripley
 * ---
 *  R : A Computer Language for Statistical Data Analysis
 *  Copyright (C) 2003-2024	The R Core Team
 *  Copyright (C) 2003-2023	The R Foundation
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

//! Isotonic regression
//! Port of r-source/src/library/stats/src/isoreg.c

use std::os::raw::{c_double, c_int};
use std::ptr;

use crate::main::errors::Rf_error;
use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::ffi::{R_FINITE, R_xlen_t, SEXP, SEXPTYPE};
use crate::sexp::globals::R_NilValue;
use crate::sexp::protect::{Rf_protect, Rf_unprotect};

// ---------------------------------------------------------------------------
// Local helpers
// ---------------------------------------------------------------------------

const R_PosInf: c_double = f64::INFINITY;

unsafe fn error(msg: &str) {
    unsafe {
        let c_msg = std::ffi::CString::new(msg).unwrap_or_default();
        Rf_error(c_msg.as_ptr());
    }
}

/// Create a named list (simplified mkNamed for known patterns)
unsafe fn mkNamed(sexptype: c_int, names: &[&str]) -> SEXP {
    unsafe {
        let len = names.len() as c_int;
        let ans = Rf_allocVector(sexptype, len);
        Rf_protect(ans);
        let nm = Rf_allocVector(SEXPTYPE::STRSXP, len);
        for i in 0..names.len() {
            let c_str = std::ffi::CString::new(names[i]).unwrap_or_default();
            SET_STRING_ELT(nm, i as R_xlen_t, Rf_mkChar(c_str.as_ptr()));
        }
        crate::attrib_core::setAttrib(ans, crate::attrib_core::R_NamesSymbol(), nm);
        Rf_unprotect(1);
        ans
    }
}

/// Resize a vector to a shorter length (simplified xlengthgets)
unsafe fn xlengthgets(x: SEXP, new_len: R_xlen_t) -> SEXP {
    unsafe {
        let old_len = XLENGTH(x);
        if old_len <= new_len {
            return x;
        }
        let old_type = TYPEOF(x);
        let ans = Rf_allocVector(old_type, new_len as c_int);
        if old_type == SEXPTYPE::REALSXP {
            for i in 0..new_len as usize {
                *REAL(ans).add(i) = *REAL(x).add(i);
            }
        } else if old_type == SEXPTYPE::INTSXP {
            for i in 0..new_len as usize {
                *INTEGER(ans).add(i) = *INTEGER(x).add(i);
            }
        }
        // Copy attributes
        let nm_attr = crate::attrib_core::getAttrib(x, crate::attrib_core::R_NamesSymbol());
        if Rf_isNull(nm_attr) == 0 {
            crate::attrib_core::setAttrib(ans, crate::attrib_core::R_NamesSymbol(), nm_attr);
        }
        ans
    }
}

// ---------------------------------------------------------------------------
// isoreg: isotonic regression
// ---------------------------------------------------------------------------

pub unsafe fn isoreg(y: SEXP) -> SEXP {
    unsafe {
        let n = XLENGTH(y);

        let anms: [&str; 5] = ["y", "yc", "yf", "iKnots", ""];
        let ans = Rf_protect(mkNamed(SEXPTYPE::VECSXP.into(), &anms));

        let yc = Rf_allocVector(SEXPTYPE::REALSXP, (n + 1) as c_int);
        let yf = Rf_allocVector(SEXPTYPE::REALSXP, n as c_int);
        let iKnots = Rf_allocVector(SEXPTYPE::INTSXP, n as c_int);

        SET_VECTOR_ELT(ans, 0, y);
        SET_VECTOR_ELT(ans, 1, yc);
        SET_VECTOR_ELT(ans, 2, yf);
        SET_VECTOR_ELT(ans, 3, iKnots);

        if n == 0 {
            Rf_unprotect(1);
            return ans;
        }

        // yc := cumsum(0, y)
        *REAL(yc) = 0.0;
        let mut tmp: c_double = 0.0;
        for i in 0..n as usize {
            tmp += *REAL(y).add(i);
            *REAL(yc).add(i + 1) = tmp;
        }

        if !R_FINITE(*REAL(yc).add(n as usize)) {
            error(&format!(
                "non-finite sum(y) == {} is not allowed",
                *REAL(yc).add(n as usize)
            ));
            Rf_unprotect(1);
            return R_NilValue();
        }

        let mut known: R_xlen_t = 0;
        let mut ip: R_xlen_t = 0;
        let mut n_ip: R_xlen_t = 0;

        loop {
            let mut slope: c_double = R_PosInf;
            let mut i: R_xlen_t = known + 1;
            while i <= n {
                tmp = (*REAL(yc).add(i as usize) - *REAL(yc).add(known as usize))
                    / (i - known) as c_double;
                if tmp < slope {
                    slope = tmp;
                    ip = i;
                }
                i += 1;
            }

            *INTEGER(iKnots).add(n_ip as usize) = ip as c_int;
            n_ip += 1;

            let mut i: R_xlen_t = known;
            while i < ip {
                *REAL(yf).add(i as usize) = (*REAL(yc).add(ip as usize)
                    - *REAL(yc).add(known as usize))
                    / (ip - known) as c_double;
                i += 1;
            }

            known = ip;
            if known >= n {
                break;
            }
        }

        if n_ip < n {
            let trimmed = xlengthgets(iKnots, n_ip);
            SET_VECTOR_ELT(ans, 3, trimmed);
        }

        Rf_unprotect(1);
        ans
    }
}
