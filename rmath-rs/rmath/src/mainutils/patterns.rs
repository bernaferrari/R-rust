#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

use std::os::raw::{c_double, c_int, c_uint, c_void};

use crate::sexp::accessors::{
    INTEGER, LENGTH, REAL, STRING_ELT, TYPEOF, VECTOR_ELT, translateChar,
};
use crate::sexp::attrib_core::{R_ClassSymbol, getAttrib};
use crate::sexp::constructors::Rf_isNull;
use crate::sexp::ffi::{R_xlen_t, SEXP, SEXPTYPE};
use crate::sexp::globals::R_NilValue;

unsafe fn error(msg: &str) -> ! {
    std::panic::panic_any(crate::sexp::context::RError {
        message: msg.to_string(),
    })
}

pub const R_GE_linearGradientPattern: c_int = 1;
pub const R_GE_radialGradientPattern: c_int = 2;
pub const R_GE_tilingPattern: c_int = 3;

// ---------------------------------------------------------------------------
// R_GE_isPattern
// ---------------------------------------------------------------------------

pub unsafe fn R_GE_isPattern(x: SEXP) -> c_int { unsafe {
    if x.is_null() || x == R_NilValue() {
        return 0;
    }
    let klass = getAttrib(x, R_ClassSymbol());
    if Rf_isNull(klass) != 0 {
        return 0;
    }
    if TYPEOF(klass) == SEXPTYPE::STRSXP.0 {
        let n = LENGTH(klass);
        for i in 0..n {
            let s = STRING_ELT(klass, i as R_xlen_t);
            if !s.is_null() {
                let cs = translateChar(s);
                if !cs.is_null() {
                    let bytes = std::ffi::CStr::from_ptr(cs).to_bytes();
                    if bytes == b"Pattern" {
                        return 1;
                    }
                }
            }
        }
    }
    0
}}

// ---------------------------------------------------------------------------
// R_GE_patternType
// ---------------------------------------------------------------------------

pub unsafe fn R_GE_patternType(pattern: SEXP) -> c_int { unsafe {
    *INTEGER(VECTOR_ELT(pattern, 0))
}}

// ---------------------------------------------------------------------------
// Linear gradient accessors
// ---------------------------------------------------------------------------

const LINEAR_X1: R_xlen_t = 1;
const LINEAR_Y1: R_xlen_t = 2;
const LINEAR_X2: R_xlen_t = 3;
const LINEAR_Y2: R_xlen_t = 4;
const LINEAR_STOPS: R_xlen_t = 5;
const LINEAR_COLOURS: R_xlen_t = 6;
const LINEAR_EXTEND: R_xlen_t = 7;

unsafe fn check_linear_gradient(pattern: SEXP) { unsafe {
    if R_GE_patternType(pattern) != R_GE_linearGradientPattern {
        error("pattern is not a linear gradient");
    }
}}

pub unsafe fn R_GE_linearGradientX1(pattern: SEXP) -> c_double { unsafe {
    check_linear_gradient(pattern);
    *REAL(VECTOR_ELT(pattern, LINEAR_X1))
}}

pub unsafe fn R_GE_linearGradientY1(pattern: SEXP) -> c_double { unsafe {
    check_linear_gradient(pattern);
    *REAL(VECTOR_ELT(pattern, LINEAR_Y1))
}}

pub unsafe fn R_GE_linearGradientX2(pattern: SEXP) -> c_double { unsafe {
    check_linear_gradient(pattern);
    *REAL(VECTOR_ELT(pattern, LINEAR_X2))
}}

pub unsafe fn R_GE_linearGradientY2(pattern: SEXP) -> c_double { unsafe {
    check_linear_gradient(pattern);
    *REAL(VECTOR_ELT(pattern, LINEAR_Y2))
}}

pub unsafe fn R_GE_linearGradientNumStops(pattern: SEXP) -> c_int { unsafe {
    check_linear_gradient(pattern);
    LENGTH(VECTOR_ELT(pattern, LINEAR_STOPS))
}}

pub unsafe fn R_GE_linearGradientStop(pattern: SEXP, i: c_int) -> c_double { unsafe {
    check_linear_gradient(pattern);
    *REAL(VECTOR_ELT(pattern, LINEAR_STOPS)).add(i as usize)
}}

pub unsafe fn R_GE_linearGradientColour(pattern: SEXP, i: c_int) -> c_uint { unsafe {
    check_linear_gradient(pattern);
    let colours = VECTOR_ELT(pattern, LINEAR_COLOURS);
    crate::mainutils::colors::RGBpar(colours as *mut c_void, i)
}}

pub unsafe fn R_GE_linearGradientExtend(pattern: SEXP) -> c_int { unsafe {
    check_linear_gradient(pattern);
    *INTEGER(VECTOR_ELT(pattern, LINEAR_EXTEND))
}}

// ---------------------------------------------------------------------------
// Radial gradient accessors
// ---------------------------------------------------------------------------

const RADIAL_CX1: R_xlen_t = 1;
const RADIAL_CY1: R_xlen_t = 2;
const RADIAL_R1: R_xlen_t = 3;
const RADIAL_CX2: R_xlen_t = 4;
const RADIAL_CY2: R_xlen_t = 5;
const RADIAL_R2: R_xlen_t = 6;
const RADIAL_STOPS: R_xlen_t = 7;
const RADIAL_COLOURS: R_xlen_t = 8;
const RADIAL_EXTEND: R_xlen_t = 9;

