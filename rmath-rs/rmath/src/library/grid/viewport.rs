/*
 *  R : A Computer Language for Statistical Data Analysis
 *  Copyright (C) 2001-3 Paul Murrell
 *                2003-2014 The R Core Team
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

//! Port of R's src/library/grid/src/viewport.c
//!
//! Viewport accessor functions and viewport transform/layout calculation.

use std::os::raw::{c_char, c_double, c_int};

use crate::eval::eval::Rf_eval;
use crate::sexp::accessors::{
    CHAR, INTEGER, LENGTH, LOGICAL, REAL, Rf_isNull, SET_VECTOR_ELT, STRING_ELT, TYPEOF, VECTOR_ELT,
};
use crate::sexp::constructors::Rf_allocVector;
use crate::sexp::constructors::Rf_cons;
use crate::sexp::constructors::Rf_mkString;
use crate::sexp::envir::findFun;
use crate::sexp::ffi::{R_xlen_t, SEXP, SEXPTYPE};
use crate::sexp::globals::R_NilValue;
use crate::sexp::protect::{Rf_protect, Rf_unprotect};
use crate::sexp::symbol::Rf_install;

use super::types::*;

// ---------------------------------------------------------------------------
// Local helper: numeric(x, index) — equivalent to REAL(x)[index]
// (from util.c, not yet ported as a separate module)
// ---------------------------------------------------------------------------

unsafe fn numeric(x: SEXP, index: c_int) -> f64 {
    *REAL(x).add(index as usize)
}

// ---------------------------------------------------------------------------
// Local helper: isLogical
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
unsafe fn isLogical(x: SEXP) -> bool {
    !x.is_null() && TYPEOF(x) == SEXPTYPE::LGLSXP.0
}

// ---------------------------------------------------------------------------
// Local helper: asBool
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
unsafe fn asBool(x: SEXP) -> bool {
    if isLogical(x) {
        *LOGICAL(x) != 0
    } else {
        false
    }
}

// ---------------------------------------------------------------------------
// Local helper: ScalarReal
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
unsafe fn ScalarReal(x: f64) -> SEXP {
    let s = Rf_allocVector(SEXPTYPE::REALSXP.0, 1);
    *REAL(s) = x;
    s
}

// ---------------------------------------------------------------------------
// Local helper: allocMatrix
// ---------------------------------------------------------------------------

unsafe fn allocMatrix(sexptype: c_int, nrow: c_int, ncol: c_int) -> SEXP {
    let dims = Rf_allocVector(SEXPTYPE::INTSXP.0, 2);
    *INTEGER(dims) = nrow;
    *INTEGER(dims).add(1) = ncol;
    let result = Rf_allocVector(sexptype, nrow * ncol);
    crate::attrib_core::setAttrib(result, crate::attrib_core::R_DimSymbol(), dims);
    result
}

// ---------------------------------------------------------------------------
// Simple viewport accessor functions
// ---------------------------------------------------------------------------

pub unsafe fn viewportX(vp: SEXP) -> SEXP {
    VECTOR_ELT(vp, VP_X as R_xlen_t)
}

pub unsafe fn viewportY(vp: SEXP) -> SEXP {
    VECTOR_ELT(vp, VP_Y as R_xlen_t)
}

pub unsafe fn viewportWidth(vp: SEXP) -> SEXP {
    VECTOR_ELT(vp, VP_WIDTH as R_xlen_t)
}

pub unsafe fn viewportHeight(vp: SEXP) -> SEXP {
    VECTOR_ELT(vp, VP_HEIGHT as R_xlen_t)
}

pub unsafe fn viewportClipSXP(vp: SEXP) -> SEXP {
    VECTOR_ELT(vp, VP_CLIP as R_xlen_t)
}

// This can be NA_LOGICAL, and it is tested for that in grd.c
pub unsafe fn viewportClip(vp: SEXP) -> c_int {
    *LOGICAL(VECTOR_ELT(vp, VP_CLIP as R_xlen_t))
}

pub unsafe fn viewportMaskSXP(vp: SEXP) -> SEXP {
    VECTOR_ELT(vp, VP_MASK as R_xlen_t)
}

pub unsafe fn viewportMask(vp: SEXP) -> bool {
    let mask = viewportMaskSXP(vp);
    if !isLogical(mask) {
        return false;
    }
    asBool(VECTOR_ELT(vp, VP_MASK as R_xlen_t))
}

pub unsafe fn viewportXScaleMin(vp: SEXP) -> f64 {
    numeric(VECTOR_ELT(vp, VP_XSCALE as R_xlen_t), 0)
}

pub unsafe fn viewportXScaleMax(vp: SEXP) -> f64 {
    numeric(VECTOR_ELT(vp, VP_XSCALE as R_xlen_t), 1)
}

pub unsafe fn viewportYScaleMin(vp: SEXP) -> f64 {
    numeric(VECTOR_ELT(vp, VP_YSCALE as R_xlen_t), 0)
}

pub unsafe fn viewportYScaleMax(vp: SEXP) -> f64 {
    numeric(VECTOR_ELT(vp, VP_YSCALE as R_xlen_t), 1)
}

pub unsafe fn viewportAngle(vp: SEXP) -> f64 {
    numeric(VECTOR_ELT(vp, VP_ANGLE as R_xlen_t), 0)
}

pub unsafe fn viewportLayout(vp: SEXP) -> SEXP {
    VECTOR_ELT(vp, VP_LAYOUT as R_xlen_t)
}

pub unsafe fn viewportHJust(vp: SEXP) -> f64 {
    *REAL(VECTOR_ELT(vp, VP_VALIDJUST as R_xlen_t))
}

pub unsafe fn viewportVJust(vp: SEXP) -> f64 {
    *REAL(VECTOR_ELT(vp, VP_VALIDJUST as R_xlen_t)).add(1)
}

pub unsafe fn viewportLayoutPosRow(vp: SEXP) -> SEXP {
    VECTOR_ELT(vp, VP_VALIDLPOSROW as R_xlen_t)
}

pub unsafe fn viewportLayoutPosCol(vp: SEXP) -> SEXP {
    VECTOR_ELT(vp, VP_VALIDLPOSCOL as R_xlen_t)
}

pub unsafe fn viewportgpar(vp: SEXP) -> SEXP {
    VECTOR_ELT(vp, PVP_GPAR as R_xlen_t)
}

pub unsafe fn viewportFontFamily(vp: SEXP) -> *const c_char {
    CHAR(STRING_ELT(
        VECTOR_ELT(
            VECTOR_ELT(vp, PVP_GPAR as R_xlen_t),
            GP_FONTFAMILY as R_xlen_t,
        ),
        0,
    ))
}

pub unsafe fn viewportFont(vp: SEXP) -> c_int {
    *INTEGER(VECTOR_ELT(
        VECTOR_ELT(vp, PVP_GPAR as R_xlen_t),
        GP_FONT as R_xlen_t,
    ))
}

pub unsafe fn viewportFontSize(vp: SEXP) -> f64 {
    *REAL(VECTOR_ELT(
        VECTOR_ELT(vp, PVP_GPAR as R_xlen_t),
        GP_FONTSIZE as R_xlen_t,
    ))
}

pub unsafe fn viewportLineHeight(vp: SEXP) -> f64 {
    *REAL(VECTOR_ELT(
        VECTOR_ELT(vp, PVP_GPAR as R_xlen_t),
        GP_LINEHEIGHT as R_xlen_t,
    ))
}

pub unsafe fn viewportCex(vp: SEXP) -> f64 {
    numeric(
        VECTOR_ELT(VECTOR_ELT(vp, PVP_GPAR as R_xlen_t), GP_CEX as R_xlen_t),
        0,
    )
}

pub unsafe fn viewportTransform(vp: SEXP) -> SEXP {
    VECTOR_ELT(vp, PVP_TRANS as R_xlen_t)
}

pub unsafe fn viewportLayoutWidths(vp: SEXP) -> SEXP {
    VECTOR_ELT(vp, PVP_WIDTHS as R_xlen_t)
}

pub unsafe fn viewportLayoutHeights(vp: SEXP) -> SEXP {
    VECTOR_ELT(vp, PVP_HEIGHTS as R_xlen_t)
}

pub unsafe fn viewportWidthCM(vp: SEXP) -> SEXP {
    VECTOR_ELT(vp, PVP_WIDTHCM as R_xlen_t)
}

pub unsafe fn viewportHeightCM(vp: SEXP) -> SEXP {
    VECTOR_ELT(vp, PVP_HEIGHTCM as R_xlen_t)
}

pub unsafe fn viewportRotation(vp: SEXP) -> SEXP {
    VECTOR_ELT(vp, PVP_ROTATION as R_xlen_t)
}

pub unsafe fn viewportClipRect(vp: SEXP) -> SEXP {
    VECTOR_ELT(vp, PVP_CLIPRECT as R_xlen_t)
}

pub unsafe fn viewportParent(vp: SEXP) -> SEXP {
    VECTOR_ELT(vp, PVP_PARENT as R_xlen_t)
}

pub unsafe fn viewportChildren(vp: SEXP) -> SEXP {
    VECTOR_ELT(vp, PVP_CHILDREN as R_xlen_t)
}

pub unsafe fn viewportDevWidthCM(vp: SEXP) -> SEXP {
    VECTOR_ELT(vp, PVP_DEVWIDTHCM as R_xlen_t)
}

pub unsafe fn viewportDevHeightCM(vp: SEXP) -> SEXP {
    VECTOR_ELT(vp, PVP_DEVHEIGHTCM as R_xlen_t)
}

pub unsafe fn viewportParentGPar(vp: SEXP) -> SEXP {
    VECTOR_ELT(vp, PVP_PARENTGPAR as R_xlen_t)
}

// ---------------------------------------------------------------------------
// fillViewportLocationFromViewport / fillViewportContextFromViewport
// ---------------------------------------------------------------------------

pub unsafe fn fillViewportLocationFromViewport(vp: SEXP, vpl: *mut LViewportLocation) {
    (*vpl).x = viewportX(vp);
    (*vpl).y = viewportY(vp);
    (*vpl).width = viewportWidth(vp);
    (*vpl).height = viewportHeight(vp);
    (*vpl).hjust = viewportHJust(vp);
    (*vpl).vjust = viewportVJust(vp);
}

pub unsafe fn fillViewportContextFromViewport(vp: SEXP, vpc: *mut LViewportContext) {
    (*vpc).xscalemin = viewportXScaleMin(vp);
    (*vpc).xscalemax = viewportXScaleMax(vp);
    (*vpc).yscalemin = viewportYScaleMin(vp);
    (*vpc).yscalemax = viewportYScaleMax(vp);
}

pub unsafe fn copyViewportContext(vpc1: LViewportContext, vpc2: *mut LViewportContext) {
    (*vpc2).xscalemin = vpc1.xscalemin;
    (*vpc2).xscalemax = vpc1.xscalemax;
    (*vpc2).yscalemin = vpc1.yscalemin;
    (*vpc2).yscalemax = vpc1.yscalemax;
}

// ---------------------------------------------------------------------------
// gcontextFromViewport — STUB: requires gpar.c
// ---------------------------------------------------------------------------

pub unsafe fn gcontextFromViewport(
    _vp: SEXP,
    _gc: *const u8, // pGEcontext — opaque until GraphicsEngine is ported
    _dd: *const u8, // pGEDevDesc
) {
    // STUB: requires gcontextFromgpar from gpar.c
}

// ---------------------------------------------------------------------------
// calcViewportTransform — STUB: requires unit.c, gpar.c, grid.c
// ---------------------------------------------------------------------------

pub unsafe fn calcViewportTransform(
    _vp: SEXP,
    _parent: SEXP,
    _incremental: bool,
    _dd: *const u8, // pGEDevDesc
) {
    // STUB: requires transformXtoINCHES, transformYtoINCHES,
    //        transformWidthtoINCHES, transformHeighttoINCHES from unit.c,
    //        gcontextFromgpar from gpar.c,
    //        getDeviceSize, checkPosRowPosCol from grid.c
}

// ---------------------------------------------------------------------------
// initVP — STUB: requires grid.c, state.c
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe fn initVP(_dd: *const u8) {
    // pGEDevDesc
    // STUB: requires gridStateElement, findFun, Rf_eval_with_gd,
    //        doSetViewport, setGridStateElement from grid.c/state.c
}
