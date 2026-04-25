#![allow(unsafe_op_in_unsafe_fn)]
// legacy C-port unsafe boundary; see docs/unsafe-op-allowlist.tsv.
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
use crate::sexp::envir::findFun;
use crate::sexp::ffi::{R_xlen_t, SEXP, SEXPTYPE};
use crate::sexp::globals::R_GlobalEnv;
use crate::sexp::globals::R_NilValue;
use crate::sexp::protect::{Rf_protect, Rf_unprotect};
use crate::sexp::symbol::Rf_install;

use super::gpar::gcontextFromgpar;
use super::grid::{doSetViewport, getDeviceSize};
use super::just::{justifyX, justifyY};
use super::layout::calcViewportLayout;
use super::state::{gridStateElement, setGridStateElement};
use super::types::*;
use super::unit::{
    LViewportContext as UnitViewportContext, transformHeighttoINCHES, transformWidthtoINCHES,
    transformXtoINCHES, transformYtoINCHES,
};

// ---------------------------------------------------------------------------
// Local helper: numeric(x, index) — equivalent to REAL(x)[index]
// (from util.c, not yet ported as a separate module)
// ---------------------------------------------------------------------------

unsafe fn numeric(x: SEXP, index: c_int) -> f64 {
    *REAL(x).add(index as usize)
}

unsafe fn scalar_real_or(x: SEXP, default_value: f64) -> f64 {
    if !x.is_null()
        && Rf_isNull(x) == 0
        && TYPEOF(x) == SEXPTYPE::REALSXP.as_c_int()
        && LENGTH(x) > 0
    {
        *REAL(x)
    } else {
        default_value
    }
}

// ---------------------------------------------------------------------------
// Local helper: isLogical
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
unsafe fn isLogical(x: SEXP) -> bool {
    !x.is_null() && TYPEOF(x) == SEXPTYPE::LGLSXP
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
    let s = Rf_allocVector(SEXPTYPE::REALSXP, 1);
    *REAL(s) = x;
    s
}

unsafe fn lang1(fn_: SEXP) -> SEXP {
    let call = Rf_cons(fn_, R_NilValue());
    if !call.is_null() {
        (*call).sxpinfo.set_type(SEXPTYPE::LANGSXP);
    }
    call
}

// ---------------------------------------------------------------------------
// Local helper: allocMatrix
// ---------------------------------------------------------------------------

