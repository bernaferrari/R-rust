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

use crate::sexp::accessors::Rf_isNull;
use crate::sexp::ffi::{R_xlen_t, SEXP, SEXPTYPE};

use super::types::*;
use super::unit::{pureNullUnitValue, transformHeight, transformWidth, unit};
use super::{
    alloc_vector, integer_elt, protect, real_elt, set_real_elt, set_vector_elt, vector_elt,
};
use super::viewport::{
    viewportHeightCM, viewportLayout, viewportLayoutHeights, viewportLayoutPosCol,
    viewportLayoutPosRow, viewportLayoutWidths, viewportWidthCM,
};

// ---------------------------------------------------------------------------
// Simple layout accessor functions
// ---------------------------------------------------------------------------

pub fn layoutNRow(l: SEXP) -> c_int {
    integer_elt(vector_elt(l, LAYOUT_NROW as R_xlen_t), 0)
}

pub fn layoutNCol(l: SEXP) -> c_int {
    integer_elt(vector_elt(l, LAYOUT_NCOL as R_xlen_t), 0)
}

pub fn layoutWidths(l: SEXP) -> SEXP {
    vector_elt(l, LAYOUT_WIDTHS as R_xlen_t)
}

pub fn layoutHeights(l: SEXP) -> SEXP {
    vector_elt(l, LAYOUT_HEIGHTS as R_xlen_t)
}

pub fn layoutRespect(l: SEXP) -> c_int {
    integer_elt(vector_elt(l, LAYOUT_VRESPECT as R_xlen_t), 0)
}

pub fn layoutRespectMat(l: SEXP) -> SEXP {
    vector_elt(l, LAYOUT_MRESPECT as R_xlen_t)
}

pub fn layoutHJust(l: SEXP) -> f64 {
    real_elt(vector_elt(l, LAYOUT_VJUST as R_xlen_t), 0)
}

pub fn layoutVJust(l: SEXP) -> f64 {
    real_elt(vector_elt(l, LAYOUT_VJUST as R_xlen_t), 1)
}

// ---------------------------------------------------------------------------
// relativeUnit — classify pure-null units as relative.
// ---------------------------------------------------------------------------

fn relativeUnit(_unit: SEXP, _index: c_int, _dd: pGEDevDesc) -> bool {
    super::unit::pureNullUnit(_unit, _index, _dd) != 0
}

// ---------------------------------------------------------------------------
// findRelWidths / findRelHeights
// ---------------------------------------------------------------------------

fn findRelWidths(layout: SEXP, relativeWidths: &mut [c_int], dd: pGEDevDesc) {
    let widths = layoutWidths(layout);
    let ncol = layoutNCol(layout);
    for i in 0..ncol {
        relativeWidths[i as usize] = if relativeUnit(widths, i, dd) { 1 } else { 0 };
    }
}

fn findRelHeights(layout: SEXP, relativeHeights: &mut [c_int], dd: pGEDevDesc) {
    let heights = layoutHeights(layout);
    let nrow = layoutNRow(layout);
    for i in 0..nrow {
        relativeHeights[i as usize] = if relativeUnit(heights, i, dd) { 1 } else { 0 };
    }
}

// ---------------------------------------------------------------------------
// allocateKnownWidths / allocateKnownHeights
// ---------------------------------------------------------------------------

fn allocateKnownWidths(
    layout: SEXP,
    relativeWidths: &[c_int],
    parentWidthCM: f64,
    parentHeightCM: f64,
    parentContext: LViewportContext,
    parentgc: pGEcontext,
    dd: pGEDevDesc,
    npcWidths: &mut [f64],
    widthLeftCM: &mut f64,
) {
    let ncol = layoutNCol(layout);
    let widths = layoutWidths(layout);
    for i in 0..ncol {
        if relativeWidths[i as usize] == 0 {
            let width_cm = transformWidth(
                widths,
                i,
                parentContext,
                parentgc,
                parentWidthCM,
                parentHeightCM,
                0,
                0,
                dd,
            ) * 2.54;
            npcWidths[i as usize] = width_cm;
            *widthLeftCM -= width_cm;
        }
    }
}

