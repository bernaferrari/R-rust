/*
 *  R : A Computer Language for Statistical Data Analysis
 *  Ported from r-source/src/library/grDevices/src/colors.c
 *
 *  Color specification, conversion, and palette management.
 *  This should be regarded as part of the graphics engine.
 */

use std::os::raw::{c_char, c_double, c_int, c_uint};

use crate::attrib_core::{R_DimNamesSymbol, R_DimSymbol, R_NamesSymbol, getAttrib, setAttrib};
use crate::main::coerce::{asInteger, asLogical, asReal, coerceVector};
use crate::main::errors::Rf_error;
use crate::main::relop::NA_STRING;
use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::ffi::{ISNAN, NA_INTEGER, R_FINITE, R_xlen_t, SEXP, SEXPTYPE};
use crate::sexp::globals::R_NilValue;
use crate::sexp::instance::with_required_current_instance;
use crate::sexp::protect::*;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const WHITE_X: c_double = 95.047;
const WHITE_Y: c_double = 100.000;
const WHITE_Z: c_double = 108.883;
const WHITE_u: c_double = 0.1978398;
const WHITE_v: c_double = 0.4683363;
const GAMMA: c_double = 2.4;
const DEG2RAD: c_double = std::f64::consts::PI / 180.0;
const MAX_PALETTE_SIZE: usize = 1024;
const NA_LOGICAL: c_int = NA_INTEGER;

// ---------------------------------------------------------------------------
// Color type and macros
// ---------------------------------------------------------------------------

type rcolor = u32;

const fn R_RED(col: rcolor) -> c_int {
    (col & 0xFF) as c_int
}
const fn R_GREEN(col: rcolor) -> c_int {
    ((col >> 8) & 0xFF) as c_int
}
const fn R_BLUE(col: rcolor) -> c_int {
    ((col >> 16) & 0xFF) as c_int
}
const fn R_ALPHA(col: rcolor) -> c_int {
    ((col >> 24) & 0xFF) as c_int
}

const fn R_RGB(r: c_int, g: c_int, b: c_int) -> rcolor {
    (r as u32 & 0xFF) | ((g as u32 & 0xFF) << 8) | ((b as u32 & 0xFF) << 16) | (0xFF_u32 << 24)
}

const fn R_RGBA(r: c_int, g: c_int, b: c_int, a: c_int) -> rcolor {
    (r as u32 & 0xFF)
        | ((g as u32 & 0xFF) << 8)
        | ((b as u32 & 0xFF) << 16)
        | ((a as u32 & 0xFF) << 24)
}

const fn R_OPAQUE(col: rcolor) -> c_int {
    ((col & 0xFF000000) == 0xFF000000) as c_int
}
const fn R_TRANSPARENT(col: rcolor) -> c_int {
    ((col & 0xFF000000) == 0) as c_int
}
const R_TRANWHITE: rcolor = 0x00FFFFFF;

pub(crate) struct GraphicsColorState {
    palette_size: c_int,
    palette: [rcolor; MAX_PALETTE_SIZE],
    palette0: [rcolor; MAX_PALETTE_SIZE],
    col_buf: [u8; 10],
}

impl Default for GraphicsColorState {
    fn default() -> Self {
        let mut arr = [0u32; MAX_PALETTE_SIZE];
        let mut i = 0;
        while i < PALETTE_INIT.len() {
            arr[i] = PALETTE_INIT[i];
            i += 1;
        }
        GraphicsColorState {
            palette_size: 8,
            palette: arr,
            palette0: [0; MAX_PALETTE_SIZE],
            col_buf: [0; 10],
        }
    }
}

#[inline]
fn with_color_state<F, R>(f: F) -> R
where
    F: FnOnce(&mut GraphicsColorState) -> R,
{
    with_required_current_instance(|instance| f(unsafe { &mut (*instance).graphics_color_state }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sexp::session::RSession;

    #[test]
    fn palette_state_is_session_local_on_same_thread() {
        let left = RSession::new();
        let right = RSession::new();

        left.with_protected(|| {
            with_color_state(|state| {
                state.palette_size = 2;
                state.palette[0] = R_RGB(1, 2, 3);
                state.palette[1] = R_RGB(4, 5, 6);
            });
        });

        right.with_protected(|| {
            with_color_state(|state| {
                assert_eq!(state.palette_size, 8);
                assert_eq!(state.palette[0], DEFAULT_PALETTE[0]);
            });
        });

        left.with_protected(|| {
            with_color_state(|state| {
                assert_eq!(state.palette_size, 2);
                assert_eq!(state.palette[0], R_RGB(1, 2, 3));
                assert_eq!(state.palette[1], R_RGB(4, 5, 6));
            });
        });
    }
}

#[cfg(test)]
#[path = "colors/owned_decode_tests.rs"]
mod owned_decode_tests;

#[cfg(test)]
#[path = "colors/numeric_tests.rs"]
mod numeric_tests;

mod builtins;
mod converters;
mod names_table;
mod numeric;
mod palette;

pub use builtins::*;
pub use converters::*;
pub use names_table::*;
pub use palette::*;