unsafe fn allocMatrix(sexptype: c_int, nrow: c_int, ncol: c_int) -> SEXP {
    let dims = Rf_allocVector(SEXPTYPE::INTSXP, 2);
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
// gcontextFromViewport — delegate through gpar context resolution.
// ---------------------------------------------------------------------------

pub unsafe fn gcontextFromViewport(vp: SEXP, gc: pGEcontext, dd: pGEDevDesc) {
    let gpar = viewportgpar(vp);
    if gpar.is_null() || Rf_isNull(gpar) != 0 {
        return;
    }

    // Narrow limitation: the opaque GEcontext layout still blocks direct field
    // writes here, but we can at least resolve/validate the viewport gpar
    // path through the shared gpar accessor.
    gcontextFromgpar(gpar, 0, gc, dd);
}

pub unsafe fn calcViewportTransform(vp: SEXP, parent: SEXP, _incremental: bool, dd: pGEDevDesc) {
    let parent_context = if parent.is_null() || Rf_isNull(parent) != 0 {
        LViewportContext {
            xscalemin: 0.0,
            xscalemax: 1.0,
            yscalemin: 0.0,
            yscalemax: 1.0,
        }
    } else {
        LViewportContext {
            xscalemin: viewportXScaleMin(parent),
            xscalemax: viewportXScaleMax(parent),
            yscalemin: viewportYScaleMin(parent),
            yscalemax: viewportYScaleMax(parent),
        }
    };
    let unit_parent_context = UnitViewportContext {
        xscalemin: parent_context.xscalemin,
        xscalemax: parent_context.xscalemax,
        yscalemin: parent_context.yscalemin,
        yscalemax: parent_context.yscalemax,
    };

    let mut parent_width_cm = if parent.is_null() || Rf_isNull(parent) != 0 {
        scalar_real_or(viewportDevWidthCM(vp), 1.0)
    } else {
        scalar_real_or(
            viewportWidthCM(parent),
            scalar_real_or(viewportDevWidthCM(parent), 1.0),
        )
    };
    let mut parent_height_cm = if parent.is_null() || Rf_isNull(parent) != 0 {
        scalar_real_or(viewportDevHeightCM(vp), 1.0)
    } else {
        scalar_real_or(
            viewportHeightCM(parent),
            scalar_real_or(viewportDevHeightCM(parent), 1.0),
        )
    };
    if !parent_width_cm.is_finite() || parent_width_cm <= 0.0 {
        parent_width_cm = 1.0;
    }
    if !parent_height_cm.is_finite() || parent_height_cm <= 0.0 {
        parent_height_cm = 1.0;
    }

    let mut gc_buf: [u8; 256] = [0; 256];
    let gc = gc_buf.as_ptr() as pGEcontext;
    gcontextFromViewport(vp, gc, dd);
    let dd = dd as pGEDevDesc;

    let width_in = transformWidthtoINCHES(
        viewportWidth(vp),
        0,
        unit_parent_context,
        gc,
        parent_width_cm,
        parent_height_cm,
        dd,
    );
    let height_in = transformHeighttoINCHES(
        viewportHeight(vp),
        0,
        unit_parent_context,
        gc,
        parent_width_cm,
        parent_height_cm,
        dd,
    );
    let x_in = transformXtoINCHES(
        viewportX(vp),
        0,
        unit_parent_context,
        gc,
        parent_width_cm,
        parent_height_cm,
        dd,
    );
    let y_in = transformYtoINCHES(
        viewportY(vp),
        0,
        unit_parent_context,
        gc,
        parent_width_cm,
        parent_height_cm,
        dd,
    );

    let left_in = justifyX(x_in, width_in, viewportHJust(vp));
    let bottom_in = justifyY(y_in, height_in, viewportVJust(vp));
    let width_cm = width_in * 2.54;
    let height_cm = height_in * 2.54;

    SET_VECTOR_ELT(vp, PVP_WIDTHCM as R_xlen_t, ScalarReal(width_cm));
    SET_VECTOR_ELT(vp, PVP_HEIGHTCM as R_xlen_t, ScalarReal(height_cm));
    SET_VECTOR_ELT(vp, PVP_ROTATION as R_xlen_t, ScalarReal(viewportAngle(vp)));

    let transform = allocMatrix(SEXPTYPE::REALSXP.as_c_int(), 3, 3);
    for i in 0..9usize {
        *REAL(transform).add(i) = 0.0;
    }
    *REAL(transform).add(0) = 1.0;
    *REAL(transform).add(4) = 1.0;
    *REAL(transform).add(8) = 1.0;
    *REAL(transform).add(6) = left_in;
    *REAL(transform).add(7) = bottom_in;
    SET_VECTOR_ELT(vp, PVP_TRANS as R_xlen_t, transform);

    let clip = Rf_allocVector(SEXPTYPE::REALSXP, 4);
    *REAL(clip).add(0) = left_in;
    *REAL(clip).add(1) = bottom_in;
    *REAL(clip).add(2) = left_in + width_in;
    *REAL(clip).add(3) = bottom_in + height_in;
    SET_VECTOR_ELT(vp, PVP_CLIPRECT as R_xlen_t, clip);

    if Rf_isNull(viewportLayout(vp)) == 0 {
        calcViewportLayout(
            vp,
            parent_width_cm,
            parent_height_cm,
            parent_context,
            gc_buf.as_ptr(),
            dd as *const u8,
        );
    }
}

// ---------------------------------------------------------------------------
// initVP — initialize the top-level viewport for the current device
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe fn initVP(dd: *const u8) {
    let dd = dd as pGEDevDesc;

    let vpfnname = Rf_protect(Rf_install(b"grid.top.level.vp\0".as_ptr() as *const c_char));
    let vpfn_env = grid_eval_env();
    let vpfn = Rf_protect(lang1(findFun(vpfnname, vpfn_env)));
    let vp = Rf_protect(Rf_eval(vpfn, R_GlobalEnv()));

    let mut dev_width_cm: c_double = 0.0;
    let mut dev_height_cm: c_double = 0.0;
    getDeviceSize(dd, &mut dev_width_cm, &mut dev_height_cm);

    let xscale = Rf_protect(Rf_allocVector(SEXPTYPE::REALSXP, 2));
    *REAL(xscale).add(0) = 0.0;
    *REAL(xscale).add(1) = dev_width_cm;
    SET_VECTOR_ELT(vp, VP_XSCALE as R_xlen_t, xscale);

    let yscale = Rf_protect(Rf_allocVector(SEXPTYPE::REALSXP, 2));
    *REAL(yscale).add(0) = 0.0;
    *REAL(yscale).add(1) = dev_height_cm;
    SET_VECTOR_ELT(vp, VP_YSCALE as R_xlen_t, yscale);

    let currentgp = gridStateElement(dd, GSS_GPAR);
    SET_VECTOR_ELT(vp, PVP_GPAR as R_xlen_t, currentgp);

    let vp = doSetViewport(vp, 1, 1, dd);
    setGridStateElement(dd, GSS_VP, vp);

    Rf_unprotect(5);
}