fn allocateKnownHeights(
    layout: SEXP,
    relativeHeights: &[c_int],
    parentWidthCM: f64,
    parentHeightCM: f64,
    parentContext: LViewportContext,
    parentgc: pGEcontext,
    dd: pGEDevDesc,
    npcHeights: &mut [f64],
    heightLeftCM: &mut f64,
) {
    let nrow = layoutNRow(layout);
    let heights = layoutHeights(layout);
    for i in 0..nrow {
        if relativeHeights[i as usize] == 0 {
            let height_cm = transformHeight(
                heights,
                i,
                parentContext,
                parentgc,
                parentWidthCM,
                parentHeightCM,
                0,
                0,
                dd,
            ) * 2.54;
            npcHeights[i as usize] = height_cm;
            *heightLeftCM -= height_cm;
        }
    }
}

// ---------------------------------------------------------------------------
// colRespected / rowRespected
// ---------------------------------------------------------------------------

fn colRespected(col: c_int, layout: SEXP) -> c_int {
    let mut result: c_int = 0;
    let respect = layoutRespect(layout);
    let respect_mat = layoutRespectMat(layout);
    if respect == 1 {
        result = 1;
    } else {
        let nrow = layoutNRow(layout);
        for i in 0..nrow {
            if integer_elt(respect_mat, (col * nrow + i) as R_xlen_t) != 0 {
                result = 1;
                break;
            }
        }
    }
    result
}

fn rowRespected(row: c_int, layout: SEXP) -> c_int {
    let mut result: c_int = 0;
    let respect = layoutRespect(layout);
    let respect_mat = layoutRespectMat(layout);
    if respect == 1 {
        result = 1;
    } else {
        let ncol = layoutNCol(layout);
        for i in 0..ncol {
            if integer_elt(respect_mat, (i * layoutNRow(layout) + row) as R_xlen_t) != 0 {
                result = 1;
                break;
            }
        }
    }
    result
}

// ---------------------------------------------------------------------------
// totalWidth / totalHeight
// ---------------------------------------------------------------------------

fn totalWidth(
    _layout: SEXP,
    _relativeWidths: &[c_int],
    _parentContext: LViewportContext,
    _parentgc: pGEcontext,
    _dd: pGEDevDesc,
) -> f64 {
    let widths = layoutWidths(_layout);
    let mut total = 0.0;
    for i in 0..layoutNCol(_layout) {
        if _relativeWidths[i as usize] != 0 {
            total += pureNullUnitValue(widths, i);
        }
    }
    total
}

fn totalHeight(
    _layout: SEXP,
    _relativeHeights: &[c_int],
    _parentContext: LViewportContext,
    _parentgc: pGEcontext,
    _dd: pGEDevDesc,
) -> f64 {
    let heights = layoutHeights(_layout);
    let mut total = 0.0;
    for i in 0..layoutNRow(_layout) {
        if _relativeHeights[i as usize] != 0 {
            total += pureNullUnitValue(heights, i);
        }
    }
    total
}

// ---------------------------------------------------------------------------
// allocateRespected
// ---------------------------------------------------------------------------

fn allocateRespected(
    _layout: SEXP,
    _relativeWidths: &[c_int],
    _relativeHeights: &[c_int],
    _reducedWidthCM: &mut f64,
    _reducedHeightCM: &mut f64,
    _parentContext: LViewportContext,
    _parentgc: pGEcontext,
    _dd: pGEDevDesc,
    npcWidths: &mut [f64],
    npcHeights: &mut [f64],
) {
    let widths = layoutWidths(_layout);
    let heights = layoutHeights(_layout);
    let sum_width = totalWidth(_layout, _relativeWidths, _parentContext, _parentgc, _dd);
    let sum_height = totalHeight(_layout, _relativeHeights, _parentContext, _parentgc, _dd);

    let temp_width_cm = *_reducedWidthCM;
    let temp_height_cm = *_reducedHeightCM;
    let (mut denom, mut mult) = if temp_height_cm * sum_width > sum_height * temp_width_cm {
        (sum_width, temp_width_cm)
    } else {
        (sum_height, temp_height_cm)
    };

    for i in 0..layoutNCol(_layout) {
        if _relativeWidths[i as usize] != 0 && colRespected(i, _layout) != 0 {
            if sum_height == 0.0 {
                denom = sum_width;
                mult = temp_width_cm;
            }
            npcWidths[i as usize] = pureNullUnitValue(widths, i) / denom * mult;
            *_reducedWidthCM -= npcWidths[i as usize];
        }
    }

    for i in 0..layoutNRow(_layout) {
        if _relativeHeights[i as usize] != 0 && rowRespected(i, _layout) != 0 {
            if sum_width == 0.0 {
                denom = sum_height;
                mult = temp_height_cm;
            }
            npcHeights[i as usize] = pureNullUnitValue(heights, i) / denom * mult;
            *_reducedHeightCM -= npcHeights[i as usize];
        }
    }
}

