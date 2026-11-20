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
use super::protect;
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
use super::{
    alloc_vector, char_ptr, install, integer_elt, logical_elt, real_elt, scalar_real,
    set_integer_elt, set_real_elt, set_vector_elt, string_elt, vector_elt,
};

// ---------------------------------------------------------------------------
// Local helper: numeric(x, index) — equivalent to REAL(x)[index]
// (from util.c, not yet ported as a separate module)
// ---------------------------------------------------------------------------

fn numeric(x: SEXP, index: c_int) -> f64 {
    real_elt(x, index as R_xlen_t)
}

fn scalar_real_or(x: SEXP, default_value: f64) -> f64 {
    if !x.is_null()
        && Rf_isNull(x) == 0
        && TYPEOF(x) == SEXPTYPE::REALSXP.as_c_int()
        && LENGTH(x) > 0
    {
        real_elt(x, 0)
    } else {
        default_value
    }
}

// ---------------------------------------------------------------------------
// Local helper: isLogical
// ---------------------------------------------------------------------------

fn isLogical(x: SEXP) -> bool {
    !x.is_null() && TYPEOF(x) == SEXPTYPE::LGLSXP
}

// ---------------------------------------------------------------------------
// Local helper: asBool
// ---------------------------------------------------------------------------

fn asBool(x: SEXP) -> bool {
    if isLogical(x) {
        logical_elt(x, 0) != 0
    } else {
        false
    }
}

// ---------------------------------------------------------------------------
// Local helper: ScalarReal
// ---------------------------------------------------------------------------

fn ScalarReal(x: f64) -> SEXP {
    scalar_real(x)
}

unsafe fn lang1(fn_: SEXP) -> SEXP {
    unsafe {
        let call = Rf_cons(fn_, R_NilValue());
        if !call.is_null() {
            crate::sexp::accessors::SET_TYPEOF(call, SEXPTYPE::LANGSXP.as_c_int());
        }
        call
    }
}

// ---------------------------------------------------------------------------
// Local helper: allocMatrix
// ---------------------------------------------------------------------------

unsafe fn allocMatrix(sexptype: c_int, nrow: c_int, ncol: c_int) -> SEXP {
    unsafe {
        let dims = alloc_vector(SEXPTYPE::INTSXP, 2);
        set_integer_elt(dims, 0, nrow);
        set_integer_elt(dims, 1, ncol);
        let result = Rf_allocVector(sexptype, nrow * ncol);
        crate::attrib_core::setAttrib(result, crate::attrib_core::R_DimSymbol(), dims);
        result
    }
}

// ---------------------------------------------------------------------------
// Simple viewport accessor functions
// ---------------------------------------------------------------------------

pub fn viewportX(vp: SEXP) -> SEXP {
    vector_elt(vp, VP_X as R_xlen_t)
}

pub fn viewportY(vp: SEXP) -> SEXP {
    vector_elt(vp, VP_Y as R_xlen_t)
}

pub fn viewportWidth(vp: SEXP) -> SEXP {
    vector_elt(vp, VP_WIDTH as R_xlen_t)
}

pub fn viewportHeight(vp: SEXP) -> SEXP {
    vector_elt(vp, VP_HEIGHT as R_xlen_t)
}

pub fn viewportClipSXP(vp: SEXP) -> SEXP {
    vector_elt(vp, VP_CLIP as R_xlen_t)
}

// This can be NA_LOGICAL, and it is tested for that in grd.c
pub fn viewportClip(vp: SEXP) -> c_int {
    logical_elt(vector_elt(vp, VP_CLIP as R_xlen_t), 0)
}

pub fn viewportMaskSXP(vp: SEXP) -> SEXP {
    vector_elt(vp, VP_MASK as R_xlen_t)
}

pub fn viewportMask(vp: SEXP) -> bool {
    let mask = viewportMaskSXP(vp);
    if !isLogical(mask) {
        return false;
    }
    asBool(vector_elt(vp, VP_MASK as R_xlen_t))
}

pub fn viewportXScaleMin(vp: SEXP) -> f64 {
    real_elt(vector_elt(vp, VP_XSCALE as R_xlen_t), 0)
}

pub fn viewportXScaleMax(vp: SEXP) -> f64 {
    real_elt(vector_elt(vp, VP_XSCALE as R_xlen_t), 1)
}

pub fn viewportYScaleMin(vp: SEXP) -> f64 {
    real_elt(vector_elt(vp, VP_YSCALE as R_xlen_t), 0)
}

