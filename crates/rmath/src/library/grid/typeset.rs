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

//! Port of R's src/library/grid/src/typeset.c
//!
//! Glyph rendering for text typesetting in grid.

use std::ffi::c_void;
use std::os::raw::{c_char, c_int};

use crate::mainutils::{colors, engine as ge};
use crate::sexp::accessors::{INTEGER, LENGTH};
use crate::sexp::ffi::{R_xlen_t, SEXP};
use crate::sexp::globals::R_NilValue;

use super::gpar::gcontextFromgpar;
use super::grid::{getDevice, getViewportTransform};
use super::state::gridStateElement;
use super::types::*;
use super::unit::transformLocn;
use super::viewport::fillViewportContextFromViewport;
use super::{
    char_ptr, duplicate, integer_elt, mk_string, protect, real_elt, set_vector_elt, string_elt,
    vector_elt,
};

/// GE_INCHES constant for unit conversion
const GE_INCHES: c_int = crate::mainutils::engine::GE_INCHES;

struct GridModeGuard {
    dd: pGEDevDesc,
}

impl GridModeGuard {
    // SAFETY: `GEMode` is an engine mode-switch bridge; callers pass the live
    // device returned by `getDevice`.
    unsafe fn enter(dd: pGEDevDesc) -> Self {
        // SAFETY: forwarded raw pointers satisfy `GEMode`'s contract per this function's own `# Safety` requirements.
        unsafe { ge::GEMode(1, dd) };
        Self { dd }
    }
}

impl Drop for GridModeGuard {
    fn drop(&mut self) {
        // SAFETY: `dd` was a live device when the guard was entered.
        // SAFETY: `dd` is the live current graphics device handle obtained from the engine for this call; other pointer arguments are locals or literals.
        unsafe { ge::GEMode(0, self.dd) };
    }
}

// ---------------------------------------------------------------------------
// renderGlyphs — internal function to render glyph runs
// ---------------------------------------------------------------------------

