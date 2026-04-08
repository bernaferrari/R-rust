#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

/*
 *  R : A Computer Language for Statistical Data Analysis
 *  Copyright (C) 2019	     The R Foundation
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
 *
 *  Ported from r-source/src/main/patterns.c
 *
 *  This should be regarded as part of the graphics engine.
 *
 *  C API for graphics devices to interrogate gradient SEXPs.
 *  MUST match R structures in ../library/grDevices/R/patterns.R
 */

use std::os::raw::{c_char, c_double, c_int, c_uint, c_void};

use crate::attrib_core::{R_ClassSymbol, getAttrib};
use crate::main::colors::RGBpar;
use crate::main::errors::Rf_error;
use crate::sexp::accessors::*;
use crate::sexp::ffi::*;
use crate::sexp::globals::R_NilValue;

/* Pattern type constants, matching R_ext/GraphicsEngine.h */
const R_GE_linearGradientPattern: c_int = 1;
const R_GE_radialGradientPattern: c_int = 2;
const R_GE_tilingPattern: c_int = 3;

/* Linear gradient component indices (R_xlen_t = i64 for VECTOR_ELT) */
const linear_gradient_x1: i64 = 1;
const linear_gradient_y1: i64 = 2;
const linear_gradient_x2: i64 = 3;
const linear_gradient_y2: i64 = 4;
const linear_gradient_stops: i64 = 5;
const linear_gradient_colours: i64 = 6;
const linear_gradient_extend: i64 = 7;

/* Radial gradient component indices */
const radial_gradient_cx1: i64 = 1;
const radial_gradient_cy1: i64 = 2;
const radial_gradient_r1: i64 = 3;
const radial_gradient_cx2: i64 = 4;
const radial_gradient_cy2: i64 = 5;
const radial_gradient_r2: i64 = 6;
const radial_gradient_stops: i64 = 7;
const radial_gradient_colours: i64 = 8;
const radial_gradient_extend: i64 = 9;

/* Tiling pattern component indices */
const tiling_pattern_function: i64 = 1;
const tiling_pattern_x: i64 = 2;
const tiling_pattern_y: i64 = 3;
const tiling_pattern_width: i64 = 4;
const tiling_pattern_height: i64 = 5;
const tiling_pattern_extend: i64 = 6;

/// Helper: check if SEXP inherits from a given class.
unsafe fn Rf_inherits(x: SEXP, what: *const c_char) -> c_int {
    unsafe {
        let class_name = std::ffi::CStr::from_ptr(what);
        let class_str = class_name.to_str().unwrap_or("");
        let s_class = getAttrib(x, R_ClassSymbol());
        if s_class == R_NilValue() {
            return 0;
        }
        let n = LENGTH(s_class);
        let mut i: c_int = 0;
        while i < n {
            let elt = STRING_ELT(s_class, i as i64);
            if elt.is_null() {
                i += 1;
                continue;
            }
            let bytes = CHAR(elt);
            if bytes.is_null() {
                i += 1;
                continue;
            }
            let s = std::ffi::CStr::from_ptr(bytes).to_str().unwrap_or("");
            if s == class_str {
                return 1;
            }
            i += 1;
        }
        0
    }
}

/// Rboolean type (0 = FALSE, 1 = TRUE).
type Rboolean = c_int;

/// R_GE_isPattern -- check if SEXP is a Pattern object.
pub unsafe fn R_GE_isPattern(x: SEXP) -> Rboolean {
    unsafe {
        let pat = std::ffi::CString::new("Pattern").expect("CString::new failed: contains null byte");
        Rf_inherits(x, pat.as_ptr())
    }
}

/// R_GE_patternType -- get the pattern type (component 0).
pub unsafe fn R_GE_patternType(pattern: SEXP) -> c_int {
    unsafe { *INTEGER(VECTOR_ELT(pattern, 0)) }
}

/* ========================================================================
 * Linear gradients
 * ======================================================================== */

macro_rules! checkLinearGradient {
    ($pattern:expr) => {
        if R_GE_patternType($pattern) != R_GE_linearGradientPattern {
            Rf_error(b"pattern is not a linear gradient\0".as_ptr() as *const c_char);
        }
    };
}

pub unsafe fn R_GE_linearGradientX1(pattern: SEXP) -> c_double {
    unsafe {
        checkLinearGradient!(pattern);
        *REAL(VECTOR_ELT(pattern, linear_gradient_x1))
    }
}

pub unsafe fn R_GE_linearGradientY1(pattern: SEXP) -> c_double {
    unsafe {
        checkLinearGradient!(pattern);
        *REAL(VECTOR_ELT(pattern, linear_gradient_y1))
    }
}

pub unsafe fn R_GE_linearGradientX2(pattern: SEXP) -> c_double {
    unsafe {
        checkLinearGradient!(pattern);
        *REAL(VECTOR_ELT(pattern, linear_gradient_x2))
    }
}

pub unsafe fn R_GE_linearGradientY2(pattern: SEXP) -> c_double {
    unsafe {
        checkLinearGradient!(pattern);
        *REAL(VECTOR_ELT(pattern, linear_gradient_y2))
    }
}

pub unsafe fn R_GE_linearGradientNumStops(pattern: SEXP) -> c_int {
    unsafe {
        checkLinearGradient!(pattern);
        LENGTH(VECTOR_ELT(pattern, linear_gradient_stops))
    }
}