unsafe fn check_radial_gradient(pattern: SEXP) { unsafe {
    if R_GE_patternType(pattern) != R_GE_radialGradientPattern {
        error("pattern is not a radial gradient");
    }
}}

pub unsafe fn R_GE_radialGradientCX1(pattern: SEXP) -> c_double { unsafe {
    check_radial_gradient(pattern);
    *REAL(VECTOR_ELT(pattern, RADIAL_CX1))
}}

pub unsafe fn R_GE_radialGradientCY1(pattern: SEXP) -> c_double { unsafe {
    check_radial_gradient(pattern);
    *REAL(VECTOR_ELT(pattern, RADIAL_CY1))
}}

pub unsafe fn R_GE_radialGradientR1(pattern: SEXP) -> c_double { unsafe {
    check_radial_gradient(pattern);
    *REAL(VECTOR_ELT(pattern, RADIAL_R1))
}}

pub unsafe fn R_GE_radialGradientCX2(pattern: SEXP) -> c_double { unsafe {
    check_radial_gradient(pattern);
    *REAL(VECTOR_ELT(pattern, RADIAL_CX2))
}}

pub unsafe fn R_GE_radialGradientCY2(pattern: SEXP) -> c_double { unsafe {
    check_radial_gradient(pattern);
    *REAL(VECTOR_ELT(pattern, RADIAL_CY2))
}}

pub unsafe fn R_GE_radialGradientR2(pattern: SEXP) -> c_double { unsafe {
    check_radial_gradient(pattern);
    *REAL(VECTOR_ELT(pattern, RADIAL_R2))
}}

pub unsafe fn R_GE_radialGradientNumStops(pattern: SEXP) -> c_int { unsafe {
    check_radial_gradient(pattern);
    LENGTH(VECTOR_ELT(pattern, RADIAL_STOPS))
}}

pub unsafe fn R_GE_radialGradientStop(pattern: SEXP, i: c_int) -> c_double { unsafe {
    check_radial_gradient(pattern);
    *REAL(VECTOR_ELT(pattern, RADIAL_STOPS)).add(i as usize)
}}

pub unsafe fn R_GE_radialGradientColour(pattern: SEXP, i: c_int) -> c_uint { unsafe {
    check_radial_gradient(pattern);
    let colours = VECTOR_ELT(pattern, RADIAL_COLOURS);
    crate::mainutils::colors::RGBpar(colours as *mut c_void, i)
}}

pub unsafe fn R_GE_radialGradientExtend(pattern: SEXP) -> c_int { unsafe {
    check_radial_gradient(pattern);
    *INTEGER(VECTOR_ELT(pattern, RADIAL_EXTEND))
}}

// ---------------------------------------------------------------------------
// Tiling pattern accessors
// ---------------------------------------------------------------------------

const TILING_FUNCTION: R_xlen_t = 1;
const TILING_X: R_xlen_t = 2;
const TILING_Y: R_xlen_t = 3;
const TILING_WIDTH: R_xlen_t = 4;
const TILING_HEIGHT: R_xlen_t = 5;
const TILING_EXTEND: R_xlen_t = 6;

unsafe fn check_tiling_pattern(pattern: SEXP) { unsafe {
    if R_GE_patternType(pattern) != R_GE_tilingPattern {
        error("pattern is not a tiling pattern");
    }
}}

pub unsafe fn R_GE_tilingPatternFunction(pattern: SEXP) -> SEXP { unsafe {
    check_tiling_pattern(pattern);
    VECTOR_ELT(pattern, TILING_FUNCTION)
}}

pub unsafe fn R_GE_tilingPatternX(pattern: SEXP) -> c_double { unsafe {
    check_tiling_pattern(pattern);
    *REAL(VECTOR_ELT(pattern, TILING_X))
}}

pub unsafe fn R_GE_tilingPatternY(pattern: SEXP) -> c_double { unsafe {
    check_tiling_pattern(pattern);
    *REAL(VECTOR_ELT(pattern, TILING_Y))
}}

pub unsafe fn R_GE_tilingPatternWidth(pattern: SEXP) -> c_double { unsafe {
    check_tiling_pattern(pattern);
    *REAL(VECTOR_ELT(pattern, TILING_WIDTH))
}}

pub unsafe fn R_GE_tilingPatternHeight(pattern: SEXP) -> c_double { unsafe {
    check_tiling_pattern(pattern);
    *REAL(VECTOR_ELT(pattern, TILING_HEIGHT))
}}

pub unsafe fn R_GE_tilingPatternExtend(pattern: SEXP) -> c_int { unsafe {
    check_tiling_pattern(pattern);
    *INTEGER(VECTOR_ELT(pattern, TILING_EXTEND))
}}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pattern_type_constants() {
        assert_eq!(R_GE_linearGradientPattern, 1);
        assert_eq!(R_GE_radialGradientPattern, 2);
        assert_eq!(R_GE_tilingPattern, 3);
    }

    #[test]
    fn test_is_pattern_nil() {
        unsafe {
            assert_eq!(R_GE_isPattern(std::ptr::null_mut()), 0);
            assert_eq!(R_GE_isPattern(R_NilValue()), 0);
        }
    }
}
