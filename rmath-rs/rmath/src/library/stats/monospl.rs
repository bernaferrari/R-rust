// legacy C-port unsafe boundary; see docs/unsafe-op-allowlist.tsv.
/*  R : A Computer Language for Statistical Data Analysis
 *  Copyright (C) 2010	The R Foundation
 *  Copyright (C) 2016	The R Core Team
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

//! Monotone spline interpolation (Fritsch & Carlson 1980)
//! Port of r-source/src/library/stats/src/monoSpl.c

use std::os::raw::{c_double, c_int};

use crate::main::duplicate::duplicate;
use crate::main::errors::Rf_error;
use crate::sexp::accessors::*;
use crate::sexp::constructors::Rf_allocVector;
use crate::sexp::ffi::{SEXP, SEXPTYPE};
use crate::sexp::globals::R_NilValue;
use crate::sexp::protect::{Rf_protect, Rf_unprotect};

// ---------------------------------------------------------------------------
// Local helpers
// ---------------------------------------------------------------------------

unsafe fn isInteger(x: SEXP) -> bool {
    unsafe { TYPEOF(x) == SEXPTYPE::INTSXP }
}

unsafe fn isReal(x: SEXP) -> bool {
    unsafe { crate::main::coerce::isReal(x) != 0 }
}

unsafe fn error(msg: &str) {
    unsafe {
        let c_msg = std::ffi::CString::new(msg).unwrap_or_default();
        Rf_error(c_msg.as_ptr());
    }
}

// ---------------------------------------------------------------------------
// monoFC_mod: modify slopes using Fritsch & Carlson (1980)
// ---------------------------------------------------------------------------

/// Modify the slopes m_k := s'(x_k) using Fritsch & Carlson (1980)'s algorithm.
///
/// @param m  numeric vector of length n, the preliminary desired slopes s'(x_i)
/// @param S  the divided differences (y_{i+1} - y_i) / (x_{i+1} - x_i); i = 1:(n-1)
/// @param n  == length(m) == 1 + length(S)
///
/// Note that m[] is modified in place.
pub unsafe fn monoFC_mod(m: *mut c_double, S: *mut c_double, n: c_int) {
    unsafe {
        if n < 2 {
            error("n must be at least two");
            return;
        }

        for k in 0..(n - 1) as usize {
            let sk = *S.add(k);
            let k1 = k + 1;

            if sk == 0.0 {
                *m.add(k) = 0.0;
                *m.add(k1) = 0.0;
            } else {
                let alpha = *m.add(k) / sk;
                let beta = *m.add(k1) / sk;
                let a2b3 = 2.0 * alpha + beta - 3.0;
                let ab23 = alpha + 2.0 * beta - 3.0;

                if a2b3 > 0.0 && ab23 > 0.0 && alpha * (a2b3 + ab23) < a2b3 * a2b3 {
                    // Outside the monotonicity region => fix slopes
                    let tau_s = 3.0 * sk / (alpha * alpha + beta * beta).sqrt();
                    *m.add(k) = tau_s * alpha;
                    *m.add(k1) = tau_s * beta;
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// monoFC_m: SEXP interface
// ---------------------------------------------------------------------------

pub unsafe fn monoFC_m(m: SEXP, sx: SEXP) -> SEXP {
    unsafe {
        let n = LENGTH(m);

        let val = if isInteger(m) {
            // Coerce integer to real
            let coerced = Rf_allocVector(SEXPTYPE::REALSXP, n);
            Rf_protect(coerced);
            for i in 0..n as usize {
                let iv = *INTEGER(m).add(i);
                *REAL(coerced).add(i) = iv as c_double;
            }
            Rf_unprotect(1);
            coerced
        } else {
            if !isReal(m) {
                error("Argument m must be numeric");
                return R_NilValue();
            }
            duplicate(m)
        };

        Rf_protect(val);

        if n < 2 {
            error("length(m) must be at least two");
            Rf_unprotect(1);
            return R_NilValue();
        }
        if !isReal(sx) || LENGTH(sx) != n - 1 {
            error("Argument Sx must be numeric vector one shorter than m[]");
            Rf_unprotect(1);
            return R_NilValue();
        }

        // Fix up the slopes m[] := val[]:
        monoFC_mod(REAL(val), REAL(sx), n);

        Rf_unprotect(1);
        val
    }
}