// ---------------------------------------------------------------------------
// setRespectedZero
// ---------------------------------------------------------------------------

fn setRespectedZero(
    layout: SEXP,
    relativeWidths: &[c_int],
    relativeHeights: &[c_int],
    npcWidths: &mut [f64],
    npcHeights: &mut [f64],
) {
    let ncol = layoutNCol(layout);
    for i in 0..ncol {
        if relativeWidths[i as usize] != 0 && colRespected(i, layout) != 0 {
            npcWidths[i as usize] = 0.0;
        }
    }
    let nrow = layoutNRow(layout);
    for i in 0..nrow {
        if relativeHeights[i as usize] != 0 && rowRespected(i, layout) != 0 {
            npcHeights[i as usize] = 0.0;
        }
    }
}

// ---------------------------------------------------------------------------
// totalUnrespectedWidth / totalUnrespectedHeight
// ---------------------------------------------------------------------------

fn totalUnrespectedWidth(
    _layout: SEXP,
    _relativeWidths: &[c_int],
    _parentContext: LViewportContext,
    _parentgc: pGEcontext,
    _dd: pGEDevDesc,
) -> f64 {
    let widths = layoutWidths(_layout);
    let mut total = 0.0;
    for i in 0..layoutNCol(_layout) {
        if _relativeWidths[i as usize] != 0 && colRespected(i, _layout) == 0 {
            total += pureNullUnitValue(widths, i);
        }
    }
    total
}

fn totalUnrespectedHeight(
    _layout: SEXP,
    _relativeHeights: &[c_int],
    _parentContext: LViewportContext,
    _parentgc: pGEcontext,
    _dd: pGEDevDesc,
) -> f64 {
    let heights = layoutHeights(_layout);
    let mut total = 0.0;
    for i in 0..layoutNRow(_layout) {
        if _relativeHeights[i as usize] != 0 && rowRespected(i, _layout) == 0 {
            total += pureNullUnitValue(heights, i);
        }
    }
    total
}

// ---------------------------------------------------------------------------
// setRemainingWidthZero / setRemainingHeightZero
// ---------------------------------------------------------------------------

fn setRemainingWidthZero(layout: SEXP, relativeWidths: &[c_int], npcWidths: &mut [f64]) {
    let ncol = layoutNCol(layout);
    for i in 0..ncol {
        if relativeWidths[i as usize] != 0 && colRespected(i, layout) == 0 {
            npcWidths[i as usize] = 0.0;
        }
    }
}

fn setRemainingHeightZero(layout: SEXP, relativeHeights: &[c_int], npcHeights: &mut [f64]) {
    let nrow = layoutNRow(layout);
    for i in 0..nrow {
        if relativeHeights[i as usize] != 0 && rowRespected(i, layout) == 0 {
            npcHeights[i as usize] = 0.0;
        }
    }
}

// ---------------------------------------------------------------------------
// allocateRemainingWidth / allocateRemainingHeight
// ---------------------------------------------------------------------------

fn allocateRemainingWidth(
    _layout: SEXP,
    _relativeWidths: &[c_int],
    _remainingWidthCM: f64,
    _parentContext: LViewportContext,
    _parentgc: pGEcontext,
    _dd: pGEDevDesc,
    npcWidths: &mut [f64],
) {
    let sum_width =
        totalUnrespectedWidth(_layout, _relativeWidths, _parentContext, _parentgc, _dd);
    if sum_width > 0.0 {
        let widths = layoutWidths(_layout);
        for i in 0..layoutNCol(_layout) {
            if _relativeWidths[i as usize] != 0 && colRespected(i, _layout) == 0 {
                npcWidths[i as usize] =
                    _remainingWidthCM * pureNullUnitValue(widths, i) / sum_width;
            }
        }
    } else {
        setRemainingWidthZero(_layout, _relativeWidths, npcWidths);
    }
}