pub fn viewportYScaleMax(vp: SEXP) -> f64 {
    real_elt(vector_elt(vp, VP_YSCALE as R_xlen_t), 1)
}

pub fn viewportAngle(vp: SEXP) -> f64 {
    real_elt(vector_elt(vp, VP_ANGLE as R_xlen_t), 0)
}

pub fn viewportLayout(vp: SEXP) -> SEXP {
    vector_elt(vp, VP_LAYOUT as R_xlen_t)
}

pub fn viewportHJust(vp: SEXP) -> f64 {
    real_elt(vector_elt(vp, VP_VALIDJUST as R_xlen_t), 0)
}

pub fn viewportVJust(vp: SEXP) -> f64 {
    real_elt(vector_elt(vp, VP_VALIDJUST as R_xlen_t), 1)
}

pub fn viewportLayoutPosRow(vp: SEXP) -> SEXP {
    vector_elt(vp, VP_VALIDLPOSROW as R_xlen_t)
}

pub fn viewportLayoutPosCol(vp: SEXP) -> SEXP {
    vector_elt(vp, VP_VALIDLPOSCOL as R_xlen_t)
}

pub fn viewportgpar(vp: SEXP) -> SEXP {
    vector_elt(vp, PVP_GPAR as R_xlen_t)
}

pub fn viewportFontFamily(vp: SEXP) -> *const c_char {
    char_ptr(string_elt(
        vector_elt(vector_elt(vp, PVP_GPAR as R_xlen_t), GP_FONTFAMILY as R_xlen_t),
        0,
    ))
}

pub fn viewportFont(vp: SEXP) -> c_int {
    integer_elt(
        vector_elt(vector_elt(vp, PVP_GPAR as R_xlen_t), GP_FONT as R_xlen_t),
        0,
    )
}

pub fn viewportFontSize(vp: SEXP) -> f64 {
    real_elt(
        vector_elt(vector_elt(vp, PVP_GPAR as R_xlen_t), GP_FONTSIZE as R_xlen_t),
        0,
    )
}

pub fn viewportLineHeight(vp: SEXP) -> f64 {
    real_elt(
        vector_elt(vector_elt(vp, PVP_GPAR as R_xlen_t), GP_LINEHEIGHT as R_xlen_t),
        0,
    )
}

pub fn viewportCex(vp: SEXP) -> f64 {
    real_elt(
        vector_elt(vector_elt(vp, PVP_GPAR as R_xlen_t), GP_CEX as R_xlen_t),
        0,
    )
}

pub fn viewportTransform(vp: SEXP) -> SEXP {
    vector_elt(vp, PVP_TRANS as R_xlen_t)
}

pub fn viewportLayoutWidths(vp: SEXP) -> SEXP {
    vector_elt(vp, PVP_WIDTHS as R_xlen_t)
}

pub fn viewportLayoutHeights(vp: SEXP) -> SEXP {
    vector_elt(vp, PVP_HEIGHTS as R_xlen_t)
}

pub fn viewportWidthCM(vp: SEXP) -> SEXP {
    vector_elt(vp, PVP_WIDTHCM as R_xlen_t)
}

pub fn viewportHeightCM(vp: SEXP) -> SEXP {
    vector_elt(vp, PVP_HEIGHTCM as R_xlen_t)
}

pub fn viewportRotation(vp: SEXP) -> SEXP {
    vector_elt(vp, PVP_ROTATION as R_xlen_t)
}

pub fn viewportClipRect(vp: SEXP) -> SEXP {
    vector_elt(vp, PVP_CLIPRECT as R_xlen_t)
}

pub fn viewportParent(vp: SEXP) -> SEXP {
    vector_elt(vp, PVP_PARENT as R_xlen_t)
}

pub fn viewportChildren(vp: SEXP) -> SEXP {
    vector_elt(vp, PVP_CHILDREN as R_xlen_t)
}

pub fn viewportDevWidthCM(vp: SEXP) -> SEXP {
    vector_elt(vp, PVP_DEVWIDTHCM as R_xlen_t)
}

pub fn viewportDevHeightCM(vp: SEXP) -> SEXP {
    vector_elt(vp, PVP_DEVHEIGHTCM as R_xlen_t)
}

pub fn viewportParentGPar(vp: SEXP) -> SEXP {
    vector_elt(vp, PVP_PARENTGPAR as R_xlen_t)
}

