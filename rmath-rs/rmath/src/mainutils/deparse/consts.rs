#![allow(unused_variables)]
#![allow(unused_assignments)]
#![allow(non_snake_case, non_upper_case_globals, dead_code)]

use super::*;

// ---------------------------------------------------------------------------
// Constants from deparse.c
// ---------------------------------------------------------------------------

/// Buffer size for deparsing strings.
pub const BUFSIZE: c_int = 512;

/// Minimum allowed cutoff value.
pub const MIN_CUTOFF: c_int = 20;

/// Default cutoff value for line width.
pub const DEFAULT_CUTOFF: c_int = 60;

/// Maximum allowed cutoff value (must be < BUFSIZE).
pub const MAX_CUTOFF: c_int = BUFSIZE - 12;

// ---------------------------------------------------------------------------
// Deparse option flags
// ---------------------------------------------------------------------------

/// Keep NAs as NA_real_, NA_integer_, NA_character_, NA_complex_ (not just NA).
pub const KEEPNA: c_int = 1;

/// Keep integer constants with trailing L (e.g. 1L).
pub const KEEPINTEGER: c_int = 2;

/// Show attributes in deparse output.
pub const SHOWATTRIBUTES: c_int = 4;

/// Use source references if available.
pub const USESOURCE: c_int = 8;

/// Delay promises (show as <promise: ...>).
pub const DELAYPROMISES: c_int = 16;

/// S-compatible deparse (use old-style quoting, etc.).
pub const S_COMPAT: c_int = 32;

/// Quote expressions.
pub const QUOTEEXPRESSIONS: c_int = 64;

/// Use hex notation for floating-point numbers.
pub const HEXNUMERIC: c_int = 128;

/// Use 17 significant digits for real numbers.
pub const DIGITS17: c_int = 256;

/// Show names nicely (as tag = value).
pub const NICE_NAMES: c_int = 512;

/// Warn if deparsed result may not be source()-able.
pub const WARNINCOMPLETE: c_int = 1024;

/// Simple deparse options (no quoting, no attributes, no delay).
pub const SIMPLEDEPARSE: c_int = 0;

/// Default deparse options. Mirrors upstream DEFAULTDEPARSE
/// (KEEPINTEGER | KEEPNA | NICE_NAMES) plus SHOWATTRIBUTES which the port
/// has historically included — integer constants deparse with the trailing
/// "L" in call attribution, matching stock R.
pub const DEFAULTDEPARSE: c_int = KEEPNA | KEEPINTEGER | NICE_NAMES | SHOWATTRIBUTES;

/// R's user-facing default `deparse()` control set:
/// keepNA, keepInteger, niceNames, and showAttributes.
pub const DEFAULT_USER_DEPARSE: c_int = KEEPNA | KEEPINTEGER | NICE_NAMES | SHOWATTRIBUTES;

/// Simple opts mask: keep KEEPINTEGER | USESOURCE | KEEPNA | S_COMPAT | WARNINCOMPLETE.
pub const SIMPLE_OPTS: c_int = !(QUOTEEXPRESSIONS | SHOWATTRIBUTES | DELAYPROMISES);

/// Show attributes or nice names.
pub const SHOW_ATTR_OR_NMS: c_int = SHOWATTRIBUTES | NICE_NAMES;

// ---------------------------------------------------------------------------
// Precedence constants (local aliases for names.rs values)
// ---------------------------------------------------------------------------

/// Precedence level for comparison operators (<, >, ==, etc.).
pub const PREC_COMPARE: c_int = N_PREC_COMPARE;
/// Precedence level for sum operators (+, -).
pub const PREC_SUM: c_int = N_PREC_SUM;
/// Precedence level for sign operators (unary +, -).
pub const PREC_SIGN: c_int = N_PREC_SIGN;
/// Precedence level for %op% operators.
pub const PREC_PERCENT: c_int = N_PREC_PERCENT;
/// Precedence level for subset operators ([, [[).
pub const PREC_SUBSET: c_int = N_PREC_SUBSET;