pub unsafe fn R_GE_linearGradientStop(pattern: SEXP, i: c_int) -> c_double {
    unsafe {
        checkLinearGradient!(pattern);
        *REAL(VECTOR_ELT(pattern, linear_gradient_stops)).add(i as usize)
    }
}

/// rcolor: R color type (unsigned int).
type rcolor = c_uint;

pub unsafe fn R_GE_linearGradientColour(pattern: SEXP, i: c_int) -> rcolor {
    unsafe {
        checkLinearGradient!(pattern);
        RGBpar(
            VECTOR_ELT(pattern, linear_gradient_colours) as *mut c_void,
            i,
        )
    }
}

pub unsafe fn R_GE_linearGradientExtend(pattern: SEXP) -> c_int {
    unsafe {
        checkLinearGradient!(pattern);
        *INTEGER(VECTOR_ELT(pattern, linear_gradient_extend))
    }
}

/* ========================================================================
 * Radial gradients
 * ======================================================================== */

macro_rules! checkRadialGradient {
    ($pattern:expr) => {
        if R_GE_patternType($pattern) != R_GE_radialGradientPattern {
            Rf_error(b"pattern is not a radial gradient\0".as_ptr() as *const c_char);
        }
    };
}

pub unsafe fn R_GE_radialGradientCX1(pattern: SEXP) -> c_double {
    unsafe {
        checkRadialGradient!(pattern);
        *REAL(VECTOR_ELT(pattern, radial_gradient_cx1))
    }
}

pub unsafe fn R_GE_radialGradientCY1(pattern: SEXP) -> c_double {
    unsafe {
        checkRadialGradient!(pattern);
        *REAL(VECTOR_ELT(pattern, radial_gradient_cy1))
    }
}

pub unsafe fn R_GE_radialGradientR1(pattern: SEXP) -> c_double {
    unsafe {
        checkRadialGradient!(pattern);
        *REAL(VECTOR_ELT(pattern, radial_gradient_r1))
    }
}

pub unsafe fn R_GE_radialGradientCX2(pattern: SEXP) -> c_double {
    unsafe {
        checkRadialGradient!(pattern);
        *REAL(VECTOR_ELT(pattern, radial_gradient_cx2))
    }
}

pub unsafe fn R_GE_radialGradientCY2(pattern: SEXP) -> c_double {
    unsafe {
        checkRadialGradient!(pattern);
        *REAL(VECTOR_ELT(pattern, radial_gradient_cy2))
    }
}

pub unsafe fn R_GE_radialGradientR2(pattern: SEXP) -> c_double {
    unsafe {
        checkRadialGradient!(pattern);
        *REAL(VECTOR_ELT(pattern, radial_gradient_r2))
    }
}

pub unsafe fn R_GE_radialGradientNumStops(pattern: SEXP) -> c_int {
    unsafe {
        checkRadialGradient!(pattern);
        LENGTH(VECTOR_ELT(pattern, radial_gradient_stops))
    }
}

pub unsafe fn R_GE_radialGradientStop(pattern: SEXP, i: c_int) -> c_double {
    unsafe {
        checkRadialGradient!(pattern);
        *REAL(VECTOR_ELT(pattern, radial_gradient_stops)).add(i as usize)
    }
}

pub unsafe fn R_GE_radialGradientColour(pattern: SEXP, i: c_int) -> rcolor {
    unsafe {
        checkRadialGradient!(pattern);
        RGBpar(
            VECTOR_ELT(pattern, radial_gradient_colours) as *mut c_void,
            i,
        )
    }
}

pub unsafe fn R_GE_radialGradientExtend(pattern: SEXP) -> c_int {
    unsafe {
        checkRadialGradient!(pattern);
        *INTEGER(VECTOR_ELT(pattern, radial_gradient_extend))
    }
}

/* ========================================================================
 * Tiling patterns
 * ======================================================================== */

macro_rules! checkTilingPattern {
    ($pattern:expr) => {
        if R_GE_patternType($pattern) != R_GE_tilingPattern {
            Rf_error(b"pattern is not a tiling pattern\0".as_ptr() as *const c_char);
        }
    };
}

pub unsafe fn R_GE_tilingPatternFunction(pattern: SEXP) -> SEXP {
    unsafe {
        checkTilingPattern!(pattern);
        VECTOR_ELT(pattern, tiling_pattern_function)
    }
}

pub unsafe fn R_GE_tilingPatternX(pattern: SEXP) -> c_double {
    unsafe {
        checkTilingPattern!(pattern);
        *REAL(VECTOR_ELT(pattern, tiling_pattern_x))
    }
}

pub unsafe fn R_GE_tilingPatternY(pattern: SEXP) -> c_double {
    unsafe {
        checkTilingPattern!(pattern);
        *REAL(VECTOR_ELT(pattern, tiling_pattern_y))
    }
}

pub unsafe fn R_GE_tilingPatternWidth(pattern: SEXP) -> c_double {
    unsafe {
        checkTilingPattern!(pattern);
        *REAL(VECTOR_ELT(pattern, tiling_pattern_width))
    }
}

pub unsafe fn R_GE_tilingPatternHeight(pattern: SEXP) -> c_double {
    unsafe {
        checkTilingPattern!(pattern);
        *REAL(VECTOR_ELT(pattern, tiling_pattern_height))
    }
}

pub unsafe fn R_GE_tilingPatternExtend(pattern: SEXP) -> c_int {
    unsafe {
        checkTilingPattern!(pattern);
        *INTEGER(VECTOR_ELT(pattern, tiling_pattern_extend))
    }
}