fn allocateRemainingHeight(
    _layout: SEXP,
    _relativeHeights: &[c_int],
    _remainingHeightCM: f64,
    _parentContext: LViewportContext,
    _parentgc: pGEcontext,
    _dd: pGEDevDesc,
    npcHeights: &mut [f64],
) {
    let sum_height =
        totalUnrespectedHeight(_layout, _relativeHeights, _parentContext, _parentgc, _dd);
    if sum_height > 0.0 {
        let heights = layoutHeights(_layout);
        for i in 0..layoutNRow(_layout) {
            if _relativeHeights[i as usize] != 0 && rowRespected(i, _layout) == 0 {
                npcHeights[i as usize] =
                    _remainingHeightCM * pureNullUnitValue(heights, i) / sum_height;
            }
        }
    } else {
        setRemainingHeightZero(_layout, _relativeHeights, npcHeights);
    }
}

// ---------------------------------------------------------------------------
// sumDims — static helper
// ---------------------------------------------------------------------------

fn sumDims(dims: SEXP, from: c_int, to: c_int) -> f64 {
    if to < from {
        return 0.0;
    }
    let mut s: f64 = 0.0;
    for i in from..=to {
        s += real_elt(dims, i as R_xlen_t);
    }
    s
}

// ---------------------------------------------------------------------------
// allocationRemaining
// ---------------------------------------------------------------------------

fn allocationRemaining(initial: f64, remaining: f64) -> bool {
    if initial == 0.0 {
        false
    } else if initial > 0.0 {
        remaining > 0.0
    } else {
        remaining < 0.0
    }
}

// ---------------------------------------------------------------------------
// calcViewportLayout
// ---------------------------------------------------------------------------

pub fn calcViewportLayout(
    viewport: SEXP,
    parentWidthCM: f64,
    parentHeightCM: f64,
    parentContext: LViewportContext,
    parentgc: pGEcontext,
    dd: pGEDevDesc,
) {
    let layout = viewportLayout(viewport);
    let ncol = layoutNCol(layout);
    let nrow = layoutNRow(layout);
    let mut npcWidths = vec![0.0; ncol as usize];
    let mut npcHeights = vec![0.0; nrow as usize];
    let mut relativeWidths = vec![0; ncol as usize];
    let mut relativeHeights = vec![0; nrow as usize];
    let mut reducedWidthCM = parentWidthCM;
    let mut reducedHeightCM = parentHeightCM;

    findRelWidths(layout, &mut relativeWidths, dd);
    findRelHeights(layout, &mut relativeHeights, dd);

    allocateKnownWidths(
        layout,
        &relativeWidths,
        parentWidthCM,
        parentHeightCM,
        parentContext,
        parentgc,
        dd,
        &mut npcWidths,
        &mut reducedWidthCM,
    );
    allocateKnownHeights(
        layout,
        &relativeHeights,
        parentWidthCM,
        parentHeightCM,
        parentContext,
        parentgc,
        dd,
        &mut npcHeights,
        &mut reducedHeightCM,
    );

    if allocationRemaining(parentWidthCM, reducedWidthCM)
        || allocationRemaining(parentHeightCM, reducedHeightCM)
    {
        allocateRespected(
            layout,
            &relativeWidths,
            &relativeHeights,
            &mut reducedWidthCM,
            &mut reducedHeightCM,
            parentContext,
            parentgc,
            dd,
            &mut npcWidths,
            &mut npcHeights,
        );
    } else {
        setRespectedZero(
            layout,
            &relativeWidths,
            &relativeHeights,
            &mut npcWidths,
            &mut npcHeights,
        );
    }

    if allocationRemaining(parentWidthCM, reducedWidthCM) {
        allocateRemainingWidth(
            layout,
            &relativeWidths,
            reducedWidthCM,
            parentContext,
            parentgc,
            dd,
            &mut npcWidths,
        );
    } else {
        setRemainingWidthZero(layout, &relativeWidths, &mut npcWidths);
    }

    if allocationRemaining(parentHeightCM, reducedHeightCM) {
        allocateRemainingHeight(
            layout,
            &relativeHeights,
            reducedHeightCM,
            parentContext,
            parentgc,
            dd,
            &mut npcHeights,
        );
    } else {
        setRemainingHeightZero(layout, &relativeHeights, &mut npcHeights);
    }

    let currentWidths = alloc_vector(SEXPTYPE::REALSXP, ncol);
    let _widths_guard = protect(currentWidths);
    let currentHeights = alloc_vector(SEXPTYPE::REALSXP, nrow);
    let _heights_guard = protect(currentHeights);
    for i in 0..ncol {
        set_real_elt(currentWidths, i as R_xlen_t, npcWidths[i as usize]);
    }
    for i in 0..nrow {
        set_real_elt(currentHeights, i as R_xlen_t, npcHeights[i as usize]);
    }
    set_vector_elt(viewport, PVP_WIDTHS as R_xlen_t, currentWidths);
    set_vector_elt(viewport, PVP_HEIGHTS as R_xlen_t, currentHeights);
}

