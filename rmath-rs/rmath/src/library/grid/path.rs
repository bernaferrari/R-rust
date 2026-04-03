#![allow(unsafe_op_in_unsafe_fn)]
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

use std::os::raw::c_int;

use crate::sexp::accessors::{INTEGER, Rf_isNull};
use crate::sexp::ffi::SEXP;
use crate::sexp::globals::R_NilValue;
use crate::sexp::protect::{Rf_protect, Rf_unprotect};

use super::types::*;

// ---------------------------------------------------------------------------
// External stubs for GE functions not yet ported
// ---------------------------------------------------------------------------

/// ScalarLogical — create a single-element logical vector
unsafe fn ScalarLogical(x: c_int) -> SEXP {
    let s = crate::sexp::constructors::Rf_allocVector(crate::sexp::ffi::SEXPTYPE::LGLSXP.0, 1);
    *crate::sexp::accessors::LOGICAL(s) = x;
    s
}

/// getDevice — get the current graphics device
unsafe fn getDevice() -> *const u8 {
    // STUB: requires grid.c
    std::ptr::null()
}

/// gridStateElement — get a grid state element from device
unsafe fn gridStateElement(_dd: *const u8, _elementIndex: c_int) -> SEXP {
    // STUB: requires state.c
    R_NilValue()
}

/// setGridStateElement — set a grid state element on device
unsafe fn setGridStateElement(_dd: *const u8, _elementIndex: c_int, _value: SEXP) {
    // STUB: requires state.c
}

/// GEMode — set graphics engine mode
unsafe fn GEMode(_mode: c_int, _dd: *const u8) {
    // STUB: requires GraphicsEngine
}

/// gcontextFromgpar — create graphics context from gpar
unsafe fn gcontextFromgpar(_gp: SEXP, _i: c_int, _gc: *mut u8, _dd: *const u8) {
    // STUB: requires gpar.c
}

/// GEStroke — stroke a path on the device
unsafe fn GEStroke(_path: SEXP, _gc: *const u8, _dd: *const u8) {
    // STUB: requires GraphicsEngine
}

/// GEFill — fill a path on the device
unsafe fn GEFill(_path: SEXP, _rule: c_int, _gc: *const u8, _dd: *const u8) {
    // STUB: requires GraphicsEngine
}

/// GEFillStroke — fill and stroke a path on the device
unsafe fn GEFillStroke(_path: SEXP, _rule: c_int, _gc: *const u8, _dd: *const u8) {
    // STUB: requires GraphicsEngine
}

/// Rf_duplicate — deep copy an R object
unsafe fn Rf_duplicate(x: SEXP) -> SEXP {
    crate::main::duplicate::Rf_duplicate(x)
}

/// resolveGPar — resolve gpar (e.g., pattern fills)
unsafe fn resolveGPar(_gp: SEXP, _by_name: bool) -> SEXP {
    // STUB: requires gpar.c
    R_NilValue()
}

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
    if TYPEOF(klass) != crate::sexp::ffi::SEXPTYPE::STRSXP.0 {
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

#[unsafe(no_mangle)]
pub unsafe extern "C" fn L_stroke(path: SEXP) -> SEXP {
    // R_GE_gcontext gc — opaque struct, allocated on stack
    let mut _gc: [u8; 256] = [0; 256]; // placeholder for R_GE_gcontext
    let dd = getDevice();
    let currentgp = gridStateElement(dd, GSS_GPAR);
    gcontextFromgpar(currentgp, 0, _gc.as_mut_ptr(), dd);

    GEMode(1, dd);
    setGridStateElement(dd, GSS_RESOLVINGPATH, ScalarLogical(1));
    GEStroke(path, _gc.as_ptr(), dd);
    setGridStateElement(dd, GSS_RESOLVINGPATH, ScalarLogical(0));
    GEMode(0, dd);

    R_NilValue()
}

// ---------------------------------------------------------------------------
// L_fill — fill a path
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe extern "C" fn L_fill(path: SEXP, rule: SEXP) -> SEXP {
    let mut _gc: [u8; 256] = [0; 256];
    let dd = getDevice();
    let currentgp = Rf_protect(Rf_duplicate(gridStateElement(dd, GSS_GPAR)));
    let resolved_fill = Rf_protect(resolveGPar(currentgp, false));
    gcontextFromgpar(currentgp, 0, _gc.as_mut_ptr(), dd);

    GEMode(1, dd);
    setGridStateElement(dd, GSS_RESOLVINGPATH, ScalarLogical(1));
    GEFill(path, *INTEGER(rule), _gc.as_ptr(), dd);
    setGridStateElement(dd, GSS_RESOLVINGPATH, ScalarLogical(0));

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
        // dd->dev->releasePattern(patternRef, dd->dev);
        let _ = pattern_ref;
    }
    Rf_unprotect(2);
    GEMode(0, dd);

    R_NilValue()
}

// ---------------------------------------------------------------------------
// L_fillStroke — fill and stroke a path
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe extern "C" fn L_fillStroke(path: SEXP, rule: SEXP) -> SEXP {
    let mut _gc: [u8; 256] = [0; 256];
    let dd = getDevice();
    let currentgp = Rf_protect(Rf_duplicate(gridStateElement(dd, GSS_GPAR)));
    let resolved_fill = Rf_protect(resolveGPar(currentgp, false));
    gcontextFromgpar(currentgp, 0, _gc.as_mut_ptr(), dd);

    GEMode(1, dd);
    setGridStateElement(dd, GSS_RESOLVINGPATH, ScalarLogical(1));
    GEFillStroke(path, *INTEGER(rule), _gc.as_ptr(), dd);
    setGridStateElement(dd, GSS_RESOLVINGPATH, ScalarLogical(0));

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
        // dd->dev->releasePattern(patternRef, dd->dev);
        let _ = pattern_ref;
    }
    Rf_unprotect(2);
    GEMode(0, dd);

    R_NilValue()
}
