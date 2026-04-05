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

//! Port of R's src/library/grid/src/layout.c
//!
//! Layout accessor functions and viewport layout calculations.

use std::os::raw::c_int;

use crate::sexp::accessors::{INTEGER, LENGTH, REAL, Rf_isNull, SET_VECTOR_ELT, VECTOR_ELT};
use crate::sexp::constructors::Rf_allocVector;
use crate::sexp::ffi::{R_xlen_t, SEXP, SEXPTYPE};
use crate::sexp::protect::{Rf_protect, Rf_unprotect};

use super::types::*;

// ---------------------------------------------------------------------------
// Simple layout accessor functions
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe extern "C" fn layoutNRow(l: SEXP) -> c_int {
    *INTEGER(VECTOR_ELT(l, LAYOUT_NROW as R_xlen_t))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn layoutNCol(l: SEXP) -> c_int {
    *INTEGER(VECTOR_ELT(l, LAYOUT_NCOL as R_xlen_t))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn layoutWidths(l: SEXP) -> SEXP {
    VECTOR_ELT(l, LAYOUT_WIDTHS as R_xlen_t)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn layoutHeights(l: SEXP) -> SEXP {
    VECTOR_ELT(l, LAYOUT_HEIGHTS as R_xlen_t)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn layoutRespect(l: SEXP) -> c_int {
    *INTEGER(VECTOR_ELT(l, LAYOUT_VRESPECT as R_xlen_t))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn layoutRespectMat(l: SEXP) -> *mut c_int {
    INTEGER(VECTOR_ELT(l, LAYOUT_MRESPECT as R_xlen_t))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn layoutHJust(l: SEXP) -> f64 {
    *REAL(VECTOR_ELT(l, LAYOUT_VJUST as R_xlen_t))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn layoutVJust(l: SEXP) -> f64 {
    *REAL(VECTOR_ELT(l, LAYOUT_VJUST as R_xlen_t)).add(1)
}

// ---------------------------------------------------------------------------
// relativeUnit — STUB: requires pureNullUnit from unit.c
// ---------------------------------------------------------------------------

unsafe fn relativeUnit(_unit: SEXP, _index: c_int, _dd: *const u8) -> bool {
    // STUB: requires pureNullUnit from unit.c
    false
}

// ---------------------------------------------------------------------------
// findRelWidths / findRelHeights
// ---------------------------------------------------------------------------

unsafe fn findRelWidths(layout: SEXP, relativeWidths: *mut c_int, dd: *const u8) {
    let widths = layoutWidths(layout);
    let ncol = layoutNCol(layout);
    for i in 0..ncol {
        *relativeWidths.add(i as usize) = if relativeUnit(widths, i, dd) { 1 } else { 0 };
    }
}

unsafe fn findRelHeights(layout: SEXP, relativeHeights: *mut c_int, dd: *const u8) {
    let heights = layoutHeights(layout);
    let nrow = layoutNRow(layout);
    for i in 0..nrow {
        *relativeHeights.add(i as usize) = if relativeUnit(heights, i, dd) { 1 } else { 0 };
    }
}

// ---------------------------------------------------------------------------
// allocateKnownWidths / allocateKnownHeights — STUBS: require transformWidth/transformHeight
// ---------------------------------------------------------------------------

unsafe fn allocateKnownWidths(
    layout: SEXP,
    relativeWidths: *mut c_int,
    parentWidthCM: f64,
    parentHeightCM: f64,
    parentContext: LViewportContext,
    parentgc: *const u8,
    dd: *const u8,
    npcWidths: *mut f64,
    widthLeftCM: *mut f64,
) {
    let widths = layoutWidths(layout);
    let ncol = layoutNCol(layout);
    for i in 0..ncol {
        if *relativeWidths.add(i as usize) == 0 {
            // STUB: transformWidth from unit.c not yet ported
            let _ = (
                layout,
                parentContext,
                parentgc,
                parentWidthCM,
                parentHeightCM,
                dd,
            );
            *npcWidths.add(i as usize) = 0.0;
            *widthLeftCM -= 0.0;
        }
    }
}

unsafe fn allocateKnownHeights(
    layout: SEXP,
    relativeHeights: *mut c_int,
    parentWidthCM: f64,
    parentHeightCM: f64,
    parentContext: LViewportContext,
    parentgc: *const u8,
    dd: *const u8,
    npcHeights: *mut f64,
    heightLeftCM: *mut f64,
) {
    let heights = layoutHeights(layout);
    let nrow = layoutNRow(layout);
    for i in 0..nrow {
        if *relativeHeights.add(i as usize) == 0 {
            // STUB: transformHeight from unit.c not yet ported
            let _ = (
                layout,
                parentContext,
                parentgc,
                parentWidthCM,
                parentHeightCM,
                dd,
            );
            *npcHeights.add(i as usize) = 0.0;
            *heightLeftCM -= 0.0;
        }
    }
}

// ---------------------------------------------------------------------------
// colRespected / rowRespected
// ---------------------------------------------------------------------------

unsafe fn colRespected(col: c_int, layout: SEXP) -> c_int {
    let mut result: c_int = 0;
    let respect = layoutRespect(layout);
    let respect_mat = layoutRespectMat(layout);
    if respect == 1 {
        result = 1;
    } else {
        let nrow = layoutNRow(layout);
        for i in 0..nrow {
            if *respect_mat.add((col * nrow + i) as usize) != 0 {
                result = 1;
                break;
            }
        }
    }
    result
}

unsafe fn rowRespected(row: c_int, layout: SEXP) -> c_int {
    let mut result: c_int = 0;
    let respect = layoutRespect(layout);
    let respect_mat = layoutRespectMat(layout);
    if respect == 1 {
        result = 1;
    } else {
        let ncol = layoutNCol(layout);
        for i in 0..ncol {
            if *respect_mat.add((i * layoutNRow(layout) + row) as usize) != 0 {
                result = 1;
                break;
            }
        }
    }
    result
}

// ---------------------------------------------------------------------------
// totalWidth / totalHeight — STUBS: require transformWidth/transformHeight
// ---------------------------------------------------------------------------

unsafe fn totalWidth(
    _layout: SEXP,
    _relativeWidths: *mut c_int,
    _parentContext: LViewportContext,
    _parentgc: *const u8,
    _dd: *const u8,
) -> f64 {
    // STUB: requires transformWidth from unit.c
    0.0
}

unsafe fn totalHeight(
    _layout: SEXP,
    _relativeHeights: *mut c_int,
    _parentContext: LViewportContext,
    _parentgc: *const u8,
    _dd: *const u8,
) -> f64 {
    // STUB: requires transformHeight from unit.c
    0.0
}

// ---------------------------------------------------------------------------
// allocateRespected — STUB
// ---------------------------------------------------------------------------

unsafe fn allocateRespected(
    _layout: SEXP,
    _relativeWidths: *mut c_int,
    _relativeHeights: *mut c_int,
    _reducedWidthCM: *mut f64,
    _reducedHeightCM: *mut f64,
    _parentContext: LViewportContext,
    _parentgc: *const u8,
    _dd: *const u8,
    _npcWidths: *mut f64,
    _npcHeights: *mut f64,
) {
    // STUB: requires pureNullUnitValue, transformWidth, transformHeight from unit.c
}

// ---------------------------------------------------------------------------
// setRespectedZero
// ---------------------------------------------------------------------------

unsafe fn setRespectedZero(
    layout: SEXP,
    relativeWidths: *mut c_int,
    relativeHeights: *mut c_int,
    npcWidths: *mut f64,
    npcHeights: *mut f64,
) {
    let ncol = layoutNCol(layout);
    for i in 0..ncol {
        if *relativeWidths.add(i as usize) != 0 {
            if colRespected(i, layout) != 0 {
                *npcWidths.add(i as usize) = 0.0;
            }
        }
    }
    let nrow = layoutNRow(layout);
    for i in 0..nrow {
        if *relativeHeights.add(i as usize) != 0 {
            if rowRespected(i, layout) != 0 {
                *npcHeights.add(i as usize) = 0.0;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// totalUnrespectedWidth / totalUnrespectedHeight — STUBS
// ---------------------------------------------------------------------------

unsafe fn totalUnrespectedWidth(
    _layout: SEXP,
    _relativeWidths: *mut c_int,
    _parentContext: LViewportContext,
    _parentgc: *const u8,
    _dd: *const u8,
) -> f64 {
    // STUB: requires transformWidth from unit.c
    0.0
}

unsafe fn totalUnrespectedHeight(
    _layout: SEXP,
    _relativeHeights: *mut c_int,
    _parentContext: LViewportContext,
    _parentgc: *const u8,
    _dd: *const u8,
) -> f64 {
    // STUB: requires transformHeight from unit.c
    0.0
}

// ---------------------------------------------------------------------------
// setRemainingWidthZero / setRemainingHeightZero
// ---------------------------------------------------------------------------

unsafe fn setRemainingWidthZero(layout: SEXP, relativeWidths: *mut c_int, npcWidths: *mut f64) {
    let ncol = layoutNCol(layout);
    for i in 0..ncol {
        if *relativeWidths.add(i as usize) != 0 {
            if colRespected(i, layout) == 0 {
                *npcWidths.add(i as usize) = 0.0;
            }
        }
    }
}

unsafe fn setRemainingHeightZero(layout: SEXP, relativeHeights: *mut c_int, npcHeights: *mut f64) {
    let nrow = layoutNRow(layout);
    for i in 0..nrow {
        if *relativeHeights.add(i as usize) != 0 {
            if rowRespected(i, layout) == 0 {
                *npcHeights.add(i as usize) = 0.0;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// allocateRemainingWidth / allocateRemainingHeight — STUBS
// ---------------------------------------------------------------------------

unsafe fn allocateRemainingWidth(
    _layout: SEXP,
    _relativeWidths: *mut c_int,
    _remainingWidthCM: f64,
    _parentContext: LViewportContext,
    _parentgc: *const u8,
    _dd: *const u8,
    npcWidths: *mut f64,
) {
    // STUB: requires transformWidth from unit.c
    // For now, set all remaining widths to zero
}

unsafe fn allocateRemainingHeight(
    _layout: SEXP,
    _relativeHeights: *mut c_int,
    _remainingHeightCM: f64,
    _parentContext: LViewportContext,
    _parentgc: *const u8,
    _dd: *const u8,
    npcHeights: *mut f64,
) {
    // STUB: requires transformHeight from unit.c
    // For now, set all remaining heights to zero
}

// ---------------------------------------------------------------------------
// sumDims — static helper
// ---------------------------------------------------------------------------

unsafe fn sumDims(dims: *const f64, from: c_int, to: c_int) -> f64 {
    let mut s: f64 = 0.0;
    for i in from..=to {
        s += *dims.add(i as usize);
    }
    s
}

// ---------------------------------------------------------------------------
// allocationRemaining
// ---------------------------------------------------------------------------

unsafe fn allocationRemaining(initial: f64, remaining: f64) -> bool {
    if initial == 0.0 {
        true
    } else if initial > 0.0 {
        remaining > 0.0
    } else {
        remaining < 0.0
    }
}

// ---------------------------------------------------------------------------
// calcViewportLayout — STUB (partially implemented structure)
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe extern "C" fn calcViewportLayout(
    viewport: SEXP,
    parentWidthCM: f64,
    parentHeightCM: f64,
    parentContext: LViewportContext,
    parentgc: *const u8,
    dd: *const u8,
) {
    // STUB: requires transformWidth, transformHeight, pureNullUnit,
    //        pureNullUnitValue from unit.c
    //
    // Full implementation requires:
    //   findRelWidths, findRelHeights, allocateKnownWidths, allocateKnownHeights,
    //   allocateRespected, allocateRemainingWidth, allocateRemainingHeight,
    //   all of which depend on transformWidth/transformHeight from unit.c
    let _ = (
        viewport,
        parentWidthCM,
        parentHeightCM,
        parentContext,
        parentgc,
        dd,
    );
}

// ---------------------------------------------------------------------------
// checkPosRowPosCol
// ---------------------------------------------------------------------------

use crate::library::grid::viewport::{viewportLayout, viewportLayoutPosCol, viewportLayoutPosRow};

#[unsafe(no_mangle)]
pub unsafe extern "C" fn checkPosRowPosCol(vp: SEXP, parent: SEXP) -> bool {
    let parent_layout = viewportLayout(parent);
    let ncol = layoutNCol(parent_layout);
    let nrow = layoutNRow(parent_layout);
    if Rf_isNull(viewportLayoutPosRow(vp)) == 0 {
        let lpr = viewportLayoutPosRow(vp);
        if *INTEGER(lpr) < 1 || *INTEGER(lpr).add(1) > nrow {
            return false;
        }
    }
    if Rf_isNull(viewportLayoutPosCol(vp)) == 0 {
        let lpc = viewportLayoutPosCol(vp);
        if *INTEGER(lpc) < 1 || *INTEGER(lpc).add(1) > ncol {
            return false;
        }
    }
    true
}

// ---------------------------------------------------------------------------
// calcViewportLocationFromLayout — STUB
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe extern "C" fn calcViewportLocationFromLayout(
    _layoutPosRow: SEXP,
    _layoutPosCol: SEXP,
    _parent: SEXP,
    _vpl: *mut LViewportLocation,
) {
    // STUB: requires unit() from unit.c, subRegion, and viewport accessors
}