// ---------------------------------------------------------------------------
// checkPosRowPosCol
// ---------------------------------------------------------------------------

pub fn checkPosRowPosCol(vp: SEXP, parent: SEXP) -> bool {
    let parent_layout = viewportLayout(parent);
    let ncol = layoutNCol(parent_layout);
    let nrow = layoutNRow(parent_layout);
    if Rf_isNull(viewportLayoutPosRow(vp)) == 0 {
        let lpr = viewportLayoutPosRow(vp);
        if integer_elt(lpr, 0) < 1 || integer_elt(lpr, 1) > nrow {
            return false;
        }
    }
    if Rf_isNull(viewportLayoutPosCol(vp)) == 0 {
        let lpc = viewportLayoutPosCol(vp);
        if integer_elt(lpc, 0) < 1 || integer_elt(lpc, 1) > ncol {
            return false;
        }
    }
    true
}

// ---------------------------------------------------------------------------
// calcViewportLocationFromLayout
// ---------------------------------------------------------------------------

pub fn calcViewportLocationFromLayout(
    layoutPosRow: SEXP,
    layoutPosCol: SEXP,
    parent: SEXP,
    vpl: &mut LViewportLocation,
) {
    let layout = viewportLayout(parent);
    let (minrow, maxrow) = if Rf_isNull(layoutPosRow) != 0 {
        (0, layoutNRow(layout) - 1)
    } else {
        (
            integer_elt(layoutPosRow, 0) - 1,
            integer_elt(layoutPosRow, 1) - 1,
        )
    };
    let (mincol, maxcol) = if Rf_isNull(layoutPosCol) != 0 {
        (0, layoutNCol(layout) - 1)
    } else {
        (
            integer_elt(layoutPosCol, 0) - 1,
            integer_elt(layoutPosCol, 1) - 1,
        )
    };

    let widths = viewportLayoutWidths(parent);
    let heights = viewportLayoutHeights(parent);
    let parent_width_cm = real_elt(viewportWidthCM(parent), 0);
    let parent_height_cm = real_elt(viewportHeightCM(parent), 0);
    let (left, bottom, width, height) = subRegion(
        layout,
        minrow,
        maxrow,
        mincol,
        maxcol,
        widths,
        heights,
        parent_width_cm,
        parent_height_cm,
    );

    let x = unit(left, super::unit::L_CM);
    let _x_guard = protect(x);
    let y = unit(bottom, super::unit::L_CM);
    let _y_guard = protect(y);
    let w = unit(width, super::unit::L_CM);
    let _w_guard = protect(w);
    let h = unit(height, super::unit::L_CM);
    let _h_guard = protect(h);
    vpl.x = x;
    vpl.y = y;
    vpl.width = w;
    vpl.height = h;
    vpl.hjust = 0.0;
    vpl.vjust = 0.0;
}

