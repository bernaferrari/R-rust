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

use crate::mainutils::engine as ge;
use crate::mainutils::graphics_ffi::rmath_grid_release_pattern;
use crate::sexp::accessors::{INTEGER, Rf_isNull};
use crate::sexp::constructors::Rf_ScalarLogical;
use crate::sexp::ffi::SEXP;
use crate::sexp::globals::R_NilValue;
use crate::sexp::protect::protect;

use super::gpar::{gcontextFromgpar, resolveGPar};
use super::grid::getDevice;
use super::state::{gridStateElement, setGridStateElement};
use super::types::*;

struct GridPathGuard {
    dd: pGEDevDesc,
}

impl GridPathGuard {
    unsafe fn enter(dd: pGEDevDesc) -> Self {
        unsafe {
            ge::GEMode(1, dd);
            setGridStateElement(dd, GSS_RESOLVINGPATH, Rf_ScalarLogical(1));
        }
        Self { dd }
    }
}

impl Drop for GridPathGuard {
    fn drop(&mut self) {
        unsafe {
            setGridStateElement(self.dd, GSS_RESOLVINGPATH, Rf_ScalarLogical(0));
            ge::GEMode(0, self.dd);
        }
    }
}

/// getListElement — get a named element from a list
unsafe fn getListElement(list: SEXP, str: *const std::os::raw::c_char) -> SEXP {
    unsafe { super::util::getListElement(list, str as *mut std::os::raw::c_char) }
}

/// Rf_inherits — check if object inherits from a class
unsafe fn Rf_inherits(x: SEXP, what: *const std::os::raw::c_char) -> c_int {
    if x.is_null() || what.is_null() {
        return 0;
    }
    let klass = unsafe { crate::attrib_core::getAttrib(x, crate::attrib_core::R_ClassSymbol()) };
    if klass.is_null() || klass == unsafe { R_NilValue() } {
        return 0;
    }
    use crate::sexp::accessors::{CHAR, LENGTH, STRING_ELT, TYPEOF};
    use std::ffi::CStr;
    if unsafe { TYPEOF(klass) } != crate::sexp::ffi::SEXPTYPE::STRSXP {
        return 0;
    }
    let cn = match unsafe { CStr::from_ptr(what) }.to_str() {
        Ok(s) => s,
        Err(_) => return 0,
    };
    let n = unsafe { LENGTH(klass) };
    for i in 0..n {
        let elt = unsafe { STRING_ELT(klass, i as crate::sexp::ffi::R_xlen_t) };
        if !elt.is_null() {
            let cs = unsafe { CHAR(elt) };
            if !cs.is_null() {
                if let Ok(s2) = unsafe { CStr::from_ptr(cs) }.to_str() {
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
    let mut _gc: [u8; 256] = [0; 256];
    let dd = unsafe { getDevice() };
    if dd.is_null() {
        return unsafe { R_NilValue() };
    }
    let currentgp = unsafe { gridStateElement(dd, GSS_GPAR) };
    unsafe { gcontextFromgpar(currentgp, 0, _gc.as_mut_ptr() as *const c_void, dd) };

    let _scope = unsafe { GridPathGuard::enter(dd) };
    unsafe { ge::GEStroke(path, _gc.as_ptr() as *const c_void, dd) };

    unsafe { R_NilValue() }
}

// ---------------------------------------------------------------------------
// L_fill — fill a path
// ---------------------------------------------------------------------------

pub unsafe fn L_fill(path: SEXP, rule: SEXP) -> SEXP {
    let mut _gc: [u8; 256] = [0; 256];
    let dd = unsafe { getDevice() };
    if dd.is_null() {
        return unsafe { R_NilValue() };
    }
    let currentgp = unsafe { crate::main::duplicate::Rf_duplicate(gridStateElement(dd, GSS_GPAR)) };
    let _currentgp_guard = protect(currentgp);
    let resolved_fill = unsafe { resolveGPar(currentgp, 0) };
    let _resolved_fill_guard = protect(resolved_fill);
    unsafe { gcontextFromgpar(currentgp, 0, _gc.as_mut_ptr() as *const c_void, dd) };

    let _scope = unsafe { GridPathGuard::enter(dd) };
    unsafe { ge::GEFill(path, *INTEGER(rule), _gc.as_ptr() as *const c_void, dd) };

    if unsafe { Rf_isNull(resolved_fill) } == 0
        && unsafe {
            Rf_inherits(
                resolved_fill,
                b"GridGrobPattern\0".as_ptr() as *const std::os::raw::c_char,
            )
        } != 0
    {
        let pattern_ref = unsafe {
            getListElement(
                resolved_fill,
                b"index\0".as_ptr() as *const std::os::raw::c_char,
            )
        };
        unsafe {
            rmath_grid_release_pattern(
                dd as crate::mainutils::graphics_ffi::pGEDevDesc,
                pattern_ref,
            );
        }
    }

    unsafe { R_NilValue() }
}

// ---------------------------------------------------------------------------
// L_fillStroke — fill and stroke a path
// ---------------------------------------------------------------------------

pub unsafe fn L_fillStroke(path: SEXP, rule: SEXP) -> SEXP {
    let mut _gc: [u8; 256] = [0; 256];
    let dd = unsafe { getDevice() };
    if dd.is_null() {
        return unsafe { R_NilValue() };
    }
    let currentgp = unsafe { crate::main::duplicate::Rf_duplicate(gridStateElement(dd, GSS_GPAR)) };
    let _currentgp_guard = protect(currentgp);
    let resolved_fill = unsafe { resolveGPar(currentgp, 0) };
    let _resolved_fill_guard = protect(resolved_fill);
    unsafe { gcontextFromgpar(currentgp, 0, _gc.as_mut_ptr() as *const c_void, dd) };

    let _scope = unsafe { GridPathGuard::enter(dd) };
    unsafe { ge::GEFillStroke(path, *INTEGER(rule), _gc.as_ptr() as *const c_void, dd) };

    if unsafe { Rf_isNull(resolved_fill) } == 0
        && unsafe {
            Rf_inherits(
                resolved_fill,
                b"GridGrobPattern\0".as_ptr() as *const std::os::raw::c_char,
            )
        } != 0
    {
        let pattern_ref = unsafe {
            getListElement(
                resolved_fill,
                b"index\0".as_ptr() as *const std::os::raw::c_char,
            )
        };
        unsafe {
            rmath_grid_release_pattern(
                dd as crate::mainutils::graphics_ffi::pGEDevDesc,
                pattern_ref,
            );
        }
    }

    unsafe { R_NilValue() }
}
