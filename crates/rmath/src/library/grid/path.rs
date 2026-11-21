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
use crate::sexp::accessors::Rf_isNull;
use crate::sexp::ffi::SEXP;
use crate::sexp::globals::R_NilValue;

use super::gpar::{gcontextFromgpar, resolveGPar};
use super::grid::getDevice;
use super::state::{gridStateElement, setGridStateElement};
use super::types::*;
use super::{duplicate, inherits, integer_elt, protect, scalar_logical};

struct GridPathGuard {
    dd: pGEDevDesc,
}

impl GridPathGuard {
    // SAFETY: caller must hold a live device pointer; `GEMode` is an engine
    // mode-switch bridge whose raw device contract is discharged by callers
    // passing the `getDevice` result.
    unsafe fn enter(dd: pGEDevDesc) -> Self {
        ge::GEMode(1, dd);
        setGridStateElement(dd, GSS_RESOLVINGPATH, scalar_logical(1));
        Self { dd }
    }
}

impl Drop for GridPathGuard {
    fn drop(&mut self) {
        setGridStateElement(self.dd, GSS_RESOLVINGPATH, scalar_logical(0));
        // SAFETY: `dd` was a live device when the guard was entered; engine
        // mode teardown mirrors `enter`.
        ge::GEMode(0, self.dd);
    }
}

/// getListElement — get a named element from a list
fn getListElement(list: SEXP, str: *const std::os::raw::c_char) -> SEXP {
    super::util::getListElement(list, str)
}

// ---------------------------------------------------------------------------
// L_stroke — stroke a path
// ---------------------------------------------------------------------------

pub fn L_stroke(path: SEXP) -> SEXP {
    let mut _gc: [u8; 256] = [0; 256];
    // SAFETY: device registry bridge; a null result means no open device.
    let dd = unsafe { getDevice() };
    if dd.is_null() {
        return R_NilValue();
    }
    let currentgp = gridStateElement(dd, GSS_GPAR);
    gcontextFromgpar(currentgp, 0, _gc.as_mut_ptr() as *const c_void, dd);

    // SAFETY: `dd` is the live device returned by `getDevice` above; `gc` is
    // the opaque engine context buffer just filled by `gcontextFromgpar`.
    let _scope = unsafe { GridPathGuard::enter(dd) };
    ge::GEStroke(path, _gc.as_ptr() as *const c_void, dd);

    R_NilValue()
}

// ---------------------------------------------------------------------------
// L_fill — fill a path
// ---------------------------------------------------------------------------

pub fn L_fill(path: SEXP, rule: SEXP) -> SEXP {
    let mut _gc: [u8; 256] = [0; 256];
    // SAFETY: device registry bridge; a null result means no open device.
    let dd = unsafe { getDevice() };
    if dd.is_null() {
        return R_NilValue();
    }
    let currentgp = duplicate(gridStateElement(dd, GSS_GPAR));
    let _currentgp_guard = protect(currentgp);
    let resolved_fill = resolveGPar(currentgp, 0);
    let _resolved_fill_guard = protect(resolved_fill);
    gcontextFromgpar(currentgp, 0, _gc.as_mut_ptr() as *const c_void, dd);

    // SAFETY: `dd` is the live device returned by `getDevice` above; `gc` is
    // the opaque engine context buffer just filled by `gcontextFromgpar`.
    let _scope = unsafe { GridPathGuard::enter(dd) };
    ge::GEFill(
        path,
        integer_elt(rule, 0),
        _gc.as_ptr() as *const c_void,
        dd,
    );

    if Rf_isNull(resolved_fill) == 0 && inherits(resolved_fill, c"GridGrobPattern") {
        let pattern_ref = getListElement(
            resolved_fill,
            b"index\0".as_ptr() as *const std::os::raw::c_char,
        );
        // SAFETY: `dd` is the live device; `pattern_ref` is the pattern index
        // element read from the resolved fill above.
        unsafe {
            rmath_grid_release_pattern(
                dd as crate::mainutils::graphics_ffi::pGEDevDesc,
                pattern_ref,
            );
        }
    }

    R_NilValue()
}

// ---------------------------------------------------------------------------
// L_fillStroke — fill and stroke a path
// ---------------------------------------------------------------------------

pub fn L_fillStroke(path: SEXP, rule: SEXP) -> SEXP {
    let mut _gc: [u8; 256] = [0; 256];
    // SAFETY: device registry bridge; a null result means no open device.
    let dd = unsafe { getDevice() };
    if dd.is_null() {
        return R_NilValue();
    }
    let currentgp = duplicate(gridStateElement(dd, GSS_GPAR));
    let _currentgp_guard = protect(currentgp);
    let resolved_fill = resolveGPar(currentgp, 0);
    let _resolved_fill_guard = protect(resolved_fill);
    gcontextFromgpar(currentgp, 0, _gc.as_mut_ptr() as *const c_void, dd);

    // SAFETY: `dd` is the live device returned by `getDevice` above; `gc` is
    // the opaque engine context buffer just filled by `gcontextFromgpar`.
    let _scope = unsafe { GridPathGuard::enter(dd) };
    ge::GEFillStroke(
        path,
        integer_elt(rule, 0),
        _gc.as_ptr() as *const c_void,
        dd,
    );

    if Rf_isNull(resolved_fill) == 0 && inherits(resolved_fill, c"GridGrobPattern") {
        let pattern_ref = getListElement(
            resolved_fill,
            b"index\0".as_ptr() as *const std::os::raw::c_char,
        );
        // SAFETY: `dd` is the live device; `pattern_ref` is the pattern index
        // element read from the resolved fill above.
        unsafe {
            rmath_grid_release_pattern(
                dd as crate::mainutils::graphics_ffi::pGEDevDesc,
                pattern_ref,
            );
        }
    }

    R_NilValue()
}
