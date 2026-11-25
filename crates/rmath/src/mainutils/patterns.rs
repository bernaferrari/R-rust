#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

use std::os::raw::{c_double, c_int, c_uint, c_void};

use crate::sexp::accessors::{LENGTH, TYPEOF, VECTOR_ELT};
use crate::sexp::attrib_core::{R_ClassSymbol, getAttrib};
use crate::sexp::constructors::Rf_isNull;
use crate::sexp::ffi::{R_xlen_t, SEXP, SEXPTYPE};
use crate::sexp::globals::R_NilValue;
use crate::sexp::object::Sexp;

fn error(msg: &str) -> ! {
    std::panic::panic_any(crate::sexp::context::RError {
        message: msg.to_string(),
    })
}

fn checked_sexp(x: SEXP) -> Sexp<'static> {
    crate::mainutils::essentials::owner_sexp(crate::mainutils::essentials::active_owner(), x)
}

fn checked_error(error: crate::sexp::object::SexpError) -> ! {
    crate::sexp::context::r_error(error.to_string())
}

fn pattern_elt(pattern: SEXP, index: R_xlen_t) -> Sexp<'static> {
    checked_sexp(pattern)
        .try_vector_elt(index)
        .unwrap_or_else(|error| checked_error(error))
}

fn pattern_real(pattern: SEXP, index: R_xlen_t) -> c_double {
    pattern_elt(pattern, index)
        .try_real_elt(0)
        .unwrap_or_else(|error| checked_error(error))
}

fn pattern_int(pattern: SEXP, index: R_xlen_t) -> c_int {
    pattern_elt(pattern, index)
        .try_integer_elt(0)
        .unwrap_or_else(|error| checked_error(error))
}

pub const R_GE_linearGradientPattern: c_int = 1;
pub const R_GE_radialGradientPattern: c_int = 2;
pub const R_GE_tilingPattern: c_int = 3;

// ---------------------------------------------------------------------------
// R_GE_isPattern
// ---------------------------------------------------------------------------

pub fn R_GE_isPattern(x: SEXP) -> c_int {
    if x.is_null() || x == R_NilValue() {
        return 0;
    }
    let klass = getAttrib(x, R_ClassSymbol());
    if Rf_isNull(klass) != 0 {
        return 0;
    }
    if TYPEOF(klass) == SEXPTYPE::STRSXP {
        let owned = checked_sexp(klass);
        let n = LENGTH(klass);
        for i in 0..n {
            if matches!(
                owned.try_string_value_elt(i as R_xlen_t),
                Ok(Some(ref text)) if text == "Pattern"
            ) {
                return 1;
            }
        }
    }
    0
}

// ---------------------------------------------------------------------------
// R_GE_patternType
// ---------------------------------------------------------------------------

pub fn R_GE_patternType(pattern: SEXP) -> c_int {
    pattern_int(pattern, 0)
}

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

fn check_linear_gradient(pattern: SEXP) {
    if R_GE_patternType(pattern) != R_GE_linearGradientPattern {
        error("pattern is not a linear gradient");
    }
}









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

fn check_radial_gradient(pattern: SEXP) {
    if R_GE_patternType(pattern) != R_GE_radialGradientPattern {
        error("pattern is not a radial gradient");
    }
}











// ---------------------------------------------------------------------------
// Tiling pattern accessors
// ---------------------------------------------------------------------------

const TILING_FUNCTION: R_xlen_t = 1;
const TILING_X: R_xlen_t = 2;
const TILING_Y: R_xlen_t = 3;
const TILING_WIDTH: R_xlen_t = 4;
const TILING_HEIGHT: R_xlen_t = 5;
const TILING_EXTEND: R_xlen_t = 6;

fn check_tiling_pattern(pattern: SEXP) {
    if R_GE_patternType(pattern) != R_GE_tilingPattern {
        error("pattern is not a tiling pattern");
    }
}







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
        assert_eq!(R_GE_isPattern(std::ptr::null_mut()), 0);
        assert_eq!(R_GE_isPattern(R_NilValue()), 0);
    }
}