// ---------------------------------------------------------------------------
// fillViewportLocationFromViewport / fillViewportContextFromViewport
// ---------------------------------------------------------------------------

pub fn fillViewportLocationFromViewport(vp: SEXP, vpl: &mut LViewportLocation) {
    vpl.x = viewportX(vp);
    vpl.y = viewportY(vp);
    vpl.width = viewportWidth(vp);
    vpl.height = viewportHeight(vp);
    vpl.hjust = viewportHJust(vp);
    vpl.vjust = viewportVJust(vp);
}

pub fn fillViewportContextFromViewport(vp: SEXP, vpc: &mut LViewportContext) {
    vpc.xscalemin = viewportXScaleMin(vp);
    vpc.xscalemax = viewportXScaleMax(vp);
    vpc.yscalemin = viewportYScaleMin(vp);
    vpc.yscalemax = viewportYScaleMax(vp);
}

pub fn copyViewportContext(vpc1: LViewportContext, vpc2: &mut LViewportContext) {
    vpc2.xscalemin = vpc1.xscalemin;
    vpc2.xscalemax = vpc1.xscalemax;
    vpc2.yscalemin = vpc1.yscalemin;
    vpc2.yscalemax = vpc1.yscalemax;
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
    // SAFETY: raw context/device pointers keep their caller contracts.
    unsafe { gcontextFromgpar(gpar, 0, gc, dd) };
}

pub unsafe fn calcViewportTransform(vp: SEXP, parent: SEXP, _incremental: bool, dd: pGEDevDesc) {
    unsafe {
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

        set_vector_elt(vp, PVP_WIDTHCM as R_xlen_t, ScalarReal(width_cm));
        set_vector_elt(vp, PVP_HEIGHTCM as R_xlen_t, ScalarReal(height_cm));
        set_vector_elt(vp, PVP_ROTATION as R_xlen_t, ScalarReal(viewportAngle(vp)));

        let transform = allocMatrix(SEXPTYPE::REALSXP.as_c_int(), 3, 3);
        for i in 0..9usize {
            set_real_elt(transform, i as R_xlen_t, 0.0);
        }
        set_real_elt(transform, 0, 1.0);
        set_real_elt(transform, 4, 1.0);
        set_real_elt(transform, 8, 1.0);
        set_real_elt(transform, 6, left_in);
        set_real_elt(transform, 7, bottom_in);
        set_vector_elt(vp, PVP_TRANS as R_xlen_t, transform);

        let clip = Rf_allocVector(SEXPTYPE::REALSXP, 4);
        set_real_elt(clip, 0, left_in);
        set_real_elt(clip, 1, bottom_in);
        set_real_elt(clip, 2, left_in + width_in);
        set_real_elt(clip, 3, bottom_in + height_in);
        set_vector_elt(vp, PVP_CLIPRECT as R_xlen_t, clip);

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
}

// ---------------------------------------------------------------------------
// initVP — initialize the top-level viewport for the current device
// ---------------------------------------------------------------------------

pub unsafe fn initVP(dd: *const u8) {
    unsafe {
        let dd = dd as pGEDevDesc;

        let vpfnname = install("grid.top.level.vp");
        let _vpfnname_guard = protect(vpfnname);
        let vpfn_env = grid_eval_env();
        let vpfn = lang1(findFun(vpfnname, vpfn_env));
        let _vpfn_guard = protect(vpfn);
        let vp = Rf_eval(vpfn, R_GlobalEnv());
        let _vp_guard = protect(vp);

        let (dev_width_cm, dev_height_cm) = getDeviceSize(dd);

        let xscale = Rf_allocVector(SEXPTYPE::REALSXP, 2);
        let _xscale_guard = protect(xscale);
        set_real_elt(xscale, 0, 0.0);
        set_real_elt(xscale, 1, dev_width_cm);
        set_vector_elt(vp, VP_XSCALE as R_xlen_t, xscale);

        let yscale = Rf_allocVector(SEXPTYPE::REALSXP, 2);
        let _yscale_guard = protect(yscale);
        set_real_elt(yscale, 0, 0.0);
        set_real_elt(yscale, 1, dev_height_cm);
        set_vector_elt(vp, VP_YSCALE as R_xlen_t, yscale);

        let currentgp = gridStateElement(dd, GSS_GPAR);
        set_vector_elt(vp, PVP_GPAR as R_xlen_t, currentgp);

        let vp = doSetViewport(vp, 1, 1, dd);
        setGridStateElement(dd, GSS_VP, vp);
    }
}
