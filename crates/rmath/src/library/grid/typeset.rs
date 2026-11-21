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
    char_cstring, duplicate, integer_elt, mk_string, protect, real_elt, set_vector_elt, string_elt,
    vector_elt,
};

/// GE_INCHES constant for unit conversion
const GE_INCHES: c_int = crate::mainutils::engine::GE_INCHES;

struct GridModeGuard {
    dd: pGEDevDesc,
}

impl GridModeGuard {
    fn enter(dd: pGEDevDesc) -> Self {
        ge::GEMode(1, dd);
        Self { dd }
    }
}

impl Drop for GridModeGuard {
    fn drop(&mut self) {
        ge::GEMode(0, self.dd);
    }
}

// ---------------------------------------------------------------------------
// renderGlyphs — internal function to render glyph runs
// ---------------------------------------------------------------------------

fn renderGlyphs(runs: SEXP, glyphInfo: SEXP, x: SEXP, y: SEXP, draw: bool) {
    let nruns = LENGTH(runs);
    let dd = getDevice();
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
    let _gc = R_GE_gcontext::zeroed();
    gcontextFromgpar(currentgp, 0, Some(&_gc), dd);

    let currentgp = duplicate(currentgp);
    let _currentgp_guard = protect(currentgp);
    let fill = mk_string("black");
    set_vector_elt(currentgp, GP_FILL as R_xlen_t, fill);

    let (vpWidthCM, vpHeightCM, transform, rotationAngle) = getViewportTransform(currentvp, dd);

    let mut vpc = LViewportContext {
        xscalemin: 0.0,
        xscalemax: 1.0,
        yscalemin: 0.0,
        yscalemax: 1.0,
    };
    fillViewportContextFromViewport(currentvp, &mut vpc);

    let _mode_guard = if draw {
        Some(GridModeGuard::enter(dd))
    } else {
        None
    };

    let glyphs = ge::R_GE_glyphInfoGlyphs(glyphInfo);
    let fonts = ge::R_GE_glyphInfoFonts(glyphInfo);
    let glyph_id = ge::R_GE_glyphID(glyphs);
    let n = LENGTH(glyph_id);
    let ids: Vec<c_int> = (0..n)
        .map(|i| integer_elt(glyph_id, i as R_xlen_t))
        .collect();

    // Allocate gx and gy arrays
    let mut gx: Vec<f64> = vec![0.0; n as usize];
    let mut gy: Vec<f64> = vec![0.0; n as usize];

    for i in 0..n {
        let (xx, yy) = transformLocn(
            x,
            y,
            i,
            vpc,
            Some(&_gc),
            vpWidthCM,
            vpHeightCM,
            dd,
            &transform,
        );
        gx[i as usize] = ge::toDeviceX(xx, GE_INCHES, dd);
        gy[i as usize] = ge::toDeviceY(yy, GE_INCHES, dd);
    }

    let glyph_font = ge::R_GE_glyphFont(glyphs);
    let glyph_size = ge::R_GE_glyphSize(glyphs);
    let glyph_colour = ge::R_GE_glyphColour(glyphs);

    let mut offset: c_int = 0;
    for i in 0..nruns {
        let run_length = integer_elt(runs, i as R_xlen_t);
        let font = vector_elt(
            fonts,
            (integer_elt(glyph_font, offset as R_xlen_t) - 1) as R_xlen_t,
        );
        let size = real_elt(glyph_size, offset as R_xlen_t);
        let glyph_rotation = if ge::R_GE_hasGlyphRotation(glyphs) != 0 {
            real_elt(ge::R_GE_glyphRotation(glyphs), offset as R_xlen_t)
        } else {
            0.0
        };
        let final_rotation = rotationAngle + glyph_rotation;

        // The C original copies at most 50 bytes of the colour name into a
        // NUL-terminated 51-byte buffer.
        let colour_name = char_cstring(string_elt(glyph_colour, offset as R_xlen_t));
        let mut colstr: [std::os::raw::c_char; 51] = [0; 51];
        for (dst, src) in colstr
            .iter_mut()
            .zip(colour_name.as_bytes().iter().take(50))
        {
            *dst = *src as std::os::raw::c_char;
        }
        // SAFETY: `colstr` is a NUL-terminated buffer sized for the parser.
        let colour = unsafe { colors::R_GE_str2col(colstr.as_ptr()) as c_int };

        let start = (offset.max(0) as usize).min(ids.len());
        let end = (start + run_length.max(0) as usize).min(ids.len());
        ge::GEGlyph(
            &ids[start..end],
            &gx[start..end],
            &gy[start..end],
            font,
            size,
            colour,
            final_rotation,
            dd,
        );
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