// ---------------------------------------------------------------------------
// PPinfo kinds (local aliases for names.rs values)
// ---------------------------------------------------------------------------

pub const PP_BINARY: c_int = N_PP_BINARY;
pub const PP_BINARY2: c_int = N_PP_BINARY2;
pub const PP_UNARY: c_int = N_PP_UNARY;
pub const PP_SUBSET: c_int = N_PP_SUBSET;
pub const PP_SUBASS: c_int = N_PP_SUBASS;
pub const PP_DOLLAR: c_int = N_PP_DOLLAR;
pub const PP_ASSIGN: c_int = N_PP_ASSIGN;
pub const PP_ASSIGN2: c_int = N_PP_ASSIGN2;
pub const PP_IF: c_int = N_PP_IF;
pub const PP_WHILE: c_int = N_PP_WHILE;
pub const PP_FOR: c_int = N_PP_FOR;
pub const PP_REPEAT: c_int = N_PP_REPEAT;
pub const PP_FUNCALL: c_int = N_PP_FUNCALL;
pub const PP_RETURN: c_int = N_PP_RETURN;
pub const PP_PAREN: c_int = N_PP_PAREN;
pub const PP_CURLY: c_int = N_PP_CURLY;
pub const PP_FOREIGN: c_int = N_PP_FOREIGN;
pub const PP_FUNCTION: c_int = N_PP_FUNCTION;
pub const PP_BREAK: c_int = N_PP_BREAK;
pub const PP_NEXT: c_int = N_PP_NEXT;

// ---------------------------------------------------------------------------
// Attribute type enum for deparsing
// ---------------------------------------------------------------------------

/// Unknown attribute state.
pub const ATTR_UNKNOWN: c_int = -1;
/// Simple object (no attributes shown).
pub const ATTR_SIMPLE: c_int = 0;
/// Object with OK names (names written as n1 = v1).
pub const ATTR_OK_NAMES: c_int = 1;
/// Object with structure attributes (non-names only).
pub const ATTR_STRUC_ATTR: c_int = 2;
/// Object with structure attributes including names.
pub const ATTR_STRUC_NMS_A: c_int = 3;

// ---------------------------------------------------------------------------
// NB / NB2 constants for complex encoding
// ---------------------------------------------------------------------------
pub const NB: usize = 1000;
pub const NB2: usize = 2 * NB + 25;

pub struct DeparseRuntimeState {
    pub quote_buf: [u8; 1024],
    pub int_buf: [c_char; 32],
    pub logical_buf: [c_char; 8],
    pub real_buf: [c_char; 64],
    pub string_buf: [u8; 2048],
    pub raw_buf: [c_char; 8],
    pub hex_buf: [c_char; 64],
    pub dig_buf: [c_char; 64],
    pub cplx_buf: [c_char; NB2],
    pub hex_cplx: [c_char; 128],
    pub dig_cplx: [c_char; 128],
    pub cplx_buf2: [c_char; 256],
    pub browse_lines: c_int,
}

impl Default for DeparseRuntimeState {
    fn default() -> Self {
        DeparseRuntimeState {
            quote_buf: [0; 1024],
            int_buf: [0; 32],
            logical_buf: [0; 8],
            real_buf: [0; 64],
            string_buf: [0; 2048],
            raw_buf: [0; 8],
            hex_buf: [0; 64],
            dig_buf: [0; 64],
            cplx_buf: [0; NB2],
            hex_cplx: [0; 128],
            dig_cplx: [0; 128],
            cplx_buf2: [0; 256],
            browse_lines: 0,
        }
    }
}

pub fn with_deparse_runtime<F, R>(f: F) -> R
where
    F: FnOnce(&mut DeparseRuntimeState) -> R,
{
    crate::sexp::instance::with_required_current_instance(|inst| f(&mut inst.eval_state.deparse))
}

pub fn get_browse_lines() -> c_int {
    with_deparse_runtime(|state| state.browse_lines)
}

pub fn set_browse_lines(value: c_int) {
    with_deparse_runtime(|state| state.browse_lines = value);
}
