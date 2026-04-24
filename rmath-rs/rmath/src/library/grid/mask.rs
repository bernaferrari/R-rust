/*
 *  R : A Computer Language for Statistical Data Analysis
 *  Copyright (C) 2001-3 Paul Murrell
 *                2003-2025 The R Core Team
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

//! Port of R's src/library/grid/src/mask.c
//!
//! Grid mask type checking and resolution.

use std::os::raw::c_int;

use crate::mainutils::engine as ge;
use crate::sexp::constructors::Rf_ScalarLogical;
use crate::sexp::constructors::Rf_lang2;
use crate::sexp::envir::findFun;
use crate::sexp::ffi::SEXP;
use crate::sexp::globals::R_NilValue;
use crate::sexp::protect::{Rf_protect, Rf_unprotect};
use crate::sexp::symbol::Rf_install;

use super::types::{pGEDevDesc, *};

// ---------------------------------------------------------------------------
// Local helpers
// ---------------------------------------------------------------------------

/// Rf_inherits — check if object inherits from a given class
unsafe fn Rf_inherits(x: SEXP, what: *const std::os::raw::c_char) -> c_int {
    if x.is_null() || what.is_null() {
        return 0;
    }
    let klass = crate::attrib_core::getAttrib(x, crate::attrib_core::R_ClassSymbol());
    if klass.is_null() || klass == R_NilValue() {
        return 0;
    }
    use crate::sexp::accessors::{CHAR, LENGTH, STRING_ELT, TYPEOF};
    use std::ffi::CStr;
    if TYPEOF(klass) != crate::sexp::ffi::SEXPTYPE::STRSXP {
        return 0;
    }
    let cn = match CStr::from_ptr(what).to_str() {
        Ok(s) => s,
        Err(_) => return 0,
    };
    let n = LENGTH(klass);
    for i in 0..n {
        let elt = STRING_ELT(klass, i as crate::sexp::ffi::R_xlen_t);
        if !elt.is_null() {
            let cs = CHAR(elt);
            if !cs.is_null() {
                if let Ok(s2) = CStr::from_ptr(cs).to_str() {
                    if s2 == cn {
                        return 1;
                    }
                }
            }
        }
    }
    0
}

// ---------------------------------------------------------------------------
// isMask — check if object is a GridMask
// ---------------------------------------------------------------------------

pub unsafe fn isMask(mask: SEXP) -> bool {
    Rf_inherits(mask, b"GridMask\0".as_ptr() as *const std::os::raw::c_char) != 0
}

// ---------------------------------------------------------------------------
// resolveMask — resolve a mask via R callback
// ---------------------------------------------------------------------------

pub unsafe fn resolveMask(mask: SEXP, dd: pGEDevDesc) -> SEXP {
    // Use the shared grid eval env so mask callbacks see the same
    // initialization state as the rest of grid.
    let env = grid_eval_env();
    let resolve_fn = Rf_protect(findFun(
        Rf_install(b"resolveMask\0".as_ptr() as *const std::os::raw::c_char),
        env,
    ));
    let r_fcall = Rf_protect(Rf_lang2(resolve_fn, mask));
    let result = ge::Rf_eval_with_gd(r_fcall, env, dd);
    Rf_unprotect(2);
    result
}