fn renderGlyphs(runs: SEXP, glyphInfo: SEXP, x: SEXP, y: SEXP, draw: bool) {
    let nruns = LENGTH(runs);
    // SAFETY: device registry bridge; a null result means no open device.
    let dd = unsafe { getDevice() };
    if dd.is_null() {
        return;
    }
    let currentvp = gridStateElement(dd, GSS_VP);
    let currentgp = gridStateElement(dd, GSS_GPAR);
    if currentvp.is_null()
        || currentvp == R_NilValue()
        || currentgp.is_null()
        || currentgp == R_NilValue()
    {
        return;
    }

    // R_GE_gcontext gc — opaque layout.
    // The shared engine module still owns the fallback glyph-info path for now.
    // accessors; keep this call path delegated there until the GE glyphInfo
    // SEXP layout is available.
    let mut _gc: [u8; 256] = [0; 256];
    gcontextFromgpar(currentgp, 0, _gc.as_mut_ptr() as *const c_void, dd);

    let currentgp = duplicate(currentgp);
    let _currentgp_guard = protect(currentgp);
    let fill = mk_string("black");
    set_vector_elt(currentgp, GP_FILL as R_xlen_t, fill);

    // SAFETY: `currentvp`/`dd` keep their raw device-state contracts; the
    // viewport transform bridge writes through `dd`.
    // SAFETY: `dd` is the live current graphics device handle obtained from the engine for this call; other pointer arguments are locals or literals.
    let (vpWidthCM, vpHeightCM, transform, rotationAngle) =
        unsafe { getViewportTransform(currentvp, dd) };

    let mut vpc = LViewportContext {
        xscalemin: 0.0,
        xscalemax: 1.0,
        yscalemin: 0.0,
        yscalemax: 1.0,
    };
    fillViewportContextFromViewport(currentvp, &mut vpc);

    let _mode_guard = if draw {
        // SAFETY: `dd` is the live device returned by `getDevice` above.
        Some(unsafe { GridModeGuard::enter(dd) })
    } else {
        None
    };

    // SAFETY: glyph-info accessors are raw engine SEXP bridges; `glyphInfo`
    // is the checked object passed down from `L_glyph`.
    let glyphs = unsafe { ge::R_GE_glyphInfoGlyphs(glyphInfo) };
    let fonts = unsafe { ge::R_GE_glyphInfoFonts(glyphInfo) };
    let glyph_id = unsafe { ge::R_GE_glyphID(glyphs) };
    // SAFETY: `GEGlyph` below needs the raw glyph-id buffer; the pointer is
    // lent only for that engine call.
    let id = unsafe { INTEGER(glyph_id) };
    let n = LENGTH(glyph_id);

    // Allocate gx and gy arrays
    let mut gx: Vec<f64> = vec![0.0; n as usize];
    let mut gy: Vec<f64> = vec![0.0; n as usize];

    for i in 0..n {
        // SAFETY: pointer arguments are live locals, NUL-terminated literals or owned CString/Vec buffers of this caller, valid for the call.
        let (xx, yy) = unsafe {
            transformLocn(
                x,
                y,
                i,
                vpc,
                _gc.as_ptr() as *const c_void,
                vpWidthCM,
                vpHeightCM,
                dd,
                &transform,
            )
        };
        // SAFETY: `dd` is the live device; `toDevice*` are engine coordinate
        // bridges over the device state.
        // SAFETY: `dd` is the live current graphics device handle obtained from the engine for this call; other pointer arguments are locals or literals.
        gx[i as usize] = unsafe { ge::toDeviceX(xx, GE_INCHES, dd) };
        // SAFETY: `dd` is the live current graphics device handle obtained from the engine for this call; other pointer arguments are locals or literals.
        gy[i as usize] = unsafe { ge::toDeviceY(yy, GE_INCHES, dd) };
    }

    // SAFETY: remaining per-glyph vectors are raw engine SEXP bridges over the
    // live `glyphs` list returned above.
    let glyph_font = unsafe { ge::R_GE_glyphFont(glyphs) };
    let glyph_size = unsafe { ge::R_GE_glyphSize(glyphs) };
    let glyph_colour = unsafe { ge::R_GE_glyphColour(glyphs) };

    let mut offset: c_int = 0;
    for i in 0..nruns {
        let run_length = integer_elt(runs, i as R_xlen_t);
        let font = vector_elt(
            fonts,
            (integer_elt(glyph_font, offset as R_xlen_t) - 1) as R_xlen_t,
        );
        let size = real_elt(glyph_size, offset as R_xlen_t);
        let glyph_rotation = if unsafe { ge::R_GE_hasGlyphRotation(glyphs) } != 0 {
            // SAFETY: presence was just confirmed by the flag above.
            real_elt(
                unsafe { ge::R_GE_glyphRotation(glyphs) },
                offset as R_xlen_t,
            )
        } else {
            0.0
        };
        let final_rotation = rotationAngle + glyph_rotation;

        let mut colstr: [std::os::raw::c_char; 51] = [0; 51];
        let colour_src = char_ptr(string_elt(glyph_colour, offset as R_xlen_t));
        // SAFETY: `colour_src` is a NUL-terminated CHAR payload; the 51-byte
        // destination buffer is fully initialized and truncated like the C
        // original.
        unsafe {
            std::ptr::copy_nonoverlapping(colour_src, colstr.as_mut_ptr(), 50);
        }
        // SAFETY: `colstr` is a NUL-terminated buffer sized for the parser.
        let colour = unsafe { colors::R_GE_str2col(colstr.as_ptr()) as c_int };

        // SAFETY: `id`, `gx`, `gy`, `font` and `dd` are the raw engine-side
        // pointers this bridge consumes; all were produced above.
        unsafe {
            ge::GEGlyph(
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
        }
        offset += run_length;
    }

    drop(_mode_guard);
}

// ---------------------------------------------------------------------------
// L_glyph — public entry point for glyph rendering
// ---------------------------------------------------------------------------

pub fn L_glyph(runs: SEXP, glyphInfo: SEXP, x: SEXP, y: SEXP) -> SEXP {
    renderGlyphs(runs, glyphInfo, x, y, true);
    R_NilValue()
}
