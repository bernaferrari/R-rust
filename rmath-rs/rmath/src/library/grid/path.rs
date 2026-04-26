#![allow(unsafe_op_in_unsafe_fn)]
// legacy C-port unsafe boundary; see docs/unsafe-op-allowlist.tsv.
/*
 *  R : A Computer Language for Statistical Data Analysis
 *  Copyright (C) 2001-3 Paul Murrell
 *                2003-2024 The R Core Team
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

//! Port of R's src/library/grid/src/path.c
//!
//! Path drawing operations: stroke, fill, fillStroke.

use std::ffi::c_void;
use std::os::raw::c_int;

use crate::sexp::accessors::{INTEGER, Rf_isNull};
use crate::sexp::constructors::Rf_ScalarLogical;
use crate::sexp::ffi::SEXP;
use crate::sexp::globals::R_NilValue;
use crate::sexp::protect::{Rf_protect, Rf_unprotect};

use crate::mainutils::engine as ge;
use crate::mainutils::graphics_ffi::rmath_grid_release_pattern;

use super::gpar::{gcontextFromgpar, resolveGPar};
use super::grid::getDevice;
use super::state::{gridStateElement, setGridStateElement};
use super::types::*;

/// getListElement — get a named element from a list
unsafe fn getListElement(list: SEXP, str: *const std::os::raw::c_char) -> SEXP {
    super::util::getListElement(list, str as *mut std::os::raw::c_char)
}

/// Rf_inherits — check if object inherits from a class
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
// L_stroke — stroke a path
// ---------------------------------------------------------------------------

pub unsafe fn L_stroke(path: SEXP) -> SEXP {
    // R_GE_gcontext gc — opaque struct, allocated on stack.
    // engine::GEStroke is still a headless no-op here; keep the call for parity wiring.
    // routed through the shared engine entry point so the device backend can
    // be filled in centrally without adding another local shim.
    let mut _gc: [u8; 256] = [0; 256];
    let dd = getDevice();
    if dd.is_null() {
        return R_NilValue();
    }
    let currentgp = gridStateElement(dd, GSS_GPAR);
    gcontextFromgpar(currentgp, 0, _gc.as_mut_ptr() as *const c_void, dd);

    ge::GEMode(1, dd);
    setGridStateElement(dd, GSS_RESOLVINGPATH, Rf_ScalarLogical(1));
    ge::GEStroke(path, _gc.as_ptr() as *const c_void, dd);
    setGridStateElement(dd, GSS_RESOLVINGPATH, Rf_ScalarLogical(0));
    ge::GEMode(0, dd);

    R_NilValue()
}

// ---------------------------------------------------------------------------
// L_fill — fill a path
// ---------------------------------------------------------------------------

pub unsafe fn L_fill(path: SEXP, rule: SEXP) -> SEXP {
    let mut _gc: [u8; 256] = [0; 256];
    let dd = getDevice();
    if dd.is_null() {
        return R_NilValue();
    }
    let currentgp = Rf_protect(crate::main::duplicate::Rf_duplicate(gridStateElement(
        dd, GSS_GPAR,
    )));
    let resolved_fill = Rf_protect(resolveGPar(currentgp, 0));
    gcontextFromgpar(currentgp, 0, _gc.as_mut_ptr() as *const c_void, dd);

    ge::GEMode(1, dd);
    setGridStateElement(dd, GSS_RESOLVINGPATH, Rf_ScalarLogical(1));
    ge::GEFill(path, *INTEGER(rule), _gc.as_ptr() as *const c_void, dd);
    setGridStateElement(dd, GSS_RESOLVINGPATH, Rf_ScalarLogical(0));

    if Rf_isNull(resolved_fill) == 0
        && Rf_inherits(
            resolved_fill,
            b"GridGrobPattern\0".as_ptr() as *const std::os::raw::c_char,
        ) != 0
    {
        let pattern_ref = getListElement(
            resolved_fill,
            b"index\0".as_ptr() as *const std::os::raw::c_char,
        );
        rmath_grid_release_pattern(
            dd as crate::mainutils::graphics_ffi::pGEDevDesc,
            pattern_ref,
        );
    }
    Rf_unprotect(2);
    ge::GEMode(0, dd);

    R_NilValue()
}

// ---------------------------------------------------------------------------
// L_fillStroke — fill and stroke a path
// ---------------------------------------------------------------------------

pub unsafe fn L_fillStroke(path: SEXP, rule: SEXP) -> SEXP {
    let mut _gc: [u8; 256] = [0; 256];
    let dd = getDevice();
    if dd.is_null() {
        return R_NilValue();
    }
    let currentgp = Rf_protect(crate::main::duplicate::Rf_duplicate(gridStateElement(
        dd, GSS_GPAR,
    )));
    let resolved_fill = Rf_protect(resolveGPar(currentgp, 0));
    gcontextFromgpar(currentgp, 0, _gc.as_mut_ptr() as *const c_void, dd);

    ge::GEMode(1, dd);
    setGridStateElement(dd, GSS_RESOLVINGPATH, Rf_ScalarLogical(1));
    ge::GEFillStroke(path, *INTEGER(rule), _gc.as_ptr() as *const c_void, dd);
    setGridStateElement(dd, GSS_RESOLVINGPATH, Rf_ScalarLogical(0));

    if Rf_isNull(resolved_fill) == 0
        && Rf_inherits(
            resolved_fill,
            b"GridGrobPattern\0".as_ptr() as *const std::os::raw::c_char,
        ) != 0
    {
        let pattern_ref = getListElement(
            resolved_fill,
            b"index\0".as_ptr() as *const std::os::raw::c_char,
        );
        rmath_grid_release_pattern(
            dd as crate::mainutils::graphics_ffi::pGEDevDesc,
            pattern_ref,
        );
    }
    Rf_unprotect(2);
    ge::GEMode(0, dd);

    R_NilValue()
}
