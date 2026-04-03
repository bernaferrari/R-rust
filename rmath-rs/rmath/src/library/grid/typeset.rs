#![allow(unsafe_op_in_unsafe_fn)]
/*
 *  R : A Computer Language for Statistical Data Analysis
 *  Copyright (C) 2001-2025 The R Core Team
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

//! Port of R's src/library/grid/src/typeset.c
//!
//! Glyph rendering for text typesetting in grid.

use std::os::raw::c_int;

use crate::sexp::accessors::{CHAR, INTEGER, LENGTH, REAL, SET_VECTOR_ELT, STRING_ELT, VECTOR_ELT};
use crate::sexp::constructors::Rf_allocVector;
use crate::sexp::constructors::Rf_mkString;
use crate::sexp::ffi::{R_xlen_t, SEXP, SEXPTYPE};
use crate::sexp::globals::R_NilValue;
use crate::sexp::protect::{Rf_protect, Rf_unprotect};

use super::types::*;

// ---------------------------------------------------------------------------
// External stubs for GE functions not yet ported
// ---------------------------------------------------------------------------

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

/// GEMode — set graphics engine mode
unsafe fn GEMode(_mode: c_int, _dd: *const u8) {
    // STUB: requires GraphicsEngine
}

/// gcontextFromgpar — create graphics context from gpar
unsafe fn gcontextFromgpar(_gp: SEXP, _i: c_int, _gc: *mut u8, _dd: *const u8) {
    // STUB: requires gpar.c
}

/// Rf_duplicate — deep copy an R object
unsafe fn Rf_duplicate(x: SEXP) -> SEXP {
    crate::main::duplicate::Rf_duplicate(x)
}

/// getViewportTransform — get viewport transform info
unsafe fn getViewportTransform(
    _currentvp: SEXP,
    _dd: *const u8,
    _vpWidthCM: *mut f64,
    _vpHeightCM: *mut f64,
    _transform: *mut LTransform,
    _rotationAngle: *mut f64,
) {
    // STUB: requires grid.c
}

/// fillViewportContextFromViewport — fill viewport context from viewport
unsafe fn fillViewportContextFromViewport(_vp: SEXP, _vpc: *mut LViewportContext) {
    // STUB: requires viewport.rs (will be available when types.rs is done)
}

/// transformLocn — transform a location
unsafe fn transformLocn(
    _x: SEXP,
    _y: SEXP,
    _index: c_int,
    _vpc: LViewportContext,
    _gc: *const u8,
    _vpWidthCM: f64,
    _vpHeightCM: f64,
    _dd: *const u8,
    _transform: LTransform,
    _xx: *mut f64,
    _yy: *mut f64,
) {
    // STUB: requires unit.c
}

/// toDeviceX — convert inches to device x coordinate
unsafe fn toDeviceX(_x: f64, _unit: c_int, _dd: *const u8) -> f64 {
    // STUB: requires GraphicsEngine
    0.0
}

/// toDeviceY — convert inches to device y coordinate
unsafe fn toDeviceY(_y: f64, _unit: c_int, _dd: *const u8) -> f64 {
    // STUB: requires GraphicsEngine
    0.0
}

/// R_GE_glyphInfoGlyphs — get glyphs from glyph info
unsafe fn R_GE_glyphInfoGlyphs(_glyphInfo: SEXP) -> SEXP {
    R_NilValue()
}

/// R_GE_glyphInfoFonts — get fonts from glyph info
unsafe fn R_GE_glyphInfoFonts(_glyphInfo: SEXP) -> SEXP {
    R_NilValue()
}

/// R_GE_glyphID — get glyph IDs
unsafe fn R_GE_glyphID(_glyphs: SEXP) -> SEXP {
    R_NilValue()
}

/// R_GE_glyphFont — get glyph font indices
unsafe fn R_GE_glyphFont(_glyphs: SEXP) -> SEXP {
    R_NilValue()
}

/// R_GE_glyphSize — get glyph sizes
unsafe fn R_GE_glyphSize(_glyphs: SEXP) -> SEXP {
    R_NilValue()
}

/// R_GE_hasGlyphRotation — check if glyph info has rotation
unsafe fn R_GE_hasGlyphRotation(_glyphs: SEXP) -> bool {
    false
}

/// R_GE_glyphRotation — get glyph rotation angles
unsafe fn R_GE_glyphRotation(_glyphs: SEXP) -> SEXP {
    R_NilValue()
}

/// R_GE_glyphColour — get glyph colours
unsafe fn R_GE_glyphColour(_glyphs: SEXP) -> SEXP {
    R_NilValue()
}

/// R_GE_str2col — convert colour string to integer
unsafe fn R_GE_str2col(_colstr: *const std::os::raw::c_char) -> c_int {
    0
}

/// GEGlyph — render glyphs on device
unsafe fn GEGlyph(
    _n: c_int,
    _id: *const c_int,
    _x: *const f64,
    _y: *const f64,
    _font: SEXP,
    _size: f64,
    _colour: c_int,
    _rotation: f64,
    _dd: *const u8,
) {
    // STUB: requires GraphicsEngine
}

/// GE_INCHES constant for unit conversion
const GE_INCHES: c_int = 8;

// ---------------------------------------------------------------------------
// renderGlyphs — internal function to render glyph runs
// ---------------------------------------------------------------------------

unsafe fn renderGlyphs(runs: SEXP, glyphInfo: SEXP, x: SEXP, y: SEXP, draw: bool) {
    let nruns = LENGTH(runs);
    let dd = getDevice();
    let currentvp = gridStateElement(dd, GSS_VP);
    let currentgp = gridStateElement(dd, GSS_GPAR);

    // R_GE_gcontext gc — placeholder
    let mut _gc: [u8; 256] = [0; 256];
    gcontextFromgpar(currentgp, 0, _gc.as_mut_ptr(), dd);

    let currentgp = Rf_protect(Rf_duplicate(currentgp));
    // Set gp$fill to "black" to avoid pattern resolution
    SET_VECTOR_ELT(
        currentgp,
        GP_FILL as R_xlen_t,
        Rf_mkString(b"black\0".as_ptr() as *const std::os::raw::c_char),
    );

    let mut vpWidthCM: f64 = 0.0;
    let mut vpHeightCM: f64 = 0.0;
    let mut transform: LTransform = [[0.0; 3]; 3];
    let mut rotationAngle: f64 = 0.0;
    getViewportTransform(
        currentvp,
        dd,
        &mut vpWidthCM,
        &mut vpHeightCM,
        &mut transform,
        &mut rotationAngle,
    );

    let mut vpc = LViewportContext {
        xscalemin: 0.0,
        xscalemax: 1.0,
        yscalemin: 0.0,
        yscalemax: 1.0,
    };
    fillViewportContextFromViewport(currentvp, &mut vpc);

    if draw {
        GEMode(1, dd);
    }

    let glyphs = R_GE_glyphInfoGlyphs(glyphInfo);
    let fonts = R_GE_glyphInfoFonts(glyphInfo);
    let id = INTEGER(R_GE_glyphID(glyphs));
    let n = LENGTH(R_GE_glyphID(glyphs));

    // Allocate gx and gy arrays
    let mut gx: Vec<f64> = vec![0.0; n as usize];
    let mut gy: Vec<f64> = vec![0.0; n as usize];

    for i in 0..n {
        let mut xx: f64 = 0.0;
        let mut yy: f64 = 0.0;
        transformLocn(
            x,
            y,
            i,
            vpc,
            _gc.as_ptr(),
            vpWidthCM,
            vpHeightCM,
            dd,
            transform,
            &mut xx,
            &mut yy,
        );
        gx[i as usize] = toDeviceX(xx, GE_INCHES, dd);
        gy[i as usize] = toDeviceY(yy, GE_INCHES, dd);
    }

    let mut offset: c_int = 0;
    for i in 0..nruns {
        let run_length = *INTEGER(runs).add(i as usize);
        let font = VECTOR_ELT(
            fonts,
            (*INTEGER(R_GE_glyphFont(glyphs)).add(offset as usize) - 1) as R_xlen_t,
        );
        let size = *REAL(R_GE_glyphSize(glyphs)).add(offset as usize);
        let glyph_rotation = if R_GE_hasGlyphRotation(glyphs) {
            *REAL(R_GE_glyphRotation(glyphs)).add(offset as usize)
        } else {
            0.0
        };
        let final_rotation = rotationAngle + glyph_rotation;

        let mut colstr: [std::os::raw::c_char; 51] = [0; 51];
        std::ptr::copy_nonoverlapping(
            CHAR(STRING_ELT(R_GE_glyphColour(glyphs), offset as R_xlen_t)),
            colstr.as_mut_ptr(),
            50,
        );
        let colour = R_GE_str2col(colstr.as_ptr());

        GEGlyph(
            run_length,
            id.add(offset as usize),
            gx.as_ptr().add(offset as usize),
            gy.as_ptr().add(offset as usize),
            font,
            size,
            colour,
            final_rotation,
            dd,
        );
        offset += run_length;
    }

    if draw {
        GEMode(0, dd);
    }
    Rf_unprotect(1);
}

// ---------------------------------------------------------------------------
// L_glyph — public entry point for glyph rendering
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe extern "C" fn L_glyph(runs: SEXP, glyphInfo: SEXP, x: SEXP, y: SEXP) -> SEXP {
    renderGlyphs(runs, glyphInfo, x, y, true);
    R_NilValue()
}
