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
use crate::sexp::accessors::{CHAR, INTEGER, LENGTH, REAL, SET_VECTOR_ELT, STRING_ELT, VECTOR_ELT};
use crate::sexp::constructors::Rf_mkString;
use crate::sexp::ffi::{R_xlen_t, SEXP};
use crate::sexp::globals::R_NilValue;
use crate::sexp::protect::protect;

use super::gpar::gcontextFromgpar;
use super::grid::{getDevice, getViewportTransform};
use super::state::gridStateElement;
use super::types::*;
use super::unit::transformLocn;
use super::viewport::fillViewportContextFromViewport;

/// GE_INCHES constant for unit conversion
const GE_INCHES: c_int = crate::mainutils::engine::GE_INCHES;

struct GridModeGuard {
    dd: pGEDevDesc,
}

impl GridModeGuard {
    unsafe fn enter(dd: pGEDevDesc) -> Self {
        unsafe {
            ge::GEMode(1, dd);
        }
        Self { dd }
    }
}

impl Drop for GridModeGuard {
    fn drop(&mut self) {
        unsafe {
            ge::GEMode(0, self.dd);
        }
    }
}

// ---------------------------------------------------------------------------
// renderGlyphs — internal function to render glyph runs
// ---------------------------------------------------------------------------

unsafe fn renderGlyphs(runs: SEXP, glyphInfo: SEXP, x: SEXP, y: SEXP, draw: bool) {
    let nruns = unsafe { LENGTH(runs) };
    let dd = unsafe { getDevice() };
    if dd.is_null() {
        return;
    }
    let currentvp = unsafe { gridStateElement(dd, GSS_VP) };
    let currentgp = unsafe { gridStateElement(dd, GSS_GPAR) };
    if currentvp.is_null()
        || currentvp == unsafe { R_NilValue() }
        || currentgp.is_null()
        || currentgp == unsafe { R_NilValue() }
    {
        return;
    }

    // R_GE_gcontext gc — opaque layout.
    // The shared engine module still owns the fallback glyph-info path for now.
    // accessors; keep this call path delegated there until the GE glyphInfo
    // SEXP layout is available.
    let mut _gc: [u8; 256] = [0; 256];
    unsafe { gcontextFromgpar(currentgp, 0, _gc.as_mut_ptr() as *const c_void, dd) };

    let currentgp = unsafe { crate::main::duplicate::Rf_duplicate(currentgp) };
    let _currentgp_guard = protect(currentgp);
    let fill = unsafe { Rf_mkString(b"black\0".as_ptr() as *const std::os::raw::c_char) };
    unsafe {
        SET_VECTOR_ELT(currentgp, GP_FILL as R_xlen_t, fill);
    }

    let mut vpWidthCM: f64 = 0.0;
    let mut vpHeightCM: f64 = 0.0;
    let mut transform: LTransform = [[0.0; 3]; 3];
    let mut rotationAngle: f64 = 0.0;
    unsafe {
        getViewportTransform(
            currentvp,
            dd,
            &mut vpWidthCM,
            &mut vpHeightCM,
            &mut transform,
            &mut rotationAngle,
        );
    }

    let mut vpc = LViewportContext {
        xscalemin: 0.0,
        xscalemax: 1.0,
        yscalemin: 0.0,
        yscalemax: 1.0,
    };
    unsafe {
        fillViewportContextFromViewport(currentvp, &mut vpc);
    }

    let _mode_guard = if draw {
        Some(unsafe { GridModeGuard::enter(dd) })
    } else {
        None
    };

    let glyphs = unsafe { ge::R_GE_glyphInfoGlyphs(glyphInfo) };
    let fonts = unsafe { ge::R_GE_glyphInfoFonts(glyphInfo) };
    let id = unsafe { INTEGER(ge::R_GE_glyphID(glyphs)) };
    let n = unsafe { LENGTH(ge::R_GE_glyphID(glyphs)) };

    // Allocate gx and gy arrays
    let mut gx: Vec<f64> = vec![0.0; n as usize];
    let mut gy: Vec<f64> = vec![0.0; n as usize];

    for i in 0..n {
        let mut xx: f64 = 0.0;
        let mut yy: f64 = 0.0;
        unsafe {
            transformLocn(
                x,
                y,
                i,
                vpc,
                _gc.as_ptr() as *const c_void,
                vpWidthCM,
                vpHeightCM,
                dd,
                &mut transform,
                &mut xx,
                &mut yy,
            );
        }
        gx[i as usize] = unsafe { ge::toDeviceX(xx, GE_INCHES, dd) };
        gy[i as usize] = unsafe { ge::toDeviceY(yy, GE_INCHES, dd) };
    }

    let mut offset: c_int = 0;
    for i in 0..nruns {
        let run_length = unsafe { *INTEGER(runs).add(i as usize) };
        let font = unsafe {
            VECTOR_ELT(
                fonts,
                (*INTEGER(ge::R_GE_glyphFont(glyphs)).add(offset as usize) - 1) as R_xlen_t,
            )
        };
        let size = unsafe { *REAL(ge::R_GE_glyphSize(glyphs)).add(offset as usize) };
        let glyph_rotation = if unsafe { ge::R_GE_hasGlyphRotation(glyphs) } != 0 {
            unsafe { *REAL(ge::R_GE_glyphRotation(glyphs)).add(offset as usize) }
        } else {
            0.0
        };
        let final_rotation = rotationAngle + glyph_rotation;

        let mut colstr: [std::os::raw::c_char; 51] = [0; 51];
        unsafe {
            std::ptr::copy_nonoverlapping(
                CHAR(STRING_ELT(ge::R_GE_glyphColour(glyphs), offset as R_xlen_t)),
                colstr.as_mut_ptr(),
                50,
            );
        }
        let colour = unsafe { colors::R_GE_str2col(colstr.as_ptr()) as c_int };

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

pub unsafe fn L_glyph(runs: SEXP, glyphInfo: SEXP, x: SEXP, y: SEXP) -> SEXP {
    unsafe { renderGlyphs(runs, glyphInfo, x, y, true) };
    unsafe { R_NilValue() }
}