fn subRegion(
    layout: SEXP,
    minrow: c_int,
    maxrow: c_int,
    mincol: c_int,
    maxcol: c_int,
    widths: SEXP,
    heights: SEXP,
    parentWidthCM: f64,
    parentHeightCM: f64,
) -> (f64, f64, f64, f64) {
    let hjust = layoutHJust(layout);
    let vjust = layoutVJust(layout);
    let total_width = sumDims(widths, 0, layoutNCol(layout) - 1);
    let total_height = sumDims(heights, 0, layoutNRow(layout) - 1);
    let width = sumDims(widths, mincol, maxcol);
    let height = sumDims(heights, minrow, maxrow);
    let left = parentWidthCM * hjust - total_width * hjust + sumDims(widths, 0, mincol - 1);
    let bottom =
        parentHeightCM * vjust + (1.0 - vjust) * total_height - sumDims(heights, 0, maxrow);
    (left, bottom, width, height)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::grid::unit::{L_CM, L_NULL, constructUnits, unit, unitLength, unitUnit};
    use crate::sexp::accessors::{INTEGER, REAL, SET_VECTOR_ELT};
    use crate::sexp::constructors::{Rf_allocVector, Rf_ScalarInteger};
    use crate::sexp::ffi::SEXPTYPE;
    use crate::sexp::globals::R_NilValue;
    use crate::sexp::protect::protect;

    unsafe fn mk_unit_vec(values: &[f64], unit_id: c_int) -> SEXP {
        unsafe {
            let amount = Rf_allocVector(SEXPTYPE::REALSXP, values.len() as c_int);
            let _amount_guard = protect(amount);
            let data = Rf_allocVector(SEXPTYPE::VECSXP, values.len() as c_int);
            let _data_guard = protect(data);
            let unit_type = Rf_allocVector(SEXPTYPE::INTSXP, values.len() as c_int);
            let _unit_type_guard = protect(unit_type);
            for (i, value) in values.iter().enumerate() {
                *REAL(amount).add(i) = *value;
                SET_VECTOR_ELT(data, i as R_xlen_t, R_NilValue());
                *INTEGER(unit_type).add(i) = unit_id;
            }
            let result = constructUnits(amount, data, unit_type);
            result
        }
    }

    unsafe fn mk_layout(
        widths: SEXP,
        heights: SEXP,
        respect_mat: &[c_int],
        nrow: c_int,
        ncol: c_int,
    ) -> SEXP {
        unsafe {
            let layout = Rf_allocVector(SEXPTYPE::VECSXP, 9);
            let _layout_guard = protect(layout);
            SET_VECTOR_ELT(layout, LAYOUT_NROW as R_xlen_t, Rf_ScalarInteger(nrow));
            SET_VECTOR_ELT(layout, LAYOUT_NCOL as R_xlen_t, Rf_ScalarInteger(ncol));
            SET_VECTOR_ELT(layout, LAYOUT_WIDTHS as R_xlen_t, widths);
            SET_VECTOR_ELT(layout, LAYOUT_HEIGHTS as R_xlen_t, heights);
            SET_VECTOR_ELT(layout, LAYOUT_RESPECT as R_xlen_t, Rf_ScalarInteger(0));
            SET_VECTOR_ELT(layout, LAYOUT_VRESPECT as R_xlen_t, Rf_ScalarInteger(0));
            let respect = Rf_allocVector(SEXPTYPE::INTSXP, respect_mat.len() as c_int);
            let _respect_guard = protect(respect);
            for (i, value) in respect_mat.iter().enumerate() {
                *INTEGER(respect).add(i) = *value;
            }
            SET_VECTOR_ELT(layout, LAYOUT_MRESPECT as R_xlen_t, respect);
            let just = Rf_allocVector(SEXPTYPE::REALSXP, 2);
            let _just_guard = protect(just);
            *REAL(just) = 0.0;
            *REAL(just).add(1) = 0.0;
            SET_VECTOR_ELT(layout, LAYOUT_JUST as R_xlen_t, R_NilValue());
            SET_VECTOR_ELT(layout, LAYOUT_VJUST as R_xlen_t, just);
            layout
        }
    }

    unsafe fn mk_viewport(layout: SEXP) -> SEXP {
        unsafe {
            let vp = Rf_allocVector(SEXPTYPE::VECSXP, PVP_MASK as c_int + 1);
            let _vp_guard = protect(vp);
            SET_VECTOR_ELT(vp, VP_LAYOUT as R_xlen_t, layout);
            SET_VECTOR_ELT(vp, PVP_WIDTHCM as R_xlen_t, Rf_ScalarReal(10.0));
            SET_VECTOR_ELT(vp, PVP_HEIGHTCM as R_xlen_t, Rf_ScalarReal(6.0));
            SET_VECTOR_ELT(vp, VP_VALIDLPOSROW as R_xlen_t, R_NilValue());
            SET_VECTOR_ELT(vp, VP_VALIDLPOSCOL as R_xlen_t, R_NilValue());
            vp
        }
    }

    #[test]
    fn relative_unit_tracks_null_units() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let null_unit = unit(1.0, L_NULL);
            let cm_unit = unit(1.0, L_CM);

            assert!(relativeUnit(null_unit, 0, std::ptr::null()));
            assert!(!relativeUnit(cm_unit, 0, std::ptr::null()));
        }
    }

    #[test]
    fn calc_viewport_layout_allocates_known_and_null_units() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let widths = mk_unit_vec(&[1.0, 1.0, 1.0], L_NULL);
            let heights = mk_unit_vec(&[1.0, 1.0], L_CM);
            let layout = mk_layout(widths, heights, &[0, 0, 0, 0, 0, 0], 2, 3);
            let vp = mk_viewport(layout);

            calcViewportLayout(
                vp,
                10.0,
                6.0,
                LViewportContext::default(),
                std::ptr::null(),
                std::ptr::null(),
            );

            let layout_widths = VECTOR_ELT(vp, PVP_WIDTHS as R_xlen_t);
            let layout_heights = VECTOR_ELT(vp, PVP_HEIGHTS as R_xlen_t);

            assert!((*REAL(layout_widths) - 3.3333333333333335).abs() < 1e-12);
            assert!((*REAL(layout_widths).add(1) - 3.3333333333333335).abs() < 1e-12);
            assert!((*REAL(layout_widths).add(2) - 3.3333333333333335).abs() < 1e-12);
            assert!((*REAL(layout_heights) - 1.0).abs() < 1e-12);
            assert!((*REAL(layout_heights).add(1) - 1.0).abs() < 1e-12);
        }
    }

    #[test]
    fn calc_viewport_location_from_layout_uses_layout_slices() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let widths = mk_unit_vec(&[1.0, 1.0, 1.0], L_NULL);
            let heights = mk_unit_vec(&[1.0, 1.0], L_CM);
            let layout = mk_layout(widths, heights, &[0, 0, 0, 0, 0, 0], 2, 3);
            let vp = mk_viewport(layout);

            calcViewportLayout(
                vp,
                10.0,
                6.0,
                LViewportContext::default(),
                std::ptr::null(),
                std::ptr::null(),
            );

            let mut vpl = LViewportLocation {
                x: std::ptr::null_mut(),
                y: std::ptr::null_mut(),
                width: std::ptr::null_mut(),
                height: std::ptr::null_mut(),
                hjust: 0.0,
                vjust: 0.0,
            };
            let row = Rf_allocVector(SEXPTYPE::INTSXP, 2);
            let _row_guard = protect(row);
            *INTEGER(row) = 1;
            *INTEGER(row).add(1) = 2;
            let col = Rf_allocVector(SEXPTYPE::INTSXP, 2);
            let _col_guard = protect(col);
            *INTEGER(col) = 2;
            *INTEGER(col).add(1) = 3;

            calcViewportLocationFromLayout(row, col, vp, &mut vpl);

            assert_eq!(unitLength(vpl.x), 1);
            assert_eq!(unitUnit(vpl.x, 0), L_CM);
            assert!((crate::library::grid::unit::unitValue(vpl.x, 0) - 10.0 / 3.0).abs() < 1e-12);
            assert_eq!(unitUnit(vpl.width, 0), L_CM);
            assert!(
                (crate::library::grid::unit::unitValue(vpl.width, 0) - 20.0 / 3.0).abs() < 1e-12
            );
            assert_eq!(unitUnit(vpl.height, 0), L_CM);
            assert!((crate::library::grid::unit::unitValue(vpl.height, 0) - 2.0).abs() < 1e-12);
        }
    }
}
